use postgres_with_quic::{
    build_root_cert_store,
    monitor::write_metrics,
    pool::{ClientWrapper, QuicManager, TcpManager},
};
use std::{
    net::SocketAddr,
    ops::Deref,
    sync::{Arc, Mutex, atomic::AtomicI64},
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() {
    run().await;
}

async fn run() {
    use std::env::var;
    let tls_cert_path = var("TLS_CERT_PATH").unwrap();

    let query = var("TEST_QUERY").unwrap();
    let start_rate: u64 = var("START_RATE_QPS").unwrap().parse().unwrap();
    let end_rate: u64 = var("END_RATE_QPS").unwrap().parse().unwrap();
    let ramp_duration_secs = var("RAMP_DURATION_SECS").unwrap().parse().unwrap();

    let username = var("DB_USER").unwrap();
    let password = var("DB_PASS").unwrap();
    let hostname = var("DB_HOSTNAME").unwrap();
    let dbname = var("DB_NAME").unwrap();

    let metrics_file_path = var("METRICS_FILE_PATH").unwrap();
    let max_conns: usize = var("MAX_POOL_SIZE").unwrap().parse().unwrap();
    let metrics_interval: u64 = var("METRICS_INTERVAL_MS").unwrap().parse().unwrap();

    let mut config = tokio_postgres::Config::new();
    config.user(username);
    config.password(password);
    config.dbname(dbname);
    config.host(hostname);

    let mut using_quic = false;

    let db_addr: SocketAddr = if let Ok(hostaddr) = var("TCP_DB_ADDR") {
        config.ssl_mode(tokio_postgres::config::SslMode::Require);
        hostaddr.parse().unwrap()
    } else if let Ok(hostaddr) = var("QUIC_DB_ADDR") {
        config.ssl_mode(tokio_postgres::config::SslMode::Disable);
        using_quic = true;
        hostaddr.parse().unwrap()
    } else {
        panic!("No TCP_DB_ADDR or QUIC_DB_ADDR env var set");
    };

    config.hostaddr(db_addr.ip());
    config.port(db_addr.port());

    let active_queries = Arc::new(AtomicI64::new(0));
    let failed_queries = Arc::new(AtomicI64::new(0));
    let query_times = Arc::new(std::sync::Mutex::new(Vec::<u64>::with_capacity(1024)));

    let cert_store = Arc::new(build_root_cert_store(&tls_cert_path));

    let mut client_cfg = rustls::ClientConfig::builder()
        .with_root_certificates(cert_store)
        .with_no_client_auth();

    client_cfg.alpn_protocols.push(b"h3".to_vec());

    let (tx, rx) = tokio::sync::mpsc::channel(32); // they are consumed immediately anyway

    let f1 = tokio::spawn(send_queries(
        tx,
        start_rate,
        end_rate,
        Duration::from_secs(ramp_duration_secs),
    ));

    let f2 = tokio::spawn(write_metrics(
        metrics_file_path,
        Duration::from_millis(metrics_interval),
        active_queries.clone(),
        failed_queries.clone(),
        query_times.clone(),
    ));

    if using_quic {
        println!("Using QUIC");
        let always_open_a_new_connection = match var("QUIC_ALWAYS_OPEN_A_CONNECTION") {
            Ok(s) => s == "true",
            Err(_) => false,
        };

        let streams_per_connection = match var("QUIC_STREAMS_PER_CONN") {
            Ok(val) => val.parse::<u64>().unwrap_or(100),
            Err(_) => 100,
        };

        let manager = QuicManager::new(
            Arc::new(client_cfg),
            config,
            always_open_a_new_connection,
            streams_per_connection,
        )
        .unwrap();

        let f3 = pool_test(
            manager,
            max_conns,
            rx,
            query,
            active_queries.clone(),
            failed_queries.clone(),
            query_times.clone(),
        );
        let result = futures::future::join3(f1, f2, f3).await;
        println!("{:?}", result);
    } else {
        println!("Using TCP");
        let manager = TcpManager::new(config, client_cfg);

        let f3 = pool_test(
            manager,
            max_conns,
            rx,
            query,
            active_queries.clone(),
            failed_queries.clone(),
            query_times.clone(),
        );
        let result = futures::future::join3(f1, f2, f3).await;
        println!("{:?}", result);
    }
}

async fn pool_test<M>(
    manager: M,
    max_conns: usize,
    mut incoming_queries: tokio::sync::mpsc::Receiver<()>,
    query: String,
    active_queries: Arc<AtomicI64>,
    failed_queries: Arc<AtomicI64>,
    query_times: Arc<Mutex<Vec<u64>>>,
) where
    M: deadpool::managed::Manager<Type = ClientWrapper, Error = anyhow::Error> + Sync + 'static,
{
    let pool = deadpool::managed::Pool::<M>::builder(manager)
        .max_size(max_conns)
        .build()
        .unwrap();

    let query: Arc<str> = query.into(); // having this normal cloned, should show us pending/halted queries memory hog, but I wanted to see only them memory of connections overhead

    while let Some(_) = incoming_queries.recv().await {
        let active_queries = active_queries.clone();
        let failed_queries = failed_queries.clone();
        let pool = pool.clone();
        let query_times = query_times.clone();
        let q = query.clone();
        active_queries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        tokio::spawn(async move {
            let start = Instant::now();
            if let Err(err) = pool.get().await.unwrap().query(q.deref(), &[]).await {
                eprintln!("{}", err);
                failed_queries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            let time_took = start.elapsed();
            active_queries.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

            {
                query_times
                    .lock()
                    .unwrap()
                    .push(time_took.as_micros() as u64);
            }
        });
    }
}

async fn send_queries(
    tx: tokio::sync::mpsc::Sender<()>,
    start_rate: u64,
    end_rate: u64,
    ramp_duration: Duration,
) {
    let start = Instant::now();

    let mut number_of_queries = 0;

    let mut interval = tokio::time::interval(Duration::from_millis(500));

    loop {
        let _ = interval.tick().await;

        let elapsed = start.elapsed().as_secs_f64();

        if elapsed > ramp_duration.as_secs_f64() {
            break; // stop at end of ramp
        }
        let progress = elapsed / ramp_duration.as_secs_f64();
        let current_rate = ((start_rate + (end_rate - start_rate)) as f64 * progress) as u64;

        for _ in 0..current_rate {
            if let Err(err) = tx.send(()).await {
                eprintln!("{}", err);
                break;
            }
        }
        number_of_queries += current_rate;
    }

    println!(
        "Total number of queries sent {} in {} ms",
        number_of_queries,
        start.elapsed().as_millis()
    );
}

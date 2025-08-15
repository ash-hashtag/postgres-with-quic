use postgres_with_quic::{
    build_root_cert_store, make_quic_connection2,
    pool::{ClientWrapper, QuicManager, TcpManager},
};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex, atomic::AtomicI64},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() {
    run().await;
}

async fn run() {
    use std::env::var;
    let tls_cert_path = var("TLS_CERT_PATH").unwrap();

    let query = var("TEST_QUERY").unwrap();
    let start_rate: f64 = var("START_RATE_QPS").unwrap().parse().unwrap();
    let end_rate: f64 = var("END_RATE_QPS").unwrap().parse().unwrap();
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
    let query_times = Arc::new(std::sync::Mutex::new(Vec::<u64>::with_capacity(1024)));

    let cert_store = Arc::new(build_root_cert_store(&tls_cert_path));

    let mut client_cfg = rustls::ClientConfig::builder()
        .with_root_certificates(cert_store)
        .with_no_client_auth();

    client_cfg.alpn_protocols.push("h3".as_bytes().to_vec());

    let (tx, rx) = tokio::sync::mpsc::channel(16); // they are consumed immediately anyway

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());

    let f1 = send_queries(
        tx,
        query,
        start_rate,
        end_rate,
        Duration::from_secs(ramp_duration_secs),
    );

    let f2 = collect_metrics(
        active_queries.clone(),
        query_times.clone(),
        metrics_file_path,
        Duration::from_millis(metrics_interval),
        shutdown_rx,
    );

    if using_quic {
        println!("Using QUIC");
        let always_open_a_new_connection = match var("QUIC_ALWAYS_OPEN_A_CONNECTION") {
            Ok(s) => s == "true",
            Err(_) => false,
        };
        let manager =
            QuicManager::new(Arc::new(client_cfg), config, always_open_a_new_connection).unwrap();

        let f3 = pool_test(
            manager,
            max_conns,
            rx,
            active_queries.clone(),
            query_times.clone(),
            shutdown_tx,
        );
        futures::future::join3(f1, f2, f3).await;
    } else {
        println!("Using TCP");
        let manager = TcpManager::new(config, client_cfg);

        let f3 = pool_test(
            manager,
            max_conns,
            rx,
            active_queries.clone(),
            query_times.clone(),
            shutdown_tx,
        );
        futures::future::join3(f1, f2, f3).await;
    }
}

async fn collect_metrics(
    active_queries: Arc<AtomicI64>,
    query_times: Arc<Mutex<Vec<u64>>>,
    metrics_file_path: String,
    interval: Duration,
    mut shutdown_rx: tokio::sync::watch::Receiver<()>,
) {
    let mut query_metrics_file = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(metrics_file_path)
        .await
        .unwrap();

    query_metrics_file
        .write_all(b"timestamp_ms,active_number_of_queries,number_of_queries_processed,average_query_time_us\n")
        .await
        .unwrap();

    let mut tick_interval = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            _ = tick_interval.tick() => {

            }
        }

        tick_interval.tick().await;
        let now = SystemTime::now();
        let now_ms = now.duration_since(UNIX_EPOCH).unwrap().as_millis();

        let (sum, len) = {
            let mut query_times = query_times.lock().unwrap();
            let len = query_times.len();

            if len == 0 {
                break;
            }

            let mut sum = 0u64;
            for q in query_times.iter() {
                sum += q;
            }

            query_times.clear();

            (sum, len)
        };

        let average = sum as f64 / len as f64;

        let query_metric = format!(
            "{},{},{},{}\n",
            now_ms,
            active_queries.load(std::sync::atomic::Ordering::Relaxed),
            len,
            average,
        );

        print!("{}", query_metric);

        query_metrics_file
            .write(query_metric.as_bytes())
            .await
            .unwrap();
    }
}

async fn pool_test<M>(
    manager: M,
    max_conns: usize,
    mut incoming_queries: tokio::sync::mpsc::Receiver<String>,
    active_queries: Arc<AtomicI64>,
    query_times: Arc<Mutex<Vec<u64>>>,
    shutdown_tx: tokio::sync::watch::Sender<()>,
) where
    M: deadpool::managed::Manager<Type = ClientWrapper, Error = anyhow::Error> + Sync + 'static,
{
    let pool = deadpool::managed::Pool::<M>::builder(manager)
        .max_size(max_conns)
        .build()
        .unwrap();

    while let Some(q) = incoming_queries.recv().await {
        let active_queries = active_queries.clone();
        let pool = pool.clone();
        let query_times = query_times.clone();
        tokio::spawn(async move {
            active_queries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let start = SystemTime::now();
            if let Err(err) = pool.get().await.unwrap().query(q.as_str(), &[]).await {
                eprintln!("{}", err);
            }
            let time_took = start.elapsed().unwrap();
            active_queries.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

            {
                query_times
                    .lock()
                    .unwrap()
                    .push(time_took.as_micros() as u64);
            }
        });
    }

    let _ = shutdown_tx.send(()).unwrap();
}

async fn send_queries(
    tx: tokio::sync::mpsc::Sender<String>,
    query: String,
    start_rate: f64,
    end_rate: f64,
    ramp_duration: Duration,
) {
    let start = Instant::now();

    let mut number_of_queries = 0;

    loop {
        let elapsed = start.elapsed().as_secs_f64();

        if elapsed > ramp_duration.as_secs_f64() {
            break; // stop at end of ramp
        }
        let progress = elapsed / ramp_duration.as_secs_f64();
        let current_rate = start_rate + (end_rate - start_rate) * progress;

        // adjust interval dynamically

        let wait_period = Duration::from_secs_f64(1.0 / current_rate);

        let _ = tokio::time::sleep(wait_period).await;

        if let Err(err) = tx.send(query.clone()).await {
            eprintln!("{}", err);
            break;
        }

        number_of_queries += 1;
    }

    println!(
        "Total number of queries sent {} in {} ms",
        number_of_queries,
        start.elapsed().as_millis()
    );
}

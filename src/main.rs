use futures::task::UnsafeFutureObj;
use postgres_with_quic::{pool::TcpManager, *};
use std::{
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{io::AsyncWriteExt, sync::Mutex, time::interval};
use tokio_postgres::NoTls;

#[tokio::main]
async fn main() {
    let tls_cert_path = std::env::var("TLS_CERT_PATH").unwrap();
    let query = "SELECT * FROM pgbench_accounts LIMIT 10";
    let mut config = tokio_postgres::Config::new();
    config.user("admin_user");
    config.password("admin_pass");
    config.dbname("simple_db");

    let mut quic_config = config.clone();
    let db_addr: std::net::SocketAddr = "127.0.0.1:6432".parse().unwrap();
    let quic_db_addr: std::net::SocketAddr = "127.0.0.1:5436".parse().unwrap();

    // config.hostaddr(db_addr.ip());
    config.port(db_addr.port());
    config.host("localhost");
    config.ssl_mode(tokio_postgres::config::SslMode::Require);

    let root_store = Arc::new(build_root_cert_store(tls_cert_path.as_str()));
    let connect = tls_connect(root_store);
    let (client, conn) = config.connect(connect.clone()).await.unwrap();
    let client = Arc::new(client);
    let handle = tokio::spawn(conn);
    let mut handles = Vec::with_capacity(100);
    let instant = Instant::now();
    for _ in 0..100 {
        let client = client.clone();
        let h = tokio::spawn(async move {
            let _ = client.query(query, &[]).await.unwrap();
        });
        handles.push(h);
    }
    futures::future::join_all(handles).await;
    println!(
        "Took {} ms from default connection",
        instant.elapsed().as_millis()
    );
    handle.abort();

    quic_config.hostaddr(quic_db_addr.ip());
    quic_config.port(quic_db_addr.port());
    quic_config.ssl_mode(tokio_postgres::config::SslMode::Disable);

    let connect = s2n_quic::client::Connect::new(quic_db_addr).with_server_name("localhost");
    let quic_client = make_quicc_client(tls_cert_path.as_str());

    let conn = quic_client.connect(connect.clone()).await.unwrap();
    let conn = Arc::new(Mutex::new(conn));

    let mut handles = Vec::with_capacity(100);
    for _ in 0..100 {
        let quic_config = quic_config.clone();
        let conn = conn.clone();
        let h = tokio::spawn(async move {
            let stream = { conn.lock().await.open_bidirectional_stream().await.unwrap() };

            let (client, conn) = quic_config.connect_raw(stream, NoTls).await.unwrap();
            let handle = tokio::spawn(conn);
            let _ = client.query(query, &[]).await.unwrap();
            let _ = client;
            handle.abort();
        });
        handles.push(h);
    }

    futures::future::join_all(handles).await;
    println!(
        "Took {} ms from quic connections",
        instant.elapsed().as_millis()
    );
    // println!(
    //     "Average Quic Connection Establishment: {} ms",
    //     sum.load(std::sync::atomic::Ordering::Relaxed) as f64 / (n * 1000) as f64
    // );
    // let rows = make_connection_and_query(config, root_cert_store.clone(), query).await;
    // println!("Rows returned {}", rows.len());

    // let rows = make_quic_connection_and_query(quic_config, root_cert_store.clone(), query).await;
    // println!("Rows returned {}", rows.len());
    // let conn = make_quicc_conn(client_cfg, db_addr).await.unwrap();
    // let (send, recv) = conn.open_bi().await.unwrap();

    // let combined = Combined {
    //     reader: recv,
    //     writer: send,
    // };

    // println!("Connected");
    // let (client, connection) = config.connect_raw(combined, NoTls).await.unwrap();
    // let connection_end = tokio::spawn(connection);

    // println!("Querying...");

    // let rows = client
    //     .query("SELECT * FROM pgbench_accounts LIMIT 10", &[])
    //     .await
    //     .unwrap();

    // println!("Got {} ", rows.len());
    // for row in rows {
    //     let aid: i32 = row.get(0);
    //     let bid: i32 = row.get(1);
    //     let abalance: i32 = row.get(2);
    //     let filler: String = row.get(3);
    //     println!("{aid}, {bid}, {abalance}, {filler}");
    // }

    // let rows = client
    //     .simple_query("SELECT * FROM pgbench_accounts LIMIT 10")
    //     .await
    //     .unwrap();
    // println!("Got {} ", rows.len());

    // for row in rows {
    //     if let SimpleQueryMessage::Row(row) = row {
    //         let aid: i32 = row.get(0).unwrap().parse().unwrap();
    //         let bid: i32 = row.get(1).unwrap().parse().unwrap();
    //         let abalance: i32 = row.get(2).unwrap().parse().unwrap();
    //         let filler = row.get(3).unwrap().to_string();

    //         println!("{aid}, {bid}, {abalance}, {filler}");
    //     }
    // }

    // let _ = connection_end.await.unwrap();
}

async fn quic_pool_test(pg_config: tokio_postgres::Config, tls: rustls::ClientConfig) {}

async fn tcp_pool_test(
    pg_config: tokio_postgres::Config,
    tls: rustls::ClientConfig,
    max_conns: usize,

    mut incoming_queries: tokio::sync::mpsc::Receiver<String>,
) {
    let manager = TcpManager::new(pg_config, tls);

    let pool = deadpool::managed::Pool::<TcpManager>::builder(manager)
        .max_size(max_conns)
        .build()
        .unwrap();

    let active_queries = Arc::new(AtomicU64::new(0));
    let active_queries_clone = active_queries.clone();
    let query_times = Arc::new(std::sync::Mutex::new(Vec::<u64>::with_capacity(1024)));
    let query_times_clone = query_times.clone();
    tokio::spawn(async move {
        let mut query_metrics_file = tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open("/tmp/query_metrics.csv")
            .await
            .unwrap();

        query_metrics_file
            .write_all(b"timestamp_ms,number_of_queries,average_query_time_ms\n")
            .await
            .unwrap();

        let mut last_time = SystemTime::now();
        let interval = Duration::from_secs(1);
        loop {
            let time_left = SystemTime::now().duration_since(last_time).unwrap();
            if let Some(time_to_sleep) = interval.checked_sub(time_left) {
                tokio::time::sleep(time_to_sleep).await;
            }
            let now = SystemTime::now();
            let now_ms = now.duration_since(UNIX_EPOCH).unwrap().as_millis();
            last_time = now;

            let average = {
                let mut query_times = query_times_clone.lock().unwrap();
                let len = query_times.len();

                if len == 0 {
                    break;
                }

                let mut sum = 0u64;
                for q in query_times.iter() {
                    sum += q;
                }

                query_times.clear();

                (sum as f64) / (len as f64)
            };

            let query_metric = format!(
                "{},{},{}",
                now_ms,
                active_queries_clone.load(std::sync::atomic::Ordering::Relaxed),
                average,
            );

            query_metrics_file
                .write(query_metric.as_bytes())
                .await
                .unwrap();
        }
    });

    while let Some(q) = incoming_queries.recv().await {
        let active_queries = active_queries.clone();
        let pool = pool.clone();
        let query_times = query_times.clone();
        tokio::spawn(async move {
            active_queries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let start = SystemTime::now();
            let _ = pool.get().await.unwrap().query(q.as_str(), &[]).await;
            let time_took = start.elapsed().unwrap();
            active_queries.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            query_times
                .lock()
                .unwrap()
                .push(time_took.as_millis() as u64);
        });
    }
}

async fn send_queries(tx: tokio::sync::mpsc::Sender<String>) {
    let query = String::from("SELECT * FROM pgbench_accounts LIMIT 10");
    let start_rate = 10.0;
    let end_rate = 1000.0;
    let ramp_duration = Duration::from_secs(30); // total ramp-up time

    let start = Instant::now();

    loop {
        let elapsed = start.elapsed().as_secs_f64();

        if elapsed > ramp_duration.as_secs_f64() {
            break; // stop at end of ramp
        }
        let progress = elapsed / ramp_duration.as_secs_f64();
        let current_rate = start_rate + (end_rate - start_rate) * progress;

        // adjust interval dynamically
        let mut tick = interval(Duration::from_secs_f64(1.0 / current_rate));

        tick.tick().await; // wait until next tick
        if let Err(err) = tx.send(query.clone()).await {
            eprintln!("{}", err);
            break;
        }
    }
}

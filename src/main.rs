use postgres_with_quic::*;
use std::{sync::Arc, time::Instant};
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
    let instant = Instant::now();
    let (client, conn) = config.connect(tls_connect(root_store)).await.unwrap();
    let _ = tokio::spawn(conn);
    let _ = client.query(query, &[]).await.unwrap();

    println!(
        "default connection took: {} ms",
        instant.elapsed().as_millis()
    );

    quic_config.hostaddr(quic_db_addr.ip());
    quic_config.port(quic_db_addr.port());
    quic_config.ssl_mode(tokio_postgres::config::SslMode::Disable);

    let connect = s2n_quic::client::Connect::new(quic_db_addr).with_server_name("localhost");
    let quic_client = make_quicc_client(tls_cert_path.as_str());
    let instant = Instant::now();
    let mut conn = quic_client.connect(connect).await.unwrap();
    let stream = conn.open_bidirectional_stream().await.unwrap();

    let (client, conn) = quic_config.connect_raw(stream, NoTls).await.unwrap();
    let _ = tokio::spawn(conn);

    let _ = client.query(query, &[]).await.unwrap();

    println!("quic connection took: {} ms", instant.elapsed().as_millis());
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

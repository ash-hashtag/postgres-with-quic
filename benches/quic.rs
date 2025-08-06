use std::{path::Path, sync::Arc};

use criterion::{Criterion, criterion_group, criterion_main};

use postgres_with_quic::*;
use tokio_postgres::NoTls;

fn quic_connection_time(c: &mut Criterion) {
    let group_name = "quic_connection_time";
    let mut group = c.benchmark_group(group_name);
    group.sample_size(20);

    let tls_cert_path = std::env::var("TLS_CERT_PATH").unwrap();
    let mut config = tokio_postgres::Config::new();

    let quic_db_addr: std::net::SocketAddr = "127.0.0.1:5436".parse().unwrap();

    config.user("admin_user");
    config.password("admin_pass");
    config.dbname("simple_db");
    config.hostaddr(quic_db_addr.ip());
    config.port(quic_db_addr.port());
    config.ssl_mode(tokio_postgres::config::SslMode::Disable);

    let query = "SELECT * FROM pgbench_accounts LIMIT 100";

    let connect = s2n_quic::client::Connect::new(quic_db_addr).with_server_name("localhost");

    for size in [1, 10, 30] {
        group.bench_function(format!("{}_{}", group_name, size), |b| {
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter(|| async {
                    let client_builder = s2n_quic::Client::builder()
                        .with_tls(Path::new(tls_cert_path.as_str()))
                        .unwrap()
                        .with_io("0.0.0.0:0")
                        .unwrap();
                    let client = client_builder.start().unwrap();

                    let mut connection = client.connect(connect.clone()).await.unwrap();
                    let stream = connection.open_bidirectional_stream().await.unwrap();

                    let (client, conn) = config.connect_raw(stream, NoTls).await.unwrap();
                    let conn = tokio::spawn(conn);

                    for _ in 0..size {
                        let _ = client.query(query, &[]).await;
                    }

                    conn.abort();
                });
        });
    }
    group.finish();
}

fn default_connection_time(c: &mut Criterion) {
    let group_name = "default_connection_time";
    let mut group = c.benchmark_group(group_name);
    group.sample_size(20);

    let tls_cert_path = std::env::var("TLS_CERT_PATH").unwrap();
    let mut config = tokio_postgres::Config::new();

    let db_addr: std::net::SocketAddr = "127.0.0.1:6432".parse().unwrap();

    config.user("admin_user");
    config.password("admin_pass");
    config.dbname("simple_db");
    config.port(db_addr.port());
    config.host("localhost");
    config.ssl_mode(tokio_postgres::config::SslMode::Require);

    let query = "SELECT * FROM pgbench_accounts LIMIT 100";

    for size in [1, 10, 30] {
        group.bench_function(format!("{}_{}", group_name, size), |b| {
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter(|| async {
                    let root_cert_store = Arc::new(build_root_cert_store(&tls_cert_path));
                    let (client, conn) = make_connection(config.clone(), root_cert_store).await;
                    for _ in 0..size {
                        let _ = client.query(query, &[]).await;
                    }

                    conn.abort();
                });
        });
    }
    group.finish();
}

// criterion_group!(benches, default_connection_time, quic_connection_time);
criterion_group!(benches, quic_connection_time, default_connection_time);
criterion_main!(benches);

pub mod pool;

use rustls::{
    RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use std::{net::SocketAddr, path::Path, sync::Arc};
use tokio_postgres_rustls::MakeRustlsConnect;

use tokio::task::JoinHandle;
use tokio_postgres::NoTls;

pub fn make_quicc_client(tls_cert_path: &str) -> s2n_quic::Client {
    let client = s2n_quic::Client::builder()
        .with_tls(Path::new(tls_cert_path))
        .unwrap()
        .with_io("0.0.0.0:0")
        .unwrap()
        .start()
        .unwrap();

    client
}

pub fn tls_connect(root_store: Arc<RootCertStore>) -> MakeRustlsConnect {
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    return MakeRustlsConnect::new(tls_config);
}

pub async fn make_connection(
    config: tokio_postgres::Config,
    root_store: Arc<RootCertStore>,
) -> (
    tokio_postgres::Client,
    JoinHandle<Result<(), tokio_postgres::Error>>,
) {
    let (client, conn) = config.connect(tls_connect(root_store)).await.unwrap();
    let conn_end = tokio::spawn(conn);
    return (client, conn_end);
}

pub async fn make_quic_connection2(
    config: tokio_postgres::Config,
    tls_cert_path: &str,
) -> (
    tokio_postgres::Client,
    JoinHandle<Result<(), tokio_postgres::Error>>,
) {
    let client = make_quicc_client(tls_cert_path);

    let addr = SocketAddr::new(config.get_hostaddrs()[0], config.get_ports()[0]);

    let connect = s2n_quic::client::Connect::new(addr).with_server_name("localhost");

    let mut connection = client.connect(connect).await.unwrap();
    let stream = connection.open_bidirectional_stream().await.unwrap();

    let (client, conn) = config.connect_raw(stream, NoTls).await.unwrap();
    let conn_end = tokio::spawn(conn);

    return (client, conn_end);
}

pub fn build_root_cert_store(cert_path: &str) -> RootCertStore {
    let mut store = RootCertStore::empty();
    store
        .add(CertificateDer::from_pem_file(cert_path).unwrap())
        .unwrap();

    return store;
}

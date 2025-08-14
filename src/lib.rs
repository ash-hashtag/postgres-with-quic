mod pool;

use deadpool_postgres::{ManagerConfig, StatementCaches};
use pin_project::pin_project;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio_postgres_rustls::MakeRustlsConnect;

use quinn::{
    ClientConfig,
    crypto::rustls::QuicClientConfig,
    rustls::{
        RootCertStore,
        pki_types::{CertificateDer, pem::PemObject},
    },
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    task::JoinHandle,
};
use tokio_postgres::NoTls;

use tokio_postgres::GenericClient;
pub async fn make_quicc_conn(
    client_cfg: ClientConfig,
    connect_to: SocketAddr,
) -> Result<quinn::Connection, quinn::ConnectionError> {
    let endpoint =
        quinn::Endpoint::client(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))
            .unwrap();

    let conn = endpoint
        .connect_with(client_cfg, connect_to, "localhost")
        .unwrap();
    return conn.await;
}

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
pub async fn make_quic_connection(
    config: tokio_postgres::Config,
    root_store: Arc<RootCertStore>,
) -> (
    tokio_postgres::Client,
    JoinHandle<Result<(), tokio_postgres::Error>>,
) {
    let mut tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store.clone())
        .with_no_client_auth();
    tls_config.enable_early_data = true;

    let cfg = QuicClientConfig::try_from(tls_config).unwrap();

    let addr = SocketAddr::new(config.get_hostaddrs()[0], config.get_ports()[0]);

    let endpoint =
        quinn::Endpoint::client(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))
            .unwrap();

    let client_config = quinn::ClientConfig::new(Arc::new(cfg));
    let conn = endpoint
        .connect_with(client_config, addr, "localhost")
        .unwrap();
    let conn = conn.await.unwrap();

    let (send, recv) = conn.open_bi().await.unwrap();

    let combined = Combined {
        reader: recv,
        writer: send,
    };
    let (client, conn) = config.connect_raw(combined, NoTls).await.unwrap();
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

pub fn build_root_cert_store(tls_cert_path: &str) -> RootCertStore {
    let mut root_cert_store = RootCertStore::empty();
    let certificate = CertificateDer::from_pem_file(tls_cert_path).unwrap();
    root_cert_store.add(certificate).unwrap();
    return root_cert_store;
}

#[pin_project]
struct Combined<R, W> {
    #[pin]
    reader: R,
    #[pin]
    writer: W,
}

impl<R, W> AsyncRead for Combined<R, W>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.project().reader).poll_read(cx, buf)
    }
}

impl<R, W> AsyncWrite for Combined<R, W>
where
    W: AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.project().writer).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.project().writer).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.project().writer).poll_shutdown(cx)
    }
}

pub struct QuicConnector {
    connection: Arc<tokio::sync::Mutex<s2n_quic::Connection>>,
}

impl deadpool_postgres::Connect for QuicConnector {
    fn connect(
        &self,
        pg_config: &tokio_postgres::Config,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<
                        (tokio_postgres::Client, JoinHandle<()>),
                        tokio_postgres::Error,
                    >,
                > + Send,
        >,
    > {
        let conn = self.connection.clone();
        let pg_config = pg_config.clone();
        Box::pin(async move {
            let mut conn = conn.lock().await;
            let stream = match conn.open_bidirectional_stream().await {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("{}", err);
                    let (client, conn) = pg_config.connect(NoTls).await?;

                    let handle = tokio::spawn(async move {
                        if let Err(err) = conn.await {
                            eprintln!("Connection closed {}", err);
                        }
                    });
                    return Ok((client, handle));
                }
            };

            let (client, conn) = pg_config.connect_raw(stream, NoTls).await?;

            let handle = tokio::spawn(async move {
                if let Err(err) = conn.await {
                    eprintln!("Connection closed {}", err);
                }
            });
            return Ok((client, handle));
        })
    }
}

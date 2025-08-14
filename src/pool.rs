use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use s2n_quic::provider::limits::Limits;
use tokio::sync::Mutex;
use tokio_postgres::NoTls;
use tokio_postgres_rustls::MakeRustlsConnect;

type PgClient = tokio_postgres::Client;

pub struct QuicManager {
    connection: tokio::sync::Mutex<Option<s2n_quic::Connection>>,
    pg_config: tokio_postgres::Config,
    connect: s2n_quic::client::Connect,
    client: s2n_quic::Client,
    always_open_a_new_connection: bool,
}

impl QuicManager {
    pub fn new(
        tls: Arc<rustls::ClientConfig>,
        config: tokio_postgres::Config,
        always_open_a_new_connection: bool,
    ) -> anyhow::Result<Self> {
        if config.get_hosts().is_empty()
            || config.get_hostaddrs().is_empty()
            || config.get_ports().is_empty()
        {
            return Err(anyhow::anyhow!("Hosts/HostAddrs/Ports is empty"));
        }

        let provider = s2n_quic::provider::tls::rustls::Client::from(tls.clone());

        let client = s2n_quic::Client::builder()
            .with_tls(provider)?
            .with_io("0.0.0.0:0")?
            .with_limits(
                Limits::new()
                    .with_max_open_local_bidirectional_streams(1000)?
                    .with_max_open_remote_bidirectional_streams(1000)?,
            )?
            .start()?;

        let addr = SocketAddr::new(config.get_hostaddrs()[0], config.get_ports()[0]);
        let hostname = match &config.get_hosts()[0] {
            tokio_postgres::config::Host::Tcp(name) => name,
            _ => return Err(anyhow::anyhow!("No host name found")),
        };
        let connect = s2n_quic::client::Connect::new(addr).with_server_name(hostname.as_str());
        Ok(Self {
            connection: Mutex::new(None),
            pg_config: config,
            connect,
            client,
            always_open_a_new_connection,
        })
    }

    async fn open_stream(&self) -> anyhow::Result<s2n_quic::stream::BidirectionalStream> {
        if self.always_open_a_new_connection {
            let mut connection = self.client.connect(self.connect.clone()).await?;
            let s = connection.open_bidirectional_stream().await?;
            return Ok(s);
        }

        let mut conn = self.connection.lock().await;

        if let Some(connection) = &mut *conn {
            if let Ok(s) = connection.open_bidirectional_stream().await {
                return Ok(s);
            }
        }
        {
            let mut connection = self.client.connect(self.connect.clone()).await?;

            let s = connection.open_bidirectional_stream().await?;
            *conn = Some(connection);

            return Ok(s);
        }
    }
}

impl deadpool::managed::Manager for QuicManager {
    type Type = ClientWrapper;

    type Error = anyhow::Error;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        let stream = self.open_stream().await?;

        let (client, connection) = self.pg_config.connect_raw(stream, NoTls).await?;

        let conn_task = tokio::spawn(async move {
            if let Err(err) = connection.await {
                eprintln!("{}", err);
            }
        });

        Ok(ClientWrapper::new(client, conn_task))
    }

    async fn recycle(
        &self,
        obj: &mut Self::Type,
        _: &deadpool_postgres::Metrics,
    ) -> deadpool::managed::RecycleResult<Self::Error> {
        if obj.is_closed() {
            return Err(deadpool::managed::RecycleError::Message(
                "Connection closed".into(),
            ));
        }

        Ok(())
    }
}

pub struct TcpManager {
    pg_config: tokio_postgres::Config,
    tls_connect: MakeRustlsConnect,
}

impl TcpManager {
    pub fn new(pg_config: tokio_postgres::Config, tls: rustls::ClientConfig) -> Self {
        Self {
            tls_connect: MakeRustlsConnect::new(tls),
            pg_config,
        }
    }
}

impl deadpool::managed::Manager for TcpManager {
    type Type = ClientWrapper;

    type Error = anyhow::Error;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        let (client, connection) = self.pg_config.connect(self.tls_connect.clone()).await?;

        let conn_task = tokio::spawn(async move {
            let _ = connection.await;
        });

        Ok(ClientWrapper::new(client, conn_task))
    }

    async fn recycle(
        &self,
        obj: &mut Self::Type,
        _: &deadpool_postgres::Metrics,
    ) -> deadpool::managed::RecycleResult<Self::Error> {
        if obj.is_closed() {
            return Err(deadpool::managed::RecycleError::Message(
                "Connection closed".into(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ClientWrapper {
    /// Original [`PgClient`].
    client: tokio_postgres::Client,

    /// A handle to the connection task that should be aborted when the client
    /// wrapper is dropped.
    conn_task: tokio::task::JoinHandle<()>,
}

impl ClientWrapper {
    /// Create a new [`ClientWrapper`] instance using the given
    /// [`tokio_postgres::Client`] and handle to the connection task.
    #[must_use]
    pub fn new(client: tokio_postgres::Client, conn_task: tokio::task::JoinHandle<()>) -> Self {
        Self { client, conn_task }
    }
}

impl Deref for ClientWrapper {
    type Target = PgClient;

    fn deref(&self) -> &PgClient {
        &self.client
    }
}

impl DerefMut for ClientWrapper {
    fn deref_mut(&mut self) -> &mut PgClient {
        &mut self.client
    }
}

impl Drop for ClientWrapper {
    fn drop(&mut self) {
        self.conn_task.abort()
    }
}

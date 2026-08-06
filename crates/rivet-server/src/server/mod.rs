//! `net.minecraft.server` — the server surface for the skeleton slice.
//!
//! `ServerConfig` is the immutable config snapshot (OWNERSHIP "config
//! snapshots" exception — `Arc`-shared), `Server` owns the configuration and
//! delegates to `ServerConnectionListener` for the TCP accept loop.

pub mod network;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use network::server_connection_listener::ServerConnectionListener;

/// `ServerConnectionListener` port config: `MinecraftServer`'s bind surface plus
/// the slice-local knobs. Fields are immutable after startup.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// `ServerConnectionListener.startTcpServerListener(InetAddress, int)`.
    pub bind_host: IpAddr,
    /// Server port (Paper default 25565).
    pub port: u16,
    /// Slice-local safety cap on concurrent TCP connections. Paper has no
    /// pre-login TCP cap; it rejects at login via max-players (sub-issue #101).
    /// This bound deterministically closes sockets beyond it so a misbehaving
    /// client cannot pile up accepted connections on the tokio side.
    pub max_connections: usize,
    /// Per-connection read timeout (Paper `new ReadTimeoutHandler(30)` seconds).
    pub read_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind_host: IpAddr::from([0, 0, 0, 0]),
            port: 25565,
            max_connections: 100,
            read_timeout: Duration::from_secs(30),
        }
    }
}

/// The server: owns the immutable config and runs the connection listener.
/// Mirrors `MinecraftServer` owning a `ServerConnectionListener`.
pub struct Server {
    config: Arc<ServerConfig>,
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Server {
            config: Arc::new(config),
        }
    }

    /// The immutable config snapshot.
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Bind the TCP listener without accepting yet (tests use this to learn the
    /// ephemeral port from `TcpListener::local_addr`).
    pub async fn bind(&self) -> std::io::Result<tokio::net::TcpListener> {
        ServerConnectionListener::new(self.config.clone())
            .bind()
            .await
    }

    /// Serve on an already-bound listener: accept loop + per-connection tasks.
    pub async fn serve(self, listener: tokio::net::TcpListener) -> std::io::Result<()> {
        ServerConnectionListener::new(self.config.clone())
            .serve(listener)
            .await
    }

    /// Bind and serve (the binary entry path).
    pub async fn run(self) -> std::io::Result<()> {
        let listener = self.bind().await?;
        self.serve(listener).await
    }
}

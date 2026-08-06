use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::net::tcp::OwnedReadHalf;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use super::connection::Connection;
use super::connection_id::ConnectionId;
use super::packet_listener::{DisconnectReason, PacketListener};
use super::server_handshake_packet_listener::ServerHandshakePacketListener;
use crate::server::ServerConfig;

/// `net.minecraft.server.network.ServerConnectionListener` — the accept loop and
/// per-connection task spawner.
///
/// Port config (Paper 26.2):
///   - `startTcpServerListener(SocketAddress)`: binds and defers accepting until
///     `acceptConnections()` (CraftBukkit `AUTO_READ=false`). This slice binds
///     and immediately accepts, but keeps the split (`Server::bind` → `serve`)
///     so tests can learn the ephemeral port.
///   - `TCP_NODELAY` set on every accepted socket.
///   - `ReadTimeoutHandler(30)` — 30-second read timeout per connection.
///   - initial listener `ServerHandshakePacketListenerImpl`.
///
/// The `max_connections` cap is a slice-local safety bound: Paper has no
/// pre-login TCP cap (it rejects over capacity at login), so this closes sockets
/// beyond the configured limit rather than accepting unbounded connections on
/// the tokio side.
pub struct ServerConnectionListener {
    config: Arc<ServerConfig>,
    next_id: AtomicUsize,
    /// Active connection count (for the slice-local cap). The OWNERSHIP
    /// "connection registry" exception: this is the connection registry, shared
    /// between the accept task and per-connection tasks.
    active: Arc<AtomicUsize>,
}

impl ServerConnectionListener {
    pub fn new(config: Arc<ServerConfig>) -> Self {
        ServerConnectionListener {
            config,
            next_id: AtomicUsize::new(0),
            active: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Bind the TCP listener on the configured address.
    pub async fn bind(&self) -> std::io::Result<TcpListener> {
        let addr = std::net::SocketAddr::new(self.config.bind_host, self.config.port);
        let listener = TcpListener::bind(addr).await?;
        info!(%addr, "server listening");
        Ok(listener)
    }

    /// Accept loop: for each socket, check the connection cap, then spawn a
    /// per-connection task. Runs until the listener errors or closes.
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        let mut tasks = JoinSet::new();
        loop {
            // Reap finished connection tasks so the JoinSet does not grow
            // unboundedly over the lifetime of the server (Paper removes closed
            // channels from its registry in the tick loop; `try_join_next` is the
            // tokio analog of draining completed connections each accept).
            while tasks.try_join_next().is_some() {}

            let (socket, remote) = match listener.accept().await {
                Ok(ok) => ok,
                Err(e) => {
                    warn!(error = %e, "accept error");
                    continue;
                }
            };
            if self.accept_connection() {
                // `ChannelOption.TCP_NODELAY` — disable Nagle on the accepted socket.
                let _ = socket.set_nodelay(true);
                let id = ConnectionId(self.next_id.fetch_add(1, Ordering::Relaxed) as u64);
                let active = Arc::clone(&self.active);
                let config = Arc::clone(&self.config);
                tasks.spawn(async move {
                    run_connection(id, socket, remote, config, active).await;
                });
            } else {
                // Over the cap: close deterministically without a connection task.
                debug!(%remote, "closing socket over max_connections");
                drop(socket);
            }
        }
    }

    /// Reserve a slot under `max_connections`. Returns false when at capacity.
    fn accept_connection(&self) -> bool {
        let active = self.active.load(Ordering::SeqCst);
        if active >= self.config.max_connections {
            return false;
        }
        let _ = self.active.fetch_add(1, Ordering::SeqCst);
        true
    }
}

/// Per-connection task: read with a 30s timeout, feed `Connection::process_inbound`
/// (framing + listener dispatch), flush outbound, and close deterministically on
/// the first malformed/unsupported frame or on EOF/timeout. This is the tokio
/// side of OWNERSHIP §Network — no game state is touched.
async fn run_connection(
    id: ConnectionId,
    socket: tokio::net::TcpStream,
    remote: std::net::SocketAddr,
    config: Arc<ServerConfig>,
    active: Arc<AtomicUsize>,
) {
    info!(%id, %remote, "connection established");

    let (read, write) = socket.into_split();
    let mut conn = Connection::new(id, remote, Arc::clone(&config), write);
    let mut listener: Box<dyn PacketListener> = Box::new(ServerHandshakePacketListener);

    let reason = conn_loop(&mut conn, &mut listener, &config, read).await;
    match reason {
        DisconnectReason::EndOfStream => debug!(%id, %remote, "EOF"),
        DisconnectReason::Timeout => warn!(%id, %remote, "read timeout"),
        DisconnectReason::Malformed(msg) => warn!(%id, %remote, "malformed: {msg}"),
        DisconnectReason::Unsupported(msg) => info!(%id, %remote, "unsupported: {msg}"),
    }
    conn.close().await;
    active.fetch_sub(1, Ordering::SeqCst);
}

/// The per-connection read/dispatch loop. Reads a chunk under the configured
/// timeout, decodes frames, dispatches to the listener, flushes outbound. Returns
/// the disconnect reason that ends the loop (EOF, timeout, or a frame the
/// listener rejected); the connection is closed by the caller.
async fn conn_loop(
    conn: &mut Connection,
    listener: &mut Box<dyn PacketListener>,
    config: &ServerConfig,
    mut read: OwnedReadHalf,
) -> DisconnectReason {
    let mut chunk = [0u8; 4096];
    loop {
        let n = match timeout(config.read_timeout, read.read(&mut chunk)).await {
            Err(_) => return DisconnectReason::Timeout,
            Ok(Err(_)) => return DisconnectReason::EndOfStream,
            Ok(Ok(0)) => return DisconnectReason::EndOfStream,
            Ok(Ok(n)) => n,
        };
        if let Err(reason) = conn.process_inbound(&chunk[..n], listener) {
            return reason;
        }
        // Flush after each inbound batch — the analog of netty flushing the
        // pending outbound queue when the read loop runs.
        if conn.flush_out().await.is_err() {
            return DisconnectReason::EndOfStream;
        }
    }
}

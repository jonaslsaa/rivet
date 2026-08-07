use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use super::connection::{Connection, InboundOutcome};
use super::connection_id::ConnectionId;
use super::packet_listener::{DisconnectReason, PacketListener};
use super::server_handshake_packet_listener::ServerHandshakePacketListener;
use crate::server::ServerConfig;
use crate::server::tick::channels::{OutboundEvent, ServerboundFrame};
use crate::server::tick::endpoint::{NetworkEndpoint, RegisterResult};
use crate::server::tick::shutdown::Shutdown;

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
    /// The tick boundary: registration + shutdown. This is the tokio side of
    /// OWNERSHIP §Network — only the channel ends and the endpoint (no game
    /// state) live here.
    endpoint: Arc<NetworkEndpoint>,
    shutdown: Arc<Shutdown>,
}

impl ServerConnectionListener {
    pub fn new(
        config: Arc<ServerConfig>,
        endpoint: Arc<NetworkEndpoint>,
        shutdown: Arc<Shutdown>,
    ) -> Self {
        ServerConnectionListener {
            config,
            next_id: AtomicUsize::new(0),
            active: Arc::new(AtomicUsize::new(0)),
            endpoint,
            shutdown,
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
    /// per-connection task. Stops when the listener closes or shutdown is
    /// requested (the tokio side of orderly shutdown; the tick thread joins
    /// after).
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        let mut tasks = JoinSet::new();
        loop {
            if self.shutdown.is_requested() {
                break;
            }
            // Reap finished connection tasks so the JoinSet does not grow
            // unboundedly over the lifetime of the server (Paper removes closed
            // channels from its registry in the tick loop; `try_join_next` is the
            // tokio analog of draining completed connections each accept).
            while tasks.try_join_next().is_some() {}

            let accept = tokio::select! {
                accepted = listener.accept() => accepted,
                _ = self.shutdown.wait_async() => {
                    // Shutdown requested while waiting for a socket: stop accepting.
                    break;
                }
            };
            let (socket, remote) = match accept {
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
                let endpoint = Arc::clone(&self.endpoint);
                let shutdown = Arc::clone(&self.shutdown);
                tasks.spawn(async move {
                    run_connection(id, socket, remote, config, active, endpoint, shutdown).await;
                });
            } else {
                // Over the cap: close deterministically without a connection task.
                debug!(%remote, "closing socket over max_connections");
                drop(socket);
            }
        }
        // Shutdown: stop accepting, then wait for every per-connection task to
        // finish its graceful close before returning. Each task wakes on the
        // shutdown signal and drains+flushes its outbound channel, so the drain
        // is bounded and queued frames reach the client (Paper's `stopServer`
        // disconnecting each player before the listener shuts down).
        while tasks.join_next().await.is_some() {}
        Ok(())
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
/// (framing + listener dispatch), drain the tick→network outbound channel into
/// the socket, and close deterministically on the first malformed/unsupported
/// frame, on EOF/timeout, on shutdown, or when the tick side drops the channel
/// (outbound overflow policy / server stop). This is the tokio side of OWNERSHIP
/// §Network — no game state is touched.
#[allow(clippy::too_many_arguments)]
async fn run_connection(
    id: ConnectionId,
    socket: tokio::net::TcpStream,
    remote: std::net::SocketAddr,
    config: Arc<ServerConfig>,
    active: Arc<AtomicUsize>,
    endpoint: Arc<NetworkEndpoint>,
    shutdown: Arc<Shutdown>,
) {
    info!(%id, %remote, "connection established");

    let (read, write) = socket.into_split();
    let mut conn = Connection::new(
        id,
        remote,
        Arc::clone(&config),
        Arc::clone(&shutdown),
        write,
    );
    let mut listener: Box<dyn PacketListener> = Box::new(ServerHandshakePacketListener);

    // Own the tick-side channel ends handed over on registration. `in_tx` is
    // kept for the play-state packet path (epic #10 / #96) and lives for the
    // connection's lifetime, so the tick side sees an open inbound channel until
    // this task exits. `out_rx` is drained below.
    let (in_tx, in_rx) = mpsc::channel::<ServerboundFrame>(config.inbound_channel_capacity);
    let (out_tx, out_rx) = mpsc::channel::<OutboundEvent>(config.outbound_channel_capacity);
    let registered = endpoint
        .register_connection(id, remote, in_rx, out_tx)
        .await;
    let mut in_tx = Some(in_tx);
    if registered == RegisterResult::ServerShuttingDown {
        // The tick thread exited while we waited (it drains the lifecycle
        // channel each tick, so a full channel here is overload backpressure,
        // not a stop — only a closed channel means the tick thread is gone).
        debug!(%id, %remote, "connection not registered (server shutting down)");
        conn.close().await;
        active.fetch_sub(1, Ordering::SeqCst);
        return;
    }

    let reason = conn_loop(
        &mut conn,
        &mut listener,
        &config,
        read,
        out_rx,
        in_tx
            .as_ref()
            .expect("inbound sender kept for the play handoff"),
        shutdown,
    )
    .await;
    // `Connection.handleDisconnection` fires `onDisconnect(DisconnectionDetails)`
    // on the current listener when the channel closes. The loop ending IS the
    // channel close, so the last-installed listener gets its disconnect hook
    // before the socket is torn down (all current listeners no-op, but the hook
    // is the faithful call site).
    listener.on_disconnect();
    match &reason {
        DisconnectReason::EndOfStream => debug!(%id, %remote, "EOF"),
        DisconnectReason::Timeout => warn!(%id, %remote, "read timeout"),
        DisconnectReason::Malformed(msg) => warn!(%id, %remote, "malformed: {msg}"),
        DisconnectReason::Unsupported(msg) => info!(%id, %remote, "unsupported: {msg}"),
        DisconnectReason::ServerShutdown => debug!(%id, %remote, "server shutdown"),
        DisconnectReason::RequestHandled => info!(%id, %remote, "status request handled"),
        DisconnectReason::Overflow => warn!(%id, %remote, "outbound overflow, disconnected"),
        DisconnectReason::InboundOverflow(msg) => {
            warn!(%id, %remote, "inbound overflow, disconnected: {msg}")
        }
    }
    // Best-effort: tell the tick side the connection is gone. If the lifecycle
    // channel is full, the registry self-heals when it next drains the closed
    // inbound channel.
    endpoint.connection_closed(id, reason.clone());
    // Dropping `in_tx` closes the inbound channel; the tick registry prunes the
    // connection on its next drain.
    in_tx.take();
    conn.close().await;
    active.fetch_sub(1, Ordering::SeqCst);
}

/// Handle one outbound event, flushing queued frames before a disconnect. Returns
/// `Some(reason)` when the connection should close (a `Disconnect` event, or the
/// channel being gone), `None` to keep looping.
///
/// `bound` selects the flush flavor: `None` (normal operation) uses the
/// backpressure-then-shutdown-abort flush; `Some(timeout)` (the shutdown drain)
/// bounds the write with a wall-clock timeout, since shutdown is already
/// requested there.
async fn handle_outbound(
    conn: &mut Connection,
    event: OutboundEvent,
    bound: Option<Duration>,
) -> Option<DisconnectReason> {
    match event {
        OutboundEvent::Packet { frame } => {
            conn.queue_raw_frame(frame);
            None
        }
        OutboundEvent::Disconnect { reason } => {
            // Paper's `send(disconnect, thenRun(disconnect))` ordering: flush
            // any frames already queued before this disconnect, then close.
            let flushed = match bound {
                Some(timeout) => conn.flush_out_bounded(timeout).await,
                None => conn.flush_out().await,
            };
            if flushed.is_err() {
                return Some(DisconnectReason::EndOfStream);
            }
            Some(reason)
        }
    }
}

/// Non-blocking drain of whatever the tick thread has queued, flushing frames as
/// they are consumed. Returns the disconnect reason when a `Disconnect` event is
/// encountered (its own flush happens inside [`handle_outbound`]); `None` when
/// the channel is simply empty or already gone (the caller reports the terminal
/// reason). Normal-path writes backpressure (no wall-clock timeout).
async fn drain_outbound(
    conn: &mut Connection,
    out_rx: &mut mpsc::Receiver<OutboundEvent>,
) -> Option<DisconnectReason> {
    // Empty and Disconnected both mean "nothing more to drain" here: a
    // Disconnect event (returned by handle_outbound) already carried its own
    // close reason, and any frames are flushed below.
    while let Ok(event) = out_rx.try_recv() {
        if let Some(reason) = handle_outbound(conn, event, None).await {
            return Some(reason);
        }
    }
    if conn.flush_out().await.is_err() {
        return Some(DisconnectReason::EndOfStream);
    }
    None
}

/// Blocking drain of the outbound channel to completion, used once shutdown is
/// requested. The tick thread's final pass queues its last frames — the
/// `ServerShutdown` Disconnect it sends to every live connection, plus anything
/// the final `run_tick` tickables queued — and then drops every `out_tx`, so
/// waiting on `recv()` consumes that pass in order and `None` means the stop is
/// complete. Frames are flushed as they are consumed, so they reach the client
/// before the socket closes (Paper's `stopServer` disconnecting each player
/// before the listener shuts down). The drain timeout bounds both the wait and
/// the writes: shutdown is already requested here, so the shutdown-abort flush
/// would never flush, and a non-reading peer would otherwise stall the drain.
async fn drain_to_close(
    conn: &mut Connection,
    out_rx: &mut mpsc::Receiver<OutboundEvent>,
    drain_timeout: Duration,
) -> DisconnectReason {
    loop {
        match tokio::time::timeout(drain_timeout, out_rx.recv()).await {
            Ok(Some(event)) => {
                if let Some(reason) = handle_outbound(conn, event, Some(drain_timeout)).await {
                    return reason;
                }
                // A packet: keep draining (the Disconnect comes after it).
                continue;
            }
            Ok(None) | Err(_) => {
                // `None`: the tick side dropped every `out_tx` — the stop is
                // complete. Timeout: the tick side never finished; close anyway.
                if conn.flush_out_bounded(drain_timeout).await.is_err() {
                    return DisconnectReason::EndOfStream;
                }
                return DisconnectReason::ServerShutdown;
            }
        }
    }
}

/// The play-state forwarding exit: `forward_play` failed. A
/// [`DisconnectReason::ServerShutdown`] error means the tick-side inbound
/// channel is gone — the tick thread stopped, or pruned this connection — so
/// the queued-outbound flush contract applies exactly like the
/// `shutdown.wait_async()` branch: drain the outbound channel to completion
/// (the tick's final frames and its in-band `ServerShutdown` Disconnect) before
/// closing, so queued frames reach the client. Any other error (a corrupted
/// frame, the inbound drain budget) is a terminal protocol close; the caller's
/// `conn.close()` flushes `out_buf`.
///
/// When the drain observes the channel close without a server stop (an
/// outbound-overload prune), the low reason is [`DisconnectReason::Overflow`],
/// not `ServerShutdown` — mirroring the `out_rx.recv() == None` branch.
async fn forward_play_failed(
    conn: &mut Connection,
    out_rx: &mut mpsc::Receiver<OutboundEvent>,
    reason: DisconnectReason,
    shutdown: &Shutdown,
    drain_timeout: Duration,
) -> DisconnectReason {
    if reason != DisconnectReason::ServerShutdown {
        return reason;
    }
    let drained = drain_to_close(conn, out_rx, drain_timeout).await;
    if drained == DisconnectReason::ServerShutdown && !shutdown.is_requested() {
        return DisconnectReason::Overflow;
    }
    drained
}

/// The per-connection read/dispatch loop. Reads a chunk under the configured
/// timeout, decodes frames, dispatches to the listener, drains the tick→network
/// outbound channel into the socket, and flushes. On the configuration→play
/// handoff ([`InboundOutcome::Play`]) the loop stops dispatching to a listener
/// and forwards every decoded frame to the tick thread over `in_tx` — the
/// OWNERSHIP §Network play boundary. Returns the disconnect reason that ends
/// the loop; the connection is closed by the caller.
async fn conn_loop(
    conn: &mut Connection,
    listener: &mut Box<dyn PacketListener>,
    config: &ServerConfig,
    mut read: OwnedReadHalf,
    mut out_rx: mpsc::Receiver<OutboundEvent>,
    in_tx: &mpsc::Sender<ServerboundFrame>,
    shutdown: Arc<Shutdown>,
) -> DisconnectReason {
    // Whether the connection has crossed the configuration→play boundary and
    // frames are now forwarded to the tick thread instead of a listener.
    let mut in_play = false;
    let mut chunk = [0u8; 4096];
    loop {
        // Non-blocking drain of whatever the tick thread has queued.
        if let Some(reason) = drain_outbound(conn, &mut out_rx).await {
            return reason;
        }

        // Block on the socket read, but wake on shutdown and on any event the
        // tick thread enqueues (so a single-connection outbound event is handled
        // promptly, not after the next inbound packet or the 30s timeout).
        let n = tokio::select! {
            n = read.read(&mut chunk) => match n {
                Err(_) => return DisconnectReason::EndOfStream,
                Ok(0) => return DisconnectReason::EndOfStream,
                Ok(n) => n,
            },
            _ = shutdown.wait_async() => {
                // Shutdown requested: stop reading, then drain the outbound
                // channel to completion so the tick thread's final pass (its
                // last frames and the `ServerShutdown` Disconnect) flushes
                // before the socket closes. Bounded by the read timeout.
                return drain_to_close(conn, &mut out_rx, config.read_timeout).await;
            }
            // A `None` recv means the channel closed (tick side dropped it) —
            // returned immediately, never a busy loop. The tick side drops the
            // channel when `ConnectionRegistry::send` prunes a full channel
            // (overload policy), or — on the server-stop path — only after the
            // in-band `ServerShutdown` Disconnect was already delivered (and thus
            // consumed above). A silent close during a running server is
            // overload; the same close with shutdown requested is a stop.
            event = out_rx.recv() => {
                let event = match event {
                    Some(event) => event,
                    None => {
                        if conn.flush_out().await.is_err() {
                            return DisconnectReason::EndOfStream;
                        }
                        if shutdown.is_requested() {
                            return DisconnectReason::ServerShutdown;
                        }
                        return DisconnectReason::Overflow;
                    }
                };
                if let Some(reason) = handle_outbound(conn, event, None).await {
                    return reason;
                }
                // One event handled; loop to drain the rest and flush.
                continue;
            }
            _ = timeout(config.read_timeout, std::future::pending::<()>()) => {
                return DisconnectReason::Timeout;
            }
        };
        // Decode panics (a truncated scalar read — `FriendlyByteBuf.readLong`
        // on a short body) are caught at the decode boundary in
        // [`decode_packet`], which returns a clean `DisconnectReason::Malformed`
        // (Java's `PacketDecoder` turning the unchecked `IndexOutOfBoundsException`
        // into a close). Nothing here panics on hostile input: the frame decoder,
        // the compression decoder, and every listener body path return `Err`
        // deterministically, so the task tail in `run_connection` (cap decrement,
        // `on_disconnect`, `connection_closed`) always runs.
        if in_play {
            // Play state: forward decoded frames to the tick thread. No
            // listener; the tick thread owns play-state dispatch (#101).
            if let Err(reason) = conn.forward_play(Some(&chunk[..n]), in_tx).await {
                return forward_play_failed(
                    conn,
                    &mut out_rx,
                    reason,
                    &shutdown,
                    config.read_timeout,
                )
                .await;
            }
        } else {
            match conn.process_inbound(&chunk[..n], listener) {
                Ok(InboundOutcome::Keep) => {}
                Ok(InboundOutcome::Play) => {
                    in_play = true;
                    // Frames already buffered when the handoff fired (a client
                    // that coalesced `finish_configuration` with a play packet)
                    // are drained into the tick channel now.
                    if let Err(reason) = conn.forward_play(None, in_tx).await {
                        return forward_play_failed(
                            conn,
                            &mut out_rx,
                            reason,
                            &shutdown,
                            config.read_timeout,
                        )
                        .await;
                    }
                }
                Err(reason) => return reason,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rivet_protocol::generated::protocol::ConnectionProtocol;
    use rivet_protocol::varint21_length_field_prepender::encode_frame;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    use crate::server::network::packet_listener::ListenerOutcome;

    fn test_config() -> Arc<ServerConfig> {
        Arc::new(ServerConfig::default())
    }

    /// A throwaway connected `Connection` + its client socket (the client reads
    /// whatever the server flushes).
    async fn conn_and_client() -> (Connection, TcpStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (_read, write) = server_sock.into_split();
        let conn = Connection::new(
            ConnectionId(1),
            addr,
            test_config(),
            Arc::new(Shutdown::new()),
            write,
        );
        (conn, client)
    }

    /// Read one VarInt21 frame payload from the client socket.
    async fn read_frame(client: &mut TcpStream) -> Vec<u8> {
        let mut header = Vec::new();
        loop {
            let mut b = [0u8; 1];
            client.read_exact(&mut b).await.unwrap();
            header.push(b[0]);
            if b[0] & 0x80 == 0 {
                break;
            }
        }
        let mut out: u32 = 0;
        for (i, &byte) in header.iter().enumerate() {
            out |= ((byte & 0x7F) as u32) << (i * 7);
        }
        let mut payload = vec![0u8; out as usize];
        client.read_exact(&mut payload).await.unwrap();
        payload
    }

    /// A listener that transitions to the play state on its first frame,
    /// signalling a `Notify` so the test knows the handoff happened.
    struct PlayOnFirst {
        played: Arc<tokio::sync::Notify>,
    }

    impl PacketListener for PlayOnFirst {
        fn protocol(&self) -> ConnectionProtocol {
            ConnectionProtocol::Configuration
        }

        fn handle_frame(
            &mut self,
            _frame: Bytes,
            _conn: &mut Connection,
            _config: &ServerConfig,
        ) -> Result<ListenerOutcome, DisconnectReason> {
            self.played.notify_one();
            Ok(ListenerOutcome::Play)
        }
    }

    /// `forward_play_failed` passes a non-`ServerShutdown` reason straight
    /// through without draining the outbound channel.
    #[tokio::test]
    async fn forward_play_failed_passes_through_non_shutdown() {
        let (mut conn, _client) = conn_and_client().await;
        let (out_tx, mut out_rx) = mpsc::channel::<OutboundEvent>(4);
        // A queued outbound frame that must NOT be drained by the passthrough.
        out_tx
            .send(OutboundEvent::Packet {
                frame: Bytes::from_static(b"stays"),
            })
            .await
            .unwrap();
        let shutdown = Arc::new(Shutdown::new());

        let reason = forward_play_failed(
            &mut conn,
            &mut out_rx,
            DisconnectReason::Malformed("bad".into()),
            &shutdown,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(reason, DisconnectReason::Malformed("bad".into()));
        // The outbound frame is untouched.
        assert!(matches!(
            out_rx.try_recv(),
            Ok(OutboundEvent::Packet { .. })
        ));
    }

    /// `forward_play_failed` with `ServerShutdown` and no server stop drains the
    /// queued outbound to the client (the queued-frame flush contract) and maps
    /// the low reason to `Overflow`, mirroring the `out_rx.recv() == None`
    /// branch.
    #[tokio::test]
    async fn forward_play_failed_server_shutdown_without_stop_drains_and_overflows() {
        let (mut conn, mut client) = conn_and_client().await;
        let (out_tx, mut out_rx) = mpsc::channel::<OutboundEvent>(4);
        out_tx
            .send(OutboundEvent::Packet {
                frame: Bytes::from(encode_frame(b"CONTROL").unwrap().to_vec()),
            })
            .await
            .unwrap();
        drop(out_tx); // drain_to_close sees None after the queued packet
        let shutdown = Arc::new(Shutdown::new()); // NOT requested

        let reason = forward_play_failed(
            &mut conn,
            &mut out_rx,
            DisconnectReason::ServerShutdown,
            &shutdown,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(reason, DisconnectReason::Overflow);
        // The queued outbound frame reached the client before the close.
        assert_eq!(read_frame(&mut client).await, b"CONTROL");
    }

    /// `forward_play_failed` with `ServerShutdown` during a real server stop
    /// drains the outbound and reports `ServerShutdown`.
    #[tokio::test]
    async fn forward_play_failed_server_shutdown_with_stop_drains_and_reports_shutdown() {
        let (mut conn, mut client) = conn_and_client().await;
        let (out_tx, mut out_rx) = mpsc::channel::<OutboundEvent>(4);
        out_tx
            .send(OutboundEvent::Packet {
                frame: Bytes::from(encode_frame(b"CONTROL").unwrap().to_vec()),
            })
            .await
            .unwrap();
        drop(out_tx);
        let shutdown = Arc::new(Shutdown::new());
        shutdown.request();

        let reason = forward_play_failed(
            &mut conn,
            &mut out_rx,
            DisconnectReason::ServerShutdown,
            &shutdown,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(reason, DisconnectReason::ServerShutdown);
        assert_eq!(read_frame(&mut client).await, b"CONTROL");
    }

    /// The deterministic play-exit regression: `conn_loop` parked inside
    /// `forward_play` on a full inbound channel. Dropping the tick-side receiver
    /// fails the parked `send`, `forward_play` reports `ServerShutdown`, and
    /// `forward_play_failed` drains the queued outbound to the client before the
    /// connection closes.
    #[tokio::test]
    async fn conn_loop_parked_in_forward_play_flushes_queued_outbound_on_tick_gone() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(addr).await.unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (read, write) = server_sock.into_split();
        let config = test_config();
        let shutdown = Arc::new(Shutdown::new());
        let mut conn = Connection::new(
            ConnectionId(1),
            addr,
            Arc::clone(&config),
            Arc::clone(&shutdown),
            write,
        );

        // Inbound channel capacity 1: a single forwarded frame fills it, so the
        // second `send` parks.
        let (in_tx, in_rx) = mpsc::channel::<ServerboundFrame>(1);
        let (out_tx, out_rx) = mpsc::channel::<OutboundEvent>(4);
        let played = Arc::new(tokio::sync::Notify::new());
        let mut listener_box: Box<dyn PacketListener> = Box::new(PlayOnFirst {
            played: Arc::clone(&played),
        });
        let in_tx_for_task = in_tx.clone();

        let task = tokio::spawn(async move {
            conn_loop(
                &mut conn,
                &mut listener_box,
                &config,
                read,
                out_rx,
                &in_tx_for_task,
                shutdown,
            )
            .await
        });

        // 1. Handoff trigger: the listener returns Play, conn_loop enters the
        // play state (forward_play(None) drains nothing buffered).
        client
            .write_all(&encode_frame(&[0x00]).unwrap())
            .await
            .unwrap();
        played.notified().await;

        // 2. Two play frames in one write: the first fills the channel (capacity
        // 1), the second parks `forward_play` inside `send().await`.
        let mut ab = Vec::new();
        ab.extend_from_slice(&encode_frame(&[0x01]).unwrap());
        ab.extend_from_slice(&encode_frame(&[0x02]).unwrap());
        client.write_all(&ab).await.unwrap();

        // 3. Deterministically wait for the park: the channel is full (the first
        // frame was pushed), so conn_loop is inside forward_play.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while in_tx.capacity() != 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "conn_loop never filled the inbound channel"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        // 4. Queue the outbound control frame, then drop the tick-side receiver
        // and sender. The parked send fails, forward_play_failed drains the
        // queued outbound to the client.
        out_tx
            .send(OutboundEvent::Packet {
                frame: Bytes::from(encode_frame(b"CONTROL").unwrap().to_vec()),
            })
            .await
            .unwrap();
        drop(in_rx);
        drop(out_tx);

        // 5. The queued outbound reaches the client before the close; the loop
        // exits with Overflow (ServerShutdown error, no server stop requested).
        assert_eq!(read_frame(&mut client).await, b"CONTROL");
        let reason = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("conn_loop did not exit")
            .unwrap();
        assert_eq!(reason, DisconnectReason::Overflow);
    }
}

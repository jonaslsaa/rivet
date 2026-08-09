//! `net.minecraft.server` — the server surface.
//!
//! `ServerConfig` is the immutable config snapshot (OWNERSHIP "config
//! snapshots" exception — `Arc`-shared), `Server` owns the configuration and
//! runs the two halves of the M1 spine: the tokio-side accept loop
//! (`ServerConnectionListener`) and the sync tick thread (`tick::ServerTickLoop`,
//! OWNERSHIP "one owner: the tick thread"). The connection registry is the only
//! shared mutable structure — the OWNERSHIP "connection registry" exception.

pub mod keepalive;
pub mod level;
pub mod lighting;
pub mod movement_math;
pub mod movement_trace;
pub mod network;
pub mod player;
pub mod teleport_ack;
pub mod tick;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::server::tick::TickContext;
use crate::server::tick::channels::LifecycleEvent;
use network::server_connection_listener::ServerConnectionListener;
use tick::endpoint::NetworkEndpoint;
use tick::shutdown::Shutdown;
use tick::time::RealTime;
use tick::time::TickTime;
use tick::{ServerTickLoop, TickScheduler, TickStats, Tickable};

/// `ServerConnectionListener` port config plus the tick-loop knobs (issue #93).
/// Fields are immutable after startup.
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
    /// Compression threshold — Paper `MinecraftServer.getCompressionThreshold()`
    /// returns `256`; a negative value disables compression. Sent to the client
    /// in `ClientboundLoginCompressionPacket` and applied by
    /// `Connection::setupCompression` at login.
    pub compression_threshold: i32,
    /// Tick interval — Paper `TickRateManager.nanosecondsPerTick()` = 1s / 20.
    pub tick_interval: Duration,
    /// Max backlogged ticks the schedule catches up before dropping (Paper
    /// `GlobalConfiguration misc.catchupTicks`, default 5).
    pub catchup_ticks: u64,
    /// Per-connection inbound (network→tick) channel capacity.
    pub inbound_channel_capacity: usize,
    /// Per-connection outbound (tick→network) channel capacity.
    pub outbound_channel_capacity: usize,
    /// Network→tick lifecycle/registration channel capacity.
    pub lifecycle_capacity: usize,
    /// Live play sessions: when set, `Server::new` wires the tick-owned
    /// [`PlayerSessionManager`](player::session::PlayerSessionManager) that
    /// consumes configuration→play handoffs and fires the join burst (issue
    /// #101 Slice B). Off by default so the offline-login tests exercise the
    /// handoff seam without the burst; the M1 binary enables it.
    pub enable_join: bool,
    /// The keepalive kick limit (`paper.playerconnection.keepalive`, issue
    /// #236). Paper's default 30s is pinned; a shorter value lets the live
    /// keepalive tests exercise the timeout window without a half-minute wait
    /// each (the 1s transmit cadence is never configurable, as in Java).
    pub keepalive_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind_host: IpAddr::from([0, 0, 0, 0]),
            port: 25565,
            max_connections: 100,
            read_timeout: Duration::from_secs(30),
            compression_threshold: 256,
            tick_interval: Duration::from_millis(50),
            catchup_ticks: 5,
            inbound_channel_capacity: 1024,
            outbound_channel_capacity: 1024,
            lifecycle_capacity: 256,
            enable_join: false,
            keepalive_timeout: Duration::from_secs(30),
        }
    }
}

/// The server: owns the immutable config, the network→tick lifecycle channel,
/// and the shutdown signal; runs the accept loop and the tick thread. Mirrors
/// `MinecraftServer` owning a `ServerConnectionListener` plus the "Server
/// thread".
pub struct Server {
    config: Arc<ServerConfig>,
    endpoint: Arc<NetworkEndpoint>,
    shutdown: Arc<Shutdown>,
    stats: Arc<TickStats>,
    tickables: Vec<Tickable>,
    lifecycle_rx: Option<mpsc::Receiver<LifecycleEvent>>,
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        let config = Arc::new(config);
        let shutdown = Arc::new(Shutdown::new());
        let (lifecycle_tx, lifecycle_rx) = mpsc::channel(config.lifecycle_capacity);
        let endpoint = Arc::new(NetworkEndpoint::new(lifecycle_tx, shutdown.clone()));
        let mut tickables: Vec<Tickable> = Vec::new();
        // Live play sessions (issue #101 Slice B): the tick-owned session manager
        // that consumes configuration→play handoffs and fires the join burst. Off
        // by default so the offline-login tests exercise the handoff seam without
        // the burst; the M1 binary enables it. The session manager is moved into
        // the tick thread by `serve` (`std::mem::take`).
        if config.enable_join {
            let mut session = player::session::default_session_config(config.compression_threshold);
            session.keepalive_timeout_ns = config.keepalive_timeout.as_nanos() as i64;
            tickables.push(player::session::session_manager_tickable(session));
        }
        Server {
            config,
            endpoint,
            shutdown,
            stats: Arc::new(TickStats::default()),
            tickables,
            lifecycle_rx: Some(lifecycle_rx),
        }
    }

    /// Register a tickable that runs every tick on the tick thread. Tests use
    /// this to observe tick processing; play-state systems land here later.
    pub fn with_tickable(
        mut self,
        tickable: impl FnMut(&mut TickContext) + Send + 'static,
    ) -> Self {
        self.tickables.push(Box::new(tickable));
        self
    }

    /// Live tick-thread counters (tick number, connected connections).
    pub fn stats(&self) -> Arc<TickStats> {
        Arc::clone(&self.stats)
    }

    /// The shutdown handle, for a signal handler / external caller.
    pub fn shutdown_handle(&self) -> Arc<Shutdown> {
        Arc::clone(&self.shutdown)
    }

    /// Request orderly shutdown: the accept loop stops, connection tasks close,
    /// and the tick thread drains and exits.
    pub fn shutdown(&self) {
        self.endpoint.shutdown();
    }

    /// The immutable config snapshot.
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Bind the TCP listener without accepting yet (tests use this to learn the
    /// ephemeral port from `TcpListener::local_addr`).
    pub async fn bind(&self) -> std::io::Result<tokio::net::TcpListener> {
        ServerConnectionListener::new(
            self.config.clone(),
            Arc::clone(&self.endpoint),
            Arc::clone(&self.shutdown),
        )
        .bind()
        .await
    }

    /// Serve on an already-bound listener: spawn the tick thread, then run the
    /// accept loop until shutdown, then join the tick thread. If the future is
    /// dropped before that (cancellation, runtime teardown), the [`TickThread`]
    /// guard requests shutdown so the tick thread stops ticking and exits on its
    /// own instead of leaking.
    pub async fn serve(mut self, listener: tokio::net::TcpListener) -> std::io::Result<()> {
        let lifecycle_rx = self
            .lifecycle_rx
            .take()
            .expect("Server::serve called more than once");

        // Spawn the tick thread first, so registrations from the accept loop
        // below have a receiver to land in.
        let config = Arc::clone(&self.config);
        let shutdown = Arc::clone(&self.shutdown);
        let tickables = std::mem::take(&mut self.tickables);
        let stats = Arc::clone(&self.stats);
        let tick_handle = std::thread::Builder::new()
            .name("rivet-tick".into())
            .spawn(move || {
                let time: Arc<dyn TickTime> = Arc::new(RealTime::new(Arc::clone(&shutdown)));
                let scheduler = TickScheduler::new(
                    config.tick_interval.as_nanos(),
                    config.catchup_ticks,
                    time.now_nanos(),
                );
                let loop_ =
                    ServerTickLoop::new(scheduler, time, shutdown, lifecycle_rx, tickables, stats);
                loop_.run();
            })
            .expect("spawn tick thread");
        let tick_thread = TickThread::new(Arc::clone(&self.shutdown), tick_handle);

        // Accept loop until shutdown, then join the tick thread.
        let listener_ = ServerConnectionListener::new(
            self.config.clone(),
            Arc::clone(&self.endpoint),
            Arc::clone(&self.shutdown),
        );
        let result = listener_.serve(listener).await;

        // Normal path: the tick thread sees shutdown too (idempotent) and is
        // joined before serve returns.
        tick_thread.join();
        result
    }

    /// Bind and serve (the binary entry path).
    pub async fn run(self) -> std::io::Result<()> {
        let listener = self.bind().await?;
        self.serve(listener).await
    }
}

/// Owns the spawned tick OS thread for `Server::serve`.
///
/// On the normal path, [`TickThread::join`] requests shutdown and joins the
/// thread before `serve` returns. If the `serve` future is dropped first —
/// cancellation, runtime teardown, a panic in the accept loop — `Drop` requests
/// shutdown and briefly waits for the loop to observe it (its idle sleep wakes
/// on the condvar), joining when it exits; only a thread still running after the
/// grace period is detached (`JoinHandle` dropped). A detached thread always
/// exits on its own once it observes the flag (it cannot loop forever), so the
/// handle is never the thing keeping it alive.
struct TickThread {
    shutdown: Arc<Shutdown>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TickThread {
    fn new(shutdown: Arc<Shutdown>, handle: std::thread::JoinHandle<()>) -> Self {
        TickThread {
            shutdown,
            handle: Some(handle),
        }
    }

    /// Request shutdown and join the tick thread (the accept loop has returned).
    fn join(mut self) {
        self.shutdown.request();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TickThread {
    fn drop(&mut self) {
        // Cancelled before `join`: request shutdown so the loop wakes from its
        // idle sleep, then wait briefly for it to exit. The drop may run on an
        // async worker (task cancellation), so the wait is bounded — a short
        // sleep loop is acceptable here because the thread is exiting anyway
        // (the flag is set, so its idle wait wakes immediately and the loop
        // terminates). Joining when it exits reclaims the OS thread rather than
        // leaving a short-lived detached thread behind; anything still running
        // after the grace period finishes independently and detaches cleanly.
        self.shutdown.request();
        if let Some(handle) = self.handle.take() {
            let deadline = Instant::now() + Duration::from_millis(100);
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

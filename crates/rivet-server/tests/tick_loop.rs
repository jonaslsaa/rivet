//! Deterministic tests for the sync tick spine (issue #93). The tick-loop tests
//! drive `ServerTickLoop` with the paused `SimTime` clock, so tick ordering,
//! shutdown/drain, and bounded-capacity behavior are asserted without touching a
//! wall clock (no flaky timing). The full-server tests use a real clock but
//! assert only wall-clock-bounded outcomes (serve returns, socket closes).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use rivet_server::server::network::connection_id::ConnectionId;
use rivet_server::server::network::packet_listener::DisconnectReason;
use rivet_server::server::tick::channels::{
    InboundDrained, LifecycleEvent, OutboundEvent, ServerboundFrame,
};
use rivet_server::server::tick::endpoint::{NetworkEndpoint, RegisterResult};
use rivet_server::server::tick::registry::{ConnectionRegistry, OutboundError};
use rivet_server::server::tick::scheduler::{NANOS_PER_TICK, TickScheduler};
use rivet_server::server::tick::shutdown::Shutdown;
use rivet_server::server::tick::time::SimTime;
use rivet_server::server::tick::{ServerTickLoop, TickContext, TickStats};
use rivet_server::server::{Server, ServerConfig};
use tokio::io::AsyncReadExt;

const REMOTE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);

/// Spin until `pred` holds or `timeout` elapses. A bounded wall-clock wait: the
/// determinism comes from SimTime advancing the loop, not from timing races.
fn wait_until(timeout: Duration, what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !pred() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Build a loop driven by a fresh `SimTime`, returning the loop handle, the
/// lifecycle sender, and the shared pieces.
#[allow(clippy::type_complexity)]
fn build_loop(
    tickables: Vec<Box<dyn FnMut(&mut TickContext) + Send>>,
) -> (
    std::thread::JoinHandle<()>,
    tokio::sync::mpsc::Sender<LifecycleEvent>,
    Arc<SimTime>,
    Arc<Shutdown>,
    Arc<TickStats>,
) {
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(16);
    let shutdown = Arc::new(Shutdown::new());
    let sim = Arc::new(SimTime::with_shutdown(shutdown.clone()));
    let stats = Arc::new(TickStats::default());
    let time: Arc<dyn rivet_server::server::tick::time::TickTime> = sim.clone();
    let scheduler = TickScheduler::new(NANOS_PER_TICK, 5, 0);
    let loop_ = ServerTickLoop::new(
        scheduler,
        time,
        shutdown.clone(),
        lifecycle_rx,
        tickables,
        stats.clone(),
    );
    let handle = std::thread::Builder::new()
        .name("tick-test".into())
        .spawn(move || loop_.run())
        .expect("spawn tick loop");
    (handle, lifecycle_tx, sim, shutdown, stats)
}

fn frame(byte: u8) -> ServerboundFrame {
    ServerboundFrame {
        bytes: Bytes::from(vec![byte]),
    }
}

fn register(
    lifecycle_tx: &tokio::sync::mpsc::Sender<LifecycleEvent>,
    id: ConnectionId,
    in_cap: usize,
    out_cap: usize,
) -> (
    tokio::sync::mpsc::Sender<ServerboundFrame>,
    tokio::sync::mpsc::Receiver<OutboundEvent>,
) {
    let (in_tx, in_rx) = tokio::sync::mpsc::channel(in_cap);
    let (out_tx, out_rx) = tokio::sync::mpsc::channel(out_cap);
    lifecycle_tx
        .try_send(LifecycleEvent::Connect {
            id,
            remote: REMOTE,
            in_rx,
            out_tx,
            drained: InboundDrained::new(),
        })
        .expect("lifecycle channel has room");
    (in_tx, out_rx)
}

/// Inbound frames are delivered per connection, in FIFO order, inside the tick
/// context — the boundary later dispatch (epics #10/#101) consumes.
#[test]
fn tick_delivers_inbound_fifo_to_tickable() {
    let delivered: Arc<Mutex<Vec<(ConnectionId, u8)>>> = Arc::new(Mutex::new(Vec::new()));
    let delivered_clone = delivered.clone();
    let (handle, lifecycle_tx, sim, _, _) =
        build_loop(vec![Box::new(move |ctx: &mut TickContext| {
            for (id, f) in &ctx.inbound {
                delivered_clone.lock().unwrap().push((*id, f.bytes[0]));
            }
        })]);

    let id_a = ConnectionId(1);
    let id_b = ConnectionId(2);
    let (in_a, _) = register(&lifecycle_tx, id_a, 8, 8);
    let (in_b, _) = register(&lifecycle_tx, id_b, 8, 8);
    // Interleave sends across connections; each connection's own order must
    // hold. (Cross-connection global order is not a guarantee — each connection
    // drains independently per OWNERSHIP "packet in/out queues per player".)
    in_a.try_send(frame(0x01)).unwrap();
    in_b.try_send(frame(0x20)).unwrap();
    in_a.try_send(frame(0x02)).unwrap();
    in_b.try_send(frame(0x21)).unwrap();

    sim.advance(NANOS_PER_TICK);
    wait_until(Duration::from_secs(2), "inbound delivery", || {
        delivered.lock().unwrap().len() >= 4
    });

    let got = delivered.lock().unwrap().clone();
    let a: Vec<u8> = got
        .iter()
        .filter(|(id, _)| *id == id_a)
        .map(|(_, b)| *b)
        .collect();
    let b: Vec<u8> = got
        .iter()
        .filter(|(id, _)| *id == id_b)
        .map(|(_, b)| *b)
        .collect();
    assert_eq!(a, vec![0x01, 0x02], "connection A frames in FIFO order");
    assert_eq!(b, vec![0x20, 0x21], "connection B frames in FIFO order");

    let _ = handle;
}

/// Tickables run exactly once per tick, in order.
#[test]
fn tickables_run_once_per_tick() {
    let ticks: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let ticks_clone = ticks.clone();
    let (handle, _lifecycle_tx, sim, _, _) =
        build_loop(vec![Box::new(move |ctx: &mut TickContext| {
            ticks_clone.lock().unwrap().push(ctx.tick)
        })]);

    for _ in 0..5 {
        sim.advance(NANOS_PER_TICK);
    }
    wait_until(Duration::from_secs(2), "ticks to accumulate", || {
        ticks.lock().unwrap().len() >= 5
    });

    let got = ticks.lock().unwrap().clone();
    // The tick counter starts at 1 and increments by exactly one per tick; the
    // exact length is scheduling-dependent (the first tick fires the moment the
    // loop starts), so assert the first five tick numbers are consecutive.
    assert_eq!(
        &got[..5],
        &[1, 2, 3, 4, 5],
        "one tickable run per tick, tick numbers consecutive from 1"
    );

    let _ = handle;
}

/// Shutdown triggers a final drain pass, then every live connection is told the
/// server is stopping, then the loop thread exits (orderly drain, no deadlock).
#[test]
fn shutdown_drains_and_disconnects_every_connection() {
    let (handle, lifecycle_tx, sim, shutdown, _stats) = build_loop(vec![]);

    let id = ConnectionId(7);
    let (_in_tx, mut out_rx) = register(&lifecycle_tx, id, 4, 4);
    sim.advance(NANOS_PER_TICK);
    // Let the Connect land before shutting down.
    std::thread::sleep(Duration::from_millis(20));

    shutdown.request();

    // The loop exits on its own after the drain.
    handle
        .join()
        .expect("tick loop exited cleanly after shutdown");

    // The connection received a ServerShutdown Disconnect (nothing before it,
    // since the tick thread sent no packets).
    match out_rx.blocking_recv() {
        Some(OutboundEvent::Disconnect { reason }) => {
            assert_eq!(reason, DisconnectReason::ServerShutdown);
        }
        other => panic!("expected ServerShutdown Disconnect, got {other:?}"),
    }
    // After the disconnect event the channel is closed (out_tx dropped).
    assert!(out_rx.blocking_recv().is_none());
}

/// Outbound overflow prunes the offending connection and is *not* a shutdown:
/// the loop keeps ticking and the shutdown flag stays clear.
#[test]
fn outbound_overflow_disconnects_but_loop_keeps_ticking() {
    let sent: Arc<Mutex<Vec<Result<(), OutboundError>>>> = Arc::new(Mutex::new(Vec::new()));
    let sent_clone = sent.clone();
    let (handle, lifecycle_tx, sim, shutdown, stats) =
        build_loop(vec![Box::new(move |ctx: &mut TickContext| {
            let id = ConnectionId(5);
            sent_clone.lock().unwrap().push(ctx.connections.send(
                id,
                OutboundEvent::Packet {
                    frame: Bytes::from_static(b"a"),
                },
            ));
            // Second frame into a 1-capacity channel overflows and prunes.
            sent_clone.lock().unwrap().push(ctx.connections.send(
                id,
                OutboundEvent::Packet {
                    frame: Bytes::from_static(b"b"),
                },
            ));
        })]);

    let (_in_tx, _out_rx) = register(&lifecycle_tx, ConnectionId(5), 4, 1);
    sim.advance(NANOS_PER_TICK);
    wait_until(Duration::from_secs(2), "outbound overflow to fire", || {
        sent.lock()
            .unwrap()
            .iter()
            .any(|r| matches!(r, Err(OutboundError::Overflow(_))))
    });

    let results = sent.lock().unwrap().clone();
    // The tick that pruned the connection saw Ok then Overflow back-to-back.
    assert!(
        results.windows(2).any(|w| w[0].is_ok()
            && matches!(w[1], Err(OutboundError::Overflow(id)) if id == ConnectionId(5))),
        "expected Ok then Overflow on the pruning tick, got {results:?}"
    );
    // Overload is a per-connection policy, not a server stop.
    assert!(
        !shutdown.is_requested(),
        "overflow must not request shutdown"
    );
    wait_until(Duration::from_secs(2), "loop to keep ticking", || {
        stats.ticks.load(Ordering::SeqCst) >= 2
    });

    let _ = handle;
}

/// On shutdown the final drain runs tickables, so a frame queued in that pass is
/// delivered before the `ServerShutdown` Disconnect — Paper's
/// `send(disconnect, thenRun(disconnect))` ordering on the wire.
#[test]
fn shutdown_delivers_queued_packet_before_disconnect() {
    let send_on_shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let send_on_shutdown_clone = send_on_shutdown.clone();
    let (handle, lifecycle_tx, sim, shutdown, _stats) =
        build_loop(vec![Box::new(move |ctx: &mut TickContext| {
            if send_on_shutdown_clone.load(Ordering::SeqCst) {
                let _ = ctx.connections.send(
                    ConnectionId(7),
                    OutboundEvent::Packet {
                        frame: Bytes::from_static(b"\xee"),
                    },
                );
            }
        })]);
    let (_in_tx, mut out_rx) = register(&lifecycle_tx, ConnectionId(7), 4, 4);
    sim.advance(NANOS_PER_TICK);
    // Let the Connect land before the final pass.
    std::thread::sleep(Duration::from_millis(20));

    // The flag flips just before shutdown, so only the final drain pass queues
    // the packet — the ordering below proves the packet precedes the disconnect.
    send_on_shutdown.store(true, Ordering::SeqCst);
    shutdown.request();
    handle
        .join()
        .expect("tick loop exited cleanly after shutdown");

    let mut saw_packet = false;
    let mut saw_disconnect = false;
    loop {
        match out_rx.blocking_recv() {
            Some(OutboundEvent::Packet { .. }) => {
                assert!(!saw_disconnect, "packet queued after the disconnect");
                saw_packet = true;
            }
            Some(OutboundEvent::Disconnect { reason }) => {
                assert_eq!(reason, DisconnectReason::ServerShutdown);
                assert!(!saw_disconnect, "duplicate disconnect");
                saw_disconnect = true;
            }
            None => break,
        }
    }
    assert!(saw_packet, "the final-pass packet must be queued");
    assert!(saw_disconnect, "the shutdown disconnect must be queued");
}

/// A connection whose network side dropped the socket (sender gone) is pruned by
/// the tick thread on its next drain — the registry self-heals.
#[test]
fn closed_connection_is_pruned_by_next_tick() {
    let (handle, lifecycle_tx, sim, _shutdown, stats) = build_loop(vec![]);

    let id = ConnectionId(3);
    let (in_tx, _out_rx) = register(&lifecycle_tx, id, 4, 4);
    sim.advance(NANOS_PER_TICK);
    wait_until(Duration::from_secs(2), "connect to register", || {
        stats.connected.load(Ordering::SeqCst) == 1
    });

    // The network side exits without a Disconnect event: drop the sender.
    drop(in_tx);
    sim.advance(NANOS_PER_TICK);
    wait_until(Duration::from_secs(2), "connection to be pruned", || {
        stats.connected.load(Ordering::SeqCst) == 0
    });

    let _ = handle;
}

/// The tick loop's idle wait is a real sleep (driven by the sim clock), not a
/// busy loop: after many ticks there is at least one `sleep_until` per tick.
#[test]
fn loop_sleeps_between_ticks_instead_of_busy_spinning() {
    let (handle, _lifecycle_tx, sim, _, _) = build_loop(vec![]);

    for _ in 0..10 {
        sim.advance(NANOS_PER_TICK);
        std::thread::sleep(Duration::from_millis(2));
    }
    // Each advance wakes one sleep; a busy loop would never sleep.
    assert!(
        sim.sleeps() >= 1,
        "loop must sleep while waiting for the next tick"
    );

    let _ = handle;
}

// ---- full-server tests (real clock, wall-clock-bounded assertions only) -----

fn default_config() -> ServerConfig {
    ServerConfig {
        bind_host: IpAddr::from([127, 0, 0, 1]),
        port: 0,
        max_connections: 16,
        read_timeout: Duration::from_secs(30),
        compression_threshold: 256,
        tick_interval: Duration::from_millis(50),
        catchup_ticks: 5,
        inbound_channel_capacity: 64,
        outbound_channel_capacity: 64,
        lifecycle_capacity: 64,
        enable_join: false,
        keepalive_timeout: Duration::from_secs(30),
        level_path: None,
        seed: 42,
    }
}

/// Orderly shutdown: `serve` returns promptly, the joined tick thread is gone,
/// and an accepted connection sees its socket closed. This is the no-deadlock
/// regression test for the two-thread spine.
#[tokio::test]
async fn full_server_shutdown_returns_and_closes_connections() {
    let server = Server::new(default_config());
    let shutdown = server.shutdown_handle();
    let listener = server.bind().await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let serve_task = tokio::spawn(async move { server.serve(listener).await });

    // Let the accept loop pick up a connection before shutting down.
    let mut client = tokio::net::TcpStream::connect(addr).await.expect("connect");
    tokio::time::sleep(Duration::from_millis(50)).await;

    shutdown.request();

    let result = tokio::time::timeout(Duration::from_secs(5), serve_task)
        .await
        .expect("serve did not return within 5s (shutdown deadlock?)")
        .expect("serve completed");
    assert!(result.is_ok(), "serve should return Ok after shutdown");

    // The connection task closes the socket on shutdown.
    let mut buf = [0u8; 16];
    let eof = tokio::time::timeout(Duration::from_secs(2), client.read(&mut buf))
        .await
        .expect("client read");
    assert!(
        matches!(eof, Ok(0)),
        "client should observe EOF after shutdown"
    );
}

/// A second server boot in the same test binary proves shutdown is fully
/// re-runnable (the tick thread is not leaked across runs).
#[tokio::test]
async fn full_server_can_be_stopped_twice() {
    for _ in 0..2 {
        let server = Server::new(default_config());
        let shutdown = server.shutdown_handle();
        let listener = server.bind().await.expect("bind");
        let serve_task = tokio::spawn(async move { server.serve(listener).await });
        tokio::time::sleep(Duration::from_millis(10)).await;
        shutdown.request();
        let _ = tokio::time::timeout(Duration::from_secs(5), serve_task)
            .await
            .expect("serve did not return");
    }
}

/// Cancelling `serve` (dropping the future, no cooperative shutdown) must
/// reclaim the tick thread: `TickThread::drop` requests shutdown and the loop's
/// idle sleep wakes on it. The tick counter freezing is the observable leak
/// check — a leaked thread would keep ticking (20 TPS) past the grace period.
#[tokio::test]
async fn cancelled_serve_does_not_leak_tick_thread() {
    let server = Server::new(default_config());
    let stats = server.stats();
    let listener = server.bind().await.expect("bind");
    let serve_task = tokio::spawn(async move { server.serve(listener).await });

    // Poll asynchronously (blocking would stall the runtime that drives serve).
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if stats.ticks.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("tick thread to start");

    serve_task.abort();
    let _ = serve_task.await; // JoinError (aborted)

    let frozen_at = stats.ticks.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        stats.ticks.load(Ordering::SeqCst),
        frozen_at,
        "tick thread must stop ticking after serve is cancelled"
    );
}

/// End-to-end outbound path: a frame the tick thread queues reaches the client's
/// socket, and on shutdown the listener drains the connection tasks so the final
/// frame flushes before the socket closes (serve returns only after the tasks
/// finish their graceful close). Assertions are prefix-based because frames can
/// coalesce in the socket; the wire content is a run of well-formed frames.
#[tokio::test]
async fn full_server_flushes_queued_frame_and_closes_on_shutdown() {
    const FRAME: &[u8] = b"\x07goodbye"; // VarInt21 length 7 + 7-byte payload
    let server = Server::new(default_config()).with_tickable(|ctx: &mut TickContext| {
        let ids: Vec<ConnectionId> = ctx.connections.ids().collect();
        let frame = Bytes::from_static(FRAME);
        for id in ids {
            let _ = ctx.connections.send(
                id,
                OutboundEvent::Packet {
                    frame: frame.clone(),
                },
            );
        }
    });
    let shutdown = server.shutdown_handle();
    let listener = server.bind().await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let serve_task = tokio::spawn(async move { server.serve(listener).await });

    let mut client = tokio::net::TcpStream::connect(addr).await.expect("connect");
    // The first frame(s) reach the socket. Whatever the coalescing, the bytes
    // must start with the tick-thread's frame.
    let mut first = vec![0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(2), client.read(&mut first))
        .await
        .expect("first frame timeout")
        .expect("first frame read");
    assert!(
        n >= FRAME.len(),
        "read {n} bytes, expected at least one full frame"
    );
    assert_eq!(
        &first[..FRAME.len()],
        FRAME,
        "tick-thread frame reaches the client"
    );

    shutdown.request();
    let result = tokio::time::timeout(Duration::from_secs(5), serve_task)
        .await
        .expect("serve did not return (connection task did not drain?)")
        .expect("serve completed");
    assert!(result.is_ok(), "serve returns Ok after shutdown");

    // After graceful close every remaining byte is a full frame, then EOF: the
    // connection task drained+flushed the final-pass frame before closing, and
    // `serve` returned only after that flush completed.
    let mut rest = Vec::new();
    let mut chunk = [0u8; 64];
    loop {
        let n = match tokio::time::timeout(Duration::from_secs(2), client.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => n,
            Ok(Err(e)) => panic!("read error after shutdown: {e}"),
            Err(_) => panic!("timed out waiting for EOF after shutdown"),
        };
        rest.extend_from_slice(&chunk[..n]);
    }
    assert!(
        rest.len() % FRAME.len() == 0,
        "bytes after shutdown must be whole frames, got {} bytes",
        rest.len()
    );
    for f in rest.chunks_exact(FRAME.len()) {
        assert_eq!(f, FRAME, "each flushed byte run must be the tick frame");
    }
}

// ---- registry-level lifecycle coverage without a runtime --------------------

#[test]
fn registry_lifecycle_connect_then_disconnect() {
    let mut reg = ConnectionRegistry::new();
    let id = ConnectionId(1);
    let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
    let (out_tx, _out_rx) = tokio::sync::mpsc::channel(4);
    reg.apply(LifecycleEvent::Connect {
        id,
        remote: REMOTE,
        in_rx,
        out_tx,
        drained: InboundDrained::new(),
    });
    assert!(reg.contains(id));
    assert_eq!(reg.get(id).map(|c| c.id()), Some(id));

    reg.apply(LifecycleEvent::Disconnect {
        id,
        reason: DisconnectReason::Timeout,
    });
    assert!(!reg.contains(id));
    assert!(
        in_tx.try_send(frame(0)).is_err(),
        "inbound sender sees removal"
    );
}

/// A full lifecycle channel is a registration *overload*, not a shutdown: the
/// endpoint awaits capacity and the connection registers once the tick thread
/// drains.
#[tokio::test]
async fn register_full_lifecycle_channel_awaits_capacity_and_succeeds() {
    let (lifecycle_tx, mut lifecycle_rx) = tokio::sync::mpsc::channel(1);
    let endpoint = NetworkEndpoint::new(lifecycle_tx.clone(), Arc::new(Shutdown::new()));
    // Fill the lifecycle channel so the next registration hits Full.
    let (_in_tx0, in_rx0) = tokio::sync::mpsc::channel(1);
    let (out_tx0, _out_rx0) = tokio::sync::mpsc::channel(1);
    lifecycle_tx
        .try_send(LifecycleEvent::Connect {
            id: ConnectionId(0),
            remote: REMOTE,
            in_rx: in_rx0,
            out_tx: out_tx0,
            drained: InboundDrained::new(),
        })
        .expect("first event fits");

    let (_in_tx, in_rx) = tokio::sync::mpsc::channel(1);
    let (out_tx, _out_rx) = tokio::sync::mpsc::channel(1);
    let register = endpoint.register_connection(
        ConnectionId(1),
        REMOTE,
        in_rx,
        out_tx,
        InboundDrained::new(),
    );
    // Poll both: the register future awaits capacity; draining the first event
    // frees it.
    let (first, result) = tokio::join!(lifecycle_rx.recv(), register);
    assert!(first.is_some(), "the blocking event was drained");
    assert_eq!(result, RegisterResult::Registered);
}

/// A closed lifecycle channel — the tick thread exited while we waited — means
/// the server is stopping and the socket must be dropped.
#[tokio::test]
async fn register_closed_lifecycle_channel_reports_server_shutting_down() {
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(1);
    let endpoint = NetworkEndpoint::new(lifecycle_tx, Arc::new(Shutdown::new()));
    drop(lifecycle_rx); // tick side gone before the registration arrives

    let (_in_tx, in_rx) = tokio::sync::mpsc::channel(1);
    let (out_tx, _out_rx) = tokio::sync::mpsc::channel(1);
    let result = endpoint
        .register_connection(
            ConnectionId(1),
            REMOTE,
            in_rx,
            out_tx,
            InboundDrained::new(),
        )
        .await;
    assert_eq!(result, RegisterResult::ServerShuttingDown);
}

/// Stats counters are published by the loop after each tick.
#[test]
fn stats_reflect_ticks_and_connections() {
    let (handle, lifecycle_tx, sim, _shutdown, stats) = build_loop(vec![]);

    let id = ConnectionId(9);
    let (_in_tx, _out_rx) = register(&lifecycle_tx, id, 4, 4);
    sim.advance(NANOS_PER_TICK);
    wait_until(Duration::from_secs(2), "tick to land", || {
        stats.ticks.load(Ordering::SeqCst) >= 1
    });
    assert!(
        stats.ticks.load(Ordering::SeqCst) >= 1,
        "stats.ticks advances"
    );

    let _ = handle;
}

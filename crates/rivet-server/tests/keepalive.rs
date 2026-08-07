//! End-to-end keepalive integration tests (issue #157) on the real tick loop,
//! driven by the deterministic `SimTime` clock. The `KeepaliveState` machine is
//! owned by the *tick thread* — a tickable drives it each tick — and both the
//! clientbound `keep_alive` frame (through `ctx.connections.send`) and the
//! serverbound response (drained from `ctx.inbound`) flow through the real
//! per-connection bounded channels, exercising the ownership pattern the
//! play-side integration (#96) will use.
//!
//! No wall clock is touched: `SimTime` advances, the loop wakes, ticks run,
//! frames flow. `wait_until` bounds each assertion so a stalled loop fails
//! loudly instead of hanging.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes, BytesMut};
use rivet_protocol::var_int;
use rivet_protocol::varint21_length_field_prepender::encode_frame;
use rivet_server::server::keepalive::{KeepaliveResponseOutcome, KeepaliveState};
use rivet_server::server::network::connection_id::ConnectionId;
use rivet_server::server::network::keepalive::{KeepaliveSink, drive_keepalive};
use rivet_server::server::network::packet_listener::DisconnectReason;
use rivet_server::server::tick::channels::{
    InboundDrained, LifecycleEvent, OutboundEvent, ServerboundFrame,
};
use rivet_server::server::tick::registry::ConnectionRegistry;
use rivet_server::server::tick::scheduler::{NANOS_PER_TICK, TickScheduler};
use rivet_server::server::tick::shutdown::Shutdown;
use rivet_server::server::tick::time::SimTime;
use rivet_server::server::tick::{ServerTickLoop, TickContext, TickStats};

const REMOTE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
/// `ConfigurationProtocols` keep_alive packet id (serverbound and clientbound
/// are both 4 in the generated table).
const KEEP_ALIVE_PACKET_ID: i32 = 4;

fn wait_until(timeout: Duration, what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !pred() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A keep_alive frame (`Clientbound`/`Serverbound` share the wire shape):
/// `varint21(len) ++ varint(packetId 4) ++ long id`.
fn keepalive_frame(id: i64) -> Bytes {
    let mut payload = Vec::with_capacity(1 + 8);
    var_int::write(&mut payload, KEEP_ALIVE_PACKET_ID);
    payload.extend_from_slice(&id.to_be_bytes());
    Bytes::from(encode_frame(&payload).unwrap().to_vec())
}

/// Read a Protocol VarInt, returning `None` on a truncated or over-long frame.
/// The network decoder rejects malformed frames before the tick thread in
/// production; the tick-side keepalive handler must never panic on whatever
/// bytes reach it, so this decoder is total (unlike `var_int::read`).
fn read_varint_checked(buf: &mut BytesMut) -> Option<i32> {
    let mut out: u32 = 0;
    for i in 0..5u32 {
        if !buf.has_remaining() {
            return None;
        }
        let b = buf.get_u8();
        out |= ((b & 0x7F) as u32) << (i * 7);
        if b & 0x80 == 0 {
            return Some(out as i32);
        }
    }
    None
}

/// Decode a keep_alive frame into its challenge id; `None` if the frame is not
/// a keep_alive packet or is malformed/truncated. A frame is
/// `varint21(length) ++ varint(packetId) ++ long id` (the length prefix is what
/// `encode_frame` writes and what crosses the tick→network channel).
fn decode_keepalive_id(frame: &Bytes) -> Option<i64> {
    let mut buf = BytesMut::from(&frame[..]);
    // Skip the VarInt21 frame length prefix (and require it to agree with the
    // body), then read the packet id and the 8-byte challenge id.
    let frame_len = read_varint_checked(&mut buf)? as usize;
    if frame_len != buf.len() {
        return None; // length header disagrees with the body — malformed
    }
    if read_varint_checked(&mut buf)? != KEEP_ALIVE_PACKET_ID {
        return None; // not a keep_alive packet
    }
    if buf.remaining() < 8 {
        return None; // truncated long id
    }
    let mut id = [0u8; 8];
    buf.copy_to_slice(&mut id);
    Some(i64::from_be_bytes(id))
}

/// Per-connection keepalive state the tick thread owns, shared with the test
/// through a `Mutex` (test scaffolding only — the production machine has no
/// locks; OWNERSHIP "one owner: the tick thread").
#[derive(Debug, Default)]
struct ConnCell {
    keepalive: KeepaliveState,
    /// Challenge ids transmitted this session, in order.
    sends: Vec<i64>,
    /// Set once a keepalive-related disconnect fires.
    disconnected: Option<DisconnectReason>,
}

/// The tickable's outbound sink: records the send and pushes the real
/// clientbound frame into the connection's bounded channel (Paper's
/// `this.send(new ClientboundKeepAlivePacket(...))`).
struct CellSink<'a> {
    cell: &'a mut ConnCell,
    id: ConnectionId,
    connections: &'a mut ConnectionRegistry,
}

impl KeepaliveSink for CellSink<'_> {
    fn send_keepalive(&mut self, challenge_id: i64) -> Result<(), DisconnectReason> {
        self.cell.sends.push(challenge_id);
        // Route through the real outbound channel. Gone/Overflow means the
        // connection is already closed or pruned by backpressure; the pending
        // challenge stays in the machine, exactly as Paper's netty send would
        // leave it (the connection is gone either way).
        let _ = self.connections.send(
            self.id,
            OutboundEvent::Packet {
                frame: keepalive_frame(challenge_id),
            },
        );
        Ok(())
    }

    fn disconnect_timeout(&mut self) -> DisconnectReason {
        self.cell.disconnected = Some(DisconnectReason::Timeout);
        DisconnectReason::Timeout
    }
}

struct KeepaliveLoop {
    handle: std::thread::JoinHandle<()>,
    #[allow(dead_code)] // keeps the loop alive for the test's duration
    sim: Arc<SimTime>,
    #[allow(dead_code)] // keeps the shutdown signal alive for the loop's idle sleep
    shutdown: Arc<Shutdown>,
    /// Live tick/connection counters, so tests can deterministically wait for
    /// the loop to reach a tick or apply a lifecycle removal.
    stats: Arc<TickStats>,
}

/// Build the tick loop with a keepalive tickable owning `cells`. Returns the
/// loop handle and the lifecycle sender so tests can register connections.
///
/// The tickable runs ON the tick thread: (1) it drives the keepalive of every
/// connection currently in the registry from the shared SimTime clock (Paper's
/// `keepConnectionAlive`), sending the clientbound frame through the registry
/// channel; (2) it drains `ctx.inbound` and feeds serverbound keep_alive
/// responses into `handleKeepAlive` (rx = the same clock), disconnecting on an
/// out-of-order or unmatched id.
///
/// Only registered connections are driven. Keepalive state is owned by a live
/// connection's listener; once a connection leaves the registry (lifecycle
/// `Disconnect` or an overflow prune) its state is no longer ticked — exactly
/// the ownership a play-side integration (#96) will rely on.
#[allow(clippy::type_complexity)]
fn build_keepalive_loop(
    cells: Arc<Mutex<Vec<ConnCell>>>,
    sim: Arc<SimTime>,
    shutdown: Arc<Shutdown>,
) -> (KeepaliveLoop, tokio::sync::mpsc::Sender<LifecycleEvent>) {
    let stats = Arc::new(TickStats::default());
    let stats_for_return = Arc::clone(&stats);
    let time: Arc<dyn rivet_server::server::tick::time::TickTime> = sim.clone();
    let scheduler = TickScheduler::new(NANOS_PER_TICK, 5, 0);
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(16);

    let cells_c = Arc::clone(&cells);
    let sim_c = Arc::clone(&sim);
    let tickables: Vec<Box<dyn FnMut(&mut TickContext) + Send>> = vec![Box::new(move |ctx| {
        let now_ns = sim_c.now_nanos() as i64;
        let now_ms = now_ns / 1_000_000;
        let mut cells = cells_c.lock().unwrap();

        // Outbound: Paper's `keepConnectionAlive` per connection, for
        // connections that are actually in the registry. The state is taken
        // out, driven, and put back (OWNERSHIP take-tick-putback).
        let ids: Vec<ConnectionId> = ctx.connections.ids().collect();
        for id in ids {
            let idx = (id.0 as usize).saturating_sub(1);
            let Some(cell) = cells.get_mut(idx) else {
                continue;
            };
            if cell.disconnected.is_some() {
                continue;
            }
            let mut ka = std::mem::take(&mut cell.keepalive);
            let mut sink = CellSink {
                cell,
                id,
                connections: ctx.connections,
            };
            let _ = drive_keepalive(&mut ka, now_ns, now_ms, &mut sink);
            cell.keepalive = ka;
        }

        // Inbound: a serverbound keep_alive response routes into `handleKeepAlive`.
        for (id, frame) in &ctx.inbound {
            let Some(challenge_id) = decode_keepalive_id(&frame.bytes) else {
                continue;
            };
            let idx = (id.0 as usize).saturating_sub(1);
            let Some(cell) = cells.get_mut(idx) else {
                continue;
            };
            if cell.disconnected.is_some() {
                continue;
            }
            let outcome = cell.keepalive.handle_keepalive(challenge_id, now_ns);
            // Out-of-order / no-match: Java disconnects with TIMEOUT.
            if outcome != KeepaliveResponseOutcome::Accepted {
                cell.disconnected = Some(DisconnectReason::Timeout);
            }
        }
    })];

    let loop_ = ServerTickLoop::new(
        scheduler,
        time,
        shutdown.clone(),
        lifecycle_rx,
        tickables,
        stats,
    );
    let handle = std::thread::Builder::new()
        .name("keepalive-tick".into())
        .spawn(move || loop_.run())
        .expect("spawn tick loop");
    (
        KeepaliveLoop {
            handle,
            sim,
            shutdown,
            stats: stats_for_return,
        },
        lifecycle_tx,
    )
}

/// Register a connection with the given id and channel capacities; returns the
/// inbound sender (to inject client responses) and outbound receiver (to
/// observe clientbound frames).
fn register_with_id(
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

/// Register connection id 1.
fn register(
    lifecycle_tx: &tokio::sync::mpsc::Sender<LifecycleEvent>,
    in_cap: usize,
    out_cap: usize,
) -> (
    tokio::sync::mpsc::Sender<ServerboundFrame>,
    tokio::sync::mpsc::Receiver<OutboundEvent>,
) {
    register_with_id(lifecycle_tx, ConnectionId(1), in_cap, out_cap)
}

/// Advance the sim clock by `ticks` ticks (each `NANOS_PER_TICK`), letting the
/// loop process between advances.
fn advance(sim: &Arc<SimTime>, ticks: u64) {
    for _ in 0..ticks {
        sim.advance(NANOS_PER_TICK);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn one_cell() -> Arc<Mutex<Vec<ConnCell>>> {
    Arc::new(Mutex::new(vec![ConnCell {
        keepalive: KeepaliveState::new(0),
        ..ConnCell::default()
    }]))
}

// ---- send cadence -----------------------------------------------------------

/// The clientbound keep_alive is transmitted once per second, with the millis
/// reading as the challenge id.
#[test]
fn keepalive_sends_once_per_second_and_ids_are_millis() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (_in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 50); // 2500ms
    wait_until(Duration::from_secs(2), "two keepalive sends", || {
        cells.lock().unwrap()[0].sends.len() >= 2
    });

    let sends = cells.lock().unwrap()[0].sends.clone();
    assert_eq!(sends, vec![1000, 2000], "one send per second, id == millis");
    assert_eq!(cells.lock().unwrap()[0].keepalive.pending_len(), 2);

    let _ = loop_.handle;
}

/// Before a full second elapses, nothing is transmitted.
#[test]
fn no_send_before_first_second() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (_in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 19); // 950ms
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        cells.lock().unwrap()[0].sends.len(),
        0,
        "no challenge before 1s elapses"
    );

    advance(&sim, 1); // 1000ms
    wait_until(Duration::from_secs(2), "first keepalive send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });
    assert_eq!(cells.lock().unwrap()[0].sends[0], 1000);

    let _ = loop_.handle;
}

// ---- clientbound wire -------------------------------------------------------

/// The clientbound frame the tick thread pushes through the real channel has
/// the configuration keep_alive packet id (4) and the big-endian long body.
#[test]
fn clientbound_frame_reaches_outbound_channel_with_wire_shape() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (_in_tx, mut out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // 1250ms: one send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // Drain the real outbound channel (the predicate must not consume — collect
    // into a Vec and inspect).
    let mut frames = Vec::new();
    wait_until(Duration::from_secs(2), "outbound frame", || {
        while let Ok(OutboundEvent::Packet { frame }) = out_rx.try_recv() {
            frames.push(frame);
        }
        !frames.is_empty()
    });

    let frame = &frames[0];
    assert_eq!(frame[1], 4, "configuration clientbound keep_alive id");
    let body_start = 2;
    assert_eq!(
        &frame[body_start..body_start + 8],
        &1000_i64.to_be_bytes(),
        "the challenge id is the millis reading, big-endian"
    );

    let _ = loop_.handle;
}

// ---- response validation ----------------------------------------------------

/// A serverbound keep_alive echoing the oldest challenge clears it and updates
/// the latency readout; the connection stays up.
#[test]
fn valid_response_via_inbound_clears_pending() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // The client echoes 1000 back; rx is the tick thread's clock. Advance so the
    // loop wakes, drains the inbound channel, and processes the response.
    in_tx
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(1000),
        })
        .unwrap();
    advance(&sim, 1);

    wait_until(Duration::from_secs(2), "pending cleared", || {
        cells.lock().unwrap()[0].keepalive.pending_len() == 0
    });
    assert!(
        cells.lock().unwrap()[0].disconnected.is_none(),
        "valid response must not disconnect"
    );
    // Latency updated (the unit tests pin the exact value; here it is >= 0 and
    // the pending was cleared by an Accepted verdict).
    assert!(
        cells.lock().unwrap()[0].keepalive.latency_ms() >= 0,
        "latency was updated by the accepted response"
    );

    let _ = loop_.handle;
}

/// An out-of-order response (matching a non-oldest challenge) disconnects with
/// TIMEOUT, exactly as Java's `handleKeepAlive` second loop.
#[test]
fn out_of_order_response_via_inbound_disconnects() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 45); // sends at 1000 and 2000
    wait_until(Duration::from_secs(2), "two sends", || {
        cells.lock().unwrap()[0].sends.len() >= 2
    });

    // Respond to the SECOND challenge (2000) first — it is pending but not oldest.
    in_tx
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(2000),
        })
        .unwrap();
    // Wake the loop so it drains the inbound channel and processes the response.
    advance(&sim, 1);

    wait_until(Duration::from_secs(2), "out-of-order disconnect", || {
        cells.lock().unwrap()[0].disconnected.is_some()
    });
    assert_eq!(
        cells.lock().unwrap()[0].disconnected,
        Some(DisconnectReason::Timeout)
    );

    let _ = loop_.handle;
}

/// A response matching no pending challenge disconnects with TIMEOUT ("without
/// matching challenge").
#[test]
fn unmatched_response_via_inbound_disconnects() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    in_tx
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(9999), // never sent
        })
        .unwrap();
    // Wake the loop so it drains the inbound channel and processes the response.
    advance(&sim, 1);

    wait_until(Duration::from_secs(2), "unmatched disconnect", || {
        cells.lock().unwrap()[0].disconnected.is_some()
    });
    assert_eq!(
        cells.lock().unwrap()[0].disconnected,
        Some(DisconnectReason::Timeout)
    );

    let _ = loop_.handle;
}

// ---- timeout ----------------------------------------------------------------

/// No response within 30s: the tick thread's keepalive tick disconnects with
/// `disconnect.timeout` (TIMEOUT). The strict `>` boundary is unit-tested;
/// here the full loop performs the kick.
#[test]
fn no_response_for_30s_disconnects_with_timeout() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    // A wide-enough outbound for the 32 one-per-second challenges (no prune).
    let (_in_tx, _out_rx) = register(&lifecycle_tx, 8, 64);

    advance(&sim, 640); // 32s
    wait_until(Duration::from_secs(2), "keepalive timeout", || {
        cells.lock().unwrap()[0].disconnected.is_some()
    });

    let cell = cells.lock().unwrap();
    assert_eq!(
        cell[0].disconnected,
        Some(DisconnectReason::Timeout),
        "kick reason is disconnect.timeout"
    );
    // The first challenge still went out at 1s before the kick.
    assert_eq!(cell[0].sends[0], 1000);

    let _ = loop_.handle;
}

/// A client that responds to every challenge survives past the 30s keepalive
/// window (the issue #157 DoD).
#[test]
fn responding_client_survives_past_30s_window() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, mut out_rx) = register(&lifecycle_tx, 16, 16);

    let mut last_answered = 0;
    for _ in 0..35 {
        advance(&sim, 20); // 1s
        // Drain the clientbound channel (a real client reads it), so the
        // outbound queue never backs up and prunes the connection.
        while let Ok(OutboundEvent::Packet { .. }) = out_rx.try_recv() {}
        // Answer any challenge sent this second (echo through the real inbound
        // channel; the tick thread handles it).
        let sends = cells.lock().unwrap()[0].sends.clone();
        for &id in &sends[last_answered..] {
            in_tx
                .try_send(ServerboundFrame {
                    bytes: keepalive_frame(id),
                })
                .unwrap();
        }
        last_answered = sends.len();
    }

    wait_until(Duration::from_secs(2), "sends to accumulate", || {
        cells.lock().unwrap()[0].sends.len() >= 30
    });
    assert!(
        cells.lock().unwrap()[0].disconnected.is_none(),
        "a responding client must never be kicked"
    );

    let _ = loop_.handle;
}

// ---- backpressure -----------------------------------------------------------

/// A full outbound channel (the bounded tick→network queue) prunes the
/// connection when a keepalive send overflows — the issue #93 backpressure
/// policy applies to keepalive frames exactly as to any outbound packet, and
/// it is not a server stop. The pruned connection's keepalive state stops
/// being driven; a healthy second connection is unaffected.
#[test]
fn outbound_overflow_prunes_connection_and_loop_survives() {
    // id 1: capacity-1 outbound that is never drained — the 2s send overflows
    // it and prunes the connection. id 2: healthy and drained.
    let cells = Arc::new(Mutex::new(vec![
        ConnCell {
            keepalive: KeepaliveState::new(0),
            ..ConnCell::default()
        },
        ConnCell {
            keepalive: KeepaliveState::new(0),
            ..ConnCell::default()
        },
    ]));
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (_in_tx1, mut out_rx1) = register_with_id(&lifecycle_tx, ConnectionId(1), 8, 1);
    let (_in_tx2, mut out_rx2) = register_with_id(&lifecycle_tx, ConnectionId(2), 8, 8);

    advance(&sim, 50); // 2500ms: sends at 1000 and 2000 for both connections
    wait_until(Duration::from_secs(2), "id1 pruned by overflow", || {
        while let Ok(OutboundEvent::Packet { .. }) = out_rx1.try_recv() {}
        out_rx1.try_recv().is_err() // channel closed => registry pruned it
    });

    assert_eq!(
        cells.lock().unwrap()[0].sends.len(),
        2,
        "id1 sent at 1s and 2s, then its channel overflowed and it was pruned"
    );
    assert!(
        !shutdown.is_requested(),
        "keepalive outbound overflow is not a server stop"
    );

    // id 2 (drained) keeps receiving keepalives; id 1's state is no longer
    // driven now that its connection left the registry.
    for _ in 0..5 {
        advance(&sim, 20); // 1s
        while let Ok(OutboundEvent::Packet { .. }) = out_rx2.try_recv() {}
    }
    wait_until(Duration::from_secs(2), "id2 keeps sending", || {
        cells.lock().unwrap()[1].sends.len() >= 5
    });
    assert_eq!(
        cells.lock().unwrap()[0].sends.len(),
        2,
        "a pruned connection's keepalive is never driven again"
    );

    let _ = loop_.handle;
}

// ---- registration ----------------------------------------------------------

/// A `LifecycleEvent::Disconnect` removes the connection from the registry and
/// its keepalive state is no longer ticked (no further challenges are sent).
#[test]
fn lifecycle_disconnect_stops_keepalive_drive() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (_in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // 1250ms: one send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // The client dropped the connection; the network side tells the tick thread.
    lifecycle_tx
        .try_send(LifecycleEvent::Disconnect {
            id: ConnectionId(1),
            reason: DisconnectReason::EndOfStream,
        })
        .expect("lifecycle channel has room");
    // Wake the loop (its idle sleep blocks until the sim advances) so it
    // applies the disconnect and publishes `connected == 0`.
    advance(&sim, 1);
    wait_until(
        Duration::from_secs(2),
        "connection removed from registry",
        || loop_.stats.connected.load(Ordering::Relaxed) == 0,
    );

    let sends_before = cells.lock().unwrap()[0].sends.len();
    advance(&sim, 60); // 3000ms — two keepalive intervals
    let sends_after = cells.lock().unwrap()[0].sends.len();
    assert_eq!(
        sends_after, sends_before,
        "a disconnected connection's keepalive state is no longer driven"
    );

    let _ = loop_.handle;
}

/// Two registered connections are driven independently: each gets its own
/// challenges, and a response on one never touches the other's pending queue.
#[test]
fn two_connections_each_drive_independent_keepalive() {
    let cells = Arc::new(Mutex::new(vec![
        ConnCell {
            keepalive: KeepaliveState::new(0),
            ..ConnCell::default()
        },
        ConnCell {
            keepalive: KeepaliveState::new(0),
            ..ConnCell::default()
        },
    ]));
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx1, _out_rx1) = register_with_id(&lifecycle_tx, ConnectionId(1), 8, 8);
    let (_in_tx2, _out_rx2) = register_with_id(&lifecycle_tx, ConnectionId(2), 8, 8);

    advance(&sim, 25); // 1250ms: both send at 1000ms
    wait_until(Duration::from_secs(2), "both first sends", || {
        let cells = cells.lock().unwrap();
        !cells[0].sends.is_empty() && !cells[1].sends.is_empty()
    });
    assert_eq!(cells.lock().unwrap()[0].sends, vec![1000]);
    assert_eq!(cells.lock().unwrap()[1].sends, vec![1000]);

    // A response on conn 1 clears only conn 1's pending.
    in_tx1
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(1000),
        })
        .unwrap();
    advance(&sim, 1);
    wait_until(Duration::from_secs(2), "conn1 pending cleared", || {
        cells.lock().unwrap()[0].keepalive.pending_len() == 0
    });
    assert_eq!(
        cells.lock().unwrap()[1].keepalive.pending_len(),
        1,
        "conn 2 is untouched by conn 1's response"
    );

    let _ = loop_.handle;
}

// ---- hostile responses -----------------------------------------------------

/// Replaying a challenge id that was already accepted is a "without matching
/// challenge" disconnect — Paper does not silently ignore duplicates.
#[test]
fn replayed_keepalive_response_disconnects() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // A valid response clears the challenge...
    in_tx
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(1000),
        })
        .unwrap();
    advance(&sim, 1);
    wait_until(Duration::from_secs(2), "accepted", || {
        cells.lock().unwrap()[0].keepalive.pending_len() == 0
    });
    assert!(cells.lock().unwrap()[0].disconnected.is_none());

    // ...and replaying that id is a "without matching challenge" disconnect.
    in_tx
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(1000),
        })
        .unwrap();
    advance(&sim, 1);
    wait_until(Duration::from_secs(2), "replay disconnect", || {
        cells.lock().unwrap()[0].disconnected.is_some()
    });
    assert_eq!(
        cells.lock().unwrap()[0].disconnected,
        Some(DisconnectReason::Timeout)
    );

    let _ = loop_.handle;
}

// ---- truncation / mutation -------------------------------------------------

/// A keepalive frame truncated mid-id must be skipped, not crash the tick
/// thread; the loop keeps serving the connection afterwards.
#[test]
fn truncated_keepalive_frame_does_not_crash_loop() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // A keepalive frame with only 3 of the 8 id bytes. The network decoder
    // would reject this before the tick thread in production; the tick-side
    // handler must skip it, not panic.
    let mut payload = Vec::with_capacity(1 + 3);
    var_int::write(&mut payload, KEEP_ALIVE_PACKET_ID);
    payload.extend_from_slice(&[0x00, 0x00, 0x00]);
    let truncated = Bytes::from(encode_frame(&payload).unwrap().to_vec());
    in_tx
        .try_send(ServerboundFrame { bytes: truncated })
        .unwrap();

    advance(&sim, 5);
    std::thread::sleep(Duration::from_millis(50));
    {
        let cell = cells.lock().unwrap();
        assert!(cell[0].disconnected.is_none(), "truncated frame is skipped");
        assert_eq!(
            cell[0].keepalive.pending_len(),
            1,
            "challenge still pending"
        );
    }

    // The loop is alive: a later valid response is still accepted.
    in_tx
        .try_send(ServerboundFrame {
            bytes: keepalive_frame(1000),
        })
        .unwrap();
    advance(&sim, 1);
    wait_until(
        Duration::from_secs(2),
        "valid response accepted after truncation",
        || cells.lock().unwrap()[0].keepalive.pending_len() == 0,
    );
    assert!(cells.lock().unwrap()[0].disconnected.is_none());

    let _ = loop_.handle;
}

/// A well-formed frame for a *different* packet must not be misrouted as a
/// keepalive response.
#[test]
fn non_keepalive_packet_frame_is_skipped() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // A frame with packet id 1 (e.g. chat) and an 8-byte body: the keepalive
    // handler ignores it entirely.
    let mut payload = Vec::with_capacity(1 + 8);
    var_int::write(&mut payload, 1); // not the keep_alive id
    payload.extend_from_slice(&1000_i64.to_be_bytes());
    let other = Bytes::from(encode_frame(&payload).unwrap().to_vec());
    in_tx.try_send(ServerboundFrame { bytes: other }).unwrap();

    advance(&sim, 5);
    std::thread::sleep(Duration::from_millis(50));
    let cell = cells.lock().unwrap();
    assert!(
        cell[0].disconnected.is_none(),
        "non-keepalive packet is ignored"
    );
    assert_eq!(
        cell[0].keepalive.pending_len(),
        1,
        "challenge still pending"
    );

    let _ = loop_.handle;
}

/// A keepalive frame whose VarInt21 length header disagrees with its body is
/// malformed and must be skipped.
#[test]
fn length_header_mismatch_is_skipped() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    let (in_tx, _out_rx) = register(&lifecycle_tx, 8, 8);

    advance(&sim, 25); // send at 1000ms
    wait_until(Duration::from_secs(2), "first send", || {
        !cells.lock().unwrap()[0].sends.is_empty()
    });

    // Frame claims 3 body bytes but carries 9 (packet id + 8-byte id).
    let mut payload = Vec::new();
    var_int::write(&mut payload, KEEP_ALIVE_PACKET_ID);
    payload.extend_from_slice(&1000_i64.to_be_bytes());
    let mut bad = Vec::new();
    var_int::write(&mut bad, 3); // wrong length header
    bad.extend_from_slice(&payload);
    in_tx
        .try_send(ServerboundFrame {
            bytes: Bytes::from(bad),
        })
        .unwrap();

    advance(&sim, 5);
    std::thread::sleep(Duration::from_millis(50));
    let cell = cells.lock().unwrap();
    assert!(
        cell[0].disconnected.is_none(),
        "mismatched length header is skipped"
    );
    assert_eq!(
        cell[0].keepalive.pending_len(),
        1,
        "challenge still pending"
    );

    let _ = loop_.handle;
}

// ---- simulated-time boundaries ---------------------------------------------

/// At exactly `KEEPALIVE_LIMIT` (30s) the strict `>` does NOT kick; one 50ms
/// tick past the limit it does. Pins the boundary on the live loop, where the
/// unit tests pin it on the pure machine.
#[test]
fn exact_30s_boundary_is_strict() {
    let cells = one_cell();
    let sim = Arc::new(SimTime::new());
    let shutdown = Arc::new(Shutdown::new());
    let (loop_, lifecycle_tx) = build_keepalive_loop(cells.clone(), sim.clone(), shutdown.clone());
    // 31 one-per-second challenges fit without pruning.
    let (_in_tx, _out_rx) = register(&lifecycle_tx, 8, 64);

    // The first challenge is sent at t=1s. Advance to t=31s exactly: it has
    // been pending exactly KEEPALIVE_LIMIT (30s) — strict `>` must not kick.
    advance(&sim, 620); // 31000ms
    wait_until(Duration::from_secs(2), "tick at 31s processed", || {
        loop_.stats.ticks.load(Ordering::Relaxed) >= 621
    });
    {
        let cell = cells.lock().unwrap();
        assert!(
            cell[0].disconnected.is_none(),
            "at exactly KEEPALIVE_LIMIT the strict `>` does not kick"
        );
        assert_eq!(
            cell[0].keepalive.pending_len(),
            31,
            "all challenges (1s..31s) are still pending"
        );
    }

    // One 50ms tick past the limit: 30.05s elapsed — the kick fires.
    advance(&sim, 1); // 31050ms
    wait_until(Duration::from_secs(2), "kick past the limit", || {
        cells.lock().unwrap()[0].disconnected.is_some()
    });
    assert_eq!(
        cells.lock().unwrap()[0].disconnected,
        Some(DisconnectReason::Timeout)
    );

    let _ = loop_.handle;
}

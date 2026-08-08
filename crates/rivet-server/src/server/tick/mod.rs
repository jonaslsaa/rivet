//! The sync tick thread — the "Server thread" (Paper `MinecraftServer.runServer`
//! → `tickServer`). One authoritative owner of mutable game state (OWNERSHIP:
//! "one owner, IDs everywhere else"); async network tasks exchange typed
//! commands/events with it over bounded channels and never hold game state.
//!
//! This slice (issue #93) delivers the *spine*: a deterministic 20 TPS loop with
//! Paper's monotonic schedule + catch-up cap, per-connection bounded
//! network⇄tick channel pairs keyed by `ConnectionId`, orderly startup/shutdown,
//! and overload/backpressure. Play-state packet *bodies* are epic #10; login
//! completion that routes frames here is #96. Until then the inbound/outbound
//! packet paths are infrastructure exercised by tests.

pub mod channels;
pub mod endpoint;
pub mod registry;
pub mod scheduler;
pub mod shutdown;
pub mod time;

pub use scheduler::{DEFAULT_CATCHUP_TICKS, NANOS_PER_TICK, TickScheduler};

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tokio::sync::mpsc;

use crate::server::network::connection_id::ConnectionId;
use crate::server::network::packet_listener::DisconnectReason;
use channels::{
    LifecycleEvent, MAX_INBOUND_BYTES_PER_TICK, MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN,
    MAX_INBOUND_FRAMES_PER_DRAIN, MAX_INBOUND_FRAMES_PER_TICK, ServerboundFrame,
};
use registry::{ConnectionRegistry, DrainOutcome};
use shutdown::Shutdown;
use time::TickTime;

/// A unit of tick work. `tickables` run on the tick thread each tick after the
/// lifecycle + inbound drain, with `&mut` access to the connection registry —
/// the OWNERSHIP boundary: mutable game state lives on the tick thread only.
pub type Tickable = Box<dyn FnMut(&mut TickContext) + Send>;

/// Per-tick context handed to tickables.
pub struct TickContext<'a> {
    /// The current tick number (`MinecraftServer.currentTick`, 1-based).
    pub tick: u64,
    /// The tick-side connection registry.
    pub connections: &'a mut ConnectionRegistry,
    /// Inbound frames drained from all connections this tick, in per-connection
    /// FIFO order. Play-state dispatch (epics #10/#101/#158) consumes these;
    /// this slice delivers them so the boundary is observable.
    pub inbound: Vec<(ConnectionId, ServerboundFrame)>,
}

/// Live counters published by the tick thread for observability and tests
/// (`Arc`-shared; the tick thread is the only writer).
#[derive(Debug, Default)]
pub struct TickStats {
    /// Number of ticks run (`MinecraftServer.tickCount`).
    pub ticks: AtomicU64,
    /// Connections currently registered on the tick side.
    pub connected: AtomicUsize,
}

/// The synchronous tick loop. Runs on its own OS thread; the `Shutdown` signal
/// ends it via an orderly drain.
pub struct ServerTickLoop {
    scheduler: TickScheduler,
    time: Arc<dyn TickTime>,
    shutdown: Arc<Shutdown>,
    registry: ConnectionRegistry,
    lifecycle_rx: mpsc::Receiver<LifecycleEvent>,
    tickables: Vec<Tickable>,
    stats: Arc<TickStats>,
}

impl ServerTickLoop {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scheduler: TickScheduler,
        time: Arc<dyn TickTime>,
        shutdown: Arc<Shutdown>,
        lifecycle_rx: mpsc::Receiver<LifecycleEvent>,
        tickables: Vec<Tickable>,
        stats: Arc<TickStats>,
    ) -> Self {
        ServerTickLoop {
            scheduler,
            time,
            shutdown,
            registry: ConnectionRegistry::new(),
            lifecycle_rx,
            tickables,
            stats,
        }
    }

    /// Run until shutdown, sleeping between ticks. On shutdown, one final
    /// drain-and-deliver pass runs so already-accepted events are observed,
    /// every live connection is told the server is stopping, and then dropping
    /// the registry drops every `out_tx` (the remaining network side closes).
    pub fn run(mut self) {
        while !self.shutdown.is_requested() {
            let now = self.time.now_nanos();
            if self.scheduler.start_tick(now) {
                self.run_tick();
            } else {
                self.time.sleep_until(self.scheduler.next_deadline_nanos());
            }
        }
        // Final pass: deliver anything already accepted into the channels.
        self.run_tick();
        // Tell every live connection the server is stopping (best-effort; a full
        // channel or dead connection is simply dropped).
        let ids: Vec<ConnectionId> = self.registry.ids().collect();
        for id in ids {
            let _ = self.registry.send(
                id,
                channels::OutboundEvent::Disconnect {
                    reason: DisconnectReason::ServerShutdown,
                },
            );
        }
        // Dropping `self` drops the registry, and with it every `out_tx` — any
        // connection whose channel still has room sees `recv() == None` and
        // closes its socket.
    }

    fn run_tick(&mut self) {
        let tick = self.scheduler.tick_count();

        // 1. Lifecycle: apply Connect/Disconnect events.
        while let Ok(event) = self.lifecycle_rx.try_recv() {
            self.registry.apply(event);
        }

        // 2. Inbound: drain every connection's channel (per-connection FIFO),
        // bounded by the per-connection budget (inside drain_one_bounded) and
        // the aggregate per-tick budget. Once the aggregate budget is exhausted
        // the remaining frames stay retained in the channels for a later tick.
        // The drain order is a rotating round-robin (`rotated_ids`): the start
        // advances past the connections given budget each tick, so the aggregate
        // cap cannot indefinitely starve the connections that sort last.
        let ids = self.registry.rotated_ids();
        let mut attempted = 0usize;
        let mut inbound = Vec::new();
        let mut aggregate_frames = 0usize;
        let mut aggregate_bytes = 0usize;
        for id in ids.iter().copied() {
            let remaining_frames = MAX_INBOUND_FRAMES_PER_TICK.saturating_sub(aggregate_frames);
            let remaining_bytes = MAX_INBOUND_BYTES_PER_TICK.saturating_sub(aggregate_bytes);
            if remaining_frames == 0 || remaining_bytes == 0 {
                break;
            }
            // Clamp the per-connection budget with whatever aggregate remains:
            // one connection can never consume more than its per-connection
            // allowance (1024 frames / 16 MiB) even when the aggregate budget is
            // untouched, because `drain_one_bounded` treats its arguments as the
            // authoritative caps.
            let cap_frames = remaining_frames.min(MAX_INBOUND_FRAMES_PER_DRAIN);
            let cap_bytes = remaining_bytes.min(MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN);
            match self.registry.drain_one_bounded(id, cap_frames, cap_bytes) {
                DrainOutcome::Drained(frames) => {
                    aggregate_frames += frames.len();
                    aggregate_bytes += frames.iter().map(|f| f.bytes.len()).sum::<usize>();
                    inbound.extend(frames.into_iter().map(|f| (id, f)));
                }
                DrainOutcome::Closed => {
                    tracing::debug!(%id, "tick registry pruned closed connection");
                }
            }
            attempted += 1;
        }
        // Fairness: the next tick's drain starts at the connection right after
        // the last one this tick got budget to attempt (wrapping to the head
        // when everyone was attempted), so the same connections cannot always
        // consume the aggregate budget first.
        if let Some(next) = ids.get(attempted).copied().or_else(|| ids.first().copied()) {
            self.registry.advance_drain_cursor(next);
        }

        // 3. Tickables, with disjoint mutable borrows of registry + tickables.
        let ServerTickLoop {
            registry,
            tickables,
            ..
        } = &mut *self;
        let mut ctx = TickContext {
            tick,
            connections: registry,
            inbound,
        };
        for tickable in tickables {
            tickable(&mut ctx);
        }

        self.stats.ticks.store(tick, Ordering::Relaxed);
        self.stats
            .connected
            .store(self.registry.len(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use channels::{InboundDrained, ServerboundFrame};
    use scheduler::TickScheduler;
    use shutdown::Shutdown;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::sync::Mutex;
    use time::SimTime;

    const REMOTE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);

    /// The recorded-delivery log shared with a tickable: `(connection, bytes)`.
    type Recorded = Arc<Mutex<Vec<(ConnectionId, Vec<u8>)>>>;

    fn frame(byte: u8) -> ServerboundFrame {
        ServerboundFrame {
            bytes: Bytes::from(vec![byte]),
        }
    }

    /// Build a `ServerTickLoop` whose tickable records every delivered inbound
    /// frame (the production-path observation: `run_tick` → `TickContext.inbound`).
    fn test_loop(
        recorded: Arc<Mutex<Vec<Vec<u8>>>>,
    ) -> (ServerTickLoop, tokio::sync::mpsc::Sender<LifecycleEvent>) {
        let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(64);
        let shutdown = Arc::new(Shutdown::new());
        let sim = Arc::new(SimTime::with_shutdown(Arc::clone(&shutdown)));
        let time: Arc<dyn time::TickTime> = sim;
        let scheduler = TickScheduler::new(NANOS_PER_TICK, 5, 0);
        let recorded_c = Arc::clone(&recorded);
        let tickables: Vec<Tickable> = vec![Box::new(move |ctx| {
            for (_id, frame) in &ctx.inbound {
                recorded_c.lock().unwrap().push(frame.bytes.to_vec());
            }
        })];
        let loop_ = ServerTickLoop::new(
            scheduler,
            time,
            shutdown,
            lifecycle_rx,
            tickables,
            Arc::new(TickStats::default()),
        );
        (loop_, lifecycle_tx)
    }

    /// Like [`test_loop`], but records `(ConnectionId, bytes)` so multi-connection
    /// tests can assert per-connection distribution and FIFO order.
    fn test_loop_record_ids(
        recorded: Recorded,
    ) -> (ServerTickLoop, tokio::sync::mpsc::Sender<LifecycleEvent>) {
        let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(64);
        let shutdown = Arc::new(Shutdown::new());
        let sim = Arc::new(SimTime::with_shutdown(Arc::clone(&shutdown)));
        let time: Arc<dyn time::TickTime> = sim;
        let scheduler = TickScheduler::new(NANOS_PER_TICK, 5, 0);
        let recorded_c = Arc::clone(&recorded);
        let tickables: Vec<Tickable> = vec![Box::new(move |ctx| {
            for (id, frame) in &ctx.inbound {
                recorded_c.lock().unwrap().push((*id, frame.bytes.to_vec()));
            }
        })];
        let loop_ = ServerTickLoop::new(
            scheduler,
            time,
            shutdown,
            lifecycle_rx,
            tickables,
            Arc::new(TickStats::default()),
        );
        (loop_, lifecycle_tx)
    }

    /// Like [`test_loop`], but sums delivered bytes instead of copying them —
    /// the aggregate byte-budget test deals in 8 MiB frames, so it avoids the
    /// extra copy of recording every frame.
    fn test_loop_sum_bytes(
        sum: Arc<std::sync::atomic::AtomicUsize>,
    ) -> (ServerTickLoop, tokio::sync::mpsc::Sender<LifecycleEvent>) {
        let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(64);
        let shutdown = Arc::new(Shutdown::new());
        let sim = Arc::new(SimTime::with_shutdown(Arc::clone(&shutdown)));
        let time: Arc<dyn time::TickTime> = sim;
        let scheduler = TickScheduler::new(NANOS_PER_TICK, 5, 0);
        let sum_c = Arc::clone(&sum);
        let tickables: Vec<Tickable> = vec![Box::new(move |ctx| {
            for (_id, frame) in &ctx.inbound {
                sum_c.fetch_add(frame.bytes.len(), std::sync::atomic::Ordering::Relaxed);
            }
        })];
        let loop_ = ServerTickLoop::new(
            scheduler,
            time,
            shutdown,
            lifecycle_rx,
            tickables,
            Arc::new(TickStats::default()),
        );
        (loop_, lifecycle_tx)
    }

    /// A frame whose bytes encode `(id, seq)` so tests can verify per-connection
    /// FIFO order across an aggregate drain (the byte budget is far away).
    fn frame_seq(id: u8, seq: usize) -> ServerboundFrame {
        ServerboundFrame {
            bytes: Bytes::from(vec![id, (seq & 0xFF) as u8, ((seq >> 8) & 0xFF) as u8]),
        }
    }

    /// Group delivered frames per connection, preserving delivery order within
    /// each connection (the recorded log is in drain order, which is
    /// per-connection FIFO interleaved across connections).
    fn group_by_connection(
        recorded: &[(ConnectionId, Vec<u8>)],
    ) -> std::collections::HashMap<ConnectionId, Vec<Vec<u8>>> {
        let mut map: std::collections::HashMap<ConnectionId, Vec<Vec<u8>>> =
            std::collections::HashMap::new();
        for (id, bytes) in recorded {
            map.entry(*id).or_default().push(bytes.clone());
        }
        map
    }

    /// Assert that one connection's recorded frames arrived in the exact send
    /// order (each frame carries its sequence in bytes 1..3).
    fn assert_fifo(id: ConnectionId, frames: &[Vec<u8>]) {
        for (k, f) in frames.iter().enumerate() {
            assert_eq!(f[0], id.0 as u8, "connection {id} frames all belong to it");
            let seq = (f[1] as usize) | ((f[2] as usize) << 8);
            assert_eq!(seq, k, "connection {id} frames arrive in FIFO order");
        }
    }

    /// Production-path regression: even with the aggregate budget untouched
    /// (only one connection), a single connection never consumes more than its
    /// per-connection allowance in one tick, and the retained remainder is
    /// delivered on a later tick.
    #[test]
    fn run_tick_clamps_to_per_connection_budget() {
        let recorded = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let (mut loop_, lifecycle_tx) = test_loop(Arc::clone(&recorded));

        // One connection prefilled with more than the per-connection frame cap
        // (1024) but far below the aggregate cap (8192). The channel capacity is
        // above the cap so the cap, not the channel depth, is binding.
        let (in_tx, in_rx) = tokio::sync::mpsc::channel(8192);
        let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
        let id = ConnectionId(1);
        lifecycle_tx
            .try_send(LifecycleEvent::Connect {
                id,
                remote: REMOTE,
                in_rx,
                out_tx,
                drained: InboundDrained::new(),
            })
            .unwrap();
        for _ in 0..(MAX_INBOUND_FRAMES_PER_DRAIN + 76) {
            in_tx.try_send(frame(0)).unwrap();
        }

        // Tick 1: exactly the per-connection allowance (1024), not the aggregate.
        loop_.run_tick();
        assert_eq!(
            recorded.lock().unwrap().len(),
            MAX_INBOUND_FRAMES_PER_DRAIN,
            "one connection must not exceed its per-connection allowance"
        );

        // Tick 2: the retained remainder is delivered.
        loop_.run_tick();
        assert_eq!(
            recorded.lock().unwrap().len(),
            MAX_INBOUND_FRAMES_PER_DRAIN + 76,
            "the retained remainder is delivered on a later tick"
        );
    }

    /// The same production-path clamp applies to the byte budget: one connection
    /// prefilled with 3 × 8 MiB frames delivers 16 MiB (the per-connection byte
    /// allowance) on the first tick and the retained third frame later.
    #[test]
    fn run_tick_clamps_to_per_connection_byte_budget() {
        let recorded = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let (mut loop_, lifecycle_tx) = test_loop(Arc::clone(&recorded));

        let (in_tx, in_rx) = tokio::sync::mpsc::channel(64);
        let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
        let id = ConnectionId(1);
        lifecycle_tx
            .try_send(LifecycleEvent::Connect {
                id,
                remote: REMOTE,
                in_rx,
                out_tx,
                drained: InboundDrained::new(),
            })
            .unwrap();
        for _ in 0..3 {
            in_tx
                .try_send(ServerboundFrame {
                    bytes: Bytes::from(vec![0x42u8; 8 * 1024 * 1024]),
                })
                .unwrap();
        }

        loop_.run_tick();
        let first: usize = recorded.lock().unwrap().iter().map(|f| f.len()).sum();
        assert_eq!(
            first,
            2 * 8 * 1024 * 1024,
            "16 MiB per-connection byte allowance"
        );

        loop_.run_tick();
        let total: usize = recorded.lock().unwrap().iter().map(|f| f.len()).sum();
        assert_eq!(
            total,
            3 * 8 * 1024 * 1024,
            "retained third frame delivered later"
        );
    }

    /// Production-path aggregate frame budget: 9 flooding connections x 1024
    /// frames each = 9216 frames. The aggregate budget (8192 = 8 x 1024) divides
    /// evenly here, so tick 1 delivers exactly 8 connections at their per-
    /// connection cap and the 9th is not touched (the aggregate loop breaks at
    /// remaining == 0 before it). Its frames are delivered — in FIFO order — on
    /// tick 2. In general a connection whose remaining aggregate budget is less
    /// than its per-connection cap IS partially drained, with the excess retained
    /// FIFO for a later drain — that is intentional, not a boundary violation.
    #[test]
    fn run_tick_aggregate_frame_cap_across_connections() {
        let recorded = Arc::new(Mutex::new(Vec::<(ConnectionId, Vec<u8>)>::new()));
        let (mut loop_, lifecycle_tx) = test_loop_record_ids(Arc::clone(&recorded));

        let mut senders = Vec::new();
        for i in 0..9u64 {
            let (in_tx, in_rx) = tokio::sync::mpsc::channel(8192);
            let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
            let id = ConnectionId(i + 1);
            lifecycle_tx
                .try_send(LifecycleEvent::Connect {
                    id,
                    remote: REMOTE,
                    in_rx,
                    out_tx,
                    drained: InboundDrained::new(),
                })
                .unwrap();
            for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
                in_tx.try_send(frame_seq((i + 1) as u8, seq)).unwrap();
            }
            senders.push(in_tx);
        }

        loop_.run_tick();
        let guard = recorded.lock().unwrap();
        let first = group_by_connection(&guard);
        let counts: std::collections::HashMap<ConnectionId, usize> = first
            .iter()
            .map(|(id, frames)| (*id, frames.len()))
            .collect();
        assert_eq!(
            counts.values().sum::<usize>(),
            MAX_INBOUND_FRAMES_PER_TICK,
            "tick 1 delivers exactly the aggregate frame budget"
        );
        // Because 8192 divides evenly by 1024, exactly 8 connections drain at
        // their full per-connection cap and the 9th is untouched (the aggregate
        // budget is exhausted between connections). A remainder-limited
        // connection would instead be intentionally partial, FIFO-retained.
        let fully_served = counts
            .values()
            .filter(|&&n| n == MAX_INBOUND_FRAMES_PER_DRAIN)
            .count();
        let partially = counts
            .values()
            .filter(|&&n| n != 0 && n != MAX_INBOUND_FRAMES_PER_DRAIN)
            .count();
        assert_eq!(fully_served, 8, "8 connections drained to their cap");
        assert_eq!(partially, 0, "the exact division leaves no partial drain");
        for (id, frames) in &first {
            assert_fifo(*id, frames);
        }
        drop(guard);

        loop_.run_tick();
        let guard = recorded.lock().unwrap();
        let second = group_by_connection(&guard);
        assert_eq!(
            second.values().map(|f| f.len()).sum::<usize>(),
            MAX_INBOUND_FRAMES_PER_TICK + MAX_INBOUND_FRAMES_PER_DRAIN,
            "the retained connection is delivered on a later tick"
        );
        assert_eq!(second.len(), 9, "all 9 connections delivered by tick 2");
        for (id, frames) in &second {
            assert_fifo(*id, frames);
        }
    }

    /// Production-path aggregate byte budget: 9 connections x 2 x 8 MiB frames =
    /// 144 MiB total. The aggregate byte budget (128 MiB = 8 x 16 MiB) binds
    /// across connections: 8 connections deliver their full 16 MiB on tick 1,
    /// the 9th is retained and delivered later.
    #[test]
    fn run_tick_aggregate_byte_cap_across_connections() {
        let sum = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (mut loop_, lifecycle_tx) = test_loop_sum_bytes(Arc::clone(&sum));

        let mut senders = Vec::new();
        for i in 0..9u64 {
            let (in_tx, in_rx) = tokio::sync::mpsc::channel(64);
            let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
            let id = ConnectionId(i + 1);
            lifecycle_tx
                .try_send(LifecycleEvent::Connect {
                    id,
                    remote: REMOTE,
                    in_rx,
                    out_tx,
                    drained: InboundDrained::new(),
                })
                .unwrap();
            for _ in 0..2 {
                in_tx
                    .try_send(ServerboundFrame {
                        bytes: Bytes::from(vec![0x42u8; 8 * 1024 * 1024]),
                    })
                    .unwrap();
            }
            senders.push(in_tx);
        }

        loop_.run_tick();
        assert_eq!(
            sum.load(std::sync::atomic::Ordering::Relaxed),
            MAX_INBOUND_BYTES_PER_TICK,
            "tick 1 delivers exactly the aggregate byte budget"
        );

        loop_.run_tick();
        assert_eq!(
            sum.load(std::sync::atomic::Ordering::Relaxed),
            MAX_INBOUND_BYTES_PER_TICK + MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN,
            "the retained connection's bytes are delivered on a later tick"
        );
    }

    /// Per-tick per-connection delivered-frame counts, derived by diffing the
    /// cumulative recorded log against its length before this tick.
    fn per_tick_counts(
        recorded: &[(ConnectionId, Vec<u8>)],
        prev_len: usize,
    ) -> std::collections::HashMap<ConnectionId, usize> {
        let mut counts: std::collections::HashMap<ConnectionId, usize> =
            std::collections::HashMap::new();
        for (id, _) in &recorded[prev_len..] {
            *counts.entry(*id).or_default() += 1;
        }
        counts
    }

    /// Per-tick per-connection delivered-byte counts, derived by diffing the
    /// cumulative recorded log against its length before this tick.
    fn per_tick_bytes(
        recorded: &[(ConnectionId, Vec<u8>)],
        prev_len: usize,
    ) -> std::collections::HashMap<ConnectionId, usize> {
        let mut counts: std::collections::HashMap<ConnectionId, usize> =
            std::collections::HashMap::new();
        for (id, bytes) in &recorded[prev_len..] {
            *counts.entry(*id).or_default() += bytes.len();
        }
        counts
    }

    /// Fairness regression: 9 continuously-flooding connections share the
    /// aggregate frame budget (8 × the per-connection allowance) exactly, so a
    /// fixed drain order would serve the same 8 connections every tick and the
    /// 9th would starve forever. The rotating round-robin start serves every
    /// connection within `len` ticks, skipping a different connection each tick
    /// (9, 8, 7, ..., 1) so the tail always gets its turn.
    #[test]
    fn run_tick_rotating_drain_serves_all_within_len_ticks() {
        const N: u64 = 9;
        let recorded = Arc::new(Mutex::new(Vec::<(ConnectionId, Vec<u8>)>::new()));
        let (mut loop_, lifecycle_tx) = test_loop_record_ids(Arc::clone(&recorded));

        let mut senders = Vec::new();
        for i in 0..N {
            let (in_tx, in_rx) = tokio::sync::mpsc::channel(4096);
            let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
            let id = ConnectionId(i + 1);
            lifecycle_tx
                .try_send(LifecycleEvent::Connect {
                    id,
                    remote: REMOTE,
                    in_rx,
                    out_tx,
                    drained: InboundDrained::new(),
                })
                .unwrap();
            for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
                in_tx.try_send(frame_seq((i + 1) as u8, seq)).unwrap();
            }
            senders.push(in_tx);
        }

        let mut prev_len = 0usize;
        for tick in 1..=N as usize {
            loop_.run_tick();
            let guard = recorded.lock().unwrap();
            let counts = per_tick_counts(&guard, prev_len);
            prev_len = guard.len();
            drop(guard);

            assert_eq!(
                counts.values().sum::<usize>(),
                MAX_INBOUND_FRAMES_PER_TICK,
                "each tick delivers exactly the aggregate frame budget"
            );
            // The skipped connection rotates: N+1-tick (9, 8, ..., 1). The
            // other 8 connections serve at their full per-connection cap.
            let skipped = ConnectionId((N as usize + 1 - tick) as u64);
            for i in 1..=N as usize {
                let id = ConnectionId(i as u64);
                let expected = if id == skipped {
                    0
                } else {
                    MAX_INBOUND_FRAMES_PER_DRAIN
                };
                assert_eq!(
                    counts.get(&id).copied().unwrap_or(0),
                    expected,
                    "tick {tick}: connection {id} must serve at its cap (or be the rotated skip)"
                );
            }

            // Sustain the flood: refill every channel to the per-connection cap
            // (a full channel is already at the cap; try_send fails harmlessly).
            for (i, tx) in senders.iter().enumerate() {
                for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
                    let _ = tx.try_send(frame_seq((i + 1) as u8, seq));
                }
            }
        }
    }

    /// The byte half of the same fairness bound: 9 flooding connections each
    /// carrying 2 × 8 MiB (16 MiB, the per-connection byte allowance), sustained.
    /// The aggregate byte budget (8 × 16 MiB) is exhausted by 8 connections per
    /// tick; the rotating start still serves every connection within `len` ticks,
    /// so a byte-flooding tail connection survives instead of starving forever.
    #[test]
    fn run_tick_rotating_drain_serves_all_byte_flood() {
        const N: u64 = 9;
        const FRAME: usize = 8 * 1024 * 1024;
        let recorded = Arc::new(Mutex::new(Vec::<(ConnectionId, Vec<u8>)>::new()));
        let (mut loop_, lifecycle_tx) = test_loop_record_ids(Arc::clone(&recorded));

        let mut senders = Vec::new();
        for i in 0..N {
            let (in_tx, in_rx) = tokio::sync::mpsc::channel(64);
            let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
            let id = ConnectionId(i + 1);
            lifecycle_tx
                .try_send(LifecycleEvent::Connect {
                    id,
                    remote: REMOTE,
                    in_rx,
                    out_tx,
                    drained: InboundDrained::new(),
                })
                .unwrap();
            for _ in 0..2 {
                in_tx
                    .try_send(ServerboundFrame {
                        bytes: Bytes::from(vec![0x42u8; FRAME]),
                    })
                    .unwrap();
            }
            senders.push(in_tx);
        }

        let mut prev_len = 0usize;
        for tick in 1..=N as usize {
            loop_.run_tick();
            let guard = recorded.lock().unwrap();
            let bytes = per_tick_bytes(&guard, prev_len);
            prev_len = guard.len();
            drop(guard);

            assert_eq!(
                bytes.values().sum::<usize>(),
                MAX_INBOUND_BYTES_PER_TICK,
                "each tick delivers exactly the aggregate byte budget"
            );
            let skipped = ConnectionId((N as usize + 1 - tick) as u64);
            for i in 1..=N as usize {
                let id = ConnectionId(i as u64);
                let expected = if id == skipped { 0 } else { 2 * FRAME };
                assert_eq!(
                    bytes.get(&id).copied().unwrap_or(0),
                    expected,
                    "tick {tick}: connection {id} must deliver its byte allowance (or be the rotated skip)"
                );
            }
            for tx in &senders {
                for _ in 0..2 {
                    let _ = tx.try_send(ServerboundFrame {
                        bytes: Bytes::from(vec![0x42u8; FRAME]),
                    });
                }
            }
        }
    }

    /// Cursor-wrap fairness: a connection that joins late (the highest id, so it
    /// sorts last) is served promptly because the drain start rotates. The 8
    /// established connections keep the aggregate budget exactly exhausted each
    /// tick (each drains at its per-connection cap, so the cursor wraps back to
    /// the head every pass), then the 9th registers. A fixed drain order would
    /// serve connections 1-8 forever and starve the newcomer; the rotation moves
    /// it to the head of the very next pass, so it is served within 2 ticks of
    /// joining.
    #[test]
    fn run_tick_rotating_cursor_wrap_serves_late_joiner() {
        let recorded = Arc::new(Mutex::new(Vec::<(ConnectionId, Vec<u8>)>::new()));
        let (mut loop_, lifecycle_tx) = test_loop_record_ids(Arc::clone(&recorded));

        let mut senders = Vec::new();
        for i in 0..8u64 {
            let (in_tx, in_rx) = tokio::sync::mpsc::channel(4096);
            let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
            let id = ConnectionId(i + 1);
            lifecycle_tx
                .try_send(LifecycleEvent::Connect {
                    id,
                    remote: REMOTE,
                    in_rx,
                    out_tx,
                    drained: InboundDrained::new(),
                })
                .unwrap();
            for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
                in_tx.try_send(frame_seq((i + 1) as u8, seq)).unwrap();
            }
            senders.push(in_tx);
        }

        // Two ticks where the 8 connections exhaust the budget exactly (each at
        // its cap; the cursor wraps back to the head each pass). The established
        // connections refill to the cap every tick so the budget stays consumed.
        let mut prev_len = 0usize;
        for _ in 0..2 {
            loop_.run_tick();
            assert_eq!(
                recorded.lock().unwrap().len() - prev_len,
                MAX_INBOUND_FRAMES_PER_TICK,
                "8 connections exhaust the aggregate budget at the exact boundary"
            );
            prev_len = recorded.lock().unwrap().len();
            for (i, tx) in senders.iter().enumerate() {
                for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
                    let _ = tx.try_send(frame_seq((i + 1) as u8, seq));
                }
            }
        }

        // The 9th (highest-id) connection joins while the others keep flooding.
        let (late_tx, in_rx) = tokio::sync::mpsc::channel(4096);
        let (out_tx, _out_rx) = tokio::sync::mpsc::channel(64);
        let late = ConnectionId(9);
        lifecycle_tx
            .try_send(LifecycleEvent::Connect {
                id: late,
                remote: REMOTE,
                in_rx,
                out_tx,
                drained: InboundDrained::new(),
            })
            .unwrap();
        for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
            late_tx.try_send(frame_seq(9, seq)).unwrap();
        }
        senders.push(late_tx);

        // A helper that refills every connection to the per-connection cap each
        // tick so the aggregate budget stays exhausted every pass (a fixed order
        // would then starve the newcomer forever).
        let refill_all = || {
            for (i, tx) in senders.iter().enumerate() {
                for seq in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
                    let _ = tx.try_send(frame_seq((i + 1) as u8, seq));
                }
            }
        };

        // Pass A: the rotation still starts at the head (the 8-connection passes
        // wrapped the cursor to id 1), so the newcomer sorts last and is skipped.
        loop_.run_tick();
        let pass_a = {
            let guard = recorded.lock().unwrap();
            let c = per_tick_counts(&guard, prev_len);
            prev_len = guard.len();
            c
        };
        assert_eq!(
            pass_a.get(&late).copied().unwrap_or(0),
            0,
            "the late joiner sorts last on the pass whose rotation started at the head"
        );
        refill_all();
        // Pass B: the cursor advanced past the newcomer, so the rotation starts
        // at it and it is served at the head of the aggregate budget — even
        // though the established connections refilled and the budget is again
        // fully exhausted (a fixed order would skip it forever).
        loop_.run_tick();
        let guard = recorded.lock().unwrap();
        let served = guard[prev_len..]
            .iter()
            .filter(|(id, _)| *id == late)
            .count();
        assert!(
            served > 0,
            "the late-joining connection must be served on the pass after the one that skipped it"
        );
        assert!(
            served <= MAX_INBOUND_FRAMES_PER_DRAIN,
            "the late-joining connection serves at most its per-connection cap"
        );
        drop(guard);
    }
}

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
    LifecycleEvent, MAX_INBOUND_BYTES_PER_TICK, MAX_INBOUND_FRAMES_PER_TICK, ServerboundFrame,
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
        let ids: Vec<ConnectionId> = self.registry.ids().collect();
        let mut inbound = Vec::new();
        let mut aggregate_frames = 0usize;
        let mut aggregate_bytes = 0usize;
        for id in ids {
            let remaining_frames = MAX_INBOUND_FRAMES_PER_TICK.saturating_sub(aggregate_frames);
            let remaining_bytes = MAX_INBOUND_BYTES_PER_TICK.saturating_sub(aggregate_bytes);
            if remaining_frames == 0 || remaining_bytes == 0 {
                break;
            }
            match self
                .registry
                .drain_one_bounded(id, remaining_frames, remaining_bytes)
            {
                DrainOutcome::Drained(frames) => {
                    aggregate_frames += frames.len();
                    aggregate_bytes += frames.iter().map(|f| f.bytes.len()).sum::<usize>();
                    inbound.extend(frames.into_iter().map(|f| (id, f)));
                }
                DrainOutcome::Closed => {
                    tracing::debug!(%id, "tick registry pruned closed connection");
                }
            }
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

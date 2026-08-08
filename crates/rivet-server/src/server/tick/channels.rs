//! The typed events crossing the network⇄tick boundary, and the per-connection
//! channel ends the tick thread owns.
//!
//! OWNERSHIP §Network: handshake/status/login run on the tokio side; play-state
//! packets cross to the tick thread over bounded channels keyed by
//! `ConnectionId`. This slice (issue #93) builds the boundary — ordering,
//! capacity, overload — without play-state packet *bodies* (epic #10). The
//! inbound packet path stays empty in production until login completion (#96)
//! starts routing frames here; both directions are exercised by tests.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use rivet_protocol::protocol::common::client_information::ClientInformation;
use rivet_registry::core::GameProfile;
use tokio::sync::mpsc;

use crate::server::network::connection_id::ConnectionId;
use crate::server::network::packet_listener::DisconnectReason;

/// Slice-local inbound drain budget, frame-count half: the maximum number of
/// frames one inbound drain may deliver to the tick thread per tick before the
/// connection is considered to be flooding.
///
/// This is the *authoritative* per-tick bound, enforced on the tick side by
/// [`ConnectionRegistry::drain_one`](super::registry::ConnectionRegistry::drain_one):
/// one tick never delivers more than this many frames from one connection. It is
/// also the admission cap enforced on the tokio side by
/// [`Connection::forward_play`](crate::server::network::connection::Connection::forward_play)
/// between drains. Both sides use the same number so a hostile client cannot
/// make one tick drain beyond it by racing the sender window against the drain
/// (the window resets on observed drain progress, which a concurrently draining
/// tick can allow mid-drain).
///
/// No Java analog: Paper's netty inbound pipeline has no per-tick frame ceiling,
/// but it also never funnels frames into a fixed-depth bounded channel the way
/// this slice does, so the bound is the Rust-side analog of the existing
/// `MAXIMUM_UNCOMPRESSED_LENGTH` safety cap.
///
/// Disconnecting a client that exceeds the budget (see
/// [`Connection::forward_play`](crate::server::network::connection::Connection::forward_play))
/// is deliberate anti-flood policy, not TCP backpressure: the budget exists to
/// stop decompressed-frame memory amplification in the bounded channel, so it
/// kicks in before socket-level backpressure would.
pub const MAX_INBOUND_FRAMES_PER_DRAIN: usize = 1024;

/// Slice-local inbound drain budget, decompressed-bytes half: the maximum
/// cumulative decompressed bytes one inbound drain may deliver to the tick
/// thread per tick before the connection is considered to be flooding.
///
/// Each compressed frame can decompress to up to
/// `rivet_protocol::compression_decoder::MAXIMUM_UNCOMPRESSED_LENGTH` (8 MiB).
/// Without this bound a single drain could deliver multi-GiB into a 1024-deep
/// bounded channel (8 GiB of 8 MiB frames) — the compressed-frame memory
/// amplification this budget closes. Like [`MAX_INBOUND_FRAMES_PER_DRAIN`], it
/// is enforced authoritatively on the tick side by `drain_one` (which checks
/// before receiving, so per-tick delivery never exceeds the budget) and as an
/// admission cap by `Connection::forward_play` (which may overshoot by one
/// frame before it trips — a memory-retention backstop, not the per-tick bound).
pub const MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN: usize = 16 * 1024 * 1024;

/// Slice-local *aggregate* inbound budget, frame-count half: the maximum total
/// frames the tick thread delivers across *all* connections in one tick.
///
/// The per-connection budget ([`MAX_INBOUND_FRAMES_PER_DRAIN`]) bounds a single
/// connection; the aggregate bound exists because N flooding connections could
/// otherwise deliver N × per-connection work in one tick. It is a simple
/// aggregate cap, not a fair-share scheduler: once the budget is exhausted the
/// tick stops draining (the excess stays retained in the channels for a later
/// tick), and the per-connection budget is never exceeded regardless. A fair
/// round-robin across connections is deferred (recorded in #93/#96).
pub const MAX_INBOUND_FRAMES_PER_TICK: usize = 8 * MAX_INBOUND_FRAMES_PER_DRAIN;

/// Slice-local *aggregate* inbound budget, decompressed-bytes half: the maximum
/// total decompressed bytes the tick thread delivers across *all* connections in
/// one tick. See [`MAX_INBOUND_FRAMES_PER_TICK`].
pub const MAX_INBOUND_BYTES_PER_TICK: usize = 8 * MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN;

/// A decoded inbound play-state packet handed to the tick thread. The packet
/// *body* is owned by epic #10 (protocol packet bodies); this slice carries the
/// raw encoded frame so the ordering/backpressure boundary is real.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerboundFrame {
    pub bytes: Bytes,
}

/// A per-connection cumulative counter of inbound frames the tick thread has
/// drained from the connection's channel. The tick side increments it for each
/// frame delivered by
/// [`ConnectionRegistry::drain_one_bounded`](super::registry::ConnectionRegistry::drain_one_bounded);
/// the connection side reads it in
/// [`Connection::forward_play`](crate::server::network::connection::Connection::forward_play)
/// as the authoritative "the tick is keeping up" signal for its admission window.
///
/// The counter is monotonic, per connection, and shared as an `Arc` between the
/// tick thread (the only writer) and the connection task (the only reader) — an
/// atomics-only coordination signal, not game state (OWNERSHIP). Because it
/// counts every drained frame, it detects tick progress exactly even when the
/// tick drains in bursts (a transient channel-capacity snapshot cannot).
#[derive(Clone, Debug, Default)]
pub struct InboundDrained {
    count: Arc<AtomicUsize>,
}

impl InboundDrained {
    pub fn new() -> Self {
        Self::default()
    }

    /// The tick side: record `n` frames drained (delivered) from the channel.
    pub(crate) fn record_drained(&self, n: usize) {
        self.count.fetch_add(n, Ordering::Release);
    }

    /// The connection side: total frames the tick has drained so far.
    pub fn drained(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }
}

/// A command from the tick thread to the network side of one connection. Every
/// `Packet` precedes any `Disconnect` in the channel (mirrors Paper's
/// `send(disconnect, thenRun(disconnect))` ordering): the per-connection task
/// flushes queued frames before closing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundEvent {
    /// A fully-encoded play-state frame (VarInt21 length header + packet).
    Packet { frame: Bytes },
    /// The tick thread asks the network side to close this connection.
    Disconnect { reason: DisconnectReason },
}

/// A lifecycle/registration event from the network side to the tick thread.
/// `Connect` hands ownership of the tick-side channel ends over to the tick
/// thread — the OWNERSHIP boundary: the tick thread becomes the sole owner of
/// the inbound receiver and outbound sender for that connection.
#[derive(Debug)]
pub enum LifecycleEvent {
    Connect {
        id: ConnectionId,
        remote: SocketAddr,
        in_rx: mpsc::Receiver<ServerboundFrame>,
        out_tx: mpsc::Sender<OutboundEvent>,
        /// The per-connection drained-frame counter shared with the connection's
        /// admission window (see [`InboundDrained`]).
        drained: InboundDrained,
    },
    Disconnect {
        id: ConnectionId,
        reason: DisconnectReason,
    },
    /// The configuration→play handoff (issue #101 Slice B): the authenticated
    /// profile + `ClientInformation` a connection carried across the finish
    /// configuration boundary. Sent by the network side when the configuration
    /// listener hands the connection to the play state, so the tick thread can
    /// spawn the join burst. It travels over the lifecycle channel (drained
    /// before the inbound channel each tick) so the tick applies the handoff
    /// before it sees the first coalesced play frame.
    EnterPlay {
        id: ConnectionId,
        profile: GameProfile,
        client_information: ClientInformation,
    },
}

/// The tick thread's per-connection channel ends (OWNERSHIP §Network "packet
/// in/out queues per player", keyed by `ConnectionId`). Stored in the tick-side
/// [`ConnectionRegistry`](super::registry::ConnectionRegistry).
#[derive(Debug)]
pub struct ConnChannels {
    id: ConnectionId,
    remote: SocketAddr,
    pub(crate) in_rx: mpsc::Receiver<ServerboundFrame>,
    pub(crate) out_tx: mpsc::Sender<OutboundEvent>,
    /// Cumulative frames the tick has drained from `in_rx`, shared with the
    /// connection's admission window (see [`InboundDrained`]).
    pub(crate) drained: InboundDrained,
    /// A frame dequeued but not delivered because it would cross the strict
    /// per-tick byte budget (see `ConnectionRegistry::drain_one_bounded`).
    /// Preserved in FIFO order ahead of the channel and delivered by the next
    /// drain — deterministic retention, never dropped.
    pub(crate) pending_frame: Option<ServerboundFrame>,
    /// The configuration→play handoff payload, present once the network side
    /// sent [`LifecycleEvent::EnterPlay`] and until the session manager consumes
    /// it to spawn the join burst (issue #101 Slice B). Stored on the connection
    /// so the handoff and the first coalesced play frames cannot be torn apart.
    play_handoff: Option<(GameProfile, ClientInformation)>,
}

impl ConnChannels {
    pub fn new(
        id: ConnectionId,
        remote: SocketAddr,
        in_rx: mpsc::Receiver<ServerboundFrame>,
        out_tx: mpsc::Sender<OutboundEvent>,
        drained: InboundDrained,
    ) -> Self {
        ConnChannels {
            id,
            remote,
            in_rx,
            out_tx,
            drained,
            pending_frame: None,
            play_handoff: None,
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub fn remote(&self) -> SocketAddr {
        self.remote
    }

    /// Record the configuration→play handoff ([`LifecycleEvent::EnterPlay`]).
    pub(crate) fn set_play_handoff(
        &mut self,
        profile: GameProfile,
        client_information: ClientInformation,
    ) {
        self.play_handoff = Some((profile, client_information));
    }

    /// Consume the handoff; `None` when no handoff is pending.
    pub(crate) fn take_play_handoff(&mut self) -> Option<(GameProfile, ClientInformation)> {
        self.play_handoff.take()
    }
}

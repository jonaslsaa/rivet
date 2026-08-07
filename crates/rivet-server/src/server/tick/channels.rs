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

use bytes::Bytes;
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
/// (the window resets when the channel is observed empty, which a concurrently
/// draining tick can allow mid-drain).
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
    },
    Disconnect {
        id: ConnectionId,
        reason: DisconnectReason,
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
    /// A frame dequeued but not delivered because it would cross the strict
    /// per-tick byte budget (see `ConnectionRegistry::drain_one_bounded`).
    /// Preserved in FIFO order ahead of the channel and delivered by the next
    /// drain — deterministic retention, never dropped.
    pub(crate) pending_frame: Option<ServerboundFrame>,
}

impl ConnChannels {
    pub fn new(
        id: ConnectionId,
        remote: SocketAddr,
        in_rx: mpsc::Receiver<ServerboundFrame>,
        out_tx: mpsc::Sender<OutboundEvent>,
    ) -> Self {
        ConnChannels {
            id,
            remote,
            in_rx,
            out_tx,
            pending_frame: None,
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub fn remote(&self) -> SocketAddr {
        self.remote
    }
}

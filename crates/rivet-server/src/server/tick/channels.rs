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
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub fn remote(&self) -> SocketAddr {
        self.remote
    }
}

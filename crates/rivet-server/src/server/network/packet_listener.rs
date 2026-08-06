use bytes::Bytes;

use rivet_protocol::generated::protocol::ConnectionProtocol;

use crate::server::ServerConfig;
use crate::server::network::connection::Connection;

/// Outcome of handling one frame: keep the current listener, or replace it with
/// a transitioned one (mirrors `Connection.setupInboundProtocol` swapping the
/// packet listener when the handshake moves to status/login).
pub enum ListenerOutcome {
    Keep,
    Switch(Box<dyn PacketListener>),
}

/// A per-state packet listener. Mirrors `net.minecraft.network.protocol`'s
/// `ServerPacketListener` interface and the handshake/status/login impls: each
/// listener owns the packet-id dispatch and body parsing for *its* protocol —
/// the boundary where protocol packet bodies (epic #10) are consumed. Deferred
/// states (login/configuration) are clean stubs that close deterministically.
///
/// A listener is `Box<dyn PacketListener + Send>` in `Connection`, matching the
/// OWNERSHIP take-tick-putback pattern: the per-connection task takes the
/// listener out, hands it the connection, and re-inserts the (possibly switched)
/// result. `Send` is required because the listener lives inside a spawned tokio
/// task (the per-connection task must be `Send`).
pub trait PacketListener: Send {
    /// `ServerPacketListener.protocol()` — the connection state this listener serves.
    fn protocol(&self) -> ConnectionProtocol;

    /// Handle one fully-framed inbound packet (packet-id varint + body). Returns
    /// a `ListenerOutcome` to keep/switch the listener, or a `DisconnectReason`
    /// to close the connection deterministically.
    fn handle_frame(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
        config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason>;

    /// `onDisconnect(DisconnectionDetails)` — called when the connection drops.
    /// No-ops for all listeners in this slice.
    fn on_disconnect(&mut self) {}
}

/// Why a connection is being closed. The observable behavior is always "close
/// the TCP socket deterministically"; the reason exists for faithful logging of
/// Paper's disconnect messages (`DisconnectionDetails`). Packet-body reasons
/// that Paper would transmit (outdated client/server, transfers disabled, ...)
/// carry the translation key in [`DisconnectReason::Unsupported`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum DisconnectReason {
    /// `disconnect.endOfStream` — peer closed the connection.
    #[error("disconnect.endOfStream")]
    EndOfStream,
    /// `disconnect.timeout` — `ReadTimeoutHandler` fired.
    #[error("disconnect.timeout")]
    Timeout,
    /// A corrupted frame or malformed packet body.
    #[error("malformed data: {0}")]
    Malformed(String),
    /// A valid-but-unsupported request (a state/packet whose body is owned by a
    /// not-yet-ported protocol issue, or a deliberately-rejected intention).
    #[error("unsupported: {0}")]
    Unsupported(String),
}

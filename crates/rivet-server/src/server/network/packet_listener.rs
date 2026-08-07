use std::any::Any;

use bytes::{Bytes, BytesMut};

use rivet_protocol::codec::StreamDecoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
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

    /// `TickablePacketListener.tick()` — the per-tick driver hook, reserved for
    /// #157 (Paper's configuration keepalive + task ticking). Not yet driven by
    /// the connection loop, so the default is a no-op; the configuration
    /// listener's keepalive/task tick mechanics land with #157.
    // RivetTodo(#157): drive listener ticks from `conn_loop` (Paper ticks every
    // listener each server tick; the per-connection task has no tick source yet).
    fn tick(&mut self) {}

    /// `onDisconnect(DisconnectionDetails)` — called when the connection drops.
    /// No-ops for all listeners in this slice.
    fn on_disconnect(&mut self) {}
}

/// Why a connection is being closed. The observable behavior is always "close
/// the TCP socket deterministically"; the reason exists for faithful logging of
/// Paper's disconnect messages (`DisconnectionDetails`). Packet-body reasons
/// that Paper would transmit (outdated client/server, transfers disabled, ...)
/// carry the translation key in [`DisconnectReason::Unsupported`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
    /// `multiplayer.disconnect.server_shutdown` — the tick thread is stopping.
    #[error("multiplayer.disconnect.server_shutdown")]
    ServerShutdown,
    /// `multiplayer.status.request_handled` — a status-protocol request was
    /// served and the connection closed on purpose: `ServerStatusPacketListenerImpl`
    /// disconnects with this reason after a ping (pong sent, then close) and on a
    /// second `status_request`. A normal terminal close, distinct from
    /// [`DisconnectReason::Unsupported`] (which reports a state/packet the server
    /// does not handle).
    #[error("multiplayer.status.request_handled")]
    RequestHandled,
    /// Slice-local outbound-overload disconnect: the tick side dropped this
    /// connection's tick→network channel when it overflowed (the bounded-channel
    /// backpressure policy of sub-issue #93). Paper's netty outbound buffer is
    /// unbounded, so there is no Java analog; the observable behavior is the
    /// same — the socket closes. Kept distinct from [`DisconnectReason::ServerShutdown`]
    /// so a client that cannot keep up is not misreported as a server stop.
    #[error("outbound overflow")]
    Overflow,
    /// Slice-local inbound-overload disconnect: a hostile client decoded more
    /// inbound frames/decompressed bytes in one inbound drain (or play-state
    /// forwarding window) than the per-connection budget allows — the
    /// compressed-frame memory-amplification bound. This is deliberate anti-flood
    /// policy that fires before socket-level TCP backpressure (which would not
    /// protect the bounded tick channel from decompressed-frame amplification).
    /// No Java analog; the observable behavior is the same as any deterministic
    /// close — the socket closes.
    #[error("inbound overflow: {0}")]
    InboundOverflow(String),
}

/// Read the packet-id varint off the front of a frame (dispatch helper). Bounds
/// the id via the never-panicking [`read_packet_id`]; the body is parsed
/// afterwards by [`decode_packet`].
pub(crate) fn packet_id(frame: &Bytes) -> Result<i32, DisconnectReason> {
    let mut buf = BytesMut::from(&frame[..]);
    super::server_handshake_packet_listener::read_packet_id(&mut buf)
}

/// Decode one packet body from a frame with a protocol `StreamCodec`.
///
/// The frame is the whole inbound packet (packet-id varint + body). The
/// packet-id varint is consumed via the never-panicking [`read_packet_id`]
/// (which also bounds the id), then the body is decoded by the codec's
/// `decode` half. That returns `Err(CodecError)` for every structurally
/// detectable failure — hostile strings, over-length buffers, out-of-range enum
/// ordinals, `IdentifierException` — exactly the netty `DecoderException`/
/// `IOException`s `PacketDecoder` lets surface and close the connection.
///
/// After the body, any bytes left in the frame are the `PacketDecoder.decode`
/// "was larger than I expected, found X bytes extra" IOException — a close, not
/// a leak into the next packet.
///
/// Truncated scalar reads deliberately panic inside the codec (Java's unchecked
/// `IndexOutOfBoundsException` on an empty buffer: the `StreamCodec` scalars
/// `read_uuid`, `read_var_int`, `read_long`, … are not wrapped by the codec
/// boundary). `PacketDecoder` turns that netty `IndexOutOfBoundsException` into
/// a close; the panic is caught here — at the decode boundary, the Rust analog
/// of `PacketDecoder.decode`'s try/catch — and returned as a clean
/// `DisconnectReason::Malformed`. The decode never panics out of this boundary:
/// hostile input (a truncated ping long, a short UUID, …) closes the connection
/// deterministically and the per-connection task's tail (cap decrement,
/// `on_disconnect`, `connection_closed`) always runs.
pub(crate) fn decode_packet<T, C>(frame: Bytes, codec: C) -> Result<T, DisconnectReason>
where
    C: StreamDecoder<FriendlyByteBuf, T>,
{
    let mut buf = BytesMut::from(&frame[..]);
    super::server_handshake_packet_listener::read_packet_id(&mut buf)?;
    let mut input = FriendlyByteBuf::new(buf);
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| codec.decode(&mut input)))
        .map_err(|payload| {
            DisconnectReason::Malformed(format!(
                "decoding {} panicked: {}",
                std::any::type_name::<T>(),
                panic_message(payload)
            ))
        })?
        .map_err(|e| {
            DisconnectReason::Malformed(format!(
                "decoding {}: {}",
                std::any::type_name::<T>(),
                e.message
            ))
        })?;
    if input.readable_bytes() != 0 {
        return Err(DisconnectReason::Malformed(format!(
            "packet was larger than expected, {} bytes extra",
            input.readable_bytes()
        )));
    }
    Ok(value)
}

/// Extract the message from a `catch_unwind` panic payload (a `&str` or
/// `String`, as `panic!` produces) for the malformed-close log line. Shared by
/// the decode boundary ([`decode_packet`]) and the encode boundary
/// (`super::server_login_packet_listener::encode_body`), which both turn a
/// `StreamCodec` panic into a `DisconnectReason` close.
pub(crate) fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        msg.to_string()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        "non-string panic".to_string()
    }
}

use bytes::{Buf, Bytes, BytesMut};

use rivet_protocol::generated::protocol::ConnectionProtocol;

use super::connection::Connection;
use super::packet_listener::{DisconnectReason, ListenerOutcome, PacketListener};
use super::server_login_packet_listener::ServerLoginPacketListener;
use super::server_status_packet_listener::ServerStatusPacketListener;
use crate::server::ServerConfig;

/// The handshake packet id — `HandshakeProtocols.SERVERBOUND` has exactly one
/// packet: `ClientIntentionPacket` (`HandshakePacketTypes.CLIENT_INTENTION`,
/// `addPacket` index 0, the generated `handshake::serverbound::PacketType::Intention.id()`).
const CLIENT_INTENTION_PACKET_ID: i32 = 0;

/// The current network protocol version — `SharedConstants.getCurrentVersion()`
/// → `RELEASE_NETWORK_PROTOCOL_VERSION` (Paper 26.2, protocol 776). Clients must
/// send exactly this in the handshake to proceed to login.
pub const PROTOCOL_VERSION: i32 = 776;

/// `net.minecraft.server.network.ServerHandshakePacketListenerImpl` — the
/// initial listener for every accepted connection
/// (`Connection.setListenerForServerboundHandshake`).
///
/// On `handleIntention` it parses the body exactly as `ClientIntentionPacket`
/// and switches protocol state per `intention`:
///   - LOGIN → `setupOutboundProtocol(LOGIN)` then, on a matching protocol
///     version, `setupInboundProtocol(LOGIN, ServerLoginPacketListenerImpl)`.
///   - STATUS → `setupOutboundProtocol(STATUS)` then `setupInboundProtocol(STATUS,
///     ServerStatusPacketListenerImpl)` (`repliesToStatus()` is unconditionally
///     true in Paper 26.2 — `MinecraftServer` has no branch).
///   - TRANSFER → closed immediately (Paper `MinecraftServer.acceptsTransfers()`
///     is false; the transfers-disabled disconnect body is a login-protocol packet
///     owned by #96 — STUB(mc.network.protocol.login) — so the connection is
///     dropped at the handshake boundary).
///   - unknown intention → `IllegalArgumentException("Unknown connection intent")`.
///
/// The protocol-version gate (inside `beginLogin`) applies to LOGIN only: a wrong
/// version is disconnected. STATUS skips it (a wrong-version client may still
/// ping). The *message* (outdated client/server) is a `ClientboundLoginDisconnectPacket`
/// body owned by epic #10/#96 — STUB(mc.network.protocol.login) — so the wrong
/// version is closed with `DisconnectReason::Unsupported` and no body sent.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerHandshakePacketListener;

impl PacketListener for ServerHandshakePacketListener {
    fn protocol(&self) -> ConnectionProtocol {
        ConnectionProtocol::Handshake
    }

    fn handle_frame(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
        _config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        let mut buf = BytesMut::from(&frame[..]);
        let packet_id = read_packet_id(&mut buf)?;

        if packet_id != CLIENT_INTENTION_PACKET_ID {
            // Vanilla dispatches only packet 0 in HANDSHAKING; any other id is a
            // malformed frame and netty closes the connection.
            return Err(DisconnectReason::Malformed(format!(
                "unknown handshake packet id {packet_id}"
            )));
        }

        let intention = parse_intention(&mut buf)?;

        // `PacketDecoder.decode` throws `IOException("... was larger than I
        // expected, found X bytes extra ...")` when the frame has bytes left
        // after the packet body, which closes the connection. Same rule here.
        if buf.has_remaining() {
            return Err(DisconnectReason::Malformed(format!(
                "handshake packet was larger than expected, {} bytes extra",
                buf.remaining()
            )));
        }

        match intention.intention {
            // `acceptsTransfers()` is false (Paper `MinecraftServer` default; the
            // `accepts-transfers` config knob is not ported). Paper sets up the
            // login CLIENTBOUND, sends `ClientboundLoginDisconnectPacket` with
            // "multiplayer.disconnect.transfers_disabled", then disconnects — the
            // connection never enters the login listener and `beginLogin`'s
            // protocol-version gate never runs. The formatted disconnect body is a
            // login-protocol packet owned by #96/epic #10 — STUB(mc.network.protocol.login) —
            // so this slice only records the close with the translation key.
            ClientIntent::Transfer => Err(DisconnectReason::Unsupported(
                "multiplayer.disconnect.transfers_disabled".into(),
            )),
            ClientIntent::Status => {
                // Vanilla skips the protocol-version check for STATUS (a wrong-version
                // client may still ping).
                // `setupOutboundProtocol(StatusProtocols.CLIENTBOUND)` then
                // `setupInboundProtocol(SERVERBOUND, ServerStatusPacketListenerImpl)`.
                conn.set_outbound_protocol(ConnectionProtocol::Status);
                Ok(ListenerOutcome::Switch(Box::new(
                    ServerStatusPacketListener::new(),
                )))
            }
            ClientIntent::Login => {
                // `beginLogin` → the three-way protocol-version gate. The *message*
                // (outdated client/server) is a `ClientboundLoginDisconnectPacket`
                // body owned by epic #10/#96 — STUB(mc.network.protocol.login) — so
                // a wrong version is closed with `DisconnectReason::Unsupported`
                // and no body sent.
                if intention.protocol_version != PROTOCOL_VERSION {
                    return Err(DisconnectReason::Unsupported(format!(
                        "client protocol {} != server protocol {PROTOCOL_VERSION}",
                        intention.protocol_version
                    )));
                }
                // `setupOutboundProtocol(LoginProtocols.CLIENTBOUND)` then
                // `setupInboundProtocol(SERVERBOUND, ServerLoginPacketListenerImpl)`.
                conn.set_outbound_protocol(ConnectionProtocol::Login);
                Ok(ListenerOutcome::Switch(Box::new(
                    ServerLoginPacketListener::new(),
                )))
            }
        }
    }

    fn on_disconnect(&mut self) {}
}

/// `ClientIntentionPacket`'s body: `(protocolVersion VarInt, hostName Utf <=
/// Short.MAX_VALUE, port ushort, intention VarInt)`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClientIntention {
    protocol_version: i32,
    host_name: String,
    port: u16,
    intention: ClientIntent,
}

/// `net.minecraft.network.protocol.handshake.ClientIntent` — the intention
/// enum. `byId` maps 1/2/3; anything else throws
/// `IllegalArgumentException("Unknown connection intent: " + id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientIntent {
    Status,
    Login,
    Transfer,
}

impl ClientIntent {
    const STATUS_ID: i32 = 1;
    const LOGIN_ID: i32 = 2;
    const TRANSFER_ID: i32 = 3;
}

/// Read the packet-id varint off the front of a frame. `VarInt.read` panics on a
/// "VarInt too big" input; a frame body can only produce that by carrying a sixth
/// continuation byte — a corrupted frame, closed deterministically here instead
/// of panicking across the task boundary.
pub(crate) fn read_packet_id(buf: &mut BytesMut) -> Result<i32, DisconnectReason> {
    read_varint(buf).map_err(|e| DisconnectReason::Malformed(format!("packet id: {e}")))
}

/// Bounded varint reader that never panics: returns a `Result` instead of
/// `crate::var_int::read`'s panic on over-long input, and requires the bytes to
/// be present in the frame.
fn read_varint(buf: &mut BytesMut) -> Result<i32, String> {
    let mut out: u32 = 0;
    for i in 0..5u32 {
        if !buf.has_remaining() {
            return Err("varint runs past end of frame".into());
        }
        let byte = buf.get_u8();
        out |= ((byte & 0x7F) as u32) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok(out as i32);
        }
    }
    Err("VarInt too big".into())
}

fn parse_intention(buf: &mut BytesMut) -> Result<ClientIntention, DisconnectReason> {
    let protocol_version = read_varint(buf)
        .map_err(|e| DisconnectReason::Malformed(format!("protocolVersion: {e}")))?;
    // Spigot: `readUtf(Short.MAX_VALUE)` (32767) for the hostname.
    let host_name = read_utf(buf).map_err(DisconnectReason::Malformed)?;
    // `readUnsignedShort()` — must be present in the frame; a truncated body is
    // a malformed packet, not a panic (`get_u16` would panic).
    if buf.remaining() < 2 {
        return Err(DisconnectReason::Malformed(format!(
            "truncated port: {} bytes left",
            buf.remaining()
        )));
    }
    let port = buf.get_u16();
    let intention_id =
        read_varint(buf).map_err(|e| DisconnectReason::Malformed(format!("intention: {e}")))?;
    let intention = match intention_id {
        ClientIntent::STATUS_ID => ClientIntent::Status,
        ClientIntent::LOGIN_ID => ClientIntent::Login,
        ClientIntent::TRANSFER_ID => ClientIntent::Transfer,
        _ => {
            return Err(DisconnectReason::Unsupported(format!(
                "Unknown connection intent: {intention_id}"
            )));
        }
    };
    Ok(ClientIntention {
        protocol_version,
        host_name,
        port,
        intention,
    })
}

/// `readUtf(int maxLength)` — wire form `(length VarInt, utf-8 bytes)`, mirroring
/// `net.minecraft.network.Utf8String.read` exactly except that errors are
/// returned as `Result` instead of panicking (a malformed frame is a deterministic
/// disconnect, not a task abort). The four checks fire in Java's order:
///
/// 1. `length > maxLength * 3` → "longer than maximum allowed";
/// 2. `length < 0` → "less than zero";
/// 3. `length > bytes left in frame` → "Not enough bytes in buffer";
/// 4. decoded UTF-16 length > maxLength → "longer than maximum allowed".
///
/// The payload is decoded with the WHATWG "UTF-8 decode" algorithm (what the JDK
/// `new String(bytes, UTF_8)` implements), reusing `rivet-protocol`'s
/// differential-tested decoder — not strict UTF-8, which would reject byte
/// sequences Java accepts (replacing them with U+FFFD).
///
/// `Short.MAX_VALUE * 3` is the Spigot hostname cap (`readUtf(Short.MAX_VALUE)`).
const MAX_HOSTNAME_UTF_BYTES: i32 = 32_767 * 3;

fn read_utf(buf: &mut BytesMut) -> Result<String, String> {
    let len = read_varint(buf)?;
    if len > MAX_HOSTNAME_UTF_BYTES {
        return Err(format!(
            "The received encoded string buffer length is longer than maximum allowed ({len} > {MAX_HOSTNAME_UTF_BYTES})"
        ));
    }
    if len < 0 {
        return Err(
            "The received encoded string buffer length is less than zero! Weird string!".into(),
        );
    }
    let available = buf.remaining() as i32;
    if len > available {
        return Err(format!(
            "Not enough bytes in buffer, expected {len}, but got {available}"
        ));
    }
    let bytes: Vec<u8> = buf.copy_to_bytes(len as usize).to_vec();
    let result = rivet_protocol::utf8_string::decode_utf8(&bytes);
    let result_length = result.encode_utf16().count() as i32;
    if result_length > 32_767 {
        return Err(format!(
            "The received string length is longer than maximum allowed ({result_length} > 32767)"
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_utf_declared_payload_longer_than_buffer_rejects_exactly() {
        // Wire: hostname length varint 100, then 6 bytes left in the frame (a
        // 3-byte hostname + 2-byte port + 1-byte intention). `Utf8String.read`
        // check 3 fires before the payload is touched.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[100]); // length varint: 100
        buf.extend_from_slice(b"abc");
        buf.extend_from_slice(&25565u16.to_be_bytes());
        buf.extend_from_slice(&[0x01]);
        let err = read_utf(&mut buf).unwrap_err();
        assert_eq!(err, "Not enough bytes in buffer, expected 100, but got 6");
    }
}

use bytes::{Buf, Bytes, BytesMut};

use rivet_protocol::generated::protocol::ConnectionProtocol;

use super::connection::Connection;
use super::packet_listener::{DisconnectReason, ListenerOutcome, PacketListener};
use crate::server::ServerConfig;

/// `net.minecraft.server.network.ServerStatusPacketListenerImpl` — the listener
/// entered from the handshake STATUS transition.
///
/// Packet ids (`StatusProtocols.SERVERBOUND`): 0 = `status_request`, 1 =
/// `ping_request`.
const STATUS_REQUEST_PACKET_ID: i32 = 0;
const PING_REQUEST_PACKET_ID: i32 = 1;
/// `StatusProtocols.CLIENTBOUND` id for `ClientboundPongResponsePacket`.
const PONG_RESPONSE_PACKET_ID: i32 = 1;

/// Slice scope: the full status JSON response is a `ServerStatus` body owned by
/// epic #10 (the `mc.network.protocol.status` unit) — STUB(mc.network.protocol.status).
/// Until it lands, the status listener accepts `status_request` (id 0) and stays
/// connected, faithfully applying the Java `hasRequestedStatus` single-request
/// guard. The ping echo (id 1) is a raw 8-byte long both ways and IS
/// implemented, because it is fully wire-typed here and exercises the outbound
/// framing path end to end.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerStatusPacketListener {
    has_requested_status: bool,
}

impl ServerStatusPacketListener {
    pub fn new() -> Self {
        ServerStatusPacketListener {
            has_requested_status: false,
        }
    }
}

impl PacketListener for ServerStatusPacketListener {
    fn protocol(&self) -> ConnectionProtocol {
        ConnectionProtocol::Status
    }

    fn handle_frame(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
        _config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        let mut buf = BytesMut::from(&frame[..]);
        let packet_id = super::server_handshake_packet_listener::read_packet_id(&mut buf)?;

        match packet_id {
            STATUS_REQUEST_PACKET_ID => {
                // `PacketDecoder.decode` throws IOException("... was larger than
                // I expected, found X bytes extra ...") when the frame has bytes
                // left after the packet body, which closes the connection. Same
                // rule here (the status_request body is empty).
                if buf.has_remaining() {
                    return Err(DisconnectReason::Malformed(format!(
                        "status request was larger than expected, {} bytes extra",
                        buf.remaining()
                    )));
                }
                if self.has_requested_status {
                    // Java disconnects on a second status_request
                    // (`multiplayer.status.request_handled`).
                    return Err(DisconnectReason::Unsupported(
                        "multiplayer.status.request_handled".into(),
                    ));
                }
                self.has_requested_status = true;
                // Paper: `StandardPaperServerListPingEventImpl.processRequest` →
                // `ClientboundStatusResponsePacket`. The JSON body is deferred
                // (epic #10) — STUB(mc.network.protocol.status). Staying open (no
                // response yet) keeps the ping_request path reachable, as in
                // vanilla.
                Ok(ListenerOutcome::Keep)
            }
            PING_REQUEST_PACKET_ID => {
                // `ServerboundPingRequestPacket` body: a single long.
                if buf.remaining() != 8 {
                    return Err(DisconnectReason::Malformed(format!(
                        "ping request body {} bytes != 8",
                        buf.remaining()
                    )));
                }
                let time = buf.get_i64();
                // `connection.send(new ClientboundPongResponsePacket(time))`
                // then `connection.disconnect(DISCONNECT_REASON)`.
                conn.send_packet(
                    ConnectionProtocol::Status,
                    PONG_RESPONSE_PACKET_ID as u32,
                    &time.to_be_bytes(),
                )
                .map_err(|e| DisconnectReason::Unsupported(format!("send failed: {e}")))?;
                Err(DisconnectReason::Unsupported(
                    "multiplayer.status.request_handled".into(),
                ))
            }
            other => Err(DisconnectReason::Malformed(format!(
                "unknown status packet id {other}"
            ))),
        }
    }

    fn on_disconnect(&mut self) {}
}

use bytes::Bytes;

use rivet_protocol::generated::packets::status::serverbound::PacketType as ServerboundStatusPacket;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::ping::clientbound_pong_response::ClientboundPongResponsePacket;
use rivet_protocol::protocol::ping::serverbound_ping_request::ServerboundPingRequestPacket;
use rivet_protocol::protocol::status::ServerStatus;
use rivet_protocol::protocol::status::clientbound_status_response_packet::ClientboundStatusResponsePacket;
use rivet_protocol::protocol::status::serverbound_status_request_packet::ServerboundStatusRequestPacket;

use super::connection::Connection;
use super::packet_listener::{
    DisconnectReason, ListenerOutcome, PacketListener, decode_packet, packet_id,
};
use super::server_login_packet_listener::encode_body;
use crate::server::ServerConfig;

/// `net.minecraft.server.network.ServerStatusPacketListenerImpl` — the listener
/// entered from the handshake STATUS transition.
///
/// Java: `ServerStatusPacketListenerImpl.java` in `working/Paper`. The listener
/// is constructed with the server's status and the connection; it serves the
/// status protocol: a `status_request` (id 0) is answered with a
/// `ClientboundStatusResponsePacket` of that status (once — a second request is
/// disconnected with `multiplayer.status.request_handled`), and a
/// `ping_request` (id 1) is answered with a `ClientboundPongResponsePacket` of
/// the echoed time, then the connection is disconnected (same reason).
///
/// Both packet ids and the outbound response ids come from the generated
/// `generated::packets::status` tables (the vanilla `StatusProtocols`
/// addPacket order); bodies are decoded/encoded with the real packet
/// `StreamCodec`s (issue #243), so framing, trailing-byte rejection, and
/// short/long ping handling match `PacketDecoder`'s Java behavior.
pub struct ServerStatusPacketListener {
    /// `ServerStatusPacketListenerImpl.status` — the status served to every
    /// status client (built once by the handshake boundary).
    status: ServerStatus,
    /// `ServerStatusPacketListenerImpl.hasRequestedStatus` — the single-request
    /// guard.
    has_requested_status: bool,
}

impl ServerStatusPacketListener {
    /// `new ServerStatusPacketListenerImpl(ServerStatus, Connection)`.
    pub fn new(status: ServerStatus) -> Self {
        ServerStatusPacketListener {
            status,
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
        let id = packet_id(&frame)?;
        match ServerboundStatusPacket::from_id(id as u32) {
            Some(ServerboundStatusPacket::StatusRequest) => {
                // `handleStatusRequest`: the body is empty
                // (`ServerboundStatusRequestPacket.STREAM_CODEC` is `unit`);
                // `decode_packet` closes on any trailing bytes (the `PacketDecoder`
                // "was larger than I expected" close).
                let _: ServerboundStatusRequestPacket =
                    decode_packet(frame, ServerboundStatusRequestPacket::stream_codec())?;
                if self.has_requested_status {
                    // Java disconnects on a second status_request
                    // (`multiplayer.status.request_handled`).
                    return Err(DisconnectReason::RequestHandled);
                }
                self.has_requested_status = true;
                // `StandardPaperServerListPingEventImpl.processRequest` →
                // `connection.send(new ClientboundStatusResponsePacket(ping))`.
                let body = encode_body(
                    ClientboundStatusResponsePacket::stream_codec(),
                    &ClientboundStatusResponsePacket::new(self.status.clone()),
                )
                .map_err(DisconnectReason::Unsupported)?;
                conn.send_packet(
                    ConnectionProtocol::Status,
                    rivet_protocol::generated::packets::status::clientbound::PacketType::StatusResponse
                        .id(),
                    &body,
                )
                .map_err(|e| DisconnectReason::Unsupported(format!("send failed: {e}")))?;
                Ok(ListenerOutcome::Keep)
            }
            Some(ServerboundStatusPacket::PingRequest) => {
                // `handlePingRequest`: the body is one long
                // (`ServerboundPingRequestPacket.STREAM_CODEC`). `decode_packet`
                // rejects trailing bytes; a truncated body panics inside the
                // codec (Java's unchecked `IndexOutOfBoundsException` on an
                // empty buffer) and the decode boundary catches it as a
                // `DisconnectReason::Malformed` close (documented on
                // `decode_packet`) — the connection-cap slot is reclaimed.
                let ping: ServerboundPingRequestPacket =
                    decode_packet(frame, ServerboundPingRequestPacket::stream_codec())?;
                // `connection.send(new ClientboundPongResponsePacket(packet.getTime()))`.
                let body = encode_body(
                    ClientboundPongResponsePacket::stream_codec(),
                    &ClientboundPongResponsePacket::new(ping.time()),
                )
                .map_err(DisconnectReason::Unsupported)?;
                conn.send_packet(
                    ConnectionProtocol::Status,
                    rivet_protocol::generated::packets::status::clientbound::PacketType::PongResponse
                        .id(),
                    &body,
                )
                .map_err(|e| DisconnectReason::Unsupported(format!("send failed: {e}")))?;
                // `connection.disconnect(DISCONNECT_REASON)`.
                Err(DisconnectReason::RequestHandled)
            }
            None => Err(DisconnectReason::Malformed(format!(
                "unknown status packet id {id}"
            ))),
        }
    }

    fn on_disconnect(&mut self) {}
}

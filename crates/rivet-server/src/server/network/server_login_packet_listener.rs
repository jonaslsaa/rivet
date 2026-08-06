use bytes::Bytes;

use rivet_protocol::generated::protocol::ConnectionProtocol;

use super::connection::Connection;
use super::packet_listener::{DisconnectReason, ListenerOutcome, PacketListener};
use crate::server::ServerConfig;

/// STUB(protocol.login) — clean stub for
/// `net.minecraft.server.network.ServerLoginPacketListenerImpl`.
///
/// The login state machine (hello/name, challenge/encryption, compression,
/// login success) is sub-issue #96; its packet bodies are epic #10 protocol-owned.
/// Until that lands, any frame received in the LOGIN state is closed
/// deterministically: a client cannot proceed past the handshake, and a
/// protocol/next-state discriminant test can assert the connection is closed
/// rather than left half-open. No packet bodies are invented here.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerLoginPacketListener;

impl ServerLoginPacketListener {
    pub fn new() -> Self {
        ServerLoginPacketListener
    }
}

impl PacketListener for ServerLoginPacketListener {
    fn protocol(&self) -> ConnectionProtocol {
        ConnectionProtocol::Login
    }

    fn handle_frame(
        &mut self,
        _frame: Bytes,
        _conn: &mut Connection,
        _config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        // No login packet bodies are ported yet (epic #10 / #96). Any login-state
        // frame is unsupported and the connection is closed.
        Err(DisconnectReason::Unsupported(
            "login state not implemented yet (issue #96)".into(),
        ))
    }

    fn on_disconnect(&mut self) {}
}

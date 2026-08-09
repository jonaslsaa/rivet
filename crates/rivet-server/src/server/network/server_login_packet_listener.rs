use bytes::{Bytes, BytesMut};

use rivet_protocol::codec::StreamEncoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::login::clientbound_login_compression_packet::ClientboundLoginCompressionPacket;
use rivet_protocol::protocol::login::clientbound_login_finished_packet::ClientboundLoginFinishedPacket;
use rivet_protocol::protocol::login::serverbound_hello_packet::ServerboundHelloPacket;
use rivet_protocol::protocol::login::serverbound_login_acknowledged_packet::{
    ServerboundLoginAcknowledgedPacket, stream_codec,
};
use rivet_registry::core::{GameProfile, create_offline_player_uuid};
use rivet_util::mth::Uuid;

use super::connection::Connection;
use super::packet_listener::{
    DisconnectReason, ListenerOutcome, PacketListener, decode_packet, packet_id, panic_message,
};
use super::server_configuration_packet_listener::ServerConfigurationPacketListener;
use crate::server::ServerConfig;

/// `LoginProtocols.SERVERBOUND` packet ids (`ServerLoginPacketListenerImpl`
/// dispatch). The generated table pins `hello` 0, `key` 1, `custom_query_answer`
/// 2, `login_acknowledged` 3, `cookie_response` 4.
const HELLO_PACKET_ID: i32 = 0;
const KEY_PACKET_ID: i32 = 1;
const CUSTOM_QUERY_ANSWER_PACKET_ID: i32 = 2;
const LOGIN_ACKNOWLEDGED_PACKET_ID: i32 = 3;
const COOKIE_RESPONSE_PACKET_ID: i32 = 4;

/// `LoginProtocols.CLIENTBOUND` ids for the offline join path: `login_finished`
/// 2, `login_compression` 3.
const CLIENTBOUND_LOGIN_FINISHED_ID: u32 = 2;
const CLIENTBOUND_LOGIN_COMPRESSION_ID: u32 = 3;

/// `net.minecraft.server.network.ServerLoginPacketListenerImpl` — the offline
/// `HELLO → VERIFYING → PROTOCOL_SWITCHING` login state machine.
///
/// Java: `ServerLoginPacketListenerImpl.java` in `working/Paper`. With
/// `online-mode=false` (`usesAuthentication()` false), `handleHello` builds the
/// offline profile (`UUIDUtil.createOfflinePlayerUUID` of the name) and goes
/// straight to `verifyLoginAndFinishConnectionSetup` → `finishLoginAndWaitForClient`:
/// send `ClientboundLoginCompressionPacket` *before* `setupCompression` (so that
/// packet goes out uncompressed), then `ClientboundLoginFinishedPacket`. The
/// client replies `ServerboundLoginAcknowledgedPacket`, which swaps the outbound
/// protocol to configuration and hands off to [`ServerConfigurationPacketListener`].
///
/// The per-server lazy session UUID (`ServerConnectionListener.getSessionId()`,
/// `UUID.randomUUID()` on first use) is canonicalized to the zero UUID here —
/// there is no `rand`/`uuid` crate in the workspace, and the pinned capture
/// fixture already normalizes `sessionId -> 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LoginState {
    /// `State.HELLO` — awaiting `ServerboundHelloPacket`.
    #[default]
    Hello,
    /// `State.VERIFYING` — offline profile built; the next step sends the
    /// compression + finished packets. Paper transitions on its next `tick()`;
    /// this slice has no tick driver, so the transition is synchronous.
    Verifying,
    /// `State.PROTOCOL_SWITCHING` — awaiting `ServerboundLoginAcknowledgedPacket`.
    ProtocolSwitching,
}

/// `net.minecraft.server.network.ServerLoginPacketListenerImpl` (offline slice).
#[derive(Debug, Default)]
pub struct ServerLoginPacketListener {
    state: LoginState,
    /// The authenticated `GameProfile` built by `handle_hello` (issue #101 Slice
    /// B). Carried to the configuration listener so the finish→play handoff can
    /// transfer it to the tick thread for the join burst.
    profile: Option<GameProfile>,
}

impl ServerLoginPacketListener {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PacketListener for ServerLoginPacketListener {
    fn protocol(&self) -> ConnectionProtocol {
        ConnectionProtocol::Login
    }

    fn handle_frame(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
        config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        match packet_id(&frame)? {
            HELLO_PACKET_ID => self.handle_hello(frame, conn, config),
            LOGIN_ACKNOWLEDGED_PACKET_ID => self.handle_login_acknowledgement(frame, conn, config),
            KEY_PACKET_ID => Err(DisconnectReason::Unsupported(
                // `handleKey` is the RSA online-auth path (`ClientboundHello`/
                // `ServerboundKey`). M1 runs offline (`usesAuthentication()`
                // false), so a client sending `key` is unsupported.
                // RivetTodo(#88): the RSA cipher pair (`ClientboundHelloPacket`/
                // `ServerboundKeyPacket`, the `KEY` state) — M1 runs offline.
                "multiplayer.disconnect.unexpected_query_response".into(),
            )),
            CUSTOM_QUERY_ANSWER_PACKET_ID => Err(DisconnectReason::Unsupported(
                // `handleCustomQueryPacket0` disconnects with
                // `DISCONNECT_UNEXPECTED_QUERY` when no query was sent (the
                // Velocity path is out of scope; no login query is ever issued).
                "multiplayer.disconnect.unexpected_query_response".into(),
            )),
            COOKIE_RESPONSE_PACKET_ID => Err(DisconnectReason::Unsupported(
                // `handleCookieResponse` disconnects with
                // `DISCONNECT_UNEXPECTED_QUERY` when no cookie was requested.
                "multiplayer.disconnect.unexpected_query_response".into(),
            )),
            other => Err(DisconnectReason::Malformed(format!(
                "unknown login packet id {other}"
            ))),
        }
    }

    fn on_disconnect(&mut self) {}
}

impl ServerLoginPacketListener {
    fn handle_hello(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
        config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        // `Validate.validState(this.state == HELLO, "Unexpected hello packet")`.
        if self.state != LoginState::Hello {
            return Err(DisconnectReason::Malformed(
                "Unexpected hello packet".into(),
            ));
        }
        let hello: ServerboundHelloPacket =
            decode_packet(frame, ServerboundHelloPacket::stream_codec())?;

        // `handleHello`: `requestedUsername = packet.name()`; with
        // `usesAuthentication()` false this slice never issues the RSA challenge,
        // so the offline profile is built directly (`createOfflineProfile` — no
        // spoofed UUID/profile in this slice).
        let name = hello.name().to_string();
        let profile = GameProfile::new_without_properties(create_offline_player_uuid(&name), name);
        // Stored so the configuration listener can carry it into the play state.
        self.profile = Some(profile.clone());

        // Paper's `startClientVerification` sets state VERIFYING, then `tick()`
        // calls `verifyLoginAndFinishConnectionSetup`. No tick driver exists yet,
        // so the two transitions run inline: VERIFYING then, on success,
        // PROTOCOL_SWITCHING (compression + finished sent).
        self.state = LoginState::Verifying;

        // `verifyLoginAndFinishConnectionSetup`: if the compression threshold is
        // >= 0, `send(ClientboundLoginCompressionPacket(threshold), thenRun(()
        // -> setupCompression(threshold, true)))`. The packet is queued BEFORE
        // `setupCompression` runs, so it goes out uncompressed and the client
        // learns the threshold before the encoder starts compressing.
        if config.compression_threshold >= 0 {
            let compression_body = encode_body(
                ClientboundLoginCompressionPacket::stream_codec(),
                &ClientboundLoginCompressionPacket::new(config.compression_threshold),
            )
            .map_err(DisconnectReason::Unsupported)?;
            conn.send_packet(
                ConnectionProtocol::Login,
                CLIENTBOUND_LOGIN_COMPRESSION_ID,
                &compression_body,
            )
            .map_err(|e| DisconnectReason::Unsupported(format!("send failed: {e}")))?;
            conn.setup_compression(config.compression_threshold, true);
        }

        // `finishLoginAndWaitForClient`: send `ClientboundLoginFinishedPacket`
        // with the per-server lazy session id. `getSessionId()` is
        // `UUID.randomUUID()` cached on first use; the random value is not
        // reproducible offline, so this slice emits the zero UUID — the capture
        // fixture's canonicalization (`sessionId -> 0`), asserted by the
        // integration tests.
        // RivetTodo(#96): the per-server lazy `ServerConnectionListener
        // .getSessionId()` random UUID (needs a `rand`/`uuid` source); the zero
        // UUID is the fixture-canonical placeholder until then.
        const ZERO_SESSION_ID: Uuid = Uuid { most: 0, least: 0 };
        let finished_body = encode_body(
            ClientboundLoginFinishedPacket::stream_codec(),
            &ClientboundLoginFinishedPacket::new(profile, ZERO_SESSION_ID),
        )
        .map_err(DisconnectReason::Unsupported)?;
        conn.send_packet(
            ConnectionProtocol::Login,
            CLIENTBOUND_LOGIN_FINISHED_ID,
            &finished_body,
        )
        .map_err(|e| DisconnectReason::Unsupported(format!("send failed: {e}")))?;

        self.state = LoginState::ProtocolSwitching;
        Ok(ListenerOutcome::Keep)
    }

    fn handle_login_acknowledgement(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
        config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        // `Validate.validState(this.state == PROTOCOL_SWITCHING, "Unexpected
        // login acknowledgement packet")`.
        if self.state != LoginState::ProtocolSwitching {
            return Err(DisconnectReason::Malformed(
                "Unexpected login acknowledgement packet".into(),
            ));
        }
        // `ServerboundLoginAcknowledgedPacket.STREAM_CODEC` is `unit(INSTANCE)` —
        // a 0-byte body; `decode_packet` closes on any trailing bytes (the
        // `PacketDecoder` "was larger than I expected" close).
        let _: ServerboundLoginAcknowledgedPacket = decode_packet(frame, stream_codec())?;

        // `handleLoginAcknowledgement`: `setupOutboundProtocol(
        // ConfigurationProtocols.CLIENTBOUND)`, build the configuration listener,
        // `setupInboundProtocol(SERVERBOUND, configListener)`, then
        // `configListener.startConfiguration()`. The outbound protocol flips
        // before `startConfiguration` sends the brand, so that brand goes out as
        // a configuration packet. (Java's final `state = ACCEPTED` is moot here:
        // the listener is replaced by the configuration one.)
        conn.set_outbound_protocol(ConnectionProtocol::Configuration);
        let profile = self
            .profile
            .clone()
            .expect("handle_hello built the profile before the ack");
        // The configuration keepalive (issue #283) is seeded with the
        // connection's monotonic reading at construction — Paper's
        // `lastKeepAliveTx = System.nanoTime()` — and the configured kick limit
        // (`paper.playerconnection.keepalive`, `ServerConfig.keepalive_timeout`).
        // `conn.monotonic_nanos()` and the tick drive (`PacketListener::tick`)
        // share the same per-connection epoch, so the 1s transmit throttle and
        // the 30s timeout count from construction exactly like Java.
        let mut config_listener = ServerConfigurationPacketListener::new(
            profile,
            conn.monotonic_nanos(),
            config.keepalive_timeout.as_nanos() as i64,
        );
        config_listener
            .start_configuration(conn)
            .map_err(DisconnectReason::Unsupported)?;
        Ok(ListenerOutcome::Switch(Box::new(config_listener)))
    }
}

/// Encode a packet body with a protocol `StreamCodec` (the `StreamEncoder`
/// half). The packet-id is NOT included — the caller passes it to
/// [`Connection::send_packet`]. The encode error is surfaced as `Err` so a
/// listener maps it to a `DisconnectReason` close — the outbound twin of the
/// decode-boundary containment in [`decode_packet`]. Java's encoder
/// `EncoderException` is netty-caught and closes the connection (cleanup runs);
/// a Rust `Result` here keeps the per-connection task's tail (cap decrement,
/// `on_disconnect`, `connection_closed`) running on every path. All current
/// encode inputs are server-built or client-bounded, so this is a defensive
/// boundary, not a reachable hostile path.
///
/// An encode panic (the unchecked scalar writes, `write_long`, `write_var_int`,
/// … on a fixed-layout body — the netty `EncoderException` a buggy writer
/// throws) is caught here the same way [`decode_packet`] catches a decode
/// panic, so the boundary never unwinds past the listener into the task tail.
pub(crate) fn encode_body<T, C>(codec: C, value: &T) -> Result<Vec<u8>, String>
where
    C: StreamEncoder<FriendlyByteBuf, T>,
{
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        codec.encode(&mut out, value)
    }))
    .map_err(|payload| {
        format!(
            "encoding {} panicked: {}",
            std::any::type_name::<T>(),
            panic_message(payload)
        )
    })?
    .map_err(|e| format!("encoding {}: {}", std::any::type_name::<T>(), e.message))?;
    Ok(out.into_inner().to_vec())
}

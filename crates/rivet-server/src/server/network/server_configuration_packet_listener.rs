use std::collections::VecDeque;

use bytes::Bytes;

use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::common::client_information::ClientInformation;
use rivet_protocol::protocol::common::clientbound_custom_payload::ClientboundCustomPayloadPacket;
use rivet_protocol::protocol::common::custom::{BrandPayload, CustomPacketPayload};
use rivet_protocol::protocol::common::serverbound_client_information::ServerboundClientInformationPacket;
use rivet_protocol::protocol::common::serverbound_custom_click_action::ServerboundCustomClickActionPacket;
use rivet_protocol::protocol::common::serverbound_custom_payload::ServerboundCustomPayloadPacket;
use rivet_protocol::protocol::common::serverbound_keep_alive::ServerboundKeepAlivePacket;
use rivet_protocol::protocol::common::serverbound_pong::ServerboundPongPacket;
use rivet_protocol::protocol::common::serverbound_resource_pack::ServerboundResourcePackPacket;

use super::connection::Connection;
use super::packet_listener::{
    DisconnectReason, ListenerOutcome, PacketListener, decode_packet, packet_id,
};
use super::server_login_packet_listener::encode_body;
use crate::server::ServerConfig;

/// `ConfigurationProtocols.SERVERBOUND` packet ids. The generated table pins:
/// `client_information` 0, `cookie_response` 1, `custom_payload` 2,
/// `finish_configuration` 3, `keep_alive` 4, `pong` 5, `resource_pack` 6,
/// `select_known_packs` 7, `custom_click_action` 8, `accept_code_of_conduct` 9.
const CLIENT_INFORMATION_PACKET_ID: i32 = 0;
const COOKIE_RESPONSE_PACKET_ID: i32 = 1;
const CUSTOM_PAYLOAD_PACKET_ID: i32 = 2;
const FINISH_CONFIGURATION_PACKET_ID: i32 = 3;
const KEEP_ALIVE_PACKET_ID: i32 = 4;
const PONG_PACKET_ID: i32 = 5;
const RESOURCE_PACK_PACKET_ID: i32 = 6;
const SELECT_KNOWN_PACKS_PACKET_ID: i32 = 7;
const CUSTOM_CLICK_ACTION_PACKET_ID: i32 = 8;
const ACCEPT_CODE_OF_CONDUCT_PACKET_ID: i32 = 9;

/// `ConfigurationProtocols.CLIENTBOUND` id for the brand packet
/// (`ClientboundCustomPayloadPacket`, registered at `custom_payload` 1).
const CLIENTBOUND_CUSTOM_PAYLOAD_ID: u32 = 1;

/// `ServerCommonPacketListenerImpl.DISCONNECT_UNEXPECTED_QUERY`.
const DISCONNECT_UNEXPECTED_QUERY: &str = "multiplayer.disconnect.unexpected_query_response";

/// The server's mod name (`MinecraftServer.getServerModName()`); Paper sends the
/// brand in `startConfiguration` via `ClientboundCustomPayloadPacket(Brand)`.
const SERVER_BRAND: &str = "Rivet";

/// `net.minecraft.server.network.ConfigurationTask` — one deferred step of the
/// configuration phase.
///
/// Java: `ConfigurationTask.java` in `working/Paper`. Each task is started by
/// `ServerConfigurationPacketListenerImpl.startNextTask` with a `Consumer<Packet>`
/// that sends one packet. This slice's only task is the honest brand followed by
/// [`RegistrySyncUnavailable`]; the full queue (registry sync, join world, spawn,
/// code of conduct, resource pack) and the client-response `finishCurrentTask`
/// machinery land with their owning units (#109/#100/#258).
trait ConfigurationTask: Send {
    /// `ConfigurationTask.start(Consumer<Packet>)` — send the task's opening
    /// packet(s). Java returns void; a task that awaits a client response (the
    /// registry sync, the resource pack) is later finished by `finishCurrentTask`
    /// — none of this slice's tasks await one.
    fn start(&mut self, conn: &mut Connection) -> Result<(), String>;
}

/// `SynchronizeRegistriesTask`'s stand-in for the offline slice.
///
/// The real task sends `ClientboundSelectKnownPacks`, then on the client's
/// `handleSelectKnownPacks` reply sends the registry/tag data and finishes. All
/// of that content — `KnownPack` negotiation,
/// `RegistrySynchronization.packRegistries`, `TagNetworkSerialization` — is
/// #109. This placeholder sends NOTHING on `start`, and the listener rejects an
/// unsolicited `select_known_packs` with Java's
/// `IllegalStateException("Unexpected response from client: received pack
/// selection, but no negotiation ongoing")` — the honest behavior when no
/// negotiation is in progress.
struct RegistrySyncUnavailable;

impl ConfigurationTask for RegistrySyncUnavailable {
    fn start(&mut self, _conn: &mut Connection) -> Result<(), String> {
        // No select-known-packs/registry/tag data — the content is #109.
        // RivetTodo(#109): `SynchronizeRegistriesTask.start` sending
        // `ClientboundSelectKnownPacks` + `handleResponse`'s registry/tag data
        // (`RegistrySynchronization.packRegistries`,
        // `TagNetworkSerialization.serializeTagsToNetwork`).
        Ok(())
    }
}

/// `net.minecraft.server.network.ServerConfigurationPacketListenerImpl` —
/// configuration phase, offline slice.
///
/// Java: `ServerConfigurationPacketListenerImpl.java` in `working/Paper`.
/// `startConfiguration` sends the brand (a `ClientboundCustomPayloadPacket`
/// wrapping `BrandPayload`), then queues the configuration tasks and starts the
/// first. The registry-sync task (`SynchronizeRegistriesTask`) is the ONLY
/// deferred step on the M1 join path — its content is registry/`KnownPack`
/// serialization (#109). This slice's task queue is Java-shaped (a FIFO drained
/// by `startNextTask`), but the deferred registry task is replaced by
/// [`RegistrySyncUnavailable`]; `currentTask`/`finishCurrentTask` arrive with
/// the tasks that await a client response.
///
/// Clientbound config packets are never sent beyond the brand; the finish→play
/// handoff (`finish_configuration` → `ClientboundFinishConfigurationPacket`,
/// `ServerGamePacketListenerImpl`) is #100/#101 and is NOT wired here.
pub struct ServerConfigurationPacketListener {
    /// `configurationTasks` — the FIFO of not-yet-started tasks.
    configuration_tasks: VecDeque<Box<dyn ConfigurationTask>>,
    /// `ServerConfigurationPacketListenerImpl.clientInformation` — the
    /// `ServerboundClientInformationPacket` value. Java initializes it from the
    /// `CommonListenerCookie` (`ClientInformation.createDefault()`); the
    /// deferred field is updated by `handleClientInformation`.
    client_information: ClientInformation,
}

impl Default for ServerConfigurationPacketListener {
    fn default() -> Self {
        ServerConfigurationPacketListener {
            configuration_tasks: VecDeque::new(),
            client_information: ClientInformation::create_default(),
        }
    }
}

impl ServerConfigurationPacketListener {
    /// `new ServerConfigurationPacketListenerImpl(server, connection, cookie)`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `startConfiguration()` — Paper sends the brand first, then queues the
    /// configuration tasks and starts the first (`startNextTask`).
    ///
    /// This slice sends ONLY the honest brand (`Rivet`) — no server links, no
    /// `update_enabled_features`, no `select_known_packs`/registry/tag data —
    /// then installs [`RegistrySyncUnavailable`] as the queue's only task.
    pub fn start_configuration(&mut self, conn: &mut Connection) -> Result<(), String> {
        // `send(new ClientboundCustomPayloadPacket(new BrandPayload(server
        // .getServerModName())))`.
        let body = encode_body(
            ClientboundCustomPayloadPacket::config_stream_codec(),
            &ClientboundCustomPayloadPacket::new(CustomPacketPayload::Brand(BrandPayload::new(
                SERVER_BRAND.to_string(),
            ))),
        );
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_CUSTOM_PAYLOAD_ID,
            &body,
        )?;

        // The full queue (`SynchronizeRegistriesTask` + optional tasks +
        // `PrepareSpawnTask` + `JoinWorldTask`) is deferred; the registry sync
        // content is #109 and the finish→play handoff is #100/#101. This slice
        // installs only the RegistrySyncUnavailable placeholder, Java-shaped
        // (a FIFO drained by startNextTask).
        // RivetTodo(#100): `PrepareSpawnTask`/`JoinWorldTask` and the
        // finish→play handoff (`finish_configuration` → `ClientboundFinish
        // ConfigurationPacket` + `spawnPlayer`).
        // RivetTodo(#101): the play-state listener
        // (`ServerGamePacketListenerImpl`) that follows `finish_configuration`.
        self.configuration_tasks
            .push_back(Box::new(RegistrySyncUnavailable));
        self.start_next_task(conn)
    }

    /// `startNextTask()` — pull the next queued task and run its `start`. Java
    /// keeps the task as `currentTask` until a client response finishes it; no
    /// task in this slice awaits one, so each is done the moment it starts.
    fn start_next_task(&mut self, conn: &mut Connection) -> Result<(), String> {
        if let Some(mut task) = self.configuration_tasks.pop_front() {
            task.start(conn)?;
        }
        Ok(())
    }
}

impl PacketListener for ServerConfigurationPacketListener {
    fn protocol(&self) -> ConnectionProtocol {
        ConnectionProtocol::Configuration
    }

    fn handle_frame(
        &mut self,
        frame: Bytes,
        _conn: &mut Connection,
        _config: &ServerConfig,
    ) -> Result<ListenerOutcome, DisconnectReason> {
        match packet_id(&frame)? {
            CLIENT_INFORMATION_PACKET_ID => {
                // `handleClientInformation` stores the value; Paper additionally
                // sets the locale attribute (adventure, out of scope).
                let packet: ServerboundClientInformationPacket =
                    decode_packet(frame, ServerboundClientInformationPacket::stream_codec())?;
                self.client_information = packet.information().clone();
                Ok(ListenerOutcome::Keep)
            }
            CUSTOM_PAYLOAD_PACKET_ID => {
                // `handleCustomPayload` → `ServerCommonPacketListenerImpl
                // .handleCustomPayload`. CraftBukkit's empty known-types list
                // decodes every serverbound payload as `DiscardedPayload`; the
                // register/unregister/brand handling (plugin messaging bridge,
                // `clientBrand`) is deferred with the plugin API.
                let packet: ServerboundCustomPayloadPacket =
                    decode_packet(frame, ServerboundCustomPayloadPacket::stream_codec())?;
                let _ = packet;
                // RivetTodo(#258): the serverbound custom-payload handling —
                // `minecraft:register`/`unregister` channel tracking and the
                // `minecraft:brand` decode (`clientBrand`). The body decodes as
                // `DiscardedPayload` here and is otherwise ignored.
                Ok(ListenerOutcome::Keep)
            }
            KEEP_ALIVE_PACKET_ID => {
                // `handleKeepAlive` — Paper's keepalive challenge tracking. The
                // periodic clientbound keepalive is deferred (#157); a serverbound
                // keepalive with no pending challenge would disconnect in Java
                // ("without matching challenge").
                let packet: ServerboundKeepAlivePacket =
                    decode_packet(frame, ServerboundKeepAlivePacket::stream_codec())?;
                let _ = packet;
                // RivetTodo(#157): the keepalive challenge state
                // (`KeepAlive.pendingKeepAlives`, out-of-order / no-challenge
                // disconnects) — the periodic `ClientboundKeepAlivePacket` and
                // the timeout/out-of-order handling are #157.
                Ok(ListenerOutcome::Keep)
            }
            PONG_PACKET_ID => {
                // `handlePong` — a no-op in Java.
                let _: ServerboundPongPacket =
                    decode_packet(frame, ServerboundPongPacket::stream_codec())?;
                Ok(ListenerOutcome::Keep)
            }
            RESOURCE_PACK_PACKET_ID => {
                // `handleResourcePackResponse` — `super.handleResourcePackResponse`
                // (the resource-pack-required kick + callbacks) plus the
                // `finishCurrentTask(ServerResourcePackConfigurationTask.TYPE)`.
                // No resource pack is ever pushed (#258), so no task finish fires.
                let packet: ServerboundResourcePackPacket =
                    decode_packet(frame, ServerboundResourcePackPacket::stream_codec())?;
                let _ = packet;
                // RivetTodo(#258): `handleResourcePackResponse` — the
                // resource-pack-required disconnect and the adventure pack
                // callbacks; no pack is pushed in this slice so there is no
                // current `ServerResourcePackConfigurationTask` to finish.
                Ok(ListenerOutcome::Keep)
            }
            CUSTOM_CLICK_ACTION_PACKET_ID => {
                // `handleCustomClickAction` — `server.handleCustomClickAction`
                // plus the Paper dialog/adventure click callbacks.
                let packet: ServerboundCustomClickActionPacket =
                    decode_packet(frame, ServerboundCustomClickActionPacket::stream_codec())?;
                let _ = packet;
                // RivetTodo(#258): `handleCustomClickAction` — the server-side
                // custom-click-action routing (server handler + click-callback
                // managers); the packet decodes here and is otherwise ignored.
                Ok(ListenerOutcome::Keep)
            }
            COOKIE_RESPONSE_PACKET_ID => {
                // `handleCookieResponse` → `DISCONNECT_UNEXPECTED_QUERY` when no
                // cookie was requested (the login cookie API is Paper-specific).
                Err(DisconnectReason::Unsupported(
                    DISCONNECT_UNEXPECTED_QUERY.into(),
                ))
            }
            SELECT_KNOWN_PACKS_PACKET_ID => {
                // `handleSelectKnownPacks`: with no `SynchronizeRegistriesTask`
                // in flight (this slice's placeholder sends nothing and finished
                // immediately), Java throws `IllegalStateException("Unexpected
                // response from client: received pack selection, but no
                // negotiation ongoing")` — surfaced here as a deterministic
                // Malformed close. The body is a `List<KnownPack>` (#109) and is
                // never parsed.
                Err(DisconnectReason::Malformed(
                    "Unexpected response from client: received pack selection, but no negotiation ongoing"
                        .into(),
                ))
            }
            FINISH_CONFIGURATION_PACKET_ID => {
                // `handleConfigurationFinished` — the finish→play handoff:
                // `setupOutboundProtocol(GameProtocols.CLIENTBOUND)`,
                // duplicate-login check, `prepareSpawnTask.spawnPlayer(...)`.
                // RivetTodo(#100): the finish→play handoff
                // (`ClientboundFinishConfigurationPacket` + the
                // `PrepareSpawnTask`/`JoinWorldTask` queue + `spawnPlayer`).
                // RivetTodo(#101): play-state listener (`ServerGamePacketListenerImpl`).
                Err(DisconnectReason::Unsupported(
                    "multiplayer.disconnect.configuration_error".into(),
                ))
            }
            ACCEPT_CODE_OF_CONDUCT_PACKET_ID => {
                // `handleAcceptCodeOfConduct` → `finishCurrentTask(
                // ServerCodeOfConductConfigurationTask.TYPE)`. No code-of-conduct
                // task is ever queued (#258), so any response is unexpected.
                // RivetTodo(#258): `ServerCodeOfConductConfigurationTask` and the
                // `accept_code_of_conduct` handshake — no CodeOfConduct packet is
                // ever sent in this slice, so the response is unsupported.
                Err(DisconnectReason::Unsupported(
                    "multiplayer.disconnect.configuration_error".into(),
                ))
            }
            other => Err(DisconnectReason::Malformed(format!(
                "unknown configuration packet id {other}"
            ))),
        }
    }

    fn on_disconnect(&mut self) {}
}

impl std::fmt::Debug for ServerConfigurationPacketListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfigurationPacketListener")
            .field("client_information", &self.client_information)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_client_information_initial_value() {
        // Java initializes `clientInformation` from the cookie
        // (`ClientInformation.createDefault()`); this slice starts there.
        let listener = ServerConfigurationPacketListener::new();
        assert_eq!(
            listener.client_information,
            ClientInformation::create_default()
        );
    }
}

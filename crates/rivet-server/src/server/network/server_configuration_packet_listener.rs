use std::collections::{HashSet, VecDeque};

use bytes::Bytes;

use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::common::client_information::ClientInformation;
use rivet_protocol::protocol::common::clientbound_custom_payload::ClientboundCustomPayloadPacket;
use rivet_protocol::protocol::common::clientbound_update_tags::ClientboundUpdateTagsPacket;
use rivet_protocol::protocol::common::custom::{BrandPayload, CustomPacketPayload};
use rivet_protocol::protocol::common::serverbound_client_information::ServerboundClientInformationPacket;
use rivet_protocol::protocol::common::serverbound_custom_click_action::ServerboundCustomClickActionPacket;
use rivet_protocol::protocol::common::serverbound_custom_payload::ServerboundCustomPayloadPacket;
use rivet_protocol::protocol::common::serverbound_keep_alive::ServerboundKeepAlivePacket;
use rivet_protocol::protocol::common::serverbound_pong::ServerboundPongPacket;
use rivet_protocol::protocol::common::serverbound_resource_pack::ServerboundResourcePackPacket;
use rivet_protocol::protocol::configuration::clientbound_registry_data::ClientboundRegistryDataPacket;
use rivet_protocol::protocol::configuration::clientbound_select_known_packs::ClientboundSelectKnownPacks;
use rivet_protocol::protocol::configuration::clientbound_update_enabled_features::ClientboundUpdateEnabledFeaturesPacket;
use rivet_protocol::protocol::configuration::serverbound_finish_configuration::ServerboundFinishConfigurationPacket;
use rivet_protocol::protocol::configuration::serverbound_select_known_packs::ServerboundSelectKnownPacks;
use rivet_registry::Identifier;
use rivet_util::KnownPack;

use super::connection::Connection;
use super::packet_listener::{
    DisconnectReason, ListenerOutcome, PacketListener, decode_packet, packet_id,
};
use super::registry_sync;
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

/// `ConfigurationProtocols.CLIENTBOUND` ids (`ConfigurationPacketTypes`, the
/// generated table pins: `custom_payload` 1, `finish_configuration` 3,
/// `registry_data` 7, `update_enabled_features` 12, `update_tags` 13,
/// `select_known_packs` 14).
const CLIENTBOUND_CUSTOM_PAYLOAD_ID: u32 = 1;
const CLIENTBOUND_REGISTRY_DATA_ID: u32 = 7;
const CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID: u32 = 12;
const CLIENTBOUND_UPDATE_TAGS_ID: u32 = 13;
const CLIENTBOUND_SELECT_KNOWN_PACKS_ID: u32 = 14;

/// `ServerCommonPacketListenerImpl.DISCONNECT_UNEXPECTED_QUERY`.
const DISCONNECT_UNEXPECTED_QUERY: &str = "multiplayer.disconnect.unexpected_query_response";

/// The server's mod name (`MinecraftServer.getServerModName()`); Paper sends the
/// brand in `startConfiguration` via `ClientboundCustomPayloadPacket(Brand)`.
const SERVER_BRAND: &str = "Rivet";

/// `ConfigurationTask.Type` id for `SynchronizeRegistriesTask` — the only task
/// this slice queues.
const SYNCHRONIZE_REGISTRIES_TASK_TYPE: &str = "synchronize_registries";
/// `ConfigurationTask.Type` id for `JoinWorldTask` — never queued here (the
/// finish→play handoff is #100/#101), so finishing it always mismatches.
const JOIN_WORLD_TASK_TYPE: &str = "join_world";

/// `net.minecraft.server.network.ConfigurationTask` — one deferred step of the
/// configuration phase.
///
/// Java: `ConfigurationTask.java` in `working/Paper`. Each task is started by
/// `ServerConfigurationPacketListenerImpl.startNextTask` with a `Consumer<Packet>`
/// that sends one packet. A task stays `currentTask` until a client response
/// finishes it via `finishCurrentTask` (`SynchronizeRegistriesTask` awaits the
/// `select_known_packs` reply). This slice queues only the registry sync.
trait ConfigurationTask: Send {
    /// `ConfigurationTask.Type.id()` — the task-type discriminator checked by
    /// `finishCurrentTask` (`ConfigurationTask.Type(String id)`).
    fn type_id(&self) -> &'static str;
    /// `ConfigurationTask.start(Consumer<Packet>)` — send the task's opening
    /// packet(s). Java returns void; a task that awaits a client response is
    /// later finished by `finishCurrentTask`.
    fn start(&mut self, conn: &mut Connection) -> Result<(), String>;
}

/// `net.minecraft.server.network.config.SynchronizeRegistriesTask` (issue #109).
///
/// Java: `SynchronizeRegistriesTask.java` in `working/Paper`. `start` sends the
/// server's `ClientboundSelectKnownPacks`; `handleResponse` sends the 29
/// `ClientboundRegistryDataPacket`s + the `ClientboundUpdateTagsPacket`, then the
/// listener finishes the task. The payload construction lives in
/// [`registry_sync`].
struct SynchronizeRegistriesTask {
    /// `requestedPacks` — the packs the client should select from
    /// (`MinecraftServer.getResourceManager().listPacks()...knownPackInfo()`).
    requested_packs: Vec<KnownPack>,
}

impl SynchronizeRegistriesTask {
    /// `new SynchronizeRegistriesTask(knownPacks, registries)` — the M1 server
    /// advertises the vanilla `minecraft:core:26.2` pack.
    fn new() -> Self {
        SynchronizeRegistriesTask {
            requested_packs: registry_sync::requested_packs(),
        }
    }
}

impl ConfigurationTask for SynchronizeRegistriesTask {
    fn type_id(&self) -> &'static str {
        SYNCHRONIZE_REGISTRIES_TASK_TYPE
    }

    fn start(&mut self, conn: &mut Connection) -> Result<(), String> {
        // `connection.accept(new ClientboundSelectKnownPacks(this.requestedPacks))`.
        let body = encode_body(
            ClientboundSelectKnownPacks::stream_codec(),
            &ClientboundSelectKnownPacks::new(self.requested_packs.clone()),
        );
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_SELECT_KNOWN_PACKS_ID,
            &body,
        )
    }
}

/// `net.minecraft.server.network.ServerConfigurationPacketListenerImpl` —
/// configuration phase, offline slice (issue #109).
///
/// Java: `ServerConfigurationPacketListenerImpl.java` in `working/Paper`.
/// `startConfiguration` sends the brand (a `ClientboundCustomPayloadPacket`
/// wrapping `BrandPayload`) and `update_enabled_features`
/// (`FeatureFlags.REGISTRY.toNames(worldData.enabledFeatures())` — `{minecraft:vanilla}`
/// on the M1 offline world), then queues the configuration tasks and starts the
/// first. The registry-sync task (`SynchronizeRegistriesTask`) is the ONLY
/// deferred step on the M1 join path; `PrepareSpawnTask`/`JoinWorldTask` (the
/// finish→play handoff) are #100/#101 and are NOT queued, so after the sync
/// task finishes the queue is empty and the connection stays in configuration
/// until `finish_configuration` (which — with no `JoinWorldTask` current —
/// mismatches `finishCurrentTask` exactly like Java and closes).
///
/// The task queue is Java-shaped (`configurationTasks` FIFO drained by
/// `startNextTask` into `currentTask`, finished by `finishCurrentTask`).
pub struct ServerConfigurationPacketListener {
    /// `configurationTasks` — the FIFO of not-yet-started tasks.
    configuration_tasks: VecDeque<Box<dyn ConfigurationTask>>,
    /// `ServerConfigurationPacketListenerImpl.currentTask` — the in-flight task
    /// awaiting a client response (`@Nullable` when idle).
    current_task: Option<Box<dyn ConfigurationTask>>,
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
            current_task: None,
            client_information: ClientInformation::create_default(),
        }
    }
}

impl ServerConfigurationPacketListener {
    /// `new ServerConfigurationPacketListenerImpl(server, connection, cookie)`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `startConfiguration()` — Paper sends the brand first, then
    /// `update_enabled_features`, then queues the configuration tasks and starts
    /// the first (`startNextTask`).
    ///
    /// This slice queues only the registry sync; the finish→play handoff
    /// (`PrepareSpawnTask`/`JoinWorldTask`) is #100/#101.
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

        // `send(new ClientboundUpdateEnabledFeaturesPacket(FeatureFlags.REGISTRY
        // .toNames(this.server.getWorldData().enabledFeatures())))` — the M1
        // offline world enables only the vanilla feature set.
        let enabled =
            ClientboundUpdateEnabledFeaturesPacket::new(HashSet::from([Identifier::parse(
                "minecraft:vanilla",
            )]));
        let body = encode_body(
            ClientboundUpdateEnabledFeaturesPacket::stream_codec(),
            &enabled,
        );
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID,
            &body,
        )?;

        // `this.synchronizeRegistriesTask = new SynchronizeRegistriesTask(...);
        // this.configurationTasks.add(this.synchronizeRegistriesTask);
        // this.addOptionalTasks(); ... this.returnToWorld();` — the optional
        // tasks (code of conduct, resource pack) are #236, and
        // `PrepareSpawnTask`/`JoinWorldTask` are the finish→play handoff.
        // RivetTodo(#100): `PrepareSpawnTask`/`JoinWorldTask` and the
        // finish→play handoff (`finish_configuration` → `ClientboundFinish
        // ConfigurationPacket` + `spawnPlayer`).
        // RivetTodo(#101): the play-state listener
        // (`ServerGamePacketListenerImpl`) that follows `finish_configuration`.
        // RivetTodo(#236): `addOptionalTasks` — the code-of-conduct and
        // resource-pack configuration tasks.
        self.configuration_tasks
            .push_back(Box::new(SynchronizeRegistriesTask::new()));
        self.start_next_task(conn)
    }

    /// `startNextTask()` — pull the next queued task, make it `currentTask`, and
    /// run its `start`. Java throws `IllegalStateException` if a task is still
    /// current; the registry sync is the only task here and it stays current
    /// until its `select_known_packs` reply arrives.
    fn start_next_task(&mut self, conn: &mut Connection) -> Result<(), String> {
        if let Some(task) = &self.current_task {
            return Err(format!("Task {} has not finished yet", task.type_id()));
        }
        if let Some(task) = self.configuration_tasks.pop_front() {
            self.current_task = Some(task);
            if let Err(e) = self.current_task.as_mut().unwrap().start(conn) {
                self.current_task = None;
                return Err(e);
            }
        }
        Ok(())
    }

    /// `finishCurrentTask(ConfigurationTask.Type)` — verify the current task's
    /// type matches, clear it, and start the next queued task. Java throws
    /// `IllegalStateException("Unexpected request for task finish, current
    /// task: ..., requested: ...")` on a mismatch — surfaced here as a Malformed
    /// close.
    fn finish_current_task(
        &mut self,
        type_id: &str,
        conn: &mut Connection,
    ) -> Result<(), DisconnectReason> {
        let current = self
            .current_task
            .as_ref()
            .map(|t| t.type_id())
            .unwrap_or("null");
        if current != type_id {
            return Err(DisconnectReason::Malformed(format!(
                "Unexpected request for task finish, current task: {current}, requested: {type_id}"
            )));
        }
        self.current_task = None;
        self.start_next_task(conn)
            .map_err(DisconnectReason::Unsupported)
    }
}

impl PacketListener for ServerConfigurationPacketListener {
    fn protocol(&self) -> ConnectionProtocol {
        ConnectionProtocol::Configuration
    }

    fn handle_frame(
        &mut self,
        frame: Bytes,
        conn: &mut Connection,
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
                // RivetTodo(#236): the serverbound custom-payload handling —
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
                // RivetTodo(#157): the listener-side wiring — this listener does
                // not yet own a `KeepaliveState`, so the periodic
                // `ClientboundKeepAlivePacket` (1 s throttle) and the serverbound
                // challenge matching (`handleKeepAlive`: pending queue,
                // out-of-order / no-challenge TIMEOUT disconnects, 30 s kick) are
                // not driven here. The pure state machine + `KeepaliveSink` seam
                // live in `server::keepalive` / `server::network::keepalive`; the
                // configuration tick hook that drives them is #96.
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
                // No resource pack is ever pushed (#236), so no task finish fires.
                let packet: ServerboundResourcePackPacket =
                    decode_packet(frame, ServerboundResourcePackPacket::stream_codec())?;
                let _ = packet;
                // RivetTodo(#236): `handleResourcePackResponse` — the
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
                // RivetTodo(#236): `handleCustomClickAction` — the server-side
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
                // `handleSelectKnownPacks` — the registry-sync negotiation
                // reply. Java throws `IllegalStateException("Unexpected response
                // from client: received pack selection, but no negotiation
                // ongoing")` when `synchronizeRegistriesTask == null`; in this
                // slice the negotiation is ongoing exactly while the sync task
                // is current, so an unsolicited reply (before the sync starts
                // or after it finished) is rejected the same way.
                if self.current_task.as_ref().map(|t| t.type_id())
                    != Some(SYNCHRONIZE_REGISTRIES_TASK_TYPE)
                {
                    return Err(DisconnectReason::Malformed(
                        "Unexpected response from client: received pack selection, but no negotiation ongoing"
                            .into(),
                    ));
                }
                let packet: ServerboundSelectKnownPacks =
                    decode_packet(frame, ServerboundSelectKnownPacks::stream_codec())?;
                // `synchronizeRegistriesTask.handleResponse(acceptedPacks, this::send)`
                // then `finishCurrentTask(SynchronizeRegistriesTask.TYPE)`.
                self.handle_sync_response(&packet, conn)?;
                Ok(ListenerOutcome::Keep)
            }
            FINISH_CONFIGURATION_PACKET_ID => {
                // `handleConfigurationFinished` — `finishCurrentTask(
                // JoinWorldTask.TYPE)`. `JoinWorldTask` is never queued in this
                // slice (#100/#101), so the type never matches and Java throws
                // `IllegalStateException` — surfaced here as a Malformed close.
                // RivetTodo(#100): the finish→play handoff
                // (`ClientboundFinishConfigurationPacket` + the
                // `PrepareSpawnTask`/`JoinWorldTask` queue + `spawnPlayer`).
                // RivetTodo(#101): play-state listener (`ServerGamePacketListenerImpl`).
                let _: ServerboundFinishConfigurationPacket = decode_packet(
                    frame,
                    rivet_protocol::protocol::configuration::serverbound_finish_configuration::stream_codec(),
                )?;
                self.finish_current_task(JOIN_WORLD_TASK_TYPE, conn)?;
                Ok(ListenerOutcome::Keep)
            }
            ACCEPT_CODE_OF_CONDUCT_PACKET_ID => {
                // `handleAcceptCodeOfConduct` → `finishCurrentTask(
                // ServerCodeOfConductConfigurationTask.TYPE)`. No code-of-conduct
                // task is ever queued (#236), so any response is unexpected.
                // RivetTodo(#236): `ServerCodeOfConductConfigurationTask` and the
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

impl ServerConfigurationPacketListener {
    /// `SynchronizeRegistriesTask.handleResponse(acceptedPacks, connection)` —
    /// send the registry/tag data and finish the task.
    ///
    /// Java: when the accepted packs equal the requested packs, the elements
    /// whose `RegistrationInfo.knownPackInfo` is in the accepted set skip their
    /// content (`Optional.empty()`); otherwise every element is fully encoded.
    /// The M1 vanilla client accepts `minecraft:core:26.2`, so every element is
    /// skipped. A client that does NOT accept cannot be served faithfully (the
    /// full NBT element codecs are unported) — see [`registry_sync::pack_registries`].
    fn handle_sync_response(
        &mut self,
        packet: &ServerboundSelectKnownPacks,
        conn: &mut Connection,
    ) -> Result<(), DisconnectReason> {
        let registry_packets = registry_sync::pack_registries(packet.known_packs())
            .map_err(DisconnectReason::Unsupported)?;
        for packet in registry_packets {
            let body = encode_body(ClientboundRegistryDataPacket::stream_codec(), &packet);
            conn.send_packet(
                ConnectionProtocol::Configuration,
                CLIENTBOUND_REGISTRY_DATA_ID,
                &body,
            )
            .map_err(DisconnectReason::Unsupported)?;
        }
        // `connection.accept(new ClientboundUpdateTagsPacket(
        // TagNetworkSerialization.serializeTagsToNetwork(this.registries)))`.
        let update_tags =
            ClientboundUpdateTagsPacket::new(registry_sync::serialize_tags_to_network());
        let body = encode_body(ClientboundUpdateTagsPacket::stream_codec(), &update_tags);
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_UPDATE_TAGS_ID,
            &body,
        )
        .map_err(DisconnectReason::Unsupported)?;

        self.finish_current_task(SYNCHRONIZE_REGISTRIES_TASK_TYPE, conn)
    }
}

impl std::fmt::Debug for ServerConfigurationPacketListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfigurationPacketListener")
            .field("client_information", &self.client_information)
            .field(
                "current_task",
                &self.current_task.as_ref().map(|t| t.type_id()),
            )
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

use std::collections::{HashSet, VecDeque};

use bytes::{Bytes, BytesMut};

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
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
use rivet_protocol::protocol::configuration::clientbound_finish_configuration::{
    ClientboundFinishConfigurationPacket, stream_codec as finish_configuration_stream_codec,
};
use rivet_protocol::protocol::configuration::clientbound_registry_data::ClientboundRegistryDataPacket;
use rivet_protocol::protocol::configuration::clientbound_select_known_packs::ClientboundSelectKnownPacks;
use rivet_protocol::protocol::configuration::clientbound_update_enabled_features::ClientboundUpdateEnabledFeaturesPacket;
use rivet_protocol::protocol::configuration::serverbound_accept_code_of_conduct::{
    ServerboundAcceptCodeOfConductPacket, stream_codec as accept_code_of_conduct_stream_codec,
};
use rivet_protocol::protocol::configuration::serverbound_finish_configuration::ServerboundFinishConfigurationPacket;
use rivet_protocol::protocol::configuration::serverbound_select_known_packs::ServerboundSelectKnownPacks;
use rivet_registry::Identifier;
use rivet_registry::core::GameProfile;
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
const CLIENTBOUND_FINISH_CONFIGURATION_ID: u32 = 3;
const CLIENTBOUND_REGISTRY_DATA_ID: u32 = 7;
const CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID: u32 = 12;
const CLIENTBOUND_UPDATE_TAGS_ID: u32 = 13;
const CLIENTBOUND_SELECT_KNOWN_PACKS_ID: u32 = 14;

/// `ServerCommonPacketListenerImpl.DISCONNECT_UNEXPECTED_QUERY`.
const DISCONNECT_UNEXPECTED_QUERY: &str = "multiplayer.disconnect.unexpected_query_response";

/// The server's mod name (`MinecraftServer.getServerModName()`); Paper sends the
/// brand in `startConfiguration` via `ClientboundCustomPayloadPacket(Brand)`.
const SERVER_BRAND: &str = "Rivet";

/// `ServerCommonPacketListenerImpl.CUSTOM_REGISTER` — the `minecraft:register`
/// channel-registration payload id (the 0-separated channel list).
const CUSTOM_REGISTER: &str = "register";
/// `ServerCommonPacketListenerImpl.CUSTOM_UNREGISTER`.
const CUSTOM_UNREGISTER: &str = "unregister";
/// `ServerCommonPacketListenerImpl.MINECRAFT_BRAND` — the `minecraft:brand`
/// payload id (Paper); the client's brand is a `utf` string read at max 256.
const MINECRAFT_BRAND: &str = "brand";

/// `Messenger.MAX_CHANNEL_SIZE` — `Integer.getInteger("paper.maxCustomChannelName",
/// Short.MAX_VALUE)`, the max length `validateAndCorrectChannel` permits.
const MAX_CUSTOM_CHANNEL_SIZE: usize = 32767;

/// The disconnect `handleCustomPayload` sends on any payload parse failure:
/// `Component.literal("Invalid custom payload payload!")`. This slice closes the
/// connection deterministically (the observable Java behavior) without a
/// formatted disconnect body, so the reason is only descriptive.
fn invalid_custom_payload() -> DisconnectReason {
    DisconnectReason::Malformed("Invalid custom payload payload!".into())
}

/// `StandardMessenger.validateAndCorrectChannel` — the per-segment validation
/// the plugin bridge applies in `addChannel`/`removeChannel`. Java CORRECTS
/// (accepts) the legacy `BungeeCord` ↔ `bungeecord:main` spellings, then
/// rejects a channel over 32767 chars, without a `:`, or not entirely
/// lowercase — every rejection is an exception the `handleCustomPayload` catch
/// converts to the invalid-payload disconnect. The corrected value has no
/// observable effect here: the tracked channel set is the plugin bridge,
/// deferred (#26), so only acceptance vs rejection matters.
///
/// `readChannelIdentifier` skips empty segments before validating, mirrored by
/// the `is_empty` early return below.
fn validate_register_channel(channel: &[u8]) -> Result<(), DisconnectReason> {
    if channel.is_empty() {
        return Ok(());
    }
    // US-ASCII decoding (`new String(..., US_ASCII)`) maps every byte to one
    // char (non-ASCII bytes become U+FFFD), so byte and char lengths agree and
    // only `A`-`Z` change under `toLowerCase(Locale.ROOT)` — the byte checks
    // mirror the string checks exactly.
    if channel == b"BungeeCord" || channel == b"bungeecord:main" {
        return Ok(());
    }
    if channel.len() > MAX_CUSTOM_CHANNEL_SIZE {
        return Err(invalid_custom_payload());
    }
    if !channel.contains(&b':') {
        return Err(invalid_custom_payload());
    }
    if channel.iter().any(u8::is_ascii_uppercase) {
        return Err(invalid_custom_payload());
    }
    Ok(())
}

/// `ConfigurationTask.Type` id for `SynchronizeRegistriesTask` — queued first;
/// the registry-sync negotiation (`select_known_packs` reply → registry/tag data).
const SYNCHRONIZE_REGISTRIES_TASK_TYPE: &str = "synchronize_registries";
/// `ConfigurationTask.Type` id for `JoinWorldTask` — queued last so the client's
/// `finish_configuration` reply finishes it and the connection hands off to play
/// (issue #96). Rivet bypasses `PrepareSpawnTask`: chunks send via the delivered
/// direct-send path (#100) and the join burst runs tick-side via the play
/// listener (#101 Slice B).
const JOIN_WORLD_TASK_TYPE: &str = "join_world";
/// `ConfigurationTask.Type` id for `ServerCodeOfConductConfigurationTask` —
/// never queued in this slice (`MinecraftServer.getCodeOfConducts()` is
/// `Map.of()`), so `handleAcceptCodeOfConduct`'s `finishCurrentTask` always
/// mismatches and closes exactly like Java's `IllegalStateException`.
const SERVER_CODE_OF_CONDUCT_TASK_TYPE: &str = "server_code_of_conduct";

/// `net.minecraft.server.network.ConfigurationTask` — one deferred step of the
/// configuration phase.
///
/// Java: `ConfigurationTask.java` in `working/Paper`. Each task is started by
/// `ServerConfigurationPacketListenerImpl.startNextTask` with a `Consumer<Packet>`
/// that sends one packet. A task stays `currentTask` until a client response
/// finishes it via `finishCurrentTask` (`SynchronizeRegistriesTask` awaits the
/// `select_known_packs` reply; `JoinWorldTask` awaits `finish_configuration`).
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
        )?;
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_SELECT_KNOWN_PACKS_ID,
            &body,
        )
    }
}

/// `net.minecraft.server.network.config.JoinWorldTask` — the terminal
/// configuration task (issue #96).
///
/// Java: `JoinWorldTask.java` in `working/Paper`. `start` sends the fieldless
/// `ClientboundFinishConfigurationPacket.INSTANCE`; the client's
/// `ServerboundFinishConfigurationPacket` reply finishes the task in
/// `handleConfigurationFinished`, after which the connection hands off to the
/// play state. Paper runs `PrepareSpawnTask` immediately before this task. Rivet
/// instead uses the delivered Moonrise direct-send path (#100), so configuration
/// reaches play before the join burst runs tick-side via the play listener
/// (issue #101 Slice B).
struct JoinWorldTask;

impl ConfigurationTask for JoinWorldTask {
    fn type_id(&self) -> &'static str {
        JOIN_WORLD_TASK_TYPE
    }

    fn start(&mut self, conn: &mut Connection) -> Result<(), String> {
        // `connection.accept(ClientboundFinishConfigurationPacket.INSTANCE)`.
        let body = encode_body(
            finish_configuration_stream_codec(),
            &ClientboundFinishConfigurationPacket,
        )?;
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_FINISH_CONFIGURATION_ID,
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
/// first. The registry-sync task (`SynchronizeRegistriesTask`) and the terminal
/// `JoinWorldTask` are the M1 queue; the spawn-chunk load (`PrepareSpawnTask`,
/// #100) that Paper runs between them is deferred. The queue is
/// [sync, join_world]: the client replies to `select_known_packs`, the server
/// sends the registry/tag data and finishes the sync, `JoinWorldTask` sends
/// `ClientboundFinishConfigurationPacket`, and the client's `finish_configuration`
/// reply finishes it — the connection then hands off to the play state
/// (`ListenerOutcome::Play`), where frames are forwarded to the tick thread and
/// the join burst (`spawnPlayer`) fires tick-side via the play listener
/// (issue #101 Slice B). A `finish_configuration` with any other task current
/// mismatches `finishCurrentTask` exactly like Java and closes.
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
    /// `ServerCommonPacketListenerImpl.clientBrand` — the `minecraft:brand`
    /// payload decoded by `handleCustomPayload` (`readUtf(256)`). Stored on
    /// this listener only: the finish→play handoff
    /// (`set_play_handoff(profile, client_information)`) carries just the
    /// profile + `ClientInformation`, so the brand is dropped at that seam.
    /// Java carries it into play via `CommonListenerCookie`; the play-side
    /// consumer (`Player.getClientBrandName()`, Paper API) is plugin-layer,
    /// deferred with the JVM adapter (#26).
    client_brand: Option<String>,
    /// The authenticated `GameProfile` the login phase built for this connection
    /// (`CommonListenerCookie.profile`, issue #101 Slice B). Carried across the
    /// finish→play handoff so the tick thread can spawn the join burst.
    profile: GameProfile,
}

impl Default for ServerConfigurationPacketListener {
    fn default() -> Self {
        // Java builds the listener from the `CommonListenerCookie`, which always
        // carries the authenticated profile; the `Default` is only for the
        // handoff tests and starts from the offline empty profile.
        ServerConfigurationPacketListener {
            configuration_tasks: VecDeque::new(),
            current_task: None,
            client_information: ClientInformation::create_default(),
            client_brand: None,
            profile: GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ),
        }
    }
}

impl ServerConfigurationPacketListener {
    /// `new ServerConfigurationPacketListenerImpl(server, connection, cookie)` —
    /// the listener for a connection whose login phase authenticated `profile`.
    pub fn new(profile: GameProfile) -> Self {
        ServerConfigurationPacketListener {
            configuration_tasks: VecDeque::new(),
            current_task: None,
            client_information: ClientInformation::create_default(),
            client_brand: None,
            profile,
        }
    }

    /// `startConfiguration()` — Paper sends the brand first, then
    /// `update_enabled_features`, then queues the configuration tasks and starts
    /// the first (`startNextTask`).
    ///
    /// This slice queues the registry sync + `JoinWorldTask` (the finish→play
    /// seam); `PrepareSpawnTask` (#100) is deferred. The join burst
    /// (`spawnPlayer`) runs tick-side via the play listener (issue #101 Slice B).
    pub fn start_configuration(&mut self, conn: &mut Connection) -> Result<(), String> {
        // `send(new ClientboundCustomPayloadPacket(new BrandPayload(server
        // .getServerModName())))`.
        let body = encode_body(
            ClientboundCustomPayloadPacket::config_stream_codec(),
            &ClientboundCustomPayloadPacket::new(CustomPacketPayload::Brand(BrandPayload::new(
                SERVER_BRAND.to_string(),
            ))),
        )?;
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
        )?;
        conn.send_packet(
            ConnectionProtocol::Configuration,
            CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID,
            &body,
        )?;

        // `this.synchronizeRegistriesTask = new SynchronizeRegistriesTask(...);
        // this.configurationTasks.add(this.synchronizeRegistriesTask);
        // this.addOptionalTasks(); this.configurationTasks.add(
        //   new PaperConfigurationTask(this)); this.returnToWorld();`
        // `returnToWorld` queues `PrepareSpawnTask` (the spawn-chunk load that
        // gates `JoinWorldTask` in Paper) then `JoinWorldTask`. It is not ported:
        // the M1 superflat chunks are sent tick-side via the Moonrise direct-send
        // path (#100), so the queue is [sync, join_world]: configuration completes
        // and the connection reaches the play handoff; the join burst fires
        // tick-side via the play listener (issue #101 Slice B), so `spawnPlayer`
        // needs no configuration-phase task here.
        // `addOptionalTasks` queues nothing in this slice: the
        // `ServerCodeOfConductConfigurationTask` and
        // `ServerResourcePackConfigurationTask` tasks (and the Paper
        // `AsyncPlayerConnectionConfigureEvent` task) are optional and this
        // Paper's `MinecraftServer.getCodeOfConducts()`/`getServerResourcePack()`
        // are empty (`Map.of()` / `Optional.empty()`), so the conditionals in
        // Paper's `addOptionalTasks` never add a task (the CoC event listener
        // count is plugin-layer, deferred with the JVM adapter #26).
        self.configuration_tasks
            .push_back(Box::new(SynchronizeRegistriesTask::new()));
        self.configuration_tasks.push_back(Box::new(JoinWorldTask));
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

    /// `ServerCommonPacketListenerImpl.handleCustomPayload(packet)` (Paper fork).
    ///
    /// Java:
    /// ```java
    /// if (!(packet.payload() instanceof DiscardedPayload discardedPayload)) {
    ///     return; // never happens here — the serverbound codec always discards
    /// }
    /// Identifier identifier = packet.payload().type().id();
    /// byte[] data = discardedPayload.data();
    /// try {
    ///     boolean registerChannel = CUSTOM_REGISTER.equals(identifier);
    ///     if (registerChannel || CUSTOM_UNREGISTER.equals(identifier)) {
    ///         // strings separated by zeros
    ///         ...
    ///         return;
    ///     }
    ///     if (identifier.equals(MINECRAFT_BRAND)) {
    ///         this.clientBrand = new FriendlyByteBuf(wrappedBuffer(data)).readUtf(256);
    ///     }
    ///     this.cserver.getMessenger().dispatchIncomingMessage(...);
    /// } catch (Exception e) {
    ///     LOGGER.error(...);
    ///     this.disconnect(Component.literal("Invalid custom payload payload!"), INVALID_PAYLOAD);
    /// }
    /// ```
    ///
    /// The register/unregister payload is a list of US-ASCII channel ids
    /// separated by `\0` (no length prefixes). Paper validates each channel
    /// against `StandardMessenger.validateAndCorrectChannel` (via the plugin
    /// bridge's `addChannel`/`removeChannel`) and fires register/unregister
    /// events — the only observable Java effect besides the connection
    /// state is that an invalid channel throws and disconnects, so this slice
    /// validates the same way and discards the corrected value. The bridge
    /// itself (the 128-channel limit and the events) is plugin-layer, deferred
    /// with the JVM adapter (#26). Empty segments (leading/consecutive/trailing
    /// `\0`) are skipped by `readChannelIdentifier` before validation.
    /// The brand decode is `readUtf(256)` — a truncated/oversize brand throws
    /// `DecoderException` (empty data throws netty `IndexOutOfBoundsException`),
    /// caught by the try/catch; here the panic-equivalent
    /// (`FriendlyByteBuf::read_utf_max` on the raw bytes) is caught and mapped
    /// to the same disconnect.
    ///
    fn handle_custom_payload(
        &mut self,
        packet: &ServerboundCustomPayloadPacket,
    ) -> Result<(), DisconnectReason> {
        let identifier = packet.payload().type_id();
        let data = match packet.payload() {
            CustomPacketPayload::Discarded(d) => d.data().to_vec(),
            CustomPacketPayload::Brand(_) => {
                // Unreachable: the serverbound codec's empty known-types list
                // never decodes to `Brand`. Guarded so a future known-type change
                // is a no-op rather than a panic.
                return Ok(());
            }
        };

        // `CUSTOM_REGISTER.equals(identifier)` compares the full identifier, so
        // a hostile `foo:register` payload does NOT match — `with_default_namespace`
        // is the `minecraft:register` Java compares against.
        if identifier == Identifier::with_default_namespace(CUSTOM_REGISTER)
            || identifier == Identifier::with_default_namespace(CUSTOM_UNREGISTER)
        {
            // `readChannelIdentifier` over the 0-separated channel list. The
            // validated channel is discarded (there is no plugin bridge yet): the
            // only observable Java effect here is that an invalid channel throws
            // and disconnects — the plugin surface (`bridge.addChannel`/
            // `removeChannel`, the 128-channel limit, the register/unregister
            // events) is deferred with the JVM adapter (#26).
            // RivetTodo(#26): the plugin bridge — `addChannel`/`removeChannel`,
            // the 128-channel limit, and the
            // `PlayerRegisterChannelEvent`/`PlayerUnregisterChannelEvent`.
            let mut start = 0;
            for (i, b) in data.iter().enumerate() {
                if *b == 0 {
                    validate_register_channel(&data[start..i])?;
                    start = i + 1;
                }
            }
            validate_register_channel(&data[start..])?;
            return Ok(());
        }

        if identifier == Identifier::with_default_namespace(MINECRAFT_BRAND) {
            // `new FriendlyByteBuf(wrappedBuffer(data)).readUtf(256)` — Java
            // throws `DecoderException` for a truncated or over-max utf (from
            // `Utf8String.read`'s available-bytes and decoded-length checks) and
            // netty `IndexOutOfBoundsException` for an empty buffer (`VarInt.read`
            // on no bytes); every one is caught by `handleCustomPayload`'s
            // `catch (Exception)` → the invalid-payload disconnect. `read_utf_max`
            // panics on the same inputs, so the panic is caught and mapped here.
            let brand = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                FriendlyByteBuf::new(BytesMut::from(data.as_slice())).read_utf_max(256)
            }))
            .map_err(|_| invalid_custom_payload())?;
            self.client_brand = Some(brand);
        }

        // `this.cserver.getMessenger().dispatchIncomingMessage(...)` — the
        // plugin-messaging dispatch (CraftBukkit's `CraftServer` messenger) is
        // plugin-layer, deferred with the JVM adapter (#26).
        // RivetTodo(#26): `CraftServer.getMessenger().dispatchIncomingMessage`
        // for plugin channels.
        Ok(())
    }

    /// `ServerCommonPacketListenerImpl.handleResourcePackResponse(packet)` —
    /// the common-impl resource-pack-required kick plus the config listener
    /// override's `ServerResourcePackConfigurationTask` finish.
    ///
    /// Java: `if (action() == DECLINED && server.isResourcePackRequired())
    /// disconnect("multiplayer.requiredTexturePrompt.disconnect")` — never fires
    /// here because `getServerResourcePack()` is `Optional.empty()`. The
    /// adventure pack callbacks (`packCallbacks`, `ResourcePackCallback`) are
    /// plugin-layer, deferred (#26). The terminal task-finish branch
    /// (`action().isTerminal() && id == serverResourcePack.id()`) can never match
    /// with no pushed pack, so nothing finishes — exactly Java's behavior with an
    /// empty pack.
    fn handle_resource_pack_response(
        &self,
        packet: &ServerboundResourcePackPacket,
    ) -> Result<(), DisconnectReason> {
        // RivetTodo(#26): the adventure `ResourcePackCallback` callbacks, which
        // need the adventure audience wiring. The configuration-task finish is
        // unreachable without a pushed resource pack, as described above.
        let _ = packet;
        Ok(())
    }

    /// `ServerCommonPacketListenerImpl.handleCustomClickAction(packet)` —
    /// `server.handleCustomClickAction(id, payload)` which only debug-logs
    /// `"Received custom click action {} with payload {}"`, then the Paper
    /// `PaperPlayerCustomClickEvent` + dialog/adventure click-callback managers
    /// (plugin-layer, deferred #26).
    fn handle_custom_click_action(&self, packet: &ServerboundCustomClickActionPacket) {
        // `MinecraftServer.handleCustomClickAction` — the debug log.
        tracing::debug!(
            "Received custom click action {} with payload {}",
            packet.id(),
            packet
                .payload()
                .map_or_else(|| "null".to_string(), |t| t.to_string())
        );
        // RivetTodo(#26): the `PaperPlayerCustomClickEvent` and the dialog/
        // adventure click-callback managers (`ClickCallbackProviderImpl`), which
        // need the adventure audience + callback registry.
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
                // .handleCustomPayload` (Paper fork): decode to
                // `DiscardedPayload`, then register/unregister/brand handling.
                // CraftBukkit's empty known-types list decodes every serverbound
                // payload as `DiscardedPayload`, so `payload().type().id()` is
                // the raw id (Paper's `identifier`).
                let packet: ServerboundCustomPayloadPacket =
                    decode_packet(frame, ServerboundCustomPayloadPacket::stream_codec())?;
                self.handle_custom_payload(&packet)?;
                Ok(ListenerOutcome::Keep)
            }
            KEEP_ALIVE_PACKET_ID => {
                // `handleKeepAlive` — Paper's keepalive challenge tracking. The
                // configuration-phase challenge lifecycle is deferred (#283); a
                // serverbound keepalive with no pending challenge would disconnect
                // in Java ("without matching challenge").
                let packet: ServerboundKeepAlivePacket =
                    decode_packet(frame, ServerboundKeepAlivePacket::stream_codec())?;
                let _ = packet;
                // RivetTodo(#283): the configuration listener does not yet own a
                // `KeepaliveState`, so periodic challenges, reply matching, and
                // timeout disconnects are not driven here. The play listener's
                // delivered keepalive wiring (#157) provides the state-machine
                // pattern; configuration still needs its own tick source and
                // lifecycle integration.
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
                // (the resource-pack-required kick, never fired here — no pack is
                // required, see below) plus the configuration listener override's
                // `finishCurrentTask(ServerResourcePackConfigurationTask.TYPE)`.
                // `MinecraftServer.getServerResourcePack()` is `Optional.empty()`
                // in this Paper, so no pack is ever pushed and no
                // `ServerResourcePackConfigurationTask` is current — the terminal
                // task-finish branch (`packet.id() == serverResourcePack.id()`)
                // can never match, exactly like Java with an empty pack.
                let packet: ServerboundResourcePackPacket =
                    decode_packet(frame, ServerboundResourcePackPacket::stream_codec())?;
                self.handle_resource_pack_response(&packet)?;
                Ok(ListenerOutcome::Keep)
            }
            CUSTOM_CLICK_ACTION_PACKET_ID => {
                // `handleCustomClickAction` — `server.handleCustomClickAction`
                // (a debug log) plus the Paper dialog/adventure click callbacks
                // (plugin-layer, deferred with the JVM adapter #26).
                let packet: ServerboundCustomClickActionPacket =
                    decode_packet(frame, ServerboundCustomClickActionPacket::stream_codec())?;
                self.handle_custom_click_action(&packet);
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
                // `handleConfigurationFinished` — finish the current
                // `JoinWorldTask` (whose `start` sent
                // `ClientboundFinishConfigurationPacket`), then swap the
                // outbound protocol to play and hand the connection off to the
                // tick thread. The duplicate-login / can-login gates and
                // `prepareSpawnTask.spawnPlayer(...)` (the join burst) run
                // tick-side via the play listener (issue #101 Slice B), which
                // consumes the forwarded frames there.
                // RivetTodo(#101): the duplicate-login / canPlayerLogin gates —
                // the can-login checks run on the tick side before the session
                // spawns in `PlayerSessionManager::spawn_session`.
                let _: ServerboundFinishConfigurationPacket = decode_packet(
                    frame,
                    rivet_protocol::protocol::configuration::serverbound_finish_configuration::stream_codec(),
                )?;
                self.finish_current_task(JOIN_WORLD_TASK_TYPE, conn)?;
                conn.set_outbound_protocol(ConnectionProtocol::Play);
                // Stash the authenticated profile + `ClientInformation` on the
                // connection; the per-connection task forwards them to the tick
                // thread as the EnterPlay handoff (issue #101 Slice B) the join
                // burst needs.
                conn.set_play_handoff(self.profile.clone(), self.client_information.clone());
                Ok(ListenerOutcome::Play)
            }
            ACCEPT_CODE_OF_CONDUCT_PACKET_ID => {
                // `handleAcceptCodeOfConduct` → `finishCurrentTask(
                // ServerCodeOfConductConfigurationTask.TYPE)`. No code-of-conduct
                // task is ever queued (`getCodeOfConducts()` is `Map.of()`), so
                // `finish_current_task` always mismatches — Java's
                // `IllegalStateException("Unexpected request for task finish, current
                // task: ..., requested: server_code_of_conduct")` — surfaced as a
                // deterministic Malformed close, exactly like Java.
                let _: ServerboundAcceptCodeOfConductPacket =
                    decode_packet(frame, accept_code_of_conduct_stream_codec())?;
                self.finish_current_task(SERVER_CODE_OF_CONDUCT_TASK_TYPE, conn)?;
                // Unreachable: no CoC task is ever current, so `finish_current_task`
                // always errors above. Kept for the type — the Ok path mirrors the
                // other task-finishing branches.
                Ok(ListenerOutcome::Keep)
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
    /// The M1 vanilla client accepts `minecraft:core:26.2`, so every vanilla
    /// element is skipped. A client that does NOT accept the pack is served the
    /// deterministic pre-baked full NBT: the canonical join capture's per-element
    /// payloads, decoded back into `Tag`s and re-encoded — see
    /// [`registry_sync::pack_registries`].
    fn handle_sync_response(
        &mut self,
        packet: &ServerboundSelectKnownPacks,
        conn: &mut Connection,
    ) -> Result<(), DisconnectReason> {
        let registry_packets = registry_sync::pack_registries(packet.known_packs())
            .map_err(DisconnectReason::Unsupported)?;
        for packet in registry_packets {
            let body = encode_body(ClientboundRegistryDataPacket::stream_codec(), &packet)
                .map_err(DisconnectReason::Unsupported)?;
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
        let body = encode_body(ClientboundUpdateTagsPacket::stream_codec(), &update_tags)
            .map_err(DisconnectReason::Unsupported)?;
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
        // (`ClientInformation.createDefault()`); this slice starts there. The
        // listener carries the authenticated profile from the login phase.
        let listener = ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
            rivet_util::mth::Uuid { most: 0, least: 0 },
            String::new(),
        ));
        assert_eq!(
            listener.client_information,
            ClientInformation::create_default()
        );
    }

    /// Protocol VarInt encode (the wire-frame builder for the handoff tests).
    fn varint(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut v = value as u32;
        loop {
            let mut byte = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if v == 0 {
                break;
            }
        }
        out
    }

    /// A throwaway `Connection` in the configuration outbound state (the write
    /// half never writes unless `flush_out` is called, which these tests do not).
    async fn config_connection() -> Connection {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { listener.accept().await.unwrap() });
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (server_sock, _) = server.await.unwrap();
        let (_read, write) = server_sock.into_split();
        let mut conn = Connection::new(
            super::super::connection_id::ConnectionId(1),
            addr,
            std::sync::Arc::new(crate::server::ServerConfig::default()),
            std::sync::Arc::new(crate::server::tick::shutdown::Shutdown::new()),
            write,
            crate::server::tick::channels::InboundDrained::new(),
        );
        conn.set_outbound_protocol(ConnectionProtocol::Configuration);
        conn
    }

    #[tokio::test]
    async fn start_configuration_queues_sync_then_join_world() {
        // The M1 queue is [synchronize_registries, join_world]: the sync starts
        // first; `PrepareSpawnTask` (#100) is deferred. Finishing the sync starts
        // `JoinWorldTask`, whose `start` sends the finish_configuration packet.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        assert_eq!(
            listener.current_task.as_ref().map(|t| t.type_id()),
            Some(SYNCHRONIZE_REGISTRIES_TASK_TYPE),
            "the registry sync starts first"
        );
        assert_eq!(listener.configuration_tasks.len(), 1);
        assert_eq!(
            listener.configuration_tasks.front().map(|t| t.type_id()),
            Some(JOIN_WORLD_TASK_TYPE),
            "JoinWorldTask is queued after the sync"
        );

        // Finish the sync: JoinWorldTask becomes current and sends its packet.
        listener
            .finish_current_task(SYNCHRONIZE_REGISTRIES_TASK_TYPE, &mut conn)
            .unwrap();
        assert_eq!(
            listener.current_task.as_ref().map(|t| t.type_id()),
            Some(JOIN_WORLD_TASK_TYPE)
        );
        assert!(listener.configuration_tasks.is_empty());
        // The finish_configuration frame was queued (the `JoinWorldTask.start`).
        // The test connection has no compression stage, so the frame is the
        // plain VarInt21 `[len 1][id 3]` (no declaredLength prefix).
        assert_eq!(
            &conn.outbound_bytes()[conn.outbound_bytes().len() - 2..],
            &[0x01, 0x03]
        );
    }

    #[tokio::test]
    async fn finish_configuration_frame_hands_off_to_play() {
        // `handleConfigurationFinished` finishes `JoinWorldTask`, flips the
        // outbound protocol to play, and reports `ListenerOutcome::Play` — the
        // seam the per-connection task uses to start forwarding frames to the
        // tick thread.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();
        listener
            .finish_current_task(SYNCHRONIZE_REGISTRIES_TASK_TYPE, &mut conn)
            .unwrap();

        // The client's `finish_configuration` reply: `[id 3]` (0-byte body) —
        // the decompressed packet payload `handle_frame` receives (the VarInt21
        // frame + compression header were already stripped).
        let frame = Bytes::from(varint(3));
        let outcome = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap();
        assert!(matches!(outcome, ListenerOutcome::Play));
        // The outbound protocol flipped so the play-state path can send.
        assert_eq!(conn.outbound_protocol(), Some(ConnectionProtocol::Play));
        assert!(
            listener.current_task.is_none(),
            "JoinWorldTask finished and the queue is empty"
        );
    }

    #[tokio::test]
    async fn finish_configuration_wrong_task_closes() {
        // A `finish_configuration` while the registry sync is current mismatches
        // `finishCurrentTask(JoinWorldTask.TYPE)` — Java's
        // `IllegalStateException` — surfaced as a deterministic Malformed close.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(varint(3));
        let err = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap_err();
        assert!(
            matches!(err, DisconnectReason::Malformed(ref m) if m.contains("Unexpected request for task finish"))
        );
    }

    /// Frame a serverbound `custom_payload` packet (`[id 2] ++ [identifier] ++
    /// [raw payload bytes]`), the decompressed body `handle_frame` receives.
    fn custom_payload_frame(id: &str, data: &[u8]) -> Vec<u8> {
        let mut body = varint(CUSTOM_PAYLOAD_PACKET_ID);
        let id_bytes = id.as_bytes();
        body.extend_from_slice(&varint(id_bytes.len() as i32));
        body.extend_from_slice(id_bytes);
        body.extend_from_slice(data);
        body
    }

    #[tokio::test]
    async fn custom_payload_register_keeps_open() {
        // `minecraft:register` with a 0-separated channel list (`bungeecord:main`
        // then `my:plugin`) — every segment is valid, so the connection stays
        // open (the plugin bridge is deferred, #26; `bungeecord:main` corrects
        // to `BungeeCord` but the corrected value is discarded).
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let data = b"bungeecord:main\x00my:plugin";
        let frame = Bytes::from(custom_payload_frame("minecraft:register", data));
        let outcome = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap();
        assert!(matches!(outcome, ListenerOutcome::Keep));
        assert!(listener.client_brand.is_none());
    }

    #[tokio::test]
    async fn custom_payload_unregister_keeps_open() {
        // `minecraft:unregister` — same validation path; the trailing empty
        // segment after `\0` is skipped by `readChannelIdentifier`, and the valid
        // `a:one` segment keeps the connection open.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(custom_payload_frame("minecraft:unregister", b"a:one\x00"));
        let outcome = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap();
        assert!(matches!(outcome, ListenerOutcome::Keep));
    }

    #[tokio::test]
    async fn custom_payload_brand_sets_client_brand() {
        // `minecraft:brand` data is a `utf` string (`\x05Paper`); the handler
        // stores it as the client brand.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(custom_payload_frame("minecraft:brand", b"\x05Paper"));
        listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap();
        assert_eq!(listener.client_brand.as_deref(), Some("Paper"));
    }

    #[tokio::test]
    async fn custom_payload_brand_truncated_closes_invalid_payload() {
        // A brand utf that declares 5 bytes but carries 2 throws Java's
        // `DecoderException("Not enough bytes in buffer, ...")` from
        // `Utf8String.read` → the catch block's `"Invalid custom payload
        // payload!"` disconnect. The panic is caught here and surfaced as that
        // Malformed reason.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(custom_payload_frame("minecraft:brand", b"\x05Pa"));
        let err = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap_err();
        assert_eq!(
            err,
            DisconnectReason::Malformed("Invalid custom payload payload!".into())
        );
    }

    #[tokio::test]
    async fn custom_payload_brand_oversize_closes_invalid_payload() {
        // A brand whose decoded length exceeds the 256 `readUtf(256)` max throws
        // Java's `DecoderException("The received string length is longer than
        // maximum allowed ...")` — a different `Utf8String.read` check than the
        // truncated case but the same shared disconnect. (257 ASCII 'a' units,
        // length varint `0x81 0x02`.)
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let mut data = vec![0x81u8, 0x02]; // varint 257
        data.extend(vec![b'a'; 257]);
        let frame = Bytes::from(custom_payload_frame("minecraft:brand", &data));
        let err = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap_err();
        assert_eq!(
            err,
            DisconnectReason::Malformed("Invalid custom payload payload!".into())
        );
    }

    #[tokio::test]
    async fn custom_payload_brand_empty_data_closes_invalid_payload() {
        // An empty brand data buffer — `VarInt.read` on zero bytes throws
        // Java's netty `IndexOutOfBoundsException` (the one case that is NOT a
        // `DecoderException`), still caught by `catch (Exception)` → the same
        // disconnect. Here the varint read on an empty buffer panics.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(custom_payload_frame("minecraft:brand", b""));
        let err = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap_err();
        assert_eq!(
            err,
            DisconnectReason::Malformed("Invalid custom payload payload!".into())
        );
    }

    /// Drive a `minecraft:register`/`minecraft:unregister` payload and return
    /// whether the connection stayed open. The channel-validation behavior is
    /// identical for both ids (Paper's `readChannelIdentifier` uses the same
    /// `validateAndCorrectChannel` in `addChannel` and `removeChannel`).
    async fn register_payload_keeps_open(id: &str, data: &[u8]) -> bool {
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(custom_payload_frame(id, data));
        matches!(
            listener.handle_frame(frame, &mut conn, &crate::server::ServerConfig::default()),
            Ok(ListenerOutcome::Keep)
        )
    }

    #[tokio::test]
    async fn register_invalid_channel_closes_invalid_payload() {
        // A `minecraft:register` channel with an uppercase segment is rejected by
        // `validateAndCorrectChannel` (`"Channel must be entirely lowercase"` →
        // `IllegalArgumentException`), which `handleCustomPayload` converts to
        // the invalid-payload disconnect.
        assert!(!register_payload_keeps_open("minecraft:register", b"Foo:Bar").await);
    }

    #[tokio::test]
    async fn register_no_separator_closes_invalid_payload() {
        // A channel without a `:` separator is rejected
        // (`"Channel must contain : separator"` → `IllegalArgumentException`).
        assert!(!register_payload_keeps_open("minecraft:register", b"no-separator").await);
    }

    #[tokio::test]
    async fn register_oversize_payload_rejected_at_decode() {
        // A 32770-byte channel-list payload exceeds the `DiscardedPayload`
        // 32767-byte decode cap, so the packet is rejected at DECODE — exactly
        // like Java's `DiscardedPayload.streamCodec(identifier, 32767)`, which
        // throws before `handleCustomPayload` runs. The `> 32767` channel
        // length check in `validateAndCorrectChannel` is therefore unreachable
        // through the real payload path (a single segment is bounded by the
        // payload size) and is exercised directly by
        // `validate_register_channel_oversize` below. Either way the connection
        // closes.
        let mut oversize = vec![b'a'; 32768];
        oversize.extend_from_slice(b":y");
        assert!(!register_payload_keeps_open("minecraft:register", &oversize).await);
    }

    #[test]
    fn validate_register_channel_oversize() {
        // Mirror of `validateAndCorrectChannel`'s max-length branch
        // (`ChannelNameTooLongException`, > 32767 chars). Unreachable through
        // `handleCustomPayload` (the payload decode cap bounds a segment), kept
        // for exact method fidelity. The boundary is exact: 32767 chars passes,
        // 32770 chars (`32768 'a'` + `:y`) throws on length before the other
        // checks.
        let mut oversize = vec![b'a'; 32768];
        oversize.extend_from_slice(b":y");
        assert!(validate_register_channel(&oversize).is_err());
        let mut at_limit = vec![b'a'; 32765];
        at_limit.extend_from_slice(b":y");
        assert!(validate_register_channel(&at_limit).is_ok());
    }

    #[tokio::test]
    async fn unregister_invalid_channel_closes_invalid_payload() {
        // `minecraft:unregister` validates through the same path — an invalid
        // channel closes the connection just like register.
        assert!(!register_payload_keeps_open("minecraft:unregister", b"Foo:Bar").await);
    }

    #[tokio::test]
    async fn register_bungeecord_correction_keeps_open() {
        // `BungeeCord` and `bungeecord:main` are CORRECTED (accepted) by
        // `validateAndCorrectChannel`, not rejected — the corrected value has no
        // observable effect here (the bridge is deferred, #26), so the
        // connection stays open.
        assert!(register_payload_keeps_open("minecraft:register", b"BungeeCord").await);
        assert!(register_payload_keeps_open("minecraft:register", b"bungeecord:main").await);
    }

    #[tokio::test]
    async fn register_empty_segments_are_skipped() {
        // Empty segments (leading/consecutive/trailing `\0`) are skipped by
        // `readChannelIdentifier` before validation, so a payload of only empty
        // segments or with an empty trailing segment stays open. The valid
        // `a:one`/`b:two` segments validate normally.
        assert!(register_payload_keeps_open("minecraft:register", b"").await);
        assert!(register_payload_keeps_open("minecraft:register", b"\x00").await);
        assert!(
            register_payload_keeps_open("minecraft:register", b"\x00a:one\x00\x00b:two\x00").await
        );
    }

    #[tokio::test]
    async fn register_mixed_valid_then_invalid_closes() {
        // Validation is per segment: a valid channel before an invalid one still
        // throws on the invalid segment and closes.
        assert!(!register_payload_keeps_open("minecraft:register", b"a:ok\x00Foo:Bar").await);
    }

    #[tokio::test]
    async fn custom_payload_other_namespace_register_is_ignored() {
        // Java's `CUSTOM_REGISTER.equals(identifier)` compares the FULL
        // identifier, so a hostile `foo:register` payload is neither a register
        // nor a brand — it falls through to the plugin dispatch (deferred, #26),
        // no channel validation runs, and `client_brand` stays unset. The invalid
        // channel bytes make this load-bearing: a namespace-insensitive match
        // would reject them.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let frame = Bytes::from(custom_payload_frame("foo:register", b"Foo:Bar"));
        listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap();
        assert!(listener.client_brand.is_none());
    }

    #[tokio::test]
    async fn resource_pack_response_keeps_open() {
        // No pack is ever pushed (`getServerResourcePack()` is empty), so a
        // DECLINED terminal response must NOT close (the required-kick never
        // fires) and the task-finish branch can never match — the connection
        // stays open. Body: `[uuid 16][action ordinal varint]`.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        let mut body = varint(RESOURCE_PACK_PACKET_ID);
        body.extend_from_slice(&[0u8; 16]);
        body.push(1); // Action.DECLINED ordinal
        let outcome = listener
            .handle_frame(
                Bytes::from(body),
                &mut conn,
                &crate::server::ServerConfig::default(),
            )
            .unwrap();
        assert!(matches!(outcome, ListenerOutcome::Keep));
        // The current task is unchanged (the sync) — nothing was finished.
        assert_eq!(
            listener.current_task.as_ref().map(|t| t.type_id()),
            Some(SYNCHRONIZE_REGISTRIES_TASK_TYPE)
        );
    }

    #[tokio::test]
    async fn accept_code_of_conduct_always_mismatches_closes() {
        // No CoC task is ever queued, so `finishCurrentTask("server_code_of_conduct")`
        // always mismatches — Java's `IllegalStateException` — a Malformed close.
        let mut conn = config_connection().await;
        let mut listener =
            ServerConfigurationPacketListener::new(GameProfile::new_without_properties(
                rivet_util::mth::Uuid { most: 0, least: 0 },
                String::new(),
            ));
        listener.start_configuration(&mut conn).unwrap();

        // Fieldless body: `[id 9]` with no trailing bytes.
        let frame = Bytes::from(varint(ACCEPT_CODE_OF_CONDUCT_PACKET_ID));
        let err = listener
            .handle_frame(frame, &mut conn, &crate::server::ServerConfig::default())
            .unwrap_err();
        assert_eq!(
            err,
            DisconnectReason::Malformed(
                "Unexpected request for task finish, current task: synchronize_registries, requested: server_code_of_conduct"
                    .into()
            )
        );
    }
}

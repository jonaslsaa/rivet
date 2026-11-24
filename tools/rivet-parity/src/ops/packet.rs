//! `packet.reencode`: the Rust twin of the reference oracle's op of that name.
//!
//! A request names a protocol phase, a direction and a network packet id plus
//! the packet body (the bytes after the id varint) as hex. Paper looks the
//! packet up in its real `ProtocolInfo`, decodes the body, requires the body to
//! be fully consumed (`PacketDecoder`'s "larger than I expected"), re-encodes
//! and answers the packet's type name and the re-encoded body, or "rejected".
//!
//! The Rust side does the same with the ported packet codecs. Every ported
//! packet struct has one entry in [`entries`], keyed by phase, direction and
//! the vanilla packet name; the network id comes from the generated
//! `PacketReport` tables, so the id <-> name mapping is itself compared with
//! Paper (the `name` result field). Only ported (phase, direction, id) triples
//! have a handler: an id Paper registers but Rivet has not ported answers
//! `Unavailable`, never a silent match, and the corpus does not contain such
//! ids.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::registries::{self, DimensionType, WorldClock};
use rivet_registry::{Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey};
use serde_json::{Map, Value, json};

use super::compare::Outcome;

/// One ported packet codec: the handler for one (phase, direction, packet).
pub struct Entry {
    pub state: &'static str,
    pub direction: &'static str,
    /// The vanilla packet name, `minecraft:...`.
    pub name: &'static str,
    /// The Rust struct this entry verifies (the scoreboard subject).
    pub subject: &'static str,
    run: fn(&[u8]) -> Result<Vec<u8>, String>,
}

impl Entry {
    /// Decode `body` and re-encode it; any error is "rejected".
    pub fn reencode(&self, body: &[u8]) -> Result<Vec<u8>, String> {
        (self.run)(body)
    }
}

/// Decode with `codec` over a plain buffer, require full consumption, re-encode.
fn plain<V: 'static>(
    codec: StreamCodec<FriendlyByteBuf, V>,
    body: &[u8],
) -> Result<Vec<u8>, String> {
    let mut input = FriendlyByteBuf::new(BytesMut::from(body));
    let value = codec.decode(&mut input).map_err(|e| e.message)?;
    if input.readable_bytes() > 0 {
        return Err(format!("{} bytes extra", input.readable_bytes()));
    }
    let mut output = FriendlyByteBuf::new(BytesMut::new());
    codec.encode(&mut output, &value).map_err(|e| e.message)?;
    Ok(output.into_inner().to_vec())
}

/// As [`plain`], over a registry-aware buffer carrying the oracle's registries.
fn with_registries<V: 'static>(
    codec: StreamCodec<RegistryFriendlyByteBuf, V>,
    body: &[u8],
) -> Result<Vec<u8>, String> {
    let access = registry_access();
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(body), access.clone());
    let value = codec.decode(&mut input).map_err(|e| e.message)?;
    if input.readable_bytes() > 0 {
        return Err(format!("{} bytes extra", input.readable_bytes()));
    }
    let mut output = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
    codec.encode(&mut output, &value).map_err(|e| e.message)?;
    Ok(output.into_inner().to_vec())
}

/// The registries the registry-aware packet codecs resolve holders through:
/// the elements of `dimension_type` and `world_clock` in the vanilla registry
/// id order (`SYNCHRONIZED_REGISTRIES`, the order Paper's frozen registries
/// hold them in) and the 40 built-in `attribute` entries (`Attributes.java`
/// registers 40), as unit-valued placeholders — the codecs only ever look up
/// holders by id. One access per thread, so a holder decoded through it
/// re-encodes through the same registry identity.
fn registry_access() -> RegistryAccess {
    thread_local! {
        static ACCESS: RegistryAccess = build_registry_access();
    }
    ACCESS.with(RegistryAccess::clone)
}

fn build_registry_access() -> RegistryAccess {
    use rivet_registry::generated::synchronized::SYNCHRONIZED_REGISTRIES;
    let names = |registry: &str| -> &'static [&'static str] {
        SYNCHRONIZED_REGISTRIES
            .iter()
            .find(|(key, _)| *key == registry)
            .map(|(_, names)| *names)
            .unwrap_or_else(|| panic!("no synchronized registry {registry}"))
    };
    let mut dimension_types = RegistryBuilder::new(&*registries::DIMENSION_TYPE);
    for name in names("minecraft:dimension_type") {
        dimension_types.register(
            &ResourceKey::create(&*registries::DIMENSION_TYPE, Identifier::parse(name)),
            std::sync::Arc::new(DimensionType),
            RegistrationInfo::BUILT_IN,
        );
    }
    let mut clocks = RegistryBuilder::new(&*registries::WORLD_CLOCK);
    for name in names("minecraft:world_clock") {
        clocks.register(
            &ResourceKey::create(&*registries::WORLD_CLOCK, Identifier::parse(name)),
            std::sync::Arc::new(WorldClock),
            RegistrationInfo::BUILT_IN,
        );
    }
    let mut attributes = RegistryBuilder::new(&*registries::ATTRIBUTE);
    for index in 0..40 {
        attributes.register(
            &ResourceKey::create(
                &*registries::ATTRIBUTE,
                Identifier::with_default_namespace(&format!("attr_{index}")),
            ),
            std::sync::Arc::new(registries::Attribute),
            RegistrationInfo::BUILT_IN,
        );
    }
    RegistryAccess::from_pairs(vec![
        (
            ResourceKey::create_registry_key(registries::ATTRIBUTE.identifier().clone()),
            Box::new(attributes.freeze()),
        ),
        (
            ResourceKey::create_registry_key(registries::DIMENSION_TYPE.identifier().clone()),
            Box::new(dimension_types.freeze()),
        ),
        (
            ResourceKey::create_registry_key(registries::WORLD_CLOCK.identifier().clone()),
            Box::new(clocks.freeze()),
        ),
    ])
}

macro_rules! entry {
    ($state:literal, $dir:literal, $name:literal, $subject:literal, $codec:expr, $run:ident) => {
        Entry {
            state: $state,
            direction: $dir,
            name: $name,
            subject: $subject,
            run: |body| $run($codec, body),
        }
    };
}

/// Every ported packet codec, one entry per (phase, direction, packet).
///
/// Not listed: the packets whose codec is a `blocked:` stub (`remove_entities`,
/// `teleport_entity`, `set_passengers`: the body panics "codec not ported"),
/// the wire-less `ClientboundBundlePacket`, and `ClientboundBundleDelimiterPacket`
/// (no Rust codec value yet). They are not ported, so nothing is compared.
pub fn entries() -> Vec<Entry> {
    use rivet_protocol::game as g;
    use rivet_protocol::protocol::common as c;
    use rivet_protocol::protocol::configuration as cfg;
    use rivet_protocol::protocol::cookie as ck;
    use rivet_protocol::protocol::game as p;
    use rivet_protocol::protocol::login as l;
    use rivet_protocol::protocol::ping as pg;
    use rivet_protocol::protocol::status as s;
    vec![
        // status
        entry!("status", "serverbound", "minecraft:status_request", "ServerboundStatusRequestPacket",
            s::serverbound_status_request_packet::ServerboundStatusRequestPacket::stream_codec(), plain),
        entry!("status", "serverbound", "minecraft:ping_request", "ServerboundPingRequestPacket",
            pg::serverbound_ping_request::ServerboundPingRequestPacket::stream_codec(), plain),
        entry!("status", "clientbound", "minecraft:status_response", "ClientboundStatusResponsePacket",
            s::clientbound_status_response_packet::ClientboundStatusResponsePacket::stream_codec(), plain),
        entry!("status", "clientbound", "minecraft:pong_response", "ClientboundPongResponsePacket",
            pg::clientbound_pong_response::ClientboundPongResponsePacket::stream_codec(), plain),
        // login
        entry!("login", "serverbound", "minecraft:hello", "ServerboundHelloPacket",
            l::serverbound_hello_packet::ServerboundHelloPacket::stream_codec(), plain),
        entry!("login", "serverbound", "minecraft:login_acknowledged", "ServerboundLoginAcknowledgedPacket",
            l::serverbound_login_acknowledged_packet::stream_codec(), plain),
        entry!("login", "serverbound", "minecraft:cookie_response", "ServerboundCookieResponsePacket",
            ck::serverbound_cookie_response::ServerboundCookieResponsePacket::stream_codec(), plain),
        entry!("login", "clientbound", "minecraft:login_finished", "ClientboundLoginFinishedPacket",
            l::clientbound_login_finished_packet::ClientboundLoginFinishedPacket::stream_codec(), plain),
        entry!("login", "clientbound", "minecraft:login_compression", "ClientboundLoginCompressionPacket",
            l::clientbound_login_compression_packet::ClientboundLoginCompressionPacket::stream_codec(), plain),
        entry!("login", "clientbound", "minecraft:cookie_request", "ClientboundCookieRequestPacket",
            ck::clientbound_cookie_request::ClientboundCookieRequestPacket::stream_codec(), plain),
        // configuration clientbound
        entry!("configuration", "clientbound", "minecraft:cookie_request", "ClientboundCookieRequestPacket",
            ck::clientbound_cookie_request::ClientboundCookieRequestPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:custom_payload", "ClientboundCustomPayloadPacket",
            c::clientbound_custom_payload::ClientboundCustomPayloadPacket::config_stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:disconnect", "ClientboundDisconnectPacket",
            c::clientbound_disconnect::ClientboundDisconnectPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:finish_configuration", "ClientboundFinishConfigurationPacket",
            cfg::clientbound_finish_configuration::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:keep_alive", "ClientboundKeepAlivePacket",
            c::clientbound_keep_alive::ClientboundKeepAlivePacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:ping", "ClientboundPingPacket",
            c::clientbound_ping::ClientboundPingPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:registry_data", "ClientboundRegistryDataPacket",
            cfg::clientbound_registry_data::ClientboundRegistryDataPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:resource_pack_pop", "ClientboundResourcePackPopPacket",
            c::clientbound_resource_pack_pop::ClientboundResourcePackPopPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:store_cookie", "ClientboundStoreCookiePacket",
            c::clientbound_store_cookie::ClientboundStoreCookiePacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:transfer", "ClientboundTransferPacket",
            c::clientbound_transfer::ClientboundTransferPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:update_enabled_features", "ClientboundUpdateEnabledFeaturesPacket",
            cfg::clientbound_update_enabled_features::ClientboundUpdateEnabledFeaturesPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:update_tags", "ClientboundUpdateTagsPacket",
            c::clientbound_update_tags::ClientboundUpdateTagsPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:select_known_packs", "ClientboundSelectKnownPacks",
            cfg::clientbound_select_known_packs::ClientboundSelectKnownPacks::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:custom_report_details", "ClientboundCustomReportDetailsPacket",
            c::clientbound_custom_report_details::ClientboundCustomReportDetailsPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:server_links", "ClientboundServerLinksPacket",
            c::clientbound_server_links::ClientboundServerLinksPacket::stream_codec(), plain),
        entry!("configuration", "clientbound", "minecraft:clear_dialog", "ClientboundClearDialogPacket",
            c::clientbound_clear_dialog::stream_codec(), plain),
        // configuration serverbound
        entry!("configuration", "serverbound", "minecraft:client_information", "ServerboundClientInformationPacket",
            c::serverbound_client_information::ServerboundClientInformationPacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:cookie_response", "ServerboundCookieResponsePacket",
            ck::serverbound_cookie_response::ServerboundCookieResponsePacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:custom_payload", "ServerboundCustomPayloadPacket",
            c::serverbound_custom_payload::ServerboundCustomPayloadPacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:finish_configuration", "ServerboundFinishConfigurationPacket",
            cfg::serverbound_finish_configuration::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:keep_alive", "ServerboundKeepAlivePacket",
            c::serverbound_keep_alive::ServerboundKeepAlivePacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:pong", "ServerboundPongPacket",
            c::serverbound_pong::ServerboundPongPacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:resource_pack", "ServerboundResourcePackPacket",
            c::serverbound_resource_pack::ServerboundResourcePackPacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:select_known_packs", "ServerboundSelectKnownPacks",
            cfg::serverbound_select_known_packs::ServerboundSelectKnownPacks::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:custom_click_action", "ServerboundCustomClickActionPacket",
            c::serverbound_custom_click_action::ServerboundCustomClickActionPacket::stream_codec(), plain),
        entry!("configuration", "serverbound", "minecraft:accept_code_of_conduct", "ServerboundAcceptCodeOfConductPacket",
            cfg::serverbound_accept_code_of_conduct::stream_codec(), plain),
        // play clientbound
        entry!("play", "clientbound", "minecraft:add_entity", "ClientboundAddEntityPacket",
            g::clientbound_add_entity_packet::add_entity_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:block_update", "ClientboundBlockUpdatePacket",
            p::clientbound_block_updates::ClientboundBlockUpdatePacket::stream_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:change_difficulty", "ClientboundChangeDifficultyPacket",
            p::clientbound_change_difficulty::ClientboundChangeDifficultyPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:chunk_batch_finished", "ClientboundChunkBatchFinishedPacket",
            p::clientbound_chunk_batch_finished::ClientboundChunkBatchFinishedPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:chunk_batch_start", "ClientboundChunkBatchStartPacket",
            p::clientbound_chunk_batch_start::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:chunks_biomes", "ClientboundChunksBiomesPacket",
            p::clientbound_chunks_biomes::ClientboundChunksBiomesPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:clear_dialog", "ClientboundClearDialogPacket",
            c::clientbound_clear_dialog::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:cookie_request", "ClientboundCookieRequestPacket",
            ck::clientbound_cookie_request::ClientboundCookieRequestPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:custom_report_details", "ClientboundCustomReportDetailsPacket",
            c::clientbound_custom_report_details::ClientboundCustomReportDetailsPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:disconnect", "ClientboundDisconnectPacket",
            c::clientbound_disconnect::ClientboundDisconnectPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:entity_event", "ClientboundEntityEventPacket",
            g::clientbound_entity_event_packet::entity_event_codec(), plain),
        entry!("play", "clientbound", "minecraft:game_event", "ClientboundGameEventPacket",
            p::clientbound_game_event::ClientboundGameEventPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:initialize_border", "ClientboundInitializeBorderPacket",
            p::clientbound_initialize_border::ClientboundInitializeBorderPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:keep_alive", "ClientboundKeepAlivePacket",
            c::clientbound_keep_alive::ClientboundKeepAlivePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:level_chunk_with_light", "ClientboundLevelChunkWithLightPacket",
            p::clientbound_level_chunk_with_light::ClientboundLevelChunkWithLightPacket::stream_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:level_event", "ClientboundLevelEventPacket",
            p::clientbound_block_updates::ClientboundLevelEventPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:light_update", "ClientboundLightUpdatePacket",
            p::clientbound_light_update::ClientboundLightUpdatePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:login", "ClientboundLoginPacket",
            p::clientbound_login::ClientboundLoginPacket::stream_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:ping", "ClientboundPingPacket",
            c::clientbound_ping::ClientboundPingPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:player_abilities", "ClientboundPlayerAbilitiesPacket",
            p::clientbound_player_abilities::ClientboundPlayerAbilitiesPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:player_info_remove", "ClientboundPlayerInfoRemovePacket",
            p::clientbound_player_info_remove::ClientboundPlayerInfoRemovePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:player_info_update", "ClientboundPlayerInfoUpdatePacket",
            p::clientbound_player_info_update::ClientboundPlayerInfoUpdatePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:player_position", "ClientboundPlayerPositionPacket",
            p::clientbound_player_position::ClientboundPlayerPositionPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:pong_response", "ClientboundPongResponsePacket",
            pg::clientbound_pong_response::ClientboundPongResponsePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:resource_pack_pop", "ClientboundResourcePackPopPacket",
            c::clientbound_resource_pack_pop::ClientboundResourcePackPopPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:section_blocks_update", "ClientboundSectionBlocksUpdatePacket",
            p::clientbound_block_updates::ClientboundSectionBlocksUpdatePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:server_links", "ClientboundServerLinksPacket",
            c::clientbound_server_links::ClientboundServerLinksPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:set_chunk_cache_center", "ClientboundSetChunkCacheCenterPacket",
            p::clientbound_set_chunk_cache_center::ClientboundSetChunkCacheCenterPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:set_chunk_cache_radius", "ClientboundSetChunkCacheRadiusPacket",
            p::clientbound_set_chunk_cache_radius::ClientboundSetChunkCacheRadiusPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:set_default_spawn_position", "ClientboundSetDefaultSpawnPositionPacket",
            p::clientbound_set_default_spawn::ClientboundSetDefaultSpawnPositionPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:set_entity_data", "ClientboundSetEntityDataPacket",
            g::clientbound_set_entity_data_packet::set_entity_data_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:set_held_slot", "ClientboundSetHeldSlotPacket",
            p::clientbound_set_held_slot::ClientboundSetHeldSlotPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:set_simulation_distance", "ClientboundSetSimulationDistancePacket",
            p::clientbound_set_simulation_distance::ClientboundSetSimulationDistancePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:set_time", "ClientboundSetTimePacket",
            p::clientbound_set_time::ClientboundSetTimePacket::stream_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:store_cookie", "ClientboundStoreCookiePacket",
            c::clientbound_store_cookie::ClientboundStoreCookiePacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:transfer", "ClientboundTransferPacket",
            c::clientbound_transfer::ClientboundTransferPacket::stream_codec(), plain),
        entry!("play", "clientbound", "minecraft:update_attributes", "ClientboundUpdateAttributesPacket",
            g::clientbound_update_attributes_packet::update_attributes_codec(), with_registries),
        entry!("play", "clientbound", "minecraft:update_tags", "ClientboundUpdateTagsPacket",
            c::clientbound_update_tags::ClientboundUpdateTagsPacket::stream_codec(), plain),
        // play serverbound
        entry!("play", "serverbound", "minecraft:accept_teleportation", "ServerboundAcceptTeleportationPacket",
            g::serversbound_accept_teleportation_packet::accept_teleportation_codec(), plain),
        entry!("play", "serverbound", "minecraft:chunk_batch_received", "ServerboundChunkBatchReceivedPacket",
            p::serverbound_chunk_batch_received::ServerboundChunkBatchReceivedPacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:client_command", "ServerboundClientCommandPacket",
            g::serversbound_client_command_packet::client_command_codec(), plain),
        entry!("play", "serverbound", "minecraft:client_information", "ServerboundClientInformationPacket",
            c::serverbound_client_information::ServerboundClientInformationPacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:client_tick_end", "ServerboundClientTickEndPacket",
            g::serversbound_client_tick_end_packet::client_tick_end_codec(), plain),
        entry!("play", "serverbound", "minecraft:cookie_response", "ServerboundCookieResponsePacket",
            ck::serverbound_cookie_response::ServerboundCookieResponsePacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:custom_click_action", "ServerboundCustomClickActionPacket",
            c::serverbound_custom_click_action::ServerboundCustomClickActionPacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:keep_alive", "ServerboundKeepAlivePacket",
            c::serverbound_keep_alive::ServerboundKeepAlivePacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:move_player_pos", "ServerboundMovePlayerPacket",
            g::serversbound_move_player_packet::pos_codec(), plain),
        entry!("play", "serverbound", "minecraft:move_player_pos_rot", "ServerboundMovePlayerPacket",
            g::serversbound_move_player_packet::pos_rot_codec(), plain),
        entry!("play", "serverbound", "minecraft:move_player_rot", "ServerboundMovePlayerPacket",
            g::serversbound_move_player_packet::rot_codec(), plain),
        entry!("play", "serverbound", "minecraft:move_player_status_only", "ServerboundMovePlayerPacket",
            g::serversbound_move_player_packet::status_only_codec(), plain),
        entry!("play", "serverbound", "minecraft:ping_request", "ServerboundPingRequestPacket",
            pg::serverbound_ping_request::ServerboundPingRequestPacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:player_action", "ServerboundPlayerActionPacket",
            g::serversbound_player_action_packet::player_action_codec(), plain),
        entry!("play", "serverbound", "minecraft:pong", "ServerboundPongPacket",
            c::serverbound_pong::ServerboundPongPacket::stream_codec(), plain),
        entry!("play", "serverbound", "minecraft:resource_pack", "ServerboundResourcePackPacket",
            c::serverbound_resource_pack::ServerboundResourcePackPacket::stream_codec(), plain),
    ]
}

/// The generated packet-name table of one (phase, direction); the index is the
/// network id.
pub fn names(state: &str, direction: &str) -> Option<&'static [&'static str]> {
    Some(match (state, direction) {
        ("handshake", "serverbound") => packets::handshake::serverbound::PACKET_BY_ID,
        ("status", "serverbound") => packets::status::serverbound::PACKET_BY_ID,
        ("status", "clientbound") => packets::status::clientbound::PACKET_BY_ID,
        ("login", "serverbound") => packets::login::serverbound::PACKET_BY_ID,
        ("login", "clientbound") => packets::login::clientbound::PACKET_BY_ID,
        ("configuration", "serverbound") => packets::configuration::serverbound::PACKET_BY_ID,
        ("configuration", "clientbound") => packets::configuration::clientbound::PACKET_BY_ID,
        ("play", "serverbound") => packets::play::serverbound::PACKET_BY_ID,
        ("play", "clientbound") => packets::play::clientbound::PACKET_BY_ID,
        _ => return None,
    })
}

/// The network id of `name` in one (phase, direction).
pub fn packet_id(state: &str, direction: &str, name: &str) -> Option<usize> {
    names(state, direction)?.iter().position(|n| *n == name)
}

thread_local! {
    /// Set while a packet handler runs: panics inside it are an answer, not a crash report.
    static IN_HANDLER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Silence the panic report for panics raised inside a packet handler; every
/// other panic still reports through the previous hook.
fn install_panic_filter() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !IN_HANDLER.with(std::cell::Cell::get) {
                previous(info);
            }
        }));
    });
}

/// Run `f`, treating a panic as the op's own answer (the ported codecs panic
/// where Java throws) and keeping its report off stderr.
pub fn guarded<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    install_panic_filter();
    IN_HANDLER.with(|flag| flag.set(true));
    let answered = catch_unwind(AssertUnwindSafe(f));
    IN_HANDLER.with(|flag| flag.set(false));
    answered
}

thread_local! {
    static ENTRIES: Vec<Entry> = entries();
}

/// The packet struct a request exercises (an unregistered id has none of its own).
pub fn subject_of(request: &Map<String, Value>) -> Option<String> {
    let state = request.get("state")?.as_str()?;
    let direction = request.get("direction")?.as_str()?;
    let id = usize::try_from(request.get("packet_id")?.as_u64()?).ok()?;
    let Some(name) = names(state, direction)?.get(id) else {
        return Some("(unregistered packet ids)".to_owned());
    };
    ENTRIES.with(|entries| {
        entries
            .iter()
            .find(|e| e.state == state && e.direction == direction && e.name == *name)
            .map(|e| e.subject.to_owned())
    })
}

/// `packet.reencode`: `{state, direction, packet_id, body}` -> `{name, body}`.
pub fn packet_reencode(request: &Map<String, Value>) -> Outcome {
    let string = |key: &str| {
        request.get(key).and_then(Value::as_str).ok_or_else(|| {
            Outcome::Unavailable(format!("request property '{key}' must be a string"))
        })
    };
    let (state, direction, body_hex) = match (string("state"), string("direction"), string("body"))
    {
        (Ok(s), Ok(d), Ok(b)) => (s, d, b),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return e,
    };
    let Some(id) = request.get("packet_id").and_then(Value::as_u64) else {
        return Outcome::Unavailable("request property 'packet_id' must be a number".into());
    };
    let Some(table) = names(state, direction) else {
        return Outcome::Unavailable(format!("no protocol for {state}/{direction}"));
    };
    let Some(body) = decode_hex(body_hex) else {
        return Outcome::Unavailable("request property 'body' must be hex".into());
    };
    // `IdDispatchCodec.decode`: an unregistered id is a DecoderException.
    let Some(name) = usize::try_from(id).ok().and_then(|i| table.get(i)) else {
        return Outcome::Rejected;
    };
    ENTRIES.with(|entries| {
        let Some(entry) = entries
            .iter()
            .find(|e| e.state == state && e.direction == direction && e.name == *name)
        else {
            return Outcome::Unavailable(format!(
                "{state}/{direction} {name} is registered in Paper but not ported in Rivet"
            ));
        };
        match guarded(|| entry.reencode(&body)) {
            Ok(Ok(encoded)) => {
                let mut result = Map::new();
                result.insert("name".into(), json!(name));
                result.insert("body".into(), json!(encode_hex(&encoded)));
                Outcome::Accepted(result)
            }
            Ok(Err(_)) => Outcome::Rejected,
            // The ported codecs report a malformed packet by panicking exactly
            // where Java throws (`DecoderException`, `IndexOutOfBoundsException`,
            // `IdentifierException`, ...): that panic is Rivet's rejection.
            Err(_) => Outcome::Rejected,
        }
    })
}

/// Lowercase hex, as `HexFormat.of().formatHex`.
pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `HexFormat.of().parseHex`: an even number of hex digits.
pub fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

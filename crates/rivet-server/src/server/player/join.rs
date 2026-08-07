//! `PlayerList.placeNewPlayer` / `PlayerList.sendLevelInfo` — the deterministic
//! play join burst (Slice A of #101), in Paper's send order.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! server/players/PlayerList.java` (`placeNewPlayer` lines 158–338,
//! `sendLevelInfo` lines 994–1015). The send order matches the `PLAY_BURST_ORDER`
//! constant in `tools/rivet-capture/src/ordering.rs` (the pinned
//! `26.2-DEV-main@0a99345` fixture).
//!
//! Paper calls `sendLevelInfo` TWICE inside `placeNewPlayer` (lines 231 and
//! 294). The burst below is the FIRST occurrence, emitted before the
//! `player_info_update` broadcast; Slice A ports only this first-occurrence
//! foundation. The second block re-sends the same four members
//! (`initialize_border` → `set_time` → `set_default_spawn_position` →
//! `game_event`) after `player_info_update` (and after the entity pairing at
//! line 291); it is deferred to Slice B. Its ordering is unambiguous: it is the
//! LAST burst member — nothing in the burst follows it in `placeNewPlayer`.
//!
//! This burst is `PLAY_BURST_ORDER` restricted to the Slice A members — the
//! ten members Paper sends in between are deferred, keeping the relative order
//! of the members that are sent:
//!
//! - `update_recipes` (133) — RivetTodo(#87), body not ported.
//! - `entity_event` (34) — RivetTodo(#222), self add-entity pairing.
//! - `recipe_book_settings` (76) / `recipe_book_add` (74) — inventory/recipe
//!   book (epic #22).
//! - `server_data` (86) — `ServerStatus`, follows the teleport in
//!   `placeNewPlayer`.
//! - `ticking_state` (127) / `ticking_step` (128) — the tick-rate manager's
//!   join burst.
//! - `container_set_content` (18) / `container_set_slot` (20) — inventory
//!   init (epic #22).
//! - `system_chat` (121) — the join message (epic #12).
//!
//! Every body is encoded with the merged #246 protocol bodies (the join packet
//! bodies in `rivet-protocol`) and framed + queued by
//! [`PlaySender`](super::play_sender::PlaySender) over the connection's bounded
//! outbound channel. The offline M1 world is the superflat `ServerLevel` (seed
//! 42, view distance 4) the #153 capture records; the capture-grounded values
//! below — three vanilla levels, `max_players 20`, the world-border defaults,
//! the two clocks, the `RivetProbe` profile — are pinned by that fixture's
//! `join_clientbound_*.hex` golden bodies.

use rivet_protocol::generated::packets::play::clientbound::PacketType;
use rivet_protocol::protocol::game::clientbound_change_difficulty::ClientboundChangeDifficultyPacket;
use rivet_protocol::protocol::game::clientbound_game_event::{
    ClientboundGameEventPacket, LEVEL_CHUNKS_LOAD_START,
};
use rivet_protocol::protocol::game::clientbound_initialize_border::ClientboundInitializeBorderPacket;
use rivet_protocol::protocol::game::clientbound_login::ClientboundLoginPacket;
use rivet_protocol::protocol::game::clientbound_player_abilities::ClientboundPlayerAbilitiesPacket;
use rivet_protocol::protocol::game::clientbound_player_info_update::{
    ACTIONS, ClientboundPlayerInfoUpdatePacket, Entry,
};
use rivet_protocol::protocol::game::clientbound_player_position::ClientboundPlayerPositionPacket;
use rivet_protocol::protocol::game::clientbound_set_default_spawn::{
    ClientboundSetDefaultSpawnPositionPacket, RespawnData as ProtocolRespawnData,
};
use rivet_protocol::protocol::game::clientbound_set_held_slot::ClientboundSetHeldSlotPacket;
use rivet_protocol::protocol::game::clientbound_set_time::ClientboundSetTimePacket;
use rivet_protocol::protocol::game::clock_network_state::ClockNetworkState;
use rivet_protocol::protocol::game::common_player_spawn_info::CommonPlayerSpawnInfo;
use rivet_protocol::protocol::game::position_move_rotation::PositionMoveRotation;
use rivet_registry::ResourceKey;
use rivet_registry::core::{Difficulty, Vec3};
use rivet_registry::holder::Holder;
use rivet_registry::registries;
use rivet_registry::registries::{DimensionType, Level};

use crate::server::level::server_level::ServerLevel;
use crate::server::network::connection_id::ConnectionId;
use crate::server::tick::registry::ConnectionRegistry;

use super::ServerPlayer;
use super::play_sender::{PlaySendError, PlaySender};

// The generated play clientbound packet ids (`GameProtocols.CLIENTBOUND_TEMPLATE`
// addPacket order; `rivet_protocol::generated::packets::play::clientbound`).
const LOGIN_ID: u32 = PacketType::Login.id();
const CHANGE_DIFFICULTY_ID: u32 = PacketType::ChangeDifficulty.id();
const PLAYER_ABILITIES_ID: u32 = PacketType::PlayerAbilities.id();
const SET_HELD_SLOT_ID: u32 = PacketType::SetHeldSlot.id();
const PLAYER_POSITION_ID: u32 = PacketType::PlayerPosition.id();
const INITIALIZE_BORDER_ID: u32 = PacketType::InitializeBorder.id();
const SET_TIME_ID: u32 = PacketType::SetTime.id();
const SET_DEFAULT_SPAWN_POSITION_ID: u32 = PacketType::SetDefaultSpawnPosition.id();
const GAME_EVENT_ID: u32 = PacketType::GameEvent.id();
const PLAYER_INFO_UPDATE_ID: u32 = PacketType::PlayerInfoUpdate.id();

/// The `WorldBorder` snapshot the M1 superflat world's border defaults to:
/// `new StaticBorderExtent(5.999997E7F)`, absolute max `29999984`, warnings
/// (blocks `5` / time `300`), centered on the spawn `(0, 0)`. The capture's
/// id-43 `initialize_border` line pins these exact values (asserted inline in
/// `join_burst.rs`'s `border_body`; there is no dedicated fixture file).
///
/// Paper's `WorldBorder.MAX_SIZE` literal is the *float* `5.999997E7F`, which
/// as a double is exactly `59999968.0` (floats near 6E7 step by 8) — the
/// capture's `old/new size` doubles encode `0x418C9C3700000000`. The decimal
/// `59999968.0` is that exact double; the f64 `5.999_997E7` would be
/// `59999970.0` and break byte fidelity.
const WORLD_BORDER_SIZE: f64 = 59_999_968.0;
const WORLD_BORDER_ABSOLUTE_MAX_SIZE: i32 = 29_999_984;
const WORLD_BORDER_WARNING_BLOCKS: i32 = 5;
const WORLD_BORDER_WARNING_TIME: i32 = 300;

/// The server-side values `PlayerList.placeNewPlayer` reads off the server (not
/// the world or the player) to build the login packet. Immutable per join.
#[derive(Debug, Clone)]
pub struct JoinConfig {
    /// `PlayerList.getMaxPlayers()` (the capture's 20).
    pub max_players: i32,
    /// `LevelData.isHardcore()`.
    pub hardcore: bool,
    /// `MinecraftServer.levelKeys()` — the three vanilla levels.
    pub level_keys: Vec<ResourceKey<Level>>,
    /// `MinecraftServer.usesAuthentication()` — false for the offline M1 world.
    pub online_mode: bool,
    /// `MinecraftServer.enforceSecureProfile()`.
    pub enforces_secure_chat: bool,
    /// `showDeathScreen = !GameRules.IMMEDIATE_RESPAWN`.
    pub show_death_screen: bool,
    /// `ServerLevel.isFlat()` — true for the superflat M1 world.
    pub is_flat: bool,
}

/// `PlayerList.placeNewPlayer(connection, player, cookie)` — the deterministic
/// play join burst, through Paper's FIRST `sendLevelInfo` occurrence (the
/// pre-`player_info_update` foundation; the second block is Slice B). Encode +
/// queue each packet in Paper's send order (the Slice A subset of
/// `PLAY_BURST_ORDER`) and return the ordered packet ids that were sent, for
/// the ordering tests.
///
/// `update_recipes` is intentionally omitted: its body is not ported
/// (RivetTodo(#87)); the burst keeps Paper's order otherwise. Entity pairing
/// (the `entity_event`/`set_entity_data` burst members) is RivetTodo(#222).
pub fn place_new_player(
    sender: &mut PlaySender,
    connections: &mut ConnectionRegistry,
    connection_id: ConnectionId,
    player: &ServerPlayer,
    level: &ServerLevel,
    join: &JoinConfig,
) -> Result<Vec<u32>, PlaySendError> {
    let mut sent = Vec::with_capacity(10);

    // `playerConnection.send(new ClientboundLoginPacket(player.getId(), ...))`.
    // `createCommonSpawnInfo` resolves the level's dimension-type holder; the M1
    // capture pins holder id 0 (the overworld), and the general resolution is
    // RivetTodo(#126).
    let spawn_info = CommonPlayerSpawnInfo::new(
        dimension_type_holder(sender, 0),
        level.dimension().clone(),
        rivet_util::java_hash::obfuscate_seed(level.seed()),
        player.game_type(),
        None,  // previous_game_type
        false, // is_debug
        join.is_flat,
        None, // last_death_location
        0,    // portal_cooldown
        level.get_sea_level(),
    );
    let login = ClientboundLoginPacket::new(
        player.player_id(),
        join.hardcore,
        join.level_keys.clone(),
        join.max_players,
        level.view().view_distance(),
        level.view().view_distance(),
        false, // reduced_debug_info
        join.show_death_screen,
        false, // do_limited_crafting
        spawn_info,
        join.online_mode,
        join.enforces_secure_chat,
    );
    let body = sender.encode_registry_body(
        ClientboundLoginPacket::stream_codec(),
        &login,
        sender.dimension_type_access(),
    )?;
    sender.send_packet(connections, connection_id, LOGIN_ID, &body)?;
    sent.push(LOGIN_ID);

    // `playerConnection.send(new ClientboundChangeDifficultyPacket(
    // levelData.getDifficulty(), levelData.isDifficultyLocked()))` — the M1
    // world is EASY, not locked.
    let body = sender.encode_body(
        ClientboundChangeDifficultyPacket::stream_codec(),
        &ClientboundChangeDifficultyPacket::new(Difficulty::Easy, false),
    )?;
    sender.send_packet(connections, connection_id, CHANGE_DIFFICULTY_ID, &body)?;
    sent.push(CHANGE_DIFFICULTY_ID);

    // `playerConnection.send(new ClientboundPlayerAbilitiesPacket(player.getAbilities()))`
    // — the `Abilities` defaults (no flags, flyingSpeed 0.05, walkingSpeed 0.1).
    let body = sender.encode_body(
        ClientboundPlayerAbilitiesPacket::stream_codec(),
        &ClientboundPlayerAbilitiesPacket::new(false, false, false, false, 0.05, 0.1),
    )?;
    sender.send_packet(connections, connection_id, PLAYER_ABILITIES_ID, &body)?;
    sent.push(PLAYER_ABILITIES_ID);

    // `playerConnection.send(new ClientboundSetHeldSlotPacket(player.getInventory().getSelectedSlot()))`.
    let body = sender.encode_body(
        ClientboundSetHeldSlotPacket::stream_codec(),
        &ClientboundSetHeldSlotPacket::new(0),
    )?;
    sender.send_packet(connections, connection_id, SET_HELD_SLOT_ID, &body)?;
    sent.push(SET_HELD_SLOT_ID);

    // RivetTodo(#87): `new ClientboundUpdateRecipesPacket(
    // recipeManager.getSynchronizedItemProperties(),
    // recipeManager.getSynchronizedStonecutterRecipes())` — the recipe-book
    // body is not ported; the burst omits it, keeping Paper's order otherwise.

    // `playerConnection.teleport(player.getX(), ...)` — the position teleport.
    let change = PositionMoveRotation::new(
        player.position(),
        Vec3::new(0.0, 0.0, 0.0),
        player.yaw(),
        player.pitch(),
    );
    let body = sender.encode_body(
        ClientboundPlayerPositionPacket::stream_codec(),
        &ClientboundPlayerPositionPacket::new(0, change, Vec::new()),
    )?;
    sender.send_packet(connections, connection_id, PLAYER_POSITION_ID, &body)?;
    sent.push(PLAYER_POSITION_ID);

    // `this.sendLevelInfo(player, level)`.
    sent.extend(send_level_info(sender, connections, connection_id, level)?);

    // `broadcastPlayerInfo(...)` → `ClientboundPlayerInfoUpdatePacket.createPlayerInitializing(List.of(player))`.
    let entry = Entry::new(
        player.uuid(),
        Some(player.profile().clone()),
        true, // listed
        0,    // latency
        player.game_type(),
        None, // display_name
        true, // show_hat
        0,    // list_order
        None, // chat_session
    );
    let body = sender.encode_body(
        ClientboundPlayerInfoUpdatePacket::stream_codec(),
        &ClientboundPlayerInfoUpdatePacket::new(ACTIONS.to_vec(), vec![entry]),
    )?;
    sender.send_packet(connections, connection_id, PLAYER_INFO_UPDATE_ID, &body)?;
    sent.push(PLAYER_INFO_UPDATE_ID);

    Ok(sent)
}

/// `PlayerList.sendLevelInfo(player, level)` — the four-packet level snapshot
/// in Paper's order, returning the ordered ids.
pub fn send_level_info(
    sender: &mut PlaySender,
    connections: &mut ConnectionRegistry,
    connection_id: ConnectionId,
    level: &ServerLevel,
) -> Result<Vec<u32>, PlaySendError> {
    let mut sent = Vec::with_capacity(4);

    // `player.connection.send(new ClientboundInitializeBorderPacket(worldBorder))`.
    let body = sender.encode_body(
        ClientboundInitializeBorderPacket::stream_codec(),
        &ClientboundInitializeBorderPacket::new(
            0.0,
            0.0,
            WORLD_BORDER_SIZE,
            WORLD_BORDER_SIZE,
            0,
            WORLD_BORDER_ABSOLUTE_MAX_SIZE,
            WORLD_BORDER_WARNING_BLOCKS,
            WORLD_BORDER_WARNING_TIME,
        ),
    )?;
    sender.send_packet(connections, connection_id, INITIALIZE_BORDER_ID, &body)?;
    sent.push(INITIALIZE_BORDER_ID);

    // `player.connection.send(level.clockManager().createFullSyncPacket(player))`
    // — the two clocks the flat world runs (day/night and weather), both at
    // `(totalTicks 0, partialTick 0.0, rate 1.0)`.
    let clock_registry_id = sender
        .world_clock_access()
        .lookup(&*registries::WORLD_CLOCK)
        .expect("WORLD_CLOCK registry present in the play sender")
        .registry_id();
    let clock_state = ClockNetworkState::new(0, 0.0, 1.0);
    let set_time = ClientboundSetTimePacket::new(
        0,
        vec![
            (Holder::reference(clock_registry_id, 0), clock_state),
            (Holder::reference(clock_registry_id, 1), clock_state),
        ],
    );
    let body = sender.encode_registry_body(
        ClientboundSetTimePacket::stream_codec(),
        &set_time,
        sender.world_clock_access(),
    )?;
    sender.send_packet(connections, connection_id, SET_TIME_ID, &body)?;
    sent.push(SET_TIME_ID);

    // `player.connection.send(new ClientboundSetDefaultSpawnPositionPacket(level.getRespawnData()))`.
    let respawn = level.get_respawn_data();
    let protocol_respawn =
        ProtocolRespawnData::new(respawn.global_pos().clone(), respawn.yaw(), respawn.pitch());
    let body = sender.encode_body(
        ClientboundSetDefaultSpawnPositionPacket::stream_codec(),
        &ClientboundSetDefaultSpawnPositionPacket::new(protocol_respawn),
    )?;
    sender.send_packet(
        connections,
        connection_id,
        SET_DEFAULT_SPAWN_POSITION_ID,
        &body,
    )?;
    sent.push(SET_DEFAULT_SPAWN_POSITION_ID);

    // `player.connection.send(new ClientboundGameEventPacket(
    // ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START, 0.0F))`.
    let body = sender.encode_body(
        ClientboundGameEventPacket::stream_codec(),
        &ClientboundGameEventPacket::new(LEVEL_CHUNKS_LOAD_START, 0.0),
    )?;
    sender.send_packet(connections, connection_id, GAME_EVENT_ID, &body)?;
    sent.push(GAME_EVENT_ID);

    Ok(sent)
}

/// The `Holder<DimensionType>` the spawn info carries — a reference to holder id
/// `id` in the connection's `DIMENSION_TYPE` registry. The M1 capture's
/// `CommonPlayerSpawnInfo` carries holder id 0 (the overworld dimension type);
/// the general level→holder resolution (`ServerLevel.dimensionType()`, which
/// needs a runtime `RegistryAccess`) is RivetTodo(#126), so Slice A pins the
/// capture's fixed id 0.
fn dimension_type_holder(sender: &PlaySender, id: u32) -> Holder<DimensionType> {
    let registry_id = sender
        .dimension_type_access()
        .lookup(&*registries::DIMENSION_TYPE)
        .expect("DIMENSION_TYPE registry present in the play sender")
        .registry_id();
    Holder::reference(registry_id, id)
}

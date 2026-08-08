//! `PlayerList.placeNewPlayer` / `PlayerList.sendLevelInfo` — the deterministic
//! play join burst (issues #101 Slice A + B), in Paper's send order.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! server/players/PlayerList.java` (`placeNewPlayer` lines 158–338,
//! `sendLevelInfo` lines 994–1015). The send order matches the `PLAY_BURST_ORDER`
//! constant in `tools/rivet-capture/src/ordering.rs` (the pinned
//! `26.2-DEV-main@0a99345` fixture).
//!
//! Paper calls `sendLevelInfo` TWICE inside `placeNewPlayer` (lines 231 and
//! 294) and `initInventoryMenu` TWICE (lines 235 and 306) — both Paper source
//! facts. They are why the single-player #153 capture records the level-snapshot
//! members twice (duplicate id-43/38/97 from the two `sendLevelInfo` calls,
//! id-18/20 from the two `initInventoryMenu` calls): the duplicates come from
//! the one join re-sending them, not from proxy merging or a second connection.
//! `set_time` (113) is sampled once only because `canonicalize` keeps racy ids'
//! first occurrence. The burst below emits BOTH `sendLevelInfo` occurrences
//! (the first before `player_info_update`, the second after the player_info
//! broadcast), matching Paper's source order.
//!
//! This burst is `PLAY_BURST_ORDER` restricted to the members that are ported —
//! the deferred members are dropped, keeping the relative order of the members
//! that are sent. The complete member set `placeNewPlayer` sends is
//! [`PLAY_BURST_ORDER`](crate::server::player::join) (all ids listed in
//! `tools/rivet-capture/src/ordering.rs`); this function emits:
//!
//! - login (49), change_difficulty (10), player_abilities (64),
//!   set_held_slot (105);
//! - `update_recipes` (133) — RivetTodo(#87), body not ported;
//! - entity_event (34) — `sendPlayerPermissionLevel`'s op-level event (body
//!   ported per #90);
//! - `recipe_book_settings` (76) / `recipe_book_add` (74) — inventory/recipe
//!   book (epic #22);
//! - player_position (72);
//! - `server_data` (86) — `ServerStatus`, follows the teleport;
//! - first `sendLevelInfo` (43/113/97/38);
//! - `ticking_state` (127) / `ticking_step` (128) — the tick-rate manager's
//!   join burst;
//! - `container_set_content` (18) / `container_set_slot` (20) — inventory
//!   init (epic #22);
//! - `system_chat` (121) — the join message (epic #12);
//! - player_info_update (70);
//! - second `sendLevelInfo` (43/113/97/38) — after the player_info broadcast;
//! - the Moonrise chunk-loader send-set (issue #100): cache radius (95),
//!   simulation distance (111), cache center (94), then the deterministic 117
//!   `level_chunk_with_light` (45) bodies. Moonrise's `addPlayer` runs at
//!   `level.addNewPlayer(player)` (before the second `sendLevelInfo` in Paper);
//!   this slice emits the cache + chunk set immediately before the second
//!   `sendLevelInfo` so the level-snapshot block stays contiguous and the
//!   chunk stream is the last burst member (the chunk order is the deterministic
//!   canonical raster `rivet-capture` byte-matches, not Paper's timing-dependent
//!   wire order). `addNewPlayer`'s `set_entity_data` (99) entity pairing is
//!   deferred.
//!   RivetTodo(#222): `addNewPlayer`'s `set_entity_data` (99) entity-pairing
//!   packet — the syncher serializers / entity-data integration are not ported.
//!
//! Every body is encoded with the merged #246 protocol bodies (the join packet
//! bodies in `rivet-protocol`) and framed + queued by
//! [`PlaySender`](super::play_sender::PlaySender) over the connection's bounded
//! outbound channel. The offline M1 world is the superflat `ServerLevel` (seed
//! 42, view distance 4) the #153 capture records; the capture-grounded values
//! below — three vanilla levels, `max_players 20`, the world-border defaults,
//! the two clocks, the `RivetProbe` profile — are pinned by that fixture's
//! `join_clientbound_*.hex` golden bodies. The `capture.jsonl` is the
//! NORMALIZED capture (`normalize::canonicalize`): it supplies the BODIES, but
//! canonicalize groups by `(state, direction, id)` and so erases ORDER — the
//! burst order comes from `PLAY_BURST_ORDER` / the Paper source, not from the
//! normalized capture's (or the raw capture's) positional order. It also
//! normalizes racy ids to fixed values (the teleport id, entity_event's
//! `entityId -> 1`, `eventId -> 0`); the deterministic real values this slice
//! emits (entity id 1, event id 24) are the Paper source facts, asserted inline
//! in `join_burst.rs`. The teleport id is *not* a fixed 0: `place_new_player`
//! runs the live `awaitingTeleport` machine (issue #158) and embeds the
//! session's real id (1 for the spawn teleport), which `join_burst.rs` pins by
//! rewriting the fixture's leading id varint.

use rivet_protocol::game::clientbound_entity_event_packet::{
    ClientboundEntityEventPacket, entity_event_codec,
};
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
use rivet_registry::core::{ChunkPos, Difficulty, Vec3};
use rivet_registry::holder::Holder;
use rivet_registry::registries;
use rivet_registry::registries::{DimensionType, Level};

use crate::server::level::player_chunk_loader::PlayerChunkLoader;
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
const ENTITY_EVENT_ID: u32 = PacketType::EntityEvent.id();
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

/// `EntityEvent.PLAYER_OP_PERMISSION_LEVEL_ALL` — the op-level event
/// `PlayerList.sendPlayerPermissionLevel` sends for a level-4 operator. The M1
/// player is `GameType.Survival` with the default op level (4), matching the
/// capture's id-34 `entity_event` line.
const PERMISSION_LEVEL_ALL: i8 = 24;

/// The values `PlayerList.placeNewPlayer` reads off the server and the level to
/// build the login packet — a mix of server-derived (`max_players`, `level_keys`,
/// `online_mode`, `enforces_secure_chat`) and level/world-derived (`hardcore`,
/// `show_death_screen`, `reduced_debug_info`, `do_limited_crafting`, `is_flat`)
/// values. Immutable per join.
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
    /// `GameRules.REDUCED_DEBUG_INFO`.
    pub reduced_debug_info: bool,
    /// `GameRules.LIMITED_CRAFTING`.
    pub do_limited_crafting: bool,
    /// `ServerLevel.isFlat()` — true for the superflat M1 world.
    pub is_flat: bool,
}

/// `PlayerList.placeNewPlayer(connection, player, cookie)` — the deterministic
/// play join burst, in Paper's send order. Encode + queue each packet in the
/// `PLAY_BURST_ORDER` order (restricted to the ported members) and return the
/// ordered packet ids that were sent, for the ordering tests.
///
/// `update_recipes` is intentionally omitted: its body is not ported
/// (RivetTodo(#87)); the burst keeps Paper's order otherwise. The
/// `set_entity_data` pairing member (the addEntity tracker) is RivetTodo(#222).
/// The cache + chunk send-set (Moonrise's `level.addNewPlayer` `addPlayer`,
/// issue #100) is emitted immediately before the second `sendLevelInfo` so the
/// level-snapshot block stays contiguous and the chunk stream is the last burst
/// member. `requested_view_distance` is the client's `ClientInformation`
/// view distance (Slice B): the Moonrise ladder feeds it through `client + 1`,
/// so the send-set DOES depend on the client — the capture client's 8 caps at
/// `load - 1` (4) → 117 chunks; a `create_default` client's 2 resolves send 3
/// → 81 chunks. `None` (the auto-config path) resolves the world's own send
/// distance, 4 on the M1 world.
///
/// `teleport_id` is the live `awaitingTeleport` id (issue #158) the caller's
/// session began for the spawn teleport (`playerConnection.teleport` in
/// `placeNewPlayer`). Paper embeds it in the player_position packet and awaits
/// the matching `accept_teleportation` ack before accepting the player's
/// position movement.
#[allow(clippy::too_many_arguments)]
pub fn place_new_player(
    sender: &mut PlaySender,
    connections: &mut ConnectionRegistry,
    connection_id: ConnectionId,
    player: &ServerPlayer,
    level: &ServerLevel,
    join: &JoinConfig,
    requested_view_distance: Option<i32>,
    teleport_id: i32,
) -> Result<Vec<u32>, PlaySendError> {
    let mut sent = Vec::with_capacity(3 + 117 + 10);

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
        level.get_simulation_distance(),
        join.reduced_debug_info,
        join.show_death_screen,
        join.do_limited_crafting,
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

    // `this.sendPlayerPermissionLevel(player)` — `ClientboundEntityEventPacket`
    // with `EntityEvent.PLAYER_OP_PERMISSION_LEVEL_ALL` (24): the op-level
    // event. The entity id is the login `playerId` (1 on the M1 world), encoded
    // as a 4-byte BE int (NOT a VarInt — the packet's wire quirk). The capture's
    // id-34 body normalizes `entityId -> 1, eventId -> 0`, so the fixture does
    // not pin the real event id; Paper's source sends 24, asserted inline in
    // `join_burst.rs`.
    let body = sender.encode_body(
        entity_event_codec(),
        &ClientboundEntityEventPacket::new(player.player_id(), PERMISSION_LEVEL_ALL),
    )?;
    sender.send_packet(connections, connection_id, ENTITY_EVENT_ID, &body)?;
    sent.push(ENTITY_EVENT_ID);

    // `playerConnection.teleport(player.getX(), ...)` — the position teleport.
    // The embedded `awaitingTeleport` id is the live session's (issue #158):
    // Paper's `placeNewPlayer` teleport increments the per-connection counter
    // to 1 and awaits the matching `accept_teleportation` ack (the capture's
    // `id 0` was normalize.rs's canonical rewrite of the counter, not a Paper
    // value). The ack matches on this id; the server records the spawn as the
    // awaited position.
    let change = PositionMoveRotation::new(
        player.position(),
        Vec3::new(0.0, 0.0, 0.0),
        player.yaw(),
        player.pitch(),
    );
    let body = sender.encode_body(
        ClientboundPlayerPositionPacket::stream_codec(),
        &ClientboundPlayerPositionPacket::new(teleport_id, change, Vec::new()),
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

    // Paper's `level.addNewPlayer(player)` → Moonrise `PlayerChunkLoaderData
    // .add()` (issue #100): the per-player chunk loader, which this slice
    // synthesizes per join at the player's chunk position and sends its
    // add-send-set immediately before the second `sendLevelInfo`. The three
    // cache packets (radius → simulation distance → center) then the 117 bare
    // `level_chunk_with_light` bodies in the deterministic X-major raster the
    // #194 fixture byte-matches. `set_entity_data` (the addEntity tracker) is
    // RivetTodo(#222).
    //
    // The loader is owned by the tick thread (its `add_and_send_chunks` reads
    // the world's chunk map); it is not stored on the player — the per-player
    // movement-driven update (#185) that needs it is deferred, so the loader is
    // built, sent, and dropped here.
    let center = ChunkPos::containing(&rivet_registry::core::BlockPos::containing(
        player.position().x,
        player.position().y,
        player.position().z,
    ));
    let mut loader = PlayerChunkLoader::new(center);
    let chunk_packets = loader
        .add_and_send_chunks(level, requested_view_distance)
        .map_err(PlaySendError::Encode)?;
    for packet in chunk_packets {
        sender.send_packet(connections, connection_id, packet.id, &packet.body)?;
        sent.push(packet.id);
    }

    // `this.sendLevelInfo(player, level)` — Paper's SECOND `sendLevelInfo`
    // occurrence, after the player_info broadcast and the addEntity tracker. It
    // re-sends the same four level-snapshot members the first call did.
    sent.extend(send_level_info(sender, connections, connection_id, level)?);

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

//! Java-grounded tests for the issue #87 join clientbound packet bodies
//! (`crates/rivet-protocol/src/protocol/game/`).
//!
//! The `mc.network.protocol.game.join` manifest unit (MANIFEST line 262) ports
//! the join-critical clientbound bodies. The packets that appear in the pinned
//! #153 join capture (`tools/rivet-capture/fixtures/join/capture.jsonl`, protocol
//! 776, Paper `26.2-DEV-main@0a99345`, seed 42, superflat, offline `RivetProbe`)
//! are committed as hex golden bodies in `tests/fixtures/join_clientbound_*.hex`;
//! this suite decodes each one and pins the wire facts, then re-encodes
//! byte-identically.
//!
//! The four fixtures:
//! - `join_clientbound_login` (113 B): `playerId 1`, the three vanilla levels,
//!   the superflat `CommonPlayerSpawnInfo` (dimension-type holder id 0,
//!   `minecraft:overworld`, seed `0xC6F218BC089104ED`, `gameType 0`, no previous
//!   game type, `isFlat true`, `seaLevel -63`).
//! - `join_clientbound_player_info_update` (37 B): all eight actions set, one
//!   offline `RivetProbe` entry (`chatSession null`, `gameMode 0`, `listed 1`,
//!   `latency 0`, no display name, `listOrder 0`, `showHat 1`).
//! - `join_clientbound_player_position` (61 B): `id 0`, position
//!   `(0, -63, 0)`, zero delta, zero rotation, empty relatives. The fixture's
//!   `id 0` is the canonical capture normalization (the captured packet's
//!   leading teleport id was rewritten to 0 for the committed corpus); the live
//!   spawn teleport embeds a real `awaitingTeleport = 1` instead (issue #158),
//!   which `join_burst.rs` covers by rewriting the leading id varint of this
//!   body to `0x01`.
//! - `join_clientbound_set_time` (29 B): `gameTime 0`, two clock updates
//!   `{holder 0, (0, 0.0, 1.0)}`, `{holder 1, (0, 0.0, 1.0)}`.
//!
//! Registration pins the vanilla play/clientbound ids against the generated
//! tables (#50); the registry-aware bodies (`login`, `set_time`) are exercised
//! through real `RegistryAccess`es (their `CodecModifier` lift into the play
//! protocol builder is deferred with #126). The hostile/truncation tests pin
//! Java's panic-vs-error split, and the mutation tests are the do-not-weaken
//! counterfactual checks.
//!
//! Gated on the `packets` feature (the `game` body modules live behind it).

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets::play::clientbound::PacketType as PlayClientbound;
use rivet_protocol::generated::protocol::{ConnectionProtocol, PacketFlow};
use rivet_protocol::protocol::game::clientbound_change_difficulty::{
    ClientboundChangeDifficultyPacket, difficulty_stream_codec,
};
use rivet_protocol::protocol::game::clientbound_game_event::{
    ClientboundGameEventPacket, LEVEL_CHUNKS_LOAD_START,
};
use rivet_protocol::protocol::game::clientbound_initialize_border::ClientboundInitializeBorderPacket;
use rivet_protocol::protocol::game::clientbound_login::ClientboundLoginPacket;
use rivet_protocol::protocol::game::clientbound_player_abilities::ClientboundPlayerAbilitiesPacket;
use rivet_protocol::protocol::game::clientbound_player_info_remove::ClientboundPlayerInfoRemovePacket;
use rivet_protocol::protocol::game::clientbound_player_info_update::{
    Action as PlayerInfoAction, ClientboundPlayerInfoUpdatePacket,
};
use rivet_protocol::protocol::game::clientbound_player_position::{
    ClientboundPlayerPositionPacket, relative_set_stream_codec,
};
use rivet_protocol::protocol::game::clientbound_set_default_spawn::ClientboundSetDefaultSpawnPositionPacket;
use rivet_protocol::protocol::game::clientbound_set_held_slot::ClientboundSetHeldSlotPacket;
use rivet_protocol::protocol::game::clientbound_set_time::ClientboundSetTimePacket;
use rivet_protocol::protocol::game::clock_network_state::ClockNetworkState;
use rivet_protocol::protocol::game::packet_types as game_packet_types;
use rivet_protocol::protocol::game::position_move_rotation::PositionMoveRotation;
use rivet_protocol::protocol::game::{clientbound_bundle, clientbound_bundle_delimiter};
use rivet_protocol::protocol::{Packet, PacketType, clientbound_protocol};
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::core::{GameProfile, GameType, GlobalPos, Vec3};
use rivet_registry::holder::Holder;
use rivet_registry::registries;
use rivet_registry::registries::{DimensionType, WorldClock};
use rivet_registry::{Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey};
use rivet_util::uuid::Uuid;
use std::fmt;
use std::panic::catch_unwind;
use std::sync::Arc;

/// The captured RivetProbe UUID: `0a9ffa92-...` (offline `nameUUIDFromBytes`).
const RIVET_PROBE_ID: Uuid = Uuid {
    most: 0x0a9f_fa92_a706_3e6f,
    least: 0x900c_f12f_869d_37eau64 as i64,
};

/// Hex body -> `Vec<u8>`.
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Load a committed golden fixture (`tests/fixtures/{name}.hex`).
fn fixture_hex(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{}.hex", env!("CARGO_MANIFEST_DIR"), name);
    hex(std::fs::read_to_string(path).expect("fixture").trim())
}

fn buf() -> FriendlyByteBuf {
    FriendlyByteBuf::new(BytesMut::new())
}

fn written(b: FriendlyByteBuf) -> Vec<u8> {
    b.into_inner().to_vec()
}

fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
    let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(_) => panic!("expected the closure to panic"),
        Err(err) => err,
    };
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

/// The offline `RivetProbe` profile — id + name, no properties (the capture's
/// `player_info_update` property count is 0).
fn probe_profile() -> GameProfile {
    GameProfile::new_without_properties(RIVET_PROBE_ID, "RivetProbe".to_string())
}

fn overworld_key() -> ResourceKey<rivet_registry::registries::Level> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("overworld"),
    )
}

fn the_nether_key() -> ResourceKey<rivet_registry::registries::Level> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("the_nether"),
    )
}

fn the_end_key() -> ResourceKey<rivet_registry::registries::Level> {
    ResourceKey::create(
        &*registries::DIMENSION,
        Identifier::with_default_namespace("the_end"),
    )
}

/// A `DIMENSION_TYPE` registry with the spawn dimension-type at holder id 0.
/// The capture's `CommonPlayerSpawnInfo` carries `dimension-type holder id 0`,
/// and `holderRegistry(DIMENSION_TYPE)` is strict-bounds, so id 0 must exist.
fn dimension_type_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*registries::DIMENSION_TYPE);
    builder.register(
        &ResourceKey::create(
            &*registries::DIMENSION_TYPE,
            Identifier::with_default_namespace("overworld"),
        ),
        Arc::new(DimensionType),
        RegistrationInfo::BUILT_IN,
    );
    let registry = builder.freeze();
    RegistryAccess::from_single_registry((*registries::DIMENSION_TYPE).clone(), registry)
}

/// A `WORLD_CLOCK` registry with two entries (the capture's set-time clock
/// updates resolve holder ids 0 and 1).
fn world_clock_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*registries::WORLD_CLOCK);
    builder.register(
        &ResourceKey::create(
            &*registries::WORLD_CLOCK,
            Identifier::with_default_namespace("day"),
        ),
        Arc::new(WorldClock),
        RegistrationInfo::BUILT_IN,
    );
    builder.register(
        &ResourceKey::create(
            &*registries::WORLD_CLOCK,
            Identifier::with_default_namespace("weather"),
        ),
        Arc::new(WorldClock),
        RegistrationInfo::BUILT_IN,
    );
    let registry = builder.freeze();
    RegistryAccess::from_single_registry((*registries::WORLD_CLOCK).clone(), registry)
}

// ---------------------------------------------------------------------------
// Captured golden bodies: decode the fixture, pin the wire facts, re-encode
// byte-identically.
// ---------------------------------------------------------------------------

#[test]
fn login_fixture_round_trips_byte_identically() {
    let access = dimension_type_access();
    let bytes = fixture_hex("join_clientbound_login");
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
    let decoded = ClientboundLoginPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);

    assert_eq!(decoded.player_id(), 1);
    assert!(!decoded.hardcore());
    assert_eq!(
        decoded.levels(),
        &[overworld_key(), the_nether_key(), the_end_key()]
    );
    assert_eq!(decoded.max_players(), 20);
    assert_eq!(decoded.chunk_radius(), 4);
    assert_eq!(decoded.simulation_distance(), 4);
    assert!(!decoded.reduced_debug_info());
    assert!(decoded.show_death_screen());
    assert!(!decoded.do_limited_crafting());
    assert!(!decoded.online_mode());
    assert!(!decoded.enforces_secure_chat());

    // The superflat CommonPlayerSpawnInfo (field 4 of 12).
    let spawn = decoded.common_player_spawn_info();
    let registry = access.lookup(&*registries::DIMENSION_TYPE).unwrap();
    assert_eq!(
        *spawn.dimension_type(),
        Holder::Reference {
            registry: registry.registry_id(),
            id: 0
        }
    );
    assert_eq!(
        spawn.dimension().identifier().to_string(),
        "minecraft:overworld"
    );
    assert_eq!(spawn.seed(), 0xC6F218BC089104EDu64 as i64);
    assert_eq!(spawn.game_type(), GameType::Survival);
    assert_eq!(spawn.previous_game_type(), None);
    assert!(!spawn.is_debug());
    assert!(spawn.is_flat());
    assert_eq!(spawn.last_death_location(), None);
    assert_eq!(spawn.portal_cooldown(), 0);
    assert_eq!(spawn.sea_level(), -63);

    // Re-encode byte-identically.
    let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
    ClientboundLoginPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(out.into_inner().to_vec(), bytes);
}

#[test]
fn player_info_update_fixture_round_trips_byte_identically() {
    let bytes = fixture_hex("join_clientbound_player_info_update");
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundPlayerInfoUpdatePacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);

    assert_eq!(
        decoded.actions(),
        &[
            PlayerInfoAction::AddPlayer,
            PlayerInfoAction::InitializeChat,
            PlayerInfoAction::UpdateGameMode,
            PlayerInfoAction::UpdateListed,
            PlayerInfoAction::UpdateLatency,
            PlayerInfoAction::UpdateDisplayName,
            PlayerInfoAction::UpdateListOrder,
            PlayerInfoAction::UpdateHat,
        ]
    );
    assert_eq!(decoded.entries().len(), 1);
    let entry = &decoded.entries()[0];
    assert_eq!(entry.profile_id(), RIVET_PROBE_ID);
    assert_eq!(entry.profile(), Some(&probe_profile()));
    assert!(entry.listed());
    assert_eq!(entry.latency(), 0);
    assert_eq!(entry.game_mode(), GameType::Survival);
    assert_eq!(entry.display_name(), None);
    assert!(entry.show_hat());
    assert_eq!(entry.list_order(), 0);
    assert_eq!(entry.chat_session(), None);

    // Re-encode byte-identically.
    let mut out = buf();
    ClientboundPlayerInfoUpdatePacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(written(out), bytes);
}

#[test]
fn player_position_fixture_round_trips_byte_identically() {
    let bytes = fixture_hex("join_clientbound_player_position");
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    let decoded = ClientboundPlayerPositionPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);

    assert_eq!(decoded.id(), 0);
    assert_eq!(decoded.change().position(), Vec3::new(0.0, -63.0, 0.0));
    assert_eq!(decoded.change().delta_movement(), Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(decoded.change().y_rot(), 0.0);
    assert_eq!(decoded.change().x_rot(), 0.0);
    assert!(decoded.relatives().is_empty());

    // Re-encode byte-identically.
    let mut out = buf();
    ClientboundPlayerPositionPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(written(out), bytes);
}

#[test]
fn set_time_fixture_round_trips_byte_identically() {
    let access = world_clock_access();
    let bytes = fixture_hex("join_clientbound_set_time");
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access.clone());
    let decoded = ClientboundSetTimePacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(input.readable_bytes(), 0);

    assert_eq!(decoded.game_time(), 0);
    assert_eq!(decoded.clock_updates().len(), 2);
    let registry = access.lookup(&*registries::WORLD_CLOCK).unwrap();
    for (i, (holder, state)) in decoded.clock_updates().iter().enumerate() {
        assert_eq!(
            *holder,
            Holder::Reference {
                registry: registry.registry_id(),
                id: i as u32
            }
        );
        assert_eq!(*state, ClockNetworkState::new(0, 0.0, 1.0));
    }

    // Re-encode byte-identically.
    let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
    ClientboundSetTimePacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    assert_eq!(out.into_inner().to_vec(), bytes);
}

// ---------------------------------------------------------------------------
// Registration: play/clientbound vanilla ids pinned against the generated
// tables (#50). The plain-`FriendlyByteBuf` bodies register directly; the
// registry-aware `login`/`set_time` are deferred from the builder with the
// `CodecModifier` lift (#126) but their vanilla ids are pinned all the same.
// ---------------------------------------------------------------------------

/// The erased play/clientbound join value for the plain-`FriendlyByteBuf`
/// bodies (Java's dispatch-table erasure).
#[derive(Debug, Clone, PartialEq)]
enum PlayClientboundJoin {
    BundleDelimiter,
    ChangeDifficulty(ClientboundChangeDifficultyPacket),
    GameEvent(ClientboundGameEventPacket),
    InitializeBorder(ClientboundInitializeBorderPacket),
    PlayerAbilities(ClientboundPlayerAbilitiesPacket),
    PlayerInfoRemove(ClientboundPlayerInfoRemovePacket),
    PlayerInfoUpdate(ClientboundPlayerInfoUpdatePacket),
    PlayerPosition(ClientboundPlayerPositionPacket),
    SetDefaultSpawnPosition(ClientboundSetDefaultSpawnPositionPacket),
    SetHeldSlot(ClientboundSetHeldSlotPacket),
}

impl Packet for PlayClientboundJoin {
    fn packet_type(&self) -> PacketType {
        match self {
            PlayClientboundJoin::BundleDelimiter => {
                game_packet_types::clientbound_bundle_delimiter()
            }
            PlayClientboundJoin::ChangeDifficulty(_) => {
                game_packet_types::clientbound_change_difficulty()
            }
            PlayClientboundJoin::GameEvent(_) => game_packet_types::clientbound_game_event(),
            PlayClientboundJoin::InitializeBorder(_) => {
                game_packet_types::clientbound_initialize_border()
            }
            PlayClientboundJoin::PlayerAbilities(_) => {
                game_packet_types::clientbound_player_abilities()
            }
            PlayClientboundJoin::PlayerInfoRemove(_) => {
                game_packet_types::clientbound_player_info_remove()
            }
            PlayClientboundJoin::PlayerInfoUpdate(_) => {
                game_packet_types::clientbound_player_info_update()
            }
            PlayClientboundJoin::PlayerPosition(_) => {
                game_packet_types::clientbound_player_position()
            }
            PlayClientboundJoin::SetDefaultSpawnPosition(_) => {
                game_packet_types::clientbound_set_default_spawn_position()
            }
            PlayClientboundJoin::SetHeldSlot(_) => game_packet_types::clientbound_set_held_slot(),
        }
    }
}

impl fmt::Display for PlayClientboundJoin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

/// `StreamCodec.map` wrap/unwrap for a concrete body codec into the erased enum
/// value — Java's dispatch-table erasure.
fn wrap_codec<V: 'static, E: 'static>(
    codec: StreamCodec<FriendlyByteBuf, V>,
    wrap: impl Fn(&V) -> E + Send + Sync + 'static,
    unwrap: impl Fn(&E) -> V + Send + Sync + 'static,
) -> StreamCodec<FriendlyByteBuf, E> {
    map(codec, wrap, unwrap)
}

#[test]
fn registration_order_matches_generated_vanilla_ids() {
    let template = clientbound_protocol::<PlayClientboundJoin>(ConnectionProtocol::Play, |b| {
        b.with_bundle_packet(
            game_packet_types::clientbound_bundle(),
            PlayClientboundJoin::BundleDelimiter,
        )
        .add_packet(
            game_packet_types::clientbound_change_difficulty(),
            wrap_codec(
                ClientboundChangeDifficultyPacket::stream_codec(),
                |v: &ClientboundChangeDifficultyPacket| PlayClientboundJoin::ChangeDifficulty(*v),
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::ChangeDifficulty(v) => *v,
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_game_event(),
            wrap_codec(
                ClientboundGameEventPacket::stream_codec(),
                |v: &ClientboundGameEventPacket| PlayClientboundJoin::GameEvent(*v),
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::GameEvent(v) => *v,
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_initialize_border(),
            wrap_codec(
                ClientboundInitializeBorderPacket::stream_codec(),
                |v: &ClientboundInitializeBorderPacket| PlayClientboundJoin::InitializeBorder(*v),
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::InitializeBorder(v) => *v,
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_player_abilities(),
            wrap_codec(
                ClientboundPlayerAbilitiesPacket::stream_codec(),
                |v: &ClientboundPlayerAbilitiesPacket| PlayClientboundJoin::PlayerAbilities(*v),
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::PlayerAbilities(v) => *v,
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_player_info_remove(),
            wrap_codec(
                ClientboundPlayerInfoRemovePacket::stream_codec(),
                |v: &ClientboundPlayerInfoRemovePacket| {
                    PlayClientboundJoin::PlayerInfoRemove(v.clone())
                },
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::PlayerInfoRemove(v) => v.clone(),
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_player_info_update(),
            wrap_codec(
                ClientboundPlayerInfoUpdatePacket::stream_codec(),
                |v: &ClientboundPlayerInfoUpdatePacket| {
                    PlayClientboundJoin::PlayerInfoUpdate(v.clone())
                },
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::PlayerInfoUpdate(v) => v.clone(),
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_player_position(),
            wrap_codec(
                ClientboundPlayerPositionPacket::stream_codec(),
                |v: &ClientboundPlayerPositionPacket| {
                    PlayClientboundJoin::PlayerPosition(v.clone())
                },
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::PlayerPosition(v) => v.clone(),
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_set_default_spawn_position(),
            wrap_codec(
                ClientboundSetDefaultSpawnPositionPacket::stream_codec(),
                |v: &ClientboundSetDefaultSpawnPositionPacket| {
                    PlayClientboundJoin::SetDefaultSpawnPosition(v.clone())
                },
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::SetDefaultSpawnPosition(v) => v.clone(),
                    _ => unreachable!(),
                },
            ),
        )
        .add_packet(
            game_packet_types::clientbound_set_held_slot(),
            wrap_codec(
                ClientboundSetHeldSlotPacket::stream_codec(),
                |v: &ClientboundSetHeldSlotPacket| PlayClientboundJoin::SetHeldSlot(*v),
                |p: &PlayClientboundJoin| match p {
                    PlayClientboundJoin::SetHeldSlot(v) => *v,
                    _ => unreachable!(),
                },
            ),
        );
    });

    // Registration order -> this slice's sequential ids (bundle_delimiter owns 0
    // via withBundlePacket, then the ten plain-codec bodies in GamePacketTypes
    // order).
    assert_eq!(
        template.details().list_packets(),
        &[
            (game_packet_types::clientbound_bundle_delimiter(), 0),
            (game_packet_types::clientbound_change_difficulty(), 1),
            (game_packet_types::clientbound_game_event(), 2),
            (game_packet_types::clientbound_initialize_border(), 3),
            (game_packet_types::clientbound_player_abilities(), 4),
            (game_packet_types::clientbound_player_info_remove(), 5),
            (game_packet_types::clientbound_player_info_update(), 6),
            (game_packet_types::clientbound_player_position(), 7),
            (
                game_packet_types::clientbound_set_default_spawn_position(),
                8
            ),
            (game_packet_types::clientbound_set_held_slot(), 9),
        ]
    );
    // The generated table (#50) pins the vanilla addPacket-order ids.
    assert_eq!(PlayClientbound::BundleDelimiter.id(), 0);
    assert_eq!(PlayClientbound::ChangeDifficulty.id(), 10);
    assert_eq!(PlayClientbound::GameEvent.id(), 38);
    assert_eq!(PlayClientbound::InitializeBorder.id(), 43);
    assert_eq!(PlayClientbound::Login.id(), 49);
    assert_eq!(PlayClientbound::PlayerAbilities.id(), 64);
    assert_eq!(PlayClientbound::PlayerInfoRemove.id(), 69);
    assert_eq!(PlayClientbound::PlayerInfoUpdate.id(), 70);
    assert_eq!(PlayClientbound::PlayerPosition.id(), 72);
    assert_eq!(PlayClientbound::SetDefaultSpawnPosition.id(), 97);
    assert_eq!(PlayClientbound::SetHeldSlot.id(), 105);
    assert_eq!(PlayClientbound::SetTime.id(), 113);
    assert_eq!(PlayClientbound::UpdateRecipes.id(), 133);

    // The discriminator constants carry the canonical names + clientbound flow.
    assert_eq!(
        game_packet_types::clientbound_login().id().to_string(),
        "minecraft:login"
    );
    assert_eq!(
        game_packet_types::clientbound_set_time().id().to_string(),
        "minecraft:set_time"
    );
    assert_eq!(
        game_packet_types::clientbound_update_recipes()
            .id()
            .to_string(),
        "minecraft:update_recipes"
    );
    for p in [
        game_packet_types::clientbound_bundle(),
        game_packet_types::clientbound_bundle_delimiter(),
        game_packet_types::clientbound_login(),
        game_packet_types::clientbound_set_time(),
    ] {
        assert_eq!(p.flow(), PacketFlow::Clientbound);
    }
}

/// Compile-time bound: the type implements the `BundlePacket` marker.
fn assert_bundle_marker<T: rivet_protocol::protocol::bundle::BundlePacket>(_t: &T) {}
/// Compile-time bound: the type implements the `BundleDelimiterPacket` marker.
fn assert_bundle_delimiter_marker<T: rivet_protocol::protocol::bundle::BundleDelimiterPacket>(
    _t: &T,
) {
}

#[test]
fn bundle_marker_types_carry_play_identities() {
    // The concrete play bundle markers (Java `ClientboundBundlePacket`/
    // `ClientboundBundleDelimiterPacket`) implement the bundle marker traits and
    // report GamePacketTypes.CLIENTBOUND_BUNDLE(_DELIMITER).
    let delim = clientbound_bundle_delimiter::ClientboundBundleDelimiterPacket;
    assert_eq!(
        delim.packet_type(),
        game_packet_types::clientbound_bundle_delimiter()
    );
    assert_bundle_delimiter_marker(&delim);

    let bundle = clientbound_bundle::ClientboundBundlePacket;
    assert_eq!(
        bundle.packet_type(),
        game_packet_types::clientbound_bundle()
    );
    assert_bundle_marker(&bundle);
}

// ---------------------------------------------------------------------------
// Hostile wire: truncation, invalid identifiers, negative counts.
// ---------------------------------------------------------------------------

#[test]
fn login_every_truncated_prefix_panics() {
    // The `read_*` primitives panic on insufficient bytes (netty EOF); every
    // field consumes at least one byte, so no proper prefix of the captured
    // login body may decode into a partial packet.
    let access = dimension_type_access();
    let full = fixture_hex("join_clientbound_login");
    for len in 0..full.len() {
        let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(&full[..len]), access.clone());
        let msg = panic_message(|| {
            let _ = ClientboundLoginPacket::stream_codec().decode(&mut input);
        });
        assert!(!msg.is_empty(), "prefix of len {len} did not panic");
    }
}

#[test]
fn login_invalid_level_identifier_panics_like_java() {
    // A level key that is not a valid Identifier (`Identifier.parse` throws a
    // `ResourceLocationException`, a RuntimeException) panics with Java's text.
    let access = dimension_type_access();
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
    input.write_int(1); // playerId
    input.write_boolean(false); // hardcore
    input.write_var_int(1); // one level
    input.write_utf("!!!"); // invalid identifier chars
    let msg = panic_message(|| {
        let _ = ClientboundLoginPacket::stream_codec().decode(&mut input);
    });
    assert_eq!(
        msg,
        "Non [a-z0-9/._-] character in path of location: minecraft:!!!"
    );
}

#[test]
fn player_info_update_truncated_entry_panics() {
    // The fixture's entry UUID is 16 bytes; a declared entry whose UUID is
    // truncated hits EOF and panics (the raw `read_uuid` scalar path).
    let mut input = buf();
    input.write_byte(0xFFu8 as i8); // all eight actions
    input.write_var_int(1); // one entry
    input.write_long(1); // 8 of the 16 uuid bytes, then EOF
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundPlayerInfoUpdatePacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );
}

#[test]
fn player_info_update_oversize_public_key_panics_like_java() {
    // The INITIALIZE_CHAT action reads `ProfilePublicKey.Data` whose public key
    // is `readByteArray(MAX_PUBLIC_KEY_LENGTH=512)`. A hostile wire declaring a
    // 513-byte key panics with Java's DecoderException text — the connection
    // closes, not silently.
    let mut input = buf();
    input.write_byte(0x02); // only INITIALIZE_CHAT
    input.write_var_int(1); // one entry
    input.write_uuid(RIVET_PROBE_ID);
    input.write_boolean(true); // chat session present
    input.write_uuid(RIVET_PROBE_ID); // sessionId
    input.write_long(0); // expiresAt epoch-milli
    input.write_var_int(513); // public key length, over the 512 bound
    let msg = panic_message(|| {
        let _ = ClientboundPlayerInfoUpdatePacket::stream_codec().decode(&mut input);
    });
    assert_eq!(msg, "ByteArray with size 513 is bigger than allowed 512");
}

#[test]
fn player_info_update_negative_property_count_passes_like_java() {
    // Java's `PropertyMap` read uses `readCount` (upper-bound only) then
    // iterates; a negative count passes and the profile keeps zero properties.
    let mut out = buf();
    out.write_byte(0x01); // only ADD_PLAYER
    out.write_var_int(1); // one entry
    out.write_uuid(RIVET_PROBE_ID);
    out.write_utf_max("RivetProbe", 16);
    out.write_var_int(-1); // negative property count
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let decoded = ClientboundPlayerInfoUpdatePacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(decoded.actions(), &[PlayerInfoAction::AddPlayer]);
    assert_eq!(decoded.entries()[0].profile(), Some(&probe_profile()));
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn set_time_negative_clock_count_panics_like_java() {
    // Java `ByteBufCodecs.map(HashMap::new, ...)` reads the count, upper-bounds
    // it at 65536, then `new HashMap<>(capacity)` throws
    // `IllegalArgumentException("Illegal initial capacity: -n")` on a negative
    // count (the ArrayList message "Illegal Capacity:" is the update_attributes
    // case; HashMap uses its own text).
    let access = world_clock_access();
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
    input.write_long(0); // gameTime
    input.write_var_int(-1); // clock count -1
    let msg = panic_message(|| {
        let _ = ClientboundSetTimePacket::stream_codec().decode(&mut input);
    });
    assert_eq!(msg, "Illegal initial capacity: -1");
}

#[test]
fn set_time_truncated_clock_state_panics() {
    // A count of 1 with a truncated ClockNetworkState (only the holder id) hits
    // EOF on the state read and panics.
    let access = world_clock_access();
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
    input.write_long(0); // gameTime
    input.write_var_int(1); // one clock update
    input.write_var_int(0); // holder id 0
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundSetTimePacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );
}

#[test]
fn player_position_truncated_position_panics() {
    // id varint 0, then only two doubles of the position Vec3 -> EOF panic.
    let mut input = buf();
    input.write_var_int(0); // id
    input.write_double(1.0);
    input.write_double(2.0);
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundPlayerPositionPacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );
}

// ---------------------------------------------------------------------------
// Controlled mutations (the do-not-weaken counterfactual checks).
// ---------------------------------------------------------------------------

#[test]
fn player_abilities_reserved_flag_bits_are_dropped_on_reencode() {
    // The wire bitfield only defines four flags (1, 2, 4, 8). A hostile/mutated
    // wire sets a reserved bit (0x10); decode reads only the defined flags, so
    // a re-encode writes the defined flags only — Java's bit-test loop.
    let mut input = buf();
    input.write_byte(0x10 | 0x01); // reserved bit + INVULNERABLE
    input.write_float(0.05);
    input.write_float(0.1);
    let decoded = ClientboundPlayerAbilitiesPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert!(decoded.is_invulnerable());
    assert!(!decoded.is_flying());
    let mut out = buf();
    ClientboundPlayerAbilitiesPacket::stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    // Re-encode drops the reserved bit: 0x01 then the two speeds.
    assert_eq!(written(out), hex("013d4ccccd3dcccccd"));
}

#[test]
fn player_info_update_action_field_order_mutation_changes_bytes() {
    // The per-entry field order is the Action ordinal order. A mutation that
    // swapped two action fields (e.g. writing `listed` before `gameMode`) would
    // change the wire bytes; the byte-exact fixture pins the real order.
    let value = {
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            fixture_hex("join_clientbound_player_info_update").as_slice(),
        ));
        ClientboundPlayerInfoUpdatePacket::stream_codec()
            .decode(&mut input)
            .unwrap()
    };
    let mut real = buf();
    ClientboundPlayerInfoUpdatePacket::stream_codec()
        .encode(&mut real, &value)
        .unwrap();
    let real_bytes = written(real);

    // The real order after the UUID is: name+props, chat, gameMode, listed,
    // latency, display, listOrder, showHat. Swap gameMode and listed -> the
    // bytes `01 00` (listed true, then gameMode 0) would come before `00 01`
    // (chat null then gameMode 0 is NOT what Java does — Java writes chat null
    // first). Assert the real prefix differs from a mutated ordering.
    let mut swapped = buf();
    swapped.write_byte(0xFFu8 as i8);
    swapped.write_var_int(1);
    swapped.write_uuid(RIVET_PROBE_ID);
    swapped.write_utf_max("RivetProbe", 16);
    swapped.write_var_int(0); // props
    swapped.write_boolean(false); // chat null
    swapped.write_boolean(true); // listed (mutated: before gameMode)
    swapped.write_var_int(0); // gameMode (mutated: after listed)
    swapped.write_var_int(0); // latency
    swapped.write_boolean(false); // display null
    swapped.write_var_int(0); // listOrder
    swapped.write_boolean(true); // showHat
    assert_ne!(swapped.into_inner().to_vec(), real_bytes);
}

#[test]
fn player_position_relative_set_packs_int_bits() {
    // `Relative.SET_STREAM_CODEC` is `ByteBufCodecs.INT.map(unpack, pack)` — a
    // big-endian int bitmask, not a list. X (bit 0) + X_ROT (bit 4) -> 0x11.
    let set = vec![
        rivet_registry::core::Relative::X,
        rivet_registry::core::Relative::XRot,
    ];
    let mut out = buf();
    relative_set_stream_codec().encode(&mut out, &set).unwrap();
    assert_eq!(written(out), hex("00000011"));
    let mut input = FriendlyByteBuf::new(BytesMut::from(hex("00000011").as_slice()));
    assert_eq!(relative_set_stream_codec().decode(&mut input).unwrap(), set);
}

#[test]
fn change_difficulty_wraps_out_of_range_id() {
    // `Difficulty.STREAM_CODEC` uses `BY_ID` with WRAP: byte 5 maps back to
    // NORMAL's neighbor — `floorMod(5, 4) = 1` -> EASY (Java WRAP semantics).
    let mut input = buf();
    input.write_byte(5);
    input.write_boolean(false);
    let decoded = difficulty_stream_codec().decode(&mut input).unwrap();
    assert_eq!(decoded, rivet_registry::core::Difficulty::Easy);
    let mut out = buf();
    difficulty_stream_codec()
        .encode(&mut out, &decoded)
        .unwrap();
    // Re-encode writes the canonical id 1.
    assert_eq!(written(out), vec![1]);
}

#[test]
fn game_event_level_chunks_load_start_matches_capture() {
    // The join capture's game_event is `LEVEL_CHUNKS_LOAD_START` (id 13),
    // param 0.0 — the game_event the flat world emits during placeNewPlayer.
    let packet = ClientboundGameEventPacket::new(LEVEL_CHUNKS_LOAD_START, 0.0);
    let mut out = buf();
    ClientboundGameEventPacket::stream_codec()
        .encode(&mut out, &packet)
        .unwrap();
    assert_eq!(written(out), hex("0d00000000"));
}

#[test]
fn set_default_spawn_position_wire_has_global_pos_then_yaw_pitch() {
    // `LevelData.RespawnData.STREAM_CODEC`: GlobalPos (dimension identifier +
    // packed BlockPos long), then yaw and pitch floats. The captured spawn is
    // `minecraft:overworld` at `BlockPos(0, -63, 0)` with zero rotation.
    let value = ClientboundSetDefaultSpawnPositionPacket::new(
        rivet_protocol::protocol::game::clientbound_set_default_spawn::RespawnData::new(
            GlobalPos::of(
                overworld_key(),
                rivet_registry::core::BlockPos::new(0, -63, 0),
            ),
            0.0,
            0.0,
        ),
    );
    let mut out = buf();
    ClientboundSetDefaultSpawnPositionPacket::stream_codec()
        .encode(&mut out, &value)
        .unwrap();
    assert_eq!(
        written(out),
        hex("136d696e6563726166743a6f766572776f726c640000000000000fc10000000000000000")
    );
}

#[test]
fn position_move_rotation_and_set_held_slot_round_trip() {
    let pmr = PositionMoveRotation::new(
        Vec3::new(1.5, -63.0, 2.25),
        Vec3::new(0.0, 0.0, 0.0),
        10.0,
        -20.0,
    );
    let mut out = buf();
    PositionMoveRotation::stream_codec()
        .encode(&mut out, &pmr)
        .unwrap();
    let bytes = written(out);
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    assert_eq!(
        PositionMoveRotation::stream_codec()
            .decode(&mut input)
            .unwrap(),
        pmr
    );
    assert_eq!(input.readable_bytes(), 0);

    let held = ClientboundSetHeldSlotPacket::new(0);
    let mut out = buf();
    ClientboundSetHeldSlotPacket::stream_codec()
        .encode(&mut out, &held)
        .unwrap();
    assert_eq!(written(out), vec![0]);
}

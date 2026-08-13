//! `server::player` — the join-burst tests (Slices A + B of #101):
//! `PlaySender` encodes + queues frames, `ServerPlayer`/`PlayerIndices` model
//! the session, and `place_new_player`/`send_level_info` emit the Paper-faithful
//! play join burst.
//!
//! Ground truth: the `tools/rivet-capture/fixtures/join/capture.jsonl` — the
//! NORMALIZED #153 capture (protocol 776, Paper `26.2-DEV-main@0a99345`,
//! offline superflat world, seed 42, view distance 4). The committed capture is
//! the deterministic canonical form (`normalize::canonicalize`), not the raw
//! transcript. The `canonicalize` function groups by `(state, direction, id)`,
//! so the normalized capture supplies the BODIES but erases ORDER — the burst
//! order comes from `PLAY_BURST_ORDER` / the Paper source, not from the
//! capture's positional adjacency. Each body is the packet payload the capture
//! proxy strips (packet id + compression prefix already removed, randomized
//! fields canonicalized), so the byte-exact assertions compare an encoded burst
//! member against the capture's normalized body. The `rivet-protocol`
//! `join_clientbound_*.hex` fixtures pin the same bodies; the #194
//! `chunk_golden_full.hex` pins the superflat chunk content. Slice B emits the
//! full ported burst: `[49,10,64,105,34,72,43,113,97,38,70]`, the #100 cache
//! packets (95/111/94), the 117 chunks (45), then the second `sendLevelInfo`
//! (43/113/97/38) — the members Paper sends in between (see the authoritative
//! list in `join.rs`'s module doc) are deferred.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use bytes::BytesMut;
use rivet_protocol::codec::StreamDecoder;
use rivet_protocol::generated::packets::play::clientbound::PacketType;
use rivet_protocol::protocol::game::clientbound_login::ClientboundLoginPacket;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_protocol::var_int;
use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::core::{GameProfile, GameType, Vec3, create_offline_player_uuid};
use rivet_registry::registries;
use rivet_registry::{RegistrationInfo, RegistryAccess, RegistryBuilder};
use rivet_server::server::level::player_chunk_loader::PlayerChunkLoader;
use rivet_server::server::level::server_level::{ServerLevel, ServerLevelConfig};
use rivet_server::server::network::connection_id::ConnectionId;
use rivet_server::server::player::join::{JoinConfig, place_new_player, send_level_info};
use rivet_server::server::player::play_sender::{PlaySendError, PlaySender};
use rivet_server::server::player::{PlayerIndices, ServerPlayer};
use rivet_server::server::tick::registry::ConnectionRegistry;

const REMOTE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);
const PROBE: &str = "RivetProbe";

/// The `ConnectionId` for the probe's connection (a stable u64 for tests).
const PROBE_ID: ConnectionId = ConnectionId(1);

/// The offline probe profile the capture records (`create_offline_player_uuid`).
fn probe_profile() -> GameProfile {
    GameProfile::new_without_properties(create_offline_player_uuid(PROBE), PROBE.to_string())
}

/// The probe `ServerPlayer` — spawn (0,-63,0) zero-rotation Survival, matching
/// the capture's login/teleport/player-info values.
fn probe_player() -> ServerPlayer {
    ServerPlayer::new(
        PROBE_ID,
        probe_profile(),
        1,
        Vec3::new(0.0, -63.0, 0.0),
        0.0,
        0.0,
        GameType::Survival,
    )
}

/// The three vanilla levels the login `levels` list carries (capture-grounded).
fn vanilla_level_keys() -> Vec<ResourceKey<rivet_registry::registries::Level>> {
    vec![
        ResourceKey::create(
            &*registries::DIMENSION,
            Identifier::with_default_namespace("overworld"),
        ),
        ResourceKey::create(
            &*registries::DIMENSION,
            Identifier::with_default_namespace("the_nether"),
        ),
        ResourceKey::create(
            &*registries::DIMENSION,
            Identifier::with_default_namespace("the_end"),
        ),
    ]
}

/// The `JoinConfig` the M1 server passes (`max_players 20`, offline, not
/// hardcore, flat superflat world with the death screen on; the
/// `reduced_debug_info`/`do_limited_crafting` game rules are off).
fn join_config() -> JoinConfig {
    JoinConfig {
        max_players: 20,
        hardcore: false,
        level_keys: vanilla_level_keys(),
        online_mode: false,
        enforces_secure_chat: false,
        show_death_screen: true,
        reduced_debug_info: false,
        do_limited_crafting: false,
        is_flat: true,
    }
}

/// The M1 superflat world (seed 42, view distance 4, spawn (0,-63,0)).
fn world() -> ServerLevel {
    ServerLevel::new(ServerLevelConfig::default())
}

/// A `DIMENSION_TYPE` registry/access with one `overworld` entry (holder id 0),
/// mirroring the capture's spawn-info `dimension_type` holder id 0.
fn dimension_type_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*registries::DIMENSION_TYPE);
    builder.register(
        &ResourceKey::create(
            &*registries::DIMENSION_TYPE,
            Identifier::with_default_namespace("overworld"),
        ),
        Arc::new(rivet_registry::registries::DimensionType),
        RegistrationInfo::BUILT_IN,
    );
    RegistryAccess::from_single_registry((*registries::DIMENSION_TYPE).clone(), builder.freeze())
}

/// A `WORLD_CLOCK` registry/access with the two clocks the flat world runs
/// (day/night and weather, holder ids 0/1), matching the capture's set-time
/// clock-update holders.
fn world_clock_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*registries::WORLD_CLOCK);
    builder.register(
        &ResourceKey::create(
            &*registries::WORLD_CLOCK,
            Identifier::with_default_namespace("day"),
        ),
        Arc::new(rivet_registry::registries::WorldClock),
        RegistrationInfo::BUILT_IN,
    );
    builder.register(
        &ResourceKey::create(
            &*registries::WORLD_CLOCK,
            Identifier::with_default_namespace("weather"),
        ),
        Arc::new(rivet_registry::registries::WorldClock),
        RegistrationInfo::BUILT_IN,
    );
    RegistryAccess::from_single_registry((*registries::WORLD_CLOCK).clone(), builder.freeze())
}

/// The `PlaySender` for the M1 server (compression threshold 256, the two
/// single-registry accesses).
fn play_sender() -> PlaySender {
    PlaySender::new(256, dimension_type_access(), world_clock_access())
}

/// A `ConnectionRegistry` with `PROBE_ID` connected over a bounded channel.
/// Returns the registry and the outbound receiver (the network-side reader).
fn registry_with_connection() -> (
    ConnectionRegistry,
    tokio::sync::mpsc::Receiver<rivet_server::server::tick::channels::OutboundEvent>,
) {
    let mut registry = ConnectionRegistry::new();
    let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
    // The whole 135-frame burst is queued before the test drains, so the
    // outbound channel must hold every frame (Paper's 1024 default does).
    let (out_tx, out_rx) = tokio::sync::mpsc::channel(256);
    registry.apply(
        rivet_server::server::tick::channels::LifecycleEvent::Connect {
            id: PROBE_ID,
            remote: REMOTE,
            in_rx,
            out_tx,
            drained: rivet_server::server::tick::channels::InboundDrained::new(),
        },
    );
    let _ = in_tx;
    (registry, out_rx)
}

/// Decode a `hex`-string (the fixture/capture body format) to bytes.
fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// Load a committed `rivet-protocol` fixture body
/// (`crates/rivet-protocol/tests/fixtures/{name}.hex` — the join-capture
/// golden bodies).
fn protocol_fixture(name: &str) -> Vec<u8> {
    let hex = std::fs::read_to_string(format!(
        "{}/../rivet-protocol/tests/fixtures/{name}.hex",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture exists");
    hex_bytes(hex.trim())
}

/// Decode one outbound `frame` back to `(packet_id, body)` using the real
/// VarInt21 frame decoder, the compression decoder, and VarInt readers — no
/// fixed 1-byte offset assumptions, so future >=128-byte bodies or packet ids
/// decode correctly. Sub-threshold bodies (the join members) are uncompressed
/// (`varint(0)`); the 7279-byte chunk bodies exceed the 256 threshold and are
/// zlib-compressed (`varint(declaredLen) ++ zlib`).
fn decode_frame(frame: &[u8]) -> (u32, Vec<u8>) {
    let mut buf = BytesMut::from(frame);
    let wire = Varint21FrameDecoder::new(None)
        .decode(&mut buf)
        .expect("frame decodes")
        .expect("frame present");
    let mut wire = BytesMut::from(&wire[..]);
    let declared_len = var_int::read(&mut wire);
    let payload = if declared_len == 0 {
        wire.to_vec()
    } else {
        let mut decoder = flate2::read::ZlibDecoder::new(&wire[..]);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut out).expect("zlib inflate");
        out
    };
    let mut payload = BytesMut::from(&payload[..]);
    let packet_id = var_int::read(&mut payload) as u32;
    (packet_id, payload.to_vec())
}

/// The `PacketType` id constants the burst asserts against.
mod ids {
    use super::*;
    pub const LOGIN: u32 = PacketType::Login.id();
    pub const CHANGE_DIFFICULTY: u32 = PacketType::ChangeDifficulty.id();
    pub const PLAYER_ABILITIES: u32 = PacketType::PlayerAbilities.id();
    pub const SET_HELD_SLOT: u32 = PacketType::SetHeldSlot.id();
    pub const ENTITY_EVENT: u32 = PacketType::EntityEvent.id();
    pub const PLAYER_POSITION: u32 = PacketType::PlayerPosition.id();
    pub const INITIALIZE_BORDER: u32 = PacketType::InitializeBorder.id();
    pub const SET_TIME: u32 = PacketType::SetTime.id();
    pub const SET_DEFAULT_SPAWN: u32 = PacketType::SetDefaultSpawnPosition.id();
    pub const GAME_EVENT: u32 = PacketType::GameEvent.id();
    pub const PLAYER_INFO_UPDATE: u32 = PacketType::PlayerInfoUpdate.id();
    pub const SET_CHUNK_CACHE_RADIUS: u32 = PacketType::SetChunkCacheRadius.id();
    pub const SET_SIMULATION_DISTANCE: u32 = PacketType::SetSimulationDistance.id();
    pub const SET_CHUNK_CACHE_CENTER: u32 = PacketType::SetChunkCacheCenter.id();
    pub const LEVEL_CHUNK_WITH_LIGHT: u32 = PacketType::LevelChunkWithLight.id();
}

// ---- PlayerIndices ----------------------------------------------------------

#[test]
fn player_indices_insert_lookup_remove() {
    let mut indices = PlayerIndices::default();
    let uuid = probe_profile().id();
    assert!(indices.is_empty());

    indices.insert(uuid, PROBE_ID);
    assert_eq!(indices.len(), 1);
    assert_eq!(indices.connection_for(&uuid), Some(PROBE_ID));
    assert_eq!(indices.uuid_for(PROBE_ID), Some(uuid));

    // Reverse lookup is kept in sync on insert and remove.
    let other = ConnectionId(2);
    indices.insert(uuid, other);
    assert_eq!(indices.connection_for(&uuid), Some(other));
    assert_eq!(indices.uuid_for(PROBE_ID), None);

    indices.remove(&uuid);
    assert!(indices.is_empty());
    assert_eq!(indices.connection_for(&uuid), None);
    assert_eq!(indices.uuid_for(other), None);
}

// ---- PlaySender framing -----------------------------------------------------

#[test]
fn play_sender_frames_below_threshold_are_uncompressed() {
    // A body below the 256-byte threshold encodes as
    // `varint(packet_id) ++ body`, then `varint21(len) ++ wire` — the
    // compression prefix is `varint(0)` (uncompressed).
    let mut sender = play_sender();
    let body = vec![0xab; 10];
    let frame = sender.encode_frame(ids::LOGIN, &body).expect("encode");
    // `wire = varint(0) ++ varint(packet_id) ++ body` = 1 + 1 + 10 = 12 bytes;
    // the VarInt21 length prefix counts those 12 bytes.
    assert_eq!(frame[0], 12);
    assert_eq!(frame[1], 0, "below-threshold frames carry the raw prefix");
    assert_eq!(frame[2], ids::LOGIN as u8);
    assert_eq!(&frame[3..], &body[..]);
}

#[test]
fn decode_frame_round_trips_multibyte_varints() {
    // A body >= 128 bytes (still below the 256 compression threshold) forces a
    // two-byte VarInt21 length prefix; a packet id >= 128 forces a two-byte id
    // varint. `decode_frame` must round-trip both (no 1-byte assumptions).
    let mut sender = play_sender();
    let body = vec![0xcd; 200];
    let frame = sender.encode_frame(300, &body).expect("encode");
    let (packet_id, decoded_body) = decode_frame(&frame);
    assert_eq!(packet_id, 300);
    assert_eq!(decoded_body, body);
}

#[test]
fn play_sender_send_over_connection_reaches_outbound_channel() {
    let mut sender = play_sender();
    let (mut connections, mut out_rx) = registry_with_connection();
    let body = vec![0xcd; 4];
    sender
        .send_packet(&mut connections, PROBE_ID, ids::LOGIN, &body)
        .expect("send");
    match out_rx.blocking_recv() {
        Some(rivet_server::server::tick::channels::OutboundEvent::Packet { frame }) => {
            assert!(frame.len() >= body.len() + 2, "id + length + body");
        }
        other => panic!("expected Packet, got {other:?}"),
    }
}

#[test]
fn play_sender_send_to_missing_connection_errors_gone() {
    let mut sender = play_sender();
    let mut connections = ConnectionRegistry::new();
    let err = sender
        .send_packet(&mut connections, ConnectionId(99), ids::LOGIN, &[])
        .unwrap_err();
    assert!(matches!(
        err,
        PlaySendError::Outbound(rivet_server::server::tick::registry::OutboundError::Gone(_))
    ));
}

// ---- Byte-exact burst bodies against the capture + fixtures -----------------

/// The `initialize_border` body the capture's id-43 line records: `center 0,0;
/// old = new size = 59999968.0` (the float `5.999997E7F` promoted to double);
/// `lerp_time 0; absolute_max_size 29999984; warning_blocks 5; warning_time
/// 300`. Paper's `sendLevelInfo` emits the same body on both of its
/// occurrences, so the single-player capture records id-43 twice (the second
/// block is the Slice-B-deferred `sendLevelInfo`); this body is the
/// deterministic value both occurrences carry.
fn border_body() -> Vec<u8> {
    hex_bytes("00000000000000000000000000000000418c9c3700000000418c9c370000000000f086a70e05ac02")
}

/// The full Slice B burst ids in Paper's send order — `PLAY_BURST_ORDER`
/// restricted to the ported members (the complete, authoritative list is in
/// `join.rs`'s module doc), with the two `sendLevelInfo` occurrences bracketing
/// the player_info update and the #100 cache + 117-chunk send-set immediately
/// before the second `sendLevelInfo`. 135 frames total: 11 join members, the 3
/// cache packets, the 117 chunks, and the 4 second-`sendLevelInfo` members.
fn burst_order() -> Vec<u32> {
    let mut order = vec![
        ids::LOGIN,
        ids::CHANGE_DIFFICULTY,
        ids::PLAYER_ABILITIES,
        ids::SET_HELD_SLOT,
        ids::ENTITY_EVENT,
        ids::PLAYER_POSITION,
        ids::INITIALIZE_BORDER,
        ids::SET_TIME,
        ids::SET_DEFAULT_SPAWN,
        ids::GAME_EVENT,
        ids::PLAYER_INFO_UPDATE,
        ids::SET_CHUNK_CACHE_RADIUS,
        ids::SET_SIMULATION_DISTANCE,
        ids::SET_CHUNK_CACHE_CENTER,
    ];
    order.extend(std::iter::repeat_n(ids::LEVEL_CHUNK_WITH_LIGHT, 117));
    order.extend([
        ids::INITIALIZE_BORDER,
        ids::SET_TIME,
        ids::SET_DEFAULT_SPAWN,
        ids::GAME_EVENT,
    ]);
    order
}

/// Run the full `place_new_player` burst into a connected registry and return
/// the ordered `(packet_id, raw_packet_body)` pairs from the outbound channel.
/// Each frame is decompressed/deframed by stripping the VarInt21 length prefix
/// and the compression prefix (below-threshold frames are uncompressed).
fn run_burst_with(world: &mut ServerLevel, config: &JoinConfig) -> Vec<(u32, Vec<u8>)> {
    run_burst_with_requested(world, config, None)
}

/// Like `run_burst_with`, but the session manager's `requested_view_distance`
/// (the client's `ClientInformation` view distance). `None` mirrors the
/// `PlayerChunkLoader` auto-config path (no client request); `Some(n)` caps at
/// `load - 1`.
fn run_burst_with_requested(
    world: &mut ServerLevel,
    config: &JoinConfig,
    requested_view_distance: Option<i32>,
) -> Vec<(u32, Vec<u8>)> {
    let mut sender = play_sender();
    let (mut connections, mut out_rx) = registry_with_connection();
    let mut loader = PlayerChunkLoader::new(world.view().center());
    let sent_ids = place_new_player(
        &mut sender,
        &mut connections,
        PROBE_ID,
        &probe_player(),
        world,
        config,
        requested_view_distance,
        &mut loader,
        1, // the spawn teleport's `awaitingTeleport` id (issue #158)
    )
    .expect("burst encodes + queues");

    let mut packets = Vec::new();
    // The outbound sender stays owned by the registry (never dropped mid-burst),
    // so `blocking_recv` would block forever — drain with `try_recv` instead.
    while let Ok(event) = out_rx.try_recv() {
        match event {
            rivet_server::server::tick::channels::OutboundEvent::Packet { frame } => {
                packets.push(decode_frame(&frame));
            }
            rivet_server::server::tick::channels::OutboundEvent::Disconnect { .. } => break,
        }
    }
    assert_eq!(
        sent_ids,
        packets.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    );
    packets
}

/// Run the burst against the default M1 world/config.
fn run_burst() -> Vec<(u32, Vec<u8>)> {
    run_burst_with(&mut world(), &join_config())
}

#[test]
fn place_new_player_sends_paper_order_and_byte_exact_bodies() {
    let packets = run_burst();
    let ids_sent: Vec<u32> = packets.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids_sent,
        burst_order(),
        "PLAY_BURST_ORDER restricted to ported members"
    );

    // The Slice B member bodies must match the pinned capture/fixture bodies
    // byte-for-byte. Chunks are checked separately (all 117 differ only in the
    // 8-byte BE coordinate header), so they're skipped here.
    for (id, body) in &packets {
        match *id {
            ids::LOGIN => assert_eq!(body, &protocol_fixture("join_clientbound_login")),
            ids::PLAYER_ABILITIES => assert_eq!(body, &hex_bytes("003d4ccccd3dcccccd")),
            ids::PLAYER_POSITION => {
                // The live spawn teleport embeds `awaitingTeleport = 1` (issue
                // #158), unlike the capture fixture's canonical id 0. The body
                // is the fixture with the leading id varint rewritten.
                let mut expected = protocol_fixture("join_clientbound_player_position");
                expected[0] = 0x01;
                assert_eq!(body, &expected, "spawn teleport id 1");
            }
            ids::SET_TIME => assert_eq!(body, &protocol_fixture("join_clientbound_set_time")),
            ids::INITIALIZE_BORDER => assert_eq!(body, &border_body()),
            ids::SET_DEFAULT_SPAWN => {
                assert_eq!(
                    body,
                    &hex_bytes(
                        "136d696e6563726166743a6f766572776f726c640000000000000fc10000000000000000"
                    )
                )
            }
            ids::CHANGE_DIFFICULTY => assert_eq!(body, &hex_bytes("0100")),
            ids::GAME_EVENT => assert_eq!(body, &hex_bytes("0d00000000")),
            ids::SET_HELD_SLOT => assert_eq!(body, &hex_bytes("00")),
            // `sendPlayerPermissionLevel`'s op-level event: `[entityId 4B BE
            // 00000001][eventId 24]` — the probe's entity id 1, PERMISSION_LEVEL_ALL.
            ids::ENTITY_EVENT => assert_eq!(body, &hex_bytes("0000000118")),
            ids::PLAYER_INFO_UPDATE => assert_eq!(
                body,
                &protocol_fixture("join_clientbound_player_info_update")
            ),
            ids::SET_CHUNK_CACHE_RADIUS => assert_eq!(body, &hex_bytes("04")),
            ids::SET_SIMULATION_DISTANCE => assert_eq!(body, &hex_bytes("04")),
            ids::SET_CHUNK_CACHE_CENTER => assert_eq!(body, &hex_bytes("0000")),
            ids::LEVEL_CHUNK_WITH_LIGHT => {} // checked in the chunk-specific test
            other => panic!("unexpected burst member {other}"),
        }
    }
}

#[test]
fn burst_sends_exactly_one_117_chunk_sequence() {
    // The one-time burst: exactly 135 frames — the 11 join members + 3 cache
    // packets + 117 chunks + 4 second-`sendLevelInfo` members. No frame is lost
    // or duplicated (the outbound channel carried every queued frame).
    let packets = run_burst();
    assert_eq!(packets.len(), 135, "full Slice B burst frame count");

    let chunk_count = packets
        .iter()
        .filter(|(id, _)| *id == ids::LEVEL_CHUNK_WITH_LIGHT)
        .count();
    assert_eq!(chunk_count, 117, "exactly one 117-chunk send-set");

    let second_level_start = packets
        .iter()
        .position(|(id, _)| *id == ids::LEVEL_CHUNK_WITH_LIGHT)
        .map(|p| p + 117)
        .expect("chunk block present");
    assert_eq!(
        packets[second_level_start..]
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        vec![
            ids::INITIALIZE_BORDER,
            ids::SET_TIME,
            ids::SET_DEFAULT_SPAWN,
            ids::GAME_EVENT,
        ],
        "second sendLevelInfo follows the chunk block"
    );
}

#[test]
fn burst_chunk_bodies_match_the_fixture_apart_from_coords() {
    // `chunk_golden_body.hex` is the canonical chunk (`body[8:]` — no coord
    // header); all 117 superflat bodies differ only in the 8-byte BE coord
    // header, so every encoded chunk body matches `header ++ golden`.
    let golden = protocol_fixture("chunk_golden_body");
    let packets = run_burst();
    let chunks: Vec<&(u32, Vec<u8>)> = packets
        .iter()
        .filter(|(id, _)| *id == ids::LEVEL_CHUNK_WITH_LIGHT)
        .collect();
    assert_eq!(chunks.len(), 117);

    let first = &chunks[0].1;
    assert_eq!(
        i32::from_be_bytes([first[0], first[1], first[2], first[3]]),
        -5,
        "first send chunk x"
    );
    assert_eq!(
        i32::from_be_bytes([first[4], first[5], first[6], first[7]]),
        -4,
        "first send chunk z"
    );
    assert_eq!(first[8..], golden, "chunk body matches the fixture");
    for (id, body) in &packets {
        if *id == ids::LEVEL_CHUNK_WITH_LIGHT {
            assert_eq!(
                &body[8..],
                golden,
                "every chunk body is the deterministic superflat content"
            );
        }
    }
}

/// Decode a captured login body back into its `ClientboundLoginPacket` (the
/// registry-aware codec needs the same `DIMENSION_TYPE` access the sender used).
fn decode_login_body(body: &[u8]) -> ClientboundLoginPacket {
    let access = dimension_type_access();
    let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(body), access);
    ClientboundLoginPacket::stream_codec()
        .decode(&mut input)
        .expect("login body decodes")
}

#[test]
fn login_uses_simulation_distance_not_view_distance() {
    // Paper: `FeatureHooks.getViewDistance(level)` for chunkRadius and
    // `FeatureHooks.getSimulationDistance(level)` for simulationDistance — they
    // can differ. A world with view 4 / simulation 3 must encode both fields
    // distinctly (a regression against duplicating view_distance for both).
    let config = ServerLevelConfig {
        simulation_distance: 3,
        ..ServerLevelConfig::default()
    };
    let mut world = ServerLevel::new(config);
    assert_eq!(world.view().view_distance(), 4);
    assert_eq!(world.get_simulation_distance(), 3);

    let packets = run_burst_with(&mut world, &join_config());
    let login_body = packets
        .iter()
        .find(|(id, _)| *id == ids::LOGIN)
        .map(|(_, body)| body.clone())
        .expect("login in burst");
    let login = decode_login_body(&login_body);
    assert_eq!(login.chunk_radius(), 4);
    assert_eq!(login.simulation_distance(), 3);
}

#[test]
fn login_encodes_non_default_config_booleans_distinctly() {
    // The login booleans come from JoinConfig, not hardcoded `false`: flipping
    // `reduced_debug_info` and `do_limited_crafting` must change the encoded
    // body (the M1 default leaves them off).
    let mut config = join_config();
    config.reduced_debug_info = true;
    config.do_limited_crafting = true;

    let packets = run_burst_with(&mut world(), &config);
    let login_body = packets
        .iter()
        .find(|(id, _)| *id == ids::LOGIN)
        .map(|(_, body)| body.clone())
        .expect("login in burst");
    let login = decode_login_body(&login_body);
    assert!(login.reduced_debug_info());
    assert!(login.do_limited_crafting());

    // The default M1 body (both false) must differ from the flipped body.
    let default_body = run_burst()
        .iter()
        .find(|(id, _)| *id == ids::LOGIN)
        .map(|(_, body)| body.clone())
        .expect("login in default burst");
    assert_ne!(default_body, login_body);
}

#[test]
fn login_encodes_the_world_seed_obfuscated() {
    // `CommonPlayerSpawnInfo.seed` is `BiomeManager.obfuscateSeed(level.getSeed())`
    // (`ServerPlayer.createCommonSpawnInfo`): the world the player joins must
    // surface as its SHA-256-obfuscated seed in the login body. A non-default
    // world seed proves the wire value comes from the world's seed, not the M1
    // fixture default 42.
    let seed = 12345;
    let config = ServerLevelConfig {
        seed,
        ..ServerLevelConfig::default()
    };
    let mut world = ServerLevel::new(config);
    assert_eq!(world.seed(), seed, "the world carries the config seed");

    let packets = run_burst_with(&mut world, &join_config());
    let login_body = packets
        .iter()
        .find(|(id, _)| *id == ids::LOGIN)
        .map(|(_, body)| body.clone())
        .expect("login in burst");
    let login = decode_login_body(&login_body);
    let obfuscated = login.common_player_spawn_info().seed();
    assert_eq!(
        obfuscated,
        rivet_util::java_hash::obfuscate_seed(seed),
        "the login seed is the obfuscated world seed"
    );
    assert_ne!(
        obfuscated,
        rivet_util::java_hash::obfuscate_seed(ServerLevelConfig::default().seed),
        "a non-default world seed must differ from the M1 fixture"
    );
}

#[test]
fn send_level_info_order_and_bodies() {
    let mut sender = play_sender();
    let (mut connections, mut out_rx) = registry_with_connection();
    let sent = send_level_info(&mut sender, &mut connections, PROBE_ID, &world())
        .expect("level info encodes + queues");

    let mut packets = Vec::new();
    // `blocking_recv` would hang (the registry keeps the outbound sender alive);
    // drain with `try_recv`.
    while let Ok(event) = out_rx.try_recv() {
        match event {
            rivet_server::server::tick::channels::OutboundEvent::Packet { frame } => {
                packets.push(decode_frame(&frame));
            }
            rivet_server::server::tick::channels::OutboundEvent::Disconnect { .. } => break,
        }
    }

    assert_eq!(
        sent,
        vec![
            ids::INITIALIZE_BORDER,
            ids::SET_TIME,
            ids::SET_DEFAULT_SPAWN,
            ids::GAME_EVENT
        ]
    );
    assert_eq!(
        packets,
        vec![
            (ids::INITIALIZE_BORDER, border_body()),
            (ids::SET_TIME, protocol_fixture("join_clientbound_set_time")),
            (
                ids::SET_DEFAULT_SPAWN,
                hex_bytes(
                    "136d696e6563726166743a6f766572776f726c640000000000000fc10000000000000000"
                )
            ),
            (ids::GAME_EVENT, hex_bytes("0d00000000")),
        ]
    );
}

// ---- Tick-loop integration: the burst runs inside a real tick ----------------

#[test]
fn join_burst_integration_tick_loop_sends_ordered_frames() {
    use rivet_server::server::tick::channels::OutboundEvent;
    use rivet_server::server::tick::scheduler::NANOS_PER_TICK;
    use rivet_server::server::tick::shutdown::Shutdown;
    use rivet_server::server::tick::time::SimTime;
    use rivet_server::server::tick::{ServerTickLoop, TickContext, TickStats};

    // Build the loop with the join burst as the sole tickable.
    let sender = std::sync::Mutex::new(play_sender());
    let player = probe_player();
    let mut level = world();
    let config = join_config();
    let (lifecycle_tx, lifecycle_rx) = tokio::sync::mpsc::channel(16);
    let shutdown = Arc::new(Shutdown::new());
    let sim = Arc::new(SimTime::with_shutdown(shutdown.clone()));
    let stats = Arc::new(TickStats::default());
    let time: Arc<dyn rivet_server::server::tick::time::TickTime> = sim.clone();
    let scheduler = rivet_server::server::tick::scheduler::TickScheduler::new(NANOS_PER_TICK, 5, 0);
    let loop_ = ServerTickLoop::new(
        scheduler,
        time,
        shutdown.clone(),
        lifecycle_rx,
        vec![Box::new(move |ctx: &mut TickContext| {
            let mut sender = sender.lock().unwrap();
            let mut loader = PlayerChunkLoader::new(level.view().center());
            let _ = place_new_player(
                &mut sender,
                ctx.connections,
                PROBE_ID,
                &player,
                &mut level,
                &config,
                None,
                &mut loader,
                1, // spawn teleport id (issue #158)
            );
        })],
        stats.clone(),
    );
    let handle = std::thread::Builder::new()
        .name("tick-join-test".into())
        .spawn(move || loop_.run())
        .expect("spawn tick loop");

    // Register the connection through the real lifecycle channel. The outbound
    // channel must hold the whole 135-frame burst before the test drains.
    let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(256);
    lifecycle_tx
        .try_send(
            rivet_server::server::tick::channels::LifecycleEvent::Connect {
                id: PROBE_ID,
                remote: REMOTE,
                in_rx,
                out_tx,
                drained: rivet_server::server::tick::channels::InboundDrained::new(),
            },
        )
        .expect("lifecycle fits");
    // Keep the network-side inbound sender alive: the tick drain prunes a
    // connection whose inbound channel is closed before the burst fires.
    let _in_tx = in_tx;

    // Advance the tick; the burst fires on the first tick.
    sim.advance(NANOS_PER_TICK);

    // Drain the outbound channel until the full 135-frame burst arrives. The
    // deadline is checked on every recv attempt (not only after a successful
    // `blocking_recv`), so a burst that never fires fails boundedly instead of
    // hanging the test.
    let mut got: Vec<u32> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while got.len() < burst_order().len() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for burst (got {got:?})"
        );
        match out_rx.try_recv() {
            Ok(OutboundEvent::Packet { frame }) => {
                let (packet_id, _) = decode_frame(&frame);
                got.push(packet_id);
            }
            Ok(OutboundEvent::Disconnect { .. }) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                panic!("outbound channel closed before the burst completed");
            }
        }
    }
    assert_eq!(got, burst_order());

    shutdown.request();
    handle.join().expect("tick loop exits cleanly");
}

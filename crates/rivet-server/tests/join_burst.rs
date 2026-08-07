//! `server::player` — the join-burst foundation tests (Slice A of #101):
//! `PlaySender` encodes + queues frames, `ServerPlayer`/`PlayerIndices` model
//! the session, and `place_new_player`/`send_level_info` emit the Paper-faithful
//! play join burst.
//!
//! Ground truth: the `tools/rivet-capture/fixtures/join/capture.jsonl` join
//! (protocol 776, Paper `26.2-DEV-main@0a99345`, offline superflat world,
//! seed 42, view distance 4). The packet bodies are the raw packet bodies the
//! capture proxy strips (packet id + compression prefix already removed), so the
//! byte-exact assertions compare an encoded burst member against the capture's
//! body. The four `rivet-protocol` `join_clientbound_*.hex` fixtures pin the same
//! bodies. Slice A covers ids `[49,10,64,105,72,43,113,97,38,70]` — the
//! `update_recipes` (133) member is deferred (RivetTodo(#87)).

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
    // The whole 10-packet burst is queued before the test drains, so the
    // outbound channel must hold every frame (Paper's 1024 default does).
    let (out_tx, out_rx) = tokio::sync::mpsc::channel(32);
    registry.apply(
        rivet_server::server::tick::channels::LifecycleEvent::Connect {
            id: PROBE_ID,
            remote: REMOTE,
            in_rx,
            out_tx,
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
/// VarInt21 frame decoder and VarInt readers — no fixed 1-byte offset
/// assumptions, so future >=128-byte bodies or packet ids decode correctly.
fn decode_frame(frame: &[u8]) -> (u32, Vec<u8>) {
    let mut buf = BytesMut::from(frame);
    let wire = Varint21FrameDecoder::new(None)
        .decode(&mut buf)
        .expect("frame decodes")
        .expect("frame present");
    let mut wire = BytesMut::from(&wire[..]);
    // Below-threshold frames are uncompressed: `varint(0) ++ varint(packet_id) ++ body`.
    let declared_len = var_int::read(&mut wire);
    assert_eq!(declared_len, 0, "below-threshold frame is uncompressed");
    let packet_id = var_int::read(&mut wire) as u32;
    (packet_id, wire.to_vec())
}

/// The `PacketType` id constants the burst asserts against.
mod ids {
    use super::*;
    pub const LOGIN: u32 = PacketType::Login.id();
    pub const CHANGE_DIFFICULTY: u32 = PacketType::ChangeDifficulty.id();
    pub const PLAYER_ABILITIES: u32 = PacketType::PlayerAbilities.id();
    pub const SET_HELD_SLOT: u32 = PacketType::SetHeldSlot.id();
    pub const PLAYER_POSITION: u32 = PacketType::PlayerPosition.id();
    pub const INITIALIZE_BORDER: u32 = PacketType::InitializeBorder.id();
    pub const SET_TIME: u32 = PacketType::SetTime.id();
    pub const SET_DEFAULT_SPAWN: u32 = PacketType::SetDefaultSpawnPosition.id();
    pub const GAME_EVENT: u32 = PacketType::GameEvent.id();
    pub const PLAYER_INFO_UPDATE: u32 = PacketType::PlayerInfoUpdate.id();
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

/// The `initialize_border` body the capture's id-43 line records (both
/// `sendLevelInfo` occurrences carry it): `center 0,0; old = new size =
/// 59999968.0` (the float `5.999997E7F` promoted to double); `lerp_time 0;
/// absolute_max_size 29999984; warning_blocks 5; warning_time 300`.
fn border_body() -> Vec<u8> {
    hex_bytes("00000000000000000000000000000000418c9c3700000000418c9c370000000000f086a70e05ac02")
}

/// The Slice A ids in Paper's send order (PLAY_BURST_ORDER minus the deferred
/// `update_recipes`/entity-pairing members).
fn slice_a_order() -> Vec<u32> {
    vec![
        ids::LOGIN,
        ids::CHANGE_DIFFICULTY,
        ids::PLAYER_ABILITIES,
        ids::SET_HELD_SLOT,
        ids::PLAYER_POSITION,
        ids::INITIALIZE_BORDER,
        ids::SET_TIME,
        ids::SET_DEFAULT_SPAWN,
        ids::GAME_EVENT,
        ids::PLAYER_INFO_UPDATE,
    ]
}

/// Run the full `place_new_player` burst into a connected registry and return
/// the ordered `(packet_id, raw_packet_body)` pairs from the outbound channel.
/// Each frame is decompressed/deframed by stripping the VarInt21 length prefix
/// and the compression prefix (below-threshold frames are uncompressed).
fn run_burst_with(world: &ServerLevel, config: &JoinConfig) -> Vec<(u32, Vec<u8>)> {
    let mut sender = play_sender();
    let (mut connections, mut out_rx) = registry_with_connection();
    let sent_ids = place_new_player(
        &mut sender,
        &mut connections,
        PROBE_ID,
        &probe_player(),
        world,
        config,
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
    run_burst_with(&world(), &join_config())
}

#[test]
fn place_new_player_sends_paper_order_and_byte_exact_bodies() {
    let packets = run_burst();
    let ids_sent: Vec<u32> = packets.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids_sent, slice_a_order(), "PLAY_BURST_ORDER prefix");

    // Each Slice A body must match the pinned capture/fixture body byte-for-byte.
    for (id, body) in &packets {
        match *id {
            ids::LOGIN => assert_eq!(body, &protocol_fixture("join_clientbound_login")),
            ids::PLAYER_ABILITIES => assert_eq!(body, &hex_bytes("003d4ccccd3dcccccd")),
            ids::PLAYER_POSITION => {
                assert_eq!(body, &protocol_fixture("join_clientbound_player_position"))
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
            ids::PLAYER_INFO_UPDATE => assert_eq!(
                body,
                &protocol_fixture("join_clientbound_player_info_update")
            ),
            other => panic!("unexpected burst member {other}"),
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
    let world = ServerLevel::new(config);
    assert_eq!(world.view().view_distance(), 4);
    assert_eq!(world.get_simulation_distance(), 3);

    let packets = run_burst_with(&world, &join_config());
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

    let packets = run_burst_with(&world(), &config);
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
    let level = world();
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
            let _ = place_new_player(
                &mut sender,
                ctx.connections,
                PROBE_ID,
                &player,
                &level,
                &config,
            );
        })],
        stats.clone(),
    );
    let handle = std::thread::Builder::new()
        .name("tick-join-test".into())
        .spawn(move || loop_.run())
        .expect("spawn tick loop");

    // Register the connection through the real lifecycle channel.
    let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(64);
    lifecycle_tx
        .try_send(
            rivet_server::server::tick::channels::LifecycleEvent::Connect {
                id: PROBE_ID,
                remote: REMOTE,
                in_rx,
                out_tx,
            },
        )
        .expect("lifecycle fits");
    // Keep the network-side inbound sender alive: the tick drain prunes a
    // connection whose inbound channel is closed before the burst fires.
    let _in_tx = in_tx;

    // Advance the tick; the burst fires on the first tick.
    sim.advance(NANOS_PER_TICK);

    // Drain the outbound channel until the burst's ten frames arrive. The
    // deadline is checked on every recv attempt (not only after a successful
    // `blocking_recv`), so a burst that never fires fails boundedly instead of
    // hanging the test.
    let mut got: Vec<u32> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while got.len() < slice_a_order().len() {
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
    assert_eq!(got, slice_a_order());

    shutdown.request();
    handle.join().expect("tick loop exits cleanly");
}

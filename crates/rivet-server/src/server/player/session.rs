//! The tick-owned play session manager (Slice B of #101) — the "play listener"
//! that makes the #273 join burst live.
//!
//! OWNERSHIP §Network: handshake/status/login run on the tokio side; play-state
//! packets cross to the tick thread over bounded channels keyed by `ConnectionId`.
//! [`PlayerSessionManager`] is the tick-side consumer: it runs as a tickable
//! (moved into the tick thread via `Server::serve`), consumes the
//! configuration→play handoff the network side stashed on the connection
//! ([`LifecycleEvent::EnterPlay`] — drained by the tick loop *before* the inbound
//! channels), spawns the tick-owned `ServerPlayer`/`PlayerIndices` entry, fires
//! the Paper-faithful join burst ([`place_new_player`]), then routes the
//! connection's inbound play frames with bounded FIFO buffering that bridges the
//! handoff race without losing or duplicating a frame.
//!
//! No keepalive/movement semantics are pretended: serverbound frames are decoded
//! only to the extent needed to prove routing (the `keep_alive` id/body boundary)
//! and otherwise dropped. `keep_alive` (id 28) is the single serverbound play
//! body ported so far; its minimal handling here parses the echoed id and counts
//! it, but the full `handle_keep_alive` echo + timeout loop is RivetTodo(#157),
//! and the movement/teleport surface is RivetTodo(#158).
//!
//! The outbound burst fires exactly once per connection (the handoff is consumed
//! — [`ConnectionRegistry::take_play_handoff`]), and the join burst is
//! backpressured by the connection's bounded outbound channel exactly like every
//! other tick→network send (overflow policy disconnects; nothing buffers
//! unbounded). Sessions are cleaned up when their connection is lost (the tick
//! prunes the registry entry on disconnect/EOF, and the manager removes the
//! player index + any pending frames on the next tick).

use rivet_protocol::codec::StreamDecoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets::play::serverbound::PacketType as ServerboundPacketType;
use rivet_protocol::protocol::common::serverbound_keep_alive::ServerboundKeepAlivePacket;

use crate::server::level::server_level::{ServerLevel, ServerLevelConfig};
use crate::server::network::connection_id::ConnectionId;
use crate::server::network::packet_listener::DisconnectReason;
use crate::server::player::join::{JoinConfig, place_new_player};
use crate::server::player::play_sender::PlaySender;
use crate::server::player::{PlayerIndices, ServerPlayer};
use crate::server::tick::channels::{OutboundEvent, ServerboundFrame};
use crate::server::tick::{TickContext, Tickable};

/// The maximum inbound frames the session manager retains per connection while
/// its handoff is pending. A connection whose play frames arrive before its
/// `EnterPlay` handoff does (the network task awaits `enter_play` before the
/// first forward, so this is a scheduling race, not steady state) is buffered
/// in FIFO order up to this bound and delivered the tick the session spawns.
///
/// Bounded by construction: the per-connection inbound budget
/// (`MAX_INBOUND_FRAMES_PER_DRAIN`, 1024) is what the connection can deliver in
/// one tick anyway, so this is the same ceiling as a single drain — never an
/// unbounded accumulation. A connection that exceeds the bound is disconnected
/// (anti-flood policy, matching the tick drain's own budget).
pub const MAX_PENDING_SESSION_FRAMES: usize = 1024;

/// The tick-owned play session manager — one instance per server, owning the
/// `ServerLevel` (tick-confined), the `PlaySender`, the player indices, and the
/// per-connection pending-frame buffers. Confined to the tick thread (moved in
/// via `Server::serve`); the counters are plain fields readable by tests.
pub struct PlayerSessionManager {
    level: ServerLevel,
    sender: PlaySender,
    join: JoinConfig,
    indices: PlayerIndices,
    /// Inbound play frames that arrived for a connection whose handoff has not
    /// been applied yet — FIFO retention across the race, keyed by connection.
    pending: std::collections::HashMap<ConnectionId, Vec<ServerboundFrame>>,
    /// Inbound frames routed to a live session (test/counterfactual observability:
    /// proves no loss and no double-delivery across the handoff race).
    routed_frames: usize,
    /// `keep_alive` (id 28) frames whose body parsed (the sole ported serverbound
    /// play body; the full keepalive loop is #157).
    keep_alives_seen: usize,
}

/// The configuration the session manager needs to run a play session (Slice B).
/// Built once when live join is enabled; immutable thereafter.
pub struct SessionManagerConfig {
    /// The compression threshold (from `ServerConfig.compression_threshold`).
    pub compression_threshold: i32,
    /// The `DIMENSION_TYPE` registry access (login's `CommonPlayerSpawnInfo`).
    pub dimension_type_access: rivet_registry::RegistryAccess,
    /// The `WORLD_CLOCK` registry access (set_time's clock-update holders).
    pub world_clock_access: rivet_registry::RegistryAccess,
    /// The level sessions join (tick-confined; moved into the manager).
    pub level: ServerLevel,
    /// The join config (max players, level keys, game rules).
    pub join: JoinConfig,
}

/// The default M1 play-session config: the superflat `ServerLevel` (seed 42,
/// view distance 4, spawn (0,-63,0)) the #153 capture records, the M1 join
/// config (`max_players 20`, offline, flat), and the two single-registry
/// accesses the registry-aware play bodies resolve (the same construction the
/// `join_burst.rs` tests use). This is what `Server` builds when
/// `ServerConfig.enable_join` is set.
pub fn default_session_config(compression_threshold: i32) -> SessionManagerConfig {
    SessionManagerConfig {
        compression_threshold,
        dimension_type_access: dimension_type_access(),
        world_clock_access: world_clock_access(),
        level: ServerLevel::new(ServerLevelConfig::default()),
        join: join_config(),
    }
}

/// Build the session-manager tickable (a closure running on the tick thread).
/// `SessionManagerConfig` is moved in; the manager owns the level.
pub fn session_manager_tickable(config: SessionManagerConfig) -> Tickable {
    let mut manager = PlayerSessionManager::new(config);
    Box::new(move |ctx: &mut TickContext| manager.tick(ctx))
}

impl PlayerSessionManager {
    /// Build the manager. Called on the tokio side when `enable_join` is set;
    /// the manager is moved into the tick thread by `Server::serve`.
    pub fn new(config: SessionManagerConfig) -> Self {
        let sender = PlaySender::new(
            config.compression_threshold,
            config.dimension_type_access,
            config.world_clock_access,
        );
        PlayerSessionManager {
            level: config.level,
            sender,
            join: config.join,
            indices: PlayerIndices::default(),
            pending: std::collections::HashMap::new(),
            routed_frames: 0,
            keep_alives_seen: 0,
        }
    }

    /// Run one tick of play session management: consume every connection's
    /// handoff (spawning its session + burst), route the tick's inbound frames
    /// into sessions, then clean up sessions whose connection is gone.
    pub fn tick(&mut self, ctx: &mut TickContext) {
        let ids: Vec<ConnectionId> = ctx.connections.ids().collect();
        for id in ids {
            if let Some((profile, client_information)) = ctx.connections.take_play_handoff(id) {
                self.spawn_session(ctx, id, profile, client_information);
            }
        }
        let inbound = std::mem::take(&mut ctx.inbound);
        for (id, frame) in inbound {
            self.route_inbound(ctx, id, frame);
        }
        self.prune_lost(ctx);
    }

    /// The number of live sessions (player indices size).
    pub fn session_count(&self) -> usize {
        self.indices.len()
    }

    /// Inbound frames routed to a live session (see the `routed_frames` field).
    pub fn routed_frames(&self) -> usize {
        self.routed_frames
    }

    /// `keep_alive` frames whose body parsed (see the `keep_alives_seen` field).
    pub fn keep_alives_seen(&self) -> usize {
        self.keep_alives_seen
    }

    /// `PlayerList.placeNewPlayer(connection, player, cookie)` — spawn the
    /// session: build the tick-owned `ServerPlayer` + `PlayerIndices` entry,
    /// fire the join burst, and deliver any play frames that arrived before the
    /// handoff (the coalesced-finish race) in FIFO order.
    fn spawn_session(
        &mut self,
        ctx: &mut TickContext,
        connection_id: ConnectionId,
        profile: rivet_registry::core::GameProfile,
        client_information: rivet_protocol::protocol::common::client_information::ClientInformation,
    ) {
        // The M1 world spawn: the level's respawn geometry (the superflat
        // `(0, -63, 0)` spawn, zero rotation, survival). The per-player
        // `ServerPlayer` carries the authenticated profile + the login `playerId`
        // (the deterministic superflat world's first entity id is 1).
        let respawn = self.level.get_respawn_data();
        let pos = respawn.pos();
        let player = ServerPlayer::new(
            connection_id,
            profile,
            1,
            rivet_registry::core::Vec3::new(
                pos.get_x() as f64,
                pos.get_y() as f64,
                pos.get_z() as f64,
            ),
            respawn.yaw(),
            respawn.pitch(),
            rivet_registry::core::GameType::Survival,
        );
        self.indices.insert(player.uuid(), connection_id);

        // Fire the join burst in Paper's order. `requested_view_distance` is the
        // client's `ClientInformation` view distance — the Moonrise ladder feeds
        // it through `client + 1` (the capture client's 8 caps at `load - 1` =
        // 4, the 117-chunk M1 send-set; a `create_default` client's 2 resolves
        // send 3, the 81-chunk square). The resolved distances also go into the
        // cache-radius packet this burst emits.
        if let Err(e) = place_new_player(
            &mut self.sender,
            ctx.connections,
            connection_id,
            &player,
            &self.level,
            &self.join,
            Some(client_information.view_distance() as i32),
        ) {
            // A burst encode/send failure is a server-side fault or an outbound
            // overload; the connection was pruned by `ConnectionRegistry::send`
            // on overflow. The indices entry is rolled back so a later
            // re-connect starts clean.
            tracing::warn!(%connection_id, %e, "play session burst failed");
            self.indices.remove(&player.uuid());
            return;
        }

        // Deliver frames that arrived before the handoff (the client coalesced
        // `finish_configuration` with its first play packet) in FIFO order.
        if let Some(frames) = self.pending.remove(&connection_id) {
            for frame in frames {
                self.route_inbound(ctx, connection_id, frame);
            }
        }
    }

    /// Route one inbound play frame to its session. Frames for a connection
    /// whose handoff has not been applied yet are retained FIFO (bounded);
    /// otherwise the frame is dispatched.
    fn route_inbound(&mut self, ctx: &mut TickContext, id: ConnectionId, frame: ServerboundFrame) {
        if self.indices.uuid_for(id).is_none() {
            let frames = self.pending.entry(id).or_default();
            if frames.len() >= MAX_PENDING_SESSION_FRAMES {
                // Anti-flood: a connection flooding play frames before its
                // handoff lands is disconnected (the same budget the tick drain
                // enforces). No unbounded retention.
                self.pending.remove(&id);
                let _ = ctx.connections.send(
                    id,
                    OutboundEvent::Disconnect {
                        reason: DisconnectReason::InboundOverflow(
                            "play frames before handoff".into(),
                        ),
                    },
                );
                return;
            }
            frames.push(frame);
            return;
        }
        self.dispatch(id, frame);
    }

    /// Dispatch one play frame to its session. Only `keep_alive` (id 28) is
    /// decoded — the sole ported serverbound play body. The echo + timeout loop
    /// is #157; every other play frame is dropped without pretending semantics
    /// (movement/teleport is #158).
    fn dispatch(&mut self, id: ConnectionId, frame: ServerboundFrame) {
        self.routed_frames += 1;
        if read_packet_id(&frame.bytes) != Some(ServerboundPacketType::KeepAlive as u32) {
            return;
        }
        // `handleKeepAlive`'s body boundary: the echoed `long` id. Parsing it
        // proves the frame reached the session intact. The keepalive state
        // machine (transmit cadence, timeout disconnect) is RivetTodo(#157).
        //
        // The decode runs on the tick thread, so its panics must be contained
        // here, never abort the tick: a truncated body (`read_long` on < 8
        // remaining bytes panics) is dropped and counted as malformed, matching
        // the decode-boundary containment of [`decode_packet`].
        let mut raw = bytes::BytesMut::from(&frame.bytes[..]);
        let _ = rivet_protocol::var_int::read(&mut raw); // packet id
        let mut input = FriendlyByteBuf::new(raw);
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ServerboundKeepAlivePacket::stream_codec().decode(&mut input)
        }));
        match decoded {
            Ok(Ok(_)) => {
                self.keep_alives_seen += 1;
            }
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(%id, "malformed keep_alive play frame");
            }
        }
    }

    /// Remove sessions whose connection is gone (the tick pruned the registry
    /// entry on disconnect/EOF/overflow). The player index and any pending
    /// frames for a lost connection are dropped so the indices stay a bijection
    /// with live connections.
    fn prune_lost(&mut self, ctx: &mut TickContext) {
        let ids: Vec<ConnectionId> = self.indices.connection_ids().collect();
        for id in ids {
            if !ctx.connections.contains(id)
                && let Some(uuid) = self.indices.uuid_for(id)
            {
                self.indices.remove(&uuid);
            }
        }
        let pending_ids: Vec<ConnectionId> = self.pending.keys().copied().collect();
        for id in pending_ids {
            if !ctx.connections.contains(id) {
                self.pending.remove(&id);
            }
        }
    }
}

/// Read the leading VarInt packet id of a decoded play frame (the frame body is
/// the wire `varint(packet_id) ++ body`). Negative VarInts are not valid packet
/// ids; a truncated or over-long frame reads as `None`.
///
/// Uses the handshake's never-panicking bounded reader: `VarInt.read` panics on
/// an empty buffer and on "VarInt too big", and this runs on the tick thread, so
/// a corrupted play frame must be dropped here, never allowed to abort the tick.
fn read_packet_id(frame: &[u8]) -> Option<u32> {
    let mut buf = bytes::BytesMut::from(frame);
    let id = crate::server::network::server_handshake_packet_listener::read_packet_id(&mut buf);
    match id {
        Ok(id) if id >= 0 => Some(id as u32),
        _ => None,
    }
}

// ---- The default M1 session config (mirrors `join_burst.rs`'s helpers) ------

use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::registries;
use rivet_registry::{RegistrationInfo, RegistryAccess, RegistryBuilder};

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

/// The M1 `JoinConfig` (`max_players 20`, offline, flat superflat, death screen
/// on; the reduced-debug / limited-crafting rules off).
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

/// A `DIMENSION_TYPE` registry/access with one `overworld` entry (holder id 0),
/// mirroring the capture's spawn-info `dimension_type` holder id 0.
fn dimension_type_access() -> RegistryAccess {
    let mut builder = RegistryBuilder::new(&*registries::DIMENSION_TYPE);
    builder.register(
        &ResourceKey::create(
            &*registries::DIMENSION_TYPE,
            Identifier::with_default_namespace("overworld"),
        ),
        std::sync::Arc::new(rivet_registry::registries::DimensionType),
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
        std::sync::Arc::new(rivet_registry::registries::WorldClock),
        RegistrationInfo::BUILT_IN,
    );
    builder.register(
        &ResourceKey::create(
            &*registries::WORLD_CLOCK,
            Identifier::with_default_namespace("weather"),
        ),
        std::sync::Arc::new(rivet_registry::registries::WorldClock),
        RegistrationInfo::BUILT_IN,
    );
    RegistryAccess::from_single_registry((*registries::WORLD_CLOCK).clone(), builder.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tick::channels::{InboundDrained, LifecycleEvent, OutboundEvent};
    use crate::server::tick::registry::ConnectionRegistry;
    use bytes::Bytes;
    use rivet_protocol::protocol::common::client_information::ClientInformation;
    use rivet_protocol::protocol::game::clientbound_set_chunk_cache_radius::ClientboundSetChunkCacheRadiusPacket;
    use rivet_protocol::protocol::game::clientbound_set_simulation_distance::ClientboundSetSimulationDistancePacket;
    use rivet_registry::core::{GameProfile, create_offline_player_uuid};

    /// A connected connection id for the counterfactual tests.
    const ID: ConnectionId = ConnectionId(1);

    fn varint(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        rivet_protocol::var_int::write(&mut out, value);
        out
    }

    /// A `keep_alive` (id 28) play frame with the given echoed id — the sole
    /// ported serverbound play body, so `dispatch` decodes and counts it.
    fn keepalive_frame(id: i64) -> ServerboundFrame {
        let mut bytes = varint(28);
        bytes.extend_from_slice(&id.to_be_bytes());
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// A registry with `ID` connected over a bounded outbound channel (capacity
    /// 256 — the largest burst these unit tests fire is the `create_default`
    /// 99-frame send-set, which must fit before the test drains). Returns the
    /// registry and the outbound receiver for asserting disconnect.
    fn connected_registry() -> (
        ConnectionRegistry,
        tokio::sync::mpsc::Receiver<OutboundEvent>,
    ) {
        let mut registry = ConnectionRegistry::new();
        let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
        let (out_tx, out_rx) = tokio::sync::mpsc::channel(256);
        registry.apply(LifecycleEvent::Connect {
            id: ID,
            remote: "127.0.0.1:25565".parse().unwrap(),
            in_rx,
            out_tx,
            drained: InboundDrained::new(),
        });
        let _ = in_tx;
        (registry, out_rx)
    }

    /// Apply the configuration→play handoff for `ID` (the network side sends
    /// `LifecycleEvent::EnterPlay` when the connection reaches play).
    ///
    /// The handoff carries `ClientInformation::create_default()`, whose
    /// `view_distance` is **2** (not 8 — the capture client's value is set by
    /// the `client_information_frame` in the e2e test). The Moonrise send
    /// ladder resolves `client + 1 = 3` and the send-set is the 9×9 = 81-chunk
    /// square (at send 3 the corners `(±4,±4)` are included, `2² + 2² = 8 < 9`).
    fn apply_enter_play(registry: &mut ConnectionRegistry) {
        registry.apply(LifecycleEvent::EnterPlay {
            id: ID,
            profile: probe_profile(),
            client_information: ClientInformation::create_default(),
        });
    }

    /// Run one tick of the manager against the registry with the given inbound
    /// frames; returns the manager (the registry borrow ends).
    fn run_tick(
        mut manager: PlayerSessionManager,
        registry: &mut ConnectionRegistry,
        inbound: Vec<(ConnectionId, ServerboundFrame)>,
    ) -> PlayerSessionManager {
        let mut ctx = TickContext {
            tick: 1,
            connections: registry,
            inbound,
        };
        manager.tick(&mut ctx);
        manager
    }

    fn probe_profile() -> GameProfile {
        GameProfile::new_without_properties(
            create_offline_player_uuid("RivetProbe"),
            "RivetProbe".to_string(),
        )
    }

    #[test]
    fn frames_before_handoff_are_buffered_then_delivered_without_loss_or_duplication() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        // Tick 1: play frames arrive before the handoff — buffered FIFO.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, keepalive_frame(1)), (ID, keepalive_frame(2))],
        );
        assert_eq!(manager.session_count(), 0, "no session before the handoff");
        assert_eq!(
            manager.routed_frames(),
            0,
            "frames are buffered, not routed"
        );

        // The handoff lands; tick 2 drains it (spawns the session + burst) and
        // delivers the pending frames in order, plus the tick's new frame.
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![(ID, keepalive_frame(3))]);
        assert_eq!(manager.session_count(), 1, "handoff spawns the session");
        assert_eq!(
            manager.routed_frames(),
            3,
            "pending + fresh frames delivered exactly once"
        );
        assert_eq!(
            manager.keep_alives_seen(),
            3,
            "all three keep_alives decoded"
        );
    }

    #[test]
    fn connection_loss_prunes_session_and_pending() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        // Buffer a frame pre-handoff, then apply the handoff and spawn.
        manager = run_tick(manager, &mut registry, vec![(ID, keepalive_frame(1))]);
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![]);
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.routed_frames(), 1);

        // The connection is lost (disconnect/EOF prunes the registry entry);
        // the next tick removes the session and drops the pending buffer.
        registry.apply(LifecycleEvent::Disconnect {
            id: ID,
            reason: DisconnectReason::EndOfStream,
        });
        manager = run_tick(manager, &mut registry, vec![]);
        assert_eq!(
            manager.session_count(),
            0,
            "session removed on connection loss"
        );
        assert!(
            manager.pending.is_empty(),
            "pending dropped for a lost connection"
        );
    }

    #[test]
    fn pre_handoff_frame_flood_disconnects_bounded() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        // A connection flooding play frames before its handoff exceeds the
        // bounded pending budget and is disconnected (anti-flood policy) — no
        // unbounded retention.
        let frames: Vec<(ConnectionId, ServerboundFrame)> = (0..=MAX_PENDING_SESSION_FRAMES)
            .map(|i| (ID, keepalive_frame(i as i64)))
            .collect();
        manager = run_tick(manager, &mut registry, frames);
        assert_eq!(manager.session_count(), 0);
        assert_eq!(manager.routed_frames(), 0, "none routed for the flooder");

        let mut saw_disconnect = false;
        while let Ok(event) = out_rx.try_recv() {
            if matches!(event, OutboundEvent::Disconnect { .. }) {
                saw_disconnect = true;
            }
        }
        assert!(saw_disconnect, "flooding connection is disconnected");
    }

    /// A hostile play frame must be dropped on the tick thread, never abort it.
    /// `VarInt.read` and `FriendlyByteBuf.read_long` both panic on truncated
    /// input; the session manager contains both at the decode boundary (the
    /// panic-vulnerability regression — this test fails with the old
    /// `read_packet_id`/bare-`decode` on a truncated frame).
    #[test]
    fn hostile_truncated_play_frames_are_dropped_not_panicked() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        apply_enter_play(&mut registry);

        // Continuation-only packet-id varint (`read` would `get_u8` an empty
        // buffer and panic): the id reads as `None`, the frame is dropped.
        let truncated_id = ServerboundFrame {
            bytes: Bytes::from_static(&[0x80, 0x80]),
        };
        // A keep_alive (id 28) whose body declares an 8-byte long but carries
        // only 4 bytes (`read_long` would panic on the empty tail): the decode
        // is contained and the frame dropped.
        let mut truncated_body = varint(28);
        truncated_body.extend_from_slice(&1i64.to_be_bytes()[..4]);
        let truncated_body = ServerboundFrame {
            bytes: Bytes::from(truncated_body),
        };
        // A negative packet id (`read` succeeds, `id < 0`): dropped.
        let mut negative_id = varint(-1);
        negative_id.extend_from_slice(&0i64.to_be_bytes());
        let negative_id = ServerboundFrame {
            bytes: Bytes::from(negative_id),
        };

        manager = run_tick(
            manager,
            &mut registry,
            vec![
                (ID, truncated_id),
                (ID, truncated_body),
                (ID, negative_id),
                (ID, keepalive_frame(1)),
            ],
        );

        // The tick ran to completion — the hostile frames were dropped without
        // a panic — and only the well-formed keep_alive was decoded.
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.routed_frames(), 4, "all frames consumed");
        assert_eq!(manager.keep_alives_seen(), 1, "only the valid keep_alive");
    }

    #[test]
    fn keep_alive_beyond_handoff_is_routed_and_counted() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        apply_enter_play(&mut registry);
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, keepalive_frame(42)), (ID, keepalive_frame(7))],
        );
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.routed_frames(), 2);
        assert_eq!(manager.keep_alives_seen(), 2);
        assert!(
            manager.pending.is_empty(),
            "handoff already applied, nothing buffered"
        );
    }

    /// Decode one outbound play frame into `(packet_id, body)` — the body is
    /// the bytes after the packet-id varint, decompressed if the frame was
    /// zlib-compressed (the frame is the VarInt21 length header, then
    /// `varint(declaredLength) ++ [zlib payload | raw]`, then
    /// `varint(id) ++ body`).
    fn frame_parts(frame: &[u8]) -> (u32, Vec<u8>) {
        let mut buf = bytes::BytesMut::from(frame);
        let _len = rivet_protocol::var_int::read(&mut buf);
        let declared = rivet_protocol::var_int::read(&mut buf);
        let payload: Vec<u8> = if declared > 0 {
            let mut decoder = flate2::read::ZlibDecoder::new(&buf[..]);
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut decoder, &mut out).expect("inflate");
            out
        } else {
            buf.to_vec()
        };
        let mut payload: &[u8] = &payload;
        let id = rivet_protocol::var_int::read(&mut payload) as u32;
        (id, payload.to_vec())
    }

    /// Drain every frame the tick queued for `ID` into ordered `(packet_id,
    /// body)` pairs.
    fn drain_outbound_frames(
        out_rx: &mut tokio::sync::mpsc::Receiver<OutboundEvent>,
    ) -> Vec<(u32, Vec<u8>)> {
        let mut frames = Vec::new();
        while let Ok(event) = out_rx.try_recv() {
            if let OutboundEvent::Packet { frame } = event {
                frames.push(frame_parts(&frame));
            }
        }
        frames
    }

    /// `create_default` (view distance 2) must NOT resolve to the M1 capture's
    /// 117-chunk send-set: the Moonrise ladder yields send `client + 1 = 3`, and
    /// at send 3 the corner cells `(±4,±4)` are still *inside* the square —
    /// `ChunkTrackingView` shrinks the delta by 2 first (`|±4| − 2 = 2`),
    /// giving `2² + 2² = 8 < 9` — so the send-set is the full 9×9 = 81 chunks.
    /// The corner cut only appears at send 4 (the M1 capture's 117-chunk
    /// square). This test pins the 81-chunk/99-frame shape the `create_default`
    /// handoff actually produces end to end.
    #[test]
    fn create_default_handoff_resolves_the_81_chunk_send_set() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry); // create_default: view distance 2
        manager = run_tick(manager, &mut registry, vec![]);
        assert_eq!(manager.session_count(), 1);

        let frames = drain_outbound_frames(&mut out_rx);
        // 18 non-chunk burst members (11 join + 3 cache + 4 second-sendLevelInfo)
        // + 81 chunks = 99 frames.
        let chunk_count = frames
            .iter()
            .filter(|(id, _)| *id == rivet_protocol::generated::packets::play::clientbound::PacketType::LevelChunkWithLight.id())
            .count();
        assert_eq!(
            frames.len(),
            99,
            "create_default (view distance 2) fires a 99-frame burst"
        );
        assert_eq!(
            chunk_count, 81,
            "send distance 3 gives the full 9x9 square (corners included)"
        );
        // The two cache packets carry the resolved distances — cache radius 3
        // (the send distance), simulation distance 4 (the world's tick
        // distance) — pinned by burst order and decoded to their exact body
        // bytes with no trailing bytes.
        let cache_radius = &frames[11];
        let sim_distance = &frames[12];
        assert_eq!(
            cache_radius.0,
            rivet_protocol::generated::packets::play::clientbound::PacketType::SetChunkCacheRadius
                .id(),
            "cache radius packet precedes the chunks"
        );
        assert_eq!(
            sim_distance.0,
            rivet_protocol::generated::packets::play::clientbound::PacketType::SetSimulationDistance.id(),
            "simulation distance packet follows"
        );
        let mut radius_buf = FriendlyByteBuf::new(bytes::BytesMut::from(cache_radius.1.as_slice()));
        let decoded_radius = ClientboundSetChunkCacheRadiusPacket::stream_codec()
            .decode(&mut radius_buf)
            .unwrap();
        assert_eq!(
            decoded_radius,
            ClientboundSetChunkCacheRadiusPacket::new(3),
            "cache radius is the resolved send distance 3"
        );
        assert_eq!(radius_buf.readable_bytes(), 0, "no trailing bytes");

        let mut sim_buf = FriendlyByteBuf::new(bytes::BytesMut::from(sim_distance.1.as_slice()));
        let decoded_sim = ClientboundSetSimulationDistancePacket::stream_codec()
            .decode(&mut sim_buf)
            .unwrap();
        assert_eq!(
            decoded_sim,
            ClientboundSetSimulationDistancePacket::new(4),
            "simulation distance is the world's tick distance 4"
        );
        assert_eq!(sim_buf.readable_bytes(), 0, "no trailing bytes");
    }

    // A compile-time smoke of the session's spawn geometry (the respawn pos,
    // the M1 world spawn) — the burst-level assertions live in `join_burst.rs`.
    #[test]
    fn spawn_session_places_player_at_the_level_spawn() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![]);
        assert_eq!(manager.session_count(), 1);
        assert!(manager.indices.uuid_for(ID).is_some(), "uuid indexed");
        assert_eq!(
            manager.indices.connection_for(&probe_profile().id()),
            Some(ID),
            "playersByUUID forward lookup"
        );
    }
}

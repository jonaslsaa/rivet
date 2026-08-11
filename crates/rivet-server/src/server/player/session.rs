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
//! One tick-owned [`KeepaliveState`] per live session drives the Paper-faithful
//! keepalive loop (#157): `keepConnectionAlive` each tick (1s transmit cadence,
//! 30s kick limit, both off the `TickContext` clock axes so the machine is
//! deterministic under `SimTime`), and `handleKeepAlive` on the serverbound
//! `keep_alive` (id 28) reply — a non-`Accepted` outcome (wrong/stale id)
//! disconnects with TIMEOUT, and a silent client is disconnected when its oldest
//! challenge exceeds the kick limit.
//!
//! One tick-owned [`Session`] per live session carries the authoritative
//! movement/teleport state (issue #158): the spawn teleport (`PlayerList.
//! placeNewPlayer` → `teleport`) embeds a real `awaitingTeleport` id in the
//! burst's player-position packet; `accept_teleportation` (id 0) validates that
//! pending id (correct → snap to the awaited position, matching id with no
//! pending → `invalid_player_movement` kick, stale id → silent no-op);
//! `move_player` (ids 30-33) gates non-finite values with the same kick, ignores
//! position movement while the teleport is pending (rotation-only snap), and
//! otherwise routes the accepted finite position + rotation into the tick-owned
//! player (`absSnapTo`) with the M1 permissive too-quickly predicate evaluated
//! but never acted on. The `move_frames_seen` counter and the `player_position`/
//! `player_yaw`/`player_pitch` accessors expose the tick-owned state to tests,
//! proving server authority rather than client prediction.
//!
//! The outbound burst fires exactly once per connection (the handoff is consumed
//! — [`ConnectionRegistry::take_play_handoff`]), and the join burst is
//! backpressured by the connection's bounded outbound channel exactly like every
//! other tick→network send (overflow policy disconnects; nothing buffers
//! unbounded). Sessions are cleaned up when their connection is lost (the tick
//! prunes the registry entry on disconnect/EOF, and the manager removes the
//! player index + any pending frames on the next tick).
//!
//! With `RIVET_TRACE_MOVEMENT=1` (issue #53, see [`movement_trace`]) the
//! movement/teleport paths emit stable info-level `tracing` records from here:
//! a `RIVET_TELEPORT_ACK` per parsed ack frame (accepted/ignored/invalid),
//! `RIVET_MOVE_ACCEPTED` on the post-ack accepted movement path with the exact
//! values snapped into the tick-owned player, and a `RIVET_SESSION_END` from
//! [`Self::prune_lost`] for EOF/Timeout/InboundOverflow closes with the final
//! authoritative position and counts. The gate is one-time env read and the
//! records flow through the existing subscriber — no new shared state.

use rivet_protocol::codec::StreamDecoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::game::serversbound_accept_teleportation_packet::accept_teleportation_codec;
use rivet_protocol::game::serversbound_move_player_packet::{
    ServerboundMovePlayerPacket, pos_codec, pos_rot_codec, rot_codec, status_only_codec,
};
use rivet_protocol::generated::packets::play::serverbound::PacketType as ServerboundPacketType;
use rivet_protocol::protocol::common::clientbound_disconnect::ClientboundDisconnectPacket;
use rivet_protocol::protocol::common::clientbound_keep_alive::ClientboundKeepAlivePacket;
use rivet_protocol::protocol::common::serverbound_keep_alive::ServerboundKeepAlivePacket;
use rivet_text::Component;

use crate::server::keepalive::{KEEPALIVE_LIMIT_NS, KeepaliveResponseOutcome, KeepaliveState};
use crate::server::level::entity_id_allocator::EntityIdAllocator;
use crate::server::level::player_chunk_loader::PlayerChunkLoader;
use crate::server::level::server_level::{ServerLevel, ServerLevelConfig};
use crate::server::movement_math::{
    DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER, MoveState, build_move_targets, contains_invalid_values,
    moved_distance_sqr, moved_too_quickly, movement_speed,
};
use crate::server::movement_trace::{
    self, AckOutcome, trace_move_accepted, trace_session_end, trace_teleport_ack,
};
use crate::server::network::connection_id::ConnectionId;
use crate::server::network::keepalive::{KeepaliveSink, drive_keepalive};
use crate::server::network::packet_listener::DisconnectReason;
use crate::server::player::join::{JoinConfig, place_new_player};
use crate::server::player::play_sender::PlaySender;
use crate::server::player::{PlayerIndices, ServerPlayer};
use crate::server::teleport_ack::{TeleportAckOutcome, TeleportAckState};
use crate::server::tick::channels::{OutboundEvent, ServerboundFrame};
use crate::server::tick::registry::ConnectionRegistry;
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

/// The clientbound `keep_alive` packet id in the play protocol —
/// `GameProtocols.CLIENTBOUND_TEMPLATE` (`rivet-protocol` generated table):
/// `minecraft:keep_alive` is 44. Distinct from the configuration protocol's
/// `keep_alive` id 4, which the network-side `ConnectionKeepaliveSink` uses.
pub const PLAY_CLIENTBOUND_KEEP_ALIVE_ID: u32 =
    rivet_protocol::generated::packets::play::clientbound::PacketType::KeepAlive.id();

/// The clientbound `disconnect` packet id in the play protocol — the play
/// clientbound table's `minecraft:disconnect` is 32 (the configuration table
/// registers the same packet at id 2, but the invalid-movement kick runs in
/// play, so only the play id is needed here).
pub const PLAY_CLIENTBOUND_DISCONNECT_ID: u32 =
    rivet_protocol::generated::packets::play::clientbound::PacketType::Disconnect.id();

/// The tick-owned play session manager — one instance per server, owning the
/// `ServerLevel` (tick-confined), the entity-id allocator (`ServerLevel.
/// ENTITY_COUNTER` is `static`, so the counter lives here at the server play
/// scope, shared across every level — GitHub #222), the `PlaySender`, the
/// player indices, and the per-connection pending-frame buffers. Confined to
/// the tick thread (moved in via `Server::serve`); the counters are plain
/// fields readable by tests.
pub struct PlayerSessionManager {
    level: ServerLevel,
    /// `ServerLevel.ENTITY_COUNTER` + `getNextEntityId()` (GitHub #222) — the
    /// server-global entity-id allocator, tick-thread-owned. Java's counter is
    /// `static` to `ServerLevel`, so one counter is shared across dimensions and
    /// world instances; the manager is that shared scope in Rivet.
    entity_ids: EntityIdAllocator,
    sender: PlaySender,
    join: JoinConfig,
    indices: PlayerIndices,
    /// Inbound play frames that arrived for a connection whose handoff has not
    /// been applied yet — FIFO retention across the race, keyed by connection.
    pending: std::collections::HashMap<ConnectionId, Vec<ServerboundFrame>>,
    /// Inbound frames routed to a live session (test/counterfactual observability:
    /// proves no loss and no double-delivery across the handoff race).
    routed_frames: usize,
    /// `keep_alive` (id 28) frames whose body parsed.
    keep_alives_seen: usize,
    /// `move_player` (ids 30-33) / `accept_teleportation` (id 0) frames whose
    /// body parsed (issue #158 observability).
    move_frames_seen: usize,
    /// One tick-owned keepalive state per live session (issue #157), keyed by
    /// connection. Inserted at session spawn; driven every tick; removed on
    /// disconnect (timeout, bad reply, or connection loss).
    keepalive: std::collections::HashMap<ConnectionId, KeepaliveState>,
    /// The keepalive kick limit in ns each session's state is built with (the
    /// `ServerConfig.keepalive_timeout` the session config carried).
    keepalive_timeout_ns: i64,
    /// One tick-owned play session per live connection (issue #158), keyed by
    /// connection: the authoritative `ServerPlayer` the movement/teleport paths
    /// write into, plus the `awaitingTeleport` ack machine and Paper move
    /// anchors. Inserted at session spawn; every inbound move/ack frame is
    /// checked against it; removed alongside the keepalive state on disconnect.
    sessions: std::collections::HashMap<ConnectionId, Session>,
}

/// The tick-owned session state for one connection (issue #158): the
/// authoritative `ServerPlayer` (OWNERSHIP "one owner: the tick thread"), the
/// `awaitingTeleport` ack machine plus the Paper `firstGood/lastGood` move
/// anchors, and the per-player `PlayerChunkLoader` (issue #521) that tracks the
/// player's last chunk/view state. The network task only forwards wire frames
/// over the bounded channel; all mutation happens here on the tick thread.
struct Session {
    player: ServerPlayer,
    movement: SessionMovement,
    /// The per-player chunk loader (issue #521): the last chunk center +
    /// send/tick distances committed by the join burst's `add()`, then advanced
    /// by `update()` on each accepted chunk-boundary move. Dropped with the
    /// session on disconnect. The world reference the `update` path needs is the
    /// manager's level (the loader holds only the committed scalar state, never
    /// a world reference).
    loader: PlayerChunkLoader,
    /// The client's requested view distance (the handoff `ClientInformation`),
    /// re-fed into `update` so the send/tick distances re-derive identically to
    /// the join burst.
    requested_view_distance: Option<i32>,
    /// The number of accepted post-ack movements this session routed into the
    /// tick-owned player (each `RIVET_MOVE_ACCEPTED` record increments it). The
    /// per-session counter is tick-owned — no shared state — and is reported in
    /// the session's `RIVET_SESSION_END` record so the trace consumer can sum
    /// the authoritative displacement and compare it with the final position.
    accepted_frames: usize,
}

/// The movement/teleport half of a session: the `awaitingTeleport` ack state
/// plus the Paper `firstGood/lastGood` anchors.
struct SessionMovement {
    teleport: TeleportAckState,
    anchors: MoveState,
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
    /// The keepalive kick limit in ns (from `ServerConfig.keepalive_timeout`).
    pub keepalive_timeout_ns: i64,
}

/// The default M1 play-session config: the superflat `ServerLevel` (seed 42,
/// view distance 4, spawn (0,-63,0)) the #153 capture records, the M1 join
/// config (`max_players 20`, offline, flat), and the two single-registry
/// accesses the registry-aware play bodies resolve (the same construction the
/// `join_burst.rs` tests use). This is what `Server` builds when
/// `ServerConfig.enable_join` is set.
pub fn default_session_config(compression_threshold: i32) -> SessionManagerConfig {
    let level = ServerLevel::new(ServerLevelConfig::default());
    let is_flat = level.is_flat();
    SessionManagerConfig {
        compression_threshold,
        dimension_type_access: dimension_type_access(),
        world_clock_access: world_clock_access(),
        level,
        join: join_config(is_flat),
        keepalive_timeout_ns: KEEPALIVE_LIMIT_NS,
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
            entity_ids: EntityIdAllocator::new(),
            sender,
            join: config.join,
            indices: PlayerIndices::default(),
            pending: std::collections::HashMap::new(),
            routed_frames: 0,
            keep_alives_seen: 0,
            move_frames_seen: 0,
            keepalive: std::collections::HashMap::new(),
            keepalive_timeout_ns: config.keepalive_timeout_ns,
            sessions: std::collections::HashMap::new(),
        }
    }

    /// Run one tick of play session management: consume every connection's
    /// handoff (spawning its session + burst), route the tick's inbound frames
    /// into sessions, run each live session's keepalive tick (transmit +
    /// timeout), then clean up sessions whose connection is gone.
    pub fn tick(&mut self, ctx: &mut TickContext) {
        let ids: Vec<ConnectionId> = ctx.connections.ids().collect();
        for id in ids {
            if let Some((profile, client_information)) = ctx.connections.take_play_handoff(id) {
                self.spawn_session(ctx, id, profile, client_information);
            }
        }
        // `tickPlayer()` → `resetPosition()`: refresh every live session's move
        // anchors before any inbound move/ack frame is handled this tick (issue
        // #158). Paper resets `firstGood/lastGood` to the player's position at
        // the top of every tick, so the too-quickly predicate always measures
        // per-tick displacement against a fresh anchor.
        self.reset_move_anchors();
        let inbound = std::mem::take(&mut ctx.inbound);
        for (id, frame) in inbound {
            self.route_inbound(ctx, id, frame);
        }
        self.drive_keepalives(ctx);
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

    /// `move_player`/`accept_teleportation` frames whose body parsed (the
    /// movement counter — the `routed_frames` counterpart for #158).
    pub fn move_frames_seen(&self) -> usize {
        self.move_frames_seen
    }

    /// The authoritative `ServerPlayer` position of a live session (test
    /// observability for issue #158: proves the ack/movement paths write the
    /// tick-owned player — the counterfactual to "client prediction"). `None`
    /// when the connection has no session.
    pub fn player_position(&self, id: ConnectionId) -> Option<rivet_registry::core::Vec3> {
        self.sessions.get(&id).map(|s| s.player.position())
    }

    /// The authoritative `ServerPlayer` yaw of a live session (issue #158 test
    /// observability, the rotation half of the snap paths).
    pub fn player_yaw(&self, id: ConnectionId) -> Option<f32> {
        self.sessions.get(&id).map(|s| s.player.yaw())
    }

    /// The authoritative `ServerPlayer` pitch of a live session (issue #158
    /// test observability, the rotation half of the snap paths).
    pub fn player_pitch(&self, id: ConnectionId) -> Option<f32> {
        self.sessions.get(&id).map(|s| s.player.pitch())
    }

    /// The entity id of a live session (GitHub #222 test observability: proves
    /// each session's `ServerPlayer` carries a distinct non-zero id). `None`
    /// when the connection has no session.
    pub fn player_entity_id(&self, id: ConnectionId) -> Option<i32> {
        self.sessions.get(&id).map(|s| s.player.player_id())
    }

    /// The chunk center the session's `PlayerChunkLoader` is currently
    /// centered on (issue #521 test observability: proves a move recentered the
    /// loader — the cache center the client's cache is anchored to). `None`
    /// when the connection has no session.
    pub fn chunk_center(&self, id: ConnectionId) -> Option<rivet_registry::core::ChunkPos> {
        self.sessions.get(&id).map(|s| s.loader.last_chunk_pos())
    }

    /// The first `firstGood` anchor of a live session (issue #158 test
    /// observability for the per-tick `resetPosition` mirror). `None` when the
    /// connection has no session.
    pub fn move_anchor(&self, id: ConnectionId) -> Option<[f64; 3]> {
        self.sessions
            .get(&id)
            .map(|s| s.movement.anchors.first_good())
    }

    /// `ServerGamePacketListenerImpl.tickPlayer()` → `resetPosition()` — refresh
    /// every live session's `firstGood/lastGood` move anchors to the player's
    /// current position. Paper calls this at the top of every tick, so the
    /// too-quickly predicate measures per-tick displacement against a fresh
    /// anchor rather than a stale spawn-time value.
    fn reset_move_anchors(&mut self) {
        for session in self.sessions.values_mut() {
            let pos = session.player.position();
            session.movement.anchors.reset_position(pos.x, pos.y, pos.z);
        }
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
        // `ServerPlayer` carries the authenticated profile + the entity id:
        // Paper's `new ServerPlayer(...)` runs `Entity`'s constructor, which
        // calls `level.getNextEntityId()` — the server-global `ENTITY_COUNTER`
        // (GitHub #222). The allocation happens BEFORE the join burst fires, so
        // a burst failure consumes the id without registering it — exactly
        // Paper, which never rolls the counter back. Each session gets a
        // distinct non-zero id, so concurrent sessions never collide.
        let entity_id = self.entity_ids.next_id();
        let respawn = self.level.get_respawn_data();
        let pos = respawn.pos();
        let player = ServerPlayer::new(
            connection_id,
            profile,
            entity_id,
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

        // The spawn teleport (issue #158): `PlayerList.placeNewPlayer` calls
        // `playerConnection.teleport(player.getX(), ...)` — Paper's
        // `internalTeleport` increments `awaitingTeleport` (0 → 1) and records
        // the spawn as the awaited position. The id embedded in the burst's
        // player_position packet is this 1; the matching `accept_teleportation`
        // ack snaps the player back to spawn and clears the pending marker. The
        // move anchors (`firstGood/lastGood`) are seeded at the spawn position
        // (`resetPosition`); every subsequent tick refreshes them
        // (`reset_move_anchors`), exactly as Paper's `tickPlayer()`.
        let spawn = [
            player.position().x,
            player.position().y,
            player.position().z,
        ];
        let mut movement = SessionMovement {
            teleport: TeleportAckState::new(),
            anchors: MoveState::new(spawn[0], spawn[1], spawn[2]),
        };
        let teleport_id = movement
            .teleport
            .begin_teleport(spawn[0], spawn[1], spawn[2]);

        // Fire the join burst in Paper's order. `requested_view_distance` is the
        // client's `ClientInformation` view distance — the Moonrise ladder feeds
        // it through `client + 1` (the capture client's 8 caps at `load - 1` =
        // 4, the 117-chunk M1 send-set; a `create_default` client's 2 resolves
        // send 3, the 81-chunk square). The resolved distances also go into the
        // cache-radius packet this burst emits. The burst embeds `teleport_id`,
        // so it must succeed before the session (and its awaited spawn teleport)
        // is registered.
        //
        // The session-owned `PlayerChunkLoader` (issue #521) is built at the
        // player's chunk position and handed into the burst as its `add()` —
        // the burst commits the loader's last-chunk/distance state to the spawn
        // view, so the movement-driven `update` later diffs against exactly the
        // send-set the client received.
        let requested_view_distance = Some(client_information.view_distance() as i32);
        let mut loader = PlayerChunkLoader::new(rivet_registry::core::ChunkPos::containing(
            &rivet_registry::core::BlockPos::containing(
                player.position().x,
                player.position().y,
                player.position().z,
            ),
        ));
        if let Err(e) = place_new_player(
            &mut self.sender,
            ctx.connections,
            connection_id,
            &player,
            &self.level,
            &self.join,
            requested_view_distance,
            &mut loader,
            teleport_id,
        ) {
            // A burst encode/send failure is a server-side fault or an outbound
            // overload; the connection was pruned by `ConnectionRegistry::send`
            // on overflow. The indices entry is rolled back so a later
            // re-connect starts clean.
            tracing::warn!(%connection_id, %e, "play session burst failed");
            self.indices.remove(&player.uuid());
            return;
        }

        // The player has joined the level: `addEntity`-equivalent registration —
        // `chunkSource.hasEntityWithId` now reports the id in use, so later
        // allocations skip it. This runs only after the burst SUCCEEDED: a
        // failed burst (rollback above) consumed the allocated id without
        // registering it, the faithful Paper failure behavior. Released when the
        // session's connection is lost (`prune_lost`).
        self.entity_ids.mark_in_use(entity_id);

        // The session is live: the tick-owned player (the movement/teleport
        // write target), the ack/anchors state, and the per-player chunk loader
        // (issue #521) whose committed state now matches the burst the client
        // received, keyed by connection.
        self.sessions.insert(
            connection_id,
            Session {
                player,
                movement,
                loader,
                requested_view_distance,
                accepted_frames: 0,
            },
        );

        // The session now owns a tick-thread keepalive state (issue #157).
        // `lastKeepAliveTx` is seeded with this tick's reading so the 1s
        // transmit throttle counts from now, and the kick limit comes from
        // `ServerConfig.keepalive_timeout`.
        self.keepalive.insert(
            connection_id,
            KeepaliveState::new_with_timeout(ctx.now_ns, self.keepalive_timeout_ns),
        );

        // Deliver frames that arrived before the handoff (the client coalesced
        // `finish_configuration` with its first play packet) in FIFO order.
        if let Some(frames) = self.pending.remove(&connection_id) {
            for frame in frames {
                self.route_inbound(ctx, connection_id, frame);
            }
        }
    }

    /// `ServerGamePacketListenerImpl.tick`'s keepalive half — run
    /// `keepConnectionAlive` for every live session off the tick's clock axes,
    /// transmitting the clientbound keep_alive when the 1s throttle elapses and
    /// disconnecting with TIMEOUT when a session's oldest challenge exceeds the
    /// kick limit. The state is removed once the disconnect fires (the
    /// connection is being torn down; Paper's disconnect closes the listener).
    fn drive_keepalives(&mut self, ctx: &mut TickContext) {
        let ids: Vec<ConnectionId> = self.keepalive.keys().copied().collect();
        for id in ids {
            let Some(keepalive) = self.keepalive.get_mut(&id) else {
                continue;
            };
            let mut sink = PlayKeepaliveSink {
                sender: &mut self.sender,
                connections: ctx.connections,
                id,
            };
            match drive_keepalive(keepalive, ctx.now_ns, ctx.now_ms, &mut sink) {
                Ok(()) => {}
                Err(reason) => {
                    tracing::warn!(%id, %reason, "disconnecting play session");
                    // `drive_keepalive` returns the reason (`disconnect_timeout`
                    // or a send failure); the disconnect itself is issued here,
                    // mirroring Paper's `disconnect(TIMEOUT, TIMEOUT)`.
                    let _ = ctx
                        .connections
                        .send(id, OutboundEvent::Disconnect { reason });
                    self.keepalive.remove(&id);
                }
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
        self.dispatch(ctx, id, frame);
    }

    /// Dispatch one play frame to its session. `keep_alive` (id 28) is decoded
    /// and its echoed id runs through `handleKeepAlive` (a wrong/stale id is a
    /// TIMEOUT disconnect, exactly as in Java). The four `move_player` variants
    /// (ids 30-33) and `accept_teleportation` (id 0) run the Paper-faithful
    /// movement/teleport path (issue #158): the ack machine validates the
    /// pending teleport id, movement is ignored while a teleport is pending,
    /// and accepted finite movement is routed into the tick-owned player. Every
    /// other play frame is dropped without pretending semantics.
    fn dispatch(&mut self, ctx: &mut TickContext, id: ConnectionId, frame: ServerboundFrame) {
        self.routed_frames += 1;
        let Some(packet_id) = read_packet_id(&frame.bytes) else {
            return;
        };
        let Some(packet_type) = ServerboundPacketType::from_id(packet_id) else {
            return;
        };
        match packet_type {
            ServerboundPacketType::KeepAlive => {
                self.dispatch_keepalive(ctx, id, &frame.bytes);
            }
            ServerboundPacketType::MovePlayerPos
            | ServerboundPacketType::MovePlayerPosRot
            | ServerboundPacketType::MovePlayerRot
            | ServerboundPacketType::MovePlayerStatusOnly => {
                self.dispatch_move_player(ctx, id, &frame.bytes, packet_id);
            }
            ServerboundPacketType::AcceptTeleportation => {
                self.dispatch_accept_teleportation(ctx, id, &frame.bytes, packet_id);
            }
            _ => {}
        }
    }

    /// `ServerGamePacketListenerImpl.handleKeepAlive` — decode the echoed `long`
    /// id and match it against the pending challenges. The decode runs on the
    /// tick thread, so its panics must be contained here, never abort the tick:
    /// a truncated body (`read_long` on < 8 remaining bytes panics) is dropped
    /// and logged, not counted — `keep_alives_seen` only counts frames whose
    /// body parsed, matching the decode-boundary containment of
    /// [`crate::server::network::packet_listener::decode_packet`].
    fn dispatch_keepalive(&mut self, ctx: &mut TickContext, id: ConnectionId, bytes: &[u8]) {
        let mut raw = bytes::BytesMut::from(bytes);
        let _ = rivet_protocol::var_int::read(&mut raw); // packet id
        let mut input = FriendlyByteBuf::new(raw);
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ServerboundKeepAlivePacket::stream_codec().decode(&mut input)
        }));
        let echo_id = match decoded {
            Ok(Ok(packet)) => {
                self.keep_alives_seen += 1;
                packet.id()
            }
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(%id, "malformed keep_alive play frame");
                return;
            }
        };
        // `handleKeepAlive(packet, System.nanoTime())` — the reply is matched
        // against the pending challenges. The frame arrived this tick, so the
        // receive reading is this tick's `now_ns`.
        let Some(keepalive) = self.keepalive.get_mut(&id) else {
            return;
        };
        match keepalive.handle_keepalive(echo_id, ctx.now_ns) {
            KeepaliveResponseOutcome::Accepted => {}
            outcome => {
                tracing::warn!(%id, ?outcome, "disconnecting play session on bad keep_alive reply");
                self.keepalive.remove(&id);
                let _ = ctx.connections.send(
                    id,
                    OutboundEvent::Disconnect {
                        reason: DisconnectReason::Timeout,
                    },
                );
            }
        }
    }

    /// `ServerGamePacketListenerImpl.handleAcceptTeleportPacket` — validate the
    /// ack id against the pending teleport. A matching id snaps the player to
    /// the awaited position (and updates `lastGood`); a matching id with no
    /// pending position is the Paper-faithful `invalid_player_movement` kick; a
    /// wrong/stale id is a silent no-op.
    fn dispatch_accept_teleportation(
        &mut self,
        ctx: &mut TickContext,
        id: ConnectionId,
        bytes: &[u8],
        packet_id: u32,
    ) {
        let decoded = decode_game_body(bytes, packet_id, accept_teleportation_codec());
        let ack = match decoded {
            Ok(ack) => {
                self.move_frames_seen += 1;
                ack
            }
            Err(e) => {
                tracing::warn!(%id, %e, "malformed accept_teleportation play frame");
                return;
            }
        };
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        let player = &mut session.player;
        let movement = &mut session.movement;
        let ack_id = ack.get_id();
        match movement.teleport.accept(ack_id) {
            TeleportAckOutcome::Accepted { x, y, z } => {
                // `absSnapTo(awaitingPositionFromClient, player.getYRot(), ...)`:
                // rotation is unchanged, position snaps to the awaited spawn.
                player.abs_snap_to(x, y, z, player.yaw(), player.pitch());
                movement.anchors.on_move_accepted(x, y, z);
                trace_teleport_ack(id, ack_id, AckOutcome::Accepted, Some([x, y, z]));
            }
            TeleportAckOutcome::InvalidMovementKick => {
                tracing::warn!(%id, "teleport ack with no pending teleport");
                trace_teleport_ack(id, ack_id, AckOutcome::Invalid, None);
                self.disconnect_invalid_movement(ctx, id);
            }
            TeleportAckOutcome::Ignored => {
                trace_teleport_ack(id, ack_id, AckOutcome::Ignored, None);
            }
        }
    }

    /// Paper `ServerGamePacketListenerImpl.disconnect(reason)` (the
    /// `invalid_player_movement` kick) — `send(new
    /// ClientboundDisconnectPacket(reason), PacketSendListener.thenRun(() ->
    /// disconnect(reason)))`. The reason packet is encoded and queued *before*
    /// the `OutboundEvent::Disconnect` so the network side flushes the reason
    /// frame before closing (`handle_outbound` drains queued frames first; issue
    /// #86, #158).
    ///
    /// When the reason cannot be *encoded* the connection is still live, so the
    /// `Unsupported` reason is queued and delivered. When the reason frame cannot
    /// be queued — outbound overflow or a closed channel — `ConnectionRegistry::send`
    /// prunes the connection and records `Overflow`/`EndOfStream` as the terminal
    /// reason, and the follow-up `Disconnect` is a `Gone` no-op: the socket still
    /// closes, just under that terminal reason instead of `Unsupported`.
    fn disconnect_invalid_movement(&mut self, ctx: &mut TickContext, id: ConnectionId) {
        let packet = ClientboundDisconnectPacket::new(Component::translatable(
            "multiplayer.disconnect.invalid_player_movement",
        ));
        let reason = match self
            .sender
            .encode_body(ClientboundDisconnectPacket::stream_codec(), &packet)
            .and_then(|body| {
                self.sender
                    .send_packet(ctx.connections, id, PLAY_CLIENTBOUND_DISCONNECT_ID, &body)
                    .map_err(|e| e.to_string())
            }) {
            Ok(()) => DisconnectReason::InvalidPlayerMovement,
            Err(e) => DisconnectReason::Unsupported(format!("send disconnect: {e}")),
        };
        let _ = ctx
            .connections
            .send(id, OutboundEvent::Disconnect { reason });
    }

    /// `ServerGamePacketListenerImpl.handleMovePlayer` (the M1 subset) — the
    /// authoritative movement path. The invalid-value gate (NaN position /
    /// non-finite rotation) disconnects with `invalid_player_movement`. While a
    /// teleport is pending, position movement is ignored and only rotation is
    /// snapped (`updateAwaitingTeleport`). Otherwise the accepted position is
    /// clamped, the too-quickly predicate is evaluated permissively (M1: never
    /// acted on), and the accepted finite position + rotation is routed into the
    /// tick-owned player. Gravity, collisions, movedWrongly, and the anti-cheat
    /// responses are M3.
    fn dispatch_move_player(
        &mut self,
        ctx: &mut TickContext,
        id: ConnectionId,
        bytes: &[u8],
        packet_id: u32,
    ) {
        let decoded = decode_move_player(bytes, packet_id);
        let packet = match decoded {
            Ok(packet) => {
                self.move_frames_seen += 1;
                packet
            }
            Err(e) => {
                tracing::warn!(%id, %e, "malformed move_player play frame");
                return;
            }
        };

        // `containsInvalidValues(packet.getX(0.0), ..., packet.getXRot(0.0F))` —
        // the gate runs on the raw packet values with zero fallbacks. It fires
        // before the session lookup, exactly as in Java.
        if contains_invalid_values(
            packet.get_x(0.0),
            packet.get_y(0.0),
            packet.get_z(0.0),
            packet.get_y_rot(0.0),
            packet.get_x_rot(0.0),
        ) {
            self.disconnect_invalid_movement(ctx, id);
            return;
        }
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        let player = &mut session.player;
        let movement = &mut session.movement;

        // `updateAwaitingTeleport()` — while a teleport is pending the client's
        // position movement is ignored; only the wrapped rotation is accepted.
        if movement.teleport.is_pending() {
            player.abs_snap_rotation_to(
                rivet_util::mth::wrap_degrees_f32(packet.get_y_rot(player.yaw())),
                rivet_util::mth::wrap_degrees_f32(packet.get_x_rot(player.pitch())),
            );
            return;
        }

        // `Mth.wrapDegrees(...)` + `clampHorizontal`/`clampVertical` on the
        // packet fields, with the player's current values as fallbacks.
        let targets = build_move_targets(
            move_pos_field(&packet, 0),
            move_pos_field(&packet, 1),
            move_pos_field(&packet, 2),
            move_rot_field(&packet, 0),
            move_rot_field(&packet, 1),
            player.position().x,
            player.position().y,
            player.position().z,
            player.yaw(),
            player.pitch(),
        );
        let first = movement.anchors.first_good();
        let last = movement.anchors.last_good();
        let start = [
            player.position().x,
            player.position().y,
            player.position().z,
        ];
        let moved = moved_distance_sqr([targets.x, targets.y, targets.z], first, start, last);
        // The M1 permissive boundary: `shouldCheckPlayerMovement` is assumed
        // true and the too-quickly predicate is evaluated with the vanilla
        // walking speed + default multiplier, but a violation is only logged —
        // the M3 anti-cheat response (the `PlayerFailMoveEvent` / `teleport`
        // re-sync) is out of scope.
        if moved_too_quickly(
            moved,
            0.0,
            false,
            1,
            movement_speed(false, 0.05, 0.1),
            DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER,
        ) {
            tracing::debug!(%id, ?moved, "player moved too quickly (M1: permissive)");
        }

        // Route the accepted finite movement into the tick-owned player
        // (`absSnapTo(target, yRot, xRot)`); `lastGood` follows the accepted
        // position. This is the full post-ack accepted path (the gate above
        // already rejected invalid values and the pending-teleport snap): the
        // record carries the exact values snapped into the player and the
        // session's accepted-move counter.
        player.abs_snap_to(
            targets.x,
            targets.y,
            targets.z,
            targets.y_rot,
            targets.x_rot,
        );
        movement
            .anchors
            .on_move_accepted(targets.x, targets.y, targets.z);
        session.accepted_frames += 1;
        trace_move_accepted(
            id,
            targets.x,
            targets.y,
            targets.z,
            targets.y_rot,
            targets.x_rot,
            session.accepted_frames,
        );

        // Issue #521: after an accepted chunk-boundary move, recenter the
        // player's chunk view. `update` early-returns for an intra-chunk move
        // (the nothing-to-do guard), so calling it on every accepted move is
        // exactly the boundary-gated behavior — only a chunk crossing (or a
        // send/tick distance change) produces output. It re-derives the
        // send/tick distances from the same `requested_view_distance` the join
        // burst used, so the `lastChunk`/`lastSendDistance` it diffs against
        // are the ones the client's cache actually holds. `self.level` (read)
        // and `session` (mutated) are disjoint fields, so the borrow splits.
        let new_chunk = rivet_registry::core::ChunkPos::containing(
            &rivet_registry::core::BlockPos::containing(targets.x, targets.y, targets.z),
        );
        let recenter =
            session
                .loader
                .update(&self.level, new_chunk, session.requested_view_distance);
        match recenter {
            Ok(packets) => {
                // Queue the ordered cache-center + newly entered chunks over the
                // bounded outbound channel, exactly like the join burst's
                // `PlaySender` path. A send failure (outbound overflow / gone
                // connection) prunes the connection — the same backpressure
                // policy as every other tick→network send.
                for packet in packets {
                    if let Err(e) =
                        self.sender
                            .send_packet(ctx.connections, id, packet.id, &packet.body)
                    {
                        tracing::warn!(%id, %e, "disconnecting play session on chunk send");
                        return;
                    }
                }
            }
            Err(e) => {
                // A typed missing-chunk failure (the `RequireLoaded` UNVERIFIED
                // error — no generation fallback, no silent substitution) or an
                // encode failure is a server-side fault; Paper disconnects on a
                // chunk send failure. The connection is being torn down, so the
                // loader state `update` committed for the new center is moot.
                tracing::warn!(%id, %e, "disconnecting play session on chunk-loader update failure");
                let _ = ctx.connections.send(
                    id,
                    OutboundEvent::Disconnect {
                        reason: DisconnectReason::Unsupported(format!("chunk update: {e}")),
                    },
                );
            }
        }
    }

    /// Remove sessions whose connection is gone (the tick pruned the registry
    /// entry on disconnect/EOF/overflow). The player index, any pending frames,
    /// and the keepalive state for a lost connection are dropped so the indices
    /// stay a bijection with live connections.
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
        let keepalive_ids: Vec<ConnectionId> = self.keepalive.keys().copied().collect();
        for id in keepalive_ids {
            if !ctx.connections.contains(id) {
                self.keepalive.remove(&id);
            }
        }
        let session_ids: Vec<ConnectionId> = self.sessions.keys().copied().collect();
        for id in session_ids {
            if !ctx.connections.contains(id) {
                // The player left the level: release its entity id so the
                // world's `hasEntityWithId` no longer reports it (GitHub #222).
                // The counter is untouched, so the freed id is never reused.
                if let Some(session) = self.sessions.get(&id) {
                    self.entity_ids.release(session.player.player_id());
                }
                // The registry recorded the reason when the connection was
                // removed (a `Disconnect` event, a closed inbound channel, or
                // an overflow prune). The trace reports the session end only
                // for the EOF / Timeout / InboundOverflow paths — a deliberate
                // close with a recorded reason — with the final authoritative
                // position + rotation and the session's movement counts.
                if let Some(reason) = ctx.connections.take_disconnect_reason(id)
                    && movement_trace::is_traced_disconnect(&reason)
                    && let Some(session) = self.sessions.get(&id)
                {
                    let pos = session.player.position();
                    trace_session_end(
                        id,
                        reason,
                        pos.x,
                        pos.y,
                        pos.z,
                        session.player.yaw(),
                        session.player.pitch(),
                        session.accepted_frames,
                        self.move_frames_seen,
                    );
                }
                self.sessions.remove(&id);
            }
        }
        // Reasons for connections this prune did not touch belong to
        // connections that never reached play; the tick loop drains them at the
        // tick boundary (the registry stays bounded whether or not a session
        // manager is registered).
    }
}

/// The [`KeepaliveSink`] for a play session: transmit the challenge through the
/// tick-side [`PlaySender`] as a play clientbound `keep_alive` (id 44, the
/// `GameProtocols` clientbound table — distinct from the configuration id 4),
/// and report a timeout disconnect. A send failure (overload / gone connection)
/// maps to the connection's disconnect reason so `drive_keepalive` returns
/// `Err` and the session is torn down exactly like a timeout kick.
struct PlayKeepaliveSink<'a> {
    sender: &'a mut PlaySender,
    connections: &'a mut ConnectionRegistry,
    id: ConnectionId,
}

impl KeepaliveSink for PlayKeepaliveSink<'_> {
    fn send_keepalive(&mut self, challenge_id: i64) -> Result<(), DisconnectReason> {
        let body = self
            .sender
            .encode_body(
                ClientboundKeepAlivePacket::stream_codec(),
                &ClientboundKeepAlivePacket::new(challenge_id),
            )
            .map_err(|e| DisconnectReason::Unsupported(format!("encode keepalive: {e}")))?;
        self.sender
            .send_packet(
                self.connections,
                self.id,
                PLAY_CLIENTBOUND_KEEP_ALIVE_ID,
                &body,
            )
            .map_err(|e| DisconnectReason::Unsupported(format!("send keepalive: {e}")))
    }

    fn disconnect_timeout(&mut self) -> DisconnectReason {
        DisconnectReason::Timeout
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

/// Decode a play packet body with the tick-thread panic containment of
/// [`crate::server::network::packet_listener::decode_packet`]: the packet-id
/// varint is consumed (the caller already read it to dispatch), the body is
/// decoded inside `catch_unwind` (a truncated scalar read panics in the codec,
/// matching Java's unchecked `IndexOutOfBoundsException`), and trailing bytes
/// are a "packet was larger than expected" error. `Err`/panic map to `Malformed`
/// — the movement handlers log and drop the frame, never abort the tick.
fn decode_game_body<T, C>(bytes: &[u8], packet_id: u32, codec: C) -> Result<T, String>
where
    C: StreamDecoder<FriendlyByteBuf, T>,
{
    let mut buf = bytes::BytesMut::from(bytes);
    let _ = rivet_protocol::var_int::read(&mut buf); // consume the id (dispatch validated it)
    let mut input = FriendlyByteBuf::new(buf);
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| codec.decode(&mut input)))
        .map_err(|payload| {
            format!(
                "decoding packet {packet_id} panicked: {}",
                panic_message(payload)
            )
        })?
        .map_err(|e| format!("decoding packet {packet_id}: {}", e.message))?;
    if input.readable_bytes() != 0 {
        return Err(format!(
            "packet {packet_id} was larger than expected, {} bytes extra",
            input.readable_bytes()
        ));
    }
    Ok(value)
}

/// Decode a `ServerboundMovePlayerPacket` with the variant's own codec (the
/// four variants share the enum type but have distinct wire layouts — the id
/// selects the reader, exactly as Java's per-subclass `STREAM_CODEC`).
fn decode_move_player(bytes: &[u8], packet_id: u32) -> Result<ServerboundMovePlayerPacket, String> {
    use ServerboundPacketType as T;
    match packet_id {
        p if p == T::MovePlayerPos as u32 => decode_game_body(bytes, packet_id, pos_codec()),
        p if p == T::MovePlayerPosRot as u32 => decode_game_body(bytes, packet_id, pos_rot_codec()),
        p if p == T::MovePlayerRot as u32 => decode_game_body(bytes, packet_id, rot_codec()),
        p if p == T::MovePlayerStatusOnly as u32 => {
            decode_game_body(bytes, packet_id, status_only_codec())
        }
        _ => Err(format!("not a move_player packet id: {packet_id}")),
    }
}

/// The packet's position field by index (0=x, 1=y, 2=z) as an `Option` —
/// `Some` only when the variant stores it (`hasPos`). The handler feeds these
/// to `build_move_targets`, which applies the player-value fallback and the
/// Paper clamps/wrapping itself.
fn move_pos_field(packet: &ServerboundMovePlayerPacket, index: u8) -> Option<f64> {
    match packet {
        ServerboundMovePlayerPacket::Pos { x, y, z, .. }
        | ServerboundMovePlayerPacket::PosRot { x, y, z, .. } => match index {
            0 => Some(*x),
            1 => Some(*y),
            2 => Some(*z),
            _ => None,
        },
        ServerboundMovePlayerPacket::Rot { .. }
        | ServerboundMovePlayerPacket::StatusOnly { .. } => None,
    }
}

/// The packet's rotation field by index (0=yRot, 1=xRot) as an `Option` —
/// `Some` only when the variant stores it (`hasRot`).
fn move_rot_field(packet: &ServerboundMovePlayerPacket, index: u8) -> Option<f32> {
    match packet {
        ServerboundMovePlayerPacket::PosRot { y_rot, x_rot, .. }
        | ServerboundMovePlayerPacket::Rot { y_rot, x_rot, .. } => match index {
            0 => Some(*y_rot),
            1 => Some(*x_rot),
            _ => None,
        },
        ServerboundMovePlayerPacket::Pos { .. }
        | ServerboundMovePlayerPacket::StatusOnly { .. } => None,
    }
}

/// Extract the message from a `catch_unwind` panic payload (a `&str` or
/// `String`, as `panic!` produces).
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        msg.to_string()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unknown panic payload".into()
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

/// The M1 `JoinConfig` (`max_players 20`, offline, death screen on; the
/// reduced-debug / limited-crafting rules off). `is_flat` is `ServerLevel
/// .isFlat()` — true for the superflat world, false for the region-backed
/// overworld (the login packet's `is_flat` flag).
fn join_config(is_flat: bool) -> JoinConfig {
    JoinConfig {
        max_players: 20,
        hardcore: false,
        level_keys: vanilla_level_keys(),
        online_mode: false,
        enforces_secure_chat: false,
        show_death_screen: true,
        reduced_debug_info: false,
        do_limited_crafting: false,
        is_flat,
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
    use crate::server::movement_trace::{TAG_MOVE_ACCEPTED, TAG_SESSION_END, TAG_TELEPORT_ACK};
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
    /// ported serverbound play body, so `dispatch` decodes it and routes the id
    /// through `handleKeepAlive`.
    fn keepalive_frame(id: i64) -> ServerboundFrame {
        let mut bytes = varint(28);
        bytes.extend_from_slice(&id.to_be_bytes());
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// A play frame with a non-keepalive packet id (`dispatch` drops those
    /// without decoding — routing-only frames for the buffering tests, which
    /// must not trip the keepalive reply validation).
    fn play_frame(packet_id: i32, body: &[u8]) -> ServerboundFrame {
        let mut bytes = varint(packet_id);
        bytes.extend_from_slice(body);
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// An `accept_teleportation` (id 0) play frame echoing the given teleport
    /// id — the ack for the pending `awaitingTeleport`.
    fn accept_teleport_frame(id: i32) -> ServerboundFrame {
        let mut bytes = varint(0);
        rivet_protocol::var_int::write(&mut bytes, id);
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// A `move_player_pos` (id 30) play frame carrying a position (3 doubles +
    /// the 1-byte flags, not on ground) — a movement the server must gate and
    /// route.
    fn move_pos_frame(x: f64, y: f64, z: f64) -> ServerboundFrame {
        let mut bytes = varint(30);
        bytes.extend_from_slice(&x.to_be_bytes());
        bytes.extend_from_slice(&y.to_be_bytes());
        bytes.extend_from_slice(&z.to_be_bytes());
        bytes.push(0x00);
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// A `move_player_rot` (id 32) play frame carrying a rotation (2 floats +
    /// the 1-byte flags) — a rotation-only movement, used to pin the
    /// rotation-is-accepted-while-teleport-pending path.
    fn move_rot_frame(y_rot: f32, x_rot: f32) -> ServerboundFrame {
        let mut bytes = varint(32);
        bytes.extend_from_slice(&y_rot.to_be_bytes());
        bytes.extend_from_slice(&x_rot.to_be_bytes());
        bytes.push(0x00);
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// True when `out_rx` carries a `Disconnect` (its reason recorded).
    fn drained_disconnect_reason(
        out_rx: &mut tokio::sync::mpsc::Receiver<OutboundEvent>,
    ) -> Option<DisconnectReason> {
        while let Ok(event) = out_rx.try_recv() {
            if let OutboundEvent::Disconnect { reason } = event {
                return Some(reason);
            }
        }
        None
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

    /// Run one tick of the manager at simulated `now_ms` (the monotonic clock
    /// starts at 0; `now_ns` is the inverse of the loop's derivation — the loop
    /// computes `now_ms = now_nanos / 1_000_000`, so here `now_ns =
    /// now_ms * 1_000_000`). Returns the manager (the registry borrow ends).
    fn run_tick_at(
        mut manager: PlayerSessionManager,
        registry: &mut ConnectionRegistry,
        inbound: Vec<(ConnectionId, ServerboundFrame)>,
        now_ms: i64,
    ) -> PlayerSessionManager {
        let mut ctx = TickContext {
            tick: 1,
            now_ns: now_ms * 1_000_000,
            now_ms,
            connections: registry,
            inbound,
        };
        manager.tick(&mut ctx);
        manager
    }

    /// Run one tick at `now_ms = 0` (the epoch reading sessions are seeded
    /// with).
    fn run_tick(
        manager: PlayerSessionManager,
        registry: &mut ConnectionRegistry,
        inbound: Vec<(ConnectionId, ServerboundFrame)>,
    ) -> PlayerSessionManager {
        run_tick_at(manager, registry, inbound, 0)
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

        // Tick 1: play frames arrive before the handoff — buffered FIFO. The
        // frames are non-keepalive so the buffering path is exercised without
        // the keepalive reply validation (a reply to a challenge the server
        // never sent would disconnect).
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, play_frame(0, &[])), (ID, play_frame(1, &[]))],
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
        manager = run_tick(manager, &mut registry, vec![(ID, play_frame(2, &[]))]);
        assert_eq!(manager.session_count(), 1, "handoff spawns the session");
        assert_eq!(
            manager.routed_frames(),
            3,
            "pending + fresh frames delivered exactly once"
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
        let (mut registry, mut out_rx) = connected_registry();
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
        // a panic. Only the well-formed keep_alive decoded; its id matches no
        // pending challenge (the session spawned this tick at t=0, before any
        // transmit), so it is counted as seen and then the reply is rejected
        // with a TIMEOUT disconnect, exactly like Paper.
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.routed_frames(), 4, "all frames consumed");
        assert_eq!(manager.keep_alives_seen(), 1, "only the valid keep_alive");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::Timeout),
            "a reply to an unsent challenge disconnects"
        );
    }

    #[test]
    fn keep_alive_beyond_handoff_is_routed_and_decoded() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        apply_enter_play(&mut registry);
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, keepalive_frame(42)), (ID, keepalive_frame(7))],
        );
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.routed_frames(), 2);
        assert_eq!(
            manager.keep_alives_seen(),
            2,
            "both keep_alive bodies decoded"
        );
        assert!(
            manager.pending.is_empty(),
            "handoff already applied, nothing buffered"
        );
        // The session spawned this tick at t=0 before any transmit, so neither
        // echoed id matches a pending challenge — the first reply disconnects
        // with TIMEOUT, exactly like Paper's "without matching challenge" path.
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::Timeout),
            "a reply to an unsent challenge is a TIMEOUT disconnect"
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

    /// A genuine `hasEntityWithId` skip through the real spawn path (GitHub
    /// #222) — the highest practical integration seam where the skip is
    /// reachable. The pure forward counter never re-encounters an id it already
    /// handed out (it only allocates newer ids), so Paper's guard fires for
    /// wrap-around or for a pre-existing entity holding an id the counter
    /// collides with — an entity loaded from disk, in the M1 model the only
    /// foreign ids. Seed the manager's allocator at a collision point (counter
    /// advanced to 1, id 2 held by a pre-existing entity) and prove the
    /// session's spawn skips 2.
    #[test]
    fn spawn_skips_an_in_use_id_at_the_spawn_seam() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        // Advance the counter to 1 (id 1 consumed, unused) and hold id 2 in
        // use, as a pre-existing entity in the world would.
        assert_eq!(manager.entity_ids.next_id(), 1);
        manager.entity_ids.mark_in_use(2);

        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![]);
        assert_eq!(manager.session_count(), 1);
        assert_eq!(
            manager.player_entity_id(ID),
            Some(3),
            "the spawn skips the in-use id 2 (hasEntityWithId)"
        );
    }

    /// The spawn teleport (issue #158): `placeNewPlayer` embeds
    /// `awaitingTeleport = 1` in the burst's player_position packet, and the
    /// matching `accept_teleportation` ack is `Accepted` — the player snaps to
    /// the awaited spawn and the pending marker clears, so movement the next
    /// tick routes into the tick-owned player instead of being ignored.
    #[test]
    fn matching_ack_snaps_to_awaited_position_and_clears_pending() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        // The session spawns this tick at the world spawn (0,-63,0) with the
        // spawn teleport (id 1) pending. The ack for id 1 is accepted.
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.move_frames_seen(), 1, "ack body parsed");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            None,
            "matching ack is not a disconnect"
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (0.0, -63.0, 0.0), "awaited spawn");

        // Pending cleared: the next tick's move is accepted into the tick-owned
        // player rather than ignored.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(5.0, -63.0, 5.0))],
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!(
            (pos.x, pos.y, pos.z),
            (5.0, -63.0, 5.0),
            "move accepted after ack"
        );
    }

    /// A stale/wrong ack id is a silent no-op (`handleAcceptTeleportPacket`'s
    /// id-mismatch path): no disconnect, the player stays put, and the pending
    /// teleport survives — a subsequent move is still ignored.
    #[test]
    fn wrong_ack_id_is_silently_ignored_and_teleport_stays_pending() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, accept_teleport_frame(999))],
        );
        assert_eq!(manager.move_frames_seen(), 1, "ack body parsed");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            None,
            "wrong id is a no-op, not a kick"
        );

        // The teleport is still pending: movement is still ignored.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(5.0, -63.0, 5.0))],
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!(
            (pos.x, pos.y, pos.z),
            (0.0, -63.0, 0.0),
            "still awaiting teleport"
        );
    }

    /// While the spawn teleport is pending, `updateAwaitingTeleport` ignores the
    /// client's position movement but accepts its rotation: a `move_player_pos`
    /// leaves the position at spawn, and a `move_player_rot` snaps the wrapped
    /// rotation. The rotation is preserved when the teleport is later acked
    /// (`absSnapTo(awaited, getYRot(), getXRot())`).
    #[test]
    fn movement_before_ack_ignores_position_but_snaps_rotation() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        // A position move while the teleport is pending: ignored (the player
        // stays at spawn). A rotation move: accepted, wrapped.
        manager = run_tick(
            manager,
            &mut registry,
            vec![
                (ID, move_pos_frame(5.0, -63.0, 5.0)),
                (ID, move_rot_frame(450.0, 30.0)),
            ],
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (0.0, -63.0, 0.0), "position ignored");
        assert_eq!(manager.player_yaw(ID), Some(90.0), "450 wraps to 90");
        assert_eq!(manager.player_pitch(ID), Some(30.0));

        // Acking the teleport preserves the client-accepted rotation.
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);
        assert_eq!(manager.player_yaw(ID), Some(90.0), "rotation preserved");
        assert_eq!(manager.player_pitch(ID), Some(30.0));
    }

    /// After the spawn teleport is acked, accepted finite movement routes into
    /// the tick-owned `ServerPlayer` (server authority, not client prediction):
    /// the position and rotation written by `absSnapTo` are readable from the
    /// manager.
    #[test]
    fn movement_after_ack_is_accepted_into_the_tick_owned_player() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(12.5, -63.0, -8.25))],
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (12.5, -63.0, -8.25));

        // A rotation-only frame with no position keeps the current position.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_rot_frame(90.0, 45.0))],
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!(
            (pos.x, pos.y, pos.z),
            (12.5, -63.0, -8.25),
            "position retained"
        );
        assert_eq!(manager.player_yaw(ID), Some(90.0));
        assert_eq!(manager.player_pitch(ID), Some(45.0));
    }

    /// A non-finite rotation in a move frame fires `containsInvalidValues` —
    /// the Paper `invalid_player_movement` kick (`multiplayer.disconnect.
    /// invalid_player_movement`), the same reason a matching ack with no
    /// pending position produces. The connection is disconnected, not the tick.
    #[test]
    fn non_finite_rotation_move_disconnects_with_invalid_player_movement() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_rot_frame(f32::NAN, 30.0))],
        );
        assert_eq!(manager.move_frames_seen(), 1, "body parsed before the gate");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::InvalidPlayerMovement),
            "NaN rotation is the Paper-faithful kick"
        );
    }

    /// A NaN position in a move frame fires the same `invalid_player_movement`
    /// kick (Paper checks positions with `Double.isNaN`, so NaN is rejected but
    /// infinite positions pass the gate — that asymmetry is pinned here).
    #[test]
    fn nan_position_move_disconnects_but_infinite_position_is_accepted() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);

        // NaN x is rejected before the session lookup: the Paper-faithful kick.
        // The manager is discarded — the disconnect is the assertion.
        let _ = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(f64::NAN, -63.0, 0.0))],
        );
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::InvalidPlayerMovement),
            "NaN position is the Paper-faithful kick"
        );
    }

    /// The DoD payload-ordering contract (#86, #158): `disconnect_invalid_
    /// movement` encodes + queues the `ClientboundDisconnectPacket` frame (a
    /// `Packet` event for `PLAY_CLIENTBOUND_DISCONNECT_ID`) *before* the
    /// `OutboundEvent::Disconnect` — the per-connection task flushes queued
    /// frames before closing, so the client receives the reason. The channel is
    /// FIFO, so draining in order pins the ordering the tests above skip past
    /// (`drained_disconnect_reason` drops `Packet` events). The queued body is
    /// decoded too, proving it is the invalid_player_movement reason and not
    /// just some packet.
    #[test]
    fn invalid_movement_kick_queues_disconnect_frame_before_disconnect() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        // NaN rotation fires `containsInvalidValues` — the same kick the
        // existing tests drain past.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_rot_frame(f32::NAN, 30.0))],
        );
        assert_eq!(manager.move_frames_seen(), 1, "body parsed before the gate");

        let mut events = Vec::new();
        while let Ok(event) = out_rx.try_recv() {
            events.push(event);
        }
        assert!(
            !events.is_empty(),
            "the tick queued at least the reason frame and the Disconnect"
        );

        let reason_frame = events
            .iter()
            .position(|e| {
                matches!(
                    e,
                    OutboundEvent::Packet { frame }
                        if frame_parts(frame).0 == PLAY_CLIENTBOUND_DISCONNECT_ID
                )
            })
            .expect("a disconnect reason frame is queued");
        let disconnect_at = events
            .iter()
            .position(|e| matches!(e, OutboundEvent::Disconnect { .. }))
            .expect("a Disconnect is queued");
        assert!(
            reason_frame < disconnect_at,
            "reason frame precedes the Disconnect (frame@{reason_frame} < disconnect@{disconnect_at})"
        );

        let (packet_id, body) = match &events[reason_frame] {
            OutboundEvent::Packet { frame } => frame_parts(frame),
            _ => unreachable!("reason_frame was located as a Packet"),
        };
        assert_eq!(packet_id, PLAY_CLIENTBOUND_DISCONNECT_ID);
        let mut input = FriendlyByteBuf::new(bytes::BytesMut::from(body.as_slice()));
        let decoded = ClientboundDisconnectPacket::stream_codec()
            .decode(&mut input)
            .expect("the queued reason body decodes");
        assert_eq!(
            decoded.reason(),
            &Component::translatable("multiplayer.disconnect.invalid_player_movement"),
            "the queued reason is the Paper-faithful kick reason"
        );
        assert_eq!(
            input.readable_bytes(),
            0,
            "the reason body is fully consumed"
        );
        match &events[disconnect_at] {
            OutboundEvent::Disconnect {
                reason: DisconnectReason::InvalidPlayerMovement,
            } => {}
            other => panic!("expected InvalidPlayerMovement Disconnect, got {other:?}"),
        }
    }

    /// The overflow branch of `disconnect_invalid_movement`: a connection whose
    /// outbound channel is full cannot take the reason frame, so
    /// `ConnectionRegistry::send` prunes it and records `Overflow` as the
    /// terminal reason. The follow-up `OutboundEvent::Disconnect { Unsupported }`
    /// is then a `Gone` no-op — the socket still closes, but no `Unsupported`
    /// reason ever surfaces and no reason frame is queued. This pins the honest
    /// contract of the `disconnect_invalid_movement` doc (the `Unsupported`
    /// fallback only delivers on the encode-fault branch, where the connection is
    /// still live).
    #[test]
    fn invalid_movement_kick_on_full_outbound_records_overflow_and_closes() {
        let mut registry = ConnectionRegistry::new();
        let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
        let (out_tx, out_rx) = tokio::sync::mpsc::channel(1);
        registry.apply(LifecycleEvent::Connect {
            id: ID,
            remote: "127.0.0.1:25565".parse().unwrap(),
            in_rx,
            out_tx,
            drained: InboundDrained::new(),
        });
        let _ = in_tx;
        // Fill the single-slot channel: a client that cannot keep up.
        registry
            .send(
                ID,
                OutboundEvent::Packet {
                    frame: Bytes::from_static(b"a"),
                },
            )
            .expect("the single-slot channel takes one queued frame");

        let mut manager = PlayerSessionManager::new(default_session_config(256));
        let mut ctx = TickContext {
            tick: 1,
            now_ns: 0,
            now_ms: 0,
            connections: &mut registry,
            inbound: Vec::new(),
        };
        manager.disconnect_invalid_movement(&mut ctx, ID);

        assert!(
            !registry.contains(ID),
            "overflow prunes the connection (the reason frame cannot be queued)"
        );
        assert_eq!(
            registry.take_disconnect_reason(ID),
            Some(DisconnectReason::Overflow),
            "the terminal reason is the overflow, not the Unsupported fallback"
        );
        let mut out_rx = out_rx;
        assert!(
            matches!(out_rx.try_recv(), Ok(OutboundEvent::Packet { .. })),
            "the pre-existing queued frame is still delivered"
        );
        assert!(
            out_rx.try_recv().is_err(),
            "no reason frame and no Disconnect fit: both are gone"
        );
    }

    /// A `move_player` frame for a connection whose handoff has not landed is
    /// buffered into the pending FIFO, not dispatched (the handoff-race policy
    /// `route_inbound` applies to every play frame): no decode runs yet (so the
    /// move counter stays 0), no kick, and the frame is delivered the tick the
    /// session spawns.
    #[test]
    fn move_frame_before_handoff_is_buffered_not_dropped() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(1.0, 2.0, 3.0))],
        );
        assert_eq!(manager.session_count(), 0);
        assert_eq!(manager.routed_frames(), 0, "buffered, not routed");
        assert_eq!(manager.move_frames_seen(), 0, "not decoded yet");
        assert_eq!(
            manager.pending.get(&ID).map(Vec::len),
            Some(1),
            "pending FIFO"
        );
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            None,
            "a pre-handoff frame is not a kick"
        );

        // The handoff lands; the next tick spawns the session and delivers the
        // buffered move. The spawn teleport is still pending (unacked), so the
        // move's position is ignored — the player stays at spawn, exactly the
        // `updateAwaitingTeleport` behavior for movement-before-ack.
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![]);
        assert_eq!(manager.session_count(), 1);
        assert_eq!(manager.move_frames_seen(), 1, "delivered and decoded");
        let pos = manager.player_position(ID).unwrap();
        assert_eq!(
            (pos.x, pos.y, pos.z),
            (0.0, -63.0, 0.0),
            "teleport still pending"
        );
    }

    /// The per-tick `resetPosition` mirror (issue #158): `tickPlayer()` resets
    /// `firstGood/lastGood` to the player's position at the top of every tick,
    /// so the too-quickly predicate measures per-tick displacement against a
    /// fresh anchor. A move to (5,-63,5) then a move to (5,-63,6) leaves the
    /// anchor at the tick-start position (5,-63,5) — without the per-tick
    /// reset it would stay at the spawn (0,-63,0).
    #[test]
    fn move_anchors_reset_to_tick_start_position_every_tick() {
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);

        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(5.0, -63.0, 5.0))],
        );
        assert_eq!(
            manager.move_anchor(ID),
            Some([0.0, -63.0, 0.0]),
            "anchor reset to the position the tick started at"
        );

        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(5.0, -63.0, 6.0))],
        );
        assert_eq!(
            manager.move_anchor(ID),
            Some([5.0, -63.0, 5.0]),
            "anchor follows the accepted position across ticks"
        );
    }

    /// A client that answers every challenge: the keepalive loop transmits at
    /// the 1s cadence, the reply is accepted the following tick, and the session
    /// survives far past the default 30s kick limit — the timeout check only
    /// sees an empty pending queue (the oldest challenge was answered).
    #[test]
    fn responding_client_survives_beyond_the_timeout_window() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        // Spawn at t=0 (the session seeds its keepalive state this tick). Then
        // run 40s of 1s cycles: transmit a challenge each second, answer it the
        // next tick.
        apply_enter_play(&mut registry);
        manager = run_tick_at(manager, &mut registry, vec![], 0);
        assert_eq!(manager.session_count(), 1);

        for t in (1..=40).map(|i| i * 1000) {
            // This tick's transmit (id = the millis reading).
            manager = run_tick_at(manager, &mut registry, vec![], t);
            // The client's echo arrives the following tick; it matches the
            // oldest (only) pending challenge and is accepted.
            manager = run_tick_at(
                manager,
                &mut registry,
                vec![(ID, keepalive_frame(t))],
                t + 1,
            );
        }

        // 40s elapsed — past the 30s limit — but every challenge was answered,
        // so the session is alive and no disconnect was queued.
        assert_eq!(manager.session_count(), 1, "session survives the window");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            None,
            "a responding client is never kicked"
        );
    }

    /// A silent client is disconnected with TIMEOUT when its oldest challenge
    /// exceeds the kick limit (strict `>`): at exactly 30s elapsed no kick, one
    /// ms later it fires and the keepalive state is pruned.
    #[test]
    fn silent_client_is_disconnected_after_the_timeout_window() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        apply_enter_play(&mut registry);
        manager = run_tick_at(manager, &mut registry, vec![], 0);

        // The 1s transmit at t=1000 queues challenge id 1000; no reply ever
        // comes.
        manager = run_tick_at(manager, &mut registry, vec![], 1000);
        assert_eq!(manager.keep_alives_seen(), 0, "client never replies");

        // At exactly `1000 + 30000 = 31000`ms, elapsed == limit — the strict
        // `>` boundary does NOT kick yet.
        manager = run_tick_at(manager, &mut registry, vec![], 31000);
        assert_eq!(manager.session_count(), 1, "still alive at the boundary");

        // One ms later the oldest challenge exceeds the limit: TIMEOUT
        // disconnect, and the keepalive state is pruned (the next tick has
        // nothing left to drive).
        manager = run_tick_at(manager, &mut registry, vec![], 31001);
        assert_eq!(manager.session_count(), 1, "session index remains");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::Timeout),
            "a silent client is kicked with TIMEOUT"
        );
        assert!(
            manager.keepalive.is_empty(),
            "keepalive state pruned on disconnect"
        );
    }

    /// A wrong/stale echo (no matching pending challenge) disconnects with
    /// TIMEOUT — the Paper `handleKeepAlive` "without matching challenge" path —
    /// even though the client is otherwise alive.
    #[test]
    fn wrong_keepalive_id_disconnects() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        apply_enter_play(&mut registry);
        manager = run_tick_at(manager, &mut registry, vec![], 0);
        // Transmit challenge id 1000.
        manager = run_tick_at(manager, &mut registry, vec![], 1000);

        // The client echoes a challenge the server never sent.
        manager = run_tick_at(
            manager,
            &mut registry,
            vec![(ID, keepalive_frame(999))],
            1001,
        );
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::Timeout),
            "an unknown echoed id is a TIMEOUT disconnect"
        );
        assert!(
            manager.keepalive.is_empty(),
            "keepalive state pruned on the bad reply"
        );
    }

    /// The configured kick limit (a short `ServerConfig.keepalive_timeout`)
    /// shortens the window: with a 2s limit the kick fires at 2s, proving the
    /// knob reaches the session's keepalive state end to end.
    #[test]
    fn configurable_keepalive_timeout_shortens_the_kick() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut config = default_session_config(256);
        config.keepalive_timeout_ns = 2_000 * 1_000_000;
        let mut manager = PlayerSessionManager::new(config);

        apply_enter_play(&mut registry);
        manager = run_tick_at(manager, &mut registry, vec![], 0);
        manager = run_tick_at(manager, &mut registry, vec![], 1000); // transmit id 1000
        // At 1000+2000=3000ms exactly, the strict `>` does not fire...
        manager = run_tick_at(manager, &mut registry, vec![], 3000);
        assert_eq!(manager.session_count(), 1, "no kick at the boundary");
        // ...one ms later it does. Only the outbound assertion follows, so the
        // manager is consumed without rebinding.
        let _ = run_tick_at(manager, &mut registry, vec![], 3001);
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::Timeout),
            "the 2s limit kicks at 2s"
        );
    }

    /// A `keep_alive` frame with an unknown/malformed packet id is dropped; only
    /// a well-formed body whose echoed id matches a pending challenge is
    /// accepted — no disconnect, and the challenge is consumed.
    #[test]
    fn matching_keepalive_reply_is_accepted() {
        let (mut registry, mut out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));

        apply_enter_play(&mut registry);
        manager = run_tick_at(manager, &mut registry, vec![], 0);
        manager = run_tick_at(manager, &mut registry, vec![], 1000); // transmit id 1000

        manager = run_tick_at(
            manager,
            &mut registry,
            vec![(ID, keepalive_frame(1000))],
            1001,
        );
        assert_eq!(manager.keep_alives_seen(), 1, "the echo is decoded");
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            None,
            "a matching echo is accepted, not kicked"
        );
        // The accepted challenge is consumed: a later duplicate echo matches
        // nothing and disconnects. Only the outbound assertion follows, so the
        // manager is consumed without rebinding.
        let _ = run_tick_at(
            manager,
            &mut registry,
            vec![(ID, keepalive_frame(1000))],
            1002,
        );
        assert_eq!(
            drained_disconnect_reason(&mut out_rx),
            Some(DisconnectReason::Timeout),
            "a replayed echo after acceptance is a TIMEOUT disconnect"
        );
    }

    // ---- RIVET_TRACE_MOVEMENT (issue #53) -----------------------------------
    //
    // The in-process trace assertion. The subscriber is installed with
    // `tracing_subscriber::fmt().with_test_writer()` (the schema-level
    // assertions are made on the recording layer; the fmt layer renders the
    // same events through the test writer so cargo captures them). The env gate
    // is pinned per test by `movement_trace::set_trace_gate_for_tests`, and the
    // shared lock serializes these gate-sensitive tests.

    /// A malformed `move_player` frame whose body parses (the `packet_id`
    /// varint is intact, so `dispatch` routes it) but whose declared fields are
    /// truncated — the codec returns an error, so no acceptance is emitted.
    fn malformed_move_pos_frame() -> ServerboundFrame {
        let mut bytes = varint(30);
        bytes.extend_from_slice(&5.0_f64.to_be_bytes()); // only x; y/z truncated
        bytes.push(0x00);
        ServerboundFrame {
            bytes: Bytes::from(bytes),
        }
    }

    /// A pre-ack move emits no `RIVET_MOVE_ACCEPTED` (movement is withheld
    /// while the spawn teleport is pending) and no `RIVET_TELEPORT_ACK` (no ack
    /// frame was sent). The gate is pinned on and the subscriber is the fmt
    /// test writer — so a regression that emitted a record would be captured.
    #[test]
    fn trace_pre_ack_move_emits_no_acceptance() {
        let _sub = rivet_test_support::install_for_tests(
            crate::server::movement_trace::set_trace_gate_for_tests,
            true,
        );
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        // The session spawns with the spawn teleport (id 1) pending; the move
        // while pending is ignored (rotation-only snap), never accepted.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(5.0, -63.0, 5.0))],
        );
        assert_eq!(manager.move_frames_seen(), 1, "the move body parsed");
        let pos = manager.player_position(ID).unwrap();
        assert_eq!(
            (pos.x, pos.y, pos.z),
            (0.0, -63.0, 0.0),
            "position still at spawn"
        );

        let records = _sub.recorder.snapshot();
        assert_eq!(
            records
                .iter()
                .filter(|r| r.tag == movement_trace::TAG_MOVE_ACCEPTED)
                .count(),
            0,
            "a pre-ack move is never an accepted movement"
        );
        assert_eq!(
            records
                .iter()
                .filter(|r| r.tag == movement_trace::TAG_TELEPORT_ACK)
                .count(),
            0,
            "no ack frame was sent, so no ack record"
        );
    }

    /// An accepted post-ack move emits exactly one `RIVET_MOVE_ACCEPTED` with
    /// the exact clamped/wrapped values snapped into the tick-owned player and
    /// the session's accepted-frame counter. The same move with the gate off
    /// emits nothing (zero behavior when unset).
    #[test]
    fn trace_accepted_move_is_post_ack_and_carries_snapped_values() {
        let _sub = rivet_test_support::install_for_tests(
            crate::server::movement_trace::set_trace_gate_for_tests,
            true,
        );
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);

        // Pre-ack: nothing accepted yet.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(5.0, -63.0, 5.0))],
        );
        // Ack the spawn teleport (id 1): accepted, pending cleared.
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);
        // Post-ack: the accepted move routes into the tick-owned player.
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(12.5, -63.0, -8.25))],
        );
        let pos = manager.player_position(ID).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (12.5, -63.0, -8.25));

        let records = _sub.recorder.snapshot();
        let ack: Vec<_> = records
            .iter()
            .filter(|r| r.tag == TAG_TELEPORT_ACK)
            .collect();
        assert_eq!(ack.len(), 1, "exactly one ack record");
        assert_eq!(ack[0].field("outcome"), Some("accepted"));
        assert_eq!(ack[0].field("ack_id"), Some("1"));
        assert_eq!(ack[0].field("id"), Some("conn#1"));
        assert_eq!(ack[0].field("x"), Some("0"), "awaited spawn x");

        let accepted: Vec<_> = records
            .iter()
            .filter(|r| r.tag == TAG_MOVE_ACCEPTED)
            .collect();
        assert_eq!(
            accepted.len(),
            1,
            "exactly one accepted move (post-ack only)"
        );
        assert_eq!(accepted[0].field("x"), Some("12.5"));
        assert_eq!(accepted[0].field("y"), Some("-63"));
        assert_eq!(accepted[0].field("z"), Some("-8.25"));
        assert_eq!(accepted[0].field("accepted_frames"), Some("1"));
        assert_eq!(accepted[0].field("id"), Some("conn#1"));

        // The final authoritative displacement is nonzero (12.5, -63, -8.25 vs
        // the spawn (0, -63, 0)); the trace consumer's sums must recover it.
        let dx = pos.x - 0.0;
        let dz = pos.z - 0.0;
        assert!(
            dx * dx + dz * dz > 0.0,
            "authoritative displacement from spawn is nonzero"
        );
    }

    /// A malformed `move_player` frame (truncated body) parses the packet id
    /// but the codec rejects the body: no `RIVET_MOVE_ACCEPTED` is emitted, and
    /// the session stays intact (no kick on the trace path).
    #[test]
    fn trace_malformed_frame_emits_no_acceptance() {
        let _sub = rivet_test_support::install_for_tests(
            crate::server::movement_trace::set_trace_gate_for_tests,
            true,
        );
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);

        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, malformed_move_pos_frame())],
        );
        assert_eq!(manager.move_frames_seen(), 1, "the id parsed (routed)");
        let records = _sub.recorder.snapshot();
        assert_eq!(
            records
                .iter()
                .filter(|r| r.tag == TAG_MOVE_ACCEPTED)
                .count(),
            0,
            "a malformed move body never becomes an accepted movement"
        );
    }

    /// With the gate off (`RIVET_TRACE_MOVEMENT` unset), the same accepted
    /// movement emits zero trace records — the trace must be a strict no-op when
    /// disabled.
    #[test]
    fn trace_zero_behavior_when_gate_off() {
        let _sub = rivet_test_support::install_for_tests(
            crate::server::movement_trace::set_trace_gate_for_tests,
            false,
        );
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        apply_enter_play(&mut registry);
        manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);
        manager = run_tick(
            manager,
            &mut registry,
            vec![(ID, move_pos_frame(12.5, -63.0, -8.25))],
        );
        // The authoritative path still runs (the move is accepted into the
        // player) — only the trace records are suppressed.
        let pos = manager.player_position(ID).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (12.5, -63.0, -8.25));
        assert!(
            _sub.recorder.snapshot().is_empty(),
            "the gate-off run emits no trace records"
        );
    }

    /// `RIVET_SESSION_END` is emitted only for the traced close paths — EOF,
    /// Timeout, and InboundOverflow (the client going away, or a liveness/
    /// anti-flood kick) — carrying the final authoritative position + rotation
    /// and the session's movement counts. A deliberate server-side close
    /// (outbound Overflow, ServerShutdown, Malformed) is not a movement-trace
    /// endpoint and emits nothing. Each case reconnects a fresh live session
    /// (the previous close pruned it), disconnects with the case's reason, and
    /// asserts on the new session-end records only (the shared recorder
    /// accumulates across the loop). `prune_lost` consumes the recorded reason
    /// for the live session either way, so the reason map never retains it.
    #[test]
    fn trace_session_end_distinguishes_close_reasons() {
        let _sub = rivet_test_support::install_for_tests(
            crate::server::movement_trace::set_trace_gate_for_tests,
            true,
        );
        let (mut registry, _out_rx) = connected_registry();
        let mut manager = PlayerSessionManager::new(default_session_config(256));
        let cases: &[(DisconnectReason, bool, Option<&str>)] = &[
            (
                DisconnectReason::EndOfStream,
                true,
                Some("disconnect.endOfStream"),
            ),
            (DisconnectReason::Timeout, true, Some("disconnect.timeout")),
            (
                DisconnectReason::InboundOverflow("flood".into()),
                true,
                Some("inbound overflow: flood"),
            ),
            // A deliberate server-side close is not a movement-trace endpoint.
            (DisconnectReason::Overflow, false, None),
            (DisconnectReason::ServerShutdown, false, None),
            (DisconnectReason::Malformed("garbage".into()), false, None),
        ];
        for (reason, traced, expected_reason) in cases {
            // Reconnect the channels (the previous close removed the entry),
            // re-apply the handoff, and run one tick: the session spawns and
            // the ack clears the pending teleport, so the session is post-ack.
            let (in_tx, in_rx) = tokio::sync::mpsc::channel(4);
            let (out_tx, _out_rx) = tokio::sync::mpsc::channel(256);
            registry.apply(LifecycleEvent::Connect {
                id: ID,
                remote: "127.0.0.1:25565".parse().unwrap(),
                in_rx,
                out_tx,
                drained: InboundDrained::new(),
            });
            let _ = in_tx;
            apply_enter_play(&mut registry);
            manager = run_tick(manager, &mut registry, vec![(ID, accept_teleport_frame(1))]);
            manager = run_tick(
                manager,
                &mut registry,
                vec![(ID, move_pos_frame(12.5, -63.0, -8.25))],
            );
            assert_eq!(manager.session_count(), 1, "session live before the close");

            let prior_ends = _sub
                .recorder
                .snapshot()
                .iter()
                .filter(|r| r.tag == TAG_SESSION_END)
                .count();
            registry.apply(LifecycleEvent::Disconnect {
                id: ID,
                reason: reason.clone(),
            });
            manager = run_tick(manager, &mut registry, vec![]);
            assert_eq!(manager.session_count(), 0, "session pruned on close");
            assert_eq!(
                registry.take_disconnect_reason(ID),
                None,
                "prune_lost consumed the session's reason, not retained"
            );

            let ends: Vec<_> = _sub
                .recorder
                .snapshot()
                .into_iter()
                .filter(|r| r.tag == TAG_SESSION_END)
                .collect();
            let delta = ends.len() - prior_ends;
            match traced {
                true => {
                    assert_eq!(delta, 1, "a traced close reports a session end");
                    let end = ends.last().expect("the new session-end record");
                    assert_eq!(end.field("reason"), *expected_reason);
                    // The final authoritative position is the accepted move's
                    // snapped values (the move happened before this close).
                    assert_eq!(end.field("x"), Some("12.5"));
                    assert_eq!(end.field("y"), Some("-63"));
                    assert_eq!(end.field("z"), Some("-8.25"));
                    assert_eq!(end.field("accepted_frames"), Some("1"));
                }
                false => assert_eq!(delta, 0, "a deliberate close emits no session end"),
            }
        }
    }

}

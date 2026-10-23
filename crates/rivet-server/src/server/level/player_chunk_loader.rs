//! Port of `ca.spottedleaf.moonrise.patches.chunk_system.player.RegionizedPlayerChunkLoader`
//! (MC 26.2, Paper) — the Moonrise direct chunk-send path (issue #100).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/
//! moonrise/patches/chunk_system/player/RegionizedPlayerChunkLoader.java`
//! (1104 lines).
//!
//! Owned by the `ca.spottedleaf.moonrise.patches.chunk_system.player` manifest
//! unit (MANIFEST line 788). This slice ports the **direct-send half** only —
//! `PlayerChunkLoaderData.add()` + the send-queue drain — reduced to the M1
//! superflat world. Moonrise bypasses vanilla `PlayerChunkSender.sendNextChunks`
//! batching: each chunk is sent as a bare `ClientboundLevelChunkWithLightPacket`
//! (the pinned #153 join capture contains no `ChunkBatchStart`/`ChunkBatchFinished`).
//!
//! **Chunk ordering is a deterministic canonicalization, not Paper's wire
//! order.** Paper's raw receive order is the `sendQueue` heap drain (squared
//! distance from the cache center), whose equal-distance tie-break depends on
//! chunk-load timing and differs across boots. `rivet-capture` therefore
//! excludes chunk order from the parity contract (`ordering.rs`:
//! "chunk order is outside the parity contract") and `canonicalize` sorts the
//! capture's chunk packets by coordinate (`normalize.rs`). This port emits that
//! deterministic X-major/Z-minor coordinate raster via
//! `ChunkTrackingView.for_each` (corners skipped) — the order the canonical
//! fixture byte-matches. A faithful port of the `sendQueue` distance order is
//! deliberately not attempted: it is non-deterministic across Paper boots.
//!
//! The chunk *pipeline* (tickets, distance maps, `SingleUserAreaMap` load/tick
//! tracking, the rate limiters, the per-stage queues) is deferred with the
//! owning unit's remaining scope (#185). On the legacy superflat world the send
//! path resolves each view chunk's content directly (one loaded spawn chunk;
//! every other view position has byte-identical deterministic superflat content
//! — the #194 fixture proves all 117 bodies differ only in the 8-byte
//! coordinate header). On a region-backed world (#185) a view chunk outside the
//! boot-time square is loaded on demand from the read-only Anvil region source
//! (`ServerLevel::load_chunk_from_region`) and installed into the `ChunkMap`
//! before encoding — see [`encode_chunk_with_light`].
//!
//! Ownership per OWNERSHIP §Network: this runs on the tick thread and produces
//! play-state packets for a connection's bounded outbound channel. The packets
//! are plain values (`id` + body); the caller frames them for the wire and
//! queues them (`ConnectionRegistry::send`). Compression stays per-connection:
//! the tick thread frames with the config threshold the M1 login applies to
//! every connection, exactly as `Connection::send_packet` does on the tokio
//! side (the #96 per-connection refinement is a `RivetTodo`).
//!
//! The `placeNewPlayer` join burst calls this per-player chunk send-set once at
//! spawn (see `join.rs`), then `update` on each chunk-boundary crossing or
//! view-distance change. `drain_ready` runs independently each tick, retaining
//! generation-pending cells without sending or fabricating content. Departed
//! pending requests are canceled; only departed delivered cells send unloads.
//! Rate limiting and the full distance-map/ticket pipeline remain deferred.

use bytes::{Bytes, BytesMut};
use std::collections::HashSet;

use rivet_protocol::codec::StreamEncoder;
use rivet_protocol::compression_encoder::CompressionEncoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets::play::clientbound::PacketType as PlayClientbound;
use rivet_protocol::protocol::game::clientbound_level_chunk_with_light::ClientboundLevelChunkWithLightPacket;
use rivet_protocol::protocol::game::clientbound_set_chunk_cache_center::ClientboundSetChunkCacheCenterPacket;
use rivet_protocol::protocol::game::clientbound_set_chunk_cache_radius::ClientboundSetChunkCacheRadiusPacket;
use rivet_protocol::protocol::game::clientbound_set_simulation_distance::ClientboundSetSimulationDistancePacket;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_protocol::var_int;
use rivet_protocol::varint21_length_field_prepender::encode_frame;
use rivet_registry::core::ChunkPos;
use rivet_registry::registries::BlockEntityType;

use super::chunk_tracking_view::ChunkTrackingView;
use super::generated_chunk_owner::GenerationConsumerId;
use super::server_level::MissingChunkPolicy;
use super::server_level::ServerLevel;

// `ServerChunkCache.setSendViewDistance` + `ChunkMap.setServerViewDistance` map
// to Moonrise `setSendDistance(viewDistance)` / `setLoadDistance(viewDistance + 1)`
// (the world half of the `ViewDistances` record). The M1 world pins these to
// the `view-distance=4` fixture; the per-player overrides are absent (values
// `-1`), so the Moonrise distance ladder below collapses to
// `tick = min(simulation, load - 1)`, `load = view + 1`, `send = view`.
//
// The `±1` steps wrap like Java ints (PORTING.md: all arithmetic wrapping
// unless proven range-safe): the `-1`/`+1` in the Java ladder are int
// additions/subtractions, so `wrapping_sub`/`wrapping_add` preserve the wrap at
// i32 extremes. The reachable M1 inputs (world distances in `[2, 32]`, the -1
// sentinels) never approach the extremes, so the wrap is unreachable today.

/// `PlatformHooks.configAutoConfigSendDistance()` — the Paper default
/// `auto-config-send-distance` is true; Rivet has no config knob yet, so the
/// `sendViewDistance` ladder resolves the world's send distance when the client
/// requests none. RivetTodo(#236): the Paper config knob.
const AUTO_CONFIG_SEND_DISTANCE: bool = true;

/// `RegionizedPlayerChunkLoader.getClientViewDistance(player)` —
/// `player.requestedViewDistance()`, `null` → -1 (the "no request" sentinel the
/// ladder resolves via auto-config). The live Slice B caller
/// (`PlayerSessionManager::spawn_session` →
/// [`place_new_player`](crate::server::player::join::place_new_player)) passes
/// `Some(client_information.view_distance())`; the `None` auto-config path
/// remains for the unit/`join_burst` callers that hand no client information.
pub fn get_client_view_distance(requested_view_distance: Option<i32>) -> i32 {
    requested_view_distance.map(|vd| vd.max(0)).unwrap_or(-1)
}

/// `PlayerChunkLoaderData.getTickDistance` — `min(playerTick < 0 ? worldTick :
/// playerTick, playerLoad < 0 ? worldLoad - 1 : playerLoad - 1)`.
pub fn get_tick_distance(
    player_tick_view_distance: i32,
    world_tick_view_distance: i32,
    player_load_view_distance: i32,
    world_load_view_distance: i32,
) -> i32 {
    (if player_tick_view_distance < 0 {
        world_tick_view_distance
    } else {
        player_tick_view_distance
    })
    .min(if player_load_view_distance < 0 {
        world_load_view_distance.wrapping_sub(1)
    } else {
        player_load_view_distance.wrapping_sub(1)
    })
}

/// `PlayerChunkLoaderData.getLoadViewDistance` — `max(tickViewDistance + 1,
/// playerLoad < 0 ? worldLoad : playerLoad)`.
pub fn get_load_view_distance(
    tick_view_distance: i32,
    player_load_view_distance: i32,
    world_load_view_distance: i32,
) -> i32 {
    tick_view_distance
        .wrapping_add(1)
        .max(if player_load_view_distance < 0 {
            world_load_view_distance
        } else {
            player_load_view_distance
        })
}

/// `PlayerChunkLoaderData.getSendViewDistance` — `min(load - 1, playerSend < 0
/// ? (!autoConfigSendDistance || client < 0 ? (worldSend < 0 ? load - 1 :
/// worldSend) : client + 1) : playerSend)`.
pub fn get_send_view_distance(
    load_view_distance: i32,
    client_view_distance: i32,
    player_send_view_distance: i32,
    world_send_view_distance: i32,
) -> i32 {
    let inner = if player_send_view_distance < 0 {
        if !AUTO_CONFIG_SEND_DISTANCE || client_view_distance < 0 {
            if world_send_view_distance < 0 {
                load_view_distance.wrapping_sub(1)
            } else {
                world_send_view_distance
            }
        } else {
            client_view_distance.wrapping_add(1)
        }
    } else {
        player_send_view_distance
    };
    load_view_distance.wrapping_sub(1).min(inner)
}

/// The `PlayerChunkLoaderData` distance ladder, derived from the world's
/// Moonrise distances with the per-player overrides absent (`-1`, resolved to
/// the world defaults): `getTickDistance` → `getLoadViewDistance` →
/// `getSendViewDistance`. Returns `(tick, load, send)`.
///
/// Both `add` (the initial send-set) and `update` (the re-center) derive the
/// same ladder. The world-pinned part is constant on the M1 world
/// (`tick = 4`, `load = 5`); `send = min(load - 1, client + 1)` also depends
/// on the per-player `requested_view_distance`, so it resolves `4` when the
/// request is `None` or `≥ 3` and lower (`1..3`) for smaller requests.
fn derive_distances(world: &ServerLevel, requested_view_distance: Option<i32>) -> (i32, i32, i32) {
    let tick = get_tick_distance(
        -1,
        world.get_simulation_distance(),
        -1,
        world.load_view_distance(),
    );
    let load = get_load_view_distance(tick, -1, world.load_view_distance());
    let client = get_client_view_distance(requested_view_distance);
    let send = get_send_view_distance(load, client, -1, world.send_view_distance());
    debug_assert!(
        send <= load && tick <= load,
        "send/tick view cannot exceed load view"
    );
    (tick, load, send)
}

/// One tick-thread-produced play-state packet: the play clientbound packet id
/// plus the encoded body (packet id NOT included — the caller frames it). Plain
/// owned values so the tick thread can queue them without holding game state
/// (OWNERSHIP §Network).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayPacket {
    /// The play clientbound packet id (`varint` on the wire).
    pub id: u32,
    /// The encoded packet body.
    pub body: Vec<u8>,
}

/// The world may have an in-flight generation request without a canonical
/// FULL chunk. That state is normal and must remain distinct from failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkSendReadiness {
    Ready,
    Pending,
}

impl PlayPacket {
    /// `new Packet(id, body)`.
    pub fn new(id: u32, body: Vec<u8>) -> Self {
        PlayPacket { id, body }
    }
}

/// Per-player desired, pending and delivered chunk state. One optional
/// generation consumer owns this player's requests until movement or removal;
/// ready disk/fixture worlds retain their synchronous direct-send behavior.
pub struct PlayerChunkLoader {
    /// Canonical raster of the current view, independent of readiness.
    desired: Vec<ChunkPos>,
    /// Desired cells not yet delivered; a pending cell never counts as sent.
    pending: Vec<ChunkPos>,
    sent: HashSet<ChunkPos>,
    generation_consumer: Option<GenerationConsumerId>,
    requests_dirty: bool,
    /// `this.lastChunkX` / `lastChunkZ` — the player's chunk position (the
    /// `ClientboundSetChunkCacheCenterPacket` center).
    last_chunk_x: i32,
    last_chunk_z: i32,
    /// `this.lastSendDistance` — the cache radius sent to the client.
    last_send_distance: i32,
    /// `this.lastTickDistance` — the simulation distance sent to the client.
    last_tick_distance: i32,
    /// Test seam (issue #100): forces a view-chunk encode failure in
    /// `add_and_send_chunks` (compiled out of non-test builds).
    #[cfg(test)]
    fail_chunk_encoding: bool,
}

impl PlayerChunkLoader {
    /// `new PlayerChunkLoaderData(world, player)` for a player at `center`.
    pub fn new(center: ChunkPos) -> Self {
        PlayerChunkLoader {
            desired: Vec::new(),
            pending: Vec::new(),
            sent: HashSet::new(),
            generation_consumer: None,
            requests_dirty: false,
            last_chunk_x: center.x(),
            last_chunk_z: center.z(),
            last_send_distance: i32::MIN,
            last_tick_distance: i32::MIN,
            #[cfg(test)]
            fail_chunk_encoding: false,
        }
    }

    /// Desired cells are visible to the owner even while none is ready.
    pub fn desired_chunks(&self) -> &[ChunkPos] {
        &self.desired
    }

    /// Actual completed chunk delivery, not a desired/pending view position.
    pub fn has_sent(&self, pos: ChunkPos) -> bool {
        self.sent.contains(&pos)
    }

    fn replace_desired(&mut self, view: &ChunkTrackingView) -> (Vec<PlayPacket>, Vec<PlayPacket>) {
        let mut desired = Vec::with_capacity(view.chunk_count());
        view.for_each(|pos| desired.push(pos));
        let next: HashSet<_> = desired.iter().copied().collect();
        let mut unloads = Vec::new();
        let mut corner_unloads = Vec::new();
        for pos in &self.desired {
            if !next.contains(pos) && self.sent.remove(pos) {
                let mut body = FriendlyByteBuf::new(BytesMut::new());
                body.write_chunk_pos(pos);
                let packet = PlayPacket::new(
                    PlayClientbound::ForgetLevelChunk.id(),
                    body.into_inner().to_vec(),
                );
                let radius = view.view_distance().wrapping_add(1);
                if pos.x().wrapping_sub(view.center().x()).wrapping_abs() <= radius
                    && pos.z().wrapping_sub(view.center().z()).wrapping_abs() <= radius
                {
                    corner_unloads.push(packet);
                } else {
                    unloads.push(packet);
                }
            }
        }
        self.pending = desired
            .iter()
            .copied()
            .filter(|pos| !self.sent.contains(pos))
            .collect();
        self.requests_dirty |= self.desired != desired;
        self.desired = desired;
        (unloads, corner_unloads)
    }

    fn sync_generation_requests(&mut self, world: &mut ServerLevel) -> Result<(), String> {
        let Some(owner) = world.generated_chunk_owner_mut() else {
            return Ok(());
        };
        let consumer = match self.generation_consumer {
            Some(consumer) => consumer,
            None => {
                let consumer = owner.new_consumer();
                self.generation_consumer = Some(consumer);
                self.requests_dirty = true;
                consumer
            }
        };
        if !self.requests_dirty {
            return Ok(());
        }
        if !owner.replace_requests(
            consumer,
            self.desired
                .iter()
                .copied()
                .map(|pos| (pos, rivet_world::chunk::status::ChunkStatus::Full)),
        ) {
            return Err("chunk generation consumer is not registered with this world".into());
        }
        self.requests_dirty = false;
        Ok(())
    }

    /// Poll every tick, even when the player has not crossed a chunk boundary.
    /// Readiness is resolved before encoding: Pending is neither an encoder
    /// failure nor a delivered chunk. Disk worlds retain synchronous loading.
    pub fn drain_ready(&mut self, world: &mut ServerLevel) -> Result<Vec<PlayPacket>, String> {
        self.sync_generation_requests(world)?;
        self.drain_ready_with(|pos| match world.prepare_chunk_for_send(pos)? {
            ChunkSendReadiness::Pending => Ok(None),
            ChunkSendReadiness::Ready => encode_chunk_with_light(pos, world).map(Some),
        })
    }

    fn drain_ready_with(
        &mut self,
        mut encode: impl FnMut(ChunkPos) -> Result<Option<Vec<u8>>, String>,
    ) -> Result<Vec<PlayPacket>, String> {
        // Preserve the synchronous disk path's eager resolution: all entered
        // cells are attempted even if one is corrupt. No partial packet set is
        // returned and no sent state is committed when any encode failed.
        let attempts: Vec<_> = self
            .pending
            .iter()
            .map(|&pos| encode(pos).map(|body| body.map(|body| (pos, body))))
            .collect();
        let ready = attempts.into_iter().collect::<Result<Vec<_>, _>>()?;
        #[cfg(test)]
        if self.fail_chunk_encoding {
            return Err("forced view-chunk encode failure (test seam)".into());
        }
        // Commit only after the complete encode pass succeeds. A returned Err
        // gives the caller no packets, so none may be recorded as delivered.
        let mut packets = Vec::with_capacity(ready.len());
        for (pos, body) in ready.into_iter().flatten() {
            self.sent.insert(pos);
            packets.push(PlayPacket::new(
                PlayClientbound::LevelChunkWithLight.id(),
                body,
            ));
        }
        self.pending.retain(|pos| !self.sent.contains(pos));
        Ok(packets)
    }

    /// Release only this player's demand. Shared generation demand from other
    /// consumers remains owned by the world scheduler.
    pub fn remove(&mut self, world: &mut ServerLevel) {
        if let Some(consumer) = self.generation_consumer.take()
            && let Some(owner) = world.generated_chunk_owner_mut()
        {
            owner.release_consumer(consumer);
        }
        self.desired.clear();
        self.pending.clear();
        self.sent.clear();
        self.requests_dirty = false;
        self.last_send_distance = i32::MIN;
        self.last_tick_distance = i32::MIN;
    }

    /// `PlayerChunkLoaderData.add()` + the send-queue drain, in one M1 step:
    /// emit cache packets (radius → simulation distance → center), then ready
    /// bare chunk packets in canonical order. The synchronous fixture emits all
    /// 117 cells; generated worlds retain unavailable cells as pending.
    ///
    /// Like Java's `add()`, `lastSendDistance`/`lastTickDistance` are committed
    /// right after the three cache packets and before the chunk drain — they
    /// record the distances sent in those packets, not the chunk set's delivery.
    ///
    /// `requested_view_distance` is `ServerPlayer.requestedViewDistance()`
    /// (issue #101). The live Slice B caller passes
    /// `Some(ClientInformation.view_distance)` (the capture client requests 8,
    /// which the ladder caps at `load - 1` → send 4, the 117-chunk M1 send-set).
    /// The unit/`join_burst` callers pass `None`, which the auto-config ladder
    /// resolves to the world's own send distance (also 4 on the M1 world).
    pub fn add_and_send_chunks(
        &mut self,
        world: &mut ServerLevel,
        requested_view_distance: Option<i32>,
    ) -> Result<Vec<PlayPacket>, String> {
        let (tick, _, send) = derive_distances(world, requested_view_distance);
        let mut packets = vec![
            PlayPacket::new(
                PlayClientbound::SetChunkCacheRadius.id(),
                encode_body(
                    ClientboundSetChunkCacheRadiusPacket::stream_codec(),
                    &ClientboundSetChunkCacheRadiusPacket::new(send),
                )?,
            ),
            PlayPacket::new(
                PlayClientbound::SetSimulationDistance.id(),
                encode_body(
                    ClientboundSetSimulationDistancePacket::stream_codec(),
                    &ClientboundSetSimulationDistancePacket::new(tick),
                )?,
            ),
            PlayPacket::new(
                PlayClientbound::SetChunkCacheCenter.id(),
                encode_body(
                    ClientboundSetChunkCacheCenterPacket::stream_codec(),
                    &ClientboundSetChunkCacheCenterPacket::new(
                        self.last_chunk_x,
                        self.last_chunk_z,
                    ),
                )?,
            ),
        ];
        self.last_send_distance = send;
        self.last_tick_distance = tick;
        let view = ChunkTrackingView::of(self.last_chunk_pos(), send);
        let (unloads, corner_unloads) = self.replace_desired(&view);
        packets.extend(unloads);
        packets.extend(corner_unloads);
        packets.extend(self.drain_ready(world)?);
        Ok(packets)
    }

    /// Reconcile the desired view, unload departed delivered cells, then emit
    /// radius/simulation/center controls and drain ready chunks. The unchanged
    /// view path still drains readiness: generation completion does not depend
    /// on another movement packet. Canonical chunk order remains X-major/Z-minor.
    /// Missing/corrupt disk chunks remain errors; generation Pending does not.
    pub fn update(
        &mut self,
        world: &mut ServerLevel,
        player_chunk: ChunkPos,
        requested_view_distance: Option<i32>,
    ) -> Result<Vec<PlayPacket>, String> {
        let (tick, _, send) = derive_distances(world, requested_view_distance);
        let center_changed = self.last_chunk_pos() != player_chunk;
        if !center_changed && self.last_send_distance == send && self.last_tick_distance == tick {
            return self.drain_ready(world);
        }
        let next_view = ChunkTrackingView::of(player_chunk, send);
        // Paper's square area-map unloads precede radius/simulation controls;
        // remaining corner-shave unloads follow those controls but precede the
        // center packet. Pending departures need no client unload at all.
        let (mut packets, corner_unloads) = self.replace_desired(&next_view);
        if self.last_send_distance != send {
            packets.push(PlayPacket::new(
                PlayClientbound::SetChunkCacheRadius.id(),
                encode_body(
                    ClientboundSetChunkCacheRadiusPacket::stream_codec(),
                    &ClientboundSetChunkCacheRadiusPacket::new(send),
                )?,
            ));
        }
        if self.last_tick_distance != tick {
            packets.push(PlayPacket::new(
                PlayClientbound::SetSimulationDistance.id(),
                encode_body(
                    ClientboundSetSimulationDistancePacket::stream_codec(),
                    &ClientboundSetSimulationDistancePacket::new(tick),
                )?,
            ));
        }
        packets.extend(corner_unloads);
        self.last_chunk_x = player_chunk.x();
        self.last_chunk_z = player_chunk.z();
        self.last_send_distance = send;
        self.last_tick_distance = tick;
        if center_changed {
            packets.push(PlayPacket::new(
                PlayClientbound::SetChunkCacheCenter.id(),
                encode_body(
                    ClientboundSetChunkCacheCenterPacket::stream_codec(),
                    &ClientboundSetChunkCacheCenterPacket::new(player_chunk.x(), player_chunk.z()),
                )?,
            ));
        }
        packets.extend(self.drain_ready(world)?);
        Ok(packets)
    }

    /// `PlayerChunkLoaderData.lastChunkX/lastChunkZ` — the cache center the
    /// last `add`/`update` emitted (the chunk the player's view is centered
    /// on). Test/observability seam for the movement-driven recenter (issue
    /// #521).
    pub fn last_chunk_pos(&self) -> ChunkPos {
        ChunkPos::new(self.last_chunk_x, self.last_chunk_z)
    }

    /// `PlayerChunkLoaderData.lastSendDistance` — the cache radius emitted by
    /// the last `add`/`update`.
    pub fn last_send_distance(&self) -> i32 {
        self.last_send_distance
    }

    /// `PlayerChunkLoaderData.lastTickDistance` — the simulation distance
    /// emitted by the last `add`/`update`.
    pub fn last_tick_distance(&self) -> i32 {
        self.last_tick_distance
    }

    /// Test seam (issue #100): force the view-chunk encode step of the next
    /// `add_and_send_chunks` call to fail. Compiles out of non-test builds.
    #[cfg(test)]
    fn set_fail_chunk_encoding(&mut self, fail: bool) {
        self.fail_chunk_encoding = fail;
    }
}

/// `PlayerChunkSender.sendChunk` — the bare `ClientboundLevelChunkWithLightPacket`
/// body for one view chunk.
///
/// **Resolution order.** A chunk already in the `ChunkMap` (the boot-time view
/// square, or a position an earlier recenter loaded) encodes directly. A
/// missing chunk resolves on demand when the world is region-backed (issue
/// #185): `load_chunk_from_region` reads + preflights + reconstructs + bridges
/// the existing current-version chunk from the read-only Anvil region and
/// installs it into the `ChunkMap` — the single authority, never a duplicate
/// cache — then the installed chunk encodes. A missing or corrupt chunk fails
/// typed with the source's `RegionBackedBootError` (wrapped in the `UNVERIFIED`
/// string the send path surfaces), never generation and never a superflat
/// fallback. The legacy superflat world (`RepeatSpawnFixture`, no region
/// source) keeps its deterministic content: any view position resolves the
/// spawn chunk's position-independent superflat build (the #194 fixture proves
/// all 117 bodies differ only in the 8-byte coordinate header).
///
/// The light is the deterministic superflat light (`#184`): Java queries the
/// `LevelLightEngine`; the engine is not ported, so every chunk carries the
/// fixed superflat sky/block layers the golden fixture pins. The light payload
/// is precomputed once at `LevelChunk` construction and cloned here, so a
/// per-chunk per-player encode never rebuilds the 26 layer arrays. The chunk-data
/// half is derived per call: the block-entity list is materialized from the
/// current pending authority (#537) through the merged #520 pure materializer
/// (see `LevelChunk::chunk_packet_data`), so the packet reflects mutations made
/// since construction rather than a construction-time snapshot.
fn encode_chunk_with_light(pos: ChunkPos, world: &mut ServerLevel) -> Result<Vec<u8>, String> {
    if world.chunk_map().get_chunk(pos).is_none() {
        match world.missing_chunk_policy() {
            MissingChunkPolicy::RequireLoaded => {
                // The world carries its read-only region source (issue #185):
                // resolve the beyond-view position on demand. Existing-only
                // read + reconstruction, synchronous on the tick thread — no
                // generation, no superflat fallback, no write. Missing/corrupt
                // stay typed UNVERIFIED.
                if world.is_region_backed() {
                    world
                        .load_chunk_from_region(pos)
                        .map_err(|e| e.to_string())?;
                } else {
                    return Err(format!(
                        "UNVERIFIED region-backed chunk {pos} is not loaded; generation and superflat fallback are disabled"
                    ));
                }
            }
            MissingChunkPolicy::RepeatSpawnFixture => {
                // Legacy superflat: no on-demand load, no region source; the
                // spawn chunk's position-independent content stands in for any
                // view position. The chunk is already guaranteed loaded.
            }
        }
    }
    let chunk = match world.chunk_map().get_chunk(pos) {
        Some(chunk) => chunk,
        None => match world.missing_chunk_policy() {
            MissingChunkPolicy::RequireLoaded => {
                return Err(format!(
                    "UNVERIFIED region-backed chunk {pos} is not loaded; generation and superflat fallback are disabled"
                ));
            }
            MissingChunkPolicy::RepeatSpawnFixture => world
                .chunk_map()
                .get_chunk(world.view().center())
                .expect("spawn chunk loaded"),
        },
    };
    let packet = ClientboundLevelChunkWithLightPacket::new(
        pos.x(),
        pos.z(),
        chunk.chunk_packet_data(),
        chunk.light_data(),
    );
    encode_chunk_body(&packet)
}

/// Encode a plain-`FriendlyByteBuf` packet body (the cache packets; packet id
/// NOT included).
fn encode_body<T>(
    codec: impl StreamEncoder<FriendlyByteBuf, T>,
    value: &T,
) -> Result<Vec<u8>, String> {
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    codec
        .encode(&mut out, value)
        .map_err(|e| format!("encoding play packet: {}", e.message))?;
    Ok(out.into_inner().to_vec())
}

/// Encode a `ClientboundLevelChunkWithLightPacket` body over the
/// registry-aware buffer. The block-entity list and this codec context resolve
/// through the same canonical built-in `BLOCK_ENTITY_TYPE` registry instance.
fn encode_chunk_body(packet: &ClientboundLevelChunkWithLightPacket) -> Result<Vec<u8>, String> {
    let mut out =
        RegistryFriendlyByteBuf::new(BytesMut::new(), BlockEntityType::built_in_registry_access());
    ClientboundLevelChunkWithLightPacket::stream_codec()
        .encode(&mut out, packet)
        .map_err(|e| format!("encoding level_chunk_with_light: {}", e.message))?;
    Ok(out.into_inner().to_vec())
}

/// Frame one play packet for the wire: `varint21(varint(declaredLen) ++
/// payload)` where `payload = varint(id) ++ body`. When `compression_threshold
/// >= 0` the payload is run through the protocol `CompressionEncoder` (zlib for
/// payloads at/over the threshold, `varint(0) ++ payload` below) — the exact
/// framing `Connection::send_packet` produces on the tokio side, so a frame
/// queued here writes byte-identically to a pre-play `send_packet` frame.
///
/// RivetTodo(#96): the per-connection threshold lives in the config for the M1
/// world (login applies `config.compression_threshold` to every connection);
/// the per-player refinement is deferred.
pub fn encode_play_frame(packet: &PlayPacket, compression_threshold: i32) -> Result<Bytes, String> {
    let mut payload = Vec::with_capacity(5 + packet.body.len());
    var_int::write(&mut payload, packet.id as i32);
    payload.extend_from_slice(&packet.body);
    let wire = if compression_threshold >= 0 {
        let mut encoder = CompressionEncoder::new(compression_threshold);
        encoder
            .encode(&payload)
            .map_err(|e| format!("compressing play packet: {}", e.message))?
    } else {
        BytesMut::from(&payload[..])
    };
    let frame = encode_frame(&wire).map_err(|e| e.message)?;
    Ok(Bytes::from(frame.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_protocol::codec::StreamDecoder;
    use rivet_protocol::compression_decoder::CompressionDecoder;
    use rivet_protocol::protocol::game::level_chunk_packet_data::{
        BlockEntityInfo, LevelChunkPacketData,
    };
    use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
    use std::sync::Arc;

    /// The #194 capture fixture's first `level_chunk_with_light` body (coords
    /// -5/-4). All 117 bodies in the capture are byte-identical apart from the
    /// 8-byte BE coordinate header.
    const GOLDEN_FULL: &str =
        include_str!("../../../../rivet-protocol/tests/fixtures/chunk_golden_full.hex");

    fn hex(s: &str) -> Vec<u8> {
        let trimmed: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
        (0..trimmed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).unwrap())
            .collect()
    }

    fn overworld() -> ServerLevel {
        ServerLevel::new(super::super::server_level::ServerLevelConfig::default())
    }

    #[test]
    fn region_backed_policy_rejects_missing_chunk_without_spawn_fallback() {
        let mut world = ServerLevel::new(super::super::server_level::ServerLevelConfig {
            missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
            ..Default::default()
        });
        let error = encode_chunk_with_light(ChunkPos::new(1, 0), &mut world).unwrap_err();
        assert!(error.contains("UNVERIFIED region-backed chunk"));
        assert!(error.contains("fallback are disabled"));
    }

    /// The expected M1 send-set order: the `-5..5` × `-5..5` raster skipping
    /// the four corners (X-major, Z-minor) — the deterministic coordinate sort
    /// `rivet-capture` canonicalizes the fixture to (`normalize.rs`), not
    /// Paper's timing-dependent wire order.
    fn expected_raster() -> Vec<ChunkPos> {
        let mut out = Vec::new();
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                if x.abs() == 5 && z.abs() == 5 {
                    continue;
                }
                out.push(ChunkPos::new(x, z));
            }
        }
        out
    }

    fn packet_with_block_entity(
        entity_type: Arc<BlockEntityType>,
    ) -> ClientboundLevelChunkWithLightPacket {
        let world = overworld();
        let chunk = world
            .chunk_map()
            .get_chunk(world.view().center())
            .expect("spawn chunk loaded");
        let chunk_data = LevelChunkPacketData::new(
            chunk.client_heightmaps(),
            chunk.sections_buffer(),
            vec![BlockEntityInfo::new(0x57, -64, entity_type, None)],
        );
        ClientboundLevelChunkWithLightPacket::new(0, 0, chunk_data, chunk.light_data())
    }

    #[test]
    fn production_chunk_encoder_handles_a_registered_block_entity_end_to_end() {
        let furnace = BlockEntityType::from_name("minecraft:furnace").unwrap();
        let packet = packet_with_block_entity(furnace.clone());

        let bytes = encode_chunk_body(&packet).expect("production chunk body encodes");
        let access = BlockEntityType::built_in_registry_access();
        let mut input = RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), access);
        let decoded = ClientboundLevelChunkWithLightPacket::stream_codec()
            .decode(&mut input)
            .expect("encoded production packet decodes");

        assert_eq!(input.readable_bytes(), 0);
        let infos = decoded.chunk_data().block_entities();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].packed_xz(), 0x57);
        assert_eq!(infos[0].y(), -64);
        assert!(Arc::ptr_eq(infos[0].entity_type(), &furnace));
    }

    #[test]
    fn distance_ladder_matches_the_m1_fixture() {
        // view-distance=4, simulation-distance=4: world (tick=4, load=5, send=4).
        let world = overworld();
        let tick = get_tick_distance(
            -1,
            world.get_simulation_distance(),
            -1,
            world.load_view_distance(),
        );
        assert_eq!(tick, 4);
        let load = get_load_view_distance(tick, -1, world.load_view_distance());
        assert_eq!(load, 5);
        // No client request (null -> -1): auto-config resolves world send = 4.
        let send = get_send_view_distance(load, -1, -1, world.send_view_distance());
        assert_eq!(send, 4);
        // The capture client requests 8 (`client_information` view distance), but
        // the ladder caps it at load - 1 = 4 — the M1 send-set is unchanged.
        let send = get_send_view_distance(load, 8, -1, world.send_view_distance());
        assert_eq!(send, 4);
        // A client requesting its own view distance is capped by load - 1.
        let send = get_send_view_distance(load, 6, -1, world.send_view_distance());
        assert_eq!(send, 4);
    }

    #[test]
    fn m1_send_set_is_117_chunks_in_deterministic_raster_order() {
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
        assert_eq!(packets.len(), 3 + 117);
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);

        let chunk_packets: Vec<&PlayPacket> = packets[3..].iter().collect();
        let coords: Vec<ChunkPos> = chunk_packets
            .iter()
            .map(|p| {
                // The chunk body starts with the BE i32 x/z header.
                let x = i32::from_be_bytes([p.body[0], p.body[1], p.body[2], p.body[3]]);
                let z = i32::from_be_bytes([p.body[4], p.body[5], p.body[6], p.body[7]]);
                ChunkPos::new(x, z)
            })
            .collect();
        assert_eq!(
            coords,
            expected_raster(),
            "send order is the deterministic coordinate raster the fixture canonicalizes to"
        );
        // The shape: every (-5..5, -5..5) cell except the four corners.
        let set: std::collections::HashSet<ChunkPos> = coords.iter().copied().collect();
        assert_eq!(set.len(), 117, "no duplicate chunks");
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                let corner = x.abs() == 5 && z.abs() == 5;
                assert_eq!(set.contains(&ChunkPos::new(x, z)), !corner, "({x},{z})");
            }
        }
    }

    #[test]
    fn cache_packets_precede_chunks_in_moonrise_order() {
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
        // radius, then simulation distance, then center — the Moonrise `add`
        // source order, before any chunk.
        assert_eq!(packets[0].id, PlayClientbound::SetChunkCacheRadius.id());
        assert_eq!(packets[0].body, vec![0x04]); // radius 4
        assert_eq!(packets[1].id, PlayClientbound::SetSimulationDistance.id());
        assert_eq!(packets[1].body, vec![0x04]); // simulation distance 4
        assert_eq!(packets[2].id, PlayClientbound::SetChunkCacheCenter.id());
        assert_eq!(packets[2].body, vec![0x00, 0x00]); // center (0, 0)
        for p in &packets[3..] {
            assert_eq!(p.id, PlayClientbound::LevelChunkWithLight.id());
        }
    }

    #[test]
    fn every_chunk_body_is_byte_identical_to_the_fixture_apart_from_coords() {
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
        let golden = hex(GOLDEN_FULL);
        // The first send chunk is (-5,-4) — the fixture's coordinates.
        assert_eq!(
            packets[3].body, golden,
            "first body matches the fixture exactly"
        );
        // Every other body matches apart from the 8-byte BE coordinate header.
        for p in &packets[3..] {
            assert_eq!(p.body[8..], golden[8..], "body matches the fixture");
            let x = i32::from_be_bytes([p.body[0], p.body[1], p.body[2], p.body[3]]);
            let z = i32::from_be_bytes([p.body[4], p.body[5], p.body[6], p.body[7]]);
            assert_eq!(
                &p.body[..8],
                &[x.to_be_bytes(), z.to_be_bytes()].concat(),
                "coordinate header is the BE x/z"
            );
        }
    }

    #[test]
    fn frames_round_trip_through_the_wire_framing() {
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
        // Frame every packet at the fixture's compression threshold 256.
        let frames: Vec<Bytes> = packets
            .iter()
            .map(|p| encode_play_frame(p, 256).unwrap())
            .collect();

        // Decode each frame back: VarInt21 framing + decompression yields the
        // packet id and the original body.
        let mut decoder = CompressionDecoder::new(256, true);
        for (packet, frame) in packets.iter().zip(&frames) {
            let mut buf = bytes::BytesMut::from(&frame[..]);
            let raw = Varint21FrameDecoder::new(None)
                .decode(&mut buf)
                .unwrap()
                .expect("full frame");
            let mut decoded = decoder.decode(&raw).unwrap();
            // `decoded` = varint(id) ++ body.
            let id = rivet_protocol::var_int::read(&mut decoded);
            assert_eq!(id as u32, packet.id);
            assert_eq!(&decoded[..], packet.body.as_slice());
        }
    }

    #[test]
    fn uncompressed_frames_are_the_plain_varint21_wire_form() {
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
        let frame = encode_play_frame(&packets[0], -1).unwrap();
        // payload = varint(95) ++ [0x04]; length 2 -> varint21 header 0x02.
        assert_eq!(frame.to_vec(), vec![0x02, 0x5F, 0x04]);
    }

    #[test]
    fn chunk_encode_error_returns_err_after_the_java_state_commit() {
        // Regression (issue #100 review): Java `PlayerChunkLoaderData.add()`
        // commits `lastSendDistance`/`lastTickDistance` right after the three
        // cache packets and BEFORE the async chunk queue drain. The M1 drain is
        // folded synchronously into this call, so a chunk-encode failure returns
        // `Err` only after the distances are committed — the Java ordering, not a
        // transactional all-or-nothing commit. The `Err` carries no send-set: the
        // cache packets built so far are dropped, never returned as a partial set.
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.set_fail_chunk_encoding(true);
        let err = loader.add_and_send_chunks(&mut world, None).unwrap_err();
        assert!(
            err.contains("chunk"),
            "error names the chunk encode step: {err}"
        );
        // Distances committed before the drain — the exact Java ordering.
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);

        // Clearing the seam restores the full 117-chunk send-set.
        loader.set_fail_chunk_encoding(false);
        let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
        assert_eq!(packets.len(), 3 + 117);
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);
    }

    /// The chunk-body coordinate header is the BE i32 x/z at the head of the
    /// body (the same layout `m1_send_set_is_117_chunks...` decodes).
    fn chunk_body_coords(body: &[u8]) -> ChunkPos {
        let x = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        let z = i32::from_be_bytes([body[4], body[5], body[6], body[7]]);
        ChunkPos::new(x, z)
    }

    /// The M1 `update` path on the default (RepeatSpawnFixture) world: the
    /// spawn send-set at (0,0), then a one-chunk-east move. Returns the
    /// `update` packet list and the loader for further assertions.
    fn loader_after_east_move() -> (PlayerChunkLoader, Vec<PlayPacket>) {
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.add_and_send_chunks(&mut world, None).unwrap();
        let packets = loader
            .update(&mut world, ChunkPos::new(1, 0), None)
            .unwrap();
        (loader, packets)
    }

    /// The cells the (0,0)→(1,0) move enters, in the union-box walker's
    /// X-major/Z-minor order: the new view's x=6 column (z=-4..4) plus the two
    /// cells the corner-shave shifts at x=5 (z=±5). 11 cells.
    fn east_move_enter() -> Vec<ChunkPos> {
        let mut out = Vec::new();
        out.push(ChunkPos::new(5, -5));
        out.push(ChunkPos::new(5, 5));
        for z in -4i32..=4 {
            out.push(ChunkPos::new(6, z));
        }
        out
    }

    /// The mirror image: the cells the (0,0)→(1,0) move leaves (re-entered by
    /// the symmetric (1,0)→(0,0) move).
    fn east_move_leave() -> Vec<ChunkPos> {
        let mut out = Vec::new();
        for z in -4i32..=4 {
            out.push(ChunkPos::new(-5, z));
        }
        out.push(ChunkPos::new(-4, -5));
        out.push(ChunkPos::new(-4, 5));
        out
    }

    #[test]
    fn update_unloads_departures_before_center_and_newly_entered_chunks() {
        let (_loader, packets) = loader_after_east_move();
        let unloaded: Vec<_> = packets[..11]
            .iter()
            .map(|packet| {
                assert_eq!(packet.id, PlayClientbound::ForgetLevelChunk.id());
                ChunkPos::unpack(i64::from_be_bytes(
                    packet.body.as_slice().try_into().unwrap(),
                ))
            })
            .collect();
        assert_eq!(unloaded, east_move_leave());
        let packets = &packets[11..];
        // Cache center first — the (1,0) varint body [0x01, 0x00].
        assert_eq!(packets[0].id, PlayClientbound::SetChunkCacheCenter.id());
        assert_eq!(packets[0].body, vec![0x01, 0x00]);
        // Then exactly the newly entered cells, in the deterministic
        // X-major/Z-minor raster the union-box walker emits.
        let coords: Vec<ChunkPos> = packets[1..]
            .iter()
            .map(|p| chunk_body_coords(&p.body))
            .collect();
        assert_eq!(coords, east_move_enter());
        assert_eq!(coords.len(), 11, "one-chunk move enters exactly 11 cells");
        for p in &packets[1..] {
            assert_eq!(p.id, PlayClientbound::LevelChunkWithLight.id());
        }
    }

    #[test]
    fn update_inside_the_same_chunk_emits_nothing() {
        // The nothing-to-do guard: same chunk, same distances → Java "nothing
        // we care about changed, so we're not re-calculating."
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.add_and_send_chunks(&mut world, None).unwrap();
        let center = world.view().center();
        let packets = loader.update(&mut world, center, None).unwrap();
        assert!(packets.is_empty());
        // State is untouched by the no-op.
        assert_eq!(loader.last_send_distance(), 4);
        assert_eq!(loader.last_tick_distance(), 4);
    }

    #[test]
    fn update_reemits_radius_when_requested_view_distance_changes_send() {
        // A view-distance change with the center fixed: Java `update()` emits
        // `updateClientChunkRadius` when `lastSentChunkRadius != sendViewDistance`
        // (folded into `lastSendDistance`), so a client request that re-derives
        // a different send distance emits the radius packet. On the M1 world
        // `send = min(load - 1, client + 1) = min(4, client + 1)`, so a request
        // of 2 yields send 3 (the world default 4 → client requests 0..3 lower
        // it). The tick distance (`min(sim, load - 1)`) is world-pinned, so no
        // simulation packet follows; the shrink sends no new chunks, and the
        // center is unchanged, so the radius packet is the whole output.
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.add_and_send_chunks(&mut world, None).unwrap();
        let center = world.view().center();
        let packets = loader.update(&mut world, center, Some(2)).unwrap();
        assert_eq!(packets.len(), 37, "36 unloads before radius shrink");
        assert!(
            packets[..36]
                .iter()
                .all(|packet| packet.id == PlayClientbound::ForgetLevelChunk.id())
        );
        assert_eq!(packets[36].id, PlayClientbound::SetChunkCacheRadius.id());
        assert_eq!(packets[36].body, vec![0x03]); // send 3
        assert_eq!(loader.last_send_distance(), 3);
        // The next update with the same request is a no-op (radius already 3).
        let center = world.view().center();
        let again = loader.update(&mut world, center, Some(2)).unwrap();
        assert!(again.is_empty());
        // Raising the request back to the world default re-emits radius 4 and
        // the newly-entered ring: the radius-4 view (117 cells) minus the
        // radius-3 view (the full 9×9 square = 81 cells) = 36 cells. Java's
        // `wantChunkSent` against the new send distance enqueues exactly these.
        let center = world.view().center();
        let packets = loader.update(&mut world, center, Some(8)).unwrap();
        assert_eq!(packets.len(), 1 + 36, "radius + the grown ring");
        assert_eq!(packets[0].id, PlayClientbound::SetChunkCacheRadius.id());
        assert_eq!(packets[0].body, vec![0x04]);
        for p in &packets[1..] {
            assert_eq!(p.id, PlayClientbound::LevelChunkWithLight.id());
        }
        assert_eq!(loader.last_send_distance(), 4);
    }

    #[test]
    fn repeated_updates_compound_deterministically() {
        // (0,0) -> (1,0) -> (2,0): the second move diffs against the first
        // move's view, entering the x=7 column and the (6,±5) corner-shift
        // cells — never re-sending the 117 original cells.
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.add_and_send_chunks(&mut world, None).unwrap();
        let first = loader
            .update(&mut world, ChunkPos::new(1, 0), None)
            .unwrap();
        assert_eq!(first.len(), 11 + 1 + 11);
        let second = loader
            .update(&mut world, ChunkPos::new(2, 0), None)
            .unwrap();
        assert_eq!(second[11].id, PlayClientbound::SetChunkCacheCenter.id());
        assert_eq!(second[11].body, vec![0x02, 0x00]);
        let coords: Vec<ChunkPos> = second[12..]
            .iter()
            .map(|p| chunk_body_coords(&p.body))
            .collect();
        // From (1,0) to (2,0): the (6,-5)/(6,5) corner-shift cells plus the
        // x=7 column z=-4..4 — the same shape as the first move, shifted east.
        let mut expected = Vec::new();
        expected.push(ChunkPos::new(6, -5));
        expected.push(ChunkPos::new(6, 5));
        for z in -4i32..=4 {
            expected.push(ChunkPos::new(7, z));
        }
        assert_eq!(coords, expected);
    }

    #[test]
    fn update_back_and_forth_is_symmetric() {
        // Moving east then west returns to the origin view and re-enters the
        // mirror-image west column — no dropped or duplicated cells.
        let mut world = overworld();
        let mut loader = PlayerChunkLoader::new(world.view().center());
        loader.add_and_send_chunks(&mut world, None).unwrap();
        loader
            .update(&mut world, ChunkPos::new(1, 0), None)
            .unwrap();
        let back = loader
            .update(&mut world, ChunkPos::new(0, 0), None)
            .unwrap();
        assert_eq!(back[11].id, PlayClientbound::SetChunkCacheCenter.id());
        assert_eq!(back[11].body, vec![0x00, 0x00]);
        let coords: Vec<ChunkPos> = back[12..]
            .iter()
            .map(|p| chunk_body_coords(&p.body))
            .collect();
        assert_eq!(coords, east_move_leave());
    }

    #[test]
    fn update_on_require_loaded_errors_typed_without_silent_substitution() {
        // A region-backed world (RequireLoaded) with only the spawn chunk
        // loaded: any enter cell outside it is missing, and the update path
        // fails typed `UNVERIFIED` — no generation fallback, no silent spawn
        // substitution. The loader is pre-add, so its initial state matches
        // Java's `lastSendDistance = Integer.MIN_VALUE`; the degenerate
        // previous view makes the diff fall back to the full next view (Java's
        // `difference` else-branch), so the error fires on the first missing
        // cell of the 117-cell view.
        let mut world = ServerLevel::new(super::super::server_level::ServerLevelConfig {
            missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
            ..Default::default()
        });
        let mut loader = PlayerChunkLoader::new(world.view().center());
        let err = loader
            .update(&mut world, ChunkPos::new(1, 0), None)
            .unwrap_err();
        assert!(
            err.contains("UNVERIFIED region-backed chunk"),
            "typed UNVERIFIED missing-chunk failure: {err}"
        );
        assert!(err.contains("fallback are disabled"));
    }

    fn coordinate_body(pos: ChunkPos) -> Vec<u8> {
        [pos.x().to_be_bytes(), pos.z().to_be_bytes()].concat()
    }

    #[test]
    fn readiness_drain_waits_without_marking_sent_and_delivers_once_without_recenter() {
        let mut loader = PlayerChunkLoader::new(ChunkPos::ZERO);
        loader.replace_desired(&ChunkTrackingView::of(ChunkPos::ZERO, 1));
        let desired = loader.desired.clone();
        assert!(loader.drain_ready_with(|_| Ok(None)).unwrap().is_empty());
        assert!(loader.sent.is_empty());
        assert_eq!(loader.pending, desired);
        let selected = [desired[0], desired[2]];
        let packets = loader
            .drain_ready_with(|pos| Ok(selected.contains(&pos).then(|| coordinate_body(pos))))
            .unwrap();
        assert_eq!(
            packets
                .iter()
                .map(|p| chunk_body_coords(&p.body))
                .collect::<Vec<_>>(),
            selected
        );
        assert_eq!(loader.sent.len(), 2);
        let rest = loader
            .drain_ready_with(|pos| Ok(Some(coordinate_body(pos))))
            .unwrap();
        assert_eq!(rest.len(), desired.len() - 2);
        assert!(loader.pending.is_empty());
        assert!(
            loader
                .drain_ready_with(|_| panic!("already delivered cell must not be encoded twice"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn movement_cancels_obsolete_pending_and_unloads_only_delivered_cells() {
        let mut loader = PlayerChunkLoader::new(ChunkPos::ZERO);
        loader.replace_desired(&ChunkTrackingView::of(ChunkPos::ZERO, 1));
        let old = loader.desired.clone();
        let sent = old[0];
        loader
            .drain_ready_with(|pos| Ok((pos == sent).then(|| coordinate_body(pos))))
            .unwrap();
        let (mut unloads, corner_unloads) =
            loader.replace_desired(&ChunkTrackingView::of(ChunkPos::new(100, 0), 1));
        unloads.extend(corner_unloads);
        assert_eq!(unloads.len(), 1);
        assert_eq!(unloads[0].id, PlayClientbound::ForgetLevelChunk.id());
        assert_eq!(
            ChunkPos::unpack(i64::from_be_bytes(
                unloads[0].body.as_slice().try_into().unwrap()
            )),
            sent
        );
        assert!(loader.pending.iter().all(|pos| !old.contains(pos)));
        assert!(loader.sent.is_empty());
        loader
            .drain_ready_with(|pos| {
                assert!(
                    !old.contains(&pos),
                    "obsolete pending chunk must not be requested or encoded"
                );
                Ok(Some(coordinate_body(pos)))
            })
            .unwrap();
    }

    #[test]
    fn failed_encode_pass_does_not_commit_packets_the_caller_never_received() {
        let mut loader = PlayerChunkLoader::new(ChunkPos::ZERO);
        loader.replace_desired(&ChunkTrackingView::of(ChunkPos::ZERO, 1));
        let desired = loader.desired.clone();
        let mut attempts = 0;
        let result = loader.drain_ready_with(|pos| {
            attempts += 1;
            if pos == desired[1] {
                Err("actual encoder failure".into())
            } else {
                Ok(Some(coordinate_body(pos)))
            }
        });
        assert_eq!(result.unwrap_err(), "actual encoder failure");
        assert_eq!(
            attempts,
            desired.len(),
            "all disk positions are attempted even when one failed"
        );
        assert!(loader.sent.is_empty());
        assert_eq!(loader.pending, desired);
        assert_eq!(
            loader
                .drain_ready_with(|pos| Ok(Some(coordinate_body(pos))))
                .unwrap()
                .len(),
            desired.len()
        );
    }
}

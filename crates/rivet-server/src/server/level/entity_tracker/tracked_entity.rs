//! Port of `net.minecraft.server.level.ChunkMap.TrackedEntity` (Paper 26.2).
//!
//! Tracks the deterministic `seenBy` viewer set for a single entity, runs
//! Paper's moonrise `moonrise$tick` nearby-player reconciliation, fires
//! `PlayerTrackEntityEvent` / `PlayerUntrackEntityEvent` hooks, and drives
//! `ServerEntity` pairing/unpairing and broadcast delivery.

use crate::server::level::entity_tracker::packets::EntityTrackerPacket;
use crate::server::level::entity_tracker::server_entity::{
    ServerEntity, ServerEntitySynchronizer, TrackedEntityState,
};
use crate::server::level::entity_tracker::visibility::{
    ViewerInfo, get_effective_range, is_visible_to_player,
};
use indexmap::IndexSet;
use rivet_registry::core::Vec3;

/// Nearby players snapshot for a chunk, mirroring Paper moonrise's
/// `NearbyPlayers.TrackedChunk` (`updateCount` + chunk key + player list).
#[derive(Debug, Clone)]
pub struct NearbyPlayersChunk {
    pub chunk_key: i64,
    /// Object identity of the Java `TrackedChunk` instance: a chunk removed when empty and
    /// recreated for the same key gets a fresh identity (`lastTrackedChunk != chunk`).
    pub identity: u64,
    pub update_count: i64,
    pub viewers: Vec<ViewerInfo>,
}

/// Host callbacks required by `TrackedEntity` and `ChunkMapEntityTracker`.
pub trait EntityTrackerHost {
    fn send_packet(&mut self, player_id: i32, packet: EntityTrackerPacket);

    /// `Entity.startSeenByPlayer(ServerPlayer)`.
    fn start_seen_by_player(&mut self, _entity_id: i32, _player_id: i32) {}
    /// `Entity.stopSeenByPlayer(ServerPlayer)`.
    fn stop_seen_by_player(&mut self, _entity_id: i32, _player_id: i32) {}

    /// Paper `PlayerTrackEntityEvent`. Returning `false` cancels pairing
    /// delivery while preserving Paper's exact `seenBy` / `onPlayerAdd()` rules.
    fn on_player_track_entity(&mut self, _player_id: i32, _entity_id: i32) -> bool {
        true
    }

    /// Paper `PlayerUntrackEntityEvent` (not cancellable).
    fn on_player_untrack_entity(&mut self, _player_id: i32, _entity_id: i32) {}

    /// `Level.debugSynchronizers().registerEntity(entity)`.
    fn on_debug_register_entity(&mut self, _entity_id: i32) {}
    /// `Level.debugSynchronizers().startTrackingEntity(player, entity)`.
    fn on_debug_start_tracking_entity(&mut self, _player_id: i32, _entity_id: i32) {}
    /// `Level.debugSynchronizers().dropEntity(entity)`.
    fn on_debug_drop_entity(&mut self, _entity_id: i32) {}

    /// CraftBukkit `PlayerVelocityEvent` hook. Returns `None` if cancelled.
    fn on_player_velocity_event(&mut self, _player_id: i32, velocity: Vec3) -> Option<Vec3> {
        Some(velocity)
    }

    /// `ChunkMap.updatePlayerStatus(ServerPlayer, boolean track)`.
    fn update_player_status(&mut self, _player_id: i32, _track: bool) {}

    /// Per-pair Bukkit visibility check (`player.getBukkitEntity().canSee(entity.getBukkitEntity())`).
    fn can_see(&self, _viewer_id: i32, _entity_id: i32) -> bool {
        true
    }

    /// Per-pair entity broadcast predicate (`entity.broadcastToPlayer(player)`).
    fn broadcast_to_player(&self, _entity_id: i32, _viewer_id: i32) -> bool {
        true
    }

    /// Per-pair Moonrise chunk tracking check (`ChunkMap.isChunkTracked(player, chunkX, chunkZ)`).
    fn is_chunk_tracked(&self, _viewer_id: i32, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
}

/// Port of `ChunkMap.TrackedEntity`.
#[derive(Debug, Clone)]
pub struct TrackedEntity {
    entity_id: i32,
    is_server_player: bool,
    pub server_entity: ServerEntity,
    range: i32,
    seen_by: IndexSet<i32>,
    last_chunk_update: i64,
    last_tracked_chunk: Option<u64>,
}

impl TrackedEntity {
    pub fn new(
        entity: &dyn TrackedEntityState,
        range: i32,
        update_interval: i32,
        track_delta: bool,
    ) -> Self {
        Self {
            entity_id: entity.entity_id(),
            is_server_player: entity.is_server_player(),
            server_entity: ServerEntity::new(entity, update_interval, track_delta),
            range,
            seen_by: IndexSet::new(),
            last_chunk_update: -1,
            last_tracked_chunk: None,
        }
    }

    pub fn entity_id(&self) -> i32 {
        self.entity_id
    }

    pub fn base_range(&self) -> i32 {
        self.range
    }

    pub fn has_players(&self) -> bool {
        !self.seen_by.is_empty()
    }

    pub fn seen_by(&self) -> &IndexSet<i32> {
        &self.seen_by
    }

    pub fn seen_by_vec(&self) -> Vec<i32> {
        self.seen_by.iter().copied().collect()
    }

    /// `ChunkMap.TrackedEntity.getEffectiveRange()`.
    pub fn effective_range(
        &self,
        indirect_passenger_ranges: &[i32],
        broadcast_range_percentage: i32,
    ) -> i32 {
        get_effective_range(
            self.range,
            indirect_passenger_ranges,
            broadcast_range_percentage,
        )
    }

    /// `ChunkMap.TrackedEntity.moonrise$tick(NearbyPlayers.TrackedChunk)`.
    pub fn moonrise_tick(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        nearby: Option<&NearbyPlayersChunk>,
        indirect_passenger_ranges: &[i32],
        broadcast_range_percentage: i32,
        range_y: f64,
        host: &mut dyn EntityTrackerHost,
    ) {
        let Some(chunk) = nearby else {
            self.clear_players(host);
            return;
        };
        if chunk.viewers.is_empty() {
            self.clear_players(host);
            return;
        }

        let prev_chunk_update = self.last_chunk_update;
        let curr_chunk_update = chunk.update_count;
        let prev_chunk = self.last_tracked_chunk;
        self.last_chunk_update = curr_chunk_update;
        self.last_tracked_chunk = Some(chunk.identity);

        let effective = self.effective_range(indirect_passenger_ranges, broadcast_range_percentage);
        for viewer in &chunk.viewers {
            self.update_player_with_effective_range(entity, viewer, effective, range_y, host);
        }

        if prev_chunk_update != curr_chunk_update || prev_chunk != Some(chunk.identity) {
            let current_viewers: Vec<i32> = self.seen_by.iter().copied().collect();
            for viewer_id in current_viewers {
                if !chunk.viewers.iter().any(|v| v.player_id == viewer_id) {
                    self.remove_player(viewer_id, host);
                }
            }
        }
    }

    /// `ChunkMap.TrackedEntity.moonrise$clearPlayers()`.
    pub fn clear_players(&mut self, host: &mut dyn EntityTrackerHost) {
        self.last_chunk_update = -1;
        self.last_tracked_chunk = None;
        if self.seen_by.is_empty() {
            return;
        }
        let viewers: Vec<i32> = self.seen_by.iter().copied().collect();
        for viewer_id in viewers {
            self.remove_player(viewer_id, host);
        }
    }

    /// `ChunkMap.TrackedEntity.broadcastRemoved()`.
    pub fn broadcast_removed(&mut self, host: &mut dyn EntityTrackerHost) {
        let viewers: Vec<i32> = self.seen_by.iter().copied().collect();
        for viewer_id in viewers {
            host.on_player_untrack_entity(viewer_id, self.entity_id);
            let mut sync = TrackedSynchronizer {
                entity_id: self.entity_id,
                is_server_player: self.is_server_player,
                seen_by: &self.seen_by,
                host,
            };
            self.server_entity
                .remove_pairing(self.entity_id, viewer_id, &mut sync);
        }
    }

    /// `ChunkMap.TrackedEntity.removePlayer(ServerPlayer)`.
    pub fn remove_player(&mut self, player_id: i32, host: &mut dyn EntityTrackerHost) -> bool {
        if !self.seen_by.shift_remove(&player_id) {
            return false;
        }
        host.on_player_untrack_entity(player_id, self.entity_id);
        let mut sync = TrackedSynchronizer {
            entity_id: self.entity_id,
            is_server_player: self.is_server_player,
            seen_by: &self.seen_by,
            host,
        };
        self.server_entity
            .remove_pairing(self.entity_id, player_id, &mut sync);
        if self.seen_by.is_empty() {
            host.on_debug_drop_entity(self.entity_id);
        }
        true
    }

    /// `ChunkMap.TrackedEntity.updatePlayer(ServerPlayer)`.
    pub fn update_player(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        viewer: &ViewerInfo,
        indirect_passenger_ranges: &[i32],
        broadcast_range_percentage: i32,
        range_y: f64,
        host: &mut dyn EntityTrackerHost,
    ) {
        let effective = self.effective_range(indirect_passenger_ranges, broadcast_range_percentage);
        self.update_player_with_effective_range(entity, viewer, effective, range_y, host);
    }

    fn update_player_with_effective_range(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        viewer: &ViewerInfo,
        effective_range: i32,
        range_y: f64,
        host: &mut dyn EntityTrackerHost,
    ) {
        if viewer.player_id == self.entity_id {
            return;
        }
        let pos = entity.position();
        let chunk_x = (pos.x.floor() as i64 >> 4) as i32;
        let chunk_z = (pos.z.floor() as i64 >> 4) as i32;
        let effective_viewer = ViewerInfo {
            broadcast_to_player: viewer.broadcast_to_player
                && host.broadcast_to_player(self.entity_id, viewer.player_id),
            chunk_tracked: viewer.chunk_tracked
                && host.is_chunk_tracked(viewer.player_id, chunk_x, chunk_z),
            can_see: viewer.can_see && host.can_see(viewer.player_id, self.entity_id),
            ..*viewer
        };
        let visible = is_visible_to_player(
            self.entity_id,
            pos,
            effective_range,
            range_y,
            &effective_viewer,
        );
        if visible {
            if self.seen_by.insert(viewer.player_id) {
                if host.on_player_track_entity(viewer.player_id, self.entity_id) {
                    let mut sync = TrackedSynchronizer {
                        entity_id: self.entity_id,
                        is_server_player: self.is_server_player,
                        seen_by: &self.seen_by,
                        host,
                    };
                    self.server_entity
                        .add_pairing(entity, viewer.player_id, &mut sync);
                    if self.seen_by.len() == 1 {
                        host.on_debug_register_entity(self.entity_id);
                    }
                    host.on_debug_start_tracking_entity(viewer.player_id, self.entity_id);
                }
                self.server_entity.on_player_add();
            }
        } else {
            self.remove_player(viewer.player_id, host);
        }
    }

    /// `ChunkMap.TrackedEntity.updatePlayers(List<ServerPlayer>)`.
    pub fn update_players(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        viewers: &[ViewerInfo],
        indirect_passenger_ranges: &[i32],
        broadcast_range_percentage: i32,
        range_y: f64,
        host: &mut dyn EntityTrackerHost,
    ) {
        let effective = self.effective_range(indirect_passenger_ranges, broadcast_range_percentage);
        for viewer in viewers {
            self.update_player_with_effective_range(entity, viewer, effective, range_y, host);
        }
    }

    /// Runs `ServerEntity::send_changes` with this tracker's `seenBy` synchronizer.
    pub fn send_changes(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        item_frame_cursor_update_interval: i32,
        host: &mut dyn EntityTrackerHost,
    ) {
        let tracked_players: Vec<i32> = self.seen_by.iter().copied().collect();
        let mut sync = TrackedSynchronizer {
            entity_id: self.entity_id,
            is_server_player: self.is_server_player,
            seen_by: &self.seen_by,
            host,
        };
        self.server_entity.send_changes(
            entity,
            &tracked_players,
            item_frame_cursor_update_interval,
            &mut sync,
        );
    }

    /// Flushes dirty equipment updates to all tracking players.
    pub fn flush_equipment_updates(
        &self,
        entity: &mut dyn TrackedEntityState,
        host: &mut dyn EntityTrackerHost,
    ) -> bool {
        let mut sync = TrackedSynchronizer {
            entity_id: self.entity_id,
            is_server_player: self.is_server_player,
            seen_by: &self.seen_by,
            host,
        };
        self.server_entity
            .flush_equipment_updates(entity, &mut sync)
    }

    /// `ChunkMap.TrackedEntity.sendToTrackingPlayers(Packet)`.
    pub fn send_to_tracking_players(
        &self,
        packet: EntityTrackerPacket,
        host: &mut dyn EntityTrackerHost,
    ) {
        for &player_id in &self.seen_by {
            host.send_packet(player_id, packet.clone());
        }
    }

    /// `ChunkMap.TrackedEntity.sendToTrackingPlayersAndSelf(Packet)`.
    pub fn send_to_tracking_players_and_self(
        &self,
        packet: EntityTrackerPacket,
        host: &mut dyn EntityTrackerHost,
    ) {
        self.send_to_tracking_players(packet.clone(), host);
        if self.is_server_player {
            host.send_packet(self.entity_id, packet);
        }
    }

    /// `ChunkMap.TrackedEntity.sendToTrackingPlayersFiltered(Packet, Predicate<ServerPlayer>)`.
    pub fn send_to_tracking_players_filtered(
        &self,
        packet: EntityTrackerPacket,
        predicate: &dyn Fn(i32) -> bool,
        host: &mut dyn EntityTrackerHost,
    ) {
        for &player_id in &self.seen_by {
            if predicate(player_id) {
                host.send_packet(player_id, packet.clone());
            }
        }
    }
}

struct TrackedSynchronizer<'a> {
    entity_id: i32,
    is_server_player: bool,
    seen_by: &'a IndexSet<i32>,
    host: &'a mut dyn EntityTrackerHost,
}

impl ServerEntitySynchronizer for TrackedSynchronizer<'_> {
    fn send_to_tracking_players(&mut self, packet: EntityTrackerPacket) {
        for &player_id in self.seen_by {
            self.host.send_packet(player_id, packet.clone());
        }
    }

    fn send_to_tracking_players_and_self(&mut self, packet: EntityTrackerPacket) {
        self.send_to_tracking_players(packet.clone());
        if self.is_server_player {
            self.host.send_packet(self.entity_id, packet);
        }
    }

    fn send_to_tracking_players_filtered(
        &mut self,
        packet: EntityTrackerPacket,
        predicate: &dyn Fn(i32) -> bool,
    ) {
        for &player_id in self.seen_by {
            if predicate(player_id) {
                self.host.send_packet(player_id, packet.clone());
            }
        }
    }

    fn send_to_player(&mut self, player_id: i32, packet: EntityTrackerPacket) {
        self.host.send_packet(player_id, packet);
    }

    fn start_seen_by_player(&mut self, entity_id: i32, player_id: i32) {
        self.host.start_seen_by_player(entity_id, player_id);
    }

    fn stop_seen_by_player(&mut self, entity_id: i32, player_id: i32) {
        self.host.stop_seen_by_player(entity_id, player_id);
    }

    fn on_player_velocity_event(&mut self, player_id: i32, velocity: Vec3) -> Option<Vec3> {
        self.host.on_player_velocity_event(player_id, velocity)
    }
}

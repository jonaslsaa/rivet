//! Port of `net.minecraft.server.level.ChunkMap` entity-tracker management
//! (`addEntity`, `removeEntity`, `newTrackerTick`, broadcast helpers, and
//! dirty equipment/link emission scheduling) (Paper 26.2).

use crate::server::level::entity_tracker::packets::EntityTrackerPacket;
use crate::server::level::entity_tracker::server_entity::TrackedEntityState;
use crate::server::level::entity_tracker::tracked_entity::{
    EntityTrackerHost, NearbyPlayersChunk, TrackedEntity,
};
use crate::server::level::entity_tracker::visibility::{
    ActivationType, TrackingEntityKind, TrackingRangeConfig, ViewerInfo, get_entity_tracking_range,
};
use crate::server::level::full_chunk_status::FullChunkStatus;
use indexmap::IndexMap;
use thiserror::Error;

/// Errors surfaced for illegal tracker operations (matching Paper's
/// `ChunkMap::addEntity` guards).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EntityTrackerError {
    #[error("illegal ChunkMap::addEntity for entity {entity_id}: {reason}")]
    IllegalAddEntity {
        entity_id: i32,
        reason: &'static str,
    },
}

/// Metadata required by `ChunkMapEntityTracker::add_entity` for an entity.
#[derive(Debug, Clone)]
pub struct EntityTrackRegistration {
    pub valid: bool,
    pub level_id: u64,
    pub is_ender_dragon_part: bool,
    pub suppress_tracker_for_login: bool,
    pub kind: TrackingEntityKind,
    pub activation_type: ActivationType,
    /// `EntityType.clientTrackingRange()` in chunks.
    pub client_tracking_range_chunks: i32,
    /// `EntityType.updateInterval()` in ticks.
    pub update_interval: i32,
    /// `EntityType.trackDeltas()`.
    pub track_delta: bool,
    /// Indirect passengers' already-modified tracking ranges in blocks.
    pub indirect_passenger_ranges: Vec<i32>,
}

/// Per-entity context supplied during `ChunkMapEntityTracker::new_tracker_tick`.
#[derive(Debug, Clone)]
pub struct TrackerTickContext {
    pub entity_id: i32,
    pub kind: TrackingEntityKind,
    pub activation_type: ActivationType,
    pub chunk_status: FullChunkStatus,
    pub nearby: Option<NearbyPlayersChunk>,
    pub indirect_passenger_ranges: Vec<i32>,
}

/// Port of `ChunkMap`'s entity tracking map (`entityMap`) and lifecycle methods.
#[derive(Debug, Clone)]
pub struct ChunkMapEntityTracker {
    level_id: u64,
    config: TrackingRangeConfig,
    entity_map: IndexMap<i32, TrackedEntity>,
}

impl ChunkMapEntityTracker {
    pub fn new(level_id: u64, config: TrackingRangeConfig) -> Self {
        Self {
            level_id,
            config,
            entity_map: IndexMap::new(),
        }
    }

    pub fn config(&self) -> &TrackingRangeConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut TrackingRangeConfig {
        &mut self.config
    }

    pub fn len(&self) -> usize {
        self.entity_map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entity_map.is_empty()
    }

    pub fn has_entity_with_id(&self, entity_id: i32) -> bool {
        self.entity_map.contains_key(&entity_id)
    }

    pub fn get(&self, entity_id: i32) -> Option<&TrackedEntity> {
        self.entity_map.get(&entity_id)
    }

    pub fn get_mut(&mut self, entity_id: i32) -> Option<&mut TrackedEntity> {
        self.entity_map.get_mut(&entity_id)
    }

    /// `ChunkMap.addEntity(Entity)`.
    ///
    /// Returns `Ok(true)` if the entity was added to `entity_map`, `Ok(false)`
    /// if legitimately skipped (`EnderDragonPart`, `suppressTrackerForLogin`, or
    /// `range == 0`), or `Err(EntityTrackerError::IllegalAddEntity)` if the
    /// entity is invalid, belongs to a different level, or is already tracked.
    #[allow(clippy::too_many_arguments)]
    pub fn add_entity<F>(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        reg: &EntityTrackRegistration,
        level_players: &[ViewerInfo],
        self_viewer: Option<&ViewerInfo>,
        mut resolve_other: F,
        host: &mut dyn EntityTrackerHost,
    ) -> Result<bool, EntityTrackerError>
    where
        F: FnMut(
            i32,
        ) -> Option<(
            &'static mut dyn TrackedEntityState,
            TrackingEntityKind,
            ActivationType,
            Vec<i32>,
        )>,
    {
        let entity_id = entity.entity_id();
        if !reg.valid {
            return Err(EntityTrackerError::IllegalAddEntity {
                entity_id,
                reason: "entity is not valid",
            });
        }
        if reg.level_id != self.level_id {
            return Err(EntityTrackerError::IllegalAddEntity {
                entity_id,
                reason: "entity belongs to a different level",
            });
        }
        if self.entity_map.contains_key(&entity_id) {
            return Err(EntityTrackerError::IllegalAddEntity {
                entity_id,
                reason: "entity is already tracked",
            });
        }
        if entity.is_server_player() && reg.suppress_tracker_for_login {
            return Ok(false);
        }
        if reg.is_ender_dragon_part {
            return Ok(false);
        }

        let default_range = reg.client_tracking_range_chunks.wrapping_mul(16);
        let range =
            get_entity_tracking_range(reg.kind, reg.activation_type, default_range, &self.config);
        if range == 0 {
            return Ok(false);
        }

        let range_y = self
            .config
            .tracking_range_y
            .range_for(reg.kind, reg.activation_type);
        let mut tracked = TrackedEntity::new(entity, range, reg.update_interval, reg.track_delta);
        tracked.update_players(
            entity,
            level_players,
            &reg.indirect_passenger_ranges,
            self.config.entity_broadcast_range_percentage,
            range_y,
            host,
        );
        let is_player = entity.is_server_player();
        self.entity_map.insert(entity_id, tracked);

        if is_player {
            host.update_player_status(entity_id, true);
            if let Some(viewer) = self_viewer {
                for (&other_id, other_tracked) in &mut self.entity_map {
                    if other_id != entity_id
                        && let Some((other_entity, other_kind, other_act, other_passengers)) =
                            resolve_other(other_id)
                    {
                        let other_range_y = self
                            .config
                            .tracking_range_y
                            .range_for(other_kind, other_act);
                        other_tracked.update_player(
                            other_entity,
                            viewer,
                            &other_passengers,
                            self.config.entity_broadcast_range_percentage,
                            other_range_y,
                            host,
                        );
                    }
                }
            }
        }

        Ok(true)
    }

    /// Simplified `add_entity` when adding a non-player entity (or when the caller
    /// handles cross-entity player updates separately via `update_player_for_all`).
    pub fn add_entity_simple(
        &mut self,
        entity: &mut dyn TrackedEntityState,
        reg: &EntityTrackRegistration,
        level_players: &[ViewerInfo],
        host: &mut dyn EntityTrackerHost,
    ) -> Result<bool, EntityTrackerError> {
        let entity_id = entity.entity_id();
        if !reg.valid {
            return Err(EntityTrackerError::IllegalAddEntity {
                entity_id,
                reason: "entity is not valid",
            });
        }
        if reg.level_id != self.level_id {
            return Err(EntityTrackerError::IllegalAddEntity {
                entity_id,
                reason: "entity belongs to a different level",
            });
        }
        if self.entity_map.contains_key(&entity_id) {
            return Err(EntityTrackerError::IllegalAddEntity {
                entity_id,
                reason: "entity is already tracked",
            });
        }
        if entity.is_server_player() && reg.suppress_tracker_for_login {
            return Ok(false);
        }
        if reg.is_ender_dragon_part {
            return Ok(false);
        }

        let default_range = reg.client_tracking_range_chunks.wrapping_mul(16);
        let range =
            get_entity_tracking_range(reg.kind, reg.activation_type, default_range, &self.config);
        if range == 0 {
            return Ok(false);
        }

        let range_y = self
            .config
            .tracking_range_y
            .range_for(reg.kind, reg.activation_type);
        let mut tracked = TrackedEntity::new(entity, range, reg.update_interval, reg.track_delta);
        tracked.update_players(
            entity,
            level_players,
            &reg.indirect_passenger_ranges,
            self.config.entity_broadcast_range_percentage,
            range_y,
            host,
        );
        let is_player = entity.is_server_player();
        self.entity_map.insert(entity_id, tracked);
        if is_player {
            host.update_player_status(entity_id, true);
        }
        Ok(true)
    }

    /// Updates a newly added `ServerPlayer` against every other tracked entity
    /// in `entity_map` (second half of `ChunkMap::addEntity` for players,
    /// avoiding `'static` lifetime requirements on borrowed arena storage).
    pub fn update_player_against_all<F>(
        &mut self,
        viewer: &ViewerInfo,
        mut with_entity: F,
        host: &mut dyn EntityTrackerHost,
    ) where
        F: FnMut(
            i32,
            &mut dyn FnMut(&mut dyn TrackedEntityState, TrackingEntityKind, ActivationType, &[i32]),
        ),
    {
        let pct = self.config.entity_broadcast_range_percentage;
        let range_y_cfg = self.config.tracking_range_y;
        for (&other_id, other_tracked) in &mut self.entity_map {
            if other_id == viewer.player_id {
                continue;
            }
            with_entity(
                other_id,
                &mut |other_entity, kind, act, passenger_ranges| {
                    let range_y = range_y_cfg.range_for(kind, act);
                    other_tracked.update_player(
                        other_entity,
                        viewer,
                        passenger_ranges,
                        pct,
                        range_y,
                        host,
                    );
                },
            );
        }
    }

    /// `ChunkMap.removeEntity(Entity)`.
    ///
    /// If `is_server_player`, calls `update_player_status(entity_id, false)` and
    /// removes the player from every tracked entity's `seenBy`. Then removes
    /// `entity_id` from `entity_map` and calls `broadcast_removed()`.
    pub fn remove_entity(
        &mut self,
        entity_id: i32,
        is_server_player: bool,
        host: &mut dyn EntityTrackerHost,
    ) -> bool {
        if is_server_player {
            host.update_player_status(entity_id, false);
            for tracked in self.entity_map.values_mut() {
                tracked.remove_player(entity_id, host);
            }
        }
        if let Some(mut tracked) = self.entity_map.shift_remove(&entity_id) {
            tracked.broadcast_removed(host);
            true
        } else {
            false
        }
    }

    /// `ChunkMap.newTrackerTick()` (Paper moonrise).
    ///
    /// For each entity in `contexts`:
    /// 1. Calls `tracked.moonrise_tick(...)` to reconcile `seenBy` against the
    ///    entity's current nearby-players chunk.
    /// 2. If `tracked.has_players() || chunk_status.is_or_after(FullChunkStatus::EntityTicking)`,
    ///    calls `tracked.send_changes(...)`.
    pub fn new_tracker_tick<F>(
        &mut self,
        contexts: &[TrackerTickContext],
        mut with_entity: F,
        host: &mut dyn EntityTrackerHost,
    ) where
        F: FnMut(i32, &mut dyn FnMut(&mut dyn TrackedEntityState)),
    {
        let pct = self.config.entity_broadcast_range_percentage;
        let range_y_cfg = self.config.tracking_range_y;
        let map_interval = self.config.item_frame_cursor_update_interval;

        for ctx in contexts {
            let Some(tracked) = self.entity_map.get_mut(&ctx.entity_id) else {
                continue;
            };
            let range_y = range_y_cfg.range_for(ctx.kind, ctx.activation_type);
            with_entity(ctx.entity_id, &mut |entity| {
                tracked.moonrise_tick(
                    entity,
                    ctx.nearby.as_ref(),
                    &ctx.indirect_passenger_ranges,
                    pct,
                    range_y,
                    host,
                );
                if tracked.has_players()
                    || ctx.chunk_status.is_or_after(FullChunkStatus::EntityTicking)
                {
                    tracked.send_changes(entity, map_interval, host);
                }
            });
        }
    }

    /// `ChunkMap.sendToTrackingPlayers(Entity, Packet)`.
    pub fn send_to_tracking_players(
        &self,
        entity_id: i32,
        packet: EntityTrackerPacket,
        host: &mut dyn EntityTrackerHost,
    ) {
        if let Some(tracked) = self.entity_map.get(&entity_id) {
            tracked.send_to_tracking_players(packet, host);
        }
    }

    /// `ChunkMap.sendToTrackingPlayersAndSelf(Entity, Packet)`.
    pub fn send_to_tracking_players_and_self(
        &self,
        entity_id: i32,
        packet: EntityTrackerPacket,
        host: &mut dyn EntityTrackerHost,
    ) {
        if let Some(tracked) = self.entity_map.get(&entity_id) {
            tracked.send_to_tracking_players_and_self(packet, host);
        }
    }

    /// `ChunkMap.sendToTrackingPlayersFiltered(Entity, Packet, Predicate<ServerPlayer>)`.
    pub fn send_to_tracking_players_filtered(
        &self,
        entity_id: i32,
        packet: EntityTrackerPacket,
        predicate: &dyn Fn(i32) -> bool,
        host: &mut dyn EntityTrackerHost,
    ) {
        if let Some(tracked) = self.entity_map.get(&entity_id) {
            tracked.send_to_tracking_players_filtered(packet, predicate, host);
        }
    }

    /// Schedules dirty equipment emission for `entity_id` to all tracking players.
    pub fn flush_equipment_updates(
        &self,
        entity: &mut dyn TrackedEntityState,
        host: &mut dyn EntityTrackerHost,
    ) -> bool {
        if let Some(tracked) = self.entity_map.get(&entity.entity_id()) {
            tracked.flush_equipment_updates(entity, host)
        } else {
            false
        }
    }

    /// Emits `ClientboundSetEntityLinkPacket` to tracking players when an
    /// entity's leash holder changes (`dest_id == 0` when unlinked).
    pub fn send_entity_link_update(
        &self,
        source_id: i32,
        dest_id: i32,
        host: &mut dyn EntityTrackerHost,
    ) {
        self.send_to_tracking_players(
            source_id,
            EntityTrackerPacket::SetEntityLink { source_id, dest_id },
            host,
        );
    }

    /// `ChunkMap.isTrackedByAnyPlayer(Entity)`.
    pub fn is_tracked_by_any_player(&self, entity_id: i32) -> bool {
        self.entity_map
            .get(&entity_id)
            .is_some_and(TrackedEntity::has_players)
    }

    /// `ChunkMap.forEachEntityTrackedBy(ServerPlayer, Consumer<Entity>)` — returns
    /// tracked entity IDs in deterministic `entity_map` order.
    pub fn entities_tracked_by(&self, player_id: i32) -> Vec<i32> {
        self.entity_map
            .iter()
            .filter_map(|(&entity_id, tracked)| {
                tracked.seen_by().contains(&player_id).then_some(entity_id)
            })
            .collect()
    }
}

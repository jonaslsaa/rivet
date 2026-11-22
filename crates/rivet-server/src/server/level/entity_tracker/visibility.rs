//! Port of Spigot `TrackingRange` and Paper `ChunkMap.TrackedEntity` visibility
//! range checks (Paper 26.2).

use rivet_registry::core::Vec3;

/// `org.spigotmc.ActivationRange.ActivationType` values consulted by
/// `TrackingRange.getEntityTrackingRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivationType {
    Raider,
    Monster,
    FlyingMonster,
    Water,
    Villager,
    Animal,
    Misc,
}

/// Entity classification for `TrackingRange.getEntityTrackingRange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackingEntityKind {
    ServerPlayer,
    EnderDragon,
    /// `ItemFrame`, `Painting`, `ItemEntity`, or `ExperienceOrb`.
    MiscTracked,
    Display,
    Other,
}

/// Paper's `entities.tracking-range-y` configuration.
///
/// A value of `-1.0` (or `None`) disables the Y-axis check for that category,
/// matching `WorldConfiguration.Entities.TrackingRangeY.get(entity, -1)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackingRangeYConfig {
    pub enabled: bool,
    pub player: f64,
    pub animal: f64,
    pub monster: f64,
    pub misc: f64,
    pub display: f64,
    pub other: f64,
}

impl Default for TrackingRangeYConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            player: 320.0,
            animal: 320.0,
            monster: 320.0,
            misc: 320.0,
            display: 320.0,
            other: 320.0,
        }
    }
}

impl TrackingRangeYConfig {
    /// Returns the configured Y tracking range for the entity category, or
    /// `-1.0` when disabled.
    pub fn range_for(&self, kind: TrackingEntityKind, activation_type: ActivationType) -> f64 {
        if !self.enabled {
            return -1.0;
        }
        if kind == TrackingEntityKind::ServerPlayer {
            return self.player;
        }
        match activation_type {
            ActivationType::Raider | ActivationType::Monster | ActivationType::FlyingMonster => {
                return self.monster;
            }
            ActivationType::Water | ActivationType::Villager | ActivationType::Animal => {
                return self.animal;
            }
            ActivationType::Misc => {}
        }
        match kind {
            TrackingEntityKind::MiscTracked => self.misc,
            TrackingEntityKind::Display => self.display,
            _ => self.other,
        }
    }
}

/// Spigot + Paper tracking range configuration for a level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackingRangeConfig {
    pub player_tracking_range: i32,
    pub animal_tracking_range: i32,
    pub monster_tracking_range: i32,
    pub misc_tracking_range: i32,
    pub display_tracking_range: i32,
    pub other_tracking_range: i32,
    /// `ChunkMap.serverViewDistance` (in chunks) used for the EnderDragon exemption.
    pub server_view_distance_chunks: i32,
    /// `MinecraftServer.getScaledTrackingDistance` percentage (`entity-broadcast-range-percentage`).
    pub entity_broadcast_range_percentage: i32,
    /// Paper `entities.tracking-range-y`.
    pub tracking_range_y: TrackingRangeYConfig,
    /// Paper `maps.item-frame-cursor-update-interval`.
    pub item_frame_cursor_update_interval: i32,
}

impl Default for TrackingRangeConfig {
    fn default() -> Self {
        Self {
            player_tracking_range: 128,
            animal_tracking_range: 96,
            monster_tracking_range: 96,
            misc_tracking_range: 96,
            display_tracking_range: 128,
            other_tracking_range: 64,
            server_view_distance_chunks: 10,
            entity_broadcast_range_percentage: 100,
            tracking_range_y: TrackingRangeYConfig::default(),
            item_frame_cursor_update_interval: 10,
        }
    }
}

/// Exact port of `org.spigotmc.TrackingRange.getEntityTrackingRange(Entity, int)`.
pub fn get_entity_tracking_range(
    kind: TrackingEntityKind,
    activation_type: ActivationType,
    default_range_blocks: i32,
    config: &TrackingRangeConfig,
) -> i32 {
    if default_range_blocks == 0 {
        return default_range_blocks;
    }
    if kind == TrackingEntityKind::ServerPlayer {
        return config.player_tracking_range;
    }
    if kind == TrackingEntityKind::EnderDragon {
        return config.server_view_distance_chunks.wrapping_shl(4);
    }
    match activation_type {
        ActivationType::Raider | ActivationType::Monster | ActivationType::FlyingMonster => {
            return config.monster_tracking_range;
        }
        ActivationType::Water | ActivationType::Villager | ActivationType::Animal => {
            return config.animal_tracking_range;
        }
        ActivationType::Misc => {}
    }
    match kind {
        TrackingEntityKind::MiscTracked => config.misc_tracking_range,
        TrackingEntityKind::Display => config.display_tracking_range,
        _ => config.other_tracking_range,
    }
}

/// `MinecraftServer.getScaledTrackingDistance(int range)`:
/// `range * entityBroadcastRangePercentage / 100`.
#[inline]
pub fn get_scaled_tracking_distance(range: i32, percentage: i32) -> i32 {
    range.wrapping_mul(percentage) / 100
}

/// Exact port of `ChunkMap.TrackedEntity.getEffectiveRange()`.
///
/// Takes the entity's base range and the already-modified tracking ranges of
/// all indirect passengers (`modifyEntityTrackingRange(p, p.clientTrackingRange << 4)`),
/// takes the maximum, and applies `getScaledTrackingDistance`.
pub fn get_effective_range(
    base_range: i32,
    indirect_passenger_ranges: &[i32],
    broadcast_range_percentage: i32,
) -> i32 {
    let mut range = base_range;
    for &passenger_range in indirect_passenger_ranges {
        range = range.max(passenger_range);
    }
    get_scaled_tracking_distance(range, broadcast_range_percentage)
}

/// Viewer attributes consulted by `ChunkMap.TrackedEntity.updatePlayer(ServerPlayer)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewerInfo {
    pub player_id: i32,
    pub position: Vec3,
    pub view_distance_chunks: i32,
    /// `Entity.broadcastToPlayer(player)`.
    pub broadcast_to_player: bool,
    /// `ChunkMap.isChunkTracked(player, entityChunkX, entityChunkZ)`.
    pub chunk_tracked: bool,
    /// CraftBukkit vanish check: `player.getBukkitEntity().canSee(entity.getBukkitEntity())`.
    pub can_see: bool,
}

/// Exact port of the visibility predicate in `ChunkMap.TrackedEntity.updatePlayer(ServerPlayer)`.
pub fn is_visible_to_player(
    entity_id: i32,
    entity_pos: Vec3,
    effective_range: i32,
    range_y: f64,
    viewer: &ViewerInfo,
) -> bool {
    if viewer.player_id == entity_id {
        return false;
    }
    let delta_x = viewer.position.x - entity_pos.x;
    let delta_z = viewer.position.z - entity_pos.z;
    let visible_range = effective_range.min(viewer.view_distance_chunks.wrapping_mul(16)) as f64;
    let distance_squared = delta_x * delta_x + delta_z * delta_z;
    let range_squared = visible_range * visible_range;

    let mut visible = distance_squared <= range_squared;
    if visible && range_y != -1.0 {
        let delta_y = viewer.position.y - entity_pos.y;
        visible = delta_y * delta_y <= range_y * range_y;
    }
    visible = visible && viewer.broadcast_to_player && viewer.chunk_tracked;
    if visible && !viewer.can_see {
        visible = false;
    }
    visible
}

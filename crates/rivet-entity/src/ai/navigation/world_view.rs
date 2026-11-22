//! Tick-thread-confined world and mob seam types for pathfinding and navigation.
//!
//! Per `OWNERSHIP.md`, pathfinding and navigation take `&dyn PathWorldView` and
//! `&MobPathingState` / `&mut MobPathingState` rather than holding `Arc<RwLock>`
//! references to worlds or entities.

use rivet_util::mth;

use crate::ai::goal::context::{Aabb, BlockPos, Vec3};
use crate::ai::navigation::path_type::{PathType, PathTypeCache};

/// Classification facts for a single block position needed by Paper 26.2's
/// `WalkNodeEvaluator.getPathTypeFromState`, `SwimNodeEvaluator`, `MoveControl`,
/// and `PathNavigation`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockPathInfo {
    /// Paper patch: unloaded chunks return `PathType::Blocked` without loading.
    pub is_loaded: bool,
    pub is_air: bool,
    /// `BlockTags.TRAPDOORS`, `Blocks.LILY_PAD`, or `Blocks.BIG_DRIPLEAF`.
    pub is_trapdoor_like: bool,
    pub is_powder_snow: bool,
    /// `Blocks.CACTUS` or `Blocks.SWEET_BERRY_BUSH`.
    pub is_damaging: bool,
    pub is_honey: bool,
    pub is_cocoa: bool,
    /// `Blocks.WITHER_ROSE` or `BlockTags.SPELEOTHEMS`.
    pub is_damage_cautious: bool,
    pub is_lava: bool,
    pub is_water: bool,
    /// `WalkNodeEvaluator.isBurningBlock(BlockState)`.
    pub is_burning: bool,
    pub is_door_open: bool,
    pub is_wood_door_closed: bool,
    pub is_iron_door_closed: bool,
    pub is_rail: bool,
    pub is_leaves: bool,
    /// `BlockTags.FENCES`, `BlockTags.WALLS`, or closed `FenceGateBlock`.
    pub is_fence_or_wall: bool,
    /// `blockState.isPathfindable(PathComputationType.LAND)`.
    pub is_pathfindable_land: bool,
    /// `blockState.isPathfindable(PathComputationType.WATER)`.
    pub is_pathfindable_water: bool,
    /// Maximum Y of the block's collision `VoxelShape` in local `[0.0, 1.0+]` space,
    /// or `None` if `collisionShape.isEmpty()`.
    pub collision_max_y: Option<f64>,
    /// Whether sky light reaches this position (`level.canSeeSky(pos)`).
    pub can_see_sky: bool,
}

impl BlockPathInfo {
    /// Standard air block (loaded, empty collision, land-pathfindable).
    pub const AIR: Self = Self {
        is_loaded: true,
        is_air: true,
        is_trapdoor_like: false,
        is_powder_snow: false,
        is_damaging: false,
        is_honey: false,
        is_cocoa: false,
        is_damage_cautious: false,
        is_lava: false,
        is_water: false,
        is_burning: false,
        is_door_open: false,
        is_wood_door_closed: false,
        is_iron_door_closed: false,
        is_rail: false,
        is_leaves: false,
        is_fence_or_wall: false,
        is_pathfindable_land: true,
        is_pathfindable_water: false,
        collision_max_y: None,
        can_see_sky: true,
    };

    /// Standard full solid cube (stone, dirt, etc.).
    pub const SOLID: Self = Self {
        is_loaded: true,
        is_air: false,
        is_trapdoor_like: false,
        is_powder_snow: false,
        is_damaging: false,
        is_honey: false,
        is_cocoa: false,
        is_damage_cautious: false,
        is_lava: false,
        is_water: false,
        is_burning: false,
        is_door_open: false,
        is_wood_door_closed: false,
        is_iron_door_closed: false,
        is_rail: false,
        is_leaves: false,
        is_fence_or_wall: false,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: Some(1.0),
        can_see_sky: false,
    };

    /// Unloaded chunk sentinel (Paper: `getBlockStateIfLoaded == null`).
    pub const UNLOADED: Self = Self {
        is_loaded: false,
        ..Self::SOLID
    };

    /// Standard water source/flowing block.
    pub const WATER: Self = Self {
        is_loaded: true,
        is_air: false,
        is_water: true,
        is_pathfindable_land: true,
        is_pathfindable_water: true,
        collision_max_y: None,
        can_see_sky: true,
        ..Self::AIR
    };

    /// Standard lava block.
    pub const LAVA: Self = Self {
        is_loaded: true,
        is_air: false,
        is_lava: true,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: None,
        ..Self::AIR
    };

    /// Standard fence/wall (collision height 1.5).
    pub const FENCE: Self = Self {
        is_loaded: true,
        is_air: false,
        is_fence_or_wall: true,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: Some(1.5),
        ..Self::AIR
    };

    /// Standard fire/burning block.
    pub const FIRE: Self = Self {
        is_loaded: true,
        is_air: false,
        is_burning: true,
        is_pathfindable_land: true,
        is_pathfindable_water: false,
        collision_max_y: None,
        ..Self::AIR
    };

    /// Standard cactus / sweet berry bush.
    pub const DAMAGING: Self = Self {
        is_loaded: true,
        is_air: false,
        is_damaging: true,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: Some(1.0),
        ..Self::AIR
    };

    /// Standard closed wooden door.
    pub const WOOD_DOOR_CLOSED: Self = Self {
        is_loaded: true,
        is_air: false,
        is_wood_door_closed: true,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: Some(1.0),
        ..Self::AIR
    };

    /// Standard open door.
    pub const DOOR_OPEN: Self = Self {
        is_loaded: true,
        is_air: false,
        is_door_open: true,
        is_pathfindable_land: true,
        is_pathfindable_water: false,
        collision_max_y: None,
        ..Self::AIR
    };

    /// Standard closed iron door.
    pub const IRON_DOOR_CLOSED: Self = Self {
        is_loaded: true,
        is_air: false,
        is_iron_door_closed: true,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: Some(1.0),
        ..Self::AIR
    };

    /// Exact port of `WalkNodeEvaluator.getPathTypeFromState(BlockGetter, BlockPos)`
    /// in Paper 26.2.
    #[must_use]
    pub const fn path_type_from_state(self) -> PathType {
        if !self.is_loaded {
            return PathType::Blocked;
        }
        if self.is_air {
            return PathType::Open;
        }
        if self.is_trapdoor_like {
            return PathType::Trapdoor;
        }
        if self.is_powder_snow {
            return PathType::PowderSnow;
        }
        if self.is_damaging {
            return PathType::DamageOther;
        }
        if self.is_honey {
            return PathType::StickyHoney;
        }
        if self.is_cocoa {
            return PathType::Cocoa;
        }
        if self.is_damage_cautious {
            return PathType::DamageCautious;
        }
        if self.is_lava {
            return PathType::Lava;
        }
        if self.is_burning {
            return PathType::DamageFire;
        }
        if self.is_door_open {
            return PathType::DoorOpen;
        }
        if self.is_wood_door_closed {
            return PathType::DoorWoodClosed;
        }
        if self.is_iron_door_closed {
            return PathType::DoorIronClosed;
        }
        if self.is_rail {
            return PathType::Rail;
        }
        if self.is_leaves {
            return PathType::Leaves;
        }
        if self.is_fence_or_wall {
            return PathType::Fence;
        }
        if !self.is_pathfindable_land {
            return PathType::Blocked;
        }
        if self.is_water {
            PathType::Water
        } else {
            PathType::Open
        }
    }

    #[must_use]
    pub const fn has_collision(self) -> bool {
        self.collision_max_y.is_some()
    }
}

/// Read-only world view used by pathfinding evaluators, navigation, and controls.
pub trait PathWorldView {
    fn min_y(&self) -> i32 {
        -64
    }

    fn max_y(&self) -> i32 {
        320
    }

    fn sea_level(&self) -> i32 {
        63
    }

    fn is_bright_outside(&self) -> bool {
        false
    }

    fn block_info(&self, pos: BlockPos) -> BlockPathInfo;

    /// Returns `true` if `aabb` does not intersect any block collision shape.
    ///
    /// Default implementation checks every integer block cell overlapped by `aabb`
    /// using `block_info(pos).collision_max_y`.
    fn is_aabb_collision_free(&self, aabb: Aabb) -> bool {
        let min_x = mth::floor_d(aabb.min_x + 1.0e-7);
        let max_x = mth::floor_d(aabb.max_x - 1.0e-7);
        let min_y = mth::floor_d(aabb.min_y + 1.0e-7);
        let max_y = mth::floor_d(aabb.max_y - 1.0e-7);
        let min_z = mth::floor_d(aabb.min_z + 1.0e-7);
        let max_z = mth::floor_d(aabb.max_z - 1.0e-7);
        // Inspect one block below min_y so protruding shapes (fences/walls with height 1.5)
        // are included, matching Paper's BlockCollisions.
        let start_y = min_y.saturating_sub(1);

        for x in min_x..=max_x {
            for y in start_y..=max_y {
                for z in min_z..=max_z {
                    let info = self.block_info(BlockPos::new(x, y, z));
                    if !info.is_loaded {
                        if y >= min_y {
                            return false;
                        }
                        continue;
                    }
                    if let Some(max_local_y) = info.collision_max_y {
                        let block_top = y as f64 + max_local_y;
                        if aabb.min_y < block_top - 1.0e-7 && aabb.max_y > y as f64 + 1.0e-7 {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Mirrors `PathNavigation.isClearForMovementBetween(Mob, Vec3, Vec3, boolean)`.
    fn is_clear_for_movement_between(&self, from: Vec3, to: Vec3, include_fluids: bool) -> bool {
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dz = to.z - from.z;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        let steps = mth::ceil_d(dist * 2.0).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let pos = BlockPos::new(
                mth::floor_d(from.x + dx * t),
                mth::floor_d(from.y + dy * t),
                mth::floor_d(from.z + dz * t),
            );
            let info = self.block_info(pos);
            if !info.is_loaded || info.has_collision() {
                return false;
            }
            if include_fluids && (info.is_water || info.is_lava) {
                return false;
            }
        }
        true
    }
}

/// Port of `net.minecraft.world.level.pathfinder.PathfindingContext` in Paper 26.2.
pub struct PathfindingContext<'a, W: PathWorldView + ?Sized> {
    level: &'a W,
    cache: Option<&'a mut PathTypeCache>,
    mob_position: BlockPos,
}

impl<'a, W: PathWorldView + ?Sized> PathfindingContext<'a, W> {
    pub fn new(level: &'a W, cache: Option<&'a mut PathTypeCache>, mob_position: BlockPos) -> Self {
        Self {
            level,
            cache,
            mob_position,
        }
    }

    #[must_use]
    pub fn level(&self) -> &W {
        self.level
    }

    #[must_use]
    pub const fn mob_position(&self) -> BlockPos {
        self.mob_position
    }

    /// Mirrors `PathfindingContext.getPathTypeFromState(int x, int y, int z)`.
    pub fn get_path_type_from_state(&mut self, x: i32, y: i32, z: i32) -> PathType {
        let pos = BlockPos::new(x, y, z);
        if let Some(cache) = self.cache.as_deref_mut() {
            cache.get_or_compute(pos, |p| self.level.block_info(p).path_type_from_state())
        } else {
            self.level.block_info(pos).path_type_from_state()
        }
    }

    #[must_use]
    pub fn get_block_info(&self, pos: BlockPos) -> BlockPathInfo {
        self.level.block_info(pos)
    }
}

/// Tick-local mob state read/mutated by pathfinding evaluators, navigation, and controls.
///
/// Mirrors the `Mob` / `LivingEntity` fields accessed by Paper 26.2's
/// `NodeEvaluator`, `PathNavigation`, `MoveControl`, `LookControl`,
/// `JumpControl`, and `BodyRotationControl`.
#[derive(Debug, Clone)]
pub struct MobPathingState {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub bb_width: f32,
    pub bb_height: f32,
    pub eye_height: f32,
    pub max_up_step: f32,
    pub max_fall_distance: i32,
    pub on_ground: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub is_passenger: bool,
    pub affected_by_fluids: bool,
    pub is_baby: bool,
    pub can_stand_on_fluid: bool,
    pub can_stand_on_powder_snow: bool,
    pub is_dolphin: bool,
    pub is_sun_sensitive: bool,
    pub has_helmet: bool,
    pub movement_speed: f64,
    pub flying_speed: f64,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_body_rot: f32,
    pub y_head_rot: f32,
    pub max_head_y_rot: f32,
    pub max_head_x_rot: f32,
    pub head_rot_speed: f32,
    pub zza: f32,
    pub xxa: f32,
    pub yya: f32,
    pub speed: f32,
    pub no_gravity: bool,
    pub jumping: bool,
    pub delta_movement: Vec3,
    pub has_mob_passenger: bool,
    malus_overrides: [Option<f32>; PathType::COUNT],
}

impl Default for MobPathingState {
    fn default() -> Self {
        Self::new(Vec3::ZERO, 0.6, 1.95)
    }
}

impl MobPathingState {
    #[must_use]
    pub fn new(pos: Vec3, bb_width: f32, bb_height: f32) -> Self {
        Self {
            pos,
            prev_pos: pos,
            bb_width,
            bb_height,
            eye_height: bb_height * 0.85,
            max_up_step: 0.6,
            max_fall_distance: 3,
            on_ground: true,
            in_water: false,
            in_lava: false,
            is_passenger: false,
            affected_by_fluids: true,
            is_baby: false,
            can_stand_on_fluid: false,
            can_stand_on_powder_snow: false,
            is_dolphin: false,
            is_sun_sensitive: false,
            has_helmet: false,
            movement_speed: 0.25,
            flying_speed: 0.4,
            y_rot: 0.0,
            x_rot: 0.0,
            y_body_rot: 0.0,
            y_head_rot: 0.0,
            max_head_y_rot: 75.0,
            max_head_x_rot: 40.0,
            head_rot_speed: 10.0,
            zza: 0.0,
            xxa: 0.0,
            yya: 0.0,
            speed: 0.0,
            no_gravity: false,
            jumping: false,
            delta_movement: Vec3::ZERO,
            has_mob_passenger: false,
            malus_overrides: [None; PathType::COUNT],
        }
    }

    /// Mirrors `Entity.getBoundingBox()`.
    #[must_use]
    pub fn bounding_box(&self) -> Aabb {
        let half = f64::from(self.bb_width) * 0.5;
        Aabb::new(
            self.pos.x - half,
            self.pos.y,
            self.pos.z - half,
            self.pos.x + half,
            self.pos.y + f64::from(self.bb_height),
            self.pos.z + half,
        )
    }

    /// Mirrors `Entity.blockPosition()`.
    #[must_use]
    pub fn block_position(&self) -> BlockPos {
        BlockPos::new(
            mth::floor_d(self.pos.x),
            mth::floor_d(self.pos.y),
            mth::floor_d(self.pos.z),
        )
    }

    #[must_use]
    pub fn eye_y(&self) -> f64 {
        self.pos.y + f64::from(self.eye_height)
    }

    #[must_use]
    pub const fn is_in_liquid(&self) -> bool {
        self.in_water || self.in_lava
    }

    /// Mirrors `Mob.getPathfindingMalus(PathType)`.
    #[must_use]
    pub fn get_pathfinding_malus(&self, path_type: PathType) -> f32 {
        self.malus_overrides[path_type.index()].unwrap_or_else(|| path_type.malus())
    }

    /// Mirrors `Mob.setPathfindingMalus(PathType, float)`.
    pub fn set_pathfinding_malus(&mut self, path_type: PathType, malus: f32) {
        self.malus_overrides[path_type.index()] = Some(malus);
    }

    /// Mirrors `LivingEntity.setSpeed(float)` which sets `speed` and `zza = speed`.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
        self.zza = speed;
    }
}

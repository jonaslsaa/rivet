//! No Java counterpart: the world and mob seam the pathfinder and navigation read.
//!
//! Tick-thread-confined world and mob seam types for pathfinding and navigation.
//!
//! Per `OWNERSHIP.md`, pathfinding and navigation take `&dyn PathWorldView` and
//! `&MobPathingState` / `&mut MobPathingState` rather than holding `Arc<RwLock>`
//! references to worlds or entities.

use rivet_util::mth;

use crate::world::entity::ai::goal::context::{Aabb, BlockPos, Vec3};
use crate::world::level::pathfinder::path_type::{PathType, PathTypeCache};

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
    /// `fluidState.is(FluidTags.WATER)` (includes waterlogged blocks).
    pub is_water: bool,
    /// `blockState.is(Blocks.WATER)`: the water block itself, not waterlogged blocks.
    pub is_water_block: bool,
    /// `WalkNodeEvaluator.isBurningBlock(BlockState)`.
    pub is_burning: bool,
    pub is_door_open: bool,
    pub is_wood_door_closed: bool,
    pub is_iron_door_closed: bool,
    pub is_rail: bool,
    pub is_leaves: bool,
    /// `BlockTags.FENCES`.
    pub is_fence: bool,
    /// `BlockTags.WALLS`.
    pub is_wall: bool,
    /// `FenceGateBlock` with `OPEN = false`.
    pub is_closed_fence_gate: bool,
    /// `BlockTags.CAULDRONS`.
    pub is_cauldron: bool,
    /// `blockState.isSolid()` (the legacy-solid flag).
    pub is_solid: bool,
    /// `blockState.isSolidRender()`.
    // RivetTodo(#781) isSolidRender is a host-supplied flag; real BlockState.isSolidRender (occlusion shape) is not ported.
    pub is_solid_render: bool,
    /// `blockState.entityCanStandOn(level, pos, mob)`: whether the block's collision
    /// shape under the navigating mob's `CollisionContext` has a full `UP` face.
    pub entity_can_stand_on: bool,
    /// `blockState.isPathfindable(PathComputationType.LAND)`.
    pub is_pathfindable_land: bool,
    /// `blockState.isPathfindable(PathComputationType.WATER)`.
    pub is_pathfindable_water: bool,
    /// Maximum Y of the block's collision `VoxelShape` in local `[0.0, 1.0+]` space,
    /// or `None` if `collisionShape.isEmpty()`.
    // RivetTodo(#781) collision VoxelShapes are reduced to a [0,1]x[0,maxY]x[0,1] box; real block shapes under the mob's CollisionContext come with the level services.
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
        is_water_block: false,
        is_burning: false,
        is_door_open: false,
        is_wood_door_closed: false,
        is_iron_door_closed: false,
        is_rail: false,
        is_leaves: false,
        is_fence: false,
        is_wall: false,
        is_closed_fence_gate: false,
        is_cauldron: false,
        is_solid: false,
        is_solid_render: false,
        entity_can_stand_on: false,
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
        is_water_block: false,
        is_burning: false,
        is_door_open: false,
        is_wood_door_closed: false,
        is_iron_door_closed: false,
        is_rail: false,
        is_leaves: false,
        is_fence: false,
        is_wall: false,
        is_closed_fence_gate: false,
        is_cauldron: false,
        is_solid: true,
        is_solid_render: true,
        entity_can_stand_on: true,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        collision_max_y: Some(1.0),
        can_see_sky: false,
    };

    /// A position whose chunk is missing from the `PathNavigationRegion`.
    ///
    /// Only Paper's patched `getPathTypeFromState` sees the chunk as missing
    /// (`getBlockStateIfLoaded == null` gives `BLOCKED`); every other region read
    /// goes through `EmptyLevelChunk`, so the remaining facts are those of `AIR`
    /// with no collision and no fluid.
    pub const UNLOADED: Self = Self {
        is_loaded: false,
        ..Self::AIR
    };

    /// Standard water source/flowing block.
    pub const WATER: Self = Self {
        is_loaded: true,
        is_air: false,
        is_water: true,
        is_water_block: true,
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

    /// Standard fence (collision height 1.5).
    pub const FENCE: Self = Self {
        is_loaded: true,
        is_air: false,
        is_fence: true,
        is_solid: true,
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
        is_solid: true,
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
        is_solid: true,
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
        is_solid: true,
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
        if self.is_fence || self.is_wall || self.is_closed_fence_gate {
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

    /// `blockState.is(BlockTags.DOORS)`: every `DoorBlock`, open or closed.
    #[must_use]
    pub const fn is_door(self) -> bool {
        self.is_door_open || self.is_wood_door_closed || self.is_iron_door_closed
    }
}

/// Read-only world view used by pathfinding evaluators, navigation, and controls.
///
/// A view serves one navigating mob: entity-dependent facts
/// ([`BlockPathInfo::entity_can_stand_on`], [`PathWorldView::clip_hits_block`])
/// use that mob's `CollisionContext`.
pub trait PathWorldView {
    fn min_y(&self) -> i32 {
        -64
    }

    /// `Level.getMaxY()`: the highest buildable block Y (inclusive).
    fn max_y(&self) -> i32 {
        319
    }

    fn sea_level(&self) -> i32 {
        63
    }

    fn block_info(&self, pos: BlockPos) -> BlockPathInfo;

    /// `PathNavigationRegion.noCollision(mob, aabb)`, the test inside
    /// `WalkNodeEvaluator.hasCollisions`.
    ///
    /// Returns `true` when `aabb` intersects no block collision `VoxelShape`
    /// (each block's real shape under the navigating mob's `CollisionContext`,
    /// read through `getChunkForCollisions`, so cells in chunks missing from the
    /// region are `EmptyLevelChunk` air and add nothing) and, per
    /// `noBorderCollision`, the world border's collision shape when
    /// `WorldBorder.isInsideCloseToBorder(mob, aabb)`. The region contributes no
    /// entity collisions.
    // RivetTodo(#781) real VoxelShape collision, WorldBorder.isInsideCloseToBorder/noBorderCollision and getChunkForCollisions unloaded-chunk semantics are provided by the PathWorldView implementor, not ported here.
    fn is_aabb_collision_free(&self, aabb: Aabb) -> bool;

    /// Paper's `WorldBorder.isWithinBounds(BlockPos)`:
    /// `x >= minX && x < maxX && z >= minZ && z < maxZ` on the block coordinates.
    // RivetTodo(#781) world border state is supplied by the host; the real WorldBorder is not ported.
    fn is_within_world_border(&self, pos: BlockPos) -> bool;

    /// One cell of Paper's `Level.clip` (Moonrise `fastClip`) for a
    /// `ClipContext(from, to, Block.COLLIDER, fluid, mob)`.
    ///
    /// Returns `true` when the cell at `pos` produces a hit: the block's
    /// collision shape under the navigating mob's `CollisionContext` clips the
    /// segment `from..to` (`VoxelShape.clip(from, to, pos) != null`), or
    /// `blocked_by_fluids` (`ClipContext.Fluid.ANY`) and the cell's fluid shape
    /// clips it. Air and positions outside the build height never hit. `Level.clip`
    /// reads chunks through `Level.getChunk`, so the implementation sees the
    /// level's real block at `pos`, never the region's unloaded sentinel.
    // RivetTodo(#781) VoxelShape.clip and fluid shapes come from the host; Level.clip chunk access is not ported.
    fn clip_hits_block(&self, pos: BlockPos, from: Vec3, to: Vec3, blocked_by_fluids: bool)
    -> bool;

    /// `Level.clip(new ClipContext(from, to, Block.COLLIDER, fluid, mob)).getType() == MISS`,
    /// the clip inside `PathNavigation.isClearForMovementBetween`.
    ///
    /// Exact port of Paper 26.2's Moonrise `Level.fastClip` voxel walk; each visited
    /// cell is tested with [`Self::clip_hits_block`] and the first hit ends the walk.
    fn is_clear_for_movement_between(&self, from: Vec3, to: Vec3, blocked_by_fluids: bool) -> bool {
        // `CollisionUtil.COLLISION_EPSILON`
        const COLLISION_EPSILON: f64 = 1.0e-7;
        let adj_x = COLLISION_EPSILON * (from.x - to.x);
        let adj_y = COLLISION_EPSILON * (from.y - to.y);
        let adj_z = COLLISION_EPSILON * (from.z - to.z);
        if adj_x == 0.0 && adj_y == 0.0 && adj_z == 0.0 {
            return true;
        }

        let to_x_adj = to.x - adj_x;
        let to_y_adj = to.y - adj_y;
        let to_z_adj = to.z - adj_z;
        let from_x_adj = from.x + adj_x;
        let from_y_adj = from.y + adj_y;
        let from_z_adj = from.z + adj_z;

        let mut curr_x = mth::floor_d(from_x_adj);
        let mut curr_y = mth::floor_d(from_y_adj);
        let mut curr_z = mth::floor_d(from_z_adj);

        let diff_x = to_x_adj - from_x_adj;
        let diff_y = to_y_adj - from_y_adj;
        let diff_z = to_z_adj - from_z_adj;

        // `Math.signum` then `(int)` truncation.
        let signum = |v: f64| -> f64 {
            if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                v
            }
        };
        let dx_double = signum(diff_x);
        let dy_double = signum(diff_y);
        let dz_double = signum(diff_z);
        let dx = dx_double as i32;
        let dy = dy_double as i32;
        let dz = dz_double as i32;

        let normalized_diff_x = if diff_x == 0.0 {
            f64::MAX
        } else {
            dx_double / diff_x
        };
        let normalized_diff_y = if diff_y == 0.0 {
            f64::MAX
        } else {
            dy_double / diff_y
        };
        let normalized_diff_z = if diff_z == 0.0 {
            f64::MAX
        } else {
            dz_double / diff_z
        };

        let frac = mth::frac_f64;
        let mut normalized_curr_x = normalized_diff_x
            * if diff_x > 0.0 {
                1.0 - frac(from_x_adj)
            } else {
                frac(from_x_adj)
            };
        let mut normalized_curr_y = normalized_diff_y
            * if diff_y > 0.0 {
                1.0 - frac(from_y_adj)
            } else {
                frac(from_y_adj)
            };
        let mut normalized_curr_z = normalized_diff_z
            * if diff_z > 0.0 {
                1.0 - frac(from_z_adj)
            } else {
                frac(from_z_adj)
            };

        loop {
            if self.clip_hits_block(
                BlockPos::new(curr_x, curr_y, curr_z),
                from,
                to,
                blocked_by_fluids,
            ) {
                return false;
            }

            if normalized_curr_x > 1.0 && normalized_curr_y > 1.0 && normalized_curr_z > 1.0 {
                return true;
            }

            if normalized_curr_x < normalized_curr_y {
                if normalized_curr_x < normalized_curr_z {
                    curr_x = curr_x.wrapping_add(dx);
                    normalized_curr_x += normalized_diff_x;
                } else {
                    curr_z = curr_z.wrapping_add(dz);
                    normalized_curr_z += normalized_diff_z;
                }
            } else if normalized_curr_y < normalized_curr_z {
                curr_y = curr_y.wrapping_add(dy);
                normalized_curr_y += normalized_diff_y;
            } else {
                curr_z = curr_z.wrapping_add(dz);
                normalized_curr_z += normalized_diff_z;
            }
        }
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
    /// `Strider.canStandOnFluid(FluidState)` (`fluid.is(FluidTags.LAVA)`); the
    /// `LivingEntity` default stands on no fluid.
    pub can_stand_on_lava: bool,
    pub is_dolphin: bool,
    pub is_sun_sensitive: bool,
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
    /// `Mob.onPathfindingStart()`, called by `WalkNodeEvaluator.prepare` and
    /// `FlyNodeEvaluator.prepare`. The `Mob` default does nothing; `Sniffer`
    /// overrides it.
    pub on_pathfinding_start: fn(&mut MobPathingState),
    /// `Mob.onPathfindingDone()`, called by `WalkNodeEvaluator.done` and
    /// `FlyNodeEvaluator.done`. The `Mob` default does nothing; `Sniffer`
    /// overrides it.
    pub on_pathfinding_done: fn(&mut MobPathingState),
    malus_overrides: [Option<f32>; PathType::COUNT],
}

/// The `Mob.onPathfindingStart()` / `Mob.onPathfindingDone()` default.
fn no_pathfinding_hook(_mob: &mut MobPathingState) {}

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
            can_stand_on_lava: false,
            is_dolphin: false,
            is_sun_sensitive: false,
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
            on_pathfinding_start: no_pathfinding_hook,
            on_pathfinding_done: no_pathfinding_hook,
            malus_overrides: [None; PathType::COUNT],
        }
    }

    /// Mirrors `Entity.setYRot(float)`: a non-finite value is discarded.
    pub fn set_y_rot(&mut self, y_rot: f32) {
        if y_rot.is_finite() {
            self.y_rot = y_rot;
        }
    }

    /// Mirrors `Entity.setXRot(float)`: a non-finite value is discarded, otherwise
    /// `xRot = Math.clamp(xRot % 360.0F, -90.0F, 90.0F)`.
    pub fn set_x_rot(&mut self, x_rot: f32) {
        if x_rot.is_finite() {
            self.x_rot = (x_rot % 360.0).clamp(-90.0, 90.0);
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

    /// Mirrors `LivingEntity.canStandOnFluid(FluidState)` for the fluid at `info`.
    #[must_use]
    pub const fn can_stand_on_fluid(&self, info: BlockPathInfo) -> bool {
        self.can_stand_on_lava && info.is_lava
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

    /// Mirrors `Mob.onPathfindingStart()`.
    pub fn on_pathfinding_start(&mut self) {
        (self.on_pathfinding_start)(self);
    }

    /// Mirrors `Mob.onPathfindingDone()`.
    pub fn on_pathfinding_done(&mut self) {
        (self.on_pathfinding_done)(self);
    }

    /// Mirrors `LivingEntity.setSpeed(float)` which sets `speed` and `zza = speed`.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
        self.zza = speed;
    }
}

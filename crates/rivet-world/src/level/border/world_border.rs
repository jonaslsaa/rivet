//! `net.minecraft.world.level.border.WorldBorder` — the world border: the
//! movable rectangle that bounds a world (issue: the `mc.world.level.border`
//! manifest unit).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! border/WorldBorder.java` (673 lines, 26.2).
//!
//! ## Structure
//!
//! `WorldBorder extends SavedData`. It holds a `Settings` record (the
//! level.dat codec shape) plus a live `BorderExtent` that is either
//! [`StaticBorderExtent`] (fixed size) or [`MovingBorderExtent`] (a timed
//! lerp between `from` and `to`). The border's horizontal bounds are
//! `getMinX()/getMaxX()/getMinZ()/getMaxZ()`, all clamped to
//! `absoluteMaxSize`.
//!
//! ## The `Settings.CODEC` (9 fields)
//!
//! Java's `Settings.CODEC` is a `RecordCodecBuilder.create` over nine fields
//! in declaration order: `center_x`, `center_z` (each `Codec.doubleRange(-
//! 2.9999984E7, 2.9999984E7)`), `damage_per_block`, `safe_zone`,
//! `warning_blocks`, `warning_time`, `size`, `lerp_time`, `lerp_target`. The
//! Rivet `record_builder` arity chain caps at six, so the record is composed
//! as 6+3: a nested six-field group builds a `MapCodec<(f64, f64, f64, f64,
//! i32, i32), Ops>`, and the outer group combines that tuple with the
//! remaining three fields. The encode order matches the single Java chain, but
//! the decode error accumulation does not exactly: Java's flat `ap9` treats
//! each of the nine fields as an independent operand (a missing field yields
//! one flat per-field error list, and the partial value carries whatever
//! subset succeeded), whereas the 6+3 nesting treats the six-tuple decoder as
//! one operand — if any inner field fails the whole tuple errors and the inner
//! partial successes are dropped, and the error report nests the six as one
//! entry. Well-formed round-trips are unaffected; the deviation is only the
//! malformed-input error/partial structure (hostile-input parity), accepted
//! while `record_builder` caps at six (see [`Settings::codec`]).
//!
//! `WorldBorder.CODEC = Settings.CODEC.xmap(WorldBorder::new,
//! Settings::new)` — the `Settings(WorldBorder)` compact constructor reads the
//! live values (`Settings::from_world_border`).
//!
//! ## Paper additions
//!
//! Paper adds: the `isBlockInBounds`/`isChunkInBounds` helpers (ported) and
//! the re-added `applySettings` (ported), plus the nullable `ServerLevel world`
//! field feeding the `io.papermc.paper.event.world.border.*` plugin events
//! fired in `setCenter`/`setSize`/`lerpSizeBetween`/
//! `MovingBorderExtent.update()` and the `lastTick` dedupe guard in `tick()`
//! (against `MinecraftServer.currentTick`).
//!
//! RivetTodo(#417): the Paper plugin-event firing and the `lastTick` dedupe
//! guard defer — they need the `ServerLevel`/`MinecraftServer.currentTick`
//! seam and the Paper event-bridge layer (#501); the event calls are omitted
//! and `tick()`/`applySettings` run the vanilla bodies.
//!
//! ## Cross-unit seams
//!
//! - `SavedData`/`SavedDataType`/`DataFixTypes` — the merged
//!   `mc.world.level.saveddata`/`mc.util.datafix` units
//!   (`crate::level::saveddata`); `WorldBorder extends SavedData` and exposes
//!   the [`TYPE`] `SavedDataType` static (pinned to `NbtOps`).
//! - `VoxelShape`/`Shapes`/`BooleanOp` — pending `mc.world.phys.shapes` unit;
//!   the [`shapes`] seam carries the `STUB(mc.world.phys.shapes)` marker.
//! - `Entity` — pending `mc.world.entity` unit; minimal `getX`/`getZ` handle
//!   in [`entity_stub`] (`// STUB(mc.world.entity)`).
//! - `AABB` — `rivet_util::mth_stubs::Aabb` (only the six fields); the
//!   `getXsize`/`getZsize` reads are inlined (`max_x - min_x`).

pub use crate::level::border::entity_stub::Entity;
pub use crate::level::border::shapes::Shapes;
pub use crate::level::border::shapes::VoxelShape;
use crate::level::saveddata::saved_data::SavedData;
use crate::level::saveddata::saved_data_type::SavedDataType;
use crate::level::saveddata::stub_data_fix_types::DataFixTypes;
use rivet_registry::Identifier;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth::{clamp_f64, lerp, max_f64, min_f64};
use rivet_util::mth_stubs::{Aabb, Vec3};
use std::sync::{Arc, LazyLock};

/// `WorldBorder.MAX_SIZE` — `5.999997E7F` (a **float** literal, widened).
/// The float nearest `59999970.0` is `59999968.0`, so the border's default
/// size is exactly `59999968.0`.
pub const MAX_SIZE: f64 = 5.999997e7_f32 as f64;

/// `WorldBorder.MAX_CENTER_COORDINATE` — `2.9999984E7` (a double literal).
pub const MAX_CENTER_COORDINATE: f64 = 2.9999984E7;

/// The `1.0E-5F` float literal (widened to double) used by the bounds checks
/// — `aabb.maxX - 1.0E-5F`, `getMaxX() - 1.0E-5F`. Exact value:
/// `9.999999747378752e-6`.
const EPSILON: f64 = 1.0e-5_f32 as f64;

/// `WorldBorder` — `extends SavedData`.
pub struct WorldBorder {
    /// `settings` — the immutable `Settings` this border was constructed with.
    settings: Settings,
    /// `initialized` — set once `applyInitialSettings` has run.
    initialized: bool,
    /// `listeners` — the `BorderChangeListener`s (copied before notification).
    listeners: Vec<Arc<dyn crate::level::border::border_change_listener::BorderChangeListener>>,
    /// `damagePerBlock` — default `0.2`.
    damage_per_block: f64,
    /// `safeZone` — default `5.0`.
    safe_zone: f64,
    /// `warningTime` — default `15`.
    warning_time: i32,
    /// `warningBlocks` — default `5`.
    warning_blocks: i32,
    /// `centerX` — default `0.0`.
    center_x: f64,
    /// `centerZ` — default `0.0`.
    center_z: f64,
    /// `absoluteMaxSize` — default `29999984`.
    absolute_max_size: i32,
    /// `extent` — `new StaticBorderExtent(5.999997E7F)`.
    extent: BorderExtent,
    /// The `SavedData` supertype state (the `dirty` flag).
    saved_data: SavedData,
    // `world` (ServerLevel, CraftBukkit) and `lastTick` (Paper) are deferred —
    // the ServerLevel seam and the `MinecraftServer.currentTick` seam live
    // outside this unit (see the module doc).
}

impl std::fmt::Debug for WorldBorder {
    /// The listener trait objects are not `Debug`; the live state is what the
    /// border is observed by.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldBorder")
            .field("settings", &self.settings)
            .field("initialized", &self.initialized)
            .field("listener_count", &self.listeners.len())
            .field("damage_per_block", &self.damage_per_block)
            .field("safe_zone", &self.safe_zone)
            .field("warning_time", &self.warning_time)
            .field("warning_blocks", &self.warning_blocks)
            .field("center_x", &self.center_x)
            .field("center_z", &self.center_z)
            .field("absolute_max_size", &self.absolute_max_size)
            .field("extent", &self.extent)
            .finish()
    }
}

impl WorldBorder {
    /// `new WorldBorder()` — `this(WorldBorder.Settings.DEFAULT)`.
    #[allow(clippy::should_implement_trait)] // Java's `new WorldBorder()` no-arg constructor.
    pub fn default() -> WorldBorder {
        WorldBorder::new(Settings::default_settings())
    }

    /// `new WorldBorder(WorldBorder.Settings)` — stores the settings; the live
    /// fields keep their defaults until `applyInitialSettings`/`applySettings`.
    pub fn new(settings: Settings) -> WorldBorder {
        WorldBorder {
            settings,
            initialized: false,
            listeners: Vec::new(),
            damage_per_block: 0.2,
            safe_zone: 5.0,
            warning_time: 15,
            warning_blocks: 5,
            center_x: 0.0,
            center_z: 0.0,
            absolute_max_size: 29999984,
            extent: BorderExtent::Static(StaticBorderExtent::new(MAX_SIZE, 0.0, 0.0, 29999984)),
            saved_data: SavedData::default(),
        }
    }

    /// `isWithinBounds(BlockPos)` — `isWithinBounds(pos.getX(), pos.getZ())`.
    pub fn is_within_bounds(&self, pos: &BlockPos) -> bool {
        self.is_within_bounds_xy(pos.get_x() as f64, pos.get_z() as f64)
    }

    /// `isWithinBounds(Vec3)` — `isWithinBounds(pos.x, pos.z)`.
    pub fn is_within_bounds_vec3(&self, pos: &Vec3) -> bool {
        self.is_within_bounds_xy(pos.x, pos.z)
    }

    /// `isWithinBounds(ChunkPos)` — both the min and the max block corners
    /// must be within bounds.
    pub fn is_within_bounds_chunk(&self, pos: &ChunkPos) -> bool {
        self.is_within_bounds_xy(pos.get_min_block_x() as f64, pos.get_min_block_z() as f64)
            && self.is_within_bounds_xy(pos.get_max_block_x() as f64, pos.get_max_block_z() as f64)
    }

    // Paper start - Bound treasure maps to world border
    // Paper's `isBlockInBounds`/`isChunkInBounds` reuse a single mutable
    // `BlockPos.MutableBlockPos` to box the coordinates; since `isWithinBounds`
    // only reads x/z, the box is dropped and the ints are checked directly
    // (observably identical).
    /// `isBlockInBounds(int x, int z)` (Paper).
    pub fn is_block_in_bounds(&self, x: i32, z: i32) -> bool {
        self.is_within_bounds_xy(x as f64, z as f64)
    }

    /// `isChunkInBounds(int chunkX, int chunkZ)` (Paper) — the
    /// `(chunkX << 4) + 15` corner (Java int shifts/adds wrap).
    pub fn is_chunk_in_bounds(&self, chunk_x: i32, chunk_z: i32) -> bool {
        let x = chunk_x.wrapping_shl(4).wrapping_add(15);
        let z = chunk_z.wrapping_shl(4).wrapping_add(15);
        self.is_within_bounds_xy(x as f64, z as f64)
    }
    // Paper end - Bound treasure maps to world border

    /// `isWithinBounds(AABB)` — the `aabb.maxX - 1.0E-5F` float-literal
    /// corners.
    pub fn is_within_bounds_aabb(&self, aabb: &Aabb) -> bool {
        self.is_within_bounds_corners(
            aabb.min_x,
            aabb.min_z,
            aabb.max_x - EPSILON,
            aabb.max_z - EPSILON,
        )
    }

    /// `isWithinBounds(double, double, double, double)` — both opposite corners
    /// must be within bounds.
    fn is_within_bounds_corners(&self, min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> bool {
        self.is_within_bounds_margin(min_x, min_z, 0.0)
            && self.is_within_bounds_margin(max_x, max_z, 0.0)
    }

    /// `isWithinBounds(double x, double z)` — `isWithinBounds(x, z, 0.0)`.
    pub fn is_within_bounds_xy(&self, x: f64, z: f64) -> bool {
        self.is_within_bounds_margin(x, z, 0.0)
    }

    /// `isWithinBounds(double x, double z, double margin)` — the half-open
    /// `[min, max)` test (`x < getMaxX() + margin` is strict).
    pub fn is_within_bounds_margin(&self, x: f64, z: f64, margin: f64) -> bool {
        x >= self.get_min_x() - margin
            && x < self.get_max_x() + margin
            && z >= self.get_min_z() - margin
            && z < self.get_max_z() + margin
    }

    /// `clampToBounds(BlockPos)`.
    pub fn clamp_to_bounds(&self, position: &BlockPos) -> BlockPos {
        self.clamp_to_bounds_xyz(
            position.get_x() as f64,
            position.get_y() as f64,
            position.get_z() as f64,
        )
    }

    /// `clampToBounds(Vec3)`.
    pub fn clamp_to_bounds_vec3(&self, position: &Vec3) -> BlockPos {
        self.clamp_to_bounds_xyz(position.x, position.y, position.z)
    }

    /// `clampToBounds(double x, double y, double z)` —
    /// `BlockPos.containing(clampVec3ToBound(x, y, z))`.
    pub fn clamp_to_bounds_xyz(&self, x: f64, y: f64, z: f64) -> BlockPos {
        let v = self.clamp_vec3_to_bound_xyz(x, y, z);
        BlockPos::containing(v.x, v.y, v.z)
    }

    /// `clampVec3ToBound(Vec3)`.
    pub fn clamp_vec3_to_bound(&self, position: &Vec3) -> Vec3 {
        self.clamp_vec3_to_bound_xyz(position.x, position.y, position.z)
    }

    /// `clampVec3ToBound(double x, double y, double z)` — the `- 1.0E-5F`
    /// upper bound, `y` untouched.
    pub fn clamp_vec3_to_bound_xyz(&self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(
            clamp_f64(x, self.get_min_x(), self.get_max_x() - EPSILON),
            y,
            clamp_f64(z, self.get_min_z(), self.get_max_z() - EPSILON),
        )
    }

    /// `getDistanceToBorder(Entity)`.
    pub fn get_distance_to_border_entity(&self, entity: &Entity) -> f64 {
        self.get_distance_to_border(entity.x, entity.z)
    }

    /// `getCollisionShape()` — the extent's `VoxelShape` (Java reads the
    /// border's live center/absmax via `WorldBorder.this`).
    pub fn get_collision_shape(&self) -> crate::level::border::shapes::VoxelShape {
        self.extent
            .get_collision_shape(self.center_x, self.center_z, self.absolute_max_size)
    }

    /// `getDistanceToBorder(double x, double z)`.
    pub fn get_distance_to_border(&self, x: f64, z: f64) -> f64 {
        let from_north = z - self.get_min_z();
        let from_south = self.get_max_z() - z;
        let from_west = x - self.get_min_x();
        let from_east = self.get_max_x() - x;
        let mut min = min_f64(from_west, from_east);
        min = min_f64(min, from_north);
        min_f64(min, from_south)
    }

    /// `isInsideCloseToBorder(Entity source, AABB boundingBox)`.
    pub fn is_inside_close_to_border(&self, source: &Entity, bounding_box: &Aabb) -> bool {
        // Java: `Math.max(Mth.absMax(getXsize(), getZsize()), 1.0)`; both
        // `Math.max` calls must propagate NaN (Java), so compose with
        // `max_f64` (the NaN-propagating mirror) rather than `f64::max`.
        let bb_max = max_f64(
            max_f64(
                (bounding_box.max_x - bounding_box.min_x).abs(),
                (bounding_box.max_z - bounding_box.min_z).abs(),
            ),
            1.0,
        );
        self.get_distance_to_border_entity(source) < bb_max * 2.0
            && self.is_within_bounds_margin(source.x, source.z, bb_max)
    }

    /// `getStatus()`.
    pub fn get_status(&self) -> crate::level::border::border_status::BorderStatus {
        self.extent.get_status()
    }

    /// `getMinX()` — `getMinX(0.0F)`.
    pub fn get_min_x(&self) -> f64 {
        self.get_min_x_delta(0.0_f32)
    }

    /// `getMinX(float deltaPartialTick)`.
    pub fn get_min_x_delta(&self, delta_partial_tick: f32) -> f64 {
        self.extent
            .get_min_x(delta_partial_tick, self.center_x, self.absolute_max_size)
    }

    /// `getMinZ()` — `getMinZ(0.0F)`.
    pub fn get_min_z(&self) -> f64 {
        self.get_min_z_delta(0.0_f32)
    }

    /// `getMinZ(float deltaPartialTick)`.
    pub fn get_min_z_delta(&self, delta_partial_tick: f32) -> f64 {
        self.extent
            .get_min_z(delta_partial_tick, self.center_z, self.absolute_max_size)
    }

    /// `getMaxX()` — `getMaxX(0.0F)`.
    pub fn get_max_x(&self) -> f64 {
        self.get_max_x_delta(0.0_f32)
    }

    /// `getMaxX(float deltaPartialTick)`.
    pub fn get_max_x_delta(&self, delta_partial_tick: f32) -> f64 {
        self.extent
            .get_max_x(delta_partial_tick, self.center_x, self.absolute_max_size)
    }

    /// `getMaxZ()` — `getMaxZ(0.0F)`.
    pub fn get_max_z(&self) -> f64 {
        self.get_max_z_delta(0.0_f32)
    }

    /// `getMaxZ(float deltaPartialTick)`.
    pub fn get_max_z_delta(&self, delta_partial_tick: f32) -> f64 {
        self.extent
            .get_max_z(delta_partial_tick, self.center_z, self.absolute_max_size)
    }

    /// `getCenterX()`.
    pub fn get_center_x(&self) -> f64 {
        self.center_x
    }

    /// `getCenterZ()`.
    pub fn get_center_z(&self) -> f64 {
        self.center_z
    }

    /// `setCenter(double x, double z)`.
    ///
    /// (Paper: fires `WorldBorderCenterChangeEvent` when `world != null` and
    /// may rewrite `x`/`z`; deferred — see `RivetTodo(#417)` in the module
    /// doc.)
    pub fn set_center(&mut self, x: f64, z: f64) {
        self.center_x = x;
        self.center_z = z;
        self.extent
            .on_center_change(self.center_x, self.center_z, self.absolute_max_size);
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_set_center(self, x, z);
        }
    }

    /// `getSize()`.
    pub fn get_size(&self) -> f64 {
        self.extent.get_size()
    }

    /// `getLerpTime()`.
    pub fn get_lerp_time(&self) -> i64 {
        self.extent.get_lerp_time()
    }

    /// `getLerpTarget()`.
    pub fn get_lerp_target(&self) -> f64 {
        self.extent.get_lerp_target()
    }

    /// `setSize(double size)`.
    ///
    /// (Paper: fires `WorldBorderBoundsChangeEvent` with
    /// `Type.INSTANT_MOVE`, possibly re-routing to `lerpSizeBetween`;
    /// deferred — see `RivetTodo(#417)` in the module doc.)
    pub fn set_size(&mut self, size: f64) {
        let (center_x, center_z, absolute_max_size) =
            (self.center_x, self.center_z, self.absolute_max_size);
        self.extent = BorderExtent::Static(StaticBorderExtent::new(
            size,
            center_x,
            center_z,
            absolute_max_size,
        ));
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_set_size(self, size);
        }
    }

    /// `lerpSizeBetween(double from, double to, long ticks, long gameTime)`.
    ///
    /// (Paper: fires `WorldBorderBoundsChangeEvent` with
    /// `INSTANT_MOVE`/`STARTED_MOVE`; deferred — see `RivetTodo(#417)` in the
    /// module doc.)
    pub fn lerp_size_between(&mut self, from: f64, to: f64, ticks: i64, game_time: i64) {
        let (center_x, center_z, absolute_max_size) =
            (self.center_x, self.center_z, self.absolute_max_size);
        self.extent = if from == to {
            BorderExtent::Static(StaticBorderExtent::new(
                to,
                center_x,
                center_z,
                absolute_max_size,
            ))
        } else {
            BorderExtent::Moving(MovingBorderExtent::new(from, to, ticks, game_time))
        };
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_lerp_size(self, from, to, ticks, game_time);
        }
    }

    /// `getListeners()` — the protected copy (`Lists.newArrayList`).
    fn get_listeners(
        &self,
    ) -> Vec<Arc<dyn crate::level::border::border_change_listener::BorderChangeListener>> {
        self.listeners.clone()
    }

    /// `addListener(BorderChangeListener)` — `contains` uses Java identity
    /// equality (no `equals` override), mirrored by `Arc::ptr_eq`.
    pub fn add_listener(
        &mut self,
        listener: Arc<dyn crate::level::border::border_change_listener::BorderChangeListener>,
    ) {
        // CraftBukkit: the duplicate check (Java `List.contains`).
        if self.listeners.iter().any(|l| Arc::ptr_eq(l, &listener)) {
            return;
        }
        self.listeners.push(listener);
    }

    /// `removeListener(BorderChangeListener)`.
    pub fn remove_listener(
        &mut self,
        listener: &Arc<dyn crate::level::border::border_change_listener::BorderChangeListener>,
    ) {
        self.listeners.retain(|l| !Arc::ptr_eq(l, listener));
    }

    /// `setAbsoluteMaxSize(int)`.
    pub fn set_absolute_max_size(&mut self, absolute_max_size: i32) {
        self.absolute_max_size = absolute_max_size;
        let (center_x, center_z) = (self.center_x, self.center_z);
        self.extent
            .on_absolute_max_size_change(center_x, center_z, self.absolute_max_size);
    }

    /// `getAbsoluteMaxSize()`.
    pub fn get_absolute_max_size(&self) -> i32 {
        self.absolute_max_size
    }

    /// `getSafeZone()`.
    pub fn get_safe_zone(&self) -> f64 {
        self.safe_zone
    }

    /// `setSafeZone(double)`.
    pub fn set_safe_zone(&mut self, safe_zone: f64) {
        self.safe_zone = safe_zone;
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_set_safe_zone(self, safe_zone);
        }
    }

    /// `getDamagePerBlock()`.
    pub fn get_damage_per_block(&self) -> f64 {
        self.damage_per_block
    }

    /// `setDamagePerBlock(double)`.
    pub fn set_damage_per_block(&mut self, damage_per_block: f64) {
        self.damage_per_block = damage_per_block;
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_set_damage_per_block(self, damage_per_block);
        }
    }

    /// `getLerpSpeed()`.
    pub fn get_lerp_speed(&self) -> f64 {
        self.extent.get_lerp_speed()
    }

    /// `getWarningTime()`.
    pub fn get_warning_time(&self) -> i32 {
        self.warning_time
    }

    /// `setWarningTime(int)`.
    pub fn set_warning_time(&mut self, warning_time: i32) {
        self.warning_time = warning_time;
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_set_warning_time(self, warning_time);
        }
    }

    /// `getWarningBlocks()`.
    pub fn get_warning_blocks(&self) -> i32 {
        self.warning_blocks
    }

    /// `setWarningBlocks(int)`.
    pub fn set_warning_blocks(&mut self, warning_blocks: i32) {
        self.warning_blocks = warning_blocks;
        self.saved_data.set_dirty();

        for listener in self.get_listeners() {
            listener.on_set_warning_blocks(self, warning_blocks);
        }
    }

    /// `tick()` — `this.extent = this.extent.update()`.
    ///
    /// (Paper: the `lastTick == MinecraftServer.currentTick` dedupe guard is
    /// deferred — see `RivetTodo(#417)` in the module doc; with no `ServerLevel`
    /// the border is only ticked explicitly.)
    pub fn tick(&mut self) {
        let (center_x, center_z, absolute_max_size) =
            (self.center_x, self.center_z, self.absolute_max_size);
        let (extent, dirty) = self.extent.update(center_x, center_z, absolute_max_size);
        self.extent = extent;
        if dirty {
            // `MovingBorderExtent.update()` calls `WorldBorder.this.setDirty()`
            // unconditionally; the flag is applied here (Java calls it inside
            // `update()` — observably identical).
            self.saved_data.set_dirty();
        }
    }

    /// `applyInitialSettings(long gameTime)` — one-shot application of
    /// `settings` (via the setters, so listeners fire).
    pub fn apply_initial_settings(&mut self, game_time: i64) {
        if !self.initialized {
            let (center_x, center_z) = (self.settings.center_x, self.settings.center_z);
            self.set_center(center_x, center_z);
            let damage_per_block = self.settings.damage_per_block;
            self.set_damage_per_block(damage_per_block);
            let safe_zone = self.settings.safe_zone;
            self.set_safe_zone(safe_zone);
            let warning_blocks = self.settings.warning_blocks;
            self.set_warning_blocks(warning_blocks);
            let warning_time = self.settings.warning_time;
            self.set_warning_time(warning_time);
            if self.settings.lerp_time > 0 {
                let (size, lerp_target, lerp_time) = (
                    self.settings.size,
                    self.settings.lerp_target,
                    self.settings.lerp_time,
                );
                self.lerp_size_between(size, lerp_target, lerp_time, game_time);
            } else {
                let size = self.settings.size;
                self.set_size(size);
            }

            self.initialized = true;
        }
    }

    // Paper start - add back applySettings
    /// `applySettings(WorldBorder.Settings)` (Paper) — like
    /// `applyInitialSettings` but unconditioned. Paper reads `gameTime` from
    /// `this.world.getGameTime()` when the border has a world, else `0`; with
    /// the ServerLevel seam deferred the game time is always `0`.
    pub fn apply_settings(&mut self, settings: Settings) {
        self.set_center(settings.center_x, settings.center_z);
        self.set_damage_per_block(settings.damage_per_block);
        self.set_safe_zone(settings.safe_zone);
        self.set_warning_blocks(settings.warning_blocks);
        self.set_warning_time(settings.warning_time);
        if settings.lerp_time > 0 {
            // Paper: `world != null ? world.getGameTime() : 0` — no ServerLevel
            // seam, so always 0 (RivetTodo(#417)).
            let game_time = 0;
            self.lerp_size_between(
                settings.size,
                settings.lerp_target,
                settings.lerp_time,
                game_time,
            );
        } else {
            self.set_size(settings.size);
        }
    }
    // Paper end - add back applySettings

    // --- inherited `SavedData` surface ---

    /// `isDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.saved_data.is_dirty()
    }

    /// `setDirty()`.
    pub fn set_dirty(&mut self) {
        self.saved_data.set_dirty();
    }

    /// `setDirty(boolean)`.
    pub fn set_dirty_flag(&mut self, dirty: bool) {
        self.saved_data.set_dirty_flag(dirty);
    }

    /// `WorldBorder.CODEC` — `Settings.CODEC.xmap(WorldBorder::new,
    /// Settings::new)`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<WorldBorder, Ops>>
    where
        WorldBorder: 'static,
    {
        codec::xmap(
            Settings::codec::<Ops>(),
            Arc::new(|s: &Settings| WorldBorder::new(*s)),
            Arc::new(Settings::from_world_border),
        )
    }
}

/// `WorldBorder.TYPE` — `new SavedDataType<>(Identifier.
/// withDefaultNamespace("world_border"), WorldBorder::new, WorldBorder.CODEC,
/// DataFixTypes.SAVED_DATA_WORLD_BORDER)`. Java's `static final` singleton is a
/// `LazyLock` static pinned to `NbtOps` (the disk runtime's ops) — the same
/// shape as the `mc.world.level.saveddata` payloads' `TYPE` statics.
pub static TYPE: LazyLock<SavedDataType<WorldBorder>> = LazyLock::new(|| {
    SavedDataType::new(
        Identifier::with_default_namespace("world_border"),
        Arc::new(WorldBorder::default),
        WorldBorder::codec::<rivet_nbt::nbt_ops::NbtOps>(),
        DataFixTypes::SavedDataWorldBorder,
    )
});

/// `WorldBorder.BorderExtent` — the private extent interface, as a sum type.
/// `update()` returns the next extent plus whether `setDirty()` fired inside
/// the update (only [`MovingBorderExtent`] marks dirty).
#[derive(Debug)]
enum BorderExtent {
    /// `StaticBorderExtent`.
    Static(StaticBorderExtent),
    /// `MovingBorderExtent`.
    Moving(MovingBorderExtent),
}

impl BorderExtent {
    /// `getMinX(float)` — the `getMinX()` accessor of the extent, with the
    /// border state the Java inner class reads via `WorldBorder.this`.
    fn get_min_x(&self, delta_partial_tick: f32, center_x: f64, absolute_max_size: i32) -> f64 {
        match self {
            BorderExtent::Static(s) => s.get_min_x(delta_partial_tick),
            BorderExtent::Moving(m) => m.get_min_x(delta_partial_tick, center_x, absolute_max_size),
        }
    }

    /// `getMinZ(float)`.
    fn get_min_z(&self, delta_partial_tick: f32, center_z: f64, absolute_max_size: i32) -> f64 {
        match self {
            BorderExtent::Static(s) => s.get_min_z(delta_partial_tick),
            BorderExtent::Moving(m) => m.get_min_z(delta_partial_tick, center_z, absolute_max_size),
        }
    }

    /// `getMaxX(float)`.
    fn get_max_x(&self, delta_partial_tick: f32, center_x: f64, absolute_max_size: i32) -> f64 {
        match self {
            BorderExtent::Static(s) => s.get_max_x(delta_partial_tick),
            BorderExtent::Moving(m) => m.get_max_x(delta_partial_tick, center_x, absolute_max_size),
        }
    }

    /// `getMaxZ(float)`.
    fn get_max_z(&self, delta_partial_tick: f32, center_z: f64, absolute_max_size: i32) -> f64 {
        match self {
            BorderExtent::Static(s) => s.get_max_z(delta_partial_tick),
            BorderExtent::Moving(m) => m.get_max_z(delta_partial_tick, center_z, absolute_max_size),
        }
    }

    /// `getSize()`.
    fn get_size(&self) -> f64 {
        match self {
            BorderExtent::Static(s) => s.size,
            BorderExtent::Moving(m) => m.size,
        }
    }

    /// `getLerpSpeed()`.
    fn get_lerp_speed(&self) -> f64 {
        match self {
            BorderExtent::Static(_) => 0.0,
            BorderExtent::Moving(m) => {
                (m.from - m.to).abs() / m.lerp_end.wrapping_sub(m.lerp_begin) as f64
            }
        }
    }

    /// `getLerpTime()`.
    fn get_lerp_time(&self) -> i64 {
        match self {
            BorderExtent::Static(_) => 0,
            BorderExtent::Moving(m) => m.lerp_progress,
        }
    }

    /// `getLerpTarget()`.
    fn get_lerp_target(&self) -> f64 {
        match self {
            BorderExtent::Static(s) => s.size,
            BorderExtent::Moving(m) => m.to,
        }
    }

    /// `getStatus()`.
    fn get_status(&self) -> crate::level::border::border_status::BorderStatus {
        match self {
            BorderExtent::Static(_) => {
                crate::level::border::border_status::BorderStatus::Stationary
            }
            BorderExtent::Moving(m) => {
                if m.to < m.from {
                    crate::level::border::border_status::BorderStatus::Shrinking
                } else {
                    crate::level::border::border_status::BorderStatus::Growing
                }
            }
        }
    }

    /// `onCenterChange()`.
    fn on_center_change(&mut self, center_x: f64, center_z: f64, absolute_max_size: i32) {
        if let BorderExtent::Static(s) = self {
            s.update_box(center_x, center_z, absolute_max_size);
        }
    }

    /// `onAbsoluteMaxSizeChange()`.
    fn on_absolute_max_size_change(
        &mut self,
        center_x: f64,
        center_z: f64,
        absolute_max_size: i32,
    ) {
        if let BorderExtent::Static(s) = self {
            s.update_box(center_x, center_z, absolute_max_size);
        }
    }

    /// `update()` — advance one tick and return the replacement extent plus
    /// whether `setDirty()` fired (see [`WorldBorder::tick`]). The border's
    /// center/absmax are the values Java reads via `WorldBorder.this` when a
    /// finished `MovingBorderExtent` builds its replacement `StaticBorderExtent`
    /// (whose constructor runs `updateBox`).
    fn update(
        &mut self,
        center_x: f64,
        center_z: f64,
        absolute_max_size: i32,
    ) -> (BorderExtent, bool) {
        match self {
            BorderExtent::Static(s) => (BorderExtent::Static(*s), false),
            BorderExtent::Moving(m) => {
                m.lerp_progress = m.lerp_progress.wrapping_sub(1);
                m.previous_size = m.size;
                m.size = m.calculate_size();
                // Paper start - Add worldborder events (deferred: the
                // `WorldBorderBoundsChangeFinishEvent` fires when finished and
                // `world != null`).
                // Paper end
                if m.lerp_progress <= 0 {
                    (
                        BorderExtent::Static(StaticBorderExtent::new(
                            m.to,
                            center_x,
                            center_z,
                            absolute_max_size,
                        )),
                        true,
                    )
                } else {
                    (BorderExtent::Moving(*m), true)
                }
            }
        }
    }

    /// `getCollisionShape()` — `WorldBorder.this`'s live center/absmax.
    fn get_collision_shape(
        &self,
        center_x: f64,
        center_z: f64,
        absolute_max_size: i32,
    ) -> crate::level::border::shapes::VoxelShape {
        match self {
            BorderExtent::Static(s) => s.shape,
            BorderExtent::Moving(m) => {
                let min_x = m.get_min_x(0.0_f32, center_x, absolute_max_size).floor();
                let min_z = m.get_min_z(0.0_f32, center_z, absolute_max_size).floor();
                let max_x = m.get_max_x(0.0_f32, center_x, absolute_max_size).ceil();
                let max_z = m.get_max_z(0.0_f32, center_z, absolute_max_size).ceil();
                Shapes::border_wall(min_x, min_z, max_x, max_z)
            }
        }
    }
}

/// `WorldBorder.MovingBorderExtent` — the timed lerp extent.
///
/// Java fields: `from`, `to`, `lerpEnd` (`long`), `lerpBegin` (`long`),
/// `lerpDuration` (`double`, widened from the `long` duration),
/// `lerpProgress` (`long`), `size`, `previousSize`.
#[derive(Debug, Clone, Copy)]
struct MovingBorderExtent {
    from: f64,
    to: f64,
    lerp_end: i64,
    lerp_begin: i64,
    lerp_duration: f64,
    lerp_progress: i64,
    size: f64,
    previous_size: f64,
}

impl MovingBorderExtent {
    /// `new MovingBorderExtent(double from, double to, long duration, long
    /// gameTime)`.
    fn new(from: f64, to: f64, duration: i64, game_time: i64) -> MovingBorderExtent {
        let lerp_duration = duration as f64;
        let lerp_begin = game_time;
        let lerp_end = lerp_begin.wrapping_add(duration);
        let lerp_progress = duration;
        let mut extent = MovingBorderExtent {
            from,
            to,
            lerp_end,
            lerp_begin,
            lerp_duration,
            lerp_progress,
            size: 0.0,
            previous_size: 0.0,
        };
        let size = extent.calculate_size();
        extent.size = size;
        extent.previous_size = size;
        extent
    }

    /// `getPreviousSize()` — used by the `getMinX`/`getMaxX` lerp.
    fn get_previous_size(&self) -> f64 {
        self.previous_size
    }

    /// `calculateSize()` — `progress < 1.0 ? Mth.lerp(progress, from, to) :
    /// to`, where `progress = (lerpDuration - lerpProgress) / lerpDuration`.
    fn calculate_size(&self) -> f64 {
        let progress = (self.lerp_duration - self.lerp_progress as f64) / self.lerp_duration;
        if progress < 1.0 {
            lerp(progress, self.from, self.to)
        } else {
            self.to
        }
    }

    /// `getMinX(float)` — the moving half-width around `center_x`, clamped to
    /// `absoluteMaxSize`.
    fn get_min_x(&self, delta_partial_tick: f32, center_x: f64, absolute_max_size: i32) -> f64 {
        clamp_f64(
            center_x
                - lerp(
                    delta_partial_tick as f64,
                    self.get_previous_size(),
                    self.size,
                ) / 2.0,
            absolute_max_size.wrapping_neg() as f64,
            absolute_max_size as f64,
        )
    }

    /// `getMinZ(float)`.
    fn get_min_z(&self, delta_partial_tick: f32, center_z: f64, absolute_max_size: i32) -> f64 {
        clamp_f64(
            center_z
                - lerp(
                    delta_partial_tick as f64,
                    self.get_previous_size(),
                    self.size,
                ) / 2.0,
            absolute_max_size.wrapping_neg() as f64,
            absolute_max_size as f64,
        )
    }

    /// `getMaxX(float)`.
    fn get_max_x(&self, delta_partial_tick: f32, center_x: f64, absolute_max_size: i32) -> f64 {
        clamp_f64(
            center_x
                + lerp(
                    delta_partial_tick as f64,
                    self.get_previous_size(),
                    self.size,
                ) / 2.0,
            absolute_max_size.wrapping_neg() as f64,
            absolute_max_size as f64,
        )
    }

    /// `getMaxZ(float)`.
    fn get_max_z(&self, delta_partial_tick: f32, center_z: f64, absolute_max_size: i32) -> f64 {
        clamp_f64(
            center_z
                + lerp(
                    delta_partial_tick as f64,
                    self.get_previous_size(),
                    self.size,
                ) / 2.0,
            absolute_max_size.wrapping_neg() as f64,
            absolute_max_size as f64,
        )
    }
}

/// `WorldBorder.StaticBorderExtent` — the fixed-size extent with a cached box.
#[derive(Debug, Clone, Copy)]
struct StaticBorderExtent {
    size: f64,
    min_x: f64,
    min_z: f64,
    max_x: f64,
    max_z: f64,
    shape: crate::level::border::shapes::VoxelShape,
}

impl StaticBorderExtent {
    /// `new StaticBorderExtent(double size)` — runs `updateBox()`.
    fn new(size: f64, center_x: f64, center_z: f64, absolute_max_size: i32) -> StaticBorderExtent {
        let mut extent = StaticBorderExtent {
            size,
            min_x: 0.0,
            min_z: 0.0,
            max_x: 0.0,
            max_z: 0.0,
            shape: crate::level::border::shapes::VoxelShape::INFINITY,
        };
        extent.update_box(center_x, center_z, absolute_max_size);
        extent
    }

    /// `updateBox()` — recomputes the clamped box and the collision shape.
    /// The lower clamp bound is Java's `-absoluteMaxSize` **int** negation
    /// (wraps for `i32::MIN`), widened to double.
    fn update_box(&mut self, center_x: f64, center_z: f64, absolute_max_size: i32) {
        let abs = absolute_max_size as f64;
        let abs_neg = absolute_max_size.wrapping_neg() as f64;
        self.min_x = clamp_f64(center_x - self.size / 2.0, abs_neg, abs);
        self.min_z = clamp_f64(center_z - self.size / 2.0, abs_neg, abs);
        self.max_x = clamp_f64(center_x + self.size / 2.0, abs_neg, abs);
        self.max_z = clamp_f64(center_z + self.size / 2.0, abs_neg, abs);
        self.shape = Shapes::border_wall(
            self.get_min_x(0.0_f32).floor(),
            self.get_min_z(0.0_f32).floor(),
            self.get_max_x(0.0_f32).ceil(),
            self.get_max_z(0.0_f32).ceil(),
        );
    }

    /// `getMinX(float)` — the cached box.
    fn get_min_x(&self, _delta_partial_tick: f32) -> f64 {
        self.min_x
    }

    /// `getMaxX(float)`.
    fn get_max_x(&self, _delta_partial_tick: f32) -> f64 {
        self.max_x
    }

    /// `getMinZ(float)`.
    fn get_min_z(&self, _delta_partial_tick: f32) -> f64 {
        self.min_z
    }

    /// `getMaxZ(float)`.
    fn get_max_z(&self, _delta_partial_tick: f32) -> f64 {
        self.max_z
    }
}

/// Java's `Double.compare(a, b) == 0` — the record-`equals` contract for a
/// `double` component. `Double.doubleToLongBits` canonicalizes every NaN to
/// `0x7ff8000000000000` (so all NaNs compare equal regardless of sign/payload)
/// and keeps the sign bit of zero (so `-0.0 != 0.0`), which IEEE `==` (via
/// `to_bits` alone) would not.
fn double_eq(a: f64, b: f64) -> bool {
    let canonical = |x: f64| {
        if x.is_nan() {
            0x7ff8_0000_0000_0000_u64
        } else {
            x.to_bits()
        }
    };
    canonical(a) == canonical(b)
}

/// `WorldBorder.Settings` — the nine-field record.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// `centerX`.
    center_x: f64,
    /// `centerZ`.
    center_z: f64,
    /// `damagePerBlock`.
    damage_per_block: f64,
    /// `safeZone`.
    safe_zone: f64,
    /// `warningBlocks`.
    warning_blocks: i32,
    /// `warningTime`.
    warning_time: i32,
    /// `size`.
    size: f64,
    /// `lerpTime`.
    lerp_time: i64,
    /// `lerpTarget`.
    lerp_target: f64,
}

impl PartialEq for Settings {
    /// Java's auto-generated record `equals`: each `double` component is
    /// compared with `Double.compare` (canonical NaNs equal; `-0.0 != 0.0`)
    /// and each `int`/`long` component with `==`. IEEE `==` (a derived
    /// `PartialEq`) would diverge on NaN and `-0.0`, so equality is
    /// implemented manually.
    fn eq(&self, other: &Self) -> bool {
        double_eq(self.center_x, other.center_x)
            && double_eq(self.center_z, other.center_z)
            && double_eq(self.damage_per_block, other.damage_per_block)
            && double_eq(self.safe_zone, other.safe_zone)
            && self.warning_blocks == other.warning_blocks
            && self.warning_time == other.warning_time
            && double_eq(self.size, other.size)
            && self.lerp_time == other.lerp_time
            && double_eq(self.lerp_target, other.lerp_target)
    }
}

impl Settings {
    /// `Settings.DEFAULT` — `new Settings(0.0, 0.0, 0.2, 5.0, 5, 300,
    /// 5.999997E7F, 0L, 0.0)`.
    pub fn default_settings() -> Settings {
        Settings::new(0.0, 0.0, 0.2, 5.0, 5, 300, MAX_SIZE, 0, 0.0)
    }

    /// The canonical constructor.
    #[allow(clippy::too_many_arguments)] // mirrors Java's 9-parameter constructor exactly
    pub fn new(
        center_x: f64,
        center_z: f64,
        damage_per_block: f64,
        safe_zone: f64,
        warning_blocks: i32,
        warning_time: i32,
        size: f64,
        lerp_time: i64,
        lerp_target: f64,
    ) -> Settings {
        Settings {
            center_x,
            center_z,
            damage_per_block,
            safe_zone,
            warning_blocks,
            warning_time,
            size,
            lerp_time,
            lerp_target,
        }
    }

    /// `Settings(WorldBorder)` — the compact constructor reading the live
    /// values.
    pub fn from_world_border(border: &WorldBorder) -> Settings {
        Settings::new(
            border.center_x,
            border.center_z,
            border.damage_per_block,
            border.safe_zone,
            border.warning_blocks,
            border.warning_time,
            border.extent.get_size(),
            border.extent.get_lerp_time(),
            border.extent.get_lerp_target(),
        )
    }

    /// `Settings.centerX()`.
    pub fn center_x(&self) -> f64 {
        self.center_x
    }

    /// `Settings.centerZ()`.
    pub fn center_z(&self) -> f64 {
        self.center_z
    }

    /// `Settings.damagePerBlock()`.
    pub fn damage_per_block(&self) -> f64 {
        self.damage_per_block
    }

    /// `Settings.safeZone()`.
    pub fn safe_zone(&self) -> f64 {
        self.safe_zone
    }

    /// `Settings.warningBlocks()`.
    pub fn warning_blocks(&self) -> i32 {
        self.warning_blocks
    }

    /// `Settings.warningTime()`.
    pub fn warning_time(&self) -> i32 {
        self.warning_time
    }

    /// `Settings.size()`.
    pub fn size(&self) -> f64 {
        self.size
    }

    /// `Settings.lerpTime()`.
    pub fn lerp_time(&self) -> i64 {
        self.lerp_time
    }

    /// `Settings.lerpTarget()`.
    pub fn lerp_target(&self) -> f64 {
        self.lerp_target
    }

    /// Normalize a live border settings snapshot for a detached generated
    /// region. The live extent's current size is already the size visible at
    /// the snapshot point; carrying its interpolation metadata would replay a
    /// moving border from an unrelated game time. Paper's region therefore
    /// receives the current center/current size as a stationary extent.
    pub fn current_bounds_snapshot(self) -> Settings {
        Settings::new(
            self.center_x,
            self.center_z,
            self.damage_per_block,
            self.safe_zone,
            self.warning_blocks,
            self.warning_time,
            self.size,
            0,
            self.size,
        )
    }

    /// `Settings.CODEC` — the nine-field record codec (see the module doc for
    /// the 6+3 composition). All fields are mandatory `fieldOf`s in Java's
    /// declaration order.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Settings, Ops>>
    where
        Settings: 'static,
    {
        record_builder::create(|instance| {
            // Fields 1-6: `center_x`, `center_z`, `damage_per_block`,
            // `safe_zone`, `warning_blocks`, `warning_time` — composed into a
            // nested six-tuple map codec (the record_builder arity chain caps
            // at six; the getters here project out of the tuple).
            #[allow(clippy::type_complexity)] // the six-tuple map codec arity cap (module doc)
            let first_six: Arc<
                dyn rivet_serialization::map_codec::MapCodec<(f64, f64, f64, f64, i32, i32), Ops>,
            > = record_builder::map_codec(|inner| {
                inner
                    .group(RecordCodecBuilder::of_named(
                        Arc::new(|six: &(f64, f64, f64, f64, i32, i32)| six.0),
                        "center_x".to_string(),
                        codec::double_range(-2.9999984E7, 2.9999984E7),
                    ))
                    .and(RecordCodecBuilder::of_named(
                        Arc::new(|six: &(f64, f64, f64, f64, i32, i32)| six.1),
                        "center_z".to_string(),
                        codec::double_range(-2.9999984E7, 2.9999984E7),
                    ))
                    .and(RecordCodecBuilder::of_named(
                        Arc::new(|six: &(f64, f64, f64, f64, i32, i32)| six.2),
                        "damage_per_block".to_string(),
                        codec::double_codec::<Ops>(),
                    ))
                    .and(RecordCodecBuilder::of_named(
                        Arc::new(|six: &(f64, f64, f64, f64, i32, i32)| six.3),
                        "safe_zone".to_string(),
                        codec::double_codec::<Ops>(),
                    ))
                    .and(RecordCodecBuilder::of_named(
                        Arc::new(|six: &(f64, f64, f64, f64, i32, i32)| six.4),
                        "warning_blocks".to_string(),
                        codec::int_codec::<Ops>(),
                    ))
                    .and(RecordCodecBuilder::of_named(
                        Arc::new(|six: &(f64, f64, f64, f64, i32, i32)| six.5),
                        "warning_time".to_string(),
                        codec::int_codec::<Ops>(),
                    ))
                    .apply(
                        inner,
                        Arc::new(
                            |center_x,
                             center_z,
                             damage_per_block,
                             safe_zone,
                             warning_blocks,
                             warning_time| {
                                (
                                    center_x,
                                    center_z,
                                    damage_per_block,
                                    safe_zone,
                                    warning_blocks,
                                    warning_time,
                                )
                            },
                        ),
                    )
            });

            // Fields 7-9: `size`, `lerp_time`, `lerp_target`.
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &Settings| {
                        (
                            s.center_x,
                            s.center_z,
                            s.damage_per_block,
                            s.safe_zone,
                            s.warning_blocks,
                            s.warning_time,
                        )
                    }),
                    first_six,
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|s: &Settings| s.size),
                    "size".to_string(),
                    codec::double_codec::<Ops>(),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|s: &Settings| s.lerp_time),
                    "lerp_time".to_string(),
                    codec::long_codec::<Ops>(),
                ))
                .and(RecordCodecBuilder::of_named(
                    Arc::new(|s: &Settings| s.lerp_target),
                    "lerp_target".to_string(),
                    codec::double_codec::<Ops>(),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |six: (f64, f64, f64, f64, i32, i32), size, lerp_time, lerp_target| {
                            Settings::new(
                                six.0,
                                six.1,
                                six.2,
                                six.3,
                                six.4,
                                six.5,
                                size,
                                lerp_time,
                                lerp_target,
                            )
                        },
                    ),
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::border::border_change_listener::BorderChangeListener;
    use crate::level::border::border_status::BorderStatus;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The Paper-grounded `Settings` used by the codec and lifecycle tests.
    /// `lerp_time` is 0 so the applied extent is static (Java's
    /// `applyInitialSettings` branch).
    fn sample_settings() -> Settings {
        Settings::new(10.0, 20.0, 0.4, 8.0, 7, 30, 100.0, 0, 0.0)
    }

    fn encode_json(
        codec: &Arc<dyn Codec<Settings, JsonOps>>,
        settings: &Settings,
    ) -> serde_json::Value {
        codec
            .encode_start(&JsonOps::INSTANCE, settings)
            .result()
            .expect("encode")
            .clone()
    }

    fn decode_json(
        codec: &Arc<dyn Codec<Settings, JsonOps>>,
        value: &serde_json::Value,
    ) -> Settings {
        *codec
            .parse(&JsonOps::INSTANCE, value)
            .result()
            .expect("decode")
    }

    /// Records the listener calls a border fires (in order). `BorderChangeListener`
    /// is `Send + Sync`, so the recording store is a `Mutex` rather than a
    /// `RefCell`.
    #[derive(Debug, Default)]
    struct RecordingListener {
        events: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingListener {
        fn new() -> Arc<Self> {
            Arc::new(RecordingListener::default())
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl BorderChangeListener for RecordingListener {
        fn on_set_size(&self, _border: &WorldBorder, new_size: f64) {
            self.events
                .lock()
                .unwrap()
                .push(format!("set_size({new_size})"));
        }
        fn on_lerp_size(
            &self,
            _border: &WorldBorder,
            from_size: f64,
            target_size: f64,
            ticks: i64,
            game_time: i64,
        ) {
            self.events.lock().unwrap().push(format!(
                "lerp_size({from_size},{target_size},{ticks},{game_time})"
            ));
        }
        fn on_set_center(&self, _border: &WorldBorder, x: f64, z: f64) {
            self.events
                .lock()
                .unwrap()
                .push(format!("set_center({x},{z})"));
        }
        fn on_set_warning_time(&self, _border: &WorldBorder, time: i32) {
            self.events
                .lock()
                .unwrap()
                .push(format!("set_warning_time({time})"));
        }
        fn on_set_warning_blocks(&self, _border: &WorldBorder, blocks: i32) {
            self.events
                .lock()
                .unwrap()
                .push(format!("set_warning_blocks({blocks})"));
        }
        fn on_set_damage_per_block(&self, _border: &WorldBorder, damage_per_block: f64) {
            self.events
                .lock()
                .unwrap()
                .push(format!("set_damage_per_block({damage_per_block})"));
        }
        fn on_set_safe_zone(&self, _border: &WorldBorder, safe_zone: f64) {
            self.events
                .lock()
                .unwrap()
                .push(format!("set_safe_zone({safe_zone})"));
        }
    }

    #[test]
    fn default_size_is_the_widened_float_literal() {
        // `WorldBorder.MAX_SIZE = 5.999997E7F` — the float nearest 59999970.0,
        // so the default border size (and `Settings.DEFAULT.size`) is exactly
        // 59999968.0 (see the `MAX_SIZE` doc).
        let border = WorldBorder::default();
        assert_eq!(border.get_size(), 59999968.0);
        assert_eq!(Settings::default_settings().size(), 59999968.0);
        assert_eq!(MAX_SIZE, 59999968.0);
        // Default center, warnings, safe zone, damage.
        assert_eq!(border.get_center_x(), 0.0);
        assert_eq!(border.get_center_z(), 0.0);
        assert_eq!(border.get_warning_time(), 15);
        assert_eq!(border.get_warning_blocks(), 5);
        assert_eq!(border.get_safe_zone(), 5.0);
        assert_eq!(border.get_damage_per_block(), 0.2);
        // Default absolute max.
        assert_eq!(border.get_absolute_max_size(), 29999984);
        // Default status is STATIONARY.
        assert_eq!(border.get_status(), BorderStatus::Stationary);
    }

    #[test]
    fn default_border_bounds_are_half_the_size() {
        let border = WorldBorder::default();
        let half = MAX_SIZE / 2.0;
        assert_eq!(border.get_min_x(), -half);
        assert_eq!(border.get_max_x(), half);
        assert_eq!(border.get_min_z(), -half);
        assert_eq!(border.get_max_z(), half);
    }

    #[test]
    fn is_within_bounds_is_half_open() {
        let border = WorldBorder::default();
        // Center is in bounds.
        assert!(border.is_within_bounds_xy(0.0, 0.0));
        // The max edge is EXCLUSIVE (`x < getMaxX() + margin`), so a point
        // exactly on maxX is NOT within bounds.
        assert!(!border.is_within_bounds_xy(border.get_max_x(), 0.0));
        // The min edge is INCLUSIVE (`x >= getMinX() - margin`).
        assert!(border.is_within_bounds_xy(border.get_min_x(), 0.0));
        // Just inside maxX is within bounds.
        assert!(border.is_within_bounds_xy(border.get_max_x() - 1.0, 0.0));
        // Just outside minX is not.
        assert!(!border.is_within_bounds_xy(border.get_min_x() - 1.0, 0.0));
    }

    #[test]
    fn is_within_bounds_chunk_checks_both_corners() {
        // A border centered at (5, 5) with size 40 spans `[-15, 25)`, so chunk
        // 1 (blocks 16..31) straddles the max edge: its min corner is inside
        // but its max corner is outside.
        let mut border = WorldBorder::default();
        border.set_center(5.0, 5.0);
        border.set_size(40.0);
        // A chunk fully inside is within bounds.
        assert!(border.is_within_bounds_chunk(&ChunkPos::new(0, 0)));
        // The straddling chunk is NOT within bounds (Java requires BOTH the
        // min and max corners).
        let straddle = ChunkPos::new(1, 1);
        assert!(!border.is_within_bounds_chunk(&straddle));
        // But its min corner alone IS within bounds.
        assert!(border.is_within_bounds_xy(
            straddle.get_min_block_x() as f64,
            straddle.get_min_block_z() as f64
        ));
    }

    #[test]
    fn paper_is_block_and_chunk_in_bounds() {
        let border = WorldBorder::default();
        // Paper's `isBlockInBounds` boxes at y=64, which is irrelevant to the
        // x/z test; `isChunkInBounds` tests the `(chunkX << 4) + 15` corner.
        assert!(border.is_block_in_bounds(0, 0));
        assert!(border.is_block_in_bounds(border.get_min_x() as i32, 0));
        assert!(!border.is_block_in_bounds(border.get_max_x() as i32, 0));
        // Chunk 0 is within bounds; a chunk whose max corner is outside is not.
        assert!(border.is_chunk_in_bounds(0, 0));
        let far_chunk_x = (border.get_max_x() as i32) / 16;
        assert!(!border.is_chunk_in_bounds(far_chunk_x, 0));
    }

    #[test]
    fn is_within_bounds_margin_expands_the_box() {
        let border = WorldBorder::default();
        // A point just outside the border becomes within bounds once the
        // margin is large enough.
        assert!(!border.is_within_bounds_xy(border.get_max_x() + 0.5, 0.0));
        assert!(border.is_within_bounds_margin(border.get_max_x() + 0.5, 0.0, 1.0));
        // The max check is still exclusive: maxX + margin itself is not within
        // bounds (`x < getMaxX() + margin`).
        assert!(!border.is_within_bounds_margin(border.get_max_x() + 1.0, 0.0, 1.0));
    }

    #[test]
    fn is_within_bounds_aabb_uses_the_epsilon_corners() {
        let border = WorldBorder::default();
        // `aabb.maxX - 1.0E-5F` — an AABB whose maxX is exactly the border
        // max would otherwise sit on the (exclusive) edge.
        let aabb = Aabb {
            min_x: 0.0,
            min_y: 0.0,
            min_z: 0.0,
            max_x: border.get_max_x(),
            max_y: 256.0,
            max_z: border.get_max_z(),
        };
        assert!(border.is_within_bounds_aabb(&aabb));
        // The EPSILON is subtracted BEFORE the half-open test, so `maxX`
        // (exclusive) is tested as `maxX - EPSILON` (inclusive-ok).
        assert_eq!(border.get_max_x() - EPSILON, aabb.max_x - EPSILON);
    }

    #[test]
    fn clamp_to_bounds_clamps_block_pos() {
        let border = WorldBorder::default();
        // A position far outside the border clamps to the min corner.
        let clamped = border.clamp_to_bounds(&BlockPos::new(-30000000, 64, -30000000));
        assert_eq!(clamped.get_x(), border.get_min_x() as i32);
        assert_eq!(clamped.get_z(), border.get_min_z() as i32);
        assert_eq!(clamped.get_y(), 64);
        // A position at the border edge is left alone (min edge inclusive).
        let at_edge = BlockPos::new(border.get_min_x() as i32, 64, 0);
        assert_eq!(
            border.clamp_to_bounds(&at_edge).get_x(),
            border.get_min_x() as i32
        );
    }

    #[test]
    fn clamp_vec3_to_bound_uses_epsilon_and_keeps_y() {
        let border = WorldBorder::default();
        // `clampVec3ToBound` clamps x/z into `[minX, maxX - 1.0E-5F]` and
        // leaves y untouched.
        let out = border.clamp_vec3_to_bound(&Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(out.x, 1.0);
        assert_eq!(out.y, 2.0);
        assert_eq!(out.z, 3.0);
        let clamped = border.clamp_vec3_to_bound(&Vec3::new(
            border.get_max_x() + 100.0,
            -999.0,
            border.get_min_z() - 100.0,
        ));
        assert_eq!(clamped.x, border.get_max_x() - EPSILON);
        assert_eq!(clamped.z, border.get_min_z());
        assert_eq!(clamped.y, -999.0);
    }

    #[test]
    fn get_distance_to_border_is_min_of_the_four_edges() {
        let border = WorldBorder::default();
        // At the center, every edge is `size/2` away.
        assert_eq!(border.get_distance_to_border(0.0, 0.0), MAX_SIZE / 2.0);
        // Just inside the max-x edge, the east distance is tiny.
        let east = border.get_distance_to_border(border.get_max_x() - 1.0, 0.0);
        assert_eq!(east, 1.0);
        // On the border it is 0.
        assert_eq!(border.get_distance_to_border(border.get_max_x(), 0.0), 0.0);
        // Outside the border the distance is negative.
        assert_eq!(
            border.get_distance_to_border(border.get_max_x() + 5.0, 0.0),
            -5.0
        );
        // Entity overload reads the position.
        let entity = Entity::new(border.get_max_x() + 3.0, 64.0, 0.0);
        assert_eq!(border.get_distance_to_border_entity(&entity), -3.0);
    }

    #[test]
    fn get_collision_shape_bounds_are_floored_and_ceiled() {
        let border = WorldBorder::default();
        let shape = border.get_collision_shape();
        // The wall is `Shapes.join(INFINITY, box, ONLY_FIRST)` — the box
        // corners (Java floors/ceils the min/max). The stub carries no
        // "infinity" flag (the wall is NOT the full infinite solid; it is the
        // infinite solid minus the inner box).
        assert_eq!(shape.min_x, border.get_min_x().floor());
        assert_eq!(shape.min_z, border.get_min_z().floor());
        assert_eq!(shape.max_x, border.get_max_x().ceil());
        assert_eq!(shape.max_z, border.get_max_z().ceil());
    }

    #[test]
    fn negative_size_stores_a_degenerate_shape_without_panicking() {
        // Java's `Shapes.box` throws `IllegalArgumentException("The min values
        // need to be smaller or equals to the max values")` when a min
        // coordinate exceeds its max. A border of size -100 has
        // `minX = 50 > maxX = -50`, so `StaticBorderExtent.updateBox()` throws
        // at construction in Java (`setSize(-100)`). The shapes STUB
        // (mc.world.phys.shapes) stores the degenerate corners silently — the
        // real `Shapes.box` port must replicate that validation. The box
        // accessors themselves match Java (they only invert, never throw).
        let mut border = WorldBorder::default();
        border.set_size(-100.0);
        assert_eq!(border.get_size(), -100.0);
        assert_eq!(border.get_status(), BorderStatus::Stationary);
        assert!(border.get_min_x() > border.get_max_x());
        assert_eq!(border.get_min_x(), 50.0);
        assert_eq!(border.get_max_x(), -50.0);
        // The collision shape carries the same degenerate (floored/ceiled)
        // corners — the stub does not throw where Java's `Shapes.box` would.
        let shape = border.get_collision_shape();
        assert_eq!(shape.min_x, 50.0);
        assert_eq!(shape.min_z, 50.0);
        assert_eq!(shape.max_x, -50.0);
        assert_eq!(shape.max_z, -50.0);
    }

    #[test]
    fn set_center_moves_and_updates_extent_and_dirty() {
        let mut border = WorldBorder::default();
        border.add_listener(RecordingListener::new());
        assert!(!border.is_dirty());
        border.set_center(100.0, -200.0);
        assert_eq!(border.get_center_x(), 100.0);
        assert_eq!(border.get_center_z(), -200.0);
        // The static extent box is recentered around the new center.
        assert_eq!(border.get_min_x(), 100.0 - MAX_SIZE / 2.0);
        assert_eq!(border.get_max_z(), -200.0 + MAX_SIZE / 2.0);
        assert!(border.is_dirty());
    }

    #[test]
    fn set_size_rebuilds_a_static_extent() {
        let mut border = WorldBorder::default();
        border.set_size(1000.0);
        assert_eq!(border.get_size(), 1000.0);
        assert_eq!(border.get_status(), BorderStatus::Stationary);
        assert_eq!(border.get_min_x(), -500.0);
        assert_eq!(border.get_max_x(), 500.0);
        assert_eq!(border.get_lerp_time(), 0);
        assert_eq!(border.get_lerp_target(), 1000.0);
        assert_eq!(border.get_lerp_speed(), 0.0);
    }

    #[test]
    fn lerp_size_between_from_equals_to_is_static() {
        let mut border = WorldBorder::default();
        border.lerp_size_between(500.0, 500.0, 100, 1000);
        assert_eq!(border.get_status(), BorderStatus::Stationary);
        assert_eq!(border.get_size(), 500.0);
        assert_eq!(border.get_lerp_time(), 0);
    }

    #[test]
    fn moving_extent_reports_growing_speed_and_target() {
        let mut border = WorldBorder::default();
        border.lerp_size_between(100.0, 300.0, 100, 1000);
        assert_eq!(border.get_status(), BorderStatus::Growing);
        // `getLerpSpeed = Math.abs(from - to) / (lerpEnd - lerpBegin)`
        assert_eq!(border.get_lerp_speed(), 2.0);
        assert_eq!(border.get_lerp_target(), 300.0);
        assert_eq!(border.get_lerp_time(), 100);
        // The moving box is centered around the border's center.
        assert_eq!(border.get_size(), 100.0);
        assert_eq!(border.get_min_x(), -50.0);
        assert_eq!(border.get_max_x(), 50.0);
    }

    #[test]
    fn moving_extent_lerps_then_transitions_to_static() {
        let mut border = WorldBorder::default();
        border.lerp_size_between(100.0, 200.0, 2, 0);
        assert_eq!(border.get_status(), BorderStatus::Growing);
        // At construction `lerpProgress = duration = 2`, so the first tick's
        // `update()` still lerps: `progress = (2-1)/2 = 0.5` →
        // `lerp(0.5, 100, 200) = 150`.
        border.tick();
        assert_eq!(border.get_status(), BorderStatus::Growing);
        assert_eq!(border.get_size(), 150.0);
        // Second tick finishes: `lerpProgress` reaches 0 → `size = to` and the
        // extent becomes a `StaticBorderExtent` (status STATIONARY).
        border.tick();
        assert_eq!(border.get_status(), BorderStatus::Stationary);
        assert_eq!(border.get_size(), 200.0);
        assert_eq!(border.get_min_x(), -100.0);
        assert_eq!(border.get_max_x(), 100.0);
        assert_eq!(border.get_lerp_time(), 0);
        // A further tick keeps it static and no longer dirty-marking.
        border.tick();
        assert_eq!(border.get_status(), BorderStatus::Stationary);
        assert_eq!(border.get_size(), 200.0);
    }

    #[test]
    fn moving_extent_shrinking_status() {
        let mut border = WorldBorder::default();
        border.lerp_size_between(200.0, 100.0, 50, 0);
        assert_eq!(border.get_status(), BorderStatus::Shrinking);
        assert_eq!(border.get_lerp_speed(), 2.0);
    }

    #[test]
    fn lerp_size_notifies_listeners_and_marks_dirty() {
        let mut border = WorldBorder::default();
        let listener = RecordingListener::new();
        border.add_listener(listener.clone());
        border.lerp_size_between(100.0, 300.0, 100, 1000);
        assert!(border.is_dirty());
        assert_eq!(
            listener.events(),
            vec!["lerp_size(100,300,100,1000)".to_string()]
        );
    }

    #[test]
    fn listeners_are_notified_in_order_and_copy_is_safe() {
        let mut border = WorldBorder::default();
        let first = RecordingListener::new();
        let second = RecordingListener::new();
        border.add_listener(first.clone());
        border.add_listener(second.clone());
        // `addListener` is a no-op for an already-added (identity-equal)
        // listener.
        border.add_listener(first.clone());
        border.set_center(1.0, 2.0);
        assert_eq!(first.events(), vec!["set_center(1,2)".to_string()]);
        assert_eq!(second.events(), vec!["set_center(1,2)".to_string()]);
        // `removeListener` stops future notifications.
        let first_as_trait: Arc<dyn BorderChangeListener> = first.clone();
        border.remove_listener(&first_as_trait);
        border.set_center(3.0, 4.0);
        assert_eq!(first.events(), vec!["set_center(1,2)".to_string()]);
        assert_eq!(
            second.events(),
            vec!["set_center(1,2)".to_string(), "set_center(3,4)".to_string()]
        );
    }

    #[test]
    fn each_setter_fires_its_listener_notification() {
        let mut border = WorldBorder::default();
        let listener = RecordingListener::new();
        border.add_listener(listener.clone());
        border.set_safe_zone(7.0);
        border.set_damage_per_block(0.5);
        border.set_warning_time(42);
        border.set_warning_blocks(9);
        assert_eq!(
            listener.events(),
            vec![
                "set_safe_zone(7)".to_string(),
                "set_damage_per_block(0.5)".to_string(),
                "set_warning_time(42)".to_string(),
                "set_warning_blocks(9)".to_string(),
            ]
        );
    }

    #[test]
    fn setters_update_state_and_dirty_flag() {
        let mut border = WorldBorder::default();
        border.set_safe_zone(7.0);
        border.set_damage_per_block(0.5);
        border.set_warning_time(42);
        border.set_warning_blocks(9);
        assert_eq!(border.get_safe_zone(), 7.0);
        assert_eq!(border.get_damage_per_block(), 0.5);
        assert_eq!(border.get_warning_time(), 42);
        assert_eq!(border.get_warning_blocks(), 9);
        assert!(border.is_dirty());
    }

    #[test]
    fn dirty_flag_can_be_cleared_and_redirtied() {
        // The inherited `SavedData` surface: `setDirty()` marks, `setDirty(false)`
        // clears, `setDirty(true)` redirties.
        let mut border = WorldBorder::default();
        assert!(!border.is_dirty());
        border.set_dirty();
        assert!(border.is_dirty());
        border.set_dirty_flag(false);
        assert!(!border.is_dirty());
        border.set_dirty_flag(true);
        assert!(border.is_dirty());
    }

    #[test]
    fn set_absolute_max_size_reclamps_the_box() {
        let mut border = WorldBorder::default();
        border.set_size(MAX_SIZE);
        border.set_absolute_max_size(1000);
        // The default border is bigger than the new absolute max, so its box
        // clamps to `[-1000, 1000]` (Java `updateBox` clamps to
        // `[-absoluteMaxSize, absoluteMaxSize]`).
        assert_eq!(border.get_min_x(), -1000.0);
        assert_eq!(border.get_max_x(), 1000.0);
        assert_eq!(border.get_min_z(), -1000.0);
        assert_eq!(border.get_max_z(), 1000.0);
    }

    #[test]
    fn apply_initial_settings_applies_once_and_notifies() {
        // `applyInitialSettings` applies the border's STORED settings (its
        // constructor arg), so build the border from `sample_settings`.
        let mut border = WorldBorder::new(sample_settings());
        let listener = RecordingListener::new();
        border.add_listener(listener.clone());
        border.apply_initial_settings(0);
        // All settings applied through the setters.
        assert_eq!(border.get_center_x(), 10.0);
        assert_eq!(border.get_center_z(), 20.0);
        assert_eq!(border.get_damage_per_block(), 0.4);
        assert_eq!(border.get_safe_zone(), 8.0);
        assert_eq!(border.get_warning_blocks(), 7);
        assert_eq!(border.get_warning_time(), 30);
        assert_eq!(border.get_size(), 100.0);
        let calls = listener.events();
        assert_eq!(
            calls,
            vec![
                "set_center(10,20)".to_string(),
                "set_damage_per_block(0.4)".to_string(),
                "set_safe_zone(8)".to_string(),
                "set_warning_blocks(7)".to_string(),
                "set_warning_time(30)".to_string(),
                "set_size(100)".to_string(),
            ]
        );
        // Second call is a no-op (the `initialized` guard).
        border.apply_initial_settings(0);
        assert_eq!(listener.events().len(), calls.len());
    }

    #[test]
    fn apply_initial_settings_with_lerp_uses_moving_extent() {
        let settings = Settings::new(0.0, 0.0, 0.2, 5.0, 5, 15, 100.0, 50, 200.0);
        let mut border = WorldBorder::new(settings);
        border.apply_initial_settings(1000);
        // `lerp_time > 0` routes to `lerpSizeBetween(size, lerpTarget,
        // lerp_time, gameTime)`.
        assert_eq!(border.get_status(), BorderStatus::Growing);
        assert_eq!(border.get_lerp_time(), 50);
        assert_eq!(border.get_lerp_target(), 200.0);
        assert_eq!(border.get_size(), 100.0);
    }

    #[test]
    fn settings_compact_constructor_reads_live_values() {
        let mut border = WorldBorder::default();
        border.set_center(10.0, 20.0);
        border.set_damage_per_block(0.4);
        border.set_safe_zone(8.0);
        border.set_warning_blocks(7);
        border.set_warning_time(30);
        border.set_size(100.0);
        let settings = Settings::from_world_border(&border);
        assert_eq!(settings.center_x(), 10.0);
        assert_eq!(settings.center_z(), 20.0);
        assert_eq!(settings.damage_per_block(), 0.4);
        assert_eq!(settings.safe_zone(), 8.0);
        assert_eq!(settings.warning_blocks(), 7);
        assert_eq!(settings.warning_time(), 30);
        assert_eq!(settings.size(), 100.0);
        assert_eq!(settings.lerp_time(), 0);
        assert_eq!(settings.lerp_target(), 100.0);
    }

    #[test]
    fn current_bounds_snapshot_freezes_live_border_extent() {
        let mut border =
            WorldBorder::new(Settings::new(10.0, 20.0, 0.4, 8.0, 7, 30, 100.0, 50, 200.0));
        border.apply_initial_settings(1000);
        let snapshot = Settings::from_world_border(&border).current_bounds_snapshot();
        assert_eq!(snapshot.center_x(), 10.0);
        assert_eq!(snapshot.center_z(), 20.0);
        assert_eq!(snapshot.size(), 100.0);
        assert_eq!(snapshot.lerp_time(), 0);
        assert_eq!(snapshot.lerp_target(), 100.0);
    }

    #[test]
    fn settings_codec_round_trips_and_uses_java_field_names() {
        let settings = sample_settings();
        let codec = Settings::codec::<JsonOps>();
        let encoded = encode_json(&codec, &settings);
        // All nine fields, in Java's declaration order, with the exact DFU
        // field names. The doubleRange `center_x`/`center_z` encode as
        // floating JSON.
        assert_eq!(
            encoded,
            json!({
                "center_x": 10.0,
                "center_z": 20.0,
                "damage_per_block": 0.4,
                "safe_zone": 8.0,
                "warning_blocks": 7,
                "warning_time": 30,
                "size": 100.0,
                "lerp_time": 0,
                "lerp_target": 0.0,
            })
        );
        // Encode→decode round-trips exactly.
        let decoded = decode_json(&codec, &encoded);
        assert_eq!(decoded, settings);
    }

    #[test]
    fn settings_codec_round_trips_nonzero_lerp() {
        let settings = Settings::new(1.0, 2.0, 3.0, 4.0, 5, 6, 7.0, 8, 9.0);
        let codec = Settings::codec::<JsonOps>();
        let decoded = decode_json(&codec, &encode_json(&codec, &settings));
        assert_eq!(decoded, settings);
        assert_eq!(decoded.lerp_time(), 8);
        assert_eq!(decoded.lerp_target(), 9.0);
    }

    #[test]
    fn settings_codec_rejects_center_outside_double_range() {
        // `center_x`/`center_z` use `Codec.doubleRange(-2.9999984E7,
        // 2.9999984E7)`.
        let codec = Settings::codec::<JsonOps>();
        let bad = json!({
            "center_x": 3.0E7,
            "center_z": 0.0,
            "damage_per_block": 0.2,
            "safe_zone": 5.0,
            "warning_blocks": 5,
            "warning_time": 15,
            "size": 100.0,
            "lerp_time": 0,
            "lerp_target": 0.0,
        });
        let result = codec.parse(&JsonOps::INSTANCE, &bad);
        assert!(result.is_error(), "expected range error, got: {result:?}");
    }

    #[test]
    fn settings_codec_errors_on_missing_field() {
        // Every field is a mandatory `fieldOf`; dropping one is an error.
        let codec = Settings::codec::<JsonOps>();
        let missing = json!({
            "center_x": 0.0,
            "center_z": 0.0,
            "damage_per_block": 0.2,
            "safe_zone": 5.0,
            "warning_blocks": 5,
            "warning_time": 15,
            "size": 100.0,
            "lerp_time": 0,
        });
        let result = codec.parse(&JsonOps::INSTANCE, &missing);
        assert!(
            result.is_error(),
            "expected missing-field error, got: {result:?}"
        );
    }

    #[test]
    fn world_border_codec_round_trips_via_settings_xmap() {
        // `WorldBorder.CODEC = Settings.CODEC.xmap(WorldBorder::new,
        // Settings::new)` — the border's serialized form IS the settings
        // record (the compact constructor reads the live values). Encoding a
        // border snapshots its live state into `Settings`.
        let mut border = WorldBorder::new(sample_settings());
        border.apply_initial_settings(0);
        let codec: Arc<dyn Codec<WorldBorder, JsonOps>> = WorldBorder::codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &border)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "center_x": 10.0,
                "center_z": 20.0,
                "damage_per_block": 0.4,
                "safe_zone": 8.0,
                "warning_blocks": 7,
                "warning_time": 30,
                "size": 100.0,
                "lerp_time": 0,
                "lerp_target": 100.0,
            })
        );
        // Decode builds a border whose STORED settings are the snapshot; the
        // live fields stay at their defaults until `applyInitialSettings`
        // re-applies them (Java `new WorldBorder(Settings)` behaves the same).
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result_or_partial_silent()
            .expect("decode");
        assert_eq!(decoded.get_center_x(), 0.0);
        assert_eq!(decoded.get_size(), MAX_SIZE);
        // The stored `settings` record (private field, read here directly —
        // Java has no `getSettings()` accessor).
        assert_eq!(decoded.settings.center_x, 10.0);
        assert_eq!(decoded.settings.size, 100.0);
        assert_eq!(decoded.settings.lerp_target, 100.0);
        // Round-trip through `apply_initial_settings` reproduces the state.
        let mut applied = decoded;
        applied.apply_initial_settings(0);
        assert_eq!(applied.get_center_x(), 10.0);
        assert_eq!(applied.get_size(), 100.0);
        assert_eq!(applied.get_lerp_target(), 100.0);
    }

    #[test]
    fn type_is_world_border_in_default_namespace() {
        // `WorldBorder.TYPE = new SavedDataType<>(Identifier.
        // withDefaultNamespace("world_border"), WorldBorder::new, CODEC,
        // DataFixTypes.SAVED_DATA_WORLD_BORDER)`.
        let handle: &SavedDataType<WorldBorder> = &TYPE;
        assert_eq!(
            handle.id(),
            &Identifier::with_default_namespace("world_border")
        );
        assert_eq!(handle.data_fix_type(), DataFixTypes::SavedDataWorldBorder);
        assert_eq!(
            format!("{handle:?}"),
            "SavedDataType[minecraft:world_border]"
        );
        // Equality is by id (the `SavedDataType.equals` contract).
        assert_eq!(&*TYPE, handle);
    }

    #[test]
    fn apply_settings_is_unconditioned() {
        // Paper's `applySettings` runs the same setter sequence but without
        // the `initialized` guard.
        let mut border = WorldBorder::default();
        border.apply_initial_settings(0); // initializes
        border.apply_settings(sample_settings());
        assert_eq!(border.get_center_x(), 10.0);
        assert_eq!(border.get_center_z(), 20.0);
        assert_eq!(border.get_damage_per_block(), 0.4);
        assert_eq!(border.get_size(), 100.0);
    }
}

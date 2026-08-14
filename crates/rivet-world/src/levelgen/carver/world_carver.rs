//! Port of `net.minecraft.world.level.levelgen.carver.WorldCarver` (abstract
//! class, 26.2) — the carver identity/behavior split plus the full #180 carve
//! algorithm.
//!
//! Java `WorldCarver<C>` is an object with identity: the three static
//! constants (`CAVE`/`NETHER_CAVE`/`CANYON`) are `register(...)` calls into
//! `BuiltInRegistries.CARVER`, and `ConfiguredWorldCarver` dispatches through
//! it. The Rust port mirrors `Feature`'s identity split (`FeatureId` +
//! `FeatureBehavior`):
//! - [`WorldCarverId`] — the registry-held identity handle (`BuiltInRegistries.
//!   CARVER` element identity) plus its registry-key location. The three
//!   carver constants land here (`CAVE`/`NETHER_CAVE`/`CANYON`, ids 0/1/2 in
//!   registration order) — the generated `BuiltInRegistries.CARVER` table the
//!   `#126` dispatch codecs need.
//! - [`WorldCarverBehavior<C>`] — the overridable behavior contract the concrete
//!   carver structs implement (`CaveWorldCarver`/`NetherWorldCarver`/
//!   `CanyonWorldCarver`), generic over the config like Java's `WorldCarver<C>`.
//!
//! The `carve`/`isStartChunk` abstract behaviors and the protected helpers are
//! the #180 algorithm, ported in full:
//! - `carveEllipsoid` (the per-column block carving with the min/max index
//!   windows, the `xd²+zd² < 1.0` gate and the `CarveSkipChecker` per-block
//!   test).
//! - `carveBlock` — the block write + grass/myc surface replacement
//!   (`topMaterial`), the `shouldScheduleFluidUpdate` post-processing mark.
//! - `getCarveState`/`getDebugState` — the lava/`aquifer.computeSubstance`/
//!   debug-state selection.
//! - `canReplaceBlock`/`canReach`/`isDebugEnabled` and the `CarveSkipChecker`
//!   interface.
//!
//! Translation notes:
//! - The `AIR`/`CAVE_AIR`/`WATER`/`LAVA` constants: `WATER`/`LAVA` are
//!   `FluidState`s (no ported `FluidState` — the `liquids` field is dead in
//!   Java, only set in the `NetherWorldCarver` constructor and never read, so
//!   it is not ported); the carve paths that need the lava/air *block* write
//!   use `Blocks::LAVA.default_block_state()`/`Blocks::CAVE_AIR.
//!   default_block_state()` (Java's `FluidState.createLegacyBlock()` /
//!   `CAVE_AIR`).
//! - `chunk.getBlockState`/`chunk.setBlockState`/`chunk.isUpgrading`/`chunk.
//!   markPosForPostProcessing` are the `ChunkAccess` block surface owned by
//!   #399; the port exposes the smallest typed seam — the [`CarveChunk`]
//!   trait — and the concrete carvers operate on `&mut dyn CarveChunk`.
//!   [`CarveChunk`] is implemented for the worldgen `ProtoChunk` (the
//!   production CARVERS-status driver `NoiseBasedChunkGenerator::apply_carvers`
//!   binds it, RivetTodo(#399)); the remaining generic `ChunkAccess` block
//!   surface the #399 unit owns still defers.
//! - `carveBlock`'s `mask` parameter is dead in Java (passed but never read);
//!   it is dropped from the Rust signature.
//! - `Mth.floor` on a double is `mth::floor_d`, on a float `mth::floor`.
//!   `Mth.PI * currentStep / dist` is float math (`Mth.PI` is `3.1415927F`),
//!   widened to the `Mth.sin(double)` argument.
//! - `isDebugEnabled(configuration)` = `SharedConstants.DEBUG_CARVERS ||
//!   configuration.debugSettings().isDebugMode()` (`rivet_core::shared_constants:
//!   :DEBUG_CARVERS`, the pinned-false debug flag).
//! - `blockState.is(block)` compares the state's block id; `blockState.is(
//!   configuration.replaceable)` is `HolderSet::contains_id`.

use crate::block::blocks::Blocks;
use crate::levelgen::carver::canyon_carver_configuration::CanyonCarverConfiguration;
use crate::levelgen::carver::canyon_world_carver::{CANYON_ID, CanyonWorldCarver};
use crate::levelgen::carver::carver_configuration::CarverConfiguration;
use crate::levelgen::carver::carving_context::CarvingContext;
use crate::levelgen::carver::cave_carver_configuration::CaveCarverConfiguration;
use crate::levelgen::carver::cave_world_carver::{
    CAVE_ID, CaveWorldCarver, NETHER_CAVE_ID, NetherWorldCarver,
};
use crate::levelgen::noise::density_function::SinglePointContext;
use crate::levelgen::noisegen::aquifer::Aquifer;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::{BlockPos, ChunkPos, Direction, MutableBlockPos, Vec3i};
use rivet_util::RandomSource;
use rivet_util::mth;
use std::fmt::Debug;

/// `SharedConstants.DEBUG_CARVERS` — the pinned-false debug flag
/// `WorldCarver.isDebugEnabled` ORs with `configuration.debugSettings`.
const DEBUG_CARVERS: bool = rivet_core::shared_constants::DEBUG_CARVERS;

/// `net.minecraft.world.level.levelgen.carver.WorldCarver<C extends
/// CarverConfiguration>` — the abstract carver base's behavior contract.
///
/// Java `WorldCarver<C>` is an object whose identity is registered in
/// `BuiltInRegistries.CARVER`; the Rust port splits it into `WorldCarverId`
/// (the identity handle `ConfiguredWorldCarver` stores) and this behavior
/// trait (Java's virtual methods). The trait is generic over the config type
/// and its `carve`/`is_start_chunk` are generic over the random source
/// (`RandomSource` is `Sized`, not object-safe), so it is *not* object-safe:
/// the `carver_is_start_chunk`/`carver_carve` dispatch match on the id and
/// downcast the erased config, calling the concrete carver's method.
pub trait WorldCarverBehavior<C: CarverConfiguration>: Debug + Send + Sync + 'static {
    /// `WorldCarver.isStartChunk(C, RandomSource)` — the abstract behavior.
    fn is_start_chunk<R: RandomSource>(&self, configuration: &C, random: &mut R) -> bool;

    /// `WorldCarver.getRange()` — `4` by default; the concrete cave/canyon
    /// carvers scale their tunnel distance off it.
    fn get_range(&self) -> i32 {
        4
    }

    /// `WorldCarver.carve(CarvingContext, C, ChunkAccess, biomeGetter,
    /// RandomSource, Aquifer, ChunkPos, CarvingMask)` — the abstract behavior
    /// every concrete carver implements (the #180 algorithm; the `biomeGetter`
    /// is folded into the `CarvingContext.topMaterial` seam).
    #[allow(clippy::too_many_arguments)]
    fn carve<R: RandomSource>(
        &self,
        context: &CarvingContext,
        configuration: &C,
        chunk: &mut dyn CarveChunk,
        random: &mut R,
        aquifer: &dyn Aquifer,
        source_chunk_pos: &ChunkPos,
        mask: &mut crate::chunk::carving_mask::CarvingMask,
    ) -> bool;

    /// `carveEllipsoid` — the protected per-ellipsoid block-carve walk. The
    /// default impl is the base `WorldCarver.carveEllipsoid` body; a concrete
    /// carver may override it (none do in 26.2 — it is ported for the
    /// `#180` surface).
    #[allow(clippy::too_many_arguments)]
    fn carve_ellipsoid(
        &self,
        context: &CarvingContext,
        configuration: &C,
        chunk: &mut dyn CarveChunk,
        aquifer: &dyn Aquifer,
        x: f64,
        y: f64,
        z: f64,
        horizontal_radius: f64,
        vertical_radius: f64,
        mask: &mut crate::chunk::carving_mask::CarvingMask,
        skip_checker: &dyn CarveSkipChecker,
    ) -> bool {
        let chunk_pos = chunk.get_pos();
        let center_x = chunk_pos.get_middle_block_x() as f64;
        let center_z = chunk_pos.get_middle_block_z() as f64;
        let max_delta = 16.0 + horizontal_radius * 2.0;
        if (x - center_x).abs() > max_delta || (z - center_z).abs() > max_delta {
            return false;
        }
        let chunk_min_x = chunk_pos.get_min_block_x();
        let chunk_min_z = chunk_pos.get_min_block_z();
        let min_x_index = (mth::floor_d(x - horizontal_radius) - chunk_min_x - 1).max(0);
        let max_x_index = (mth::floor_d(x + horizontal_radius) - chunk_min_x).min(15);
        let min_y = (mth::floor_d(y - vertical_radius) - 1).max(context.get_min_gen_y() + 1);
        let protected_blocks_on_top = if chunk.is_upgrading() { 0 } else { 7 };
        let max_y = (mth::floor_d(y + vertical_radius) + 1)
            .min(context.get_min_gen_y() + context.get_gen_depth() - 1 - protected_blocks_on_top);
        let min_z_index = (mth::floor_d(z - horizontal_radius) - chunk_min_z - 1).max(0);
        let max_z_index = (mth::floor_d(z + horizontal_radius) - chunk_min_z).min(15);
        let mut carved = false;
        let mut block_pos = MutableBlockPos::new(0, 0, 0);
        let mut helper_pos = MutableBlockPos::new(0, 0, 0);

        for x_index in min_x_index..=max_x_index {
            let world_x = chunk_pos.get_block_x(x_index);
            let xd = (world_x as f64 + 0.5 - x) / horizontal_radius;

            for z_index in min_z_index..=max_z_index {
                let world_z = chunk_pos.get_block_z(z_index);
                let zd = (world_z as f64 + 0.5 - z) / horizontal_radius;
                if xd * xd + zd * zd >= 1.0 {
                    continue;
                }
                let mut has_grass = false;
                let mut world_y = max_y;
                while world_y > min_y {
                    let yd = (world_y as f64 - 0.5 - y) / vertical_radius;
                    if !skip_checker.should_skip(context, xd, yd, zd, world_y)
                        && (!mask.get(x_index, world_y, z_index) || is_debug_enabled(configuration))
                    {
                        mask.set(x_index, world_y, z_index);
                        block_pos.set(world_x, world_y, world_z);
                        carved |= self.carve_block(
                            context,
                            configuration,
                            chunk,
                            &mut block_pos,
                            &mut helper_pos,
                            aquifer,
                            &mut has_grass,
                        );
                    }
                    world_y -= 1;
                }
            }
        }

        carved
    }

    /// `carveBlock` — the protected per-block carve (the grass/myc detection,
    /// the `canReplaceBlock`/`isDebugEnabled` gate, the carve state write, the
    /// fluid post-processing mark and the `topMaterial` surface replacement).
    /// `NetherWorldCarver` overrides it. Java's `mask` parameter is unused and
    /// dropped.
    #[allow(clippy::too_many_arguments)]
    fn carve_block(
        &self,
        context: &CarvingContext,
        configuration: &C,
        chunk: &mut dyn CarveChunk,
        block_pos: &mut MutableBlockPos,
        helper_pos: &mut MutableBlockPos,
        aquifer: &dyn Aquifer,
        has_grass: &mut bool,
    ) -> bool {
        let block_state = chunk.get_block_state(&block_pos.immutable());
        if block_state.block() == Blocks::GRASS_BLOCK.id()
            || block_state.block() == Blocks::MYCELIUM.id()
        {
            *has_grass = true;
        }

        if !self.can_replace_block(configuration, block_state) && !is_debug_enabled(configuration) {
            return false;
        }

        let Some(state) = get_carve_state(context, configuration, &block_pos.immutable(), aquifer)
        else {
            return false;
        };

        chunk.set_block_state(&block_pos.immutable(), state);
        if aquifer.should_schedule_fluid_update() && !state.fluid_empty() {
            chunk.mark_pos_for_post_processing(&block_pos.immutable());
        }

        if *has_grass {
            // Java `setWithOffset(Vec3i, Direction)` — the mutable positions
            // are `BlockPos.MutableBlockPos` (a `Vec3i`); Rust converts to the
            // `Vec3i` value.
            helper_pos.set_with_offset(
                &Vec3i::new(block_pos.get_x(), block_pos.get_y(), block_pos.get_z()),
                &Direction::Down,
            );
            if chunk.get_block_state(&helper_pos.immutable()).block() == Blocks::DIRT.id()
                && let Some(top_material) =
                    context.top_material(&helper_pos.immutable(), !state.fluid_empty())
            {
                chunk.set_block_state(&helper_pos.immutable(), top_material);
                if !top_material.fluid_empty() {
                    chunk.mark_pos_for_post_processing(&helper_pos.immutable());
                }
            }
        }

        true
    }

    /// `canReplaceBlock` — `state.is(configuration.replaceable)` (the
    /// `HolderSet<Block>` membership by block id).
    fn can_replace_block(&self, configuration: &C, state: BlockState) -> bool {
        configuration
            .replaceable()
            .contains_id(state.block().id() as u32)
    }
}

/// `getCarveState` — the private `WorldCarver.getCarveState` helper (not
/// virtual): below the lava level the carved state is lava, otherwise the
/// aquifer substance (with the debug-mode barrier replacement on `None`).
fn get_carve_state<C: CarverConfiguration>(
    context: &CarvingContext,
    configuration: &C,
    block_pos: &BlockPos,
    aquifer: &dyn Aquifer,
) -> Option<BlockState> {
    if block_pos.get_y()
        <= configuration
            .lava_level()
            .resolve_y(context.world_context())
    {
        return Some(Blocks::LAVA.default_block_state());
    }
    let point = SinglePointContext::new(block_pos.get_x(), block_pos.get_y(), block_pos.get_z());
    match aquifer.compute_substance(&point, 0.0) {
        None => {
            if is_debug_enabled(configuration) {
                Some(configuration.debug_settings().barrier_state())
            } else {
                None
            }
        }
        Some(state) => {
            if is_debug_enabled(configuration) {
                Some(get_debug_state(configuration, state))
            } else {
                Some(state)
            }
        }
    }
}

/// `getDebugState` — the private static debug-state replacement: AIR →
/// `airState`, WATER → `waterState` (WATERLOGGED set when the debug block
/// supports it), LAVA → `lavaState`.
fn get_debug_state(configuration: &dyn CarverConfiguration, state: BlockState) -> BlockState {
    if state.block() == Blocks::AIR.id() {
        configuration.debug_settings().air_state()
    } else if state.block() == Blocks::WATER.id() {
        let debug_state = configuration.debug_settings().water_state();
        if debug_state.has_property(BlockStateProperties::WATERLOGGED) {
            debug_state
                .set_value(BlockStateProperties::WATERLOGGED, true)
                .expect("WATERLOGGED is a valid value on a waterlogged-supporting state")
        } else {
            debug_state
        }
    } else if state.block() == Blocks::LAVA.id() {
        configuration.debug_settings().lava_state()
    } else {
        state
    }
}

/// `isDebugEnabled` — the private static `SharedConstants.DEBUG_CARVERS ||
/// configuration.debugSettings.isDebugMode()`.
fn is_debug_enabled(configuration: &dyn CarverConfiguration) -> bool {
    DEBUG_CARVERS || configuration.debug_settings().is_debug_mode()
}

/// `canReach` — the protected static tunnel-distance gate
/// (`xd² + zd² - remaining² <= (thickness + 18)²`).
pub fn can_reach(
    chunk_pos: &ChunkPos,
    x: f64,
    z: f64,
    current_step: i32,
    total_steps: i32,
    thickness: f32,
) -> bool {
    let x_mid = chunk_pos.get_middle_block_x() as f64;
    let z_mid = chunk_pos.get_middle_block_z() as f64;
    let xd = x - x_mid;
    let zd = z - z_mid;
    let remaining = total_steps.wrapping_sub(current_step) as f64;
    // Java: `double rr = thickness + 2.0F + 16.0F` — float math widened to
    // double (`2.0F + 16.0F = 18.0F`).
    let rr = (thickness + 2.0_f32 + 16.0_f32) as f64;
    xd * xd + zd * zd - remaining * remaining <= rr * rr
}

/// `WorldCarver.CarveSkipChecker` — the per-block skip predicate
/// `carveEllipsoid` consults (`CaveWorldCarver.shouldSkip`,
/// `CanyonWorldCarver.shouldSkip`).
pub trait CarveSkipChecker: Send + Sync {
    /// `shouldSkip(CarvingContext, double xd, double yd, double zd, int y)`.
    fn should_skip(&self, context: &CarvingContext, xd: f64, yd: f64, zd: f64, y: i32) -> bool;
}

/// A `CarveSkipChecker` backed by a closure (Java's anonymous-lambda
/// `CarveSkipChecker`).
pub struct ClosureSkipChecker<F>(pub F)
where
    F: Fn(&CarvingContext, f64, f64, f64, i32) -> bool + Send + Sync;

impl<F> CarveSkipChecker for ClosureSkipChecker<F>
where
    F: Fn(&CarvingContext, f64, f64, f64, i32) -> bool + Send + Sync,
{
    fn should_skip(&self, context: &CarvingContext, xd: f64, yd: f64, zd: f64, y: i32) -> bool {
        (self.0)(context, xd, yd, zd, y)
    }
}

// ---------------------------------------------------------------------------
// The `ChunkAccess` block-surface seam (RivetTodo(#399) — bound for the
// worldgen ProtoChunk by the CARVERS-status driver)
// ---------------------------------------------------------------------------

/// The smallest typed seam for the `ChunkAccess`/`ProtoChunk` block surface the
/// carvers write through — `getBlockState`/`setBlockState`/`isUpgrading`/
/// `markPosForPostProcessing`/`getPos` (Java's `ChunkAccess` used directly in
/// `WorldCarver`).
///
/// RivetTodo(#399): implemented for the worldgen `ProtoChunk`
/// (`crate::chunk::proto_chunk`) and bound by the production CARVERS-status
/// driver `NoiseBasedChunkGenerator::apply_carvers`; the remaining generic
/// `ChunkAccess` block-state surface the #399 unit owns still defers.
pub trait CarveChunk: Send + Sync {
    /// `ChunkAccess.getPos()`.
    fn get_pos(&self) -> ChunkPos;
    /// `ChunkAccess.isUpgrading()` — the `protectedBlocksOnTop` selector.
    fn is_upgrading(&self) -> bool;
    /// `ChunkAccess.getBlockState(BlockPos)`.
    fn get_block_state(&self, pos: &BlockPos) -> BlockState;
    /// `ChunkAccess.setBlockState(BlockPos, BlockState)`.
    fn set_block_state(&mut self, pos: &BlockPos, state: BlockState);
    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — the
    /// `shouldScheduleFluidUpdate` post-processing mark.
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos);
}

// ---------------------------------------------------------------------------
// Carver identity + the dispatch hubs
// ---------------------------------------------------------------------------

/// `net.minecraft.core.Registry` element identity for `BuiltInRegistries.CARVER`
/// — the per-carver `u32` id (element id == holder id == network id ==
/// insertion index, OWNERSHIP.md §Registries) plus the registry-key location
/// (`register("cave", …)` → `minecraft:cave`). `ConfiguredWorldCarver` holds
/// this handle; the ids match the `CAVE`/`NETHER_CAVE`/`CANYON` registration
/// order in `WorldCarver.java`. Identity-semantic (not `Copy`), mirroring
/// `FeatureId`/`PlacementModifierTypeId` — but, unlike `FeatureId` (which
/// resolves the registry-key location through the generated `#181`
/// registration table rather than storing it on the id), it deliberately
/// carries the location because the carver registrations are 3 hand-portable
/// constants, not codegen content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldCarverId {
    /// The per-carver `u32` identity (insertion index in the carver registry).
    pub id: u32,
    /// The registry-key location of the carver's registration (`register("cave",
    /// …)` → `minecraft:cave`).
    pub location: &'static str,
}

impl WorldCarverId {
    /// `new WorldCarverId(u32, location)` — a carver's registry identity.
    pub const fn new(id: u32, location: &'static str) -> WorldCarverId {
        WorldCarverId { id, location }
    }

    /// `WorldCarver.CAVE` — `register("cave", new CaveWorldCarver(...))`, id 0.
    pub const CAVE: WorldCarverId = WorldCarverId::new(CAVE_ID, "minecraft:cave");
    /// `WorldCarver.NETHER_CAVE` — `register("nether_cave", new
    /// NetherWorldCarver(...))`, id 1.
    pub const NETHER_CAVE: WorldCarverId =
        WorldCarverId::new(NETHER_CAVE_ID, "minecraft:nether_cave");
    /// `WorldCarver.CANYON` — `register("canyon", new CanyonWorldCarver(...))`,
    /// id 2.
    pub const CANYON: WorldCarverId = WorldCarverId::new(CANYON_ID, "minecraft:canyon");
}

/// Resolve a `WorldCarverId` + erased config to its start-chunk test — the
/// `ConfiguredWorldCarver.isStartChunk` dispatch (`this.worldCarver.
/// isStartChunk(this.config, random)`). The three carver bindings are the
/// concrete carvers of this unit; the unknown-id path panics as a Rust-only
/// safety net — Java's `isStartChunk` calls `this.worldCarver.isStartChunk(
/// ...)` on a live, always-registered carver object, so there is no Java
/// failure path to mirror (the by-name `Registry.getValueOrThrow` is the #126
/// codec path, not start-chunk dispatch).
pub fn carver_is_start_chunk<R: RandomSource>(
    world_carver: WorldCarverId,
    config: &dyn CarverConfiguration,
    random: &mut R,
) -> bool {
    match world_carver.id {
        CAVE_ID => CaveWorldCarver.is_start_chunk(
            config
                .as_any()
                .downcast_ref::<CaveCarverConfiguration>()
                .expect("'cave' carver config is a CaveCarverConfiguration"),
            random,
        ),
        NETHER_CAVE_ID => NetherWorldCarver.is_start_chunk(
            config
                .as_any()
                .downcast_ref::<CaveCarverConfiguration>()
                .expect("'nether_cave' carver config is a CaveCarverConfiguration"),
            random,
        ),
        CANYON_ID => CanyonWorldCarver.is_start_chunk(
            config
                .as_any()
                .downcast_ref::<CanyonCarverConfiguration>()
                .expect("'canyon' carver config is a CanyonCarverConfiguration"),
            random,
        ),
        _ => panic!(
            "Trying to check start chunk for world carver '{}' with no registered behavior",
            world_carver.location
        ),
    }
}

/// Resolve a `WorldCarverId` + erased config to a carve — the
/// `ConfiguredWorldCarver.carve` dispatch (`this.worldCarver.carve(this.config,
/// …)`). Same match-and-downcast as `carver_is_start_chunk`, with the same
/// Rust-only safety-net panic on an unknown id (no Java analogue — Java
/// dispatches on the live carver object).
#[allow(clippy::too_many_arguments)]
pub fn carver_carve<R: RandomSource>(
    world_carver: WorldCarverId,
    config: &dyn CarverConfiguration,
    context: &CarvingContext,
    chunk: &mut dyn CarveChunk,
    random: &mut R,
    aquifer: &dyn Aquifer,
    source_chunk_pos: &ChunkPos,
    mask: &mut crate::chunk::carving_mask::CarvingMask,
) -> bool {
    match world_carver.id {
        CAVE_ID => CaveWorldCarver.carve(
            context,
            config
                .as_any()
                .downcast_ref::<CaveCarverConfiguration>()
                .expect("'cave' carver config is a CaveCarverConfiguration"),
            chunk,
            random,
            aquifer,
            source_chunk_pos,
            mask,
        ),
        NETHER_CAVE_ID => NetherWorldCarver.carve(
            context,
            config
                .as_any()
                .downcast_ref::<CaveCarverConfiguration>()
                .expect("'nether_cave' carver config is a CaveCarverConfiguration"),
            chunk,
            random,
            aquifer,
            source_chunk_pos,
            mask,
        ),
        CANYON_ID => CanyonWorldCarver.carve(
            context,
            config
                .as_any()
                .downcast_ref::<CanyonCarverConfiguration>()
                .expect("'canyon' carver config is a CanyonCarverConfiguration"),
            chunk,
            random,
            aquifer,
            source_chunk_pos,
            mask,
        ),
        _ => panic!(
            "Trying to carve with world carver '{}' with no registered behavior",
            world_carver.location
        ),
    }
}

// `WorldCarver.configured(C)` — `new ConfiguredWorldCarver<>(this,
// configuration)`. The constructor's `configuredCodec`/`configuredCodec()`
// (the `#126` dispatch codec surface, `codec.fieldOf("config").xmap(...)`)
// defer with the by-name codec unit; the record construction itself is
// `ConfiguredWorldCarver::new`. The concrete carvers' `maxDistance`
// computation uses `SectionPos::section_to_block_coord(get_range() * 2 - 1)`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::carver::carver_configuration::CarverConfigurationBase;

    /// A carver that keeps `getRange()`'s Java default of 4.
    #[derive(Debug)]
    struct DefaultRangeCarver;

    impl WorldCarverBehavior<CarverConfigurationBase> for DefaultRangeCarver {
        fn is_start_chunk<R: RandomSource>(
            &self,
            _configuration: &CarverConfigurationBase,
            _random: &mut R,
        ) -> bool {
            false
        }
        fn carve<R: RandomSource>(
            &self,
            _context: &CarvingContext,
            _configuration: &CarverConfigurationBase,
            _chunk: &mut dyn CarveChunk,
            _random: &mut R,
            _aquifer: &dyn Aquifer,
            _source_chunk_pos: &ChunkPos,
            _mask: &mut crate::chunk::carving_mask::CarvingMask,
        ) -> bool {
            false
        }
    }

    #[test]
    fn get_range_defaults_to_four() {
        assert_eq!(DefaultRangeCarver.get_range(), 4);
    }

    #[test]
    fn id_carries_the_registry_location() {
        assert_eq!(WorldCarverId::CAVE.id, 0);
        assert_eq!(WorldCarverId::CAVE.location, "minecraft:cave");
        assert_eq!(WorldCarverId::NETHER_CAVE.id, 1);
        assert_eq!(WorldCarverId::NETHER_CAVE.location, "minecraft:nether_cave");
        assert_eq!(WorldCarverId::CANYON.id, 2);
        assert_eq!(WorldCarverId::CANYON.location, "minecraft:canyon");
    }

    #[test]
    fn can_reach_matches_the_java_formula() {
        // Java `canReach` = `xd² + zd² - remaining² <= (thickness + 18)²`.
        let pos = ChunkPos::new(0, 0);
        // xd = zd = 0, remaining = 10 - 2 = 8, rr = 1 + 18 = 19 → 0 - 64 <= 361.
        assert!(can_reach(&pos, 8.0, 8.0, 2, 10, 1.0));
        // A position far beyond reach: xd = zd = 100 → 20000 - 64 > 361.
        assert!(!can_reach(&pos, 108.0, 108.0, 2, 10, 1.0));
    }
}

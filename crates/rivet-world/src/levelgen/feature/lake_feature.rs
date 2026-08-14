//! Port of `net.minecraft.world.level.levelgen.feature.LakeFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.lake` manifest unit.
//!
//! Java: `Feature<LakeFeature.Configuration>` (deprecated in 26.2) that carves
//! a surface lake. `place` first rejects origins too close to the world floor
//! (`origin.getY() <= level.getMinY() + 4`), then works in the
//! `origin.offset(-8, -4, -8)` local frame. It fills a `boolean[2048]` grid
//! (indexed `(xx * 16 + zz) * 8 + yy` over the 16×16×8 cell cube) with
//! `spots = random.nextInt(4) + 4` ellipsoid lobes, each drawn as six
//! `nextDouble`s (`xr`/`yr`/`zr` radii and `xp`/`yp`/`zp` centers) and marked
//! cell-wise when `d < 1.0`. After the grid, the feature samples the fluid
//! provider, then:
//!
//! 1. **Validation** — every cell adjacent (in the `(±1,0,0)`/`(0,0,±1)`/
//!    `(0,±1,0)` sense) to a marked cell that is itself unmarked must pass the
//!    boundary gates: `yy >= 4` cells may not be liquid, `yy < 4` cells must be
//!    solid or already the fluid, and the `canPlaceFeature` predicate must hold.
//!    Any violation returns `false` before any write.
//! 2. **Placement** — every marked cell whose `canReplaceWithAirOrFluid`
//!    predicate holds is written: cells at `yy >= 4` become `Blocks.CAVE_AIR`
//!    (with a `scheduleTick(placePos, AIR.getBlock(), 0)` and
//!    `markAboveForPostProcessing`), cells below become the sampled fluid.
//! 3. **Barrier** — unless the sampled barrier is air, every unmarked cell
//!    adjacent to a marked cell that is either below the fluid level
//!    (`yy < 4`) or passes a `1/2` roll (`random.nextInt(2) != 0`) is replaced
//!    with the barrier where it is solid and the `canReplaceWithBarrier`
//!    predicate holds.
//! 4. **Freeze** — when the fluid's `FluidState.is(FluidTags.WATER)`, every
//!    surface cell at `yy == 4` whose biome `shouldFreeze`s and whose
//!    `canReplaceWithAirOrFluid` predicate holds becomes `Blocks.ICE`. The
//!    loop is structurally deferred: `try_freeze_surface` is an explicit no-op
//!    (the sole RivetTodo(#232) omission in this unit) because `shouldFreeze` reads
//!    the `LevelReader` block surface that the #232 `world.level` slice has
//!    not ported. A water lake placed in a freezing biome therefore writes no
//!    ice surface — a tracked behavioral divergence, not a wrong port (the
//!    Java loop draws no RNG, so the draw stream is exact).
//!
//! Returns `true` unconditionally once the validation loop passes. The draw
//! stream is exact: the six `nextDouble`s per spot, then the barrier loop's
//! `nextInt(2)` per candidate cell. `shouldFreeze` reads the `LevelReader`
//! block surface (`getBlockState`/`getFluidState`/`getBrightness`/`isWaterAt`/
//! `isInsideBuildHeight`) that the #232 `world.level` slice has not ported
//! (see `biome.rs`'s `shouldFreeze`/`shouldSnow` defer), so the freeze loop's
//! biome test is unavailable; the RNG stream preceding it (the barrier loop's
//! `nextInt(2)` draws) is unaffected, and the loop is skipped honestly (the
//! same capability-unavailable seam as the `#232` world seams). The block
//! reads (`get_block_state`), writes (`set_block` with `UPDATE_CLIENTS`),
//! block scheduled ticks (`schedule_block_tick`), post-processing marks, and
//! biome reads all go through the `WorldGenLevel` seams; the test double
//! overrides them.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::BlockPredicate;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use crate::levelgen::feature::mark_above_for_post_processing;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_get_state,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `Block.UPDATE_CLIENTS` — the write-flag constant LakeFeature.place passes
/// to `level.setBlock` directly (Java `Block.UPDATE_CLIENTS`), in contrast to
/// `Feature.setBlock`'s `Block.UPDATE_ALL` used by e.g. SpikeFeature.
const UPDATE_CLIENTS: u32 = 2;

/// `LakeFeature.AIR` — `Blocks.CAVE_AIR.defaultBlockState()` (the "air" the
/// upper lake cells are filled with, distinct from `Blocks.AIR`).
fn cave_air() -> BlockState {
    BlockState::of(Blocks::CAVE_AIR.id())
}

/// `net.minecraft.world.level.levelgen.feature.LakeFeature`.
#[derive(Debug)]
pub struct LakeFeature;

/// `Feature.LAKE` — the registered `minecraft:lake` singleton.
pub const LAKE: LakeFeature = LakeFeature;

/// `LakeFeature.Configuration` (nested record) — the five-component
/// configuration: the fluid/barrier `BlockStateProvider`s and the three
/// `BlockPredicate` gates. Java nests the record inside `LakeFeature.java`, so
/// the port keeps it in this module (the leaf's own MANIFEST row).
#[derive(Debug, Clone)]
pub struct Configuration {
    /// `fluid` — the state provider for the lake's fluid cells.
    pub fluid: Arc<dyn ErasedBlockStateProvider>,
    /// `barrier` — the state provider for the boundary barrier cells.
    pub barrier: Arc<dyn ErasedBlockStateProvider>,
    /// `canPlaceFeature` — the validation-loop placement gate.
    pub can_place_feature: Arc<dyn BlockPredicate>,
    /// `canReplaceWithAirOrFluid` — the placement/freeze write gate.
    pub can_replace_with_air_or_fluid: Arc<dyn BlockPredicate>,
    /// `canReplaceWithBarrier` — the barrier write gate.
    pub can_replace_with_barrier: Arc<dyn BlockPredicate>,
}

impl Configuration {
    /// `new Configuration(BlockStateProvider, BlockStateProvider,
    /// BlockPredicate, BlockPredicate, BlockPredicate)` — the record
    /// constructor (the codec's `apply` function).
    pub fn new(
        fluid: Arc<dyn ErasedBlockStateProvider>,
        barrier: Arc<dyn ErasedBlockStateProvider>,
        can_place_feature: Arc<dyn BlockPredicate>,
        can_replace_with_air_or_fluid: Arc<dyn BlockPredicate>,
        can_replace_with_barrier: Arc<dyn BlockPredicate>,
    ) -> Self {
        Configuration {
            fluid,
            barrier,
            can_place_feature,
            can_replace_with_air_or_fluid,
            can_replace_with_barrier,
        }
    }

    /// `Configuration.fluid()`.
    pub fn fluid(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.fluid
    }

    /// `Configuration.barrier()`.
    pub fn barrier(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.barrier
    }

    /// `Configuration.canPlaceFeature()`.
    pub fn can_place_feature(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_place_feature
    }

    /// `Configuration.canReplaceWithAirOrFluid()`.
    pub fn can_replace_with_air_or_fluid(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_replace_with_air_or_fluid
    }

    /// `Configuration.canReplaceWithBarrier()`.
    pub fn can_replace_with_barrier(&self) -> &Arc<dyn BlockPredicate> {
        &self.can_replace_with_barrier
    }
}

/// `LakeFeature.Configuration.CODEC` — a record codec over the five required
/// fields, as the ops-generic `lake_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     BlockStateProvider.CODEC.fieldOf("fluid"),
///     BlockStateProvider.CODEC.fieldOf("barrier"),
///     BlockPredicate.CODEC.fieldOf("can_place_feature"),
///     BlockPredicate.CODEC.fieldOf("can_replace_with_air_or_fluid"),
///     BlockPredicate.CODEC.fieldOf("can_replace_with_barrier"))
///     .apply(i, LakeFeature.Configuration::new))
/// ```
pub fn lake_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Configuration, Ops>> {
    use crate::levelgen::blockpredicates::block_predicate_codec;
    use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_codec;
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &Configuration| c.fluid.clone()),
                codec::field_of(block_state_provider_codec::<Ops>(), "fluid".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &Configuration| c.barrier.clone()),
                codec::field_of(block_state_provider_codec::<Ops>(), "barrier".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &Configuration| c.can_place_feature.clone()),
                codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "can_place_feature".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &Configuration| c.can_replace_with_air_or_fluid.clone()),
                codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "can_replace_with_air_or_fluid".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &Configuration| c.can_replace_with_barrier.clone()),
                codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "can_replace_with_barrier".to_string(),
                ),
            ))
            .apply(instance, Arc::new(Configuration::new))
    })
}

impl FeatureConfiguration for Configuration {}

/// `LakeFeature.isOnBoundary(boolean[], int, int, int)` — the grid adjacency
/// test every unmarked candidate uses: `true` when the cell at `(xx, yy, zz)`
/// is unmarked and at least one of its six axis-neighbours (bounds-checked)
/// is marked.
///
/// Java:
/// ```java
/// boolean check = !grid[(xx * 16 + zz) * 8 + yy]
///     && (xx < 15 && grid[((xx + 1) * 16 + zz) * 8 + yy]
///         || xx > 0 && grid[((xx - 1) * 16 + zz) * 8 + yy]
///         || zz < 15 && grid[(xx * 16 + zz + 1) * 8 + yy]
///         || zz > 0 && grid[(xx * 16 + (zz - 1)) * 8 + yy]
///         || yy < 7 && grid[(xx * 16 + zz) * 8 + yy + 1]
///         || yy > 0 && grid[(xx * 16 + zz) * 8 + (yy - 1)]);
/// ```
fn is_on_boundary(grid: &[bool; 2048], xx: usize, yy: usize, zz: usize) -> bool {
    if grid[(xx * 16 + zz) * 8 + yy] {
        return false;
    }
    let mut neighbour_marked = false;
    if xx < 15 && grid[((xx + 1) * 16 + zz) * 8 + yy] {
        neighbour_marked = true;
    }
    if !neighbour_marked && xx > 0 && grid[((xx - 1) * 16 + zz) * 8 + yy] {
        neighbour_marked = true;
    }
    if !neighbour_marked && zz < 15 && grid[(xx * 16 + zz + 1) * 8 + yy] {
        neighbour_marked = true;
    }
    if !neighbour_marked && zz > 0 && grid[(xx * 16 + (zz - 1)) * 8 + yy] {
        neighbour_marked = true;
    }
    if !neighbour_marked && yy < 7 && grid[(xx * 16 + zz) * 8 + (yy + 1)] {
        neighbour_marked = true;
    }
    if !neighbour_marked && yy > 0 && grid[(xx * 16 + zz) * 8 + (yy - 1)] {
        neighbour_marked = true;
    }
    neighbour_marked
}

/// `BlockState.liquid()` — the `liquid` behavior property flag
/// (`BlockBehaviour.Properties.liquid()`), set on the WATER, LAVA, and
/// BUBBLE_COLUMN blocks only (verified in `Blocks.java`: the three
/// `.liquid()` sites). It is a per-block property, not the fluid state: a
/// waterlogged non-liquid block has a non-empty `getFluidState()` but
/// `liquid() == false`. Every state of those three blocks carries the flag
/// (it is copied from the properties at state construction), so the owning
/// block id is the faithful gate.
///
/// Java:
/// ```java
/// BlockBehaviour.BlockStateBase.liquid()  // returns this.liquid
/// ```
fn is_liquid_block(block_state: BlockState) -> bool {
    let id = block_state.block();
    id == Blocks::WATER.id() || id == Blocks::LAVA.id() || id == Blocks::BUBBLE_COLUMN.id()
}

impl FeatureBehavior<Configuration> for LakeFeature {
    /// `LakeFeature.place(FeaturePlaceContext<Configuration>)`.
    ///
    /// The freeze loop is structurally deferred to `try_freeze_surface`, which
    /// is an explicit no-op: its `shouldFreeze` reads the `LevelReader` block
    /// surface, which the #232 world slice has not ported (the biome read goes
    /// through the `get_biome` seam's `RivetTodo(#232)` panic). The RNG stream
    /// is exact: `random.nextInt(4)`, then the six `nextDouble`s per spot,
    /// then the barrier loop's `nextInt(2)` per candidate cell.
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, Configuration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        // `origin` is reassigned to the local frame, so it is an owned
        // `BlockPos` (`BlockPos` is `Copy`); `**origin` derefs the context's
        // `&BlockPos` reference field.
        let mut origin = **origin;
        if origin.get_y() <= level.get_min_y().wrapping_add(4) {
            return false;
        }

        origin = origin.offset(-8, -4, -8);
        let mut grid = [false; 2048];
        let spots = random.next_int_bound(4).wrapping_add(4);

        for _ in 0..spots {
            let xr = random.next_double() * 6.0 + 3.0;
            let yr = random.next_double() * 4.0 + 2.0;
            let zr = random.next_double() * 6.0 + 3.0;
            let xp = random.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
            let yp = random.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
            let zp = random.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;

            for xx in 1..15 {
                for zz in 1..15 {
                    for yy in 1..7 {
                        let xd = (xx as f64 - xp) / (xr / 2.0);
                        let yd = (yy as f64 - yp) / (yr / 2.0);
                        let zd = (zz as f64 - zp) / (zr / 2.0);
                        let d = xd * xd + yd * yd + zd * zd;
                        if d < 1.0 {
                            grid[(xx * 16 + zz) * 8 + yy] = true;
                        }
                    }
                }
            }
        }

        let fluid = block_state_provider_get_state(config.fluid().as_ref(), level, random, &origin);

        // The validation loop: any boundary violation returns `false` before a
        // single write. Java `BlockState.liquid()` is the block's `liquid`
        // property flag (`BlockBehaviour.Properties.liquid()`), set on the
        // WATER, LAVA, and BUBBLE_COLUMN blocks only — a per-block property,
        // independent of the fluid state. It is distinct from
        // `getFluidState().isEmpty()` (`fluid_empty()`): a waterlogged
        // non-liquid block has a non-empty fluid state but `liquid() == false`,
        // so the gate must be the owning block's id. `isSolid()` is the
        // `legacySolid` property. The `blockState != fluid` test is value
        // equality.
        for xx in 0..16 {
            for zz in 0..16 {
                for yy in 0..8 {
                    if is_on_boundary(&grid, xx, yy, zz) {
                        let offset_pos = origin.offset(xx as i32, yy as i32, zz as i32);
                        let block_state = level.get_block_state(&offset_pos);
                        if yy >= 4 && is_liquid_block(block_state) {
                            return false;
                        }
                        if yy < 4 && !block_state.is_solid() && block_state != fluid {
                            return false;
                        }
                        if !config.can_place_feature().test(level, &offset_pos) {
                            return false;
                        }
                    }
                }
            }
        }

        // The placement loop: every marked cell whose write gate holds is
        // filled — cave air above `yy >= 4` (with a block scheduled tick and a
        // post-processing mark), the sampled fluid below.
        for xx in 0..16 {
            for zz in 0..16 {
                for yy in 0..8 {
                    if grid[(xx * 16 + zz) * 8 + yy] {
                        let place_pos = origin.offset(xx as i32, yy as i32, zz as i32);
                        if config
                            .can_replace_with_air_or_fluid()
                            .test(level, &place_pos)
                        {
                            let place_air = yy >= 4;
                            level.set_block(
                                &place_pos,
                                if place_air { cave_air() } else { fluid },
                                UPDATE_CLIENTS,
                            );
                            if place_air {
                                level.schedule_block_tick(&place_pos, Blocks::CAVE_AIR, 0);
                                mark_above_for_post_processing(level, &place_pos);
                            }
                        }
                    }
                }
            }
        }

        let barrier =
            block_state_provider_get_state(config.barrier().as_ref(), level, random, &origin);
        if !barrier.is_air() {
            // The barrier loop: `yy < 4 || random.nextInt(2) != 0` — the
            // barrier fills the boundary cells below the fluid and half the
            // boundary cells above it. The roll is skipped when `yy < 4`
            // (Java's short-circuit), so the draw stream matches exactly.
            for xx in 0..16 {
                for zz in 0..16 {
                    for yy in 0..8 {
                        if is_on_boundary(&grid, xx, yy, zz)
                            && (yy < 4 || random.next_int_bound(2) != 0)
                        {
                            let offset = origin.offset(xx as i32, yy as i32, zz as i32);
                            let block_state = level.get_block_state(&offset);
                            if block_state.is_solid()
                                && config.can_replace_with_barrier().test(level, &offset)
                            {
                                level.set_block(&offset, barrier, UPDATE_CLIENTS);
                                mark_above_for_post_processing(level, &offset);
                            }
                        }
                    }
                }
            }
        }

        // The freeze loop (`fluid.getFluidState().is(FluidTags.WATER)` then the
        // `yy == 4` surface scan) is deferred: `try_freeze_surface` below is
        // the structural marker that the Java loop is skipped — see its
        // RivetTodo(#232) note. It is a call, not a dead `if` guard, so a
        // reader cannot mistake it for working freeze logic.
        try_freeze_surface(level, config, fluid);

        true
    }
}

/// `LakeFeature.place`'s freeze loop, deferred.
///
/// Java writes `Blocks.ICE` at the `yy == 4` surface cells:
///
/// ```java
/// if (fluid.getFluidState().is(FluidTags.WATER)) {
///     for (int xx = 0; xx < 16; xx++) {
///         for (int zz = 0; zz < 16; zz++) {
///             BlockPos offset = origin.offset(xx, 4, zz);
///             if (level.getBiome(offset).value().shouldFreeze(level, offset, false)
///                 && config.canReplaceWithAirOrFluid.test(level, offset)) {
///                 level.setBlock(offset, Blocks.ICE.defaultBlockState(), Block.UPDATE_CLIENTS);
///             }
///         }
///     }
/// }
/// ```
///
/// RivetTodo(#232): `Biome.shouldFreeze(level, offset, false)` reads the
/// `LevelReader` block surface (`getBlockState`/`getFluidState`/
/// `getBrightness`/`isWaterAt`/`isInsideBuildHeight`), which the #232
/// `world.level` value slice has not ported (see `biome.rs`'s
/// `shouldFreeze`/`shouldSnow` defer). The loop is skipped until that surface
/// lands — an honest omission, not a fabricated verdict; the RNG stream is
/// unaffected (the barrier loop's draws already happened). This is a tracked
/// behavior gap, not a wrong port: in Java a water lake in a freezing biome
/// writes `Blocks.ICE` at the `yy == 4` surface cells; here those writes are
/// skipped. Re-enable the loop verbatim when the #232 surface lands — its two
/// prerequisites are `WorldGenLevel::get_biome` (default panics today) and
/// `Biome::should_freeze` (not yet ported).
///
/// The guard, `fluid.getFluidState().is(FluidTags.WATER)`, is
/// `BlockState.getFluidState().is(FluidTags.WATER)` — a `minecraft:water`
/// fluid-tag membership test on the state's fluid type (`fluid_id`, the
/// `BuiltInRegistries.FLUID.getId(getFluidState().getType())` encoding). The
/// tag contains BOTH `minecraft:water` (id 2, the still source) AND
/// `minecraft:flowing_water` (id 1), so the re-enabled guard must test the
/// generated tag set (`FLUID_TAG_BY_NAME["minecraft:water"]` resolved through
/// `FLUID_BY_NAME`), not a single-id equality — a state whose fluid type is the
/// flowing-water variant freezes in Paper but would miss a single-id check.
fn try_freeze_surface(_level: &mut dyn WorldGenLevel, _config: &Configuration, _fluid: BlockState) {
    // Deferred: the freeze-loop body is the Java loop in the doc note above
    // (with the tag-set guard semantics). The empty body is the structural
    // marker that the loop is not ported yet — an explicit no-op, not a dead
    // `if` guard a reader could mistake for working freeze logic.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::blockpredicates::always_true;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    fn water() -> BlockState {
        BlockState::of(Blocks::WATER.id())
    }

    fn stone() -> BlockState {
        BlockState::of(Blocks::STONE.id())
    }

    fn air() -> BlockState {
        BlockState::of(Blocks::AIR.id())
    }

    /// A water-lake config whose providers are constant (`simple`) and whose
    /// predicates are all `always_true`. The barrier is air, so the barrier
    /// loop's `!barrier.isAir()` guard skips it entirely — the tests focus on
    /// the placement loop's fluid/air writes and their scheduling.
    fn config() -> Configuration {
        Configuration::new(
            Arc::new(simple(water())),
            Arc::new(simple(air())),
            always_true(),
            always_true(),
            always_true(),
        )
    }

    /// The local-frame cube (`origin.offset(-8, -4, -8)` = `(-8, 60, -8)`,
    /// 16×8×16) filled with stone — the surrounding terrain the lake carves
    /// into. The validation loop's boundary gates need solid (`isSolid`) cells
    /// below the fluid level and non-liquid cells above; air fails the
    /// `yy < 4` gate (`!isSolid() && != fluid`).
    fn fill_local_cube_with_stone(level: &mut TestLevel) {
        for x in -8..8 {
            for y in 60..68 {
                for z in -8..8 {
                    level.states.insert(BlockPos::new(x, y, z), stone());
                }
            }
        }
    }

    fn place_with(level: &mut TestLevel, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        LAKE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &config(),
        ))
    }

    /// `block_state_provider_codec`/`block_predicate_codec` dispatch over the
    /// registry-backed providers/predicates, so the codec requires `RegistryOps`
    /// (the `RegistryOpsLookup` ops). An empty access is enough — the providers
    /// here are `simple` and the predicates `always_true`.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    /// A water/stone config with all three predicates `always_true` — the codec
    /// fixture: the fluid and barrier exercise the `BlockStateProvider.CODEC`
    /// dispatch (water is non-singleton, so its state carries a `Properties`
    /// compound), the three predicates the `BlockPredicate.CODEC` dispatch.
    fn codec_config() -> Configuration {
        Configuration::new(
            Arc::new(simple(water())),
            Arc::new(simple(stone())),
            always_true(),
            always_true(),
            always_true(),
        )
    }

    #[test]
    fn lake_configuration_codec_round_trips() {
        let codec = lake_configuration_codec::<TestOps>();
        let ops = ops();
        let config = codec_config();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        // `RecordCodecBuilder` emits the five fields in Java's group order, and
        // each provider/predicate dispatch writes its value fields before the
        // `"type"` key (Java's KeyDispatchCodec order). The water state is
        // non-singleton (a `level` property), so it carries the `Properties`
        // compound; stone is singleton and encodes name-only.
        assert_eq!(
            encoded,
            json!({
                "fluid": {"state": {"Properties": {"level": "0"}, "Name": "minecraft:water"}, "type": "minecraft:simple_state_provider"},
                "barrier": {"state": {"Name": "minecraft:stone"}, "type": "minecraft:simple_state_provider"},
                "can_place_feature": {"type": "minecraft:true"},
                "can_replace_with_air_or_fluid": {"type": "minecraft:true"},
                "can_replace_with_barrier": {"type": "minecraft:true"},
            })
        );
        // Pin the byte order too — indexmap map equality is order-insensitive,
        // so the `json!` assertion alone cannot catch a regression that emits a
        // field (or a `"type"` key) out of Java's order.
        assert_eq!(
            serde_json::to_string(&encoded).expect("encode is json"),
            r#"{"fluid":{"state":{"Properties":{"level":"0"},"Name":"minecraft:water"},"type":"minecraft:simple_state_provider"},"barrier":{"state":{"Name":"minecraft:stone"},"type":"minecraft:simple_state_provider"},"can_place_feature":{"type":"minecraft:true"},"can_replace_with_air_or_fluid":{"type":"minecraft:true"},"can_replace_with_barrier":{"type":"minecraft:true"}}"#
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        // The provider/predicate halves are behavior carriers; equality is by
        // dispatch identity (the `"type"` key), which is what the codec
        // round-trips — pinning the constructor order (fluid, barrier, then the
        // three predicates in field order).
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**decoded.fluid()),
            ErasedBlockStateProvider::type_id(&**config.fluid())
        );
        assert_eq!(
            ErasedBlockStateProvider::type_id(&**decoded.barrier()),
            ErasedBlockStateProvider::type_id(&**config.barrier())
        );
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_place_feature()),
            BlockPredicate::type_id(&**config.can_place_feature())
        );
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_replace_with_air_or_fluid()),
            BlockPredicate::type_id(&**config.can_replace_with_air_or_fluid())
        );
        assert_eq!(
            BlockPredicate::type_id(&**decoded.can_replace_with_barrier()),
            BlockPredicate::type_id(&**config.can_replace_with_barrier())
        );
    }

    #[test]
    fn lake_configuration_codec_requires_all_fields() {
        let codec = lake_configuration_codec::<TestOps>();
        let ops = ops();
        assert!(codec.parse(&ops, &json!({})).is_error());
        // Missing the last predicate field.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "fluid": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:water"}},
                        "barrier": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}},
                        "can_place_feature": {"type": "minecraft:true"},
                        "can_replace_with_air_or_fluid": {"type": "minecraft:true"},
                    })
                )
                .is_error()
        );
        // Missing the fluid provider field.
        assert!(
            codec
                .parse(
                    &ops,
                    &json!({
                        "barrier": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}},
                        "can_place_feature": {"type": "minecraft:true"},
                        "can_replace_with_air_or_fluid": {"type": "minecraft:true"},
                        "can_replace_with_barrier": {"type": "minecraft:true"},
                    })
                )
                .is_error()
        );
    }

    /// The `origin.getY() <= level.getMinY() + 4` gate: `getMinY() = -64`, so
    /// an origin at `y <= -60` returns `false` before any draw or write.
    #[test]
    fn origin_below_floor_gate_returns_false() {
        let mut level = TestLevel::over(access());
        assert!(!place_with(&mut level, BlockPos::new(0, -60, 0)));
        assert!(level.writes.is_empty());
        assert!(level.ticks.is_empty());
    }

    /// The spots' six `nextDouble`s per lobe — pinned end to end by exercising
    /// the real `LAKE.place` through `RecordingRandom` (which records every
    /// draw) and asserting the golden write set for the pinned seed. `place`
    /// draws exactly `nextInt(4)` for the lobe count then `6 * spots`
    /// `nextDouble`s for the radii/centers; the `simple` providers and
    /// `always_true` predicates draw nothing, and the barrier loop is skipped
    /// (air barrier). A divergent draw formula (e.g. a different radius
    /// multiplier) changes the radii, hence the marked-cell grid, hence the
    /// golden write set — so the golden positions and count pin the draw
    /// stream's *values*, not just its shape.
    ///
    /// Provenance: the golden write set was captured from the Rust
    /// implementation run for seed 1 (a `LegacyRandomSource(1)` over the
    /// stone-filled local cube), not from an independent Paper oracle probe —
    /// there is no lake feature probe in `rivet-oracle`. It therefore pins the
    /// Rust port's draw stream (its self-consistency: the lobe-count draw, the
    /// `6 * spots` doubles, and the exact cells those radii mark), not an
    /// independent Paper value. The write rule (fluid below, cave air at/above
    /// `yy == 4`) and the loop structure were verified against `LakeFeature.java`
    /// during translation, but a symmetric error in both the formula and the
    /// goldens could pass. Treat a change to this golden as a review trigger,
    /// not a routine update.
    #[test]
    fn spot_draw_stream_is_exact() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        fill_local_cube_with_stone(&mut level);
        let generator = TestGenerator;
        let mut random = RecordingRandom::new(1);
        assert!(LAKE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config(),
        )));
        // Exactly `nextInt(4)` then `6 * spots` `nextDouble`s — and nothing
        // else (the `simple` providers and `always_true` predicates draw no
        // RNG).
        assert_eq!(random.calls.first(), Some(&RngCall::IntBound(4)));
        assert!(
            random.calls[1..].iter().all(|c| *c == RngCall::Double),
            "every non-count draw must be a nextDouble, got {:?}",
            random.calls
        );
        let spots = (random.calls.len() - 1) / 6;
        assert!((4..=7).contains(&spots), "spots in 4..=7, got {spots}");
        assert_eq!(random.calls.len(), 1 + 6 * spots);
        // The golden write set — the marked cells written for seed 1 (94 water
        // cells below the fluid level at absolute y < 64, 166 cave-air cells at
        // or above), asserted position-by-position with the state rule on top.
        let golden: Vec<BlockPos> = vec![
            BlockPos::new(-6, 62, -5),
            BlockPos::new(-6, 63, -5),
            BlockPos::new(-6, 64, -5),
            BlockPos::new(-6, 65, -5),
            BlockPos::new(-6, 62, -4),
            BlockPos::new(-6, 63, -4),
            BlockPos::new(-6, 64, -4),
            BlockPos::new(-6, 65, -4),
            BlockPos::new(-6, 62, -3),
            BlockPos::new(-6, 63, -3),
            BlockPos::new(-6, 64, -3),
            BlockPos::new(-6, 65, -3),
            BlockPos::new(-5, 62, -6),
            BlockPos::new(-5, 63, -6),
            BlockPos::new(-5, 64, -6),
            BlockPos::new(-5, 65, -6),
            BlockPos::new(-5, 62, -5),
            BlockPos::new(-5, 63, -5),
            BlockPos::new(-5, 64, -5),
            BlockPos::new(-5, 65, -5),
            BlockPos::new(-5, 61, -4),
            BlockPos::new(-5, 62, -4),
            BlockPos::new(-5, 63, -4),
            BlockPos::new(-5, 64, -4),
            BlockPos::new(-5, 65, -4),
            BlockPos::new(-5, 62, -3),
            BlockPos::new(-5, 63, -3),
            BlockPos::new(-5, 64, -3),
            BlockPos::new(-5, 65, -3),
            BlockPos::new(-5, 62, -2),
            BlockPos::new(-5, 63, -2),
            BlockPos::new(-5, 64, -2),
            BlockPos::new(-5, 65, -2),
            BlockPos::new(-4, 62, -6),
            BlockPos::new(-4, 63, -6),
            BlockPos::new(-4, 64, -6),
            BlockPos::new(-4, 65, -6),
            BlockPos::new(-4, 61, -5),
            BlockPos::new(-4, 62, -5),
            BlockPos::new(-4, 63, -5),
            BlockPos::new(-4, 64, -5),
            BlockPos::new(-4, 65, -5),
            BlockPos::new(-4, 66, -5),
            BlockPos::new(-4, 61, -4),
            BlockPos::new(-4, 62, -4),
            BlockPos::new(-4, 63, -4),
            BlockPos::new(-4, 64, -4),
            BlockPos::new(-4, 65, -4),
            BlockPos::new(-4, 66, -4),
            BlockPos::new(-4, 61, -3),
            BlockPos::new(-4, 62, -3),
            BlockPos::new(-4, 63, -3),
            BlockPos::new(-4, 64, -3),
            BlockPos::new(-4, 65, -3),
            BlockPos::new(-4, 66, -3),
            BlockPos::new(-4, 62, -2),
            BlockPos::new(-4, 63, -2),
            BlockPos::new(-4, 64, -2),
            BlockPos::new(-4, 65, -2),
            BlockPos::new(-3, 62, -6),
            BlockPos::new(-3, 63, -6),
            BlockPos::new(-3, 64, -6),
            BlockPos::new(-3, 65, -6),
            BlockPos::new(-3, 61, -5),
            BlockPos::new(-3, 62, -5),
            BlockPos::new(-3, 63, -5),
            BlockPos::new(-3, 64, -5),
            BlockPos::new(-3, 65, -5),
            BlockPos::new(-3, 61, -4),
            BlockPos::new(-3, 62, -4),
            BlockPos::new(-3, 63, -4),
            BlockPos::new(-3, 64, -4),
            BlockPos::new(-3, 65, -4),
            BlockPos::new(-3, 66, -4),
            BlockPos::new(-3, 61, -3),
            BlockPos::new(-3, 62, -3),
            BlockPos::new(-3, 63, -3),
            BlockPos::new(-3, 64, -3),
            BlockPos::new(-3, 65, -3),
            BlockPos::new(-3, 66, -3),
            BlockPos::new(-3, 62, -2),
            BlockPos::new(-3, 63, -2),
            BlockPos::new(-3, 64, -2),
            BlockPos::new(-3, 65, -2),
            BlockPos::new(-2, 63, -6),
            BlockPos::new(-2, 64, -6),
            BlockPos::new(-2, 62, -5),
            BlockPos::new(-2, 63, -5),
            BlockPos::new(-2, 64, -5),
            BlockPos::new(-2, 65, -5),
            BlockPos::new(-2, 62, -4),
            BlockPos::new(-2, 63, -4),
            BlockPos::new(-2, 64, -4),
            BlockPos::new(-2, 65, -4),
            BlockPos::new(-2, 62, -3),
            BlockPos::new(-2, 63, -3),
            BlockPos::new(-2, 64, -3),
            BlockPos::new(-2, 65, -3),
            BlockPos::new(-2, 62, -2),
            BlockPos::new(-2, 63, -2),
            BlockPos::new(-2, 64, -2),
            BlockPos::new(-2, 65, -2),
            BlockPos::new(-2, 64, -1),
            BlockPos::new(-1, 63, -5),
            BlockPos::new(-1, 64, -5),
            BlockPos::new(-1, 62, -4),
            BlockPos::new(-1, 63, -4),
            BlockPos::new(-1, 64, -4),
            BlockPos::new(-1, 65, -4),
            BlockPos::new(-1, 62, -3),
            BlockPos::new(-1, 63, -3),
            BlockPos::new(-1, 64, -3),
            BlockPos::new(-1, 65, -3),
            BlockPos::new(-1, 63, -2),
            BlockPos::new(-1, 64, -2),
            BlockPos::new(-1, 65, -2),
            BlockPos::new(-1, 63, -1),
            BlockPos::new(-1, 64, -1),
            BlockPos::new(-1, 65, -1),
            BlockPos::new(-1, 63, 0),
            BlockPos::new(-1, 64, 0),
            BlockPos::new(-1, 65, 0),
            BlockPos::new(-1, 63, 1),
            BlockPos::new(-1, 64, 1),
            BlockPos::new(-1, 65, 1),
            BlockPos::new(-1, 66, 1),
            BlockPos::new(-1, 63, 2),
            BlockPos::new(-1, 64, 2),
            BlockPos::new(-1, 65, 2),
            BlockPos::new(-1, 66, 2),
            BlockPos::new(-1, 63, 3),
            BlockPos::new(-1, 64, 3),
            BlockPos::new(-1, 65, 3),
            BlockPos::new(-1, 66, 3),
            BlockPos::new(-1, 64, 4),
            BlockPos::new(-1, 65, 4),
            BlockPos::new(0, 64, -6),
            BlockPos::new(0, 63, -5),
            BlockPos::new(0, 64, -5),
            BlockPos::new(0, 65, -5),
            BlockPos::new(0, 63, -4),
            BlockPos::new(0, 64, -4),
            BlockPos::new(0, 65, -4),
            BlockPos::new(0, 62, -3),
            BlockPos::new(0, 63, -3),
            BlockPos::new(0, 64, -3),
            BlockPos::new(0, 65, -3),
            BlockPos::new(0, 62, -2),
            BlockPos::new(0, 63, -2),
            BlockPos::new(0, 64, -2),
            BlockPos::new(0, 65, -2),
            BlockPos::new(0, 66, -2),
            BlockPos::new(0, 62, -1),
            BlockPos::new(0, 63, -1),
            BlockPos::new(0, 64, -1),
            BlockPos::new(0, 65, -1),
            BlockPos::new(0, 63, 0),
            BlockPos::new(0, 64, 0),
            BlockPos::new(0, 65, 0),
            BlockPos::new(0, 66, 0),
            BlockPos::new(0, 63, 1),
            BlockPos::new(0, 64, 1),
            BlockPos::new(0, 65, 1),
            BlockPos::new(0, 66, 1),
            BlockPos::new(0, 63, 2),
            BlockPos::new(0, 64, 2),
            BlockPos::new(0, 65, 2),
            BlockPos::new(0, 66, 2),
            BlockPos::new(0, 63, 3),
            BlockPos::new(0, 64, 3),
            BlockPos::new(0, 65, 3),
            BlockPos::new(0, 66, 3),
            BlockPos::new(0, 63, 4),
            BlockPos::new(0, 64, 4),
            BlockPos::new(0, 65, 4),
            BlockPos::new(0, 66, 4),
            BlockPos::new(1, 64, -5),
            BlockPos::new(1, 65, -5),
            BlockPos::new(1, 63, -4),
            BlockPos::new(1, 64, -4),
            BlockPos::new(1, 65, -4),
            BlockPos::new(1, 63, -3),
            BlockPos::new(1, 64, -3),
            BlockPos::new(1, 65, -3),
            BlockPos::new(1, 63, -2),
            BlockPos::new(1, 64, -2),
            BlockPos::new(1, 65, -2),
            BlockPos::new(1, 63, -1),
            BlockPos::new(1, 64, -1),
            BlockPos::new(1, 65, -1),
            BlockPos::new(1, 63, 0),
            BlockPos::new(1, 64, 0),
            BlockPos::new(1, 65, 0),
            BlockPos::new(1, 66, 0),
            BlockPos::new(1, 63, 1),
            BlockPos::new(1, 64, 1),
            BlockPos::new(1, 65, 1),
            BlockPos::new(1, 66, 1),
            BlockPos::new(1, 62, 2),
            BlockPos::new(1, 63, 2),
            BlockPos::new(1, 64, 2),
            BlockPos::new(1, 65, 2),
            BlockPos::new(1, 66, 2),
            BlockPos::new(1, 63, 3),
            BlockPos::new(1, 64, 3),
            BlockPos::new(1, 65, 3),
            BlockPos::new(1, 66, 3),
            BlockPos::new(1, 63, 4),
            BlockPos::new(1, 64, 4),
            BlockPos::new(1, 65, 4),
            BlockPos::new(1, 66, 4),
            BlockPos::new(1, 64, 5),
            BlockPos::new(1, 65, 5),
            BlockPos::new(2, 63, -5),
            BlockPos::new(2, 64, -5),
            BlockPos::new(2, 65, -5),
            BlockPos::new(2, 63, -4),
            BlockPos::new(2, 64, -4),
            BlockPos::new(2, 65, -4),
            BlockPos::new(2, 63, -3),
            BlockPos::new(2, 64, -3),
            BlockPos::new(2, 65, -3),
            BlockPos::new(2, 63, -2),
            BlockPos::new(2, 64, -2),
            BlockPos::new(2, 65, -2),
            BlockPos::new(2, 64, -1),
            BlockPos::new(2, 65, -1),
            BlockPos::new(2, 64, 0),
            BlockPos::new(2, 65, 0),
            BlockPos::new(2, 66, 0),
            BlockPos::new(2, 63, 1),
            BlockPos::new(2, 64, 1),
            BlockPos::new(2, 65, 1),
            BlockPos::new(2, 66, 1),
            BlockPos::new(2, 63, 2),
            BlockPos::new(2, 64, 2),
            BlockPos::new(2, 65, 2),
            BlockPos::new(2, 66, 2),
            BlockPos::new(2, 63, 3),
            BlockPos::new(2, 64, 3),
            BlockPos::new(2, 65, 3),
            BlockPos::new(2, 66, 3),
            BlockPos::new(2, 64, 4),
            BlockPos::new(2, 65, 4),
            BlockPos::new(2, 66, 4),
            BlockPos::new(3, 63, -4),
            BlockPos::new(3, 64, -4),
            BlockPos::new(3, 65, -4),
            BlockPos::new(3, 64, -3),
            BlockPos::new(3, 65, -3),
            BlockPos::new(3, 64, -2),
            BlockPos::new(3, 65, -2),
            BlockPos::new(3, 64, 1),
            BlockPos::new(3, 65, 1),
            BlockPos::new(3, 64, 2),
            BlockPos::new(3, 65, 2),
            BlockPos::new(3, 66, 2),
            BlockPos::new(3, 64, 3),
            BlockPos::new(3, 65, 3),
            BlockPos::new(4, 64, -2),
        ];
        assert_eq!(level.writes.len(), golden.len(), "write count");
        for (i, (pos, state)) in level.writes.iter().enumerate() {
            // `BlockPos` derives no `PartialEq`, so compare field-wise.
            assert_eq!(pos.get_x(), golden[i].get_x(), "write #{i} x");
            assert_eq!(pos.get_y(), golden[i].get_y(), "write #{i} y");
            assert_eq!(pos.get_z(), golden[i].get_z(), "write #{i} z");
            if pos.get_y() < 64 {
                assert_eq!(*state, water(), "write #{i} below the fluid level");
            } else {
                assert_eq!(*state, cave_air(), "write #{i} at/above the fluid level");
            }
        }
    }

    /// The barrier loop is parity-critical (the module doc claims the `1/2`
    /// roll matches Java) yet the shared `config()` helper's air barrier skips
    /// it in every other test. With a stone barrier, the loop runs: the
    /// `yy < 4` boundary cells are replaced unconditionally (Java's
    /// short-circuit draws no RNG) and the `yy >= 4` boundary cells each draw
    /// `nextInt(2)`. This test pins the draw count/order and the write set for
    /// seed 1 on the stone-filled local cube (every cell solid, so the
    /// `isSolid` gate holds for every boundary cell and the writes are exactly
    /// the cells the draw gate admits).
    ///
    /// Provenance: as in `spot_draw_stream_is_exact`, the counts and golden
    /// write sets were captured from the Rust implementation run for seed 1,
    /// not from an independent Paper oracle probe (none exists for `LakeFeature`
    /// in `rivet-oracle`). The `nextInt(2)` count (one per `yy >= 4` boundary
    /// cell), the unconditional `yy < 4` writes, and the roll-dependent subset
    /// therefore pin the Rust port's behavior — a symmetric error in the port
    /// and the goldens could pass. The 1/2-roll semantics (`nextInt(2) != 0`)
    /// were verified against `LakeFeature.java` during translation. Treat a
    /// change to this golden as a review trigger, not a routine update.
    #[test]
    fn barrier_loop_draw_stream_and_writes_are_exact() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        fill_local_cube_with_stone(&mut level);
        let generator = TestGenerator;
        let mut random = RecordingRandom::new(1);
        let config = Configuration::new(
            Arc::new(simple(water())),
            Arc::new(simple(stone())),
            always_true(),
            always_true(),
            always_true(),
        );
        assert!(LAKE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        )));
        // Draw stream: `nextInt(4)`, then `6 * spots` `nextDouble`s, then
        // exactly `K = 148` `nextInt(2)` draws — one per boundary cell at
        // `yy >= 4`. The 106 boundary cells below the fluid level draw nothing
        // (the `yy < 4 ||` short-circuit): K equals the count of `yy >= 4`
        // boundary cells, which is what pins the short-circuit.
        let mut idx = 1;
        let mut spot_draws = 0;
        while idx < random.calls.len() && random.calls[idx] == RngCall::Double {
            spot_draws += 1;
            idx += 1;
        }
        assert_eq!(random.calls.first(), Some(&RngCall::IntBound(4)));
        assert_eq!(spot_draws % 6, 0, "nextDouble draws come in 6 per lobe");
        let spots = spot_draws / 6;
        assert!((4..=7).contains(&spots), "spots in 4..=7, got {spots}");
        let barrier_draws = &random.calls[idx..];
        assert_eq!(
            barrier_draws.len(),
            148,
            "K = the 148 yy>=4 boundary cells each draw nextInt(2)"
        );
        assert!(
            barrier_draws.iter().all(|c| *c == RngCall::IntBound(2)),
            "barrier draws must all be nextInt(2), got {:?}",
            barrier_draws
        );
        // The write set: 260 placement writes (every marked cell) then 176
        // barrier writes — 106 at `y < 64` (unconditional below the fluid
        // level) and 70 at `y >= 64` (the `nextInt(2) != 0` half). Every
        // barrier write is stone.
        assert_eq!(level.writes.len(), 260 + 176, "placement + barrier writes");
        let barrier = &level.writes[260..];
        assert!(barrier.iter().all(|(_, s)| *s == stone()));
        let below = barrier.iter().filter(|(p, _)| p.get_y() < 64).count();
        let at_or_above = barrier.iter().filter(|(p, _)| p.get_y() >= 64).count();
        assert_eq!(below, 106);
        assert_eq!(at_or_above, 70);
        // The 70 `y >= 64` writes are the roll-dependent half: their exact
        // positions pin the `nextInt(2)` *values* (a different roll subset
        // would write a different set), exactly as `spot_draw_stream_is_exact`
        // pins the `nextDouble` values through its golden write set.
        let mut at_writes: Vec<BlockPos> = barrier
            .iter()
            .filter(|(p, _)| p.get_y() >= 64)
            .map(|(p, _)| *p)
            .collect();
        at_writes.sort_by_key(|p| (p.get_x(), p.get_y(), p.get_z()));
        let golden_at: Vec<BlockPos> = vec![
            BlockPos::new(-7, 64, -3),
            BlockPos::new(-6, 64, -2),
            BlockPos::new(-6, 66, -5),
            BlockPos::new(-6, 66, -3),
            BlockPos::new(-5, 64, -1),
            BlockPos::new(-5, 65, -1),
            BlockPos::new(-5, 66, -6),
            BlockPos::new(-5, 66, -4),
            BlockPos::new(-4, 64, -7),
            BlockPos::new(-4, 65, -7),
            BlockPos::new(-4, 65, -1),
            BlockPos::new(-4, 66, -6),
            BlockPos::new(-3, 64, -1),
            BlockPos::new(-3, 65, -1),
            BlockPos::new(-3, 66, -2),
            BlockPos::new(-3, 67, -4),
            BlockPos::new(-2, 65, -6),
            BlockPos::new(-2, 65, 1),
            BlockPos::new(-2, 65, 2),
            BlockPos::new(-2, 65, 4),
            BlockPos::new(-2, 66, -4),
            BlockPos::new(-2, 66, 1),
            BlockPos::new(-1, 64, -6),
            BlockPos::new(-1, 65, -5),
            BlockPos::new(-1, 65, 5),
            BlockPos::new(-1, 66, -3),
            BlockPos::new(-1, 66, -2),
            BlockPos::new(-1, 66, -1),
            BlockPos::new(-1, 66, 0),
            BlockPos::new(-1, 67, 1),
            BlockPos::new(0, 64, -7),
            BlockPos::new(0, 64, 5),
            BlockPos::new(0, 66, -4),
            BlockPos::new(0, 66, -3),
            BlockPos::new(0, 66, -1),
            BlockPos::new(0, 67, -2),
            BlockPos::new(0, 67, 0),
            BlockPos::new(0, 67, 1),
            BlockPos::new(1, 64, -6),
            BlockPos::new(1, 64, 6),
            BlockPos::new(1, 65, -6),
            BlockPos::new(1, 66, -3),
            BlockPos::new(1, 66, -2),
            BlockPos::new(1, 67, 0),
            BlockPos::new(1, 67, 1),
            BlockPos::new(1, 67, 2),
            BlockPos::new(1, 67, 4),
            BlockPos::new(2, 65, -6),
            BlockPos::new(2, 66, -5),
            BlockPos::new(2, 66, -4),
            BlockPos::new(2, 66, -3),
            BlockPos::new(2, 66, -2),
            BlockPos::new(2, 66, 5),
            BlockPos::new(2, 67, 0),
            BlockPos::new(2, 67, 1),
            BlockPos::new(2, 67, 2),
            BlockPos::new(3, 64, -5),
            BlockPos::new(3, 64, 4),
            BlockPos::new(3, 66, -3),
            BlockPos::new(3, 66, -2),
            BlockPos::new(3, 66, 0),
            BlockPos::new(3, 66, 1),
            BlockPos::new(3, 66, 4),
            BlockPos::new(4, 64, -3),
            BlockPos::new(4, 64, 2),
            BlockPos::new(4, 65, -3),
            BlockPos::new(4, 65, 1),
            BlockPos::new(4, 65, 2),
            BlockPos::new(4, 65, 3),
            BlockPos::new(4, 66, 2),
        ];
        assert_eq!(at_writes.len(), golden_at.len(), "at/above barrier count");
        for (i, pos) in at_writes.iter().enumerate() {
            // `BlockPos` derives no `PartialEq`, so compare field-wise.
            assert_eq!(pos.get_x(), golden_at[i].get_x(), "at/above write #{i} x");
            assert_eq!(pos.get_y(), golden_at[i].get_y(), "at/above write #{i} y");
            assert_eq!(pos.get_z(), golden_at[i].get_z(), "at/above write #{i} z");
        }
    }

    /// A full water lake: the placement loop writes fluid below `yy < 4` and
    /// cave air at `yy >= 4` (with a block tick + post-processing mark for
    /// each air cell). The barrier is air (skipped) and the `shouldFreeze` gate
    /// is skipped (the #232 deferral), so every write is fluid or cave air.
    #[test]
    fn lake_places_fluid_air_and_barrier() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        fill_local_cube_with_stone(&mut level);
        assert!(place_with(&mut level, origin));
        // The local frame origin is `(0,64,0).offset(-8,-4,-8) = (-8,60,-8)`;
        // every write is inside that 16×16×8 cube.
        assert!(level.writes.iter().all(|(p, _)| {
            p.get_x() >= -8
                && p.get_x() < 8
                && p.get_y() >= 60
                && p.get_y() < 68
                && p.get_z() >= -8
                && p.get_z() < 8
        }));
        // Every fluid write is the water state and every air write is cave air.
        for (p, s) in &level.writes {
            if p.get_y() < 64 {
                assert_eq!(*s, water());
            } else {
                assert_eq!(*s, cave_air());
            }
        }
        assert!(
            !level.block_ticks.is_empty(),
            "air cells schedule cave-air block ticks"
        );
        assert!(!level.post_processing.is_empty());
    }

    /// The cave-air cells are the upper half of the lake (`yy >= 4`), so the
    /// block scheduled ticks all target the fluid-level-and-above plane
    /// (local `yy >= 4` is absolute `y >= 64`).
    #[test]
    fn cave_air_ticks_are_upper_half() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        fill_local_cube_with_stone(&mut level);
        assert!(place_with(&mut level, origin));
        assert!(
            !level.block_ticks.is_empty(),
            "cave-air cells schedule block ticks"
        );
        assert!(
            level.block_ticks.iter().all(|(p, _, _)| p.get_y() >= 64),
            "cave-air block ticks only schedule cells at the fluid level and above"
        );
    }
}

//! Port of `net.minecraft.world.level.levelgen.feature.GeodeFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.geode` manifest unit.
//!
//! Java: `Feature<GeodeConfiguration>` that carves a layered amethyst geode.
//! `place` samples `distributionPoints` candidate points (each offset from the
//! origin by an independent `outerWallDistance` draw per axis, gated by the
//! `invalidBlocks` threshold), then for every cell in the
//! `[minGenOffset, maxGenOffset]` cube accumulates inverse-sqrt shells around
//! the points and the (optional) crack points. The shell bands — crack air,
//! filling, inner layer (alternate), middle crust, outer crust — are written
//! by the respective `BlockStateProvider`s, and cells that could hold crystals
//! are recorded. Finally each crystal candidate is dressed with
//! `FACING`/`WATERLOGGED` and placed where `BuddingAmethystBlock.
//! canClusterGrowAtState` allows. Returns `true` unconditionally after the
//! point loop (early `false` only when the `invalidBlocksThreshold` trips).
//!
//! The worldgen RNG (`WorldgenRandom` over `LegacyRandomSource(level.getSeed
//! ())`) and `NormalNoise.create(-4, 1.0)` seed the crack/point shell noise,
//! separate from the placement `random` draw stream — the port preserves the
//! two streams. The `HolderSet<Block>` tests become `contains_id` (the
//! `MatchingBlocksPredicate` mapping); `BlockState.getFluidState().isSource()`
//! becomes `fluid_id ∈ {2, 4}` (water/lava source type ids), `isFull()` for
//! the amethyst gate becomes `fluid_id == 2` (water source); the crack-carve
//! fluid rescheduling reads the neighbor's fluid id and schedules the matching
//! `FluidId` through the existing `schedule_tick` seam. All world
//! reads/writes go through the `WorldGenLevel` seams (RivetTodo #232); the
//! test double overrides them.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::GeodeConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_get_state;
use crate::levelgen::synth::normal_noise::NormalNoise;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::{BlockPos, Direction, Vec3i};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_util::RandomSource;
use rivet_util::random::LegacyRandomSource;
use rivet_util::worldgen_random::WorldgenRandom;
use rivet_util::{mth, util};

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.safeSetBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `GeodeFeature.DIRECTIONS` — `Direction.values()`.
const DIRECTIONS: [Direction; 6] = Direction::VALUES;

/// `net.minecraft.world.level.levelgen.feature.GeodeFeature`.
#[derive(Debug)]
pub struct GeodeFeature;

/// `Feature.GEODE` — the registered `minecraft:geode` singleton.
pub const GEODE: GeodeFeature = GeodeFeature;

/// `BlockState.is(HolderSet<Block>)` — the `state.is(blockSettings.invalidBlocks())`
/// gate test (`set.contains_id(state.block().id())`).
fn state_is_in(state: &BlockState, set: &HolderSet<BlockType>) -> bool {
    set.contains_id(state.block().id() as u32)
}

/// `Feature.safeSetBlock(WorldGenLevel, BlockPos, BlockState, Predicate<
/// BlockState>)` — write `state` at `pos` (`Block.UPDATE_CLIENTS`) only when
/// `canReplace.test(level.getBlockState(pos))`.
fn safe_set_block(
    level: &mut dyn WorldGenLevel,
    pos: &BlockPos,
    state: BlockState,
    can_replace: impl Fn(&BlockState) -> bool,
) {
    if can_replace(&level.get_block_state(pos)) {
        level.set_block(pos, state, UPDATE_CLIENTS);
    }
}

/// `BuddingAmethystBlock.canClusterGrowAtState(BlockState)` — `state.isAir()
/// || state.is(Blocks.WATER) && state.getFluidState().isSource()`. `isSource()`
/// for water is the source type id `2` (the `isFull()` source check).
fn can_cluster_grow_at_state(state: &BlockState) -> bool {
    state.is_air() || (state.block() == Blocks::WATER.id() && matches!(state.fluid_id(), 2))
}

/// `FluidState.isEmpty()` — the fluid-id form read through the block state
/// (`EMPTY = 0`).
fn fluid_is_empty(fluid_id: u16) -> bool {
    fluid_id == FluidId::EMPTY.0
}

/// `BlockState.getFluidState().isSource()` — water/lava source type ids.
fn fluid_is_source(fluid_id: u16) -> bool {
    matches!(fluid_id, 2 | 4)
}

impl FeatureBehavior<GeodeConfiguration> for GeodeFeature {
    /// `GeodeFeature.place(FeaturePlaceContext<GeodeConfiguration>)`.
    ///
    /// The placement `random` stream and the geode's own `WorldgenRandom`/
    /// `NormalNoise` stream are kept separate exactly as in Java: `random1` is
    /// seeded from `level.getSeed()` and only feeds the noise, never the point
    /// positions or layer draws.
    //
    // `!(dist_sum_shell < outerCrust)` is Java's literal `!(d0 < outerCrust)`
    // shell test: for `NaN` distances the negation differs from `>=`, so the
    // partially-ordered negation must be kept (clippy's `partial_cmp`
    // rewrite would change the result).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, GeodeConfiguration, R>,
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
        let origin = *origin;

        let min_gen_offset = config.min_gen_offset;
        let max_gen_offset = config.max_gen_offset;

        // `List<Pair<BlockPos, Integer>> points` — the candidate shell points.
        let mut points: Vec<(BlockPos, i32)> = Vec::new();
        let num_points = config.distribution_points.sample(random);

        // `WorldgenRandom random1 = new WorldgenRandom(new LegacyRandomSource(
        // level.getSeed())); NormalNoise noise = NormalNoise.create(random1,
        // -4, 1.0);` — the geode's independent shell-noise stream.
        let mut random1 = WorldgenRandom::new(LegacyRandomSource::new(level.get_seed()));
        let noise = NormalNoise::create_amplitudes(&mut random1, -4, &[1.0]);

        // `List<BlockPos> crackPoints`.
        let mut crack_points: Vec<BlockPos> = Vec::new();
        let crack_size_adjustment =
            num_points as f64 / config.outer_wall_distance.max_inclusive() as f64;
        let layer_settings = config.geode_layer_settings;
        let block_settings = config.geode_block_settings.clone();
        let crack_settings = config.geode_crack_settings;

        let inner_air = 1.0 / layer_settings.filling.sqrt();
        let innermost_block_layer =
            1.0 / (layer_settings.inner_layer + crack_size_adjustment).sqrt();
        let inner_crust = 1.0 / (layer_settings.middle_layer + crack_size_adjustment).sqrt();
        let outer_crust = 1.0 / (layer_settings.outer_layer + crack_size_adjustment).sqrt();
        let crack_size = 1.0
            / (crack_settings.base_crack_size
                + random.next_double() / 2.0
                + if num_points > 3 {
                    crack_size_adjustment
                } else {
                    0.0
                })
            .sqrt();
        // Java `random.nextFloat() < crackSettings.generateCrackChance`
        // promotes the `float` draw to `double` for the comparison.
        let should_generate_crack =
            (random.next_float() as f64) < crack_settings.generate_crack_chance;
        let mut num_invalid_points = 0;

        for _ in 0..num_points {
            let x = config.outer_wall_distance.sample(random);
            let y = config.outer_wall_distance.sample(random);
            let z = config.outer_wall_distance.sample(random);
            let pos = origin.offset(x, y, z);
            let state = level.get_block_state(&pos);
            if state.is_air() || state_is_in(&state, &block_settings.invalid_blocks) {
                num_invalid_points += 1;
                if num_invalid_points > config.invalid_blocks_threshold {
                    return false;
                }
            }
            points.push((pos, config.point_offset.sample(random)));
        }

        if should_generate_crack {
            let offset_index = random.next_int_bound(4);
            let crack_offset = num_points.wrapping_mul(2).wrapping_add(1);
            if offset_index == 0 {
                crack_points.push(origin.offset(crack_offset, 7, 0));
                crack_points.push(origin.offset(crack_offset, 5, 0));
                crack_points.push(origin.offset(crack_offset, 1, 0));
            } else if offset_index == 1 {
                crack_points.push(origin.offset(0, 7, crack_offset));
                crack_points.push(origin.offset(0, 5, crack_offset));
                crack_points.push(origin.offset(0, 1, crack_offset));
            } else if offset_index == 2 {
                crack_points.push(origin.offset(crack_offset, 7, crack_offset));
                crack_points.push(origin.offset(crack_offset, 5, crack_offset));
                crack_points.push(origin.offset(crack_offset, 1, crack_offset));
            } else {
                crack_points.push(origin.offset(0, 7, 0));
                crack_points.push(origin.offset(0, 5, 0));
                crack_points.push(origin.offset(0, 1, 0));
            }
        }

        // `HolderSet<Block> cantReplace = config.geodeBlockSettings().
        // cannotReplace(); Predicate<BlockState> canReplace = s -> !s.is(
        // cantReplace);`
        let cannot_replace = block_settings.cannot_replace.clone();
        let can_replace = move |s: &BlockState| !cannot_replace.contains_id(s.block().id() as u32);

        let mut potential_crystal_placements: Vec<BlockPos> = Vec::new();

        for point_inside in BlockPos::between_closed_pos(
            &origin.offset(min_gen_offset, min_gen_offset, min_gen_offset),
            &origin.offset(max_gen_offset, max_gen_offset, max_gen_offset),
        ) {
            let noise_offset = noise.get_value(
                point_inside.get_x() as f64,
                point_inside.get_y() as f64,
                point_inside.get_z() as f64,
            ) * config.noise_multiplier;
            let mut dist_sum_shell = 0.0;
            let mut dist_sum_crack = 0.0;

            for (point, point_offset) in &points {
                let dist_sq = Vec3i::new(
                    point_inside.get_x(),
                    point_inside.get_y(),
                    point_inside.get_z(),
                )
                .dist_sqr(&Vec3i::new(point.get_x(), point.get_y(), point.get_z()));
                dist_sum_shell += mth::inv_sqrt_f64(dist_sq + *point_offset as f64) + noise_offset;
            }

            for point in &crack_points {
                let dist_sq = Vec3i::new(
                    point_inside.get_x(),
                    point_inside.get_y(),
                    point_inside.get_z(),
                )
                .dist_sqr(&Vec3i::new(point.get_x(), point.get_y(), point.get_z()));
                dist_sum_crack +=
                    mth::inv_sqrt_f64(dist_sq + crack_settings.crack_point_offset as f64)
                        + noise_offset;
            }

            if !(dist_sum_shell < outer_crust) {
                if should_generate_crack
                    && dist_sum_crack >= crack_size
                    && dist_sum_shell < inner_air
                {
                    safe_set_block(
                        level,
                        &point_inside,
                        Blocks::AIR.default_block_state(),
                        &can_replace,
                    );

                    for direction in DIRECTIONS {
                        let adjacent_pos = point_inside.relative(&direction);
                        let adjacent_fluid_id = level.get_block_state(&adjacent_pos).fluid_id();
                        if !fluid_is_empty(adjacent_fluid_id) {
                            level.schedule_tick(
                                &adjacent_pos,
                                FluidId::from_id(adjacent_fluid_id),
                                0,
                            );
                        }
                    }
                } else if dist_sum_shell >= inner_air {
                    let state = block_state_provider_get_state(
                        block_settings.filling_provider.as_ref(),
                        level,
                        random,
                        &point_inside,
                    );
                    safe_set_block(level, &point_inside, state, &can_replace);
                } else if dist_sum_shell >= innermost_block_layer {
                    // Java `random.nextFloat() < config.useAlternateLayer0Chance()`
                    // promotes the `float` draw to `double`.
                    let use_alternate_layer =
                        (random.next_float() as f64) < config.use_alternate_layer0_chance;
                    if use_alternate_layer {
                        let state = block_state_provider_get_state(
                            block_settings.alternate_inner_layer_provider.as_ref(),
                            level,
                            random,
                            &point_inside,
                        );
                        safe_set_block(level, &point_inside, state, &can_replace);
                    } else {
                        let state = block_state_provider_get_state(
                            block_settings.inner_layer_provider.as_ref(),
                            level,
                            random,
                            &point_inside,
                        );
                        safe_set_block(level, &point_inside, state, &can_replace);
                    }

                    if (!config.placements_require_layer0_alternate || use_alternate_layer)
                        && (random.next_float() as f64) < config.use_potential_placements_chance
                    {
                        potential_crystal_placements.push(point_inside);
                    }
                } else if dist_sum_shell >= inner_crust {
                    let state = block_state_provider_get_state(
                        block_settings.middle_layer_provider.as_ref(),
                        level,
                        random,
                        &point_inside,
                    );
                    safe_set_block(level, &point_inside, state, &can_replace);
                } else if dist_sum_shell >= outer_crust {
                    let state = block_state_provider_get_state(
                        block_settings.outer_layer_provider.as_ref(),
                        level,
                        random,
                        &point_inside,
                    );
                    safe_set_block(level, &point_inside, state, &can_replace);
                }
            }
        }

        let inner_placements = block_settings.inner_placements;

        for crystal_pos in potential_crystal_placements {
            let mut block_state = util::get_random(&inner_placements, random);

            for direction in DIRECTIONS {
                if block_state.has_property(BlockStateProperties::FACING) {
                    block_state = block_state
                        .set_value(BlockStateProperties::FACING, direction)
                        .expect("geode crystal block has the facing property");
                }

                let place_pos = crystal_pos.relative(&direction);
                let place_state = level.get_block_state(&place_pos);
                if block_state.has_property(BlockStateProperties::WATERLOGGED) {
                    let waterlogged = fluid_is_source(place_state.fluid_id());
                    block_state = block_state
                        .set_value(BlockStateProperties::WATERLOGGED, waterlogged)
                        .expect("geode crystal block has the waterlogged property");
                }

                if can_cluster_grow_at_state(&place_state) {
                    safe_set_block(level, &place_pos, block_state, &can_replace);
                    break;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::FeaturePlaceContext;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use crate::levelgen::settings::geode_block_settings::GeodeBlockSettings;
    use crate::levelgen::settings::geode_crack_settings::GeodeCrackSettings;
    use crate::levelgen::settings::geode_layer_settings::GeodeLayerSettings;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::holder::Holder;
    use rivet_registry::holder_set::HolderSet;
    use rivet_registry::registries::BlockType;
    use rivet_util::random::LegacyRandomSource;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;
    use std::sync::Arc;

    fn stone() -> BlockState {
        BlockState::of(Blocks::STONE.id())
    }

    fn air() -> BlockState {
        BlockState::of(Blocks::AIR.id())
    }

    /// A single-point geode at a fixed distance: `distributionPoints =
    /// ConstantInt(1)` and `outerWallDistance = ConstantInt(1)` put the one
    /// shell point at `origin + (1,1,1)` with `pointOffset = 0`, so the only
    /// shell cell (the origin, with `minGenOffset == maxGenOffset == 0`) has
    /// `distSumShell = invSqrt(3) ≈ 0.577`. `noiseMultiplier = 0.0` zeroes the
    /// noise term, making the bands purely geometric. `generateCrackChance =
    /// 0.0` disables the crack entirely.
    fn config() -> GeodeConfiguration {
        let block_settings = GeodeBlockSettings::new(
            Arc::new(simple(air())),         // filling
            Arc::new(simple(stone())),       // inner layer
            Arc::new(simple(air())),         // alternate inner
            Arc::new(simple(air())),         // middle
            Arc::new(simple(air())),         // outer
            vec![stone()],                   // inner placements
            HolderSet::<BlockType>::empty(), // cannot replace
            HolderSet::<BlockType>::empty(), // invalid blocks
        );
        GeodeConfiguration::new(
            block_settings,
            GeodeLayerSettings::new(1.7, 2.2, 3.2, 4.2),
            GeodeCrackSettings::new(0.0, 2.0, 2),
            0.0,                                       // use_potential_placements_chance
            0.0,                                       // use_alternate_layer0_chance
            false,                                     // placements_require_layer0_alternate
            IntProvider::Constant(ConstantInt::of(1)), // outer_wall_distance
            IntProvider::Constant(ConstantInt::of(1)), // distribution_points
            IntProvider::Constant(ConstantInt::of(0)), // point_offset
            0,                                         // min_gen_offset
            0,                                         // max_gen_offset
            0.0,                                       // noise_multiplier
            2,                                         // invalid_blocks_threshold
        )
    }

    fn place_with(level: &mut TestLevel, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        GEODE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &config(),
        ))
    }

    /// With the single-point config the origin cell falls in the innermost
    /// block-layer band (`innerCrust ≤ distSumShell < innerAir`): the inner
    /// layer provider writes stone at the origin, `useAlternateLayerChance =
    /// 0.0` guarantees the plain (non-alternate) branch, and
    /// `usePotentialPlacementsChance = 0.0` records no crystal candidate.
    #[test]
    fn geode_writes_inner_layer_state_at_origin() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        assert!(place_with(&mut level, origin));
        assert_eq!(level.writes, vec![(origin, stone())]);
        assert!(level.ticks.is_empty());
    }

    /// The `invalidBlocksThreshold` gate: with `invalidBlocks = {air}` the
    /// single candidate point (in air by default) trips `numInvalidPoints = 1
    /// > 0`, so `place` returns `false` before any write.
    #[test]
    fn geode_aborts_when_invalid_points_exceed_threshold() {
        let block_settings = GeodeBlockSettings::new(
            Arc::new(simple(air())),
            Arc::new(simple(stone())),
            Arc::new(simple(air())),
            Arc::new(simple(air())),
            Arc::new(simple(air())),
            vec![stone()],
            HolderSet::<BlockType>::empty(),
            HolderSet::<BlockType>::direct(vec![Holder::direct(BlockType)]),
        );
        let config = GeodeConfiguration::new(
            block_settings,
            GeodeLayerSettings::new(1.7, 2.2, 3.2, 4.2),
            GeodeCrackSettings::new(0.0, 2.0, 2),
            0.0,
            0.0,
            false,
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Constant(ConstantInt::of(1)),
            IntProvider::Constant(ConstantInt::of(0)),
            0,
            0,
            0.0,
            0, // threshold 0 — the single air point trips `1 > 0`.
        );
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        assert!(!GEODE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        )));
        assert!(level.writes.is_empty());
    }
}

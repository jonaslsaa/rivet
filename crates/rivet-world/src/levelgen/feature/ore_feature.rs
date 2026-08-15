//! Port of `net.minecraft.world.level.levelgen.feature.OreFeature` (26.2) — the
//! `mc.world.level.levelgen.feature.ore` manifest unit.
//!
//! Java places a blob of `size` spheres: `place` lays out the blob axis
//! (`x0..x1`/`z0..z1` along a random angle, `y0..y1` by two `nextInt(3)`
//! draws), probes the `OCEAN_FLOOR_WG` height over the blob's bounding box to
//! decide whether the blob may intersect the world, and — if so — `doPlace`
//! builds a `data[size * 4]` array of per-sphere centers/radii, runs the
//! enclosing-radius elimination pass, then walks each surviving sphere's
//! bounding box in `x,y,z` order, testing the unit-sphere equation `xd*xd +
//! yd*yd + zd*zd < 1.0` with the `BitSet` dedup, and writes the first matching
//! target state.
//!
//! The port follows the exact Java structure: [`place`] computes the axis
//! endpoints and probes the height; [`do_place`] runs the data-array /
//! elimination / per-sphere traversal faithfully; [`can_place_ore`] applies
//! the per-cell gate. The write phase routes through the `WorldGenLevel`
//! surface (`get_block_state`/`set_block`) rather than a `BulkSectionAccess`:
//! Java's `getSection` returns `null` only when the section index is out of
//! range, which for the standard section-aligned world height is exactly
//! `isOutsideBuildHeight(y)` — already excluded by the unit-sphere guard — so
//! the null branch is dead code and the level read/write is behaviorally
//! identical (the section state, AIR for an absent/empty section, reflecting
//! writes made earlier in the same blob). The write is
//! `section.setBlockState(..., false)` — a raw section write with no
//! `Block.UPDATE_*` flags — ported as `set_block(pos, state, 0)`.
//!
//! `shouldSkipAirCheck` is fully ported and reachable. Fidelity notes
//! (PORTING.md):
//! - `place` uses `Math.sin`/`Math.cos` (the JVM's libm f64) for the axis
//!   endpoints — NOT `Mth.sin`/`Mth.cos` (the 65536-entry table the radius
//!   computation uses). The two are distinct implementations and must not be
//!   conflated: the endpoints are `(dir as f64).sin()`/`.cos()`, the radius is
//!   `sin(PI * step)`.
//! - `Mth.lerp(float step, double x0, double x1)` — Java's only f64 lerp takes
//!   `(double, double, double)`, so `step` widens to f64: `lerp(step as f64,
//!   x0, x1)`.
//! - `Mth.ceil(spreadXY)` is the f32 ceil cast (saturating); `Mth.floor(xx -
//!   r)`/`Mth.floor(xx + r)` are the f64 floor overload (both operands are
//!   double) — `ceil`/`floor_d` respectively. `int` arithmetic wraps.
//! - `Math.max(Mth.floor(...), xStart)` mirrors Java's `Math.max(int, int)`.
//! - The `BitSet` index is `x - xStart + (y - yStart) * sizeXZ + (z - zStart) *
//!   sizeXZ * sizeY`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::OreConfiguration;
use crate::levelgen::feature::configurations::TargetBlockState;
use crate::levelgen::feature::is_adjacent_to_air;
use crate::levelgen::heightmap::Types;
use crate::levelgen::structure::templatesystem::rule_test::erased_test;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use rivet_util::mth::{PI, ceil, floor_d, lerp, sin};

/// `net.minecraft.world.level.levelgen.feature.OreFeature`.
#[derive(Debug)]
pub struct OreFeature;

/// `Feature.ORE` — the registered `minecraft:ore` singleton (feature registry
/// insertion index 28, the dispatch table's id).
pub const ORE: OreFeature = OreFeature;

impl FeatureBehavior<OreConfiguration> for OreFeature {
    /// `OreFeature.place(FeaturePlaceContext<OreConfiguration>)` — the blob
    /// axis layout, the `OCEAN_FLOOR_WG` height probe, then `do_place`.
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, OreConfiguration, R>,
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
        let origin: &BlockPos = origin;
        let config: &OreConfiguration = config;
        place(random, origin, level, config)
    }
}

/// `OreFeature.shouldSkipAirCheck(RandomSource, float discardChanceOnAirExposure)`
/// — `discardChanceOnAirExposure <= 0.0F || (!(discardChanceOnAirExposure >=
/// 1.0F) && random.nextFloat() >= discardChanceOnAirExposure)`. Note the Java
/// short-circuit ordering: a value in `(0.0F, 1.0F)` rolls `nextFloat() >=
/// discardChanceOnAirExposure`; a value `>= 1.0F` never skips.
///
/// `#[allow(clippy::neg_cmp_op_on_partial_ord)]`: the mechanical rewrite of
/// `!(x >= 1.0)` to `x < 1.0` is NOT behavior-preserving for f32 — for NaN the
/// original evaluates `!(NaN >= 1.0)` to `true` and rolls `nextFloat() >= NaN`
/// (consuming an RNG draw), while `NaN < 1.0` short-circuits without a draw.
/// Clippy still flags the faithful form, so the lint is suppressed on it.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn should_skip_air_check(
    random: &mut impl RandomSource,
    discard_chance_on_air_exposure: f32,
) -> bool {
    discard_chance_on_air_exposure <= 0.0
        || (!(discard_chance_on_air_exposure >= 1.0)
            && random.next_float() >= discard_chance_on_air_exposure)
}

/// `OreFeature.canPlaceOre(BlockState, Function<BlockPos, BlockState>,
/// RandomSource, OreConfiguration, TargetBlockState, BlockPos)` — the per-cell
/// placement gate.
///
/// ```java
/// return targetState.target.test(orePosState, random)
///     && (shouldSkipAirCheck(random, config.discardChanceOnAirExposure)
///         || !isAdjacentToAir(blockGetter, orePos));
/// ```
///
/// Fully faithful: the first conjunct evaluates the erased `RuleTest` via the
/// templatesystem [`erased_test`] downcast dispatch (all six Paper types in
/// that unit's scope), then the air-exposure discard reads the RNG and the six
/// axis-neighbor states through `is_adjacent_to_air`. The draw order matches
/// Java's short-circuit: a rule test on a non-matching state consumes nothing,
/// a probability roll happens only on a match, and `should_skip_air_check`
/// rolls only when the rule matched.
pub fn can_place_ore<R: RandomSource>(
    ore_pos_state: &BlockState,
    block_getter: impl Fn(&BlockPos) -> BlockState,
    random: &mut R,
    config: &OreConfiguration,
    target_state: &TargetBlockState,
    ore_pos: &BlockPos,
) -> bool {
    erased_test(&target_state.target, ore_pos_state, random)
        && (should_skip_air_check(random, config.discard_chance_on_air_exposure)
            || !is_adjacent_to_air(block_getter, ore_pos))
}

/// `OreFeature.place(FeaturePlaceContext<OreConfiguration>)` — the blob axis
/// layout plus the `OCEAN_FLOOR_WG` height probe.
///
/// The geometry and RNG draw order are faithful to the Java:
/// `dir = nextFloat() * PI`, `spreadXY = size / 8.0F`,
/// `maxRadius = ceil((size / 16.0F * 2.0F + 1.0F) / 2.0F)`, the `sin`/`cos`
/// axis endpoints, then `y0 = originY + nextInt(3) - 2` and `y1 = originY +
/// nextInt(3) - 2` (two more draws). The height probe reads
/// `level.get_height_at(Types::OceanFloorWg, xprobe, zprobe)` — a live read
/// on `WorldGenRegion` (the primed chunk heightmap, `minY + 1` fallback).
///
/// The probe order is preserved exactly: the `(x_probe, z_probe)` double loop
/// over the bounding box returns at the first probe whose height clears
/// `y_start`.
pub fn place<R: RandomSource>(
    random: &mut R,
    origin: &BlockPos,
    level: &mut dyn WorldGenLevel,
    config: &OreConfiguration,
) -> bool {
    let dir = random.next_float() * PI;
    let spread_xy = config.size as f32 / 8.0f32;
    let max_radius = ceil((config.size as f32 / 16.0f32 * 2.0f32 + 1.0f32) / 2.0f32);
    let x0 = origin.get_x() as f64 + (dir as f64).sin() * spread_xy as f64;
    let x1 = origin.get_x() as f64 - (dir as f64).sin() * spread_xy as f64;
    let z0 = origin.get_z() as f64 + (dir as f64).cos() * spread_xy as f64;
    let z1 = origin.get_z() as f64 - (dir as f64).cos() * spread_xy as f64;
    let spread_y = 2;
    let y0 = origin.get_y() as f64 + random.next_int_bound(3) as f64 - 2.0;
    let y1 = origin.get_y() as f64 + random.next_int_bound(3) as f64 - 2.0;
    let x_start = origin.get_x() - ceil(spread_xy) - max_radius;
    let y_start = origin.get_y() - spread_y - max_radius;
    let z_start = origin.get_z() - ceil(spread_xy) - max_radius;
    let size_xz = 2 * (ceil(spread_xy) + max_radius);
    let size_y = 2 * (spread_y + max_radius);

    for x_probe in x_start..=x_start + size_xz {
        for z_probe in z_start..=z_start + size_xz {
            if y_start <= level.get_height_at(Types::OceanFloorWg, x_probe, z_probe) {
                return do_place(
                    level, random, config, x0, x1, z0, z1, y0, y1, x_start, y_start, z_start,
                    size_xz, size_y,
                );
            }
        }
    }

    false
}

/// `OreFeature.doPlace(WorldGenLevel, RandomSource, OreConfiguration, double
/// x0, double x1, double z0, double z1, double y0, double y1, int xStart, int
/// yStart, int zStart, int sizeXZ, int sizeY)` — the blob data array, the
/// enclosing-radius elimination, and the per-sphere block walk.
///
/// Fidelity: `r = ((Mth.sin(PI * step) + 1.0F) * ss + 1.0) / 2.0` with
/// `ss = nextDouble() * size / 16.0`; the `!(r <= 0.0)` elimination guards;
/// `r = -1.0` markers; the `xd*xd + yd*yd + zd*zd < 1.0` unit-sphere test;
/// `bitSetIndex = x - xStart + (y - yStart) * sizeXZ + (z - zStart) * sizeXZ *
/// sizeY`; the `BitSet` dedup; `!level.isOutsideBuildHeight(y)`; and
/// `placed > 0` as the verdict. `xMin..=xMax` etc. mirror the inclusive Java
/// `for` loops.
///
/// Each surviving cell's placement gate is [`can_place_ore`], and a passing
/// cell is written through the `WorldGenLevel` surface. Java's `BulkSectionAccess`
/// section cache is not ported (the null-section branch is dead code — see the
/// module doc), so the write phase reads the level and writes
/// `set_block(pos, state, 0)` for the section-raw `setBlockState(..., false)`
/// write.
///
/// `#[allow(clippy::neg_cmp_op_on_partial_ord)]`: the Java culling tests are
/// literally `!(data[...] <= 0.0)` / `!(r < 0.0)` — a NaN radius survives both
/// (Java `!(NaN <= 0.0)` is `!(false)`), which a rewrite to `> 0.0` / `>= 0.0`
/// would silently invert. Clippy flags the faithful forms, so they are
/// suppressed here.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn do_place<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    config: &OreConfiguration,
    x0: f64,
    x1: f64,
    z0: f64,
    z1: f64,
    y0: f64,
    y1: f64,
    x_start: i32,
    y_start: i32,
    z_start: i32,
    size_xz: i32,
    size_y: i32,
) -> bool {
    let mut placed = 0;
    // `BitSet(sizeXZ * sizeY * sizeXZ)` — index = `x - xStart + (y - yStart) *
    // sizeXZ + (z - zStart) * sizeXZ * sizeY`; `tested.get`/`tested.set`.
    let mut tested = vec![false; (size_xz * size_y * size_xz) as usize];
    let size = config.size;
    let mut data = vec![0.0f64; (size * 4) as usize];

    for i in 0..size {
        let step = i as f32 / size as f32;
        let xx = lerp(step as f64, x0, x1);
        let yy = lerp(step as f64, y0, y1);
        let zz = lerp(step as f64, z0, z1);
        let ss = random.next_double() * size as f64 / 16.0;
        // Java: `Mth.PI * step` is a FLOAT product (`Mth.PI` is `(float)
        // Math.PI`, `step` is float), widened to double only at the `Mth.sin`
        // call; `+ 1.0F` is a FLOAT add before the widening `* ss` to double.
        let r = ((sin((PI * step) as f64) + 1.0f32) as f64 * ss + 1.0) / 2.0;
        data[i as usize * 4] = xx;
        data[i as usize * 4 + 1] = yy;
        data[i as usize * 4 + 2] = zz;
        data[i as usize * 4 + 3] = r;
    }

    for i1 in 0..size - 1 {
        if !(data[i1 as usize * 4 + 3] <= 0.0) {
            for i2 in i1 + 1..size {
                if !(data[i2 as usize * 4 + 3] <= 0.0) {
                    let dx = data[i1 as usize * 4] - data[i2 as usize * 4];
                    let dy = data[i1 as usize * 4 + 1] - data[i2 as usize * 4 + 1];
                    let dz = data[i1 as usize * 4 + 2] - data[i2 as usize * 4 + 2];
                    let dr = data[i1 as usize * 4 + 3] - data[i2 as usize * 4 + 3];
                    if dr * dr > dx * dx + dy * dy + dz * dz {
                        if dr > 0.0 {
                            data[i2 as usize * 4 + 3] = -1.0;
                        } else {
                            data[i1 as usize * 4 + 3] = -1.0;
                        }
                    }
                }
            }
        }
    }

    // Java's `try (BulkSectionAccess ...)` is not ported: `getSection` returns
    // null only when the section index is out of range, which for the standard
    // section-aligned world height is exactly `isOutsideBuildHeight(y)` — a
    // condition the unit-sphere guard above already excludes — so the null
    // branch is dead code and the write phase reads/writes through the
    // `WorldGenLevel` surface instead (see the module doc). The level read
    // answers the section cache's semantics: AIR for an empty/absent section,
    // reflecting writes made earlier in the same blob.
    for i in 0..size {
        let r = data[i as usize * 4 + 3];
        if !(r < 0.0) {
            let xx = data[i as usize * 4];
            let yy = data[i as usize * 4 + 1];
            let zz = data[i as usize * 4 + 2];
            // `Mth.floor(double)` — both operands are double, so this is the
            // f64 floor overload (NOT the f32 `floor`): `floor_d(xx - r)`.
            let x_min = floor_d(xx - r).max(x_start);
            let y_min = floor_d(yy - r).max(y_start);
            let z_min = floor_d(zz - r).max(z_start);
            let x_max = floor_d(xx + r).max(x_min);
            let y_max = floor_d(yy + r).max(y_min);
            let z_max = floor_d(zz + r).max(z_min);

            for x in x_min..=x_max {
                let xd = (x as f64 + 0.5 - xx) / r;
                if xd * xd < 1.0 {
                    for y in y_min..=y_max {
                        let yd = (y as f64 + 0.5 - yy) / r;
                        if xd * xd + yd * yd < 1.0 {
                            for z in z_min..=z_max {
                                let zd = (z as f64 + 0.5 - zz) / r;
                                if xd * xd + yd * yd + zd * zd < 1.0
                                    && !level.is_outside_build_height(y)
                                {
                                    let bit_set_index = x - x_start
                                        + (y - y_start) * size_xz
                                        + (z - z_start) * size_xz * size_y;
                                    if !tested[bit_set_index as usize] {
                                        tested[bit_set_index as usize] = true;
                                        let ore_pos = BlockPos::new(x, y, z);
                                        if level.ensure_can_write(&ore_pos) {
                                            let block_state = level.get_block_state(&ore_pos);
                                            // Java passes `sectionGetter::getBlockState`
                                            // as `canPlaceOre`'s blockGetter — the section
                                            // cache, which returns AIR for a null section
                                            // and reflects blocks written earlier in the
                                            // same bulk session. The level read answers
                                            // both (the region reads its chunks, which the
                                            // writes have already mutated). The getter is
                                            // inlined so its borrow of `level` ends when
                                            // `can_place_ore` returns, freeing `level` for
                                            // the write.
                                            for target_state in &config.target_states {
                                                if can_place_ore(
                                                    &block_state,
                                                    |pos: &BlockPos| level.get_block_state(pos),
                                                    random,
                                                    config,
                                                    target_state,
                                                    &ore_pos,
                                                ) {
                                                    level.set_block(
                                                        &ore_pos,
                                                        target_state.state,
                                                        0,
                                                    );
                                                    placed += 1;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    placed > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_util::random::LegacyRandomSource;

    #[test]
    fn should_skip_air_check_zero_discard_always_skips() {
        // `discardChanceOnAirExposure <= 0.0F` short-circuits true — no RNG draw.
        let mut random = LegacyRandomSource::new(1);
        assert!(should_skip_air_check(&mut random, 0.0));
        assert!(should_skip_air_check(&mut random, -1.0));
    }

    #[test]
    fn should_skip_air_check_full_discard_never_skips() {
        // `>= 1.0F` — the second branch is skipped entirely, so always false.
        let mut random = LegacyRandomSource::new(1);
        assert!(!should_skip_air_check(&mut random, 1.0));
        assert!(!should_skip_air_check(&mut random, 2.0));
    }

    #[test]
    fn should_skip_air_check_nan_discard_consumes_one_draw() {
        // `discardChanceOnAirExposure = NaN`: the first conjunct is false
        // (`NaN <= 0.0`), and `!(NaN >= 1.0)` is TRUE in IEEE-754 (the rewrite
        // `NaN < 1.0` would be false), so the body rolls `random.nextFloat() >=
        // NaN` — which is always false for a finite draw — and consumes exactly
        // one RNG draw. Pin that consumption: the value drawn after the check
        // must be a fresh source's SECOND draw (one past the consumed one).
        for seed in [1i64, 7, 42, 12345, -2] {
            let mut expected_source = LegacyRandomSource::new(seed);
            expected_source.next_float();
            let expected_second = expected_source.next_float();

            let mut checked_source = LegacyRandomSource::new(seed);
            assert!(!should_skip_air_check(&mut checked_source, f32::NAN));
            assert_eq!(
                checked_source.next_float(),
                expected_second,
                "seed {seed}: NaN must consume exactly one draw"
            );
        }
    }

    #[test]
    fn should_skip_air_check_mid_discard_draws_next_float() {
        // For `discardChanceOnAirExposure = 0.5` the body is exactly
        // `random.nextFloat() >= 0.5`: the check consumes one draw, so an
        // identically-seeded source must produce the same bit as the check.
        // Pin a few seeds to make the draw path (and both outcomes) load-bearing.
        for seed in [1i64, 7, 42, 12345, -2] {
            let mut expected_source = LegacyRandomSource::new(seed);
            let expected = expected_source.next_float() >= 0.5;

            let mut checked_source = LegacyRandomSource::new(seed);
            assert_eq!(
                should_skip_air_check(&mut checked_source, 0.5),
                expected,
                "seed {seed}"
            );
        }
    }

    /// `place`'s height-probe short-circuit: when no `(x, z)` probe's
    /// `OCEAN_FLOOR_WG` height clears `y_start`, the blob is not placed — the
    /// function consumes exactly the axis draws (`nextFloat` for `dir`, two
    /// `nextInt(3)` for `y0`/`y1`) and returns `false`, never reaching
    /// `do_place`. Pinned with a `RecordingRandom` and a `TestLevel` whose
    /// column height is below `y_start` at every probe.
    #[test]
    fn place_returns_false_when_no_probe_clears_y_start() {
        use crate::levelgen::feature::test_support::{
            RecordingRandom, RngCall, TestGenerator, TestLevel, access,
        };
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        // size 9: `maxRadius = ceil((9/16*2+1)/2) = 2`, `yStart = 64 - 2 - 2
        // = 60`; a column height of 0 clears nothing, so every probe fails.
        let config = OreConfiguration::new_without_discard_chance(Vec::new(), 9);
        let mut random = RecordingRandom::new(1);
        let result = ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(!result, "no probe clears y_start -> not placed");
        assert_eq!(
            random.calls,
            vec![RngCall::Float, RngCall::IntBound(3), RngCall::IntBound(3),],
            "place consumes the axis draws (dir, y0, y1) and nothing more"
        );
    }

    /// `do_place`'s data-array construction runs above any seam: the
    /// `ss = random.nextDouble() * size / 16.0` draw happens once per sphere
    /// (`size` draws total) before the elimination/BitSet walk. With a column
    /// height that clears the probe, `place` reaches `do_place` and consumes
    /// exactly the axis draws plus `size` `next_double` draws, and the whole
    /// elimination/write walk completes without a seam panic. With an empty
    /// `target_states` list no `can_place_ore` test is ever run and nothing is
    /// written, so the verdict is `false` regardless of geometry.
    #[test]
    fn do_place_consumes_one_ss_draw_per_sphere_and_completes() {
        use crate::levelgen::feature::test_support::{
            RecordingRandom, RngCall, TestGenerator, TestLevel, access,
        };
        let mut level = TestLevel::over(access());
        // Column height 64 >= y_start 60 (size 9 at origin (0, 64, 0)), so the
        // first probe clears and `do_place` runs.
        level.height = 64;
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        let config = OreConfiguration::new_without_discard_chance(Vec::new(), 9);
        let mut random = RecordingRandom::new(1);
        let result = ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        let mut expected = vec![RngCall::Float, RngCall::IntBound(3), RngCall::IntBound(3)];
        for _ in 0..config.size {
            expected.push(RngCall::Double);
        }
        assert_eq!(
            random.calls, expected,
            "place + do_place consume the axis draws then one ss per sphere"
        );
        assert!(!result, "no target states -> nothing written -> not placed");
        assert!(
            level.writes.is_empty(),
            "empty target_states writes nothing even with a clearing column"
        );
    }

    /// Non-vacuous drive of the full ORE write path: with an always-true target
    /// and discard chance 0, every in-build-height sphere cell passes
    /// `can_place_ore` (the `should_skip_air_check` short-circuit skips the
    /// air-exposure roll and the six neighbor reads), so `do_place` writes the
    /// target state with flags 0 (`section.setBlockState(..., false)` — no
    /// `Block.UPDATE_*` flag) and returns `true` (`placed > 0`).
    #[test]
    fn do_place_writes_with_flags_zero_when_target_matches() {
        use crate::level::height_accessor::LevelHeightAccessor;
        use crate::levelgen::feature::configurations::TargetBlockState;
        use crate::levelgen::feature::test_support::{
            RecordingRandom, TestGenerator, TestLevel, access,
        };
        use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
        use rivet_registry::generated::blocks::BlockId;
        use std::sync::Arc;

        let mut level = TestLevel::over(access());
        level.height = 64; // clears y_start 60 (size 9 at origin (0, 64, 0)).
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        let ore_state = BlockState::of(BlockId(1));
        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(Arc::new(AlwaysTrueTest), ore_state)],
            9,
        );
        let mut random = RecordingRandom::new(1);
        let result = ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(
            result,
            "an always-true target writes at least one sphere cell -> placed"
        );
        assert!(
            !level.writes.is_empty(),
            "the blob's in-build-height sphere cells are written"
        );
        for (i, (pos, state)) in level.writes.iter().enumerate() {
            assert_eq!(*state, ore_state, "write carries the target state");
            assert_eq!(
                level.writes_flags[i], 0,
                "OreFeature writes with flags 0 (section.setBlockState(..., false))"
            );
            assert!(
                !level.is_outside_build_height(pos.get_y()),
                "written cells pass the Java isOutsideBuildHeight guard"
            );
        }
    }

    /// Hostile seam: `WorldGenRegion.setBlock` can refuse a write (chunk not
    /// writable), and Java's `doPlace` then skips the cell (`placed` unchanged).
    /// `ensure_can_write = false` must suppress every write, leave `placed ==
    /// 0`, and flip the verdict to `false` — while the geometry and RNG walk
    /// still complete.
    #[test]
    fn do_place_writes_nothing_when_ensure_can_write_is_false() {
        use crate::levelgen::feature::configurations::TargetBlockState;
        use crate::levelgen::feature::test_support::{
            RecordingRandom, TestGenerator, TestLevel, access,
        };
        use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
        use rivet_registry::generated::blocks::BlockId;
        use std::sync::Arc;

        let mut level = TestLevel::over(access());
        level.height = 64;
        level.can_write = false; // Java's setBlock returns false -> nothing placed.
        let origin = BlockPos::new(0, 64, 0);
        let generator = TestGenerator;
        let ore_state = BlockState::of(BlockId(1));
        let config = OreConfiguration::new_without_discard_chance(
            vec![TargetBlockState::new(Arc::new(AlwaysTrueTest), ore_state)],
            9,
        );
        let mut random = RecordingRandom::new(1);
        let result = ORE.place(&mut FeaturePlaceContext::new(
            None,
            &mut level,
            &generator,
            &mut random,
            &origin,
            &config,
        ));
        assert!(!result, "no write succeeded -> placed == 0 -> false");
        assert!(
            level.writes.is_empty(),
            "ensure_can_write == false suppresses every write"
        );
    }
}

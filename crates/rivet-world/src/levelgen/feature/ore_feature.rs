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
//! target state via the `BulkSectionAccess` section cache.
//!
//! The port follows the exact Java structure: [`place`] computes the axis
//! endpoints and probes the height; [`do_place`] runs the data-array /
//! elimination / per-sphere traversal faithfully. The two *write-decision*
//! seams defer:
//! - [`can_place_ore`] DEFERS (RivetTodo #399): its first conjunct evaluates
//!   `targetState.target().test(state, random)` on the erased `RuleTest`
//!   carrier, and the templatesystem unit's `ErasedRuleTest` deliberately has
//!   no object-safe `test` (`RandomSource` is `Sized`, so `RuleTest::test` is
//!   not dispatchable through `dyn`); the erased-evaluation surface is owned by
//!   that unit and is not ported anywhere yet. Both `place` bodies route every
//!   write through it, so neither can place until that lands.
//! - The `BulkSectionAccess`/`LevelChunkSection` section read/write routes
//!   through the [`OreSectionAccess`] typed seam (RivetTodo #232: the chunk
//!   section surface no `WorldGenLevel` provides yet).
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
//! - `SectionPos.sectionRelative` is `x & 15`.
//! - `Math.max(Mth.floor(...), xStart)` mirrors Java's `Math.max(int, int)`.
//! - The `BitSet` index is `x - xStart + (y - yStart) * sizeXZ + (z - zStart) *
//!   sizeXZ * sizeY`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::OreConfiguration;
use crate::levelgen::feature::configurations::TargetBlockState;
use crate::levelgen::heightmap::Types;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
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
/// The second conjunct (air-exposure discard) is fully portable: it reads only
/// the RNG, the discard chance, and the six axis-neighbor states. The FIRST
/// conjunct DEFERS: `targetState.target` is an `Arc<dyn ErasedRuleTest>`, and
/// the erased carrier has no object-safe `test` (the templatesystem unit's
/// documented split; `RandomSource` is `Sized`). Evaluating it would require
/// the erased-evaluation dispatch that unit owns and has not ported — this
/// unit must not fabricate it. So `can_place_ore` fails explicitly rather than
/// fabricating a verdict (the same capability-unavailable seam the `#232`
/// world seams use), marked `RivetTodo(#399)`. Both `place` bodies route every
/// write through it, so neither can place until the erased evaluation lands.
///
/// The getter is `FnMut`, not `Fn`: `do_place` passes a section-backed getter
/// that re-enters the mutable `BulkSectionAccess` cache (Java's
/// `sectionGetter::getBlockState`), and re-entering a mutable access is a
/// mutation. A read-only getter still satisfies the looser bound.
pub fn can_place_ore<R: RandomSource>(
    _ore_pos_state: &BlockState,
    _block_getter: impl FnMut(&BlockPos) -> BlockState,
    _random: &mut R,
    _config: &OreConfiguration,
    _target_state: &TargetBlockState,
    _ore_pos: &BlockPos,
) -> bool {
    panic!(
        "OreFeature.canPlaceOre is not implemented (RivetTodo #399: the erased RuleTest has no object-safe test; the templatesystem unit owns the erased-evaluation dispatch)"
    )
}

/// The `BulkSectionAccess`-shaped section surface `do_place` writes through —
/// `getSection(BlockPos)` returning the mutable `LevelChunkSection` (Java
/// `null` when the column has no section, which skips the write).
///
/// STUB(mc.world.level.levelgen.feature.ore-runtime): the section-level read
/// seam the blob writes route through. The `#232` chunk section
/// infrastructure is not reachable from a `WorldGenLevel` yet; concrete worlds
/// and test doubles override it when they land. The write path is doubly
/// deferred, but the ordering matters: `do_place` enters this seam (the
/// `get_or_insert_with` `#232` panic) the moment a write candidate is reached
/// — BEFORE [`can_place_ore`]'s `#399` gate is consulted. So a write candidate
/// observably enters the `#232` seam first, and neither a write decision nor a
/// write is observable until both land.
pub trait OreSectionAccess {
    /// `BulkSectionAccess.getSection(BlockPos)` — the mutable section at the
    /// position, or `None` (Java `null`) when the section is absent.
    fn get_section(&mut self, pos: &BlockPos) -> Option<OreSection>;
}

/// `LevelChunkSection` restricted to the ore write surface — the
/// `getBlockState`/`setBlockState` pair `do_place` uses (the same shape
/// `ImposterProtoChunk`'s `LevelChunkSection<T, B>` provides).
pub struct OreSection {
    /// `getBlockState(int, int, int)` — the section-relative read.
    pub get_block_state: Box<dyn Fn(i32, i32, i32) -> BlockState>,
    /// `setBlockState(int, int, int, BlockState)` — the section-relative write
    /// (Java's `setBlockState(..., false)` — no update flag).
    pub set_block_state: Box<dyn FnMut(i32, i32, i32, BlockState)>,
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
/// on `WorldGenRegion` (the primed chunk heightmap, `minY + 1` fallback), so
/// the height probe completes; the failure seam is the write phase.
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
/// The write path defers: each cell's placement gate is [`can_place_ore`]
/// (#399), and the section read/write routes through the [`OreSectionAccess`]
/// seam (a `LevelChunkSection`-shaped surface no production world provides
/// yet). The geometry and RNG draw order above that are faithful and
/// test-pinnable.
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

    // `try (BulkSectionAccess sectionGetter = new BulkSectionAccess(level))` —
    // the section cache is opened once for the whole write phase. The seam
    // panics on the first write candidate reached (RivetTodo #232) — before
    // `can_place_ore` (#399) is consulted — so a write candidate observably
    // enters the `#232` seam first; neither a write decision nor a write is
    // observable until both land.
    let mut section_access: Option<Box<dyn OreSectionAccess>> = None;
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
                                            let section = {
                                                let access = section_access
                                                    .get_or_insert_with(|| {
                                                        panic!("OreFeature.doPlace needs a BulkSectionAccess (RivetTodo #232): the WorldGenLevel does not provide section access")
                                                    });
                                                access.get_section(&ore_pos)
                                            };
                                            if let Some(mut section) = section {
                                                let sx = section_relative(x);
                                                let sy = section_relative(y);
                                                let sz = section_relative(z);
                                                let block_state =
                                                    (section.get_block_state)(sx, sy, sz);

                                                // Java passes
                                                // `sectionGetter::getBlockState`
                                                // (`BulkSectionAccess`) as
                                                // `canPlaceOre`'s blockGetter —
                                                // the section cache, which
                                                // returns AIR for a null section
                                                // and reflects blocks written
                                                // earlier in the same bulk
                                                // session. The port mirrors it
                                                // with a section-backed getter
                                                // re-entering `OreSectionAccess`
                                                // (the `#232` seam), never a
                                                // world-level read.
                                                let mut block_getter = |pos: &BlockPos| {
                                                    let access = section_access.as_mut().expect(
                                                        "section access is open (RivetTodo #232)",
                                                    );
                                                    match access.get_section(pos) {
                                                        Some(section) => {
                                                            let nx = section_relative(pos.get_x());
                                                            let ny = section_relative(pos.get_y());
                                                            let nz = section_relative(pos.get_z());
                                                            (section.get_block_state)(nx, ny, nz)
                                                        }
                                                        // `BulkSectionAccess.getBlockState`
                                                        // returns
                                                        // `Blocks.AIR.defaultBlockState()`
                                                        // for a null section.
                                                        None => BlockState::of(BlockId(0)),
                                                    }
                                                };

                                                for target_state in &config.target_states {
                                                    if can_place_ore(
                                                        &block_state,
                                                        &mut block_getter,
                                                        random,
                                                        config,
                                                        target_state,
                                                        &ore_pos,
                                                    ) {
                                                        (section.set_block_state)(
                                                            sx,
                                                            sy,
                                                            sz,
                                                            target_state.state,
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
    }

    placed > 0
}

/// `SectionPos.sectionRelative(int)` — `coord & 15` (the section 16-block
/// mask). `SectionPos` itself is not ported in this unit; the mask is inlined
/// at the use site.
const fn section_relative(coord: i32) -> i32 {
    coord & 15
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

    /// `sectionRelative` is the 16-block mask `coord & 15`.
    #[test]
    fn section_relative_masks_low_4_bits() {
        assert_eq!(section_relative(15), 15);
        assert_eq!(section_relative(16), 0);
        assert_eq!(section_relative(-1), 15);
        assert_eq!(section_relative(-16), 0);
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
    /// exactly the axis draws plus `size` `next_double` draws; the walk then
    /// either reaches a verdict (no candidate survives to the write seam) or
    /// fails at the `#232` `BulkSectionAccess` seam — never another panic.
    #[test]
    fn do_place_consumes_one_ss_draw_per_sphere_before_any_seam() {
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
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ORE.place(&mut FeaturePlaceContext::new(
                None,
                &mut level,
                &generator,
                &mut random,
                &origin,
                &config,
            ))
        }));
        let mut expected = vec![RngCall::Float, RngCall::IntBound(3), RngCall::IntBound(3)];
        for _ in 0..config.size {
            expected.push(RngCall::Double);
        }
        assert_eq!(
            random.calls, expected,
            "place + do_place consume the axis draws then one ss per sphere"
        );
        match result {
            Ok(_) => {}
            Err(payload) => {
                let text = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("<non-string panic>");
                assert!(
                    text.contains("RivetTodo #232"),
                    "the only panic reachable is the #232 section seam, got {text:?}"
                );
            }
        }
    }
}

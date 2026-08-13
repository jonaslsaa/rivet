//! Port of `net.minecraft.world.level.levelgen.blending.Blender` (class,
//! 26.2) — the old-world blending seam (issue #177).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! levelgen/blending/Blender.java`.
//!
//! This slice ports the full Blender value surface the noisegen shares
//! (`blendOffsetAndFactor`/`blendDensity`/`isEmpty`/`empty`), now backed by the
//! ported [`BlendingData`] (see `super::blending_data`):
//!
//! - [`Blender::empty`] / [`Blender::is_empty`] — the `EMPTY` singleton is the
//!   empty-map `Blender`; the base-class algorithms over empty maps degenerate
//!   to exactly the `EMPTY` overrides (constant `(1.0, 0.0)`, identity
//!   `blendDensity`, identity `getBiomeResolver`).
//! - [`Blender::blend_offset_and_factor`] — the non-empty fixed-height lookup
//!   and inverse-distance-4 weighted average with the smoothstep alpha.
//! - [`Blender::blend_density`] — the non-empty fixed-density lookup and the
//!   distance-weighted `lerp` blend.
//! - [`Blender::blend_biome`] + [`Blender::get_biome_resolver`] — the
//!   non-empty closest-old-chunk-biome blend with the `SHIFT_NOISE` alpha gate
//!   (the `SHIFT_NOISE` `NormalNoise` static mirrors Blender.java line 53).
//! - [`Blender::height_to_offset`] — the fixed height-to-blend-offset formula.
//!
//! The chunk/region-reading half defers as `RivetTodo(#177)` owned by the
//! blending unit (it needs `WorldGenRegion`, which is not ported yet, plus the
//! `ChunkAccess` block/heightmap/status surfaces):
//!
//! - `of(WorldGenRegion)` — the `Long2ObjectOpenHashMap` square-distance scan
//!   over `BlendingData.getOrUpdateBlendingData` (needs `region.getCenter`/
//!   `isOldChunkAround`/`getChunk`). Until it lands, the only constructible
//!   `Blender` is the empty one; a `pub(crate)` constructor mirrors the private
//!   Java ctor for the crate's tests.
//! - `generateBorderTicks`/`generateBorderTick` and
//!   `addAroundOldChunksCarvingMaskFilter`/`makeOldChunkDistanceGetter`/
//!   `makeOffsetOldChunkDistanceGetter`/`distanceToCube` (`ChunkAccess`/
//!   `ProtoChunk`/`WorldGenLevel` reads). The range constants those deferrals
//!   consume are still declared (`HEIGHT_BLENDING_RANGE_CHUNKS`,
//!   `DENSITY_BLENDING_RANGE_CHUNKS`, `OLD_CHUNK_XZ_RADIUS`).

use std::collections::BTreeMap;

use rivet_registry::Holder;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{ChunkPos, QuartPos};
use rivet_util::mth;
use rivet_util::random::XoroshiroRandomSource;
use std::sync::LazyLock;

use crate::biome::biome_resolver::BiomeResolver;
use crate::biome::climate::Sampler;
use crate::data::worldgen::noise_data::DEFAULT_SHIFT;
use crate::levelgen::blending::blending_data::BlendingData;
use crate::levelgen::noise::density_function::FunctionContext;
use crate::levelgen::synth::normal_noise::NormalNoise;

/// `HEIGHT_BLENDING_RANGE_CELLS` — `QuartPos.fromSection(7) - 1` (Blender.java
/// line 54): `(7 << 2) - 1`.
const HEIGHT_BLENDING_RANGE_CELLS: i32 = 27;

/// `HEIGHT_BLENDING_RANGE_CHUNKS` — `QuartPos.toSection(HEIGHT_BLENDING_RANGE_CELLS + 3)`
/// (Blender.java line 55): `(27 + 3) >> 2`. Used only by the deferred
/// `of(WorldGenRegion)` (RivetTodo #177).
#[allow(dead_code)]
const HEIGHT_BLENDING_RANGE_CHUNKS: i32 = 7;

/// `DENSITY_BLENDING_RANGE_CELLS` (Blender.java line 56).
const DENSITY_BLENDING_RANGE_CELLS: i32 = 2;

/// `DENSITY_BLENDING_RANGE_CHUNKS` — `QuartPos.toSection(5)` (Blender.java line
/// 57): `5 >> 2`. Used only by the deferred `of(WorldGenRegion)` (RivetTodo
/// #177).
#[allow(dead_code)]
const DENSITY_BLENDING_RANGE_CHUNKS: i32 = 1;

/// `OLD_CHUNK_XZ_RADIUS` (Blender.java line 58). Used only by the deferred
/// `makeOffsetOldChunkDistanceGetter` (RivetTodo #177).
#[allow(dead_code)]
const OLD_CHUNK_XZ_RADIUS: f64 = 8.0;

/// `SHIFT_NOISE` (Blender.java line 53) — the `NormalNoise.create(new
/// XoroshiroRandomSource(42L), NoiseData.DEFAULT_SHIFT)` static the
/// `blendBiome` alpha gate and the deferred carving-mask filter sample.
///
/// Java initializes it once at class load; the port mirrors that with a
/// `LazyLock` static (the same pattern as the `noise_data` bootstraps).
static SHIFT_NOISE: LazyLock<NormalNoise> = LazyLock::new(|| {
    NormalNoise::create(
        &mut XoroshiroRandomSource::new(42),
        (*DEFAULT_SHIFT).clone(),
    )
});

/// `Blender.BlendingOutput(double alpha, double blendingOffset)` record — the
/// `blendOffsetAndFactor` result.
///
/// Equality mirrors the Java record's generated `equals`: each `double`
/// component compares via `Double.compare` — every NaN payload canonicalizes
/// to one value, and `-0.0` is distinct from `0.0` (the derived IEEE `==`
/// treats NaN as unequal to itself and `-0.0 == 0.0`).
#[derive(Debug, Clone, Copy)]
pub struct BlendingOutput {
    alpha: f64,
    blending_offset: f64,
}

impl PartialEq for BlendingOutput {
    fn eq(&self, other: &Self) -> bool {
        // `Double.doubleToLongBits` canonical-bit comparison: every NaN maps
        // to one canonical pattern, and signed zero keeps its sign bit.
        fn canonical_bits(value: f64) -> u64 {
            if value.is_nan() {
                f64::NAN.to_bits()
            } else {
                value.to_bits()
            }
        }
        canonical_bits(self.alpha) == canonical_bits(other.alpha)
            && canonical_bits(self.blending_offset) == canonical_bits(other.blending_offset)
    }
}

impl Eq for BlendingOutput {}

impl BlendingOutput {
    /// The record constructor.
    pub fn new(alpha: f64, blending_offset: f64) -> Self {
        BlendingOutput {
            alpha,
            blending_offset,
        }
    }

    /// `alpha()` (record accessor).
    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    /// `blendingOffset()` (record accessor).
    pub fn blending_offset(&self) -> f64 {
        self.blending_offset
    }
}

/// `net.minecraft.world.level.levelgen.blender.Blender`.
///
/// The two `Long2ObjectOpenHashMap<BlendingData>` fields (Blender.java lines
/// 59-60) — height/biome data for the wide range, density data for the narrow
/// range. The private ctor is exposed `pub(crate)` so the crate's tests can
/// build a non-empty Blender; `of(WorldGenRegion)` (the production builder)
/// defers (RivetTodo #177).
///
/// The port backs the maps with `BTreeMap` rather than the default-randomized
/// `HashMap`: the weighted-average loops (`blendOffsetAndFactor`/
/// `blendDensity` and the deferred `blendBiome`) accumulate f64 sums in map
/// iteration order, and f64 addition is order-sensitive, so the iteration must
/// be deterministic for run-to-run reproducibility. Sorted chunk keys give a
/// stable, well-defined order.
///
/// Paper iterates the fastutil `Long2ObjectOpenHashMap` in probe-slot order
/// (a fixed `HashCommon.mix`-derived layout for the same insertions), which is
/// NOT ascending key order, so byte-for-byte parity of these f64 sums with
/// Paper may require reproducing that slot order once `of(WorldGenRegion)`
/// builds a multi-chunk Blender (RivetTodo #177). Until then the only
/// constructible production Blender is the empty one and the crate's tests use
/// single-entry maps, so no order-sensitive input is reachable; the
/// slot-order parity must be verified when `of()` lands.
#[derive(Debug, Clone)]
pub struct Blender {
    height_and_biome_blending_data: BTreeMap<i64, BlendingData>,
    density_blending_data: BTreeMap<i64, BlendingData>,
}

impl Blender {
    /// The private constructor (Blender.java lines 102-105). Only the crate's
    /// tests reach it today — `of(WorldGenRegion)` is deferred (RivetTodo #177).
    #[allow(dead_code)]
    pub(crate) fn new(
        height_and_biome_blending_data: BTreeMap<i64, BlendingData>,
        density_blending_data: BTreeMap<i64, BlendingData>,
    ) -> Blender {
        Blender {
            height_and_biome_blending_data,
            density_blending_data,
        }
    }

    /// `Blender.empty()` — the shared `EMPTY` singleton (Blender.java line 62).
    ///
    /// The base-class algorithms run over the empty maps, which is exactly what
    /// the Java `EMPTY` anonymous subclass overrides hard-code: `(1.0, 0.0)`,
    /// identity density, identity biome resolver.
    pub fn empty() -> Blender {
        Blender {
            height_and_biome_blending_data: BTreeMap::new(),
            density_blending_data: BTreeMap::new(),
        }
    }

    /// `isEmpty()` — `heightAndBiomeBlendingData.isEmpty() &&
    /// densityBlendingData.isEmpty()` (Blender.java lines 107-109).
    pub fn is_empty(&self) -> bool {
        self.height_and_biome_blending_data.is_empty() && self.density_blending_data.is_empty()
    }

    /// `blendOffsetAndFactor(int blockX, int blockZ)` (Blender.java lines
    /// 111-147): the fixed height under the queried cell wins immediately
    /// (`(0.0, heightToOffset)`), otherwise the inverse-distance-4 weighted
    /// average of the neighbouring old-chunk heights with a smoothstep alpha.
    pub fn blend_offset_and_factor(&self, block_x: i32, block_z: i32) -> BlendingOutput {
        let cell_x = QuartPos::from_block(block_x);
        let cell_z = QuartPos::from_block(block_z);
        let fixed_height =
            self.get_blending_data_value(cell_x, 0, cell_z, BlendingData::get_height);
        if fixed_height != f64::MAX {
            return BlendingOutput::new(0.0, Self::height_to_offset(fixed_height));
        }

        let mut total_weight = 0.0;
        let mut weighted_heights = 0.0;
        let mut closest_distance = f64::INFINITY;
        // `BTreeMap` yields the chunk keys in ascending order, so the f64
        // accumulation is deterministic run-to-run. Paper iterates the fastutil
        // `Long2ObjectOpenHashMap` in probe-slot order (a fixed
        // `HashCommon.mix`-derived layout, NOT ascending), so byte-for-byte
        // parity of these f64 sums requires reproducing that slot order once
        // `of(WorldGenRegion)` builds a multi-chunk Blender —
        // `RivetTodo(#177)`; see the struct doc.
        for (chunk_pos, blending_data) in &self.height_and_biome_blending_data {
            blending_data.iterate_heights(
                QuartPos::from_section(ChunkPos::get_x(*chunk_pos)),
                QuartPos::from_section(ChunkPos::get_z(*chunk_pos)),
                |test_cell_x, test_cell_z, height| {
                    // Java resolves `Mth.length(int, int)` to the FLOAT
                    // overload (`length(float, float)` — Mth.java line 710; the
                    // most specific for int args, ahead of
                    // `length(double, double)`), which computes
                    // `(float)Math.sqrt(lengthSquared(x, y))` with the floats
                    // widened to double for the sqrt. `mth::length_f32` mirrors
                    // that bit-for-bit; the `as f64` is the Java float widened
                    // for the range gate, `closest_distance`, and the double
                    // weight math.
                    let distance = mth::length_f32(
                        cell_x.wrapping_sub(test_cell_x) as f32,
                        cell_z.wrapping_sub(test_cell_z) as f32,
                    ) as f64;
                    // Java's exact `!(distance > HEIGHT_BLENDING_RANGE_CELLS)`
                    // (Blender.java line 127): `<=` would flip the NaN distance
                    // out of the range gate (`!(NaN > x)` is true; `NaN <= x`
                    // is false).
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    if !(distance > HEIGHT_BLENDING_RANGE_CELLS as f64) {
                        if distance < closest_distance {
                            closest_distance = distance;
                        }
                        let weight = 1.0 / (distance * distance * distance * distance);
                        weighted_heights += height * weight;
                        total_weight += weight;
                    }
                },
            );
        }
        if closest_distance == f64::INFINITY {
            return BlendingOutput::new(1.0, 0.0);
        }

        let average_height = weighted_heights / total_weight;
        // Java widens `(int)(HEIGHT_BLENDING_RANGE_CELLS + 1)` to double.
        let alpha = mth::clamp_f64(
            closest_distance / (HEIGHT_BLENDING_RANGE_CELLS + 1) as f64,
            0.0,
            1.0,
        );
        let alpha = 3.0 * alpha * alpha - 2.0 * alpha * alpha * alpha;
        BlendingOutput::new(alpha, Self::height_to_offset(average_height))
    }

    /// `heightToOffset(double height)` (Blender.java lines 149-154) — the fixed
    /// height-to-blend-offset formula. `dimensionFactor` is the literal Java
    /// `1.0` local (multiplied but a no-op); kept for fidelity.
    pub(crate) fn height_to_offset(height: f64) -> f64 {
        let dimension_factor = 1.0;
        let target_y = height + 0.5;
        let target_y_mod = mth::positive_modulo_f64(target_y, 8.0);
        dimension_factor
            * (32.0 * (target_y - 128.0) - 3.0 * (target_y - 120.0) * target_y_mod
                + 3.0 * target_y_mod * target_y_mod)
            / (128.0 * (32.0 - 3.0 * target_y_mod))
    }

    /// `blendDensity(FunctionContext, double)` (Blender.java lines 156-196): a
    /// fixed old-chunk density under the queried cell wins immediately, else
    /// the inverse-distance-4 weighted average of the neighbouring density
    /// cells `lerp`ed toward the generated noise value.
    pub fn blend_density(&self, context: &dyn FunctionContext, noise_value: f64) -> f64 {
        let cell_x = QuartPos::from_block(context.block_x());
        let cell_y = context.block_y() / 8;
        let cell_z = QuartPos::from_block(context.block_z());
        let fixed_density =
            self.get_blending_data_value(cell_x, cell_y, cell_z, BlendingData::get_density);
        if fixed_density != f64::MAX {
            return fixed_density;
        }

        let mut total_weight = 0.0;
        let mut weighted_densities = 0.0;
        let mut closest_distance = f64::INFINITY;
        // `BTreeMap` yields the chunk keys in ascending order, so the f64
        // accumulation is deterministic; Paper iterates the fastutil map in
        // probe-slot order, so the slot-order parity deferred in
        // `blend_offset_and_factor` (`RivetTodo(#177)`) applies here too.
        for (chunk_pos, blending_data) in &self.density_blending_data {
            blending_data.iterate_densities(
                QuartPos::from_section(ChunkPos::get_x(*chunk_pos)),
                QuartPos::from_section(ChunkPos::get_z(*chunk_pos)),
                cell_y.wrapping_sub(1),
                cell_y.wrapping_add(1),
                |test_cell_x, test_cell_y, test_cell_z, density| {
                    // Java wraps the int difference and the `* 2` multiply
                    // (`(cellY - testCellY) * 2`). `Mth.length(int, int, int)`
                    // resolves to `length(double, double, double)` — there is
                    // no 3-arg float `length` overload (Mth.java has only
                    // `lengthSquared(float, float, float)`), so the all-f64
                    // `length_xyz` below is exact.
                    let distance = mth::length_xyz(
                        cell_x.wrapping_sub(test_cell_x) as f64,
                        (cell_y.wrapping_sub(test_cell_y)).wrapping_mul(2) as f64,
                        cell_z.wrapping_sub(test_cell_z) as f64,
                    );
                    // Java's exact `!(distance > 2.0)` (Blender.java line 177):
                    // `<=` would flip the NaN distance out of the range gate.
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    if !(distance > DENSITY_BLENDING_RANGE_CELLS as f64) {
                        if distance < closest_distance {
                            closest_distance = distance;
                        }
                        let weight = 1.0 / (distance * distance * distance * distance);
                        weighted_densities += density * weight;
                        total_weight += weight;
                    }
                },
            );
        }
        if closest_distance == f64::INFINITY {
            return noise_value;
        }

        let average_density = weighted_densities / total_weight;
        let alpha = mth::clamp_f64(closest_distance / 3.0, 0.0, 1.0);
        mth::lerp(alpha, average_density, noise_value)
    }

    /// `getBlendingDataValue(int cellX, int cellY, int cellZ, CellValueGetter)`
    /// (Blender.java lines 198-221) — the cell lookup with the corner/edge
    /// fallbacks to the neighbouring chunks when the cell sits on a chunk
    /// boundary.
    fn get_blending_data_value(
        &self,
        cell_x: i32,
        cell_y: i32,
        cell_z: i32,
        cell_value_getter: fn(&BlendingData, i32, i32, i32) -> f64,
    ) -> f64 {
        let chunk_x = QuartPos::to_section(cell_x);
        let chunk_z = QuartPos::to_section(cell_z);
        let min_x = (cell_x & 3) == 0;
        let min_z = (cell_z & 3) == 0;
        let mut value =
            self.cell_value_at_chunk(cell_value_getter, chunk_x, chunk_z, cell_x, cell_y, cell_z);
        if value == f64::MAX {
            if min_x && min_z {
                // Java wraps the `chunkX - 1` / `chunkZ - 1`.
                value = self.cell_value_at_chunk(
                    cell_value_getter,
                    chunk_x.wrapping_sub(1),
                    chunk_z.wrapping_sub(1),
                    cell_x,
                    cell_y,
                    cell_z,
                );
            }

            if value == f64::MAX {
                if min_x {
                    value = self.cell_value_at_chunk(
                        cell_value_getter,
                        chunk_x.wrapping_sub(1),
                        chunk_z,
                        cell_x,
                        cell_y,
                        cell_z,
                    );
                }

                if value == f64::MAX && min_z {
                    value = self.cell_value_at_chunk(
                        cell_value_getter,
                        chunk_x,
                        chunk_z.wrapping_sub(1),
                        cell_x,
                        cell_y,
                        cell_z,
                    );
                }
            }
        }

        value
    }

    /// `getBlendingDataValue(CellValueGetter, int chunkX, int chunkZ, int
    /// cellX, int cellY, int cellZ)` (Blender.java lines 223-230) — the
    /// height-and-biome-map cell read at chunk-relative cell coordinates.
    ///
    /// Rust has no overloading, so this second overload keeps the distinctive
    /// `chunk_x`/`chunk_z` parameters in the name.
    fn cell_value_at_chunk(
        &self,
        cell_value_getter: fn(&BlendingData, i32, i32, i32) -> f64,
        chunk_x: i32,
        chunk_z: i32,
        cell_x: i32,
        cell_y: i32,
        cell_z: i32,
    ) -> f64 {
        let blending_data = self
            .height_and_biome_blending_data
            .get(&ChunkPos::pack_coords(chunk_x, chunk_z));
        match blending_data {
            Some(blending_data) => cell_value_getter(
                blending_data,
                // Java wraps the int differences (`cellX - QuartPos.fromSection(chunkX)`)
                // before passing them to the cell getter.
                cell_x.wrapping_sub(QuartPos::from_section(chunk_x)),
                cell_y,
                cell_z.wrapping_sub(QuartPos::from_section(chunk_z)),
            ),
            None => f64::MAX,
        }
    }

    /// `getBiomeResolver(BiomeResolver)` (Blender.java lines 232-237) — the
    /// base-class override; the `EMPTY` anonymous subclass replaces it with the
    /// identity (Blender.java lines 49-51), so an empty `Blender` returns the
    /// resolver unchanged while a non-empty one reaches this wrapped form.
    ///
    /// The wrapper resolves a quart position to the `blendBiome` result when
    /// the blend finds one, else delegates to the wrapped resolver. A non-empty
    /// `Blender` is not production-constructible until `of(WorldGenRegion)`
    /// lands (`RivetTodo(#177)`), so today this is exercised by the unit's
    /// tests only.
    ///
    /// The wrapper holds `self` by reference and the boxed delegate by
    /// ownership, mirroring Java's closure capture of `this` and `biomeResolver`
    /// (the caller transfers ownership of their resolver into the wrapper). The
    /// returned trait object is bounded by the borrow of `self`, so no deep
    /// copy of the Blender is taken.
    pub fn get_biome_resolver<'a>(
        &'a self,
        resolver: Box<dyn BiomeResolver>,
    ) -> Box<dyn BiomeResolver + 'a> {
        if self.is_empty() {
            // `EMPTY`'s identity override (Blender.java lines 49-51) returns the
            // resolver unchanged — no wrapper, no allocation.
            resolver
        } else {
            Box::new(BlendedBiomeResolver {
                blender: self,
                delegate: resolver,
            })
        }
    }

    /// `blendBiome(int quartX, int quartY, int quartZ)` (Blender.java lines
    /// 239-263) — the closest old-chunk biome within `HEIGHT_BLENDING_RANGE_CELLS`,
    /// gated by the `SHIFT_NOISE`-shifted alpha: `alpha > 0.5` delegates back
    /// to the wrapped resolver (`None` here).
    fn blend_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<Holder<BiomeId>> {
        let mut closest_distance = f64::INFINITY;
        let mut closest_biome: Option<Holder<BiomeId>> = None;
        for (chunk_pos, blending_data) in &self.height_and_biome_blending_data {
            blending_data.iterate_biomes(
                QuartPos::from_section(ChunkPos::get_x(*chunk_pos)),
                quart_y,
                QuartPos::from_section(ChunkPos::get_z(*chunk_pos)),
                |test_cell_x, test_cell_z, biome| {
                    // Java wraps the int difference (`quartX - testCellX`).
                    // `Mth.length(int, int)` resolves to the FLOAT overload
                    // (as in `blend_offset_and_factor`); `mth::length_f32`
                    // matches it bit-for-bit and the `as f64` is the Java
                    // float widened.
                    let distance = mth::length_f32(
                        quart_x.wrapping_sub(test_cell_x) as f32,
                        quart_z.wrapping_sub(test_cell_z) as f32,
                    ) as f64;
                    // Java's exact `!(distance > HEIGHT_BLENDING_RANGE_CELLS)`
                    // (Blender.java line 247): `<=` would flip the NaN distance
                    // out of the range gate. The inner `distance < closestDistance`
                    // is collapsed with `&&`; both short-circuit exactly like the
                    // nested Java `if`s.
                    #[allow(clippy::neg_cmp_op_on_partial_ord)]
                    if !(distance > HEIGHT_BLENDING_RANGE_CELLS as f64)
                        && distance < closest_distance
                    {
                        closest_biome = Some(biome);
                        closest_distance = distance;
                    }
                },
            );
        }
        if closest_distance == f64::INFINITY {
            return None;
        }

        let closest_biome = closest_biome.expect("closest_distance finite implies a biome");
        // `SHIFT_NOISE.getValue(quartX, 0.0, quartZ)` — the y input is the
        // literal Java `0.0` double constant.
        let shift_noise = SHIFT_NOISE.get_value(quart_x as f64, 0.0, quart_z as f64) * 12.0;
        let alpha = mth::clamp_f64(
            (closest_distance + shift_noise) / (HEIGHT_BLENDING_RANGE_CELLS + 1) as f64,
            0.0,
            1.0,
        );
        if alpha > 0.5 {
            None
        } else {
            Some(closest_biome)
        }
    }
}

/// The wrapped `BiomeResolver` `getBiomeResolver` returns (Blender.java lines
/// 232-237): blend the quart position through [`Blender::blend_biome`], and
/// only when the blend finds nothing delegate to the wrapped resolver. This is
/// the Java lambda as a named struct — a closure cannot implement the
/// object-safe `BiomeResolver` trait directly. `'a` is the borrow of the
/// `Blender` (Java's closure captures `this` by reference; the Blender is
/// immutable after construction, so a shared borrow is sound).
struct BlendedBiomeResolver<'a> {
    blender: &'a Blender,
    delegate: Box<dyn BiomeResolver>,
}

impl BiomeResolver for BlendedBiomeResolver<'_> {
    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId> {
        match self.blender.blend_biome(quart_x, quart_y, quart_z) {
            Some(biome) => biome,
            None => self
                .delegate
                .get_noise_biome(quart_x, quart_y, quart_z, sampler),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::climate::Climate;
    use crate::levelgen::blending::blending_data::{
        BlendingData, CELL_HORIZONTAL_MAX_INDEX_INSIDE, Packed,
    };
    use crate::levelgen::noise::density_function::SinglePointContext;
    use std::cmp::Ordering;

    /// A `BlendingData` for the overworld (`min_section = -4`, `max_section =
    /// 20`) with `heights[index]` set.
    fn data_with_height(index: usize, height: f64) -> BlendingData {
        let mut heights = vec![f64::MAX; 16];
        heights[index] = height;
        BlendingData::unpack(Some(Packed::new(-4, 20, Some(heights)))).unwrap()
    }

    /// `Blender.EMPTY.blendDensity(context, noiseValue) == noiseValue`
    /// (Blender.java line 44). The empty-map Blender's base-class algorithm
    /// degrades to the identity: no density cells within range.
    #[test]
    fn empty_blend_density_is_identity() {
        let blender = Blender::empty();
        let context = SinglePointContext::new(0, 0, 0);
        for noise_value in [0.0, 1.0, -1.0, 42.5, f64::MAX, f64::MIN] {
            assert_eq!(blender.blend_density(&context, noise_value), noise_value);
        }
        // NaN passes through unchanged (`assert_eq!` can't compare NaN).
        assert!(blender.blend_density(&context, f64::NAN).is_nan());
    }

    /// `Blender.EMPTY.blendOffsetAndFactor(blockX, blockZ) ==
    /// new Blender.BlendingOutput(1.0, 0.0)` (Blender.java line 40) — the
    /// exact empty constant, independent of the queried block coordinates.
    #[test]
    fn empty_blend_offset_and_factor_constant() {
        let blender = Blender::empty();
        for (block_x, block_z) in [(0, 0), (7, -13), (i32::MAX, i32::MIN)] {
            let output = blender.blend_offset_and_factor(block_x, block_z);
            assert_eq!(output.alpha(), 1.0);
            assert_eq!(output.blending_offset(), 0.0);
        }
    }

    /// `Blender.empty().isEmpty()` (Blender.java lines 107-109).
    #[test]
    fn empty_is_empty() {
        assert!(Blender::empty().is_empty());
    }

    /// `Blender.EMPTY.getBiomeResolver(biomeResolver)` returns its argument
    /// unchanged (Blender.java lines 49-51) — the identity override. A
    /// `BiomeResolver` that always returns `BiomeId(id)` — the test double for
    /// the wrapped resolver in `get_biome_resolver`.
    struct TestResolver(u16);

    impl BiomeResolver for TestResolver {
        fn get_noise_biome(
            &self,
            _quart_x: i32,
            _quart_y: i32,
            _quart_z: i32,
            _sampler: &Sampler,
        ) -> Holder<BiomeId> {
            Holder::direct(BiomeId(self.0))
        }
    }

    #[test]
    fn empty_get_biome_resolver_is_identity() {
        // Java's `EMPTY` subclass overrides `getBiomeResolver` to the identity
        // (Blender.java lines 49-51): the empty `Blender` returns the resolver
        // unchanged. The returned resolver therefore behaves exactly like the
        // one passed in — every quart resolves through the wrapped resolver
        // (were it a non-empty `Blender`, `blendBiome` could win).
        let blender = Blender::empty();
        let resolver = TestResolver(7);
        let sampler = Climate::empty();
        let wrapped = blender.get_biome_resolver(Box::new(resolver));
        for (x, y, z) in [(0, 0, 0), (7, -13, 5), (i32::MAX, 0, i32::MIN)] {
            assert_eq!(
                wrapped.get_noise_biome(x, y, z, &sampler),
                Holder::direct(BiomeId(7))
            );
        }
    }

    /// `BlendingOutput` value semantics — record accessors and Java record
    /// equality: each `double` component compares via `Double.compare`
    /// (every NaN payload canonicalizes; `-0.0` is distinct from `0.0`).
    #[test]
    fn blending_output_value_semantics() {
        let output = BlendingOutput::new(0.5, -3.25);
        assert_eq!(output.alpha(), 0.5);
        assert_eq!(output.blending_offset(), -3.25);
        assert_eq!(output, BlendingOutput::new(0.5, -3.25));
        assert_ne!(output, BlendingOutput::new(0.5, 0.0));
        // `Double.compare` canonicalizes every NaN payload: two distinct
        // payloads compare equal (IEEE `==` and `total_cmp` both reject).
        let nan_a = f64::from_bits(0x7ff8_0000_0000_0001);
        let nan_b = f64::from_bits(0x7ff8_0000_0000_0002);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        assert_ne!(nan_a.total_cmp(&nan_b), Ordering::Equal);
        assert_eq!(
            BlendingOutput::new(nan_a, 1.0),
            BlendingOutput::new(nan_b, 1.0)
        );
        // `Double.compare(-0.0, 0.0) != 0` — signed zero is distinct.
        assert_ne!(
            BlendingOutput::new(-0.0, 1.0),
            BlendingOutput::new(0.0, 1.0)
        );
    }

    /// `heightToOffset` (Blender.java lines 149-154) — exact `f64` values for
    /// two fixed heights.
    #[test]
    fn height_to_offset_math() {
        assert_eq!(Blender::height_to_offset(120.0), -0.06147540983606557);
        assert_eq!(Blender::height_to_offset(100.0), -0.23479729729729729);
    }

    /// `blendOffsetAndFactor` fixed-height path (Blender.java lines 114-117):
    /// a queried cell that lands on a stored old-chunk height returns
    /// `(0.0, heightToOffset(height))` without any weighting.
    #[test]
    fn blend_offset_fixed_height_wins() {
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(ChunkPos::pack_coords(0, 0), data_with_height(3, 100.0));
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        // block (0, 0) → cell (0, 0) → the stored inside cell (0, 0).
        let output = blender.blend_offset_and_factor(0, 0);
        assert_eq!(output.alpha(), 0.0);
        assert_eq!(output.blending_offset(), -0.23479729729729729);
    }

    /// `blendOffsetAndFactor` weighted path (Blender.java lines 119-147): a
    /// query away from any stored height falls back to the inverse-distance-4
    /// weighted average; the alpha is the smoothstep of
    /// `closestDistance / (RANGE + 1)`.
    #[test]
    fn blend_offset_weighted_average() {
        let mut height_and_biome = BTreeMap::new();
        // One old chunk at chunk (0, 0), height 100 at inside cell (0, 0).
        height_and_biome.insert(ChunkPos::pack_coords(0, 0), data_with_height(3, 100.0));
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        // block (4, 4) → cell (1, 1): the fixed lookup is the interior cell
        // (1, 1) → NO_VALUE, so the weighted path runs. The only neighbouring
        // height is cell (0, 0) at distance `Mth.length(1, 1)`, which Java
        // resolves to the float overload — `(float)Math.sqrt(2)` widened to
        // double — so the expected alpha uses the f32-rounded distance.
        let output = blender.blend_offset_and_factor(4, 4);
        let distance = mth::length_f32(1.0, 1.0) as f64;
        let alpha = mth::clamp_f64(
            distance / (HEIGHT_BLENDING_RANGE_CELLS + 1) as f64,
            0.0,
            1.0,
        );
        let smoothstep = 3.0 * alpha * alpha - 2.0 * alpha * alpha * alpha;
        assert_eq!(output.alpha(), smoothstep);
        assert_eq!(output.blending_offset(), -0.23479729729729729);
    }

    /// `blendDensity` fixed-density path (Blender.java lines 159-162): a
    /// queried cell on a stored old-chunk density returns it directly (already
    /// `* 0.1`-scaled by `BlendingData.getDensity`).
    #[test]
    fn blend_density_fixed_wins() {
        let mut height_and_biome = BTreeMap::new();
        let mut data = data_with_height(3, 100.0);
        // Put a density of 0.5 (→ stored 0.05) at inside cell (0, 0), cell y 5.
        let mut columns = vec![None; 16];
        let mut column = vec![0.0; 48];
        column[12] = 0.5;
        columns[3] = Some(column);
        data.densities = columns;
        height_and_biome.insert(ChunkPos::pack_coords(0, 0), data);
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        // block (0, 40, 0) → cell (0, 5, 0): the stored inside cell.
        let context = SinglePointContext::new(0, 40, 0);
        assert_eq!(blender.blend_density(&context, 99.0), 0.05);
    }

    /// `blendDensity` weighted path (Blender.java lines 165-195): a query away
    /// from any stored density falls back to the inverse-distance-4 weighted
    /// average lerped toward the generated noise value.
    #[test]
    fn blend_density_weighted_average() {
        // The density map carries the same data as the height/biome map (Java
        // `of` puts one BlendingData in both maps). Build a shared instance.
        let mut data = data_with_height(3, 100.0);
        let mut columns = vec![None; 16];
        let mut column = vec![2.0; 48];
        column[12] = 0.5; // inside cell (0, 0), cell y 5
        columns[3] = Some(column);
        data.densities = columns;
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(ChunkPos::pack_coords(0, 0), data.clone());
        let mut density_blending = BTreeMap::new();
        density_blending.insert(ChunkPos::pack_coords(0, 0), data);
        let blender = Blender::new(height_and_biome, density_blending);
        // block (4, 40, 4) → cell (1, 5, 1): interior → NO_VALUE fixed lookup,
        // so the weighted path runs over the density map. The band is
        // [4, 6) cells; the contributing density cell is (0, 5, 0) at
        // distance length(1, 0, 1) = sqrt(2); the (0, 4, 0) cell at
        // length(1, 2, 1) = sqrt(6) is outside the 2.0 density range.
        let context = SinglePointContext::new(4, 40, 4);
        let average = 0.05;
        let distance = std::f64::consts::SQRT_2;
        let alpha = mth::clamp_f64(distance / 3.0, 0.0, 1.0);
        let expected = mth::lerp(alpha, average, 1.0);
        assert_eq!(blender.blend_density(&context, 1.0), expected);
    }

    /// `getBlendingDataValue` corner fallback (Blender.java lines 204-207): a
    /// cell at a chunk's minimum corner falls back to the (x-1, z-1) chunk.
    /// The stored height lives in chunk (-1, -1) at the outside ring cell
    /// (4, 4) (`getOutsideIndex(4, 4) = 11`) — the cell the corner fallback
    /// reads for query cell (0, 0) — and chunk (0, 0) itself is absent, so
    /// only the corner branch can find it.
    #[test]
    fn blend_offset_corner_fallback_reads_neighbour() {
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(ChunkPos::pack_coords(-1, -1), data_with_height(11, 120.0));
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        // block (0, 0) → cell (0, 0): chunk (0, 0)'s min corner. The first
        // lookup (chunk (0, 0), absent) and both edge branches miss; the
        // corner fallback reads chunk (-1, -1) relative cell (4, 4).
        let output = blender.blend_offset_and_factor(0, 0);
        assert_eq!(output.alpha(), 0.0);
        assert_eq!(output.blending_offset(), -0.06147540983606557);
    }

    /// `getBlendingDataValue` min-X edge fallback (Blender.java lines
    /// 210-212): a cell on a chunk's min-X edge (non-min-Z) falls back to the
    /// (x-1, z) chunk. The stored height lives in chunk (-1, 0) at the outside
    /// ring cell (4, 1) (`getOutsideIndex(4, 1) = 14`), which the min-X branch
    /// reads for query cell (0, 1).
    #[test]
    fn blend_offset_min_x_edge_fallback_reads_neighbour() {
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(ChunkPos::pack_coords(-1, 0), data_with_height(14, 120.0));
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        // block (0, 4) → cell (0, 1): min X, non-min Z. The corner and min-Z
        // branches are skipped; the min-X fallback reads chunk (-1, 0)
        // relative cell (4, 1).
        let output = blender.blend_offset_and_factor(0, 4);
        assert_eq!(output.alpha(), 0.0);
        assert_eq!(output.blending_offset(), -0.06147540983606557);
    }

    /// `getBlendingDataValue` min-Z edge fallback (Blender.java lines
    /// 214-216): a cell on a chunk's min-Z edge (non-min-X) falls back to the
    /// (x, z-1) chunk. The stored height lives in chunk (0, -1) at the outside
    /// ring cell (1, 4) (`getOutsideIndex(1, 4) = 8`), which the min-Z branch
    /// reads for query cell (1, 0).
    #[test]
    fn blend_offset_min_z_edge_fallback_reads_neighbour() {
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(ChunkPos::pack_coords(0, -1), data_with_height(8, 120.0));
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        // block (4, 0) → cell (1, 0): non-min X, min Z. The corner and min-X
        // branches are skipped; the min-Z fallback reads chunk (0, -1)
        // relative cell (1, 4).
        let output = blender.blend_offset_and_factor(4, 0);
        assert_eq!(output.alpha(), 0.0);
        assert_eq!(output.blending_offset(), -0.06147540983606557);
    }

    /// A `BlendingData` for the overworld with a single biome at inside cell
    /// `(x, z)` (column index `getInsideIndex(x, z)`), at quart band `quart_y`.
    fn data_with_biome(x: i32, z: i32, quart_y: i32, biome: Holder<BiomeId>) -> BlendingData {
        let mut data = data_with_height(3, 100.0);
        let mut column = vec![None; 96];
        column[(quart_y - (-16)) as usize] = Some(biome);
        // The inside-cell column index `getInsideIndex(x, z)` (BlendingData.java
        // lines 352-354), inlined because the helper is private to
        // `blending_data`.
        let inside_index = (CELL_HORIZONTAL_MAX_INDEX_INSIDE
            .wrapping_sub(x)
            .wrapping_add(z)) as usize;
        data.biomes[inside_index] = Some(column);
        data
    }

    /// `blendBiome` distance-zero path (Blender.java lines 239-263): a stored
    /// biome at the queried quart cell wins regardless of the `SHIFT_NOISE`
    /// alpha (distance 0 clamps the alpha to 0). The query quart (0, y, 0)
    /// lands on the interior cell (0, 0) of chunk (0, 0).
    #[test]
    fn blend_biome_distance_zero_returns_stored_biome() {
        let biome = Holder::direct(BiomeId(12));
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(
            ChunkPos::pack_coords(0, 0),
            data_with_biome(0, 0, 0, biome.clone()),
        );
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        assert_eq!(blender.blend_biome(0, 0, 0), Some(biome));
    }

    /// `blendBiome` far-query path: a query more than `HEIGHT_BLENDING_RANGE_CELLS`
    /// from every stored biome yields `None` (delegates to the wrapped
    /// resolver). The stored inside-cell biome of chunk (0, 0) is at most
    /// `sqrt(2 * 27^2) < 39` cells away, so a query at quart (50, 0, 50) — a
    /// chunk more than 27 cells out — is out of range.
    #[test]
    fn blend_biome_out_of_range_is_none() {
        let biome = Holder::direct(BiomeId(12));
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(ChunkPos::pack_coords(0, 0), data_with_biome(0, 0, 0, biome));
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        assert_eq!(blender.blend_biome(50, 0, 50), None);
    }

    /// `getBiomeResolver` on a non-empty Blender (Blender.java lines 232-263):
    /// the wrapper's `blendBiome` wins when a stored biome is within range at
    /// distance zero, and delegates to the wrapped resolver otherwise.
    #[test]
    fn get_biome_resolver_blends_then_delegates() {
        let biome = Holder::direct(BiomeId(12));
        let mut height_and_biome = BTreeMap::new();
        height_and_biome.insert(
            ChunkPos::pack_coords(0, 0),
            data_with_biome(0, 0, 0, biome.clone()),
        );
        let blender = Blender::new(height_and_biome, BTreeMap::new());
        let resolver = TestResolver(77);
        let sampler = Climate::empty();
        let wrapped = blender.get_biome_resolver(Box::new(resolver));
        // On the stored biome → the blended biome, not the delegate.
        assert_eq!(wrapped.get_noise_biome(0, 0, 0, &sampler), biome);
        // Out of range → the delegate.
        assert_eq!(
            wrapped.get_noise_biome(50, 0, 50, &sampler),
            Holder::direct(BiomeId(77))
        );
    }
}

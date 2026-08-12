//! Port of `net.minecraft.world.level.levelgen.blending.Blender` (class,
//! 26.2) — the value prerequisite slice (issue #177).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! levelgen/blending/Blender.java`.
//!
//! This slice lands only the empty-singleton value behavior the shared Blender
//! prerequisite needs, exactly as `Blender.EMPTY` (the anonymous subclass,
//! Blender.java lines 37-52) implements it:
//!
//! - [`Blender::empty`] / [`Blender::is_empty`] — the `EMPTY` singleton.
//! - [`Blender::blend_density`] — identity (`noiseValue` unchanged).
//! - [`Blender::blend_offset_and_factor`] — the `(1.0, 0.0)` constant.
//! - [`BlendingOutput`] — the result record.
//!
//! The non-empty surface defers as `RivetTodo(#177)` owned by the
//! `mc.world.level.levelgen.blending` unit:
//!
//! - `of(WorldGenRegion)` and the `heightAndBiomeBlendingData`/
//!   `densityBlendingData` `Long2ObjectOpenHashMap<BlendingData>` fields (need
//!   `BlendingData` + `WorldGenRegion`, not ported in this slice).
//! - The weighted height/density blends behind the non-empty
//!   `blendOffsetAndFactor`/`blendDensity` (`getBlendingDataValue`,
//!   `iterateHeights`/`iterateDensities`, `heightToOffset`).
//! - `getBiomeResolver` — the identity biome-resolver seam is kept modular and
//!   defers: `BiomeResolver` is typed by the concurrently-owned
//!   `mc.world.level.levelgen.biome` climate surface, which is not yet
//!   available. No placeholder is invented; the seam slots in when that unit
//!   lands.
//! - `blendBiome` + the `SHIFT_NOISE` `NormalNoise`, and the
//!   `generateBorderTicks`/`addAroundOldChunksCarvingMaskFilter`/
//!   `makeOldChunkDistanceGetter`/`DistanceGetter` chunk-border surfaces.

use crate::levelgen::noise::density_function::FunctionContext;

/// `Blender.BlendingOutput(double alpha, double blendingOffset)` record — the
/// `blendOffsetAndFactor` result.
///
/// Equality mirrors the Java record's generated `equals`, which compares each
/// `double` component with `Double.compare(...) == 0`: NaN equals itself and
/// `-0.0` is distinct from `0.0` (the derived IEEE `==` does neither).
#[derive(Debug, Clone, Copy)]
pub struct BlendingOutput {
    alpha: f64,
    blending_offset: f64,
}

impl PartialEq for BlendingOutput {
    fn eq(&self, other: &Self) -> bool {
        // `total_cmp` mirrors `Double.compare` exactly, unlike the derived
        // `PartialEq`.
        self.alpha.total_cmp(&other.alpha).is_eq()
            && self
                .blending_offset
                .total_cmp(&other.blending_offset)
                .is_eq()
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

/// `net.minecraft.world.level.levelgen.blending.Blender`.
///
/// Only the empty singleton is constructible in this slice: the
/// `heightAndBiomeBlendingData`/`densityBlendingData`
/// `Long2ObjectOpenHashMap<BlendingData>` fields and the `of(WorldGenRegion)`
/// factory defer with the `mc.world.level.levelgen.blending` unit
/// (RivetTodo #177), so every `Blender` value is `EMPTY`.
#[derive(Debug, Clone)]
pub struct Blender;

impl Blender {
    /// `Blender.empty()` — returns the shared `EMPTY` singleton.
    pub fn empty() -> Blender {
        Blender
    }

    /// `isEmpty()` — `heightAndBiomeBlendingData.isEmpty() &&
    /// densityBlendingData.isEmpty()`.
    ///
    /// The empty singleton is the only constructible value, so this is always
    /// `true`; a non-empty `of(WorldGenRegion)` construction would return
    /// `false` (RivetTodo #177).
    pub fn is_empty(&self) -> bool {
        true
    }

    /// `blendDensity(FunctionContext, double)` — the empty singleton returns
    /// `noiseValue` unchanged.
    ///
    /// RivetTodo(#177): the non-empty path reads the density-blending data and
    /// returns the distance-weighted/lerped average.
    pub fn blend_density(&self, _context: &dyn FunctionContext, noise_value: f64) -> f64 {
        noise_value
    }

    /// `blendOffsetAndFactor(int blockX, int blockZ)` — the empty singleton's
    /// constant `BlendingOutput(1.0, 0.0)`.
    ///
    /// RivetTodo(#177): the non-empty path looks up the fixed height and
    /// computes the weighted offset/alpha (`getBlendingDataValue` +
    /// `heightToOffset`).
    pub fn blend_offset_and_factor(&self, _block_x: i32, _block_z: i32) -> BlendingOutput {
        BlendingOutput::new(1.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::density_function::SinglePointContext;

    /// `Blender.EMPTY.blendDensity(context, noiseValue) == noiseValue`
    /// (Blender.java line 44).
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

    /// `BlendingOutput` value semantics — record accessors and Java record
    /// equality: each `double` component compares via `Double.compare`
    /// (NaN equals itself; `-0.0` is distinct from `0.0`).
    #[test]
    fn blending_output_value_semantics() {
        let output = BlendingOutput::new(0.5, -3.25);
        assert_eq!(output.alpha(), 0.5);
        assert_eq!(output.blending_offset(), -3.25);
        assert_eq!(output, BlendingOutput::new(0.5, -3.25));
        assert_ne!(output, BlendingOutput::new(0.5, 0.0));
        // `Double.compare(NaN, NaN) == 0` — NaN equals itself, which the
        // derived IEEE `==` (NaN != NaN) would reject.
        assert_eq!(
            BlendingOutput::new(f64::NAN, 1.0),
            BlendingOutput::new(f64::NAN, 1.0)
        );
        // `Double.compare(-0.0, 0.0) != 0` — signed zero is distinct.
        assert_ne!(
            BlendingOutput::new(-0.0, 1.0),
            BlendingOutput::new(0.0, 1.0)
        );
    }
}

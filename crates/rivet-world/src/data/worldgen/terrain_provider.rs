//! Port of `net.minecraft.data.worldgen.TerrainProvider`.
//!
//! The overworld terrain splines: the offset/factor/jaggedness `CubicSpline`
//! builders over the `continents`/`erosion`/`ridges`/`weirdness` coordinates,
//! plus the `peaksAndValleys` ridge function. These are consumed by the biome
//! and density-function layers (`#178`/`#177`) before the data-driven
//! registries exist, so they are plain static builders over `BoundedFloat`
//! coordinates.
//!
//! Translation notes:
//! - The Java methods are generic over `I extends BoundedFloatFunction<?>`; the
//!   Rust port needs only the bounds half (`BoundedFloat`) — the builder and
//!   `Multipoint` construct use the coordinate's `min_value`/`max_value` and
//!   never `apply`. `I: Clone` is added because Java stores the *same* spline
//!   object at multiple points (`addPoint(-0.16F, beachSpline)` +
//!   `addPoint(-0.15F, beachSpline)` share one reference); the Rust enum is
//!   owned, so reuse is a value clone (`CubicSpline<I>: Clone` needs `I:
//!   Clone`).
//! - `Float2FloatFunction` (`it.unimi.dsi.fastutil`) is `Arc<dyn Fn(f32) -> f32
//!   + Send + Sync>`; `CubicSpline.builder(coordinate, valueTransformer)` maps
//!   to `CubicSpline::builder_with`.
//! - The value transformer applies only to `addPoint(float, float)`
//!   constants — never to spline-valued points (`add_spline` stores the
//!   sampler as-is) — matching Java's `Builder.addPoint`.
//! - `Mth.lerp` (f32) → `mth::lerp_f32`; `Math.max`/`Math.min` (f32) →
//!   `mth::max_f32`/`mth::min_f32` (NaN-propagating, Java semantics).
//! - `peaksAndValleys` is pure f32 `abs` — direct.
//! - Dead locals in the 26.2 source (`lowPeaks`, `valleyPlateau`, `plateau`,
//!   `erosionIndex1Middle`, `farInlandMiddle`, the `ridgeOffset`/
//!   `ridgeAmplitude` name bindings whose literals are used inline,
//!   `allowRiversBelow`, `afterRiverPoint`, `beforeRiverPoint`, `smallOffset`)
//!   are not ported: they are unobservable. The four private
//!   `DEEP_OCEAN_CONTINENTALNESS`/`OCEAN_CONTINENTALNESS`/
//!   `PLAINS_CONTINENTALNESS`/`BEACH_CONTINENTALNESS` constants are also dead
//!   code in the source and not ported.

use rivet_util::bounded_float_function::BoundedFloat;
use rivet_util::cubic_spline::CubicSpline;
use rivet_util::mth;
use std::sync::Arc;

/// `Float2FloatFunction.identity()`.
fn no_transform() -> Arc<dyn Fn(f32) -> f32 + Send + Sync> {
    Arc::new(|value| value)
}

/// `TerrainProvider.AMPLIFIED_OFFSET` — `offset < 0.0F ? offset : offset * 2.0F`.
fn amplified_offset() -> Arc<dyn Fn(f32) -> f32 + Send + Sync> {
    Arc::new(|offset| if offset < 0.0 { offset } else { offset * 2.0 })
}

/// `TerrainProvider.AMPLIFIED_FACTOR` — `1.25F - 6.25F / (factor + 5.0F)`.
fn amplified_factor() -> Arc<dyn Fn(f32) -> f32 + Send + Sync> {
    Arc::new(|factor| 1.25 - 6.25 / (factor + 5.0))
}

/// `TerrainProvider.AMPLIFIED_JAGGEDNESS` — `jaggedness * 2.0F`.
fn amplified_jaggedness() -> Arc<dyn Fn(f32) -> f32 + Send + Sync> {
    Arc::new(|jaggedness| jaggedness * 2.0)
}

/// `TerrainProvider.peaksAndValleys(float weirdness)`.
///
/// `-(Math.abs(Math.abs(weirdness) - 0.6666667F) - 0.33333334F) * 3.0F`.
pub fn peaks_and_valleys(weirdness: f32) -> f32 {
    -((weirdness.abs() - 0.6666667).abs() - 0.33333334) * 3.0
}

/// `TerrainProvider.overworldOffset(continents, erosion, ridges, amplified)`.
pub fn overworld_offset<I: BoundedFloat + Clone>(
    continents: I,
    erosion: I,
    ridges: I,
    amplified: bool,
) -> CubicSpline<I> {
    let offset_transformer = if amplified {
        amplified_offset()
    } else {
        no_transform()
    };
    let beach_spline = build_erosion_offset_spline(
        erosion.clone(),
        ridges.clone(),
        -0.15,
        0.0,
        0.0,
        0.1,
        0.0,
        -0.03,
        false,
        false,
        offset_transformer.clone(),
    );
    let low_spline = build_erosion_offset_spline(
        erosion.clone(),
        ridges.clone(),
        -0.1,
        0.03,
        0.1,
        0.1,
        0.01,
        -0.03,
        false,
        false,
        offset_transformer.clone(),
    );
    let mid_spline = build_erosion_offset_spline(
        erosion.clone(),
        ridges.clone(),
        -0.1,
        0.03,
        0.1,
        0.7,
        0.01,
        -0.03,
        true,
        true,
        offset_transformer.clone(),
    );
    let high_spline = build_erosion_offset_spline(
        erosion,
        ridges,
        -0.05,
        0.03,
        0.1,
        1.0,
        0.01,
        0.01,
        true,
        true,
        offset_transformer.clone(),
    );
    CubicSpline::builder_with(continents, offset_transformer)
        .add_point(-1.1, 0.044)
        .add_point(-1.02, -0.2222)
        .add_point(-0.51, -0.2222)
        .add_point(-0.44, -0.12)
        .add_point(-0.18, -0.12)
        .add_spline(-0.16, beach_spline.clone())
        .add_spline(-0.15, beach_spline)
        .add_spline(-0.1, low_spline)
        .add_spline(0.25, mid_spline)
        .add_spline(1.0, high_spline)
        .build()
}

/// `TerrainProvider.overworldFactor(continents, erosion, weirdness, ridges, amplified)`.
pub fn overworld_factor<I: BoundedFloat + Clone>(
    continents: I,
    erosion: I,
    weirdness: I,
    ridges: I,
    amplified: bool,
) -> CubicSpline<I> {
    let factor_transformer = if amplified {
        amplified_factor()
    } else {
        no_transform()
    };
    CubicSpline::builder_with(continents, no_transform())
        .add_point(-0.19, 3.95)
        .add_spline(
            -0.15,
            get_erosion_factor(
                erosion.clone(),
                weirdness.clone(),
                ridges.clone(),
                6.25,
                true,
                no_transform(),
            ),
        )
        .add_spline(
            -0.1,
            get_erosion_factor(
                erosion.clone(),
                weirdness.clone(),
                ridges.clone(),
                5.47,
                true,
                factor_transformer.clone(),
            ),
        )
        .add_spline(
            0.03,
            get_erosion_factor(
                erosion.clone(),
                weirdness.clone(),
                ridges.clone(),
                5.08,
                true,
                factor_transformer.clone(),
            ),
        )
        .add_spline(
            0.06,
            get_erosion_factor(erosion, weirdness, ridges, 4.69, false, factor_transformer),
        )
        .build()
}

/// `TerrainProvider.overworldJaggedness(continents, erosion, weirdness, ridges, amplified)`.
pub fn overworld_jaggedness<I: BoundedFloat + Clone>(
    continents: I,
    erosion: I,
    weirdness: I,
    ridges: I,
    amplified: bool,
) -> CubicSpline<I> {
    let jaggedness_transformer = if amplified {
        amplified_jaggedness()
    } else {
        no_transform()
    };
    CubicSpline::builder_with(continents, jaggedness_transformer.clone())
        .add_point(-0.11, 0.0)
        .add_spline(
            0.03,
            build_erosion_jaggedness_spline(
                erosion.clone(),
                weirdness.clone(),
                ridges.clone(),
                1.0,
                0.5,
                0.0,
                0.0,
                jaggedness_transformer.clone(),
            ),
        )
        .add_spline(
            0.65,
            build_erosion_jaggedness_spline(
                erosion,
                weirdness,
                ridges,
                1.0,
                1.0,
                1.0,
                0.0,
                jaggedness_transformer,
            ),
        )
        .build()
}

/// `TerrainProvider.buildErosionJaggednessSpline(erosion, weirdness, ridges, ...)`.
#[allow(clippy::too_many_arguments)] // Java's method has 8 parameters.
fn build_erosion_jaggedness_spline<I: BoundedFloat + Clone>(
    erosion: I,
    weirdness: I,
    ridges: I,
    jaggedness_factor_at_peak_ridge_and_erosion_index_0: f32,
    jaggedness_factor_at_peak_ridge_and_erosion_index_1: f32,
    jaggedness_factor_at_high_ridge_and_erosion_index_0: f32,
    jaggedness_factor_at_high_ridge_and_erosion_index_1: f32,
    jaggedness_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let ridge_jaggedness_spline_at_erosion_0 = build_ridge_jaggedness_spline(
        weirdness.clone(),
        ridges.clone(),
        jaggedness_factor_at_peak_ridge_and_erosion_index_0,
        jaggedness_factor_at_high_ridge_and_erosion_index_0,
        jaggedness_transformer.clone(),
    );
    let ridge_jaggedness_spline_at_erosion_1 = build_ridge_jaggedness_spline(
        weirdness,
        ridges,
        jaggedness_factor_at_peak_ridge_and_erosion_index_1,
        jaggedness_factor_at_high_ridge_and_erosion_index_1,
        jaggedness_transformer.clone(),
    );
    CubicSpline::builder_with(erosion, jaggedness_transformer)
        .add_spline(-1.0, ridge_jaggedness_spline_at_erosion_0)
        .add_spline(-0.78, ridge_jaggedness_spline_at_erosion_1.clone())
        .add_spline(-0.5775, ridge_jaggedness_spline_at_erosion_1)
        .add_point(-0.375, 0.0)
        .build()
}

/// `TerrainProvider.buildRidgeJaggednessSpline(weirdness, ridges, ...)`.
fn build_ridge_jaggedness_spline<I: BoundedFloat + Clone>(
    weirdness: I,
    ridges: I,
    jaggedness_factor_at_peak_ridge: f32,
    jaggedness_factor_at_high_ridge: f32,
    jaggedness_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let high_slice_start = peaks_and_valleys(0.4);
    let high_slice_end = peaks_and_valleys(0.56666666);
    let high_slice_middle = (high_slice_start + high_slice_end) / 2.0;
    let mut ridge_spline = CubicSpline::builder_with(ridges, jaggedness_transformer.clone());
    ridge_spline = ridge_spline.add_point(high_slice_start, 0.0);
    if jaggedness_factor_at_high_ridge > 0.0 {
        ridge_spline = ridge_spline.add_spline(
            high_slice_middle,
            build_weirdness_jaggedness_spline(
                weirdness.clone(),
                jaggedness_factor_at_high_ridge,
                jaggedness_transformer.clone(),
            ),
        );
    } else {
        ridge_spline = ridge_spline.add_point(high_slice_middle, 0.0);
    }
    if jaggedness_factor_at_peak_ridge > 0.0 {
        ridge_spline = ridge_spline.add_spline(
            1.0,
            build_weirdness_jaggedness_spline(
                weirdness,
                jaggedness_factor_at_peak_ridge,
                jaggedness_transformer,
            ),
        );
    } else {
        ridge_spline = ridge_spline.add_point(1.0, 0.0);
    }
    ridge_spline.build()
}

/// `TerrainProvider.buildWeirdnessJaggednessSpline(weirdness, jaggednessFactor, ...)`.
fn build_weirdness_jaggedness_spline<I: BoundedFloat>(
    weirdness: I,
    jaggedness_factor: f32,
    jaggedness_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let max_jaggedness_at_negative_weirdness = 0.63 * jaggedness_factor;
    let max_jaggedness_at_positive_weirdness = 0.3 * jaggedness_factor;
    CubicSpline::builder_with(weirdness, jaggedness_transformer)
        .add_point(-0.01, max_jaggedness_at_negative_weirdness)
        .add_point(0.01, max_jaggedness_at_positive_weirdness)
        .build()
}

/// `TerrainProvider.getErosionFactor(erosion, weirdness, ridges, baseValue, shatteredTerrain, factorTransformer)`.
fn get_erosion_factor<I: BoundedFloat + Clone>(
    erosion: I,
    weirdness: I,
    ridges: I,
    base_value: f32,
    shattered_terrain: bool,
    factor_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let base_spline = CubicSpline::builder_with(weirdness.clone(), factor_transformer.clone())
        .add_point(-0.2, 6.3)
        .add_point(0.2, base_value)
        .build();
    let mut erosion_points = CubicSpline::builder_with(erosion.clone(), factor_transformer.clone())
        .add_spline(-0.6, base_spline.clone())
        .add_spline(
            -0.5,
            CubicSpline::builder_with(weirdness.clone(), factor_transformer.clone())
                .add_point(-0.05, 6.3)
                .add_point(0.05, 2.67)
                .build(),
        )
        .add_spline(-0.35, base_spline.clone())
        .add_spline(-0.25, base_spline.clone())
        .add_spline(
            -0.1,
            CubicSpline::builder_with(weirdness.clone(), factor_transformer.clone())
                .add_point(-0.05, 2.67)
                .add_point(0.05, 6.3)
                .build(),
        )
        .add_spline(0.03, base_spline.clone());
    if shattered_terrain {
        let weirdness_shattered = CubicSpline::builder_with(weirdness, factor_transformer.clone())
            .add_point(0.0, base_value)
            .add_point(0.1, 0.625)
            .build();
        let ridges_shattered = CubicSpline::builder_with(ridges, factor_transformer.clone())
            .add_point(-0.9, base_value)
            .add_spline(-0.69, weirdness_shattered)
            .build();
        erosion_points = erosion_points
            .add_point(0.35, base_value)
            .add_spline(0.45, ridges_shattered.clone())
            .add_spline(0.55, ridges_shattered)
            .add_point(0.62, base_value);
    } else {
        let extreme_hills_terrain_from_mid_slice_and_up =
            CubicSpline::builder_with(ridges.clone(), factor_transformer.clone())
                .add_spline(-0.7, base_spline.clone())
                .add_point(-0.15, 1.37)
                .build();
        let extra_3d_noise_on_peaks_only =
            CubicSpline::builder_with(ridges, factor_transformer.clone())
                .add_spline(0.45, base_spline)
                .add_point(0.7, 1.56)
                .build();
        erosion_points = erosion_points
            .add_spline(0.05, extra_3d_noise_on_peaks_only.clone())
            .add_spline(0.4, extra_3d_noise_on_peaks_only)
            .add_spline(0.45, extreme_hills_terrain_from_mid_slice_and_up.clone())
            .add_spline(0.55, extreme_hills_terrain_from_mid_slice_and_up)
            .add_point(0.58, base_value);
    }
    erosion_points.build()
}

/// `TerrainProvider.calculateSlope(y1, y2, x1, x2)`.
fn calculate_slope(y1: f32, y2: f32, x1: f32, x2: f32) -> f32 {
    (y2 - y1) / (x2 - x1)
}

/// `TerrainProvider.buildMountainRidgeSplineWithPoints(ridges, modulation, saddle, offsetTransformer)`.
fn build_mountain_ridge_spline_with_points<I: BoundedFloat + Clone>(
    ridges: I,
    modulation: f32,
    saddle: bool,
    offset_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let mut build = CubicSpline::builder_with(ridges, offset_transformer);
    let min_point_continentalness = mountain_continentalness(-1.0, modulation, -0.7);
    let max_point_continentalness = mountain_continentalness(1.0, modulation, -0.7);
    let ridge_zero_point = calculate_mountain_ridge_zero_continentalness_point(modulation);
    if -0.65 < ridge_zero_point && ridge_zero_point < 1.0 {
        let after_river_threshold_continentalness =
            mountain_continentalness(-0.65, modulation, -0.7);
        let before_river_threshold_continentalness =
            mountain_continentalness(-0.75, modulation, -0.7);
        let min_point_derivative = calculate_slope(
            min_point_continentalness,
            before_river_threshold_continentalness,
            -1.0,
            -0.75,
        );
        build =
            build.add_point_with_derivative(-1.0, min_point_continentalness, min_point_derivative);
        build = build.add_point(-0.75, before_river_threshold_continentalness);
        build = build.add_point(-0.65, after_river_threshold_continentalness);
        let ridge_zero_point_continentalness =
            mountain_continentalness(ridge_zero_point, modulation, -0.7);
        let max_point_derivative = calculate_slope(
            ridge_zero_point_continentalness,
            max_point_continentalness,
            ridge_zero_point,
            1.0,
        );
        build = build.add_point(ridge_zero_point - 0.01, ridge_zero_point_continentalness);
        build = build.add_point_with_derivative(
            ridge_zero_point,
            ridge_zero_point_continentalness,
            max_point_derivative,
        );
        build =
            build.add_point_with_derivative(1.0, max_point_continentalness, max_point_derivative);
    } else {
        let simple_derivative = calculate_slope(
            min_point_continentalness,
            max_point_continentalness,
            -1.0,
            1.0,
        );
        if saddle {
            build = build.add_point(-1.0, mth::max_f32(0.2, min_point_continentalness));
            build = build.add_point_with_derivative(
                0.0,
                mth::lerp_f32(0.5, min_point_continentalness, max_point_continentalness),
                simple_derivative,
            );
        } else {
            build =
                build.add_point_with_derivative(-1.0, min_point_continentalness, simple_derivative);
        }
        build = build.add_point_with_derivative(1.0, max_point_continentalness, simple_derivative);
    }
    build.build()
}

/// `TerrainProvider.mountainContinentalness(ridge, modulation, allowRiversBelow)`.
fn mountain_continentalness(ridge: f32, modulation: f32, allow_rivers_below: f32) -> f32 {
    let ridge_slope = 1.0 - (1.0 - modulation) * 0.5;
    let ridge_intersect = 0.5 * (1.0 - modulation);
    let adjusted_ridge_height = (ridge + 1.17) * 0.46082947;
    let continentalness = adjusted_ridge_height * ridge_slope - ridge_intersect;
    if ridge < allow_rivers_below {
        mth::max_f32(continentalness, -0.2222)
    } else {
        mth::max_f32(continentalness, 0.0)
    }
}

/// `TerrainProvider.calculateMountainRidgeZeroContinentalnessPoint(modulation)`.
fn calculate_mountain_ridge_zero_continentalness_point(modulation: f32) -> f32 {
    let ridge_slope = 1.0 - (1.0 - modulation) * 0.5;
    let ridge_intersect = 0.5 * (1.0 - modulation);
    ridge_intersect / (0.46082947 * ridge_slope) - 1.17
}

/// `TerrainProvider.buildErosionOffsetSpline(erosion, ridges, ...)`.
#[allow(clippy::too_many_arguments)] // Java's method has 10 parameters.
pub fn build_erosion_offset_spline<I: BoundedFloat + Clone>(
    erosion: I,
    ridges: I,
    low_valley: f32,
    hill: f32,
    tall_hill: f32,
    mountain_factor: f32,
    plain: f32,
    swamp: f32,
    include_extreme_hills: bool,
    saddle: bool,
    offset_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let very_low_erosion_mountains = build_mountain_ridge_spline_with_points(
        ridges.clone(),
        mth::lerp_f32(mountain_factor, 0.6, 1.5),
        saddle,
        offset_transformer.clone(),
    );
    let low_erosion_mountains = build_mountain_ridge_spline_with_points(
        ridges.clone(),
        mth::lerp_f32(mountain_factor, 0.6, 1.0),
        saddle,
        offset_transformer.clone(),
    );
    let mountains = build_mountain_ridge_spline_with_points(
        ridges.clone(),
        mountain_factor,
        saddle,
        offset_transformer.clone(),
    );
    let wide_plateau = ridge_spline(
        ridges.clone(),
        low_valley - 0.15,
        0.5 * mountain_factor,
        mth::lerp_f32(0.5, 0.5, 0.5) * mountain_factor,
        0.5 * mountain_factor,
        0.6 * mountain_factor,
        0.5,
        offset_transformer.clone(),
    );
    let narrow_plateau = ridge_spline(
        ridges.clone(),
        low_valley,
        plain * mountain_factor,
        hill * mountain_factor,
        0.5 * mountain_factor,
        0.6 * mountain_factor,
        0.5,
        offset_transformer.clone(),
    );
    let plains = ridge_spline(
        ridges.clone(),
        low_valley,
        plain,
        plain,
        hill,
        tall_hill,
        0.5,
        offset_transformer.clone(),
    );
    let plains_far_inland = ridge_spline(
        ridges.clone(),
        low_valley,
        plain,
        plain,
        hill,
        tall_hill,
        0.5,
        offset_transformer.clone(),
    );
    let extreme_hills = CubicSpline::builder_with(ridges.clone(), offset_transformer.clone())
        .add_point(-1.0, low_valley)
        .add_spline(-0.4, plains.clone())
        .add_point(0.0, tall_hill + 0.07)
        .build();
    let swamps = ridge_spline(
        ridges,
        -0.02,
        swamp,
        swamp,
        hill,
        tall_hill,
        0.0,
        offset_transformer.clone(),
    );
    let mut builder = CubicSpline::builder_with(erosion, offset_transformer)
        .add_spline(-0.85, very_low_erosion_mountains)
        .add_spline(-0.7, low_erosion_mountains)
        .add_spline(-0.4, mountains)
        .add_spline(-0.35, wide_plateau)
        .add_spline(-0.1, narrow_plateau)
        .add_spline(0.2, plains.clone());
    if include_extreme_hills {
        builder = builder
            .add_spline(0.4, plains_far_inland.clone())
            .add_spline(0.45, extreme_hills.clone())
            .add_spline(0.55, extreme_hills)
            .add_spline(0.58, plains_far_inland);
    }
    builder.add_spline(0.7, swamps).build()
}

/// `TerrainProvider.ridgeSpline(ridges, valley, low, mid, high, peaks, minValleySteepness, offsetTransformer)`.
#[allow(clippy::too_many_arguments)] // Java's method has 8 parameters.
fn ridge_spline<I: BoundedFloat>(
    ridges: I,
    valley: f32,
    low: f32,
    mid: f32,
    high: f32,
    peaks: f32,
    min_valley_steepness: f32,
    offset_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
) -> CubicSpline<I> {
    let d1 = mth::max_f32(0.5 * (low - valley), min_valley_steepness);
    let d2 = 5.0 * (mid - low);
    CubicSpline::builder_with(ridges, offset_transformer)
        .add_point_with_derivative(-1.0, valley, d1)
        .add_point_with_derivative(-0.4, low, mth::min_f32(d1, d2))
        .add_point_with_derivative(0.0, mid, d2)
        .add_point_with_derivative(0.4, high, 2.0 * (high - mid))
        .add_point_with_derivative(1.0, peaks, 0.7 * (peaks - high))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_util::bounded_float_function::{BoundedFloatFunction, Identity};

    // A fixed-value coordinate for building splines in tests. Bounds match a
    // real signal range so the Multipoint extrapolation stays finite.
    #[derive(Debug, Clone, Copy)]
    struct Coord {
        value: f32,
        min: f32,
        max: f32,
    }
    impl BoundedFloat for Coord {
        fn min_value(&self) -> f32 {
            self.min
        }
        fn max_value(&self) -> f32 {
            self.max
        }
    }
    impl<C> BoundedFloatFunction<C> for Coord {
        fn apply(&self, _c: C) -> f32 {
            self.value
        }
    }

    fn assert_close(a: f32, b: f32) {
        let diff = (a - b).abs();
        assert!(diff <= 1e-6, "expected {a} close to {b} (diff {diff})");
    }

    #[test]
    fn peaks_and_valleys_matches_java_reference_points() {
        // -(abs(abs(w) - 2/3) - 1/3) * 3: peaks at ±2/3, valley at 0. Past the
        // ±2/3 peak the value dips back toward 0 — w=1.0 is 0x1.8p-24 (~8.9e-8)
        // from the f32 rounding of 0.6666667/0.33333334, and w=0.33333334
        // lands exactly on -0.0. These exact bits match the golden fixture's
        // `peaks_and_valleys` sweep.
        assert_eq!(peaks_and_valleys(0.0).to_bits(), (-1.0f32).to_bits());
        assert_eq!(peaks_and_valleys(0.6666667).to_bits(), 1.0f32.to_bits());
        assert_eq!(peaks_and_valleys(-0.6666667).to_bits(), 1.0f32.to_bits());
        assert_eq!(peaks_and_valleys(0.33333334).to_bits(), (-0.0f32).to_bits());
        // The fixture's `peaks_and_valleys` sweep emits 0x1.8p-24 for w=1.0
        // (the same ~8.9e-8 Java produces from the f32 rounding).
        assert_eq!(
            peaks_and_valleys(1.0).to_bits(),
            hex_f32("0x1.8p-24").to_bits()
        );
        assert!(peaks_and_valleys(f32::NAN).is_nan());
    }

    #[test]
    fn peaks_and_valleys_is_even() {
        for w in [
            -1.0,
            -0.9,
            -0.6666667,
            -0.5,
            -0.33333334,
            -0.1,
            0.0,
            0.1,
            0.5,
            0.9,
            1.0,
        ] {
            assert_close(peaks_and_valleys(w), peaks_and_valleys(-w));
        }
    }

    #[test]
    fn overworld_offset_builds_with_expected_root_points() {
        let continents = Coord {
            value: 0.2,
            min: -1.0,
            max: 1.0,
        };
        let erosion = Coord {
            value: 0.0,
            min: -1.0,
            max: 1.0,
        };
        let ridges = Coord {
            value: 0.0,
            min: -1.0,
            max: 1.0,
        };
        let spline = overworld_offset(continents, erosion, ridges, false);
        match &spline {
            CubicSpline::Multipoint(m) => {
                // The 10 root knots of the offset spline (continents).
                assert_eq!(
                    m.locations(),
                    &[
                        -1.1, -1.02, -0.51, -0.44, -0.18, -0.16, -0.15, -0.1, 0.25, 1.0
                    ]
                );
            }
            CubicSpline::Constant(_) => panic!("offset spline must be multipoint"),
        }
    }

    // ------------------------------------------------------------------
    // Coordinate-routing checks
    //
    // The golden fixtures drive all four terrain inputs with the *same*
    // `Identity` coordinate, so a regression that swaps argument roles (e.g.
    // passing `ridges` where `erosion` belongs) would still pass every golden.
    // These tests use role-distinct `RoleCoord` instances — each with a
    // different `apply` slope — and assert both the coordinate identity at
    // every nesting level and a sampled value that would differ if the
    // `continents`/`erosion`/`weirdness`/`ridges` arguments were swapped. The
    // routing is asserted for all three overworld builders: the nested
    // `weirdness`/`ridges` coordinates of `overworld_jaggedness` sit at a
    // depth (>= 2) that the offset tree never reaches, and its whole-tree
    // sample is constant-zero, so neither the depth sweep nor a swapped
    // sample is visible through the offset builder's shape.
    // ------------------------------------------------------------------

    /// A role-distinct coordinate: `role` names the terrain input it stands
    /// for, `slope` gives its `apply` a per-role value so a swap changes the
    /// sampled output.
    #[derive(Debug, Clone, Copy)]
    struct RoleCoord {
        role: &'static str,
        slope: f32,
        min: f32,
        max: f32,
    }
    impl BoundedFloat for RoleCoord {
        fn min_value(&self) -> f32 {
            self.min
        }
        fn max_value(&self) -> f32 {
            self.max
        }
    }
    impl BoundedFloatFunction<f32> for RoleCoord {
        fn apply(&self, c: f32) -> f32 {
            self.slope * c
        }
    }

    /// Pre-order `(role, depth)` of every `Multipoint` coordinate in a spline
    /// tree.
    fn collect_role_depths(
        spline: &CubicSpline<RoleCoord>,
        depth: usize,
        out: &mut Vec<(&'static str, usize)>,
    ) {
        if let CubicSpline::Multipoint(m) = spline {
            out.push((m.coordinate().role, depth));
            for value in m.values() {
                collect_role_depths(value, depth + 1, out);
            }
        }
    }

    #[test]
    fn overworld_offset_routes_coordinates_by_role() {
        let continents = RoleCoord {
            role: "continents",
            slope: 1.0,
            min: -1.0,
            max: 1.0,
        };
        let erosion = RoleCoord {
            role: "erosion",
            slope: 2.0,
            min: -1.0,
            max: 1.0,
        };
        let ridges = RoleCoord {
            role: "ridges",
            slope: 3.0,
            min: -1.0,
            max: 1.0,
        };
        let spline = overworld_offset(continents, erosion, ridges, false);

        // Nesting: root is continents; the erosion splines (beach x2, low,
        // mid, high) sit directly under it; the ridge splines are nested under
        // those (depth >= 2 — the `extreme_hills` ridge spline nests a further
        // `plains` ridge spline via `add_spline`, so depth 3 also occurs). No
        // role appears at the wrong depth: erosion only at depth 1, ridges
        // never at depth 0 or 1.
        let mut roles = Vec::new();
        collect_role_depths(&spline, 0, &mut roles);
        assert_eq!(roles.first(), Some(&("continents", 0)));
        let erosion_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "erosion").collect();
        let ridge_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "ridges").collect();
        assert!(!erosion_entries.is_empty(), "erosion splines present");
        assert!(!ridge_entries.is_empty(), "ridge splines present");
        for (_, depth) in &erosion_entries {
            assert_eq!(
                *depth, 1,
                "erosion coordinate must be a direct child of continents"
            );
        }
        for (_, depth) in &ridge_entries {
            assert!(
                *depth >= 2,
                "ridge coordinate must be nested under erosion, not at depth {depth}"
            );
        }
        // The root has 10 knots; every non-constant knot value is an
        // erosion-coordinate spline, and each nests at least one ridge spline.
        let root = match &spline {
            CubicSpline::Multipoint(m) => m,
            CubicSpline::Constant(_) => panic!("offset spline must be multipoint"),
        };
        let erosion_splines: Vec<_> = root
            .values()
            .iter()
            .filter(|v| matches!(v, CubicSpline::Multipoint(m) if m.coordinate().role == "erosion"))
            .collect();
        assert!(!erosion_splines.is_empty());
        for es in &erosion_splines {
            let mut inner = Vec::new();
            collect_role_depths(es, 1, &mut inner);
            assert!(
                inner.iter().any(|(r, _)| *r == "ridges"),
                "erosion spline nests a ridge spline"
            );
        }

        // A swap of the erosion/ridges arguments must change the sample: the
        // nested splines then read their coordinates through the wrong `apply`
        // slope.
        let correct = spline;
        let swapped = overworld_offset(continents, ridges, erosion, false);
        let probe = 0.25;
        assert_ne!(
            correct.sample(probe).to_bits(),
            swapped.sample(probe).to_bits(),
            "swapping erosion/ridges must change the sampled value at {probe}"
        );
    }

    #[test]
    fn overworld_factor_routes_coordinates_by_role() {
        let continents = RoleCoord {
            role: "continents",
            slope: 1.0,
            min: -1.0,
            max: 1.0,
        };
        let erosion = RoleCoord {
            role: "erosion",
            slope: 2.0,
            min: -1.0,
            max: 1.0,
        };
        let weirdness = RoleCoord {
            role: "weirdness",
            slope: 4.0,
            min: -1.0,
            max: 1.0,
        };
        let ridges = RoleCoord {
            role: "ridges",
            slope: 3.0,
            min: -1.0,
            max: 1.0,
        };
        let spline = overworld_factor(continents, erosion, weirdness, ridges, false);

        // Nesting: root is continents; erosion sits directly under it. The
        // `shatteredTerrain=true` factor splines nest a `weirdness` spline at
        // depth 2; the `shatteredTerrain=false` one (base 4.69) nests `ridges`
        // splines at depth 2 that in turn nest the `weirdness` base spline at
        // depth 3. So `weirdness` appears at both depth 2 and depth 3, while
        // `ridges` only at depth 2.
        let mut roles = Vec::new();
        collect_role_depths(&spline, 0, &mut roles);
        assert_eq!(roles.first(), Some(&("continents", 0)));
        let erosion_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "erosion").collect();
        let weirdness_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "weirdness").collect();
        let ridge_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "ridges").collect();
        assert!(!erosion_entries.is_empty(), "erosion splines present");
        assert!(!weirdness_entries.is_empty(), "weirdness splines present");
        assert!(!ridge_entries.is_empty(), "ridge splines present");
        for (_, depth) in &erosion_entries {
            assert_eq!(
                *depth, 1,
                "erosion coordinate must be a direct child of continents"
            );
        }
        assert!(
            weirdness_entries.iter().any(|(_, depth)| *depth == 2),
            "weirdness must appear at depth 2 (direct child of erosion)"
        );
        assert!(
            weirdness_entries.iter().any(|(_, depth)| *depth == 3),
            "weirdness must appear at depth 3 (nested under the factor ridges spline)"
        );
        for (_, depth) in &weirdness_entries {
            assert!(
                *depth >= 2,
                "weirdness coordinate must be nested at depth >= 2, not at depth {depth}"
            );
        }
        for (_, depth) in &ridge_entries {
            assert_eq!(
                *depth, 2,
                "ridge coordinate must be a direct child of erosion, not at depth {depth}"
            );
        }

        // A swap of the erosion/weirdness arguments must change some sampled
        // value: the nested splines then read their coordinates through the
        // wrong `apply` slope, which maps a probe to a different knot interval
        // and/or interpolation point. Scan across the coordinate range — a
        // swap is undetectable only at probes whose coordinates all land in
        // the same constant-extended region, so a dense sweep must hit a
        // differing sample.
        let correct = spline;
        let swapped = overworld_factor(continents, weirdness, erosion, ridges, false);
        let differing = (0..=2000).any(|i| {
            let probe = -1.0 + (2.0 * i as f32) / 2000.0;
            correct.sample(probe).to_bits() != swapped.sample(probe).to_bits()
        });
        assert!(
            differing,
            "swapping erosion/weirdness must change some sampled value"
        );
    }

    #[test]
    fn overworld_jaggedness_routes_coordinates_by_role() {
        let continents = RoleCoord {
            role: "continents",
            slope: 1.0,
            min: -1.0,
            max: 1.0,
        };
        let erosion = RoleCoord {
            role: "erosion",
            slope: 2.0,
            min: -1.0,
            max: 1.0,
        };
        let weirdness = RoleCoord {
            role: "weirdness",
            slope: 4.0,
            min: -1.0,
            max: 1.0,
        };
        let ridges = RoleCoord {
            role: "ridges",
            slope: 3.0,
            min: -1.0,
            max: 1.0,
        };
        let spline = overworld_jaggedness(continents, erosion, weirdness, ridges, false);

        // Nesting: root is continents; erosion sits directly under it; the
        // ridge splines sit under erosion (depth 2); the weirdness splines sit
        // under ridges (depth 3). This is the *deepest* of the three
        // overworld spline trees — the offset tree's weirdness-equivalent
        // nesting never reaches depth 3, so the depth sweep guards a class of
        // swap the offset test cannot see.
        let mut roles = Vec::new();
        collect_role_depths(&spline, 0, &mut roles);
        assert_eq!(roles.first(), Some(&("continents", 0)));
        let erosion_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "erosion").collect();
        let weirdness_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "weirdness").collect();
        let ridge_entries: Vec<_> = roles.iter().filter(|(r, _)| *r == "ridges").collect();
        assert!(!erosion_entries.is_empty(), "erosion splines present");
        assert!(!weirdness_entries.is_empty(), "weirdness splines present");
        assert!(!ridge_entries.is_empty(), "ridge splines present");
        for (_, depth) in &erosion_entries {
            assert_eq!(
                *depth, 1,
                "erosion coordinate must be a direct child of continents"
            );
        }
        for (_, depth) in &ridge_entries {
            assert_eq!(
                *depth, 2,
                "ridge coordinate must be nested under erosion, not at depth {depth}"
            );
        }
        for (_, depth) in &weirdness_entries {
            assert_eq!(
                *depth, 3,
                "weirdness coordinate must be nested under ridges, not at depth {depth}"
            );
        }

        // The sampled output of the whole jaggedness tree is a constant 0.0
        // for every single-coordinate probe (the root extends its first
        // constant-0 knot), so no sample exists that distinguishes a swap —
        // the structural depth sweep above is the routing guarantee.
        assert_eq!(spline.sample(-0.5).to_bits(), 0.0f32.to_bits());
    }

    // ------------------------------------------------------------------
    // Golden fixture (tools/rivet-oracle/fixtures/data-worldgen/)
    //
    // `TerrainProviderProbe` built the overworld offset/factor/jaggedness
    // splines over the identity `BoundedFloatFunction` (unbounded) in the
    // pinned Paper 26.2 runtime and emitted each spline's min/max, a sample
    // sweep, and Paper's `parityString()` output as hex-float. These tests
    // assert the Rust port against that fixture bit-exactly, exercising the
    // full spline tree (all four coordinate types share the identity input, so
    // a sample at coordinate `c` drives every nested spline at `c`).
    // ------------------------------------------------------------------

    const GOLDENS: &str = include_str!(
        "../../../../../tools/rivet-oracle/fixtures/data-worldgen/terrain-provider-goldens.json"
    );

    /// Parse a Java `Double.toHexString` value (the probe's `hexF` output) back
    /// to the exact `f32` it denotes. See crates/rivet-util/tests/cubic_spline.rs.
    fn hex_f32(s: &str) -> f32 {
        let s = s.trim();
        if s == "NaN" {
            return f32::NAN;
        }
        if s == "Infinity" {
            return f32::INFINITY;
        }
        if s == "-Infinity" {
            return f32::NEG_INFINITY;
        }
        let (neg, rest) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s),
        };
        let rest = rest.strip_prefix("0x").expect("hex float");
        let (mantissa, exp) = rest.split_once('p').expect("hex exponent");
        let exp: i32 = exp.parse().expect("decimal exponent");
        let (int_part, frac_part) = match mantissa.split_once('.') {
            Some((i, f)) => (i, f),
            None => (mantissa, ""),
        };
        let int_val = u64::from_str_radix(int_part, 16).expect("hex int");
        let mut frac_val = 0u64;
        let mut frac_bits = 0u32;
        for ch in frac_part.chars() {
            frac_val = (frac_val << 4) | ch.to_digit(16).expect("hex frac") as u64;
            frac_bits += 4;
        }
        let mut v = int_val as f64;
        if frac_bits > 0 {
            v += frac_val as f64 / (1u64 << frac_bits) as f64;
        }
        v *= 2f64.powi(exp);
        let v = if neg { -v } else { v };
        v as f32
    }

    /// Normalize the coordinate token of a parity string so the fixture's Java
    /// output (anonymous `BoundedFloatFunction$1`) is asserted byte-for-byte
    /// against Rust's (`Identity`). Same contract as crates/rivet-util/tests/
    /// cubic_spline.rs; the probe's `parityOf` already strips the per-JVM
    /// identity hash.
    fn normalize_coordinate(s: &str) -> String {
        let s = s.replace("coordinate=Identity", "coordinate=<coordinate>");
        let marker = "coordinate=net.minecraft.util.BoundedFloatFunction$";
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(start) = rest.find(marker) {
            out.push_str(&rest[..start]);
            out.push_str("coordinate=<coordinate>");
            rest = &rest[start + marker.len()..];
            rest = rest
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches('@')
                .trim_start_matches(|c: char| c.is_ascii_hexdigit());
        }
        out.push_str(rest);
        out
    }

    /// One `TerrainProviderProbe` spline case, parsed from the fixture.
    struct GoldenCase {
        min: f32,
        max: f32,
        samples: Vec<(f32, f32)>, // (coordinate, sample)
        parity: String,           // Paper's parity string (coordinate normalized)
    }

    fn golden_case(name: &str) -> GoldenCase {
        let root: serde_json::Value =
            serde_json::from_str(GOLDENS).expect("parse terrain-provider-goldens.json");
        let cases = root["cases"].as_array().expect("cases array");
        let case = cases
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("no golden case {name}"));
        GoldenCase {
            min: hex_f32(case["min"].as_str().unwrap()),
            max: hex_f32(case["max"].as_str().unwrap()),
            samples: case["samples"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| {
                    (
                        hex_f32(o["coordinate"].as_str().unwrap()),
                        hex_f32(o["sample"].as_str().unwrap()),
                    )
                })
                .collect(),
            parity: normalize_coordinate(case["parity"].as_str().unwrap()),
        }
    }

    fn assert_parity(spline: &CubicSpline<Identity>, want: &str) {
        assert_eq!(
            normalize_coordinate(&spline.parity_string()),
            want,
            "parity"
        );
    }

    /// Assert a built spline matches its golden case bit-exactly (min/max,
    /// sample sweep, and the full nested parity string).
    fn assert_golden(name: &str, spline: &CubicSpline<Identity>) {
        let g = golden_case(name);
        assert_eq!(spline.min_value().to_bits(), g.min.to_bits(), "{name} min");
        assert_eq!(spline.max_value().to_bits(), g.max.to_bits(), "{name} max");
        for (c, want) in &g.samples {
            assert_eq!(
                spline.sample(*c).to_bits(),
                want.to_bits(),
                "{name} sample({c})"
            );
        }
        assert_parity(spline, &g.parity);
    }

    #[test]
    fn golden_overworld_offset_plain() {
        assert_golden(
            "offset_plain",
            &overworld_offset(Identity, Identity, Identity, false),
        );
    }

    #[test]
    fn golden_overworld_offset_amplified() {
        assert_golden(
            "offset_amplified",
            &overworld_offset(Identity, Identity, Identity, true),
        );
    }

    #[test]
    fn golden_overworld_factor_plain() {
        assert_golden(
            "factor_plain",
            &overworld_factor(Identity, Identity, Identity, Identity, false),
        );
    }

    #[test]
    fn golden_overworld_factor_amplified() {
        assert_golden(
            "factor_amplified",
            &overworld_factor(Identity, Identity, Identity, Identity, true),
        );
    }

    #[test]
    fn golden_overworld_jaggedness_plain() {
        assert_golden(
            "jaggedness_plain",
            &overworld_jaggedness(Identity, Identity, Identity, Identity, false),
        );
    }

    #[test]
    fn golden_overworld_jaggedness_amplified() {
        assert_golden(
            "jaggedness_amplified",
            &overworld_jaggedness(Identity, Identity, Identity, Identity, true),
        );
    }

    #[test]
    fn golden_peaks_and_valleys_sweep() {
        let root: serde_json::Value =
            serde_json::from_str(GOLDENS).expect("parse terrain-provider-goldens.json");
        for o in root["peaks_and_valleys"].as_array().unwrap() {
            let weirdness = hex_f32(o["weirdness"].as_str().unwrap());
            let want = hex_f32(o["value"].as_str().unwrap());
            assert_eq!(
                peaks_and_valleys(weirdness).to_bits(),
                want.to_bits(),
                "peaks_and_valleys({weirdness})"
            );
        }
    }
}

//! Port of `net.minecraft.world.level.levelgen.synth.BlendedNoise` (class,
//! 26.2).
//!
//! The old nether-style terrain noise: three `PerlinNoise` stacks (min-limit /
//! max-limit / main) folded in `compute`. The `createUnseeded`/test-visible
//! constructors use `IntStream.rangeClosed(-15, 0)` / `rangeClosed(-7, 0)`
//! via `PerlinNoise.createLegacyForBlendedNoise`, and the value surface is
//! exposed directly (`compute`/`min_value`/`max_value`).
//!
//! Deferred seam: Java `BlendedNoise implements DensityFunction.SimpleFunction`
//! and carries a `CODEC` (`KeyDispatchDataCodec`). The `DensityFunction` layer
//! and its dispatch codecs are NOT ported here — RivetTodo(#177): `compute`
//! takes the raw block coordinates instead of a `FunctionContext`, and `codec`
//! is omitted. The `compute` loop's octave iteration and per-octave
//! `noise(..., mainSmear * pow, mainY * pow)` folding are exact.

use rivet_util::mth;
use rivet_util::random::XoroshiroRandomSource;

use crate::levelgen::synth::perlin_noise::PerlinNoise;

/// `BlendedNoise.SCALE_RANGE` — the `Codec.doubleRange(0.001, 1000.0)` bounds
/// (kept for the deferred codec seam).
#[allow(dead_code)]
const SCALE_RANGE: (f64, f64) = (0.001, 1000.0);
/// `BlendedNoise`'s `smear_scale_multiplier` range — `Codec.doubleRange(1.0,
/// 8.0)`.
#[allow(dead_code)]
const SMEAR_SCALE_MULTIPLIER_RANGE: (f64, f64) = (1.0, 8.0);

/// `net.minecraft.world.level.levelgen.synth.BlendedNoise`.
pub struct BlendedNoise {
    min_limit_noise: PerlinNoise,
    max_limit_noise: PerlinNoise,
    main_noise: PerlinNoise,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
    max_value: f64,
    /// `xzScale` — the codec field.
    #[allow(dead_code)]
    xz_scale: f64,
    /// `yScale` — the codec field.
    #[allow(dead_code)]
    y_scale: f64,
}

impl BlendedNoise {
    /// `createUnseeded(double xzScale, double yScale, double xzFactor, double
    /// yFactor, double smearScaleMultiplier)` — seeds the three stacks from a
    /// fresh `XoroshiroRandomSource(0L)`.
    pub fn create_unseeded(
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        Self::new(
            &mut XoroshiroRandomSource::new(0),
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        )
    }

    /// The test-visible `BlendedNoise(RandomSource, ...)` constructor — builds
    /// the min/max/main stacks from `rangeClosed(-15, 0)` / `rangeClosed(-7,
    /// 0)` via the legacy path.
    pub fn new(
        random: &mut impl rivet_util::random::RandomSource,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        Self::new_with_stacks(
            PerlinNoise::create_legacy_for_blended_noise(random, &(-15..=0).collect::<Vec<i32>>()),
            PerlinNoise::create_legacy_for_blended_noise(random, &(-15..=0).collect::<Vec<i32>>()),
            PerlinNoise::create_legacy_for_blended_noise(random, &(-7..=0).collect::<Vec<i32>>()),
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        )
    }

    /// The private `BlendedNoise(PerlinNoise, PerlinNoise, PerlinNoise,
    /// double, ...)` constructor. The arity mirrors Java's 8-argument private
    /// constructor exactly.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_stacks(
        min_limit_noise: PerlinNoise,
        max_limit_noise: PerlinNoise,
        main_noise: PerlinNoise,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        let xz_multiplier = 684.412 * xz_scale;
        let y_multiplier = 684.412 * y_scale;
        let max_value = min_limit_noise.max_broken_value(y_multiplier);
        BlendedNoise {
            min_limit_noise,
            max_limit_noise,
            main_noise,
            xz_multiplier,
            y_multiplier,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
            max_value,
            xz_scale,
            y_scale,
        }
    }

    /// `compute(DensityFunction.FunctionContext)` — with the `FunctionContext`
    /// seam dropped, takes the block coordinates directly.
    ///
    /// RivetTodo(#177): Java takes a `DensityFunction.FunctionContext` and
    /// reads `blockX`/`blockY`/`blockZ`; the context type is part of the
    /// unported `DensityFunction` layer, so this port takes the ints it would
    /// read.
    pub fn compute(&self, block_x: i32, block_y: i32, block_z: i32) -> f64 {
        let limit_x = block_x as f64 * self.xz_multiplier;
        let limit_y = block_y as f64 * self.y_multiplier;
        let limit_z = block_z as f64 * self.xz_multiplier;
        let main_x = limit_x / self.xz_factor;
        let main_y = limit_y / self.y_factor;
        let main_z = limit_z / self.xz_factor;
        let limit_smear = self.y_multiplier * self.smear_scale_multiplier;
        let main_smear = limit_smear / self.y_factor;
        let mut blend_min = 0.0;
        let mut blend_max = 0.0;
        let mut main_noise_value = 0.0;
        let mut pow = 1.0;

        for i in 0..8 {
            if let Some(noise) = self.main_noise.get_octave_noise(i) {
                main_noise_value += noise.noise_yscaled(
                    PerlinNoise::wrap(main_x * pow),
                    PerlinNoise::wrap(main_y * pow),
                    PerlinNoise::wrap(main_z * pow),
                    main_smear * pow,
                    main_y * pow,
                ) / pow;
            }
            pow /= 2.0;
        }

        let factor = (main_noise_value / 10.0 + 1.0) / 2.0;
        let is_max = factor >= 1.0;
        let is_min = factor <= 0.0;
        pow = 1.0;

        for i in 0..16 {
            let wx = PerlinNoise::wrap(limit_x * pow);
            let wy = PerlinNoise::wrap(limit_y * pow);
            let wz = PerlinNoise::wrap(limit_z * pow);
            let y_scale_pow = limit_smear * pow;
            if !is_max && let Some(min_noise) = self.min_limit_noise.get_octave_noise(i) {
                blend_min += min_noise.noise_yscaled(wx, wy, wz, y_scale_pow, limit_y * pow) / pow;
            }
            if !is_min && let Some(max_noise) = self.max_limit_noise.get_octave_noise(i) {
                blend_max += max_noise.noise_yscaled(wx, wy, wz, y_scale_pow, limit_y * pow) / pow;
            }
            pow /= 2.0;
        }

        mth::clamped_lerp(factor, blend_min / 512.0, blend_max / 512.0) / 128.0
    }

    /// `minValue()` — `-this.maxValue()`.
    pub fn min_value(&self) -> f64 {
        -self.max_value
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max_value
    }
}

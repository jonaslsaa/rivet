//! Port of `net.minecraft.world.level.levelgen.synth.NormalNoise` (class,
//! 26.2).
//!
//! Two `PerlinNoise` octave stacks (first/second) evaluated in quadrature:
//! `getValue` scales the input by `INPUT_FACTOR = 1.0181268882175227` for the
//! second stack and combines both with the deviation-normalized `valueFactor`.
//!
//! The DFU `NoiseParameters` record is ported with its value surface
//! (`firstOctave`/`amplitudes`); both the `DIRECT_CODEC` and the
//! holder-backed `NoiseParameters.CODEC` (`RegistryFileCodec` over
//! `Registries.NOISE`) are deferred — RivetTodo(#177) — since the noise
//! registry and holder dispatch live in the later worldgen waves.

use rivet_util::random::RandomSource;

use crate::levelgen::synth::perlin_noise::PerlinNoise;

/// `NormalNoise.INPUT_FACTOR`.
const INPUT_FACTOR: f64 = 1.0181268882175227;
/// The literal `0.16666666666666666` used in the constructor's
/// `valueFactor = 0.16666666666666666 / expectedDeviation(...)` (Java writes
/// the decimal directly).
const VALUE_FACTOR_NUMERATOR: f64 = 0.16666666666666666;

/// `net.minecraft.world.level.levelgen.synth.NormalNoise`.
pub struct NormalNoise {
    value_factor: f64,
    first: PerlinNoise,
    second: PerlinNoise,
    max_value: f64,
    parameters: NoiseParameters,
}

impl NormalNoise {
    /// `create(RandomSource, int firstOctave, double... amplitudes)`.
    pub fn create_amplitudes(
        random: &mut impl RandomSource,
        first_octave: i32,
        amplitudes: &[f64],
    ) -> Self {
        Self::new(
            random,
            NoiseParameters::new(first_octave, amplitudes.to_vec()),
            true,
        )
    }

    /// `create(RandomSource, NoiseParameters)`.
    pub fn create(random: &mut impl RandomSource, parameters: NoiseParameters) -> Self {
        Self::new(random, parameters, true)
    }

    /// `createLegacyNetherBiome(RandomSource, NoiseParameters)` — the
    /// `useNewInitialization == false` construction used by the legacy nether
    /// biome sampler.
    pub fn create_legacy_nether_biome(
        random: &mut impl RandomSource,
        parameters: NoiseParameters,
    ) -> Self {
        Self::new(random, parameters, false)
    }

    /// The private `NormalNoise(RandomSource, NoiseParameters, boolean
    /// useNewInitialization)` constructor.
    fn new(
        random: &mut impl RandomSource,
        parameters: NoiseParameters,
        use_new_initialization: bool,
    ) -> Self {
        let first_octave = parameters.first_octave;
        let amplitudes = parameters.amplitudes.clone();
        if use_new_initialization {
            let first = PerlinNoise::create(random, first_octave, amplitudes.clone());
            let second = PerlinNoise::create(random, first_octave, amplitudes.clone());
            Self::assemble(first, second, parameters, amplitudes)
        } else {
            let first = PerlinNoise::create_legacy_for_legacy_nether_biome(
                random,
                first_octave,
                amplitudes.clone(),
            );
            let second = PerlinNoise::create_legacy_for_legacy_nether_biome(
                random,
                first_octave,
                amplitudes.clone(),
            );
            Self::assemble(first, second, parameters, amplitudes)
        }
    }

    /// The tail of the constructor (after both `PerlinNoise` stacks are built):
    /// compute the non-zero octave span, the deviation-scaled `valueFactor`,
    /// and `maxValue`.
    fn assemble(
        first: PerlinNoise,
        second: PerlinNoise,
        parameters: NoiseParameters,
        amplitudes: Vec<f64>,
    ) -> Self {
        let mut min_octave = i32::MAX;
        let mut max_octave = i32::MIN;
        for (i, &amplitude) in amplitudes.iter().enumerate() {
            if amplitude != 0.0 {
                min_octave = min_octave.min(i as i32);
                max_octave = max_octave.max(i as i32);
            }
        }
        let value_factor =
            VALUE_FACTOR_NUMERATOR / Self::expected_deviation(max_octave - min_octave);
        let max_value = (first.max_value() + second.max_value()) * value_factor;
        NormalNoise {
            value_factor,
            first,
            second,
            max_value,
            parameters,
        }
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    /// `expectedDeviation(int octaveSpan)`.
    fn expected_deviation(octave_span: i32) -> f64 {
        0.1 * (1.0 + 1.0 / (octave_span as f64 + 1.0))
    }

    /// `getValue(double x, double y, double z)`.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let x2 = x * INPUT_FACTOR;
        let y2 = y * INPUT_FACTOR;
        let z2 = z * INPUT_FACTOR;
        (self.first.get_value(x, y, z) + self.second.get_value(x2, y2, z2)) * self.value_factor
    }

    /// `parameters()`.
    pub fn parameters(&self) -> &NoiseParameters {
        &self.parameters
    }
}

/// `NormalNoise.NoiseParameters` — the value record describing the octave
/// stack.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseParameters {
    /// `firstOctave`.
    pub first_octave: i32,
    /// `amplitudes`.
    pub amplitudes: Vec<f64>,
}

impl NoiseParameters {
    /// The record constructor (`(int, DoubleList)`).
    pub fn new(first_octave: i32, amplitudes: Vec<f64>) -> Self {
        NoiseParameters {
            first_octave,
            amplitudes,
        }
    }

    /// `new NoiseParameters(int firstOctave, double firstAmplitude,
    /// double... amplitudes)` — prepends `firstAmplitude` to the tail.
    pub fn new_with_first(first_octave: i32, first_amplitude: f64, amplitudes: &[f64]) -> Self {
        let mut list = amplitudes.to_vec();
        list.insert(0, first_amplitude);
        NoiseParameters {
            first_octave,
            amplitudes: list,
        }
    }
}

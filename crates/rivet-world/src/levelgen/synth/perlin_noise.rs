//! Port of `net.minecraft.world.level.levelgen.synth.PerlinNoise` (class,
//! 26.2).
//!
//! A stack of `ImprovedNoise` octaves with per-octave amplitudes. The
//! initialization path matters for exact RNG consumption:
//! - `useNewInitialization == true` (`PerlinNoise.create`): each non-zero
//!   amplitude octave seeds from `random.forkPositional().fromHashOf(
//!   "octave_" + octave)` — the octave index is `firstOctave + i`.
//! - legacy (`createLegacyForBlendedNoise` / `createLegacyForLegacyNetherBiome`):
//!   octaves consume the source sequentially, with `skipOctave` =
//!   `consumeCount(262)` for zero amplitudes.
//!
//! `getValue` re-folds octaves with `wrap(x * factor)` (`PerlinNoise.wrap` —
//! the `ROUND_OFF = 33554432` wrapping) and the input/value factor doubling /
//! halving, exactly as Java.

use rivet_util::mth;
use rivet_util::random::{PositionalRandomFactory, RandomSource};

use crate::levelgen::synth::improved_noise::ImprovedNoise;

/// `PerlinNoise.ROUND_OFF` — the `wrap` modulus (2^25).
const ROUND_OFF: f64 = 3.3554432E7;

/// `net.minecraft.world.level.levelgen.synth.PerlinNoise`.
pub struct PerlinNoise {
    noise_levels: Vec<Option<ImprovedNoise>>,
    first_octave: i32,
    amplitudes: Vec<f64>,
    lowest_freq_value_factor: f64,
    lowest_freq_input_factor: f64,
    max_value: f64,
}

impl PerlinNoise {
    /// `create(RandomSource, IntStream octaves)` — the octave-set form. The
    /// octave set is sorted/deduplicated (Java `IntRBTreeSet`), and amplitudes
    /// are `1.0` for each octave in the set, else `0.0`.
    pub fn create_octave_set(random: &mut impl RandomSource, octave_set: &[i32]) -> Self {
        let (sorted, low_freq_octaves, _high_freq_octaves, octaves) =
            super::octave_span(octave_set);
        let mut amplitudes = vec![0.0f64; octaves as usize];
        for &octave in &sorted {
            amplitudes[octave.wrapping_add(low_freq_octaves) as usize] = 1.0;
        }
        Self::new(random, low_freq_octaves.wrapping_neg(), amplitudes, true)
    }

    /// `create(RandomSource, int firstOctave, DoubleList amplitudes)` — the
    /// explicit-amplitude form.
    pub fn create(random: &mut impl RandomSource, first_octave: i32, amplitudes: Vec<f64>) -> Self {
        Self::new(random, first_octave, amplitudes, true)
    }

    /// `createLegacyForBlendedNoise(RandomSource, IntStream octaves)` — the
    /// legacy octave-set form (no `forkPositional`; sequential consumption).
    pub fn create_legacy_for_blended_noise(
        random: &mut impl RandomSource,
        octave_set: &[i32],
    ) -> Self {
        let (sorted, low_freq_octaves, _high_freq_octaves, octaves) =
            super::octave_span(octave_set);
        let mut amplitudes = vec![0.0f64; octaves as usize];
        for &octave in &sorted {
            amplitudes[octave.wrapping_add(low_freq_octaves) as usize] = 1.0;
        }
        Self::new(random, low_freq_octaves.wrapping_neg(), amplitudes, false)
    }

    /// `createLegacyForLegacyNetherBiome(RandomSource, int firstOctave,
    /// DoubleList amplitudes)`.
    pub fn create_legacy_for_legacy_nether_biome(
        random: &mut impl RandomSource,
        first_octave: i32,
        amplitudes: Vec<f64>,
    ) -> Self {
        Self::new(random, first_octave, amplitudes, false)
    }

    /// The private `PerlinNoise(RandomSource, Pair<Integer, DoubleList>,
    /// boolean useNewInitialization)` constructor.
    fn new(
        random: &mut impl RandomSource,
        first_octave: i32,
        amplitudes: Vec<f64>,
        use_new_initialization: bool,
    ) -> Self {
        let octaves = amplitudes.len() as i32;
        let zero_octave_index = first_octave.wrapping_neg();
        let mut noise_levels: Vec<Option<ImprovedNoise>> =
            (0..octaves as usize).map(|_| None).collect();
        if use_new_initialization {
            let positional = random.fork_positional();
            for i in 0..octaves {
                if amplitudes[i as usize] != 0.0 {
                    let octave = first_octave.wrapping_add(i);
                    let mut octave_random = positional.from_hash_of(&format!("octave_{octave}"));
                    noise_levels[i as usize] = Some(ImprovedNoise::new(&mut octave_random));
                }
            }
        } else {
            let zero_octave = ImprovedNoise::new(random);
            if zero_octave_index >= 0 && zero_octave_index < octaves {
                let zero_octave_amplitude = amplitudes[zero_octave_index as usize];
                if zero_octave_amplitude != 0.0 {
                    noise_levels[zero_octave_index as usize] = Some(zero_octave);
                }
            }
            for i in (0..=zero_octave_index.wrapping_sub(1)).rev() {
                if i < octaves {
                    let amplitude = amplitudes[i as usize];
                    if amplitude != 0.0 {
                        noise_levels[i as usize] = Some(ImprovedNoise::new(random));
                    } else {
                        Self::skip_octave(random);
                    }
                } else {
                    Self::skip_octave(random);
                }
            }
            let non_null = noise_levels.iter().filter(|n| n.is_some()).count();
            let non_zero = amplitudes.iter().filter(|&&a| a != 0.0).count();
            if non_null != non_zero {
                panic!(
                    "Failed to create correct number of noise levels for given non-zero amplitudes"
                );
            }
            if zero_octave_index < octaves - 1 {
                panic!("Positive octaves are temporarily disabled");
            }
        }

        let lowest_freq_input_factor = (2.0f64).powi(-zero_octave_index);
        let lowest_freq_value_factor = (2.0f64).powi(octaves - 1) / ((2.0f64).powi(octaves) - 1.0);
        let max_value = {
            let mut value = 0.0;
            let mut value_factor = lowest_freq_value_factor;
            for i in 0..octaves as usize {
                if noise_levels[i].is_some() {
                    value += amplitudes[i] * 2.0 * value_factor;
                }
                value_factor /= 2.0;
            }
            value
        };
        PerlinNoise {
            noise_levels,
            first_octave,
            amplitudes,
            lowest_freq_value_factor,
            lowest_freq_input_factor,
            max_value,
        }
    }

    /// `skipOctave(RandomSource)` — `consumeCount(262)`.
    fn skip_octave(random: &mut impl RandomSource) {
        random.consume_count(262);
    }

    /// `maxValue()`.
    pub fn max_value(&self) -> f64 {
        self.max_value
    }

    /// `getValue(double x, double y, double z)`.
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        self.get_value_yscaled(x, y, z, 0.0, 0.0)
    }

    /// `getValue(double x, double y, double z, double yScale, double yFudge)` —
    /// the deprecated overload.
    pub fn get_value_yscaled(&self, x: f64, y: f64, z: f64, y_scale: f64, y_fudge: f64) -> f64 {
        let mut value = 0.0;
        let mut factor = self.lowest_freq_input_factor;
        let mut value_factor = self.lowest_freq_value_factor;
        for (i, noise) in self.noise_levels.iter().enumerate() {
            if let Some(noise) = noise {
                let noise_val = noise.noise_yscaled(
                    Self::wrap(x * factor),
                    Self::wrap(y * factor),
                    Self::wrap(z * factor),
                    y_scale * factor,
                    y_fudge * factor,
                );
                value += self.amplitudes[i] * noise_val * value_factor;
            }
            factor *= 2.0;
            value_factor /= 2.0;
        }
        value
    }

    /// `maxBrokenValue(double yScale)` — `edgeValue(yScale + 2.0)`.
    pub fn max_broken_value(&self, y_scale: f64) -> f64 {
        self.edge_value(y_scale + 2.0)
    }

    /// `edgeValue(double noiseValue)` — the value assuming every octave
    /// returns `noiseValue` (the `maxValue`/`maxBrokenValue` computation).
    fn edge_value(&self, noise_value: f64) -> f64 {
        let mut value = 0.0;
        let mut value_factor = self.lowest_freq_value_factor;
        for (i, noise) in self.noise_levels.iter().enumerate() {
            if noise.is_some() {
                value += self.amplitudes[i] * noise_value * value_factor;
            }
            value_factor /= 2.0;
        }
        value
    }

    /// `getOctaveNoise(int i)` — `this.noiseLevels[this.noiseLevels.length - 1
    /// - i]`.
    pub fn get_octave_noise(&self, i: usize) -> Option<&ImprovedNoise> {
        self.noise_levels
            .get(self.noise_levels.len() - 1 - i)
            .and_then(|n| n.as_ref())
    }

    /// `PerlinNoise.wrap(double x)`.
    pub fn wrap(x: f64) -> f64 {
        x - mth::lfloor(x / ROUND_OFF + 0.5) as f64 * ROUND_OFF
    }

    /// `firstOctave()` — protected in Java.
    pub fn first_octave(&self) -> i32 {
        self.first_octave
    }

    /// `amplitudes()` — protected in Java.
    pub fn amplitudes(&self) -> &[f64] {
        &self.amplitudes
    }
}

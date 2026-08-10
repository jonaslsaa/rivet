//! Port of `net.minecraft.world.level.levelgen.synth.PerlinSimplexNoise`
//! (class, 26.2).
//!
//! A stack of `SimplexNoise` octaves over a sorted, distinct octave set. The
//! constructor preserves the exact RNG consumption:
//! - the zero octave is created first (consuming `SimplexNoise`'s constructor
//!   from the given source), then the higher-frequency octaves in ascending
//!   order each consume a full `SimplexNoise` or `consumeCount(262)`;
//! - if the set has positive octaves, the low-frequency half re-seeds a fresh
//!   `WorldgenRandom(LegacyRandomSource)` from
//!   `zeroOctave.getValue(zeroOctave.xo, zeroOctave.yo, zeroOctave.zo) *
//!   9.223372E18F` (a `float` multiply! — the f32 value is widened back to
//!   f64 for the `long` seed), then consumes octaves in descending order.
//!
//! `getValue(x, y, useNoiseStart)` re-folds each octave's contribution with the
//! per-level input factor doubling and value factor halving, exactly as Java.

use rivet_util::random::{LegacyRandomSource, RandomSource};
use rivet_util::worldgen_random::WorldgenRandom;

use crate::levelgen::synth::simplex_noise::SimplexNoise;

/// `net.minecraft.world.level.levelgen.synth.PerlinSimplexNoise`.
pub struct PerlinSimplexNoise {
    noise_levels: Vec<Option<SimplexNoise>>,
    highest_freq_value_factor: f64,
    highest_freq_input_factor: f64,
}

impl PerlinSimplexNoise {
    /// `new PerlinSimplexNoise(RandomSource, List<Integer> octaveSet)`.
    ///
    /// `octave_set` is sorted and deduplicated into the Java `IntRBTreeSet`
    /// order (ascending, distinct) before the octave math runs.
    pub fn new(random: &mut impl RandomSource, octave_set: &[i32]) -> Self {
        let (octaves_sorted, _low_freq_octaves, high_freq_octaves, octaves) =
            super::octave_span(octave_set);

        let mut noise_levels: Vec<Option<SimplexNoise>> =
            (0..octaves as usize).map(|_| None).collect();
        let zero_octave = SimplexNoise::new(random);
        let zero_octave_index = high_freq_octaves;
        // The positive-octave seed derives from `zeroOctave` even when the zero
        // octave is not stored in the array (Java keeps the local reference
        // alive). Compute it before moving `zero_octave` into the array.
        let positive_octave_seed = if high_freq_octaves > 0 {
            Some(
                (zero_octave.get_value_3d(zero_octave.xo, zero_octave.yo, zero_octave.zo)
                    * 9.223372e18f32 as f64) as i64,
            )
        } else {
            None
        };
        if zero_octave_index >= 0 && zero_octave_index < octaves && octaves_sorted.contains(&0) {
            noise_levels[zero_octave_index as usize] = Some(zero_octave);
        }

        for i in zero_octave_index.wrapping_add(1)..octaves {
            if i >= 0 && octaves_sorted.contains(&zero_octave_index.wrapping_sub(i)) {
                noise_levels[i as usize] = Some(SimplexNoise::new(random));
            } else {
                random.consume_count(262);
            }
        }

        if high_freq_octaves > 0 {
            let positive_octave_seed = positive_octave_seed.expect("computed above");
            // Java hardcodes `new WorldgenRandom(new LegacyRandomSource(seed))`
            // here. The concrete `LegacyRandomSource` matters: `WorldgenRandom`
            // takes the direct LCG `next(bits)` path only when the wrapped
            // source IS a `LegacyRandomSource` (Java's `instanceof`), which the
            // sealed `AlgorithmRandomSource` wrapper would defeat.
            let mut high_freq_random =
                WorldgenRandom::new(LegacyRandomSource::new(positive_octave_seed));

            for i in (0..=zero_octave_index.wrapping_sub(1)).rev() {
                if i < octaves && octaves_sorted.contains(&zero_octave_index.wrapping_sub(i)) {
                    noise_levels[i as usize] = Some(SimplexNoise::new(&mut high_freq_random));
                } else {
                    high_freq_random.consume_count(262);
                }
            }
        }

        let highest_freq_input_factor = (2.0f64).powi(high_freq_octaves);
        let highest_freq_value_factor = 1.0 / ((2.0f64).powi(octaves) - 1.0);
        PerlinSimplexNoise {
            noise_levels,
            highest_freq_value_factor,
            highest_freq_input_factor,
        }
    }

    /// `getValue(double x, double y, boolean useNoiseStart)`.
    pub fn get_value(&self, x: f64, y: f64, use_noise_start: bool) -> f64 {
        let mut value = 0.0;
        let mut factor = self.highest_freq_input_factor;
        let mut value_factor = self.highest_freq_value_factor;
        for noise_level in &self.noise_levels {
            if let Some(noise) = noise_level {
                value += noise.get_value_2d(
                    x * factor + if use_noise_start { noise.xo } else { 0.0 },
                    y * factor + if use_noise_start { noise.yo } else { 0.0 },
                ) * value_factor;
            }
            factor /= 2.0;
            value_factor *= 2.0;
        }
        value
    }
}

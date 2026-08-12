//! Port of `net.minecraft.data.worldgen.NoiseData`.
//!
//! The noise registry bootstrap: `DEFAULT_SHIFT` plus the declaration-ordered
//! `register` calls that seed every `Registries.NOISE` entry (`#177`). The
//! order is observable (the noise registry's element order), so the calls are
//! listed in the exact Java declaration order.
//!
//! Translation notes:
//! - `@Deprecated DEFAULT_SHIFT` — a `NormalNoise.NoiseParameters(-3, 1.0,
//!   1.0, 1.0, 0.0)` value; `NoiseParameters::new_with_first` prepends the
//!   first amplitude (Java's `(int, double, double...)` constructor). It is a
//!   `LazyLock` static (not `const`) because `NoiseParameters.amplitudes` is a
//!   `Vec` — the crate's `Identifier`-owning convention.
//! - `bootstrap` consumes a `&mut` `BootstrapContext` (`register` mutates the
//!   build state) and drives the two `registerBiomeNoises` groups + the ~55
//!   direct registrations in declaration order.
//! - The `-10 + octaveOffset` / `-8 + octaveOffset` / `-9 + octaveOffset`
//!   arithmetic is over small constants (octaveOffset ∈ {0, -2}) — plain
//!   `+`, range-safe (PORTING.md: wrapping only where overflow is possible).
//! - `context.register(Noises.SHIFT, DEFAULT_SHIFT)` uses the stable-lifecycle
//!   default (`register_default`); the Java source's other calls go through
//!   the private `register(...)` helper, which also uses the stable default.

use super::bootstrap_context::BootstrapContext;
use crate::levelgen::noise::noises;
use crate::levelgen::synth::normal_noise::NoiseParameters;
use rivet_registry::ResourceKey;
use std::sync::LazyLock;

/// `NoiseData.DEFAULT_SHIFT` — `@Deprecated`, `new NoiseParameters(-3, 1.0, 1.0, 1.0, 0.0)`.
pub static DEFAULT_SHIFT: LazyLock<NoiseParameters> =
    LazyLock::new(|| NoiseParameters::new_with_first(-3, 1.0, &[1.0, 1.0, 0.0]));

/// `NoiseData.bootstrap(BootstrapContext<NormalNoise.NoiseParameters>)`.
pub fn bootstrap(context: &mut impl BootstrapContext<NoiseParameters>) {
    register_biome_noises(
        context,
        0,
        &noises::TEMPERATURE,
        &noises::VEGETATION,
        &noises::CONTINENTALNESS,
        &noises::EROSION,
    );
    register_biome_noises(
        context,
        -2,
        &noises::TEMPERATURE_LARGE,
        &noises::VEGETATION_LARGE,
        &noises::CONTINENTALNESS_LARGE,
        &noises::EROSION_LARGE,
    );
    register(context, &noises::TEMPERATURE_NETHER, -7, 1.0, &[1.0]);
    register(context, &noises::VEGETATION_NETHER, -7, 1.0, &[1.0]);
    register(context, &noises::RIDGE, -7, 1.0, &[2.0, 1.0, 0.0, 0.0, 0.0]);
    context.register_default(&noises::SHIFT, (*DEFAULT_SHIFT).clone());
    register(context, &noises::AQUIFER_BARRIER, -3, 1.0, &[]);
    register(
        context,
        &noises::AQUIFER_FLUID_LEVEL_FLOODEDNESS,
        -7,
        1.0,
        &[],
    );
    register(context, &noises::AQUIFER_LAVA, -1, 1.0, &[]);
    register(context, &noises::AQUIFER_FLUID_LEVEL_SPREAD, -5, 1.0, &[]);
    register(context, &noises::PILLAR, -7, 1.0, &[1.0]);
    register(context, &noises::PILLAR_RARENESS, -8, 1.0, &[]);
    register(context, &noises::PILLAR_THICKNESS, -8, 1.0, &[]);
    register(context, &noises::SPAGHETTI_2D, -7, 1.0, &[]);
    register(context, &noises::SPAGHETTI_2D_ELEVATION, -8, 1.0, &[]);
    register(context, &noises::SPAGHETTI_2D_MODULATOR, -11, 1.0, &[]);
    register(context, &noises::SPAGHETTI_2D_THICKNESS, -11, 1.0, &[]);
    register(context, &noises::SPAGHETTI_3D_1, -7, 1.0, &[]);
    register(context, &noises::SPAGHETTI_3D_2, -7, 1.0, &[]);
    register(context, &noises::SPAGHETTI_3D_RARITY, -11, 1.0, &[]);
    register(context, &noises::SPAGHETTI_3D_THICKNESS, -8, 1.0, &[]);
    register(context, &noises::SPAGHETTI_ROUGHNESS, -5, 1.0, &[]);
    register(
        context,
        &noises::SPAGHETTI_ROUGHNESS_MODULATOR,
        -8,
        1.0,
        &[],
    );
    register(context, &noises::CAVE_ENTRANCE, -7, 0.4, &[0.5, 1.0]);
    register(context, &noises::CAVE_LAYER, -8, 1.0, &[]);
    register(
        context,
        &noises::CAVE_CHEESE,
        -8,
        0.5,
        &[1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0],
    );
    register(context, &noises::ORE_VEININESS, -8, 1.0, &[]);
    register(context, &noises::ORE_VEIN_A, -7, 1.0, &[]);
    register(context, &noises::ORE_VEIN_B, -7, 1.0, &[]);
    register(context, &noises::ORE_GAP, -5, 1.0, &[]);
    register(context, &noises::NOODLE, -8, 1.0, &[]);
    register(context, &noises::NOODLE_THICKNESS, -8, 1.0, &[]);
    register(context, &noises::NOODLE_RIDGE_A, -7, 1.0, &[]);
    register(context, &noises::NOODLE_RIDGE_B, -7, 1.0, &[]);
    register(
        context,
        &noises::JAGGED,
        -16,
        1.0,
        &[
            1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
        ],
    );
    register(context, &noises::SURFACE, -6, 1.0, &[1.0, 1.0]);
    register(
        context,
        &noises::SURFACE_SECONDARY,
        -6,
        1.0,
        &[1.0, 0.0, 1.0],
    );
    register(context, &noises::CLAY_BANDS_OFFSET, -8, 1.0, &[]);
    register(context, &noises::BADLANDS_PILLAR, -2, 1.0, &[1.0, 1.0, 1.0]);
    register(context, &noises::BADLANDS_PILLAR_ROOF, -8, 1.0, &[]);
    register(context, &noises::BADLANDS_SURFACE, -6, 1.0, &[1.0, 1.0]);
    register(context, &noises::ICEBERG_PILLAR, -6, 1.0, &[1.0, 1.0, 1.0]);
    register(context, &noises::ICEBERG_PILLAR_ROOF, -3, 1.0, &[]);
    register(context, &noises::ICEBERG_SURFACE, -6, 1.0, &[1.0, 1.0]);
    register(context, &noises::SULFUR_CAVE_GRADIENT, -5, 1.0, &[0.0, 1.0]);
    register(context, &noises::SWAMP, -2, 1.0, &[]);
    register(context, &noises::CALCITE, -9, 1.0, &[1.0, 1.0, 1.0]);
    register(context, &noises::GRAVEL, -8, 1.0, &[1.0, 1.0, 1.0]);
    register(context, &noises::POWDER_SNOW, -6, 1.0, &[1.0, 1.0, 1.0]);
    register(context, &noises::PACKED_ICE, -7, 1.0, &[1.0, 1.0, 1.0]);
    register(context, &noises::ICE, -4, 1.0, &[1.0, 1.0, 1.0]);
    register(
        context,
        &noises::SOUL_SAND_LAYER,
        -8,
        1.0,
        &[1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.013333333333333334],
    );
    register(
        context,
        &noises::GRAVEL_LAYER,
        -8,
        1.0,
        &[1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.013333333333333334],
    );
    register(
        context,
        &noises::PATCH,
        -5,
        1.0,
        &[0.0, 0.0, 0.0, 0.0, 0.013333333333333334],
    );
    register(context, &noises::NETHERRACK, -3, 1.0, &[0.0, 0.0, 0.35]);
    register(context, &noises::NETHER_WART, -3, 1.0, &[0.0, 0.0, 0.9]);
    register(context, &noises::NETHER_STATE_SELECTOR, -4, 1.0, &[]);
}

/// `NoiseData.registerBiomeNoises(context, octaveOffset, temperature, vegetation, continentalness, erosion)`.
fn register_biome_noises(
    context: &mut impl BootstrapContext<NoiseParameters>,
    octave_offset: i32,
    temperature: &ResourceKey<NoiseParameters>,
    vegetation: &ResourceKey<NoiseParameters>,
    continentalness: &ResourceKey<NoiseParameters>,
    erosion: &ResourceKey<NoiseParameters>,
) {
    register(
        context,
        temperature,
        -10 + octave_offset,
        1.5,
        &[0.0, 1.0, 0.0, 0.0, 0.0],
    );
    register(
        context,
        vegetation,
        -8 + octave_offset,
        1.0,
        &[1.0, 0.0, 0.0, 0.0, 0.0],
    );
    register(
        context,
        continentalness,
        -9 + octave_offset,
        1.0,
        &[1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0],
    );
    register(
        context,
        erosion,
        -9 + octave_offset,
        1.0,
        &[1.0, 0.0, 1.0, 1.0],
    );
}

/// `NoiseData.register(context, key, firstOctave, firstAmplitude, amplitudes...)` —
/// `context.register(key, new NoiseParameters(firstOctave, firstAmplitude, amplitudes))`.
fn register(
    context: &mut impl BootstrapContext<NoiseParameters>,
    key: &ResourceKey<NoiseParameters>,
    first_octave: i32,
    first_amplitude: f64,
    amplitudes: &[f64],
) {
    let parameters = NoiseParameters::new_with_first(first_octave, first_amplitude, amplitudes);
    context.register_default(key, parameters);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::worldgen::bootstrap_context::{RecordedRegistration, RecordingContext};
    use rivet_registry::RegistryAccess;
    use rivet_registry::holder::RegistryId;

    /// Drive the full bootstrap into a recording context and return the
    /// recorded registrations.
    fn run_bootstrap() -> Vec<RecordedRegistration<NoiseParameters>> {
        let mut context =
            RecordingContext::<NoiseParameters>::new(RegistryId(7), RegistryAccess::empty());
        bootstrap(&mut context);
        context.registrations().iter().cloned().collect()
    }

    #[test]
    fn default_shift_matches_java_parameters() {
        assert_eq!(DEFAULT_SHIFT.first_octave, -3);
        assert_eq!(DEFAULT_SHIFT.amplitudes, vec![1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn bootstrap_registers_every_noise_key_exactly_once() {
        let regs = run_bootstrap();
        let keys: Vec<String> = regs
            .iter()
            .map(|r| r.key.identifier().to_string())
            .collect();
        let mut seen = std::collections::HashSet::new();
        for k in &keys {
            assert!(seen.insert(k.clone()), "duplicate registration of {k}");
        }
        // The declaration order: the two biome-noise groups (0, then -2),
        // then the direct registrations.
        assert_eq!(keys[0], "minecraft:temperature");
        assert_eq!(keys[1], "minecraft:vegetation");
        assert_eq!(keys[2], "minecraft:continentalness");
        assert_eq!(keys[3], "minecraft:erosion");
        assert_eq!(keys[4], "minecraft:temperature_large");
        assert_eq!(keys[5], "minecraft:vegetation_large");
        assert_eq!(keys[6], "minecraft:continentalness_large");
        assert_eq!(keys[7], "minecraft:erosion_large");
        assert_eq!(keys[8], "minecraft:nether/temperature");
        assert_eq!(keys[9], "minecraft:nether/vegetation");
        assert_eq!(keys[10], "minecraft:ridge");
        assert_eq!(keys[11], "minecraft:offset");
        assert_eq!(keys[12], "minecraft:aquifer_barrier");
        assert_eq!(keys[keys.len() - 1], "minecraft:nether_state_selector");
        // 8 biome + 55 direct registrations.
        assert_eq!(keys.len(), 63);
    }

    #[test]
    fn biome_noise_octave_offsets_shift_the_large_group() {
        let regs = run_bootstrap();
        let find = |id: &str| {
            regs.iter()
                .find(|r| r.key.identifier().to_string() == format!("minecraft:{id}"))
                .unwrap()
                .value
                .first_octave
        };
        assert_eq!(find("temperature"), -10);
        assert_eq!(find("vegetation"), -8);
        assert_eq!(find("continentalness"), -9);
        assert_eq!(find("erosion"), -9);
        assert_eq!(find("temperature_large"), -12);
        assert_eq!(find("vegetation_large"), -10);
        assert_eq!(find("continentalness_large"), -11);
        assert_eq!(find("erosion_large"), -11);
    }

    #[test]
    fn shift_uses_the_deprecated_default_value() {
        let regs = run_bootstrap();
        let shift = regs
            .iter()
            .find(|r| r.key.identifier().to_string() == "minecraft:offset")
            .unwrap();
        assert_eq!(shift.value, *DEFAULT_SHIFT);
    }

    #[test]
    fn spot_check_amplitude_tails() {
        let regs = run_bootstrap();
        let find = |id: &str| {
            regs.iter()
                .find(|r| r.key.identifier().to_string() == format!("minecraft:{id}"))
                .unwrap()
                .value
                .clone()
        };
        // CAVE_CHEESE: first -8, firstAmplitude 0.5, then [1,2,1,2,1,0,2,0].
        assert_eq!(
            find("cave_cheese"),
            NoiseParameters::new_with_first(-8, 0.5, &[1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0])
        );
        // SOUL_SAND_LAYER keeps the exact 0.013333333333333334 tail value.
        assert_eq!(
            find("soul_sand_layer").amplitudes,
            vec![1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.013333333333333334]
        );
        // JAGGED: -16 with fifteen 1.0 tail amplitudes (sixteen 1.0s total
        // after prepending the first amplitude) — matches the runtime capture.
        assert_eq!(
            find("jagged"),
            NoiseParameters::new_with_first(
                -16,
                1.0,
                &[
                    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
                ]
            )
        );
    }

    // ------------------------------------------------------------------
    // Golden fixture (tools/rivet-oracle/fixtures/data-worldgen/)
    //
    // `NoiseDataProbe` ran the pinned Paper 26.2 `NoiseData.bootstrap` into a
    // recording `BootstrapContext` and emitted every registered key, first
    // octave, and amplitude vector in declaration order. This test drives the
    // Rust bootstrap into a `RecordingContext` and asserts the full order and
    // every parameter bit-for-bit against the fixture (the `0.013333...` tails
    // are the same f64 bits on both sides).
    // ------------------------------------------------------------------

    const NOISE_GOLDENS: &str = include_str!(
        "../../../../../tools/rivet-oracle/fixtures/data-worldgen/noise-data-goldens.json"
    );

    #[test]
    fn golden_registration_order_and_parameters_match_paper() {
        let root: serde_json::Value =
            serde_json::from_str(NOISE_GOLDENS).expect("parse noise-data-goldens.json");
        let regs = run_bootstrap();
        let goldens = root["registrations"].as_array().expect("registrations");
        assert_eq!(regs.len(), goldens.len(), "registration count");
        for (i, (reg, g)) in regs.iter().zip(goldens.iter()).enumerate() {
            let key = reg.key.identifier().to_string();
            assert_eq!(key, g["key"].as_str().unwrap(), "key {i}");
            assert_eq!(
                reg.value.first_octave,
                g["firstOctave"].as_i64().unwrap() as i32,
                "firstOctave {key}"
            );
            let want: Vec<f64> = g["amplitudes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_f64().unwrap())
                .collect();
            assert_eq!(reg.value.amplitudes, want, "amplitudes {key}");
        }
    }
}

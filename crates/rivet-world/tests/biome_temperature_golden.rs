//! Bit-exact golden tests for the `biome::biome` temperature/freeze behavior
//! (the `Biome.getTemperature` / `coldEnoughToSnow` / `getPrecipitationAt`
//! arithmetic and the `TemperatureModifier.FROZEN` branch decisions) against
//! the committed Paper-oracle fixture.
//!
//! Fixture provenance: `tools/rivet-oracle/src/java/BiomeTemperatureProbe.java`
//! constructs vanilla `Biome` values on the pinned Paper 26.2 runtime and
//! emits `Float.floatToIntBits` temperature outputs plus `Double.doubleToLongBits`
//! raw noise samples into
//! `tools/rivet-oracle/fixtures/biome-temperature/biome-temperature.json`. Each
//! `assert_eq!` compares the full 32-bit float pattern, so any arithmetic,
//! noise, or promotion drift from Java fails the test.
//!
//! The fixture pins the FROZEN modifier's branch decision two ways:
//! - the raw `FROZEN_TEMPERATURE_NOISE` / `BIOME_INFO_NOISE` samples are
//!   asserted bit-exactly against Rust, so an amplitude/octave drift in either
//!   noise port fails even when it does not flip a sampled branch decision;
//! - the position grid includes coordinates that sit within ~1e-6 of the
//!   branch thresholds (`ice_patches = 0.3 +/- 1.2e-6`, `small = 0.8 +/-
//!   5.4e-5`), so a constant drift in `modify_temperature` — the `* 7.0`
//!   amplitude (a relative change of ~1e-5 flips a decision), the `0.3`/`0.8`
//!   gates, or the `0.2` edge scale — flips a sampled branch decision, which
//!   changes the aggregate `getTemperature` (the FROZEN output is 0.2 when the
//!   pin fires, the base otherwise) and the bit-exact golden fails.
//!
//! The grid also samples high Y values so the FROZEN pin (`0.2`) minus the
//! snow-level drop crosses the `0.15` `warmEnoughToRain` boundary — the
//! fixture straddles it (rain above, snow below), exercising the `>= 0.15`
//! comparison and the FROZEN -> SNOW precipitation path.
//!
//! The fixture is auto-discovered by `rivet-oracle verify`; this test embeds
//! the same JSON via `include_str!` so it cannot silently drift from what the
//! oracle validates.

use rivet_registry::core::BlockPos;
use rivet_world::biome::biome::{
    BIOME_INFO_NOISE, Biome, BiomeBuilder, FROZEN_TEMPERATURE_NOISE, Precipitation,
    TEMPERATURE_NOISE, TemperatureModifier,
};
use rivet_world::biome::biome_special_effects::BiomeSpecialEffectsBuilder;
use rivet_world::biome::{BiomeGenerationSettings, MobSpawnSettings};
use serde_json::Value;

const FIXTURE: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/biome-temperature/biome-temperature.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture JSON parses")
}

/// The pinned Paper commit the fixture was captured against. The probe and the
/// runner default to this same pin; a drifted oracle (regenerated against a
/// different commit) fails this test instead of silently becoming truth.
const PAPER_PIN: &str = "26.2-DEV-main@0a99345";

#[test]
fn fixture_was_captured_against_pinned_paper() {
    let data = fixture();
    assert_eq!(
        data["paper"].as_str().unwrap(),
        PAPER_PIN,
        "fixture must be captured against the pinned Paper commit"
    );
}

/// `Float.floatToIntBits` stored as a JSON number (an int, possibly negative).
fn float_bits(v: &Value) -> u32 {
    v.as_i64().expect("floatToIntBits is a JSON int") as i32 as u32
}

/// `Double.doubleToLongBits` stored as a JSON number (a long, possibly
/// negative).
fn double_bits(v: &Value) -> u64 {
    v.as_i64().expect("doubleToLongBits is a JSON int") as u64
}

/// Rebuild a biome matching the probe's `sampleBiome(name, ...)` construction.
fn build_biome(has_precipitation: bool, temperature: f32, modifier: TemperatureModifier) -> Biome {
    let effects = BiomeSpecialEffectsBuilder::default().water_color(0).build();
    BiomeBuilder::new()
        .has_precipitation(has_precipitation)
        .temperature(temperature)
        .temperature_adjustment(modifier)
        .downfall(0.4)
        .special_effects(effects)
        .mob_spawn_settings(MobSpawnSettings::empty())
        .generation_settings(BiomeGenerationSettings::EMPTY)
        .build()
}

/// The per-(x,z) raw noise the temperature arithmetic consumes.
struct NoiseAt {
    temperature_noise: f64,
    snow_level_v: f32,
    frozen_large: f64,
    frozen_edge: f64,
    frozen_small: f64,
}

fn noise_at(noise: &Value, x: i64, z: i64) -> NoiseAt {
    for entry in noise.as_array().expect("noise is an array") {
        if entry["x"].as_i64() == Some(x) && entry["z"].as_i64() == Some(z) {
            let frozen_large = f64::from_bits(double_bits(&entry["frozenLarge"]));
            let frozen_edge = f64::from_bits(double_bits(&entry["frozenEdge"]));
            let frozen_small = f64::from_bits(double_bits(&entry["frozenSmall"]));
            return NoiseAt {
                temperature_noise: f64::from_bits(double_bits(&entry["temperatureNoise"])),
                snow_level_v: f32::from_bits(float_bits(&entry["snowLevelV"])),
                frozen_large,
                frozen_edge,
                frozen_small,
            };
        }
    }
    panic!("no noise entry for ({x}, {z})");
}

#[test]
fn temperature_outputs_match_paper_exactly() {
    let data = fixture();
    let noise = &data["noise"];
    let biomes = data["biomes"].as_array().expect("biomes is an array");

    for b in biomes {
        let name = b["name"].as_str().unwrap();
        let biome = match name {
            "plain" => build_biome(true, 0.8, TemperatureModifier::None),
            "cold" => build_biome(true, 0.0, TemperatureModifier::None),
            "frozen" => build_biome(true, 0.7, TemperatureModifier::Frozen),
            "arid" => build_biome(false, -1.0, TemperatureModifier::None),
            other => panic!("unknown biome {other}"),
        };
        for s in b["samples"].as_array().expect("samples is an array") {
            let x = s["x"].as_i64().unwrap();
            let y = s["y"].as_i64().unwrap();
            let z = s["z"].as_i64().unwrap();
            let sea_level = s["seaLevel"].as_i64().unwrap() as i32;
            let pos = BlockPos::new(x as i32, y as i32, z as i32);

            // The snow-level noise — `TEMPERATURE_NOISE.getValue(x / 8.0F,
            // z / 8.0F, false)` — pinned against Paper's raw double AND its
            // `(float)(raw * 8.0)` truncation, independent of the aggregate
            // output.
            let n = noise_at(noise, x, z);
            let raw = TEMPERATURE_NOISE.get_value(
                (x as f32 / 8.0) as f64,
                (z as f32 / 8.0) as f64,
                false,
            );
            assert_eq!(
                raw.to_bits(),
                n.temperature_noise.to_bits(),
                "TEMPERATURE_NOISE raw double at ({x},{z})"
            );
            assert_eq!(
                ((raw * 8.0) as f32).to_bits(),
                n.snow_level_v.to_bits(),
                "snow-level v at ({x},{z})"
            );

            // The exact `getTemperature` bits (Java's `floatToIntBits`).
            let got = biome.get_temperature(&pos, sea_level).to_bits();
            let want = float_bits(&s["getTemperature"]);
            assert_eq!(
                got, want,
                "{name} getTemperature at ({x},{y},{z}) sl={sea_level}"
            );

            assert_eq!(
                biome.cold_enough_to_snow(&pos, sea_level),
                s["coldEnoughToSnow"].as_bool().unwrap(),
                "{name} coldEnoughToSnow at ({x},{y},{z})"
            );
            assert_eq!(
                biome.warm_enough_to_rain(&pos, sea_level),
                s["warmEnoughToRain"].as_bool().unwrap(),
                "{name} warmEnoughToRain at ({x},{y},{z})"
            );

            let precip = biome.get_precipitation_at(&pos, sea_level);
            let precip_name = match precip {
                Precipitation::None => "none",
                Precipitation::Rain => "rain",
                Precipitation::Snow => "snow",
            };
            assert_eq!(
                precip_name,
                s["getPrecipitationAt"].as_str().unwrap(),
                "{name} getPrecipitationAt at ({x},{y},{z})"
            );

            // The FROZEN modifier's branch decision, recomputed from Paper's
            // raw noise, must match the probe's flag and the aggregate result.
            if name == "frozen" {
                // Pin the two FROZEN noise statics directly against Paper's
                // raw samples (seed 3456 / 2345) so an amplitude/octave drift
                // in either port fails even when it does not flip a sampled
                // branch decision.
                assert_eq!(
                    FROZEN_TEMPERATURE_NOISE
                        .get_value(x as f64 * 0.05, z as f64 * 0.05, false)
                        .to_bits(),
                    n.frozen_large.to_bits(),
                    "FROZEN_TEMPERATURE_NOISE at ({x},{z})"
                );
                assert_eq!(
                    BIOME_INFO_NOISE
                        .get_value(x as f64 * 0.2, z as f64 * 0.2, false)
                        .to_bits(),
                    n.frozen_edge.to_bits(),
                    "BIOME_INFO_NOISE (edge) at ({x},{z})"
                );
                assert_eq!(
                    BIOME_INFO_NOISE
                        .get_value(x as f64 * 0.09, z as f64 * 0.09, false)
                        .to_bits(),
                    n.frozen_small.to_bits(),
                    "BIOME_INFO_NOISE (small) at ({x},{z})"
                );
                let ice_patches = n.frozen_large * 7.0 + n.frozen_edge;
                let pins = ice_patches < 0.3 && n.frozen_small < 0.8;
                assert_eq!(
                    pins,
                    s["frozenPins"].as_bool().unwrap(),
                    "frozen pin decision at ({x},{z})"
                );
            }
        }
    }
}

#[test]
fn frozen_fixture_covers_all_three_branch_outcomes() {
    // The FROZEN goldens only prove the branch logic if the fixture exercises
    // all three outcomes independently: the pin fires (both sub-checks pass),
    // the outer gate fails (`icePatches >= 0.3`), and the inner gate fails
    // (`icePatches < 0.3` but `smallVariation >= 0.8`).
    let data = fixture();
    let noise = &data["noise"];
    let frozen = data["biomes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "frozen")
        .expect("frozen biome in fixture");

    let mut pin_fires = false;
    let mut outer_fails = false;
    let mut inner_fails = false;
    for entry in noise.as_array().unwrap() {
        let x = entry["x"].as_i64().unwrap();
        let z = entry["z"].as_i64().unwrap();
        let n = noise_at(noise, x, z);
        let ice_patches = n.frozen_large * 7.0 + n.frozen_edge;
        if ice_patches < 0.3 && n.frozen_small < 0.8 {
            pin_fires = true;
        } else if ice_patches >= 0.3 {
            outer_fails = true;
        } else {
            inner_fails = true;
        }
    }
    assert!(
        pin_fires && outer_fails && inner_fails,
        "frozen fixture must cover pin-fires ({pin_fires}), outer-fails ({outer_fails}), inner-fails ({inner_fails})"
    );

    // Sanity: the probe's per-sample `frozenPins` flag is consistent with the
    // raw noise (the golden assert above also recomputes it per sample).
    for s in frozen["samples"].as_array().unwrap() {
        let x = s["x"].as_i64().unwrap();
        let z = s["z"].as_i64().unwrap();
        let n = noise_at(noise, x, z);
        let ice_patches = n.frozen_large * 7.0 + n.frozen_edge;
        let pins = ice_patches < 0.3 && n.frozen_small < 0.8;
        assert_eq!(pins, s["frozenPins"].as_bool().unwrap(), "at ({x},{z})");
    }

    // The fixture must also cross the 0.15 `warmEnoughToRain` boundary on both
    // sides so the `>= 0.15` comparison itself (and the FROZEN -> SNOW path)
    // is exercised: a drift to `> 0.15` or a shifted threshold is otherwise
    // masked by every sample landing on one side. FROZEN reaches the boundary
    // only at high y (the pin 0.2 minus the snow-level drop).
    let mut above_015 = false;
    let mut below_015 = false;
    for s in frozen["samples"].as_array().unwrap() {
        let t = f32::from_bits(float_bits(&s["getTemperature"]));
        if t >= 0.15 {
            above_015 = true;
        } else {
            below_015 = true;
        }
    }
    assert!(
        above_015 && below_015,
        "frozen fixture must straddle the 0.15 warmEnoughToRain boundary (above {above_015}, below {below_015})"
    );

    // ... and the FROZEN -> SNOW precipitation path must actually fire.
    let any_snow = frozen["samples"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["getPrecipitationAt"] == "snow");
    assert!(
        any_snow,
        "frozen fixture must produce at least one SNOW sample"
    );
}

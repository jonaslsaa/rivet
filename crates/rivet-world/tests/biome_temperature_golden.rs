//! Bit-exact golden tests for `biome::biome` temperature/freeze behavior
//! (`Biome.getTemperature` / `coldEnoughToSnow` / `getPrecipitationAt` and the
//! `TemperatureModifier.FROZEN` branch) against the Paper-oracle fixture
//! (`tools/rivet-oracle/fixtures/biome-temperature/`), captured by
//! `tools/rivet-oracle/src/java/BiomeTemperatureProbe.java` on the pinned Paper
//! 26.2 runtime. Every comparison is against the full bit pattern
//! (`Float.floatToIntBits` / `Double.doubleToLongBits`), so any arithmetic,
//! noise, or promotion drift from Java fails.
//!
//! Coverage rationale not evident from the test names:
//! - the position grid sits within ~1e-6 to ~5e-5 of the FROZEN branch
//!   thresholds (the tightest positions are ice_patches = 0.3 + 1.17e-6 and
//!   small = 0.8 + 3.32e-5), so a constant drift in `modify_temperature` (the
//!   `* 7.0` amplitude, the 0.3/0.8 gates, the 0.2 edge scale) flips a sampled
//!   branch decision. A zero-width
//!   operator change (`< 0.3` to `<= 0.3`) is not caught by sampling — no noise
//!   value lands exactly on the threshold — so only the gate *values* are
//!   pinned, not the comparison direction at exact equality.
//! - high-Y samples straddle the 0.15 `warmEnoughToRain` boundary (FROZEN -> SNOW),
//!   and one sample lands exactly on 0.15, pinning the `>= 0.15` direction;
//! - every sample is captured at two `seaLevel` values (63 and 0), so a port
//!   that hardcodes the overworld snow level (80) fails the sl=0 goldens;
//! - the fixture's SHA-256 is asserted against its sibling manifest, so a
//!   focused `cargo test` enforces the same capture invariant as `rivet-oracle`
//!   (the bare/default fixture-hash verification).

use std::collections::HashMap;

use rivet_registry::core::BlockPos;
use rivet_world::biome::biome::{
    BIOME_INFO_NOISE, Biome, BiomeBuilder, FROZEN_TEMPERATURE_NOISE, Precipitation,
    TEMPERATURE_NOISE, TemperatureModifier,
};
use rivet_world::biome::biome_special_effects::BiomeSpecialEffectsBuilder;
use rivet_world::biome::{BiomeGenerationSettings, MobSpawnSettings};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/biome-temperature/biome-temperature.json");
const MANIFEST: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/biome-temperature/manifest.json");

/// The pinned Paper commit the fixture was captured against. The probe and the
/// runner default to this same pin; a drifted oracle (regenerated against a
/// different commit) fails this test instead of silently becoming truth.
const PAPER_PIN: &str = "26.2-DEV-main@0a99345";

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture JSON parses")
}

#[test]
fn fixture_was_captured_against_pinned_paper() {
    let data = fixture();
    assert_eq!(
        data["paper"].as_str().unwrap(),
        PAPER_PIN,
        "fixture must be captured against the pinned Paper commit"
    );

    // The same invariant `rivet-oracle` (bare/default) enforces: the committed
    // fixture must match its manifest's SHA-256 record, so a fixture that was
    // regenerated without re-hashing the manifest (or hand-edited) fails here.
    let manifest: Value = serde_json::from_str(MANIFEST).expect("manifest JSON parses");
    let mut hasher = Sha256::new();
    hasher.update(FIXTURE.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    let captured = &manifest["captured"][0];
    assert_eq!(captured["path"].as_str().unwrap(), "biome-temperature.json");
    assert_eq!(
        captured["sha256"].as_str().unwrap(),
        digest,
        "fixture must match the manifest SHA-256"
    );
    assert_eq!(manifest["paper"].as_str().unwrap(), PAPER_PIN);
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

/// The noise entries are keyed by (x, z) only; build a map so the y- and
/// sea-level-dependent sample loop does not linear-scan the array per sample.
fn noise_map(noise: &Value) -> HashMap<(i64, i64), NoiseAt> {
    noise
        .as_array()
        .expect("noise is an array")
        .iter()
        .map(|entry| {
            let x = entry["x"].as_i64().unwrap();
            let z = entry["z"].as_i64().unwrap();
            let frozen_large = f64::from_bits(double_bits(&entry["frozenLarge"]));
            let frozen_edge = f64::from_bits(double_bits(&entry["frozenEdge"]));
            let frozen_small = f64::from_bits(double_bits(&entry["frozenSmall"]));
            (
                (x, z),
                NoiseAt {
                    temperature_noise: f64::from_bits(double_bits(&entry["temperatureNoise"])),
                    snow_level_v: f32::from_bits(float_bits(&entry["snowLevelV"])),
                    frozen_large,
                    frozen_edge,
                    frozen_small,
                },
            )
        })
        .collect()
}

/// `icePatches = frozenLarge * 7.0 + frozenEdge` (the FROZEN modifier's outer
/// gate). This is the test's copy of the Paper FROZEN spec, used only to
/// classify grid positions for the branch-coverage invariant; it deliberately
/// mirrors the spec (not the Rust `modify_temperature`), so the coverage check
/// cannot silently follow a production drift. Production drift is caught by
/// the aggregate getTemperature goldens.
fn frozen_ice_patches(n: &NoiseAt) -> f64 {
    n.frozen_large * 7.0 + n.frozen_edge
}

/// The FROZEN pin fires when `icePatches < 0.3 && frozenSmall < 0.8` (the
/// Paper spec; see `TemperatureModifier.FROZEN`).
fn frozen_pins(n: &NoiseAt) -> bool {
    frozen_ice_patches(n) < 0.3 && n.frozen_small < 0.8
}

#[test]
fn temperature_outputs_match_paper_exactly() {
    let data = fixture();
    let biomes = data["biomes"].as_array().expect("biomes is an array");
    let noise = &data["noise"];
    let noise_by_pos = noise_map(noise);

    // `noise_map` keys by (x, z), which would silently collapse duplicate grid
    // positions; the fixture must not contain any, or a probe grid edit that
    // adds a duplicate would be invisible to both tests.
    assert_eq!(
        noise.as_array().expect("noise is an array").len(),
        noise_by_pos.len(),
        "noise array must have unique (x, z) entries"
    );

    // The raw noise the arithmetic consumes depends only on (x, z), so pin it
    // once per grid position rather than once per (x, y, z, seaLevel) sample.
    for ((x, z), n) in &noise_by_pos {
        // The snow-level noise — `TEMPERATURE_NOISE.getValue(x / 8.0F,
        // z / 8.0F, false)` — pinned against Paper's raw double AND its
        // `(float)(raw * 8.0)` truncation, independent of the aggregate output.
        let raw =
            TEMPERATURE_NOISE.get_value((*x as f32 / 8.0) as f64, (*z as f32 / 8.0) as f64, false);
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

        // Pin the two FROZEN noise statics directly against Paper's raw
        // samples (seed 3456 / 2345) so an amplitude/octave drift in either
        // port fails even when it does not flip a sampled branch decision.
        assert_eq!(
            FROZEN_TEMPERATURE_NOISE
                .get_value(*x as f64 * 0.05, *z as f64 * 0.05, false)
                .to_bits(),
            n.frozen_large.to_bits(),
            "FROZEN_TEMPERATURE_NOISE at ({x},{z})"
        );
        assert_eq!(
            BIOME_INFO_NOISE
                .get_value(*x as f64 * 0.2, *z as f64 * 0.2, false)
                .to_bits(),
            n.frozen_edge.to_bits(),
            "BIOME_INFO_NOISE (edge) at ({x},{z})"
        );
        assert_eq!(
            BIOME_INFO_NOISE
                .get_value(*x as f64 * 0.09, *z as f64 * 0.09, false)
                .to_bits(),
            n.frozen_small.to_bits(),
            "BIOME_INFO_NOISE (small) at ({x},{z})"
        );
        // The edge scale production FROZEN reads is the `x * 0.2` sample
        // asserted above (`frozen_edge`); a 0.2 -> 0.1 scale drift is caught by
        // the aggregate getTemperature goldens (the grid's gate positions flip
        // their branch decision), not by an extra raw sample.
    }

    // The y- and sea-level-dependent aggregate outputs.
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

            // The exact `getTemperature` bits (Java's `floatToIntBits`).
            let got = biome.get_temperature(&pos, sea_level).to_bits();
            let want = float_bits(&s["getTemperature"]);
            assert_eq!(
                got, want,
                "{name} getTemperature at ({x},{y},{z}) sl={sea_level}"
            );

            // The FROZEN pin is production-observable: below the snow level
            // the adjusted temperature is exactly the 0.2F pin (branch fires)
            // or the base temperature the fixture carries. At y=1 (below both
            // the sl=63 and sl=0 snow levels, 80 and 17) tie the probe's
            // emitted flag to that output without re-deriving the FROZEN
            // formula. The 0.2F pin is fixed by TemperatureModifier.FROZEN; the
            // base comes from the fixture so a regenerated base cannot leave a
            // stale constant here.
            if name == "frozen" && y == 1 {
                let expect = if s["frozenPins"].as_bool().unwrap() {
                    0.2f32.to_bits()
                } else {
                    float_bits(&b["temperature"])
                };
                assert_eq!(
                    got, expect,
                    "frozen pin observable at ({x},{y},{z}) sl={sea_level}"
                );
            }

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
    let noise_by_pos = noise_map(&data["noise"]);
    let frozen = data["biomes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "frozen")
        .expect("frozen biome in fixture");

    let mut pin_fires = false;
    let mut outer_fails = false;
    let mut inner_fails = false;
    for n in noise_by_pos.values() {
        if frozen_pins(n) {
            pin_fires = true;
        } else if frozen_ice_patches(n) >= 0.3 {
            outer_fails = true;
        } else {
            inner_fails = true;
        }
    }
    assert!(
        pin_fires && outer_fails && inner_fails,
        "frozen fixture must cover pin-fires ({pin_fires}), outer-fails ({outer_fails}), inner-fails ({inner_fails})"
    );

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

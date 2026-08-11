//! Exact-bit golden tests for the `levelgen::synth` primitive-noise classes
//! (issue #177) against the committed Paper-oracle fixture.
//!
//! Fixture provenance: `tools/rivet-oracle/src/java/SynthNoiseProbe.java`
//! captures `Double.doubleToLongBits` samples from the pinned Paper 26.2
//! runtime into `tools/rivet-oracle/fixtures/synth/synth-noise.json`. Each
//! `assert_bits` here compares the full 64-bit pattern, so any arithmetic,
//! permutation, RNG, or interpolation drift from Java fails the test.
//!
//! The fixture is auto-discovered by `rivet-oracle verify`; this test embeds
//! the same JSON via `include_str!` so it cannot silently drift from what the
//! oracle validates.

use rivet_util::random::{LegacyRandomSource, RandomSource, XoroshiroRandomSource};
use rivet_world::levelgen::synth::blended_noise::BlendedNoise;
use rivet_world::levelgen::synth::improved_noise::ImprovedNoise;
use rivet_world::levelgen::synth::noise_utils;
use rivet_world::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_world::levelgen::synth::perlin_noise::PerlinNoise;
use rivet_world::levelgen::synth::perlin_simplex_noise::PerlinSimplexNoise;
use rivet_world::levelgen::synth::simplex_noise::SimplexNoise;
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../tools/rivet-oracle/fixtures/synth/synth-noise.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture JSON parses")
}

/// `Double.doubleToLongBits` — the fixture's golden representation.
fn bits(v: f64) -> i64 {
    v.to_bits() as i64
}

/// The probe emits `octaves` as `List.toString()` (e.g. "[-3, -2, -1, 0, 1,
/// 2, 3]"); parse that string into the octave list.
fn parse_octaves(raw: &str) -> Vec<i32> {
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|s| s.trim().parse::<i32>().expect("octave parses"))
        .collect()
}

/// Build a `RandomSource` of the named kind with the given seed, mirroring the
/// probe's `random(source, seed)`. `RandomSource` is `Sized` (no trait
/// objects), so a sealed enum wraps the two concrete sources.
enum Source {
    Xoroshiro(XoroshiroRandomSource),
    Legacy(LegacyRandomSource),
}

fn source(kind: &str, seed: i64) -> Source {
    match kind {
        "xoroshiro" => Source::Xoroshiro(XoroshiroRandomSource::new(seed)),
        "legacy" => Source::Legacy(LegacyRandomSource::new(seed)),
        other => panic!("unknown source {other}"),
    }
}

impl RandomSource for Source {
    type Positional = SourcePositional;

    fn fork(&mut self) -> Self {
        match self {
            Source::Xoroshiro(inner) => Source::Xoroshiro(inner.fork()),
            Source::Legacy(inner) => Source::Legacy(inner.fork()),
        }
    }
    fn fork_positional(&mut self) -> Self::Positional {
        match self {
            Source::Xoroshiro(inner) => SourcePositional::Xoroshiro(inner.fork_positional()),
            Source::Legacy(inner) => SourcePositional::Legacy(inner.fork_positional()),
        }
    }
    fn set_seed(&mut self, seed: i64) {
        match self {
            Source::Xoroshiro(inner) => inner.set_seed(seed),
            Source::Legacy(inner) => inner.set_seed(seed),
        }
    }
    fn next_int(&mut self) -> i32 {
        match self {
            Source::Xoroshiro(inner) => inner.next_int(),
            Source::Legacy(inner) => inner.next_int(),
        }
    }
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        match self {
            Source::Xoroshiro(inner) => inner.next_int_bound(bound),
            Source::Legacy(inner) => inner.next_int_bound(bound),
        }
    }
    fn next_long(&mut self) -> i64 {
        match self {
            Source::Xoroshiro(inner) => inner.next_long(),
            Source::Legacy(inner) => inner.next_long(),
        }
    }
    fn next_boolean(&mut self) -> bool {
        match self {
            Source::Xoroshiro(inner) => inner.next_boolean(),
            Source::Legacy(inner) => inner.next_boolean(),
        }
    }
    fn next_float(&mut self) -> f32 {
        match self {
            Source::Xoroshiro(inner) => inner.next_float(),
            Source::Legacy(inner) => inner.next_float(),
        }
    }
    fn next_double(&mut self) -> f64 {
        match self {
            Source::Xoroshiro(inner) => inner.next_double(),
            Source::Legacy(inner) => inner.next_double(),
        }
    }
    fn next_gaussian(&mut self) -> f64 {
        match self {
            Source::Xoroshiro(inner) => inner.next_gaussian(),
            Source::Legacy(inner) => inner.next_gaussian(),
        }
    }
    fn consume_count(&mut self, rounds: i32) {
        match self {
            Source::Xoroshiro(inner) => inner.consume_count(rounds),
            Source::Legacy(inner) => inner.consume_count(rounds),
        }
    }
}

/// The `Positional` associated type — wraps the two concrete factories. The
/// `PerlinNoise` new-initialization path calls `fork_positional().from_hash_of
/// ("octave_...")`, so this must delegate to the real factories for the golden
/// tests to exercise the exact seeding.
enum SourcePositional {
    Xoroshiro(rivet_util::random::XoroshiroPositionalRandomFactory),
    Legacy(rivet_util::random::LegacyPositionalRandomFactory),
}
impl rivet_util::random::PositionalRandomFactory for SourcePositional {
    type Output = Source;
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output {
        match self {
            SourcePositional::Xoroshiro(inner) => Source::Xoroshiro(inner.at(x, y, z)),
            SourcePositional::Legacy(inner) => Source::Legacy(inner.at(x, y, z)),
        }
    }
    fn from_hash_of(&self, name: &str) -> Self::Output {
        match self {
            SourcePositional::Xoroshiro(inner) => Source::Xoroshiro(inner.from_hash_of(name)),
            SourcePositional::Legacy(inner) => Source::Legacy(inner.from_hash_of(name)),
        }
    }
    fn from_seed(&self, seed: i64) -> Self::Output {
        match self {
            SourcePositional::Xoroshiro(inner) => Source::Xoroshiro(inner.from_seed(seed)),
            SourcePositional::Legacy(inner) => Source::Legacy(inner.from_seed(seed)),
        }
    }
    fn parity_config_string(&self, sb: &mut String) {
        match self {
            SourcePositional::Xoroshiro(inner) => inner.parity_config_string(sb),
            SourcePositional::Legacy(inner) => inner.parity_config_string(sb),
        }
    }
}

fn assert_bits(actual: f64, expected_bits: i64, what: &str) {
    let actual_bits = bits(actual);
    assert_eq!(
        actual_bits, expected_bits,
        "{what}: got bits {actual_bits} ({actual:e}), expected bits {expected_bits}"
    );
}

#[test]
fn simplex_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["simplex"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let mut r = source(kind, seed);
        let noise = SimplexNoise::new(&mut r);
        assert_bits(
            noise.xo,
            entry["xo"].as_i64().unwrap(),
            &format!("{kind}/{seed} xo"),
        );
        assert_bits(
            noise.yo,
            entry["yo"].as_i64().unwrap(),
            &format!("{kind}/{seed} yo"),
        );
        assert_bits(
            noise.zo,
            entry["zo"].as_i64().unwrap(),
            &format!("{kind}/{seed} zo"),
        );
        assert_eq!(
            noise.perm(0) as i64,
            entry["p0"].as_i64().unwrap(),
            "p0 {kind}/{seed}"
        );
        assert_eq!(
            noise.perm(255) as i64,
            entry["p255"].as_i64().unwrap(),
            "p255 {kind}/{seed}"
        );
        for val in entry["vals"].as_array().unwrap() {
            let (x, y) = (val["x"].as_f64().unwrap(), val["y"].as_f64().unwrap());
            let v2 = noise.get_value_2d(x, y);
            let v3 = noise.get_value_3d(x, y, 0.5);
            assert_bits(
                v2,
                val["v2"].as_i64().unwrap(),
                &format!("{kind}/{seed} v2({x},{y})"),
            );
            assert_bits(
                v3,
                val["v3"].as_i64().unwrap(),
                &format!("{kind}/{seed} v3({x},{y})"),
            );
        }
    }
}

#[test]
fn improved_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["improved"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let mut r = source(kind, seed);
        let noise = ImprovedNoise::new(&mut r);
        assert_bits(
            noise.xo,
            entry["xo"].as_i64().unwrap(),
            &format!("{kind}/{seed} xo"),
        );
        assert_bits(
            noise.yo,
            entry["yo"].as_i64().unwrap(),
            &format!("{kind}/{seed} yo"),
        );
        assert_bits(
            noise.zo,
            entry["zo"].as_i64().unwrap(),
            &format!("{kind}/{seed} zo"),
        );
        assert_eq!(
            noise.perm(0) as i64,
            entry["p0"].as_i64().unwrap(),
            "p0 {kind}/{seed}"
        );
        assert_eq!(
            noise.perm(255) as i64,
            entry["p255"].as_i64().unwrap(),
            "p255 {kind}/{seed}"
        );
        for val in entry["vals"].as_array().unwrap() {
            let (x, y, z) = (
                val["x"].as_f64().unwrap(),
                val["y"].as_f64().unwrap(),
                val["z"].as_f64().unwrap(),
            );
            let v = noise.noise(x, y, z);
            let v_deprecated = noise.noise_yscaled(x, y, z, 0.25, 0.5);
            let mut deriv = [0.0f64; 3];
            let v_deriv = noise.noise_with_derivative(x, y, z, &mut deriv);
            assert_bits(
                v,
                val["v"].as_i64().unwrap(),
                &format!("{kind}/{seed} noise"),
            );
            assert_bits(
                v_deprecated,
                val["vDeprecated"].as_i64().unwrap(),
                &format!("{kind}/{seed} noiseDeprecated"),
            );
            assert_bits(
                v_deriv,
                val["vDeriv"].as_i64().unwrap(),
                &format!("{kind}/{seed} noiseDeriv"),
            );
            assert_bits(
                deriv[0],
                val["d0"].as_i64().unwrap(),
                &format!("{kind}/{seed} deriv[0]"),
            );
            assert_bits(
                deriv[1],
                val["d1"].as_i64().unwrap(),
                &format!("{kind}/{seed} deriv[1]"),
            );
            assert_bits(
                deriv[2],
                val["d2"].as_i64().unwrap(),
                &format!("{kind}/{seed} deriv[2]"),
            );
        }
    }
}

#[test]
fn perlin_simplex_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["perlin_simplex"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let octaves = parse_octaves(entry["octaves"].as_str().unwrap());
        let mut r = source(kind, seed);
        let noise = PerlinSimplexNoise::new(&mut r, &octaves);
        for val in entry["vals"].as_array().unwrap() {
            let (x, y) = (val["x"].as_f64().unwrap(), val["y"].as_f64().unwrap());
            let v_true = noise.get_value(x, y, true);
            let v_false = noise.get_value(x, y, false);
            assert_bits(
                v_true,
                val["vTrue"].as_i64().unwrap(),
                &format!("{kind}/{seed} vTrue({x},{y}) octaves {octaves:?}"),
            );
            assert_bits(
                v_false,
                val["vFalse"].as_i64().unwrap(),
                &format!("{kind}/{seed} vFalse({x},{y}) octaves {octaves:?}"),
            );
        }
    }
}

#[test]
fn perlin_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["perlin"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let octaves = parse_octaves(entry["octaves"].as_str().unwrap());
        let mut r = source(kind, seed);
        let noise = PerlinNoise::create_octave_set(&mut r, &octaves);
        assert_bits(
            noise.max_value(),
            entry["maxValue"].as_i64().unwrap(),
            &format!("{kind}/{seed} maxValue octaves {octaves:?}"),
        );
        assert_bits(
            noise.max_broken_value(1.5),
            entry["maxBrokenValue"].as_i64().unwrap(),
            &format!("{kind}/{seed} maxBrokenValue octaves {octaves:?}"),
        );
        for val in entry["vals"].as_array().unwrap() {
            let (x, y, z) = (
                val["x"].as_f64().unwrap(),
                val["y"].as_f64().unwrap(),
                val["z"].as_f64().unwrap(),
            );
            let v = noise.get_value(x, y, z);
            let v_deprecated = noise.get_value_yscaled(x, y, z, 0.25, 0.5);
            assert_bits(
                v,
                val["v"].as_i64().unwrap(),
                &format!("{kind}/{seed} v({x},{y},{z}) octaves {octaves:?}"),
            );
            assert_bits(
                v_deprecated,
                val["vDeprecated"].as_i64().unwrap(),
                &format!("{kind}/{seed} vDeprecated({x},{y},{z}) octaves {octaves:?}"),
            );
        }
    }
}

#[test]
fn perlin_amplitudes_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["perlin_amplitudes"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let mut r = source(kind, seed);
        // `PerlinNoise.create(random, -3, 1.0, 1.0, 0.5, 0.25, 0.125)` — first
        // amplitude prepended.
        let noise = PerlinNoise::create(&mut r, -3, vec![1.0, 1.0, 0.5, 0.25, 0.125]);
        assert_bits(
            noise.max_value(),
            entry["maxValue"].as_i64().unwrap(),
            &format!("{kind}/{seed} maxValue amplitudes"),
        );
        let v = noise.get_value(3.25, -7.5, 0.75);
        assert_bits(v, entry["v"].as_i64().unwrap(), &format!("{kind}/{seed} v"));
    }
}

#[test]
fn normal_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["normal"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let mut r = source(kind, seed);
        // `NormalNoise.create(random, -3, 1.0, 1.0, 0.5, 0.25, 0.125)`.
        let noise = NormalNoise::create_amplitudes(&mut r, -3, &[1.0, 1.0, 0.5, 0.25, 0.125]);
        assert_bits(
            noise.max_value(),
            entry["maxValue"].as_i64().unwrap(),
            &format!("{kind}/{seed} maxValue"),
        );
        for val in entry["vals"].as_array().unwrap() {
            let (x, y, z) = (
                val["x"].as_f64().unwrap(),
                val["y"].as_f64().unwrap(),
                val["z"].as_f64().unwrap(),
            );
            let v = noise.get_value(x, y, z);
            assert_bits(
                v,
                val["v"].as_i64().unwrap(),
                &format!("{kind}/{seed} v({x},{y},{z})"),
            );
        }
    }
}

#[test]
fn normal_legacy_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["normal_legacy"];
    for entry in section.as_array().unwrap() {
        let seed = entry["seed"].as_i64().unwrap();
        let kind = entry["source"].as_str().unwrap();
        let mut r = source(kind, seed);
        // `createLegacyNetherBiome(random, new NoiseParameters(-3,
        // List.of(1.0, 1.0, 0.5, 0.25)))`.
        let params = NoiseParameters::new(-3, vec![1.0, 1.0, 0.5, 0.25]);
        let noise = NormalNoise::create_legacy_nether_biome(&mut r, params);
        assert_bits(
            noise.max_value(),
            entry["maxValue"].as_i64().unwrap(),
            &format!("{kind}/{seed} maxValue legacy"),
        );
        let v = noise.get_value(3.25, -7.5, 0.75);
        assert_bits(
            v,
            entry["v"].as_i64().unwrap(),
            &format!("{kind}/{seed} v legacy"),
        );
    }
}

#[test]
fn blended_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["blended"];
    for entry in section.as_array().unwrap() {
        let xz_scale = entry["xzScale"].as_f64().unwrap();
        let y_scale = entry["yScale"].as_f64().unwrap();
        let xz_factor = entry["xzFactor"].as_f64().unwrap();
        let y_factor = entry["yFactor"].as_f64().unwrap();
        let smear = entry["smear"].as_f64().unwrap();
        let noise = BlendedNoise::create_unseeded(xz_scale, y_scale, xz_factor, y_factor, smear);
        assert_bits(
            noise.max_value(),
            entry["maxValue"].as_i64().unwrap(),
            &format!("{xz_scale}/{y_scale}/{xz_factor}/{y_factor}/{smear} maxValue"),
        );
        assert_bits(
            noise.min_value(),
            entry["minValue"].as_i64().unwrap(),
            &format!("{xz_scale}/{y_scale}/{xz_factor}/{y_factor}/{smear} minValue"),
        );
        for val in entry["vals"].as_array().unwrap() {
            // The probe computes `(int) pt[0]` before sampling, so the fixture
            // holds the truncated int values.
            let (x, y, z) = (
                val["x"].as_f64().unwrap() as i32,
                val["y"].as_f64().unwrap() as i32,
                val["z"].as_f64().unwrap() as i32,
            );
            let v = noise.compute(x, y, z);
            assert_bits(
                v,
                val["v"].as_i64().unwrap(),
                &format!("{xz_scale}/{y_scale}/{xz_factor}/{y_factor}/{smear} v({x},{y},{z})"),
            );
        }
    }
}

#[test]
fn noise_utils_matches_paper_exactly() {
    let fixture = fixture();
    let section = &fixture["noise_utils"];
    let entry = &section[0];

    // biasTowardsExtreme over the full 32-case table.
    for b in entry["bias"].as_array().unwrap() {
        let noise = b["noise"].as_f64().unwrap();
        let factor = b["factor"].as_f64().unwrap();
        let v = noise_utils::bias_towards_extreme(noise, factor);
        assert_bits(
            v,
            b["v"].as_i64().unwrap(),
            &format!("bias({noise},{factor})"),
        );
    }

    // parity config strings (byte[] and int[] overloads).
    let mut byte_p = [0i8; 256];
    for (i, slot) in byte_p.iter_mut().enumerate() {
        *slot = (255 - i) as i8;
    }
    let mut sb = String::new();
    noise_utils::parity_noise_octave_config_string(
        &mut sb,
        1.2345678,
        -9.8765432,
        0.000123456,
        &byte_p,
    );
    assert_eq!(sb, entry["parityByte"].as_str().unwrap());

    let mut int_p = [0i32; 256];
    for (i, slot) in int_p.iter_mut().enumerate() {
        *slot = (i * 37) as i32;
    }
    let mut sb2 = String::new();
    noise_utils::parity_noise_octave_config_string_i32(
        &mut sb2,
        1.2345678,
        -9.8765432,
        0.000123456,
        &int_p,
    );
    assert_eq!(sb2, entry["parityInt"].as_str().unwrap());

    // Exact decimal midpoint ties: `1.0625`/`-2.0625`/`0.0625` are exact
    // binary halves, so Java's `%.3f` rounds half-away-from-zero
    // ("1.063"/"-2.063"/"0.063"), which Rust's `{:.3}` (half-even) would not.
    // Pins the JDK-exact midpoint formatting on both overloads.
    let mut sb3 = String::new();
    noise_utils::parity_noise_octave_config_string(&mut sb3, 1.0625, -2.0625, 0.0625, &byte_p);
    assert_eq!(sb3, entry["parityByteTie"].as_str().unwrap());

    let mut sb4 = String::new();
    noise_utils::parity_noise_octave_config_string_i32(&mut sb4, 1.0625, -2.0625, 0.0625, &int_p);
    assert_eq!(sb4, entry["parityIntTie"].as_str().unwrap());
}

/// The `wrap` function's periodic behaviour: `wrap(x) = x - lfloor(x /
/// 3.3554432E7 + 0.5) * 3.3554432E7`.
#[test]
fn boundary_wrap_is_periodic_and_exact() {
    let cases = [
        (0.0f64, 0.0f64),
        (3.3554432E7, 0.0f64),    // exactly ROUND_OFF -> wraps to 0.0
        (1.0e9, -6_632_960.0f64), // 30 * ROUND_OFF subtracted
        (-1.0e9, 6_632_960.0f64), // 30 * ROUND_OFF added
    ];
    for (x, expected) in cases {
        let got = PerlinNoise::wrap(x);
        assert_eq!(got.to_bits(), expected.to_bits(), "wrap({x})");
    }
}

/// Java-observable constructor rejections (Paper throws the same messages in
/// the same order). Pinned so a future refactor cannot silently weaken or
/// remove an error path.
#[test]
#[should_panic(expected = "Need some octaves!")]
fn perlin_simplex_rejects_empty_octave_set() {
    PerlinSimplexNoise::new(&mut source("xoroshiro", 42), &[]);
}

#[test]
#[should_panic(expected = "Need some octaves!")]
fn perlin_noise_rejects_empty_octave_set() {
    PerlinNoise::create_octave_set(&mut source("xoroshiro", 42), &[]);
}

#[test]
#[should_panic(
    expected = "Failed to create correct number of noise levels for given non-zero amplitudes"
)]
fn perlin_noise_legacy_rejects_amplitude_mismatch() {
    // octaves=3, zero_octave_index=1: the non-zero amplitude at index 2 (a
    // positive octave) is never backed by a noise level, so the count check
    // fires before the positive-octave guard.
    PerlinNoise::create_legacy_for_legacy_nether_biome(
        &mut source("legacy", 42),
        -1,
        vec![1.0, 0.0, 1.0],
    );
}

#[test]
#[should_panic(expected = "Positive octaves are temporarily disabled")]
fn perlin_noise_legacy_rejects_positive_octaves() {
    // octaves=3, zero_octave_index=1, non-zero amplitudes only at <= index 1,
    // so the level-count check passes and the positive-octave guard fires.
    PerlinNoise::create_legacy_for_legacy_nether_biome(
        &mut source("legacy", 42),
        -1,
        vec![1.0, 1.0, 0.0],
    );
}

/// Java octave-span arithmetic wraps (`-IntTreeSet.firstInt()` and
/// `lowFreq + highFreq + 1` are int ops). A hostile set whose low+high sum
/// overflows i32 must reach the `octaves < 1` guard (not panic on the sum or
/// the negation) exactly as Java.
#[test]
#[should_panic(expected = "Total number of octaves needs to be >= 1")]
fn perlin_simplex_hostile_octave_span_wraps_like_java() {
    // low = -(-2_000_000_000) = 2_000_000_000, high = 2_000_000_000, so
    // `low + high + 1` overflows i32 and wraps negative, tripping the guard.
    PerlinSimplexNoise::new(
        &mut source("xoroshiro", 42),
        &[-2_000_000_000, 2_000_000_000],
    );
}

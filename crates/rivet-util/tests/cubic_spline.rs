//! Java-grounded tests for the `CubicSpline`/`BoundedFloatFunction` port
//! (issue #372).
//!
//! The golden arithmetic values (samples, min/max, hostile messages) were
//! captured from the pinned Paper 26.2 runtime by `SplineProbe` — see
//! `tools/rivet-oracle/src/java/SplineProbe.java` and
//! `tools/rivet-oracle/fixtures/spline/spline-goldens.json`. The probe drives
//! the spline builder through interpolation (nonzero derivatives), linear
//! extension past both ends, and nested value splines. Each sample is compared
//! bit-exactly (`to_bits`).
//!
//! The codec tests run `CubicSpline.codec(Codec<I>)` through the JSON ops
//! (INSTANCE + COMPRESSED) and assert the exact packed shape and the hostile
//! error messages (`List must have contents`, `No key ... in MapLike[..]`).

use rivet_serialization::codec as serde_codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::json_ops::JsonOps;
use rivet_util::bounded_float_function::{BoundedFloat, BoundedFloatFunction, Identity};
use rivet_util::cubic_spline::CubicSpline;
use rivet_util::cubic_spline::codec as spline_codec;
use std::sync::Arc;

fn identity_spline_build() {
    let _ = CubicSpline::<Identity>::builder(Identity).build();
}

// ---------------------------------------------------------------------------
// Golden fixture access (tools/rivet-oracle/fixtures/spline/spline-goldens.json)
// ---------------------------------------------------------------------------

/// The `SplineProbe` golden fixture, embedded so the asserted bit patterns can
/// never drift from what the probe actually captured from Paper.
const GOLDENS: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/spline/spline-goldens.json");

/// Parse a Java `Double.toHexString` value (the probe's `hexF` output) back to
/// the exact `f32` it denotes: `[sign] 0x<h>.<h>p<d>` (C99 `%a`), plus the
/// `Infinity`/`NaN` spellings. The goldens come from a `float` promoted to
/// `double`, so the parse is exact and the narrowing cast is lossless.
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

/// One `SplineProbe` spline case: constructor-computed bounds and the sample
/// sweep, parsed from the golden fixture.
struct GoldenCase {
    min: f32,
    max: f32,
    samples: Vec<(f32, f32)>, // (coordinate, sample)
}

/// Look up a spline case by name in the embedded golden fixture.
fn golden_case(name: &str) -> GoldenCase {
    let root: serde_json::Value = serde_json::from_str(GOLDENS).expect("parse spline-goldens.json");
    let cases = root["cases"].as_array().expect("cases array");
    let case = cases
        .iter()
        .find(|c| c["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no golden case {name}"));
    let min = hex_f32(case["min"].as_str().unwrap());
    let max = hex_f32(case["max"].as_str().unwrap());
    let samples = case["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                hex_f32(o["coordinate"].as_str().unwrap()),
                hex_f32(o["sample"].as_str().unwrap()),
            )
        })
        .collect();
    GoldenCase { min, max, samples }
}

// ---------------------------------------------------------------------------
// Golden arithmetic (SplineProbe, Paper 26.2)
// ---------------------------------------------------------------------------

#[test]
fn golden_constant_raw() {
    let g = golden_case("constant_raw");
    let s = CubicSpline::<Identity>::constant(1.5);
    assert_eq!(s.min_value().to_bits(), g.min.to_bits());
    assert_eq!(s.max_value().to_bits(), g.max.to_bits());
    for (c, want) in &g.samples {
        assert_eq!(s.sample(*c).to_bits(), want.to_bits(), "sample({c})");
    }
    assert_eq!(s.parity_string(), "k=1.500");
}

#[test]
fn golden_constant_one_point() {
    // A one-point multipoint is a `Multipoint`, not a raw float constant.
    let g = golden_case("constant_one_point");
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity).add_point(0.0, 3.25).build();
    assert_eq!(s.min_value().to_bits(), g.min.to_bits());
    assert_eq!(s.max_value().to_bits(), g.max.to_bits());
    for (c, want) in &g.samples {
        assert_eq!(s.sample(*c).to_bits(), want.to_bits(), "sample({c})");
    }
    assert_eq!(
        s.parity_string(),
        "Spline{coordinate=Identity, locations=[0.000], derivatives=[0.000], values=[k=3.250]}"
    );
}

#[test]
fn golden_two_point_no_deriv() {
    let g = golden_case("two_point_no_deriv");
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point(-1.0, 2.0)
        .add_point(1.0, 4.0)
        .build();
    assert_eq!(s.min_value().to_bits(), g.min.to_bits());
    assert_eq!(s.max_value().to_bits(), g.max.to_bits());
    for (c, want) in &g.samples {
        assert_eq!(s.sample(*c).to_bits(), want.to_bits(), "sample({c})");
    }
    assert_eq!(
        s.parity_string(),
        "Spline{coordinate=Identity, locations=[-1.000, 1.000], derivatives=[0.000, 0.000], values=[k=2.000, k=4.000]}"
    );
}

#[test]
fn golden_three_point_deriv() {
    // Nonzero derivatives exercise the hermite correction term.
    let g = golden_case("three_point_deriv");
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point_with_derivative(-3.0, 5.0, 1.0)
        .add_point_with_derivative(0.0, -1.0, 0.0)
        .add_point_with_derivative(3.0, 2.0, -2.0)
        .build();
    // identity coordinate is unbounded -> low extension runs to -Infinity.
    assert_eq!(s.min_value().to_bits(), g.min.to_bits());
    assert_eq!(s.max_value().to_bits(), g.max.to_bits());
    for (c, want) in &g.samples {
        assert_eq!(s.sample(*c).to_bits(), want.to_bits(), "sample({c})");
    }
}

#[test]
fn golden_neg_slope_extend() {
    // High-end extension with a nonzero derivative at the last knot.
    let g = golden_case("neg_slope_extend");
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point_with_derivative(0.0, 10.0, -1.0)
        .add_point_with_derivative(4.0, 2.0, 0.0)
        .add_point_with_derivative(8.0, -3.0, 0.5)
        .build();
    assert_eq!(s.min_value().to_bits(), g.min.to_bits());
    assert_eq!(s.max_value().to_bits(), g.max.to_bits());
    for (c, want) in &g.samples {
        assert_eq!(s.sample(*c).to_bits(), want.to_bits(), "sample({c})");
    }
}

#[test]
fn golden_nested_values() {
    // Spline values that are themselves splines.
    let g = golden_case("nested_values");
    let inner: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point(0.0, 1.0)
        .add_point(2.0, 3.0)
        .build();
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_spline(-2.0, inner.clone())
        .add_spline(2.0, CubicSpline::constant(0.5))
        .build();
    assert_eq!(s.min_value().to_bits(), g.min.to_bits());
    assert_eq!(s.max_value().to_bits(), g.max.to_bits());
    for (c, want) in &g.samples {
        assert_eq!(s.sample(*c).to_bits(), want.to_bits(), "sample({c})");
    }
    // The nested values appear in the parity string in the packed order.
    assert_eq!(
        s.parity_string(),
        "Spline{coordinate=Identity, locations=[-2.000, 2.000], derivatives=[0.000, 0.000], values=[Spline{coordinate=Identity, locations=[0.000, 2.000], derivatives=[0.000, 0.000], values=[k=1.000, k=3.000]}, k=0.500]}"
    );
}

// ---------------------------------------------------------------------------
// Parity-string formatting (Java `%.3f` tie rounding)
// ---------------------------------------------------------------------------

#[test]
fn parity_string_rounds_ties_like_java() {
    // Java's `Formatter` rounds the exact decimal half-away-from-zero; Rust
    // `{:.3}` rounds half-even, so a knot sitting exactly on a 3-decimal tie
    // (e.g. 0.0625) must still print like Java: 0.063, not 0.062.
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point(-0.0625, 1.0)
        .add_point(0.0625, 2.0)
        .add_point_with_derivative(0.125, 3.0, 0.0625)
        .build();
    let p = s.parity_string();
    assert!(p.contains("locations=[-0.063, 0.063, 0.125]"), "got: {p}");
    assert!(p.contains("derivatives=[0.000, 0.000, 0.063]"), "got: {p}");
    // A raw constant at a tie.
    assert_eq!(
        CubicSpline::<Identity>::constant(0.0625).parity_string(),
        "k=0.063"
    );
    assert_eq!(
        CubicSpline::<Identity>::constant(-2.3125).parity_string(),
        "k=-2.313"
    );
}

// ---------------------------------------------------------------------------
// Hostile builder validation (SplineProbe, Paper 26.2)
// ---------------------------------------------------------------------------

#[test]
fn hostile_descending_order() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = CubicSpline::<Identity>::builder(Identity)
            .add_point(2.0, 1.0)
            .add_point(1.0, 0.0)
            .build();
    }));
    assert!(result.is_err());
    let msg = panic_message(result);
    assert_eq!(msg, "Please register points in ascending order");
}

#[test]
fn hostile_equal_order() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = CubicSpline::<Identity>::builder(Identity)
            .add_point(1.0, 0.0)
            .add_point(1.0, 1.0)
            .build();
    }));
    assert!(result.is_err());
    let msg = panic_message(result);
    assert_eq!(msg, "Please register points in ascending order");
}

#[test]
fn hostile_empty_builder() {
    // SplineProbe: IllegalStateException "No elements added"
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(identity_spline_build));
    assert!(result.is_err());
    let msg = panic_message(result);
    assert_eq!(msg, "No elements added");
}

fn panic_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    match result.unwrap_err().downcast::<String>() {
        Ok(s) => *s,
        Err(e) => match e.downcast::<&'static str>() {
            Ok(s) => s.to_string(),
            Err(_) => panic!("panic payload was not a String"),
        },
    }
}

// ---------------------------------------------------------------------------
// Codec round trip + hostile
// ---------------------------------------------------------------------------

/// A concrete coordinate type for the codec tests. The coordinate codec is a
/// plain float (like `DensityFunctions.Spline.Coordinate` is an xmap over its
/// inner function codec).
#[derive(Clone, Debug)]
struct TestCoord(f32);
impl BoundedFloat for TestCoord {
    fn min_value(&self) -> f32 {
        -1.0
    }
    fn max_value(&self) -> f32 {
        1.0
    }
}
impl BoundedFloatFunction<f32> for TestCoord {
    fn apply(&self, c: f32) -> f32 {
        c * self.0
    }
}

fn coord_codec<O: DynamicOps + 'static>() -> Arc<dyn Codec<TestCoord, O>> {
    serde_codec::xmap(
        serde_codec::float_codec(),
        Arc::new(|f: &f32| TestCoord(*f)),
        Arc::new(|t: &TestCoord| t.0),
    )
}

type SplineCodec<O> = Arc<dyn Codec<CubicSpline<TestCoord>, O>>;

fn spline_codec_for<O: DynamicOps + 'static>() -> SplineCodec<O> {
    spline_codec(coord_codec())
}

fn round_trip<O: DynamicOps<Output = serde_json::Value> + 'static>(
    ops: &O,
    s: &CubicSpline<TestCoord>,
) -> serde_json::Value {
    let encoded = spline_codec_for::<O>()
        .encode_start(ops, s)
        .get_or_throw("encode failed")
        .clone();
    let decoded = spline_codec_for::<O>().parse(ops, &encoded);
    let decoded = decoded.get_or_throw("decode failed").clone();
    // Re-encode the decoded value: it must be byte-identical to the original.
    let reencoded = spline_codec_for::<O>()
        .encode_start(ops, &decoded)
        .get_or_throw("re-encode failed")
        .clone();
    assert_eq!(reencoded, encoded, "round-trip not stable");
    reencoded
}

#[test]
fn codec_round_trip_constant() {
    for ops in [JsonOps::INSTANCE, JsonOps::COMPRESSED] {
        let s = CubicSpline::<TestCoord>::constant(1.5);
        let json = round_trip(&ops, &s);
        // constant encodes as the raw float.
        assert_eq!(json, serde_json::json!(1.5));
    }
}

#[test]
fn codec_round_trip_multipoint() {
    for ops in [JsonOps::INSTANCE, JsonOps::COMPRESSED] {
        let s: CubicSpline<TestCoord> = CubicSpline::builder(TestCoord(2.0))
            .add_point_with_derivative(-1.0, 3.0, 0.5)
            .add_point_with_derivative(0.0, -2.0, 0.0)
            .add_spline(1.0, CubicSpline::constant(4.0))
            .build();
        let _json = round_trip(&ops, &s);
        // Also verify the decoded structure.
        let encoded = spline_codec_for::<JsonOps>()
            .encode_start(&ops, &s)
            .get_or_throw("encode")
            .clone();
        let decoded: CubicSpline<TestCoord> = spline_codec_for::<JsonOps>()
            .parse(&ops, &encoded)
            .get_or_throw("decode")
            .clone();
        match decoded {
            CubicSpline::Multipoint(m) => {
                assert_eq!(m.locations(), &[-1.0, 0.0, 1.0]);
                assert_eq!(m.derivatives(), &[0.5, 0.0, 0.0]);
                assert_eq!(m.values().len(), 3);
                match &m.values()[2] {
                    CubicSpline::Constant(v) => assert_eq!(*v, 4.0),
                    _ => panic!("expected constant value"),
                }
            }
            _ => panic!("expected multipoint"),
        }
    }
}

#[test]
fn codec_round_trip_nested() {
    for ops in [JsonOps::INSTANCE, JsonOps::COMPRESSED] {
        let inner: CubicSpline<TestCoord> = CubicSpline::builder(TestCoord(1.0))
            .add_point(0.0, 1.0)
            .add_point(2.0, 3.0)
            .build();
        let s: CubicSpline<TestCoord> = CubicSpline::builder(TestCoord(1.0))
            .add_spline(-2.0, inner)
            .add_spline(2.0, CubicSpline::constant(0.5))
            .build();
        let _json = round_trip(&ops, &s);
    }
}

#[test]
fn codec_hostile_empty_points() {
    // The `points` field is `ExtraCodecs.nonEmptyList`; an empty list must
    // fail with "List must have contents". Under INSTANCE ops the map is
    // decoded directly; under COMPRESSED ops a JSON object is not a packed
    // list, so the map-codec `compressedDecode` rejects it first ("Input is
    // not a list") — the Java `MapCodecCodec` compressed path behaves the
    // same. Assert the INSTANCE message (the nonEmptyList validation) here.
    let ops = JsonOps::INSTANCE;
    let bad = serde_json::json!({
        "coordinate": 2.0,
        "points": []
    });
    let decoded = spline_codec_for::<JsonOps>().parse(&ops, &bad);
    assert!(decoded.is_error());
    let msg = decoded.error_ref().unwrap().message().to_string();
    assert!(msg.contains("List must have contents"), "got: {msg}");
}

#[test]
fn codec_hostile_empty_points_compressed() {
    // COMPRESSED: a JSON object is not a packed list, so the either's
    // multipoint branch fails at the `compressedDecode` list check.
    let ops = JsonOps::COMPRESSED;
    let bad = serde_json::json!({
        "coordinate": 2.0,
        "points": []
    });
    let decoded = spline_codec_for::<JsonOps>().parse(&ops, &bad);
    assert!(decoded.is_error());
    let msg = decoded.error_ref().unwrap().message().to_string();
    assert!(
        msg.contains("Input is not a list") || msg.contains("List must have contents"),
        "got: {msg}"
    );
}

#[test]
fn codec_hostile_missing_field() {
    // Missing `coordinate` -> `FieldDecoder` "No key coordinate in MapLike[..]".
    let ops = JsonOps::INSTANCE;
    let bad = serde_json::json!({
        "points": [{"location": 0.0, "value": 1.0, "derivative": 0.0}]
    });
    let decoded = spline_codec_for::<JsonOps>().parse(&ops, &bad);
    assert!(decoded.is_error());
    let msg = decoded.error_ref().unwrap().message().to_string();
    assert!(msg.contains("No key coordinate"), "got: {msg}");
}

#[test]
fn codec_hostile_bad_point_field() {
    // A point missing its `derivative` field.
    let ops = JsonOps::INSTANCE;
    let bad = serde_json::json!({
        "coordinate": 1.0,
        "points": [{"location": 0.0, "value": 1.0}]
    });
    let decoded = spline_codec_for::<JsonOps>().parse(&ops, &bad);
    assert!(decoded.is_error());
}

#[test]
fn codec_hostile_nonempty_validation_ordering() {
    // `Multipoint.createFromPoints` builds through the 4-arg delegating
    // constructor, so decoding a single point still re-bounds.
    let ops = JsonOps::INSTANCE;
    let s: CubicSpline<TestCoord> = CubicSpline::builder(TestCoord(2.0))
        .add_point(0.0, 3.25)
        .build();
    // coordinate min/max is [-1, 1], locations [0] — min_input < 0 -> low
    // extension runs with derivative 0, so min == value == 3.25.
    assert_eq!(s.min_value().to_bits(), 0x40500000);
    let _ = round_trip(&ops, &s);
}

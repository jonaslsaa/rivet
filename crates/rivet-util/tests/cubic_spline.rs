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
// Golden arithmetic (SplineProbe, Paper 26.2)
// ---------------------------------------------------------------------------

#[test]
fn golden_constant_raw() {
    let s = CubicSpline::<Identity>::constant(1.5);
    assert_eq!(s.min_value().to_bits(), 0x3fc00000);
    assert_eq!(s.max_value().to_bits(), 0x3fc00000);
    for c in [-3.0f32, 0.0, 2.5, 100.0] {
        assert_eq!(s.sample(c).to_bits(), 0x3fc00000);
    }
    assert_eq!(s.parity_string(), "k=1.500");
}

#[test]
fn golden_constant_one_point() {
    // A one-point multipoint is a `Multipoint`, not a raw float constant.
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity).add_point(0.0, 3.25).build();
    assert_eq!(s.min_value().to_bits(), 0x40500000); // 3.25
    assert_eq!(s.max_value().to_bits(), 0x40500000);
    for c in [-3.0f32, 0.0, 2.5, 100.0] {
        assert_eq!(s.sample(c).to_bits(), 0x40500000);
    }
    assert_eq!(
        s.parity_string(),
        "Spline{coordinate=Identity, locations=[0.000], derivatives=[0.000], values=[k=3.250]}"
    );
}

#[test]
fn golden_two_point_no_deriv() {
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point(-1.0, 2.0)
        .add_point(1.0, 4.0)
        .build();
    assert_eq!(s.min_value().to_bits(), 0x40000000); // 2.0
    assert_eq!(s.max_value().to_bits(), 0x40800000); // 4.0
    let cases = [
        (-2.0f32, 0x40000000),
        (-1.5, 0x40000000),
        (-1.0, 0x40000000),
        (-0.5, 0x40140000), // 2.3125
        (0.0, 0x40400000),  // 3.0
        (0.5, 0x406c0000),  // 3.6875
        (1.0, 0x40800000),
        (1.5, 0x40800000),
        (2.0, 0x40800000),
    ];
    for (c, bits) in cases {
        assert_eq!(s.sample(c).to_bits(), bits, "sample({c})");
    }
    assert_eq!(
        s.parity_string(),
        "Spline{coordinate=Identity, locations=[-1.000, 1.000], derivatives=[0.000, 0.000], values=[k=2.000, k=4.000]}"
    );
}

#[test]
fn golden_three_point_deriv() {
    // Nonzero derivatives exercise the hermite correction term.
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point_with_derivative(-3.0, 5.0, 1.0)
        .add_point_with_derivative(0.0, -1.0, 0.0)
        .add_point_with_derivative(3.0, 2.0, -2.0)
        .build();
    // identity coordinate is unbounded -> low extension runs to -Infinity.
    assert_eq!(s.min_value().to_bits(), 0xff800000); // -Infinity
    assert_eq!(s.max_value().to_bits(), 0x40e80000); // 7.25
    let cases = [
        (-4.0f32, 0x40800000),
        (-3.0, 0x40a00000),
        (-2.0, 0x4078e38e),
        (-1.0, 0x3f471c72),
        (0.0, 0xbf800000),
        (1.0, 0x3e638e38),
        (2.0, 0x40071c72),
        (3.0, 0x40000000),
        (4.0, 0x00000000),
    ];
    for (c, bits) in cases {
        assert_eq!(s.sample(c).to_bits(), bits, "sample({c})");
    }
}

#[test]
fn golden_neg_slope_extend() {
    // High-end extension with a nonzero derivative at the last knot.
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point_with_derivative(0.0, 10.0, -1.0)
        .add_point_with_derivative(4.0, 2.0, 0.0)
        .add_point_with_derivative(8.0, -3.0, 0.5)
        .build();
    assert_eq!(s.min_value().to_bits(), 0xc0980000); // -4.75
    assert_eq!(s.max_value().to_bits(), 0x7f800000); // +Infinity
    let cases = [
        (-2.0f32, 0x41400000),
        (0.0, 0x41200000),
        (2.0, 0x40b00000),
        (4.0, 0x40000000),
        (6.0, 0xbf400000),
        (8.0, 0xc0400000),
        (10.0, 0xc0000000),
        (12.0, 0xbf800000),
    ];
    for (c, bits) in cases {
        assert_eq!(s.sample(c).to_bits(), bits, "sample({c})");
    }
}

#[test]
fn golden_nested_values() {
    // Spline values that are themselves splines.
    let inner: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_point(0.0, 1.0)
        .add_point(2.0, 3.0)
        .build();
    let s: CubicSpline<Identity> = CubicSpline::builder(Identity)
        .add_spline(-2.0, inner.clone())
        .add_spline(2.0, CubicSpline::constant(0.5))
        .build();
    assert_eq!(s.min_value().to_bits(), 0x3f000000); // 0.5
    assert_eq!(s.max_value().to_bits(), 0x40400000); // 3.0
    let cases = [
        (-3.0f32, 0x3f800000),
        (-2.0, 0x3f800000),
        (-1.0, 0x3f6c0000),
        (0.0, 0x3f400000),
        (1.0, 0x3f3c0000),
        (2.0, 0x3f000000),
        (3.0, 0x3f000000),
    ];
    for (c, bits) in cases {
        assert_eq!(s.sample(c).to_bits(), bits, "sample({c})");
    }
    // The nested values appear in the parity string in the packed order.
    assert_eq!(
        s.parity_string(),
        "Spline{coordinate=Identity, locations=[-2.000, 2.000], derivatives=[0.000, 0.000], values=[Spline{coordinate=Identity, locations=[0.000, 2.000], derivatives=[0.000, 0.000], values=[k=1.000, k=3.000]}, k=0.500]}"
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
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| identity_spline_build()));
    assert!(result.is_err());
    let msg = panic_message(result);
    assert_eq!(msg, "No elements added");
}

#[test]
fn hostile_empty_builder_matches_probe() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| identity_spline_build()));
    assert!(result.is_err());
    let msg = panic_message(result);
    // SplineProbe: IllegalStateException "No elements added"
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

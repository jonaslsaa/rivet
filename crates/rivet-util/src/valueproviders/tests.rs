//! Tests for `net.minecraft.util.valueproviders` (the `valueproviders` module).
//!
//! Golden sample sequences are ground truth captured from the crate's own
//! `LegacyRandomSource` + `mth` helpers (deterministic; the same RNG the oracle
//! differential harness uses), with the `nextInt(bound)` power-of-two fast path
//! and the f32-rounding in `nextFloat`/`nextDouble` exactly as `random.rs`
//! implements them. The int goldens were additionally cross-checked against an
//! independent Python model of the 48-bit LCG.
//!
//! Codec JSON shapes and validation messages are captured from the live codecs
//! and cross-checked against the Paper Java sources (the exact strings are
//! quoted in the per-file module docs). A float field encodes through
//! `JsonOps.createFloat` — `rivet-serialization` stores the `f64` nearest
//! Java's `Float.toString` literal, so `0.05f` writes `0.05`, exactly as Gson
//! renders a `JsonPrimitive(Float)` (see `create_float_uses_float_to_string_literal`
//! in `rivet-serialization`'s `json_ops_tests`; `float_provider_round_trips`
//! pins the shape end-to-end).

use crate::random::LegacyRandomSource;
use crate::valueproviders::biased_to_bottom_int::BiasedToBottomInt;
use crate::valueproviders::clamped_int::ClampedInt;
use crate::valueproviders::clamped_normal_float::ClampedNormalFloat;
use crate::valueproviders::clamped_normal_int::ClampedNormalInt;
use crate::valueproviders::constant_float::ConstantFloat;
use crate::valueproviders::constant_int::ConstantInt;
use crate::valueproviders::float_provider::{
    FloatProvider, float_provider_codec, float_provider_codec_with_bounds,
    float_provider_codec_with_min,
};
use crate::valueproviders::int_provider::{
    IntProvider, int_provider_codec, int_provider_codec_with_bounds,
    non_negative_int_provider_codec, positive_int_provider_codec,
};
use crate::valueproviders::multiplied_floats::MultipliedFloats;
use crate::valueproviders::sampled_float::SampledFloat;
use crate::valueproviders::trapezoid_float::TrapezoidFloat;
use crate::valueproviders::trapezoid_int::TrapezoidInt;
use crate::valueproviders::uniform_float::UniformFloat;
use crate::valueproviders::uniform_int::UniformInt;
use crate::valueproviders::weighted_list_int::WeightedListInt;
use crate::{Weighted, WeightedList};
use rivet_serialization::json_ops::JsonOps;
use serde_json::json;

type J = JsonOps;

// ---------------------------------------------------------------------------
// Deterministic sample goldens (LegacyRandomSource seed 42)
// ---------------------------------------------------------------------------

#[test]
fn uniform_int_sample_sequence() {
    let mut r = LegacyRandomSource::new(42);
    let p = UniformInt::of(0, 4);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![0, 3, 3, 4, 0, 0, 0, 3]);
}

#[test]
fn uniform_int_negative_min_sample_sequence() {
    let mut r = LegacyRandomSource::new(42);
    let p = UniformInt::of(-2, 2);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![-2, 1, 1, 2, -2, -2, -2, 1]);
}

#[test]
fn biased_to_bottom_int_sample_sequence() {
    let mut r = LegacyRandomSource::new(42);
    let p = BiasedToBottomInt::of(0, 4);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![0, 0, 0, 0, 3, 2, 0, 1]);
}

#[test]
fn trapezoid_int_triangle_sample_sequence() {
    // `triangle(3)`: max == -min == 3, plateau == 0 -> the nextInt(max+1) -
    // nextInt(max+1) branch.
    let mut r = LegacyRandomSource::new(42);
    let p = TrapezoidInt::triangle(3);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![2, 2, -2, -1, 2, 2, 0, -1]);
}

#[test]
fn trapezoid_int_plateau_branch_sample_sequence() {
    // `(0, 8, 2)`: range 8, plateauStart 3, plateauEnd 5.
    let mut r = LegacyRandomSource::new(42);
    let p = TrapezoidInt::of(0, 8, 2);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![2, 0, 3, 7, 1, 3, 1, 4]);
}

#[test]
fn trapezoid_int_full_plateau_sample_sequence() {
    // `(0, 8, 8)`: plateau == range -> uniform in [0, 8].
    let mut r = LegacyRandomSource::new(42);
    let p = TrapezoidInt::of(0, 8, 8);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![8, 3, 0, 8, 0, 7, 5, 2]);
}

#[test]
fn clamped_int_sample_sequence() {
    let mut r = LegacyRandomSource::new(42);
    let p = ClampedInt::of(IntProvider::Uniform(UniformInt::of(-5, 5)), 0, 3);
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![2, 0, 3, 0, 0, 3, 0, 0]);
}

#[test]
fn weighted_list_int_sample_sequence() {
    // [(const 2, w1), (uniform 5..7, w3), (const 9, w2)] — total weight 6.
    let mut r = LegacyRandomSource::new(42);
    let p = WeightedListInt::new(WeightedList::new(&[
        Weighted::new(IntProvider::Constant(ConstantInt::of(2)), 1),
        Weighted::new(IntProvider::Uniform(UniformInt::of(5, 7)), 3),
        Weighted::new(IntProvider::Constant(ConstantInt::of(9)), 2),
    ]));
    let got: Vec<i32> = (0..8).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![5, 2, 5, 7, 6, 9, 7, 2]);
}

#[test]
fn weighted_list_int_folds_min_max() {
    let p = WeightedListInt::new(WeightedList::new(&[
        Weighted::new(IntProvider::Constant(ConstantInt::of(2)), 1),
        Weighted::new(IntProvider::Uniform(UniformInt::of(5, 7)), 3),
        Weighted::new(IntProvider::Constant(ConstantInt::of(9)), 2),
    ]));
    assert_eq!(p.min_inclusive(), 2);
    assert_eq!(p.max_inclusive(), 9);
}

#[test]
fn uniform_float_sample_sequence_bits() {
    let mut r = LegacyRandomSource::new(42);
    let p = UniformFloat::of(0.0, 1.0);
    let got: Vec<u32> = (0..6).map(|_| p.sample(&mut r).to_bits()).collect();
    assert_eq!(
        got,
        vec![
            0x3f3a419d, 0x3d5fe8a0, 0x3f2ee7bb, 0x3d445c00, 0x3e9e1078, 0x3f712bbb
        ]
    );
}

#[test]
fn trapezoid_float_sample_sequence_bits() {
    let mut r = LegacyRandomSource::new(42);
    let p = TrapezoidFloat::of(0.0, 10.0, 2.0);
    let got: Vec<u32> = (0..6).map(|_| p.sample(&mut r).to_bits()).collect();
    assert_eq!(
        got,
        vec![
            0x4092b07b, 0x408950ac, 0x40b3dc0a, 0x408fc938, 0x408b7996, 0x40e73558
        ]
    );
}

#[test]
fn clamped_normal_int_sample_sequence() {
    let mut r = LegacyRandomSource::new(42);
    let p = ClampedNormalInt::of(50.0, 20.0, 0, 100);
    let got: Vec<i32> = (0..6).map(|_| p.sample(&mut r)).collect();
    assert_eq!(got, vec![72, 68, 31, 27, 55, 63]);
}

#[test]
fn clamped_normal_float_sample_sequence_bits() {
    let mut r = LegacyRandomSource::new(42);
    let p = ClampedNormalFloat::of(50.0, 20.0, 0.0, 100.0);
    let got: Vec<u32> = (0..6).map(|_| p.sample(&mut r).to_bits()).collect();
    assert_eq!(
        got,
        vec![
            0x4291ad1c, 0x4288c6bd, 0x41f80577, 0x41dee1ad, 0x425e7a6c, 0x427ec512
        ]
    );
}

#[test]
fn multiplied_floats_sample_sequence_bits() {
    // [const 2.0, uniform(0,1), const 3.0] — draws one nextFloat, in order.
    let mut r = LegacyRandomSource::new(42);
    let m = MultipliedFloats::new(vec![
        SampledFloat::Float(FloatProvider::Constant(ConstantFloat::of(2.0))),
        SampledFloat::Float(FloatProvider::Uniform(UniformFloat::of(0.0, 1.0))),
        SampledFloat::Float(FloatProvider::Constant(ConstantFloat::of(3.0))),
    ]);
    let got: Vec<u32> = (0..6).map(|_| m.sample(&mut r).to_bits()).collect();
    assert_eq!(
        got,
        vec![
            0x408bb136, 0x3ea7ee78, 0x40832dcc, 0x3e934500, 0x3fed18b4, 0x40b4e0cc
        ]
    );
}

// ---------------------------------------------------------------------------
// IntProvider / FloatProvider codec round-trips (Paper 26.2 JsonOps shapes)
// ---------------------------------------------------------------------------

#[test]
fn int_provider_constant_round_trip_as_bare_number() {
    let codec = int_provider_codec::<J>();
    let p = IntProvider::Constant(ConstantInt::of(5));
    let enc = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .result()
        .cloned()
        .unwrap();
    assert_eq!(enc, json!(5));
    let dec = codec
        .parse(&JsonOps::INSTANCE, &enc)
        .result()
        .cloned()
        .unwrap();
    assert_eq!(dec, p);
}

#[test]
fn int_provider_uniform_round_trip() {
    let codec = int_provider_codec::<J>();
    let p = IntProvider::Uniform(UniformInt::of(3, 7));
    let enc = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .result()
        .cloned()
        .unwrap();
    assert_eq!(
        enc,
        json!({"min_inclusive": 3, "max_inclusive": 7, "type": "minecraft:uniform"})
    );
    let dec = codec
        .parse(&JsonOps::INSTANCE, &enc)
        .result()
        .cloned()
        .unwrap();
    assert_eq!(dec, p);
}

#[test]
fn int_provider_all_variant_encoded_shapes() {
    let codec = int_provider_codec::<J>();
    let cases: Vec<(IntProvider, serde_json::Value)> = vec![
        (
            IntProvider::BiasedToBottom(BiasedToBottomInt::of(0, 4)),
            json!({"min_inclusive": 0, "max_inclusive": 4, "type": "minecraft:biased_to_bottom"}),
        ),
        (
            IntProvider::Trapezoid(TrapezoidInt::of(0, 8, 2)),
            json!({"min": 0, "max": 8, "plateau": 2, "type": "minecraft:trapezoid"}),
        ),
        (
            IntProvider::ClampedNormal(ClampedNormalInt::of(50.0, 20.0, 0, 100)),
            json!({"mean": 50.0, "deviation": 20.0, "min_inclusive": 0,
                   "max_inclusive": 100, "type": "minecraft:clamped_normal"}),
        ),
        (
            IntProvider::Clamped(ClampedInt::of(
                IntProvider::Uniform(UniformInt::of(-5, 5)),
                0,
                3,
            )),
            json!({"source": {"min_inclusive": -5, "max_inclusive": 5,
                               "type": "minecraft:uniform"},
                   "min_inclusive": 0, "max_inclusive": 3, "type": "minecraft:clamped"}),
        ),
        (
            IntProvider::WeightedList(WeightedListInt::new(WeightedList::new(&[
                Weighted::new(IntProvider::Constant(ConstantInt::of(2)), 1),
                Weighted::new(IntProvider::Uniform(UniformInt::of(5, 7)), 3),
            ]))),
            json!({"distribution": [{"data": 2, "weight": 1},
                                    {"data": {"min_inclusive": 5, "max_inclusive": 7,
                                              "type": "minecraft:uniform"},
                                     "weight": 3}],
                   "type": "minecraft:weighted_list"}),
        ),
    ];
    for (p, expected) in cases {
        let enc = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(enc, expected, "encoded shape for {p:?}");
        let dec = codec
            .parse(&JsonOps::INSTANCE, &enc)
            .result()
            .cloned()
            .unwrap();
        // For the `WeightedListInt` case the decode-equality assertion holds only
        // through the documented value-equality divergence: Java compares plain
        // (non-record) provider classes by reference identity, so a decoded
        // `WeightedListInt` would NOT equal the original there (see `IntProvider`'s
        // enum doc). The assertion is still a sound round-trip structural check.
        assert_eq!(dec, p, "round-trip for {p:?}");
    }
}

#[test]
fn int_provider_nested_weighted_list_clamped_round_trip() {
    // Recursion through the single recursive codec: a clamped provider nested
    // inside a weighted-list entry.
    let codec = int_provider_codec::<J>();
    let p = IntProvider::WeightedList(WeightedListInt::new(WeightedList::new(&[
        Weighted::new(
            IntProvider::Clamped(ClampedInt::of(
                IntProvider::Uniform(UniformInt::of(0, 10)),
                1,
                9,
            )),
            1,
        ),
        Weighted::new(IntProvider::Constant(ConstantInt::of(4)), 1),
    ])));
    let enc = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .result()
        .cloned()
        .unwrap();
    assert_eq!(
        enc,
        json!({"distribution": [{"data": {"source": {"min_inclusive": 0, "max_inclusive": 10,
                                                      "type": "minecraft:uniform"},
                                            "min_inclusive": 1, "max_inclusive": 9,
                                            "type": "minecraft:clamped"},
                                  "weight": 1},
                                 {"data": 4, "weight": 1}],
               "type": "minecraft:weighted_list"})
    );
    let dec = codec
        .parse(&JsonOps::INSTANCE, &enc)
        .result()
        .cloned()
        .unwrap();
    // Round-trip equality via the documented value-equality divergence (Java
    // would compare the `WeightedListInt` by reference identity) — see
    // `IntProvider`'s enum doc.
    assert_eq!(dec, p);
}

#[test]
fn clamped_int_encodes_effective_bounds() {
    // Clamp range not contained in the source range: Java's record codec
    // `forGetter(ClampedInt::minInclusive)` / `maxInclusive` bind to the
    // OVERRIDDEN accessors, so encode serializes the EFFECTIVE bounds
    // (clamped into the source range), not the raw clamp bounds. The existing
    // shape tests only use clamp ranges contained in the source range where
    // effective == raw.
    let codec = int_provider_codec::<J>();
    let p = IntProvider::Clamped(ClampedInt::of(
        IntProvider::Uniform(UniformInt::of(5, 7)),
        0,
        100,
    ));
    let enc = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .result()
        .cloned()
        .unwrap();
    assert_eq!(
        enc,
        json!({"source": {"min_inclusive": 5, "max_inclusive": 7,
                           "type": "minecraft:uniform"},
               "min_inclusive": 5, "max_inclusive": 7, "type": "minecraft:clamped"})
    );
    // Decode applies the record canonical constructor with the serialized
    // (effective) values, matching Java's `apply(i, ClampedInt::new)`.
    let dec = codec
        .parse(&JsonOps::INSTANCE, &enc)
        .result()
        .cloned()
        .unwrap();
    match dec {
        IntProvider::Clamped(c) => {
            assert_eq!(c.min_inclusive(), 5);
            assert_eq!(c.max_inclusive(), 7);
        }
        _ => panic!("expected a clamped provider"),
    }
}

#[test]
fn clamped_int_rejects_effective_max_below_effective_min() {
    // Java's `ClampedInt.MAP_CODEC` validate resolves `u.maxInclusive`/
    // `u.minInclusive` to the record's OVERRIDDEN accessors (the EFFECTIVE
    // bounds), so a source range narrower than the clamp range errors even when
    // the raw clamp bounds look valid. `ClampedInt.of(UniformInt(100,200), 0,
    // 3)` has effective bounds [100, 3] and fails with the effective values in
    // the message; the raw check (`3 < 0`) would wrongly pass.
    let codec = int_provider_codec::<J>();
    let p = IntProvider::Clamped(ClampedInt::of(
        IntProvider::Uniform(UniformInt::of(100, 200)),
        0,
        3,
    ));
    let enc_err = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        enc_err.as_deref(),
        Some("Max must be at least min, min_inclusive: 100, max_inclusive: 3")
    );
    // The same raw serialization (what a non-faithful codec would emit) also
    // fails on decode: the canonical constructor rebuilds the record with the
    // raw clamp bounds, then validate rejects on the effective bounds.
    let dec_err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"source": {"min_inclusive": 100, "max_inclusive": 200,
                               "type": "minecraft:uniform"},
                   "min_inclusive": 0, "max_inclusive": 3, "type": "minecraft:clamped"}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert!(
        dec_err
            .as_deref()
            .unwrap_or_default()
            .contains("Max must be at least min, min_inclusive: 100, max_inclusive: 3"),
        "decode error should surface the effective-bounds message, got: {dec_err:?}"
    );
}

#[test]
fn int_provider_unknown_type_error_message() {
    let codec = int_provider_codec::<J>();
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:not_a_provider", "value": 1}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some(
            "Failed to parse either. First: Not a number: {\"type\":\"minecraft:not_a_provider\",\"value\":1}; \
             Second: Unknown registry key in ResourceKey[minecraft:root / minecraft:int_provider_type]: minecraft:not_a_provider"
        )
    );
}

#[test]
fn int_provider_leading_colon_type_uses_default_namespace() {
    // `Identifier.bySeparator(":uniform", ':')` treats a separator at index 0
    // as an empty namespace: it strips the colon and applies the default
    // namespace, producing `minecraft:uniform` (a registered type). The Rust
    // `default_namespace` mirrors this exactly.
    let codec = int_provider_codec::<J>();
    let p = IntProvider::Uniform(UniformInt::of(3, 7));
    let dec = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": ":uniform", "min_inclusive": 3, "max_inclusive": 7}),
        )
        .result()
        .cloned();
    assert_eq!(dec, Some(p));
}

#[test]
fn float_provider_leading_colon_type_uses_default_namespace() {
    let codec = float_provider_codec::<J>();
    let p = FloatProvider::Uniform(UniformFloat::of(0.5, 1.5));
    let dec = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": ":uniform", "min_inclusive": 0.5, "max_exclusive": 1.5}),
        )
        .result()
        .cloned();
    assert_eq!(dec, Some(p));
}

#[test]
fn float_provider_round_trips() {
    let codec = float_provider_codec::<J>();
    let cases: Vec<(FloatProvider, serde_json::Value)> = vec![
        (FloatProvider::Constant(ConstantFloat::of(2.5)), json!(2.5)),
        (
            FloatProvider::Uniform(UniformFloat::of(0.5, 1.5)),
            json!({"min_inclusive": 0.5, "max_exclusive": 1.5, "type": "minecraft:uniform"}),
        ),
        (
            FloatProvider::Trapezoid(TrapezoidFloat::of(0.0, 10.0, 2.0)),
            json!({"min": 0.0, "max": 10.0, "plateau": 2.0, "type": "minecraft:trapezoid"}),
        ),
        (
            // `deviation` 0.05 encodes as `0.05` — `rivet-serialization` renders
            // the `Float.toString` literal (Gson's form), not the widened f64
            // decimal.
            FloatProvider::ClampedNormal(ClampedNormalFloat::of(0.5, 0.05, 0.0, 1.0)),
            json!({"mean": 0.5, "deviation": 0.05, "min": 0.0, "max": 1.0,
                   "type": "minecraft:clamped_normal"}),
        ),
    ];
    for (p, expected) in cases {
        let enc = codec
            .encode_start(&JsonOps::INSTANCE, &p)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(enc, expected, "encoded shape for {p:?}");
        let dec = codec
            .parse(&JsonOps::INSTANCE, &enc)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(dec, p, "round-trip for {p:?}");
    }
}

#[test]
fn float_provider_clamped_normal_deviation_paper_form_parses() {
    // Paper's serialization of a `ClampedNormalFloat` deviation uses the SHORT
    // `Float.toString` form (`0.05`), which `rivet-serialization`'s `JsonOps`
    // now emits on encode too. This pins that Paper's own JSON form parses to
    // the identical provider.
    let codec = float_provider_codec::<J>();
    let p = FloatProvider::ClampedNormal(ClampedNormalFloat::of(0.5, 0.05, 0.0, 1.0));
    let dec = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"mean": 0.5, "deviation": 0.05, "min": 0.0, "max": 1.0,
                   "type": "minecraft:clamped_normal"}),
        )
        .result()
        .cloned()
        .unwrap();
    assert_eq!(dec, p);
}

#[test]
fn float_provider_unknown_type_error_message() {
    let codec = float_provider_codec::<J>();
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:bogus", "value": 1.0}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some(
            "Failed to parse either. First: Not a number: {\"type\":\"minecraft:bogus\",\"value\":1.0}; \
             Second: Unknown registry key in ResourceKey[minecraft:root / minecraft:float_provider_type]: minecraft:bogus"
        )
    );
}

// ---------------------------------------------------------------------------
// Bound-codec validation (IntProviders.codec / FloatProviders.codec)
// ---------------------------------------------------------------------------

#[test]
fn int_provider_positive_codec_rejects_low() {
    let codec = positive_int_provider_codec::<J>();
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:uniform", "min_inclusive": -5, "max_inclusive": 5}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(err.as_deref(), Some("Value provider too low: 1 [-5-5]"));
}

#[test]
fn int_provider_bounds_codec_rejects_high() {
    let codec = int_provider_codec_with_bounds::<J>(-3, 3);
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:uniform", "min_inclusive": 0, "max_inclusive": 9}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(err.as_deref(), Some("Value provider too high: 3 [0-9]"));
}

#[test]
fn int_provider_non_negative_codec_accepts_ok_provider() {
    let codec = non_negative_int_provider_codec::<J>();
    let ok = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 2}),
        )
        .result()
        .cloned()
        .is_some();
    assert!(ok);
}

#[test]
fn int_provider_bounds_validate_on_encode_too() {
    let codec = positive_int_provider_codec::<J>();
    let p = IntProvider::Uniform(UniformInt::of(-5, 5));
    let err = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(err.as_deref(), Some("Value provider too low: 1 [-5-5]"));
}

#[test]
fn float_provider_bounds_codec_rejects_high() {
    let codec = float_provider_codec_with_bounds::<J>(0.0, 1.0);
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:uniform", "min_inclusive": 0.5, "max_exclusive": 2.0}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Value provider too high: 1.0 [0.5-2.0]")
    );
}

#[test]
fn float_provider_min_codec_rejects_low() {
    let codec = float_provider_codec_with_min::<J>(0.0);
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:uniform", "min_inclusive": -1.0, "max_exclusive": 1.0}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Value provider too low: 0.0 [-1.0-1.0]")
    );
}

#[test]
fn float_provider_bounds_validate_on_encode_too() {
    let codec = float_provider_codec_with_min::<J>(0.0);
    let p = FloatProvider::Uniform(UniformFloat::of(-1.0, 1.0));
    let err = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Value provider too low: 0.0 [-1.0-1.0]")
    );
}

// ---------------------------------------------------------------------------
// Concrete-provider validation messages (Paper's exact strings)
// ---------------------------------------------------------------------------

#[test]
fn uniform_int_rejects_max_below_min() {
    let codec = int_provider_codec::<J>();
    let err = codec
        .encode_start(
            &JsonOps::INSTANCE,
            &IntProvider::Uniform(UniformInt::of(5, 3)),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Max must be at least min, min_inclusive: 5, max_inclusive: 3")
    );
}

#[test]
fn trapezoid_int_rejects_plateau_too_wide() {
    let codec = int_provider_codec::<J>();
    let err = codec
        .encode_start(
            &JsonOps::INSTANCE,
            &IntProvider::Trapezoid(TrapezoidInt::of(0, 8, 9)),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Plateau can at most be the full span: [0, 8]")
    );
}

#[test]
fn weighted_list_int_rejects_empty_distribution() {
    let codec = int_provider_codec::<J>();
    let p = IntProvider::WeightedList(WeightedListInt::new(WeightedList::<IntProvider>::of()));
    let err = codec
        .encode_start(&JsonOps::INSTANCE, &p)
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Weighted list must contain at least one entry with non-zero weight")
    );
}

#[test]
fn clamped_normal_int_rejects_max_below_min() {
    let codec = int_provider_codec::<J>();
    let err = codec
        .encode_start(
            &JsonOps::INSTANCE,
            &IntProvider::ClampedNormal(ClampedNormalInt::of(0.0, 1.0, 5, 3)),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(err.as_deref(), Some("Max must be larger than min: [5, 3]"));
}

#[test]
fn clamped_normal_float_rejects_max_below_min() {
    let codec = float_provider_codec::<J>();
    let err = codec
        .encode_start(
            &JsonOps::INSTANCE,
            &FloatProvider::ClampedNormal(ClampedNormalFloat::of(0.0, 1.0, 5.0, 3.0)),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some("Max must be larger than min: [5.0, 3.0]")
    );
}

#[test]
fn uniform_float_max_exceeds_min_decode_error() {
    // UniformFloat's codec uses the record canonical `new` (no panic) and
    // validates via DataResult; a max<=min input is a decode error, never a
    // panic.
    let codec = float_provider_codec::<J>();
    let err = codec
        .parse(
            &JsonOps::INSTANCE,
            &json!({"type": "minecraft:uniform", "min_inclusive": 2.0, "max_exclusive": 1.0}),
        )
        .error_ref()
        .map(|e| e.message().to_string());
    assert_eq!(
        err.as_deref(),
        Some(
            "Failed to parse either. First: Not a number: {\"type\":\"minecraft:uniform\",\
             \"min_inclusive\":2.0,\"max_exclusive\":1.0}; \
             Second: Max must be larger than min, min: 2.0, max: 1.0"
        )
    );
}

#[test]
#[should_panic(expected = "Max must exceed min")]
fn uniform_float_of_rejects_max_leq_min() {
    // The static `of` reproduces Java's `IllegalArgumentException` exactly.
    let _ = UniformFloat::of(2.0, 1.0);
}

// ---------------------------------------------------------------------------
// toString / Display fidelity
// ---------------------------------------------------------------------------

#[test]
fn display_fidelity() {
    assert_eq!(UniformInt::of(0, 4).to_string(), "[0-4]");
    assert_eq!(BiasedToBottomInt::of(0, 4).to_string(), "[0-4]");
    assert_eq!(UniformFloat::of(0.5, 1.5).to_string(), "[0.5-1.5]");
    assert_eq!(
        TrapezoidFloat::of(0.0, 10.0, 2.0).to_string(),
        "trapezoid(2.0) in [0.0-10.0]"
    );
    assert_eq!(
        ClampedNormalFloat::of(0.5, 0.05, 0.0, 1.0).to_string(),
        "normal(0.5, 0.05) in [0.0-1.0]"
    );
    assert_eq!(
        TrapezoidInt::of(0, 8, 2).to_string(),
        "trapezoid(2) in [0-8]"
    );
    // `ClampedInt` is a record with no `toString` override, so Java uses the
    // auto-generated record toString with the RAW component values.
    assert_eq!(
        IntProvider::Clamped(ClampedInt::of(
            IntProvider::Uniform(UniformInt::of(-5, 5)),
            0,
            3
        ))
        .to_string(),
        "ClampedInt[source=[-5-5], minInclusive=0, maxInclusive=3]"
    );
}

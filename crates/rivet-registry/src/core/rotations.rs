//! `net.minecraft.core.Rotations` — a record of three floats representing an
//! entity's pose rotation.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/Rotations.java`.
//! The record canonical constructor normalizes each component: a finite `x`
//! becomes `x % 360.0F`, and `NaN`/`±Infinity` become `0.0F`. Paper adds
//! `createWithoutValidityChecks` for plugin compatibility, implemented with a
//! static `SKIP_VALIDATION` flag that bypasses the record constructor; the
//! flag has no observable effect on the returned value (it toggles once per
//! construction), so the port exposes a second constructor instead of a
//! mutable static.
//!
//! `equals`/`hashCode` follow the record spec for `float` components:
//! `Float.floatToIntBits` semantics — all `NaN` bit patterns are equal to each
//! other and `0.0F` differs from `-0.0F` (the JDK record spec uses
//! `Float.compare`, which is exactly that). The normalizing constructor can
//! still store `-0.0F` (`-0.0 % 360.0` is `-0.0`), so this is reachable through
//! the public API and must be ported, not derived.
//!
//! `CODEC` is `Codec.FLOAT.listOf().comapFlatMap(Util.fixedSize(input, 3) …)`
//! and is supportable here (rivet-serialization `float_codec`/`list`/
//! `comap_flat_map` + rivet-util `fixed_size`). RivetTodo(#126): `STREAM_CODEC`
//! (three big-endian floats) lives in `rivet-protocol`.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::java_float_format::java_float_to_string;
use rivet_util::util::fixed_size;
use std::hash::Hasher;
use std::sync::Arc;

/// `Rotations` — a `(x, y, z)` pose rotation in degrees.
///
/// `Eq` is sound despite the float components: the hand-written `PartialEq`
/// canonicalizes `NaN` to a single bit pattern, so equality is reflexive (a
/// Java record is a value type and a valid map key; Rust mirrors that).
#[derive(Clone, Copy, Debug)]
pub struct Rotations {
    x: f32,
    y: f32,
    z: f32,
}

impl Rotations {
    /// The record canonical constructor — normalizes each finite component to
    /// `v % 360.0F` and maps `NaN`/`±Infinity` to `0.0F`.
    pub fn new(x: f32, y: f32, z: f32) -> Rotations {
        Rotations {
            x: normalize_angle(x),
            y: normalize_angle(y),
            z: normalize_angle(z),
        }
    }

    /// `Rotations.createWithoutValidityChecks(float, float, float)` — Paper's
    /// plugin-compat constructor; stores the raw values with no normalization.
    pub fn create_without_validity_checks(x: f32, y: f32, z: f32) -> Rotations {
        Rotations { x, y, z }
    }

    /// `Rotations.x()`.
    pub fn x(&self) -> f32 {
        self.x
    }

    /// `Rotations.y()`.
    pub fn y(&self) -> f32 {
        self.y
    }

    /// `Rotations.z()`.
    pub fn z(&self) -> f32 {
        self.z
    }
}

/// The record constructor's per-component normalization
/// (`x % 360.0F` for finite `x`, else `0.0F`). Java `%` is the truncated
/// remainder — the sign of the result follows the dividend — matching Rust's
/// `%` for `f32`.
fn normalize_angle(v: f32) -> f32 {
    if !v.is_infinite() && !v.is_nan() {
        v % 360.0
    } else {
        0.0
    }
}

/// `Float.floatToIntBits(float)` — the raw bits with every `NaN` bit pattern
/// canonicalized to `0x7fc00000` (the JDK record `equals`/`hashCode` use
/// `floatToIntBits`, not the raw bits).
fn float_to_int_bits(v: f32) -> u32 {
    if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() }
}

impl PartialEq for Rotations {
    /// The record `equals` — `Float.compare` per component (== `floatToIntBits`
    /// equality: `NaN == NaN`, `0.0F != -0.0F`).
    fn eq(&self, other: &Self) -> bool {
        float_to_int_bits(self.x) == float_to_int_bits(other.x)
            && float_to_int_bits(self.y) == float_to_int_bits(other.y)
            && float_to_int_bits(self.z) == float_to_int_bits(other.z)
    }
}

/// `f32` has no `Eq` impl, so the marker is written by hand; it is sound
/// because the `PartialEq` above is reflexive (all `NaN` patterns collapse to
/// one bit pattern).
impl Eq for Rotations {}

impl Rotations {
    /// The Java record `hashCode` — `(Float.hashCode(x) * 31 +
    /// Float.hashCode(y)) * 31 + Float.hashCode(z)` in wrapping int arithmetic
    /// (javac's generated record `hashCode` seeds the accumulator at `0`, not
    /// `Objects.hash`'s `1`; verified against the JDK on the pinned toolchain),
    /// so there is no seed term.
    pub fn hash_code(&self) -> i32 {
        let hx = float_to_int_bits(self.x) as i32;
        let hy = float_to_int_bits(self.y) as i32;
        let hz = float_to_int_bits(self.z) as i32;
        hx.wrapping_mul(31)
            .wrapping_mul(31)
            .wrapping_add(hy.wrapping_mul(31))
            .wrapping_add(hz)
    }
}

impl std::hash::Hash for Rotations {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl std::fmt::Display for Rotations {
    /// The Java record `toString()` — `Rotations[x=…, y=…, z=…]` with each
    /// float via `Float.toString` (`java_float_to_string`, not Rust `{}`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rotations[x={}, y={}, z={}]",
            java_float_to_string(self.x),
            java_float_to_string(self.y),
            java_float_to_string(self.z)
        )
    }
}

/// `Rotations.CODEC` — `Codec.FLOAT.listOf().comapFlatMap(Util.fixedSize(input,
/// 3) -> new Rotations(...), rotations -> List.of(x, y, z))`.
pub fn rotations_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Rotations, Ops>> {
    codec::comap_flat_map::<Vec<f32>, Rotations, Ops>(
        codec::list(codec::float_codec::<Ops>()),
        Arc::new(|floats: &Vec<f32>| {
            fixed_size(floats, 3).map(|fs| Rotations::new(fs[0], fs[1], fs[2]))
        }),
        Arc::new(|rotations: &Rotations| vec![rotations.x(), rotations.y(), rotations.z()]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use std::hash::Hash;

    #[test]
    fn new_normalizes_finite_and_non_finite() {
        assert_eq!(
            Rotations::new(720.0, 45.0, -360.0),
            Rotations::create_without_validity_checks(0.0, 45.0, -0.0)
        );
        assert_eq!(
            Rotations::new(450.0, -450.0, 0.0),
            Rotations::create_without_validity_checks(90.0, -90.0, 0.0)
        );
        // NaN/±Infinity -> 0.0
        let r = Rotations::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
        assert_eq!(r.x(), 0.0);
        assert_eq!(r.y(), 0.0);
        assert_eq!(r.z(), 0.0);
    }

    #[test]
    fn create_without_validity_checks_keeps_raw_values() {
        let r = Rotations::create_without_validity_checks(f32::NAN, 360.0, 90.0);
        assert!(r.x().is_nan());
        assert_eq!(r.y(), 360.0);
        assert_eq!(r.z(), 90.0);
    }

    #[test]
    fn equals_matches_float_compare_semantics() {
        // Normal construction can store -0.0 (`-0.0 % 360.0 == -0.0`); the JDK
        // record `equals` distinguishes it from +0.0.
        assert_ne!(
            Rotations::new(0.0, 0.0, 0.0),
            Rotations::new(-0.0, 0.0, 0.0)
        );
        assert_eq!(
            Rotations::create_without_validity_checks(-0.0, 0.0, 0.0),
            Rotations::create_without_validity_checks(-0.0, 0.0, 0.0)
        );
        // NaN is equal to itself through createWithoutValidityChecks.
        let nan = Rotations::create_without_validity_checks(f32::NAN, 1.0, 2.0);
        let nan2 = Rotations::create_without_validity_checks(f32::NAN, 1.0, 2.0);
        assert_eq!(nan, nan2);
        assert_ne!(
            Rotations::create_without_validity_checks(1.0, 2.0, 3.0),
            Rotations::create_without_validity_checks(1.0, 2.0, 4.0)
        );
    }

    #[test]
    fn hash_code_matches_java_record() {
        // javac's generated record `hashCode` = `(hx*31 + hy)*31 + hz`
        // (accumulator seeded at 0), where `h` = `Float.hashCode` =
        // `floatToIntBits`. Values pinned against the JDK on the pinned
        // toolchain.
        // floatToIntBits(1.0f) = 0x3F800000 = 1065353216, (2.0f) = 0x40000000,
        // (3.0f) = 0x40400000.
        assert_eq!(Rotations::new(1.0, 2.0, 3.0).hash_code(), 1606418432);
        // All-zero rotations.
        assert_eq!(Rotations::new(0.0, 0.0, 0.0).hash_code(), 0);
        // NaN components canonicalize to 0x7fc00000 (wrapping arithmetic).
        let nan = Rotations::create_without_validity_checks(f32::NAN, 0.0, 0.0);
        assert_eq!(nan.hash_code(), -1883242496);
        // Negative zero: floatToIntBits(-0.0) = 0x80000000 = i32::MIN.
        let negz = Rotations::create_without_validity_checks(-0.0, 0.0, 0.0);
        assert_eq!(negz.hash_code(), -2147483648);
        // Through the normalizing constructor: (720, 45, -360) stores
        // (0.0, 45.0, -0.0), whose hash is pinned directly.
        assert_eq!(Rotations::new(720.0, 45.0, -360.0).hash_code(), -2075394048);
    }

    #[test]
    fn hash_matches_equality_contract() {
        let h = |r: &Rotations| {
            let mut s = std::collections::hash_map::DefaultHasher::new();
            r.hash(&mut s);
            s.finish()
        };
        let a = Rotations::new(1.0, 2.0, 3.0);
        let b = Rotations::new(1.0, 2.0, 3.0);
        assert_eq!(h(&a), h(&b));
        // +0.0 and -0.0 are unequal and hash differently.
        assert_ne!(
            h(&Rotations::new(0.0, 0.0, 0.0)),
            h(&Rotations::new(-0.0, 0.0, 0.0))
        );
        // NaN rotations are equal and hash equally.
        let nan1 = Rotations::create_without_validity_checks(f32::NAN, 5.0, 6.0);
        let nan2 = Rotations::create_without_validity_checks(f32::NAN, 5.0, 6.0);
        assert_eq!(h(&nan1), h(&nan2));
    }

    #[test]
    fn display_matches_java_record_to_string() {
        assert_eq!(
            Rotations::new(1.0, 2.0, 3.0).to_string(),
            "Rotations[x=1.0, y=2.0, z=3.0]"
        );
        assert_eq!(
            Rotations::new(0.0, -0.0, 720.0).to_string(),
            "Rotations[x=0.0, y=-0.0, z=0.0]"
        );
    }

    #[test]
    fn codec_roundtrips() {
        let ops = JsonOps::INSTANCE;
        let codec = rotations_codec::<JsonOps>();
        let r = Rotations::new(1.0, 2.0, 3.0);
        let encoded = codec.encode_start(&ops, &r).get_or_throw("encode").clone();
        assert_eq!(
            encoded,
            ops.create_list(vec![
                ops.create_float(1.0),
                ops.create_float(2.0),
                ops.create_float(3.0)
            ])
        );
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0, r);
    }

    #[test]
    fn codec_normalizes_through_the_constructor() {
        // Java decodes `Codec.FLOAT.listOf` into `new Rotations(...)`, which
        // runs the canonical constructor — so out-of-range angles are wrapped.
        let ops = JsonOps::INSTANCE;
        let codec = rotations_codec::<JsonOps>();
        let input = ops.create_list(vec![
            ops.create_float(720.0),
            ops.create_float(45.0),
            ops.create_float(-360.0),
        ]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, Rotations::new(720.0, 45.0, -360.0));
        assert_eq!(decoded.0.x(), 0.0);
        assert_eq!(decoded.0.z(), -0.0);
    }

    #[test]
    fn codec_rejects_wrong_length_with_fixed_size_message() {
        let ops = JsonOps::INSTANCE;
        let codec = rotations_codec::<JsonOps>();
        for len in [0usize, 1, 2, 4, 5] {
            let input = ops.create_list((0..len).map(|i| ops.create_float(i as f32)).collect());
            let result = codec.decode(&ops, &input);
            assert!(result.result().is_none(), "length {len} should fail");
            assert_eq!(
                result.error_ref().map(|e| e.message().to_string()),
                Some("Input is not a list of 3 elements".to_string())
            );
        }
    }

    #[test]
    fn codec_encode_preserves_negative_zero() {
        // `Rotations::new(720.0, 45.0, -360.0)` stores `(0.0, 45.0, -0.0)`:
        // the canonical constructor's `-360.0 % 360.0` is `-0.0`, and the sign
        // must survive `CODEC` encoding. JsonOps keeps the sign bit in its
        // `serde_json` numbers (read back via `as_f64`), unlike NbtOps whose
        // `create_float` goes through `FloatTag::value_of` and collapses `-0.0`
        // to `+0.0`. The structural equality against `create_float(-0.0)` alone
        // cannot catch a `+0.0` emission — `serde_json::Number` compares floats
        // with IEEE `==`, so `-0.0 == 0.0` — hence the load-bearing sign check.
        let ops = JsonOps::INSTANCE;
        let codec = rotations_codec::<JsonOps>();
        let r = Rotations::new(720.0, 45.0, -360.0);
        let encoded = codec.encode_start(&ops, &r).get_or_throw("encode").clone();
        assert_eq!(
            encoded,
            ops.create_list(vec![
                ops.create_float(0.0),
                ops.create_float(45.0),
                ops.create_float(-0.0)
            ])
        );
        assert!(
            encoded[2].as_f64().is_some_and(|f| f.is_sign_negative()),
            "encoded z component must keep the -0.0 sign bit, got {:?}",
            encoded[2].as_f64()
        );
    }
}

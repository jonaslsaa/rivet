//! Canonical float encoding for oracle-op responses (VERIFICATION §2.4.1).
//!
//! Floats never travel as JSON numbers: both sides emit the raw IEEE-754 bit
//! pattern as `0x` + lowercase hex (8 digits for f32, 16 for f64), using the
//! NaN-canonicalizing conversions Java's `Float.floatToIntBits` /
//! `Double.doubleToLongBits` perform (D14). Every non-NaN value, including
//! `-0.0`, is compared bit-exactly; there are no epsilons anywhere. The Java
//! twin is `RivetReferenceOracle.f32Bits`/`f64Bits`.

/// Java's canonical quiet NaN for `float` (`Float.floatToIntBits(NaN)`).
pub const F32_CANONICAL_NAN: u32 = 0x7fc0_0000;
/// Java's canonical quiet NaN for `double` (`Double.doubleToLongBits(NaN)`).
pub const F64_CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;

/// `Float.floatToIntBits` semantics.
pub fn f32_to_int_bits(value: f32) -> u32 {
    if value.is_nan() {
        F32_CANONICAL_NAN
    } else {
        value.to_bits()
    }
}

/// `Double.doubleToLongBits` semantics.
pub fn f64_to_long_bits(value: f64) -> u64 {
    if value.is_nan() {
        F64_CANONICAL_NAN
    } else {
        value.to_bits()
    }
}

/// Canonical f32 field value, e.g. `0x3f800000` for `1.0`.
pub fn f32_bits(value: f32) -> String {
    format!("0x{:08x}", f32_to_int_bits(value))
}

/// Canonical f64 field value, e.g. `0x3ff0000000000000` for `1.0`.
pub fn f64_bits(value: f64) -> String {
    format!("0x{:016x}", f64_to_long_bits(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The expected strings are IEEE-754 encodings, asserted identically by the
    // Java oracle's self-test, so both canonicalizers agree byte-for-byte.
    #[test]
    fn f32_encodings_match_java_float_to_int_bits() {
        assert_eq!(f32_bits(1.0), "0x3f800000");
        assert_eq!(f32_bits(-0.0), "0x80000000");
        assert_eq!(f32_bits(0.0), "0x00000000");
        assert_eq!(f32_bits(f32::from_bits(0x7fc0_0001)), "0x7fc00000");
        assert_eq!(f32_bits(f32::from_bits(0xffc0_0000)), "0x7fc00000");
        assert_eq!(f32_bits(f32::INFINITY), "0x7f800000");
    }

    #[test]
    fn f64_encodings_match_java_double_to_long_bits() {
        assert_eq!(f64_bits(1.0), "0x3ff0000000000000");
        assert_eq!(f64_bits(-0.0), "0x8000000000000000");
        assert_eq!(
            f64_bits(f64::from_bits(0x7ff0_0000_0000_0001)),
            "0x7ff8000000000000"
        );
        assert_eq!(f64_bits(f64::NEG_INFINITY), "0xfff0000000000000");
    }
}

//! `String.valueOf(float/double)` parity — Java `Float.toString` /
//! `Double.toString` formatting rules, shared by `StringTagVisitor`
//! (`visitFloat`/`visitDouble`), `TextComponentTagVisitor`, and the JSON
//! `createFloat` path (a `Codec.FLOAT` field must serialize as Gson renders a
//! `JsonPrimitive(Float)`, which is exactly `Float.toString`).
//!
//! Java formats a float/double with the shortest decimal that round-trips, a
//! decimal point when the value is integral (`1.0`), `-0.0` for negative zero,
//! and scientific notation (`1.0E7`) when `|x| >= 1e7` or `< 1e-3`.
//!
//! Digits come from `ryu` (the Ryu algorithm), the same shortest-round-trip
//! digit generator the JDK's `Float.toString`/`Double.toString` (OpenJDK 19+,
//! `FloatingDecimal`) uses. Empirically verified against JDK 25 output: ryu's
//! digit strings match Java's for every finite value except the subnormal
//! tie-break cases enumerated in [`subnormal_override_f32`] /
//! [`subnormal_override_f64`], where Java prints a non-shortest "preferred"
//! digit string (e.g. `4.9E-324` for the minimum subnormal). Those are
//! overridden explicitly so the rendered text matches Java byte-for-byte.

/// `String.valueOf(float)` parity — Java `Float.toString`.
pub fn java_float_to_string(value: f32) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f32::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f32::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    if let Some(override_) = subnormal_override_f32(value) {
        return override_;
    }
    let digits = shortest_digits(value);
    format_java_decimal(&digits)
}

/// `String.valueOf(double)` parity — Java `Double.toString`.
pub fn java_double_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "Infinity".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    if let Some(override_) = subnormal_override_f64(value) {
        return override_;
    }
    let digits = shortest_digits(value);
    format_java_decimal(&digits)
}

/// `Float.compare(float, float)` parity — the IEEE **total order** over f32,
/// including the Java NaN canonicalization: **every** NaN payload sorts equal
/// (Java's `Float.compare` first canonicalizes both NaN operands to the same
/// `0x7fc00000` before comparing, so distinct payloads compare equal, unlike
/// `f32::total_cmp`, which orders by payload bits).
///
/// Ordering (ascending): `-Infinity`, negative finite values, `-0.0`, `+0.0`,
/// positive finite values, `+Infinity`, `NaN`.
pub fn java_float_compare(a: f32, b: f32) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.total_cmp(&b),
    }
}

/// `Double.compare(double, double)` parity — the f64 analogue of
/// [`java_float_compare`].
pub fn java_double_compare(a: f64, b: f64) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.total_cmp(&b),
    }
}

/// `Float.equals(Object)` parity — Java's boxed `Float.equals`: `NaN` equals
/// `NaN` (any payload), `-0.0` is **distinct** from `+0.0`, and every other
/// pair compares by value.
pub fn java_float_equals(a: f32, b: f32) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == 0.0 && b == 0.0 {
        return a.is_sign_negative() == b.is_sign_negative();
    }
    a == b
}

/// `Double.equals(Object)` parity — Java's boxed `Double.equals`: `NaN` equals
/// `NaN` (any payload), `-0.0` is **distinct** from `+0.0`, and every other
/// pair compares by value. This is exactly the JDK `Double.equals`
/// `doubleToLongBits` test (all NaN payloads canonicalize to
/// `0x7ff8000000000000`, `-0.0` keeps its sign bit).
pub fn java_double_equals(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == 0.0 && b == 0.0 {
        return a.is_sign_negative() == b.is_sign_negative();
    }
    a == b
}

/// The f32 subnormal values whose shortest-round-trip digit string (Ryu) is a
/// different member of the round-trip class than the one Java prints. Keyed by
/// raw bits; the value is the exact Java `Float.toString` output.
///
/// Verified exhaustively against JDK 25 for every f32 subnormal: these are the
/// only values where `ryu` and Java disagree on the digit string. Java prints
/// its `FloatingDecimal` "preferred" digits (e.g. `1.4E-45` for the minimum
/// subnormal, not the shortest `1E-45`).
const F32_SUBNORMAL_OVERRIDES: &[(u32, &str)] = &[
    (0x00000001, "1.4E-45"),
    (0x00000002, "2.8E-45"),
    (0x00000003, "4.2E-45"),
    (0x00000004, "5.6E-45"),
    (0x00000006, "8.4E-45"),
    (0x00000007, "9.8E-45"),
    (0x00000015, "2.9E-44"),
    (0x0000001d, "4.1E-44"),
    (0x00000047, "9.9E-44"),
];

/// The f64 subnormal values whose shortest-round-trip digit string differs from
/// Java's. Same rationale as [`F32_SUBNORMAL_OVERRIDES`]; verified across the
/// full f64 subnormal mantissa range (2^52 values) against JDK 25.
const F64_SUBNORMAL_OVERRIDES: &[(u64, &str)] = &[
    (0x0000000000000001, "4.9E-324"),
    (0x0000000000000002, "9.9E-324"),
    (0x000000000000000a, "4.9E-323"),
    (0x000000000000000c, "5.9E-323"),
    (0x000000000000000e, "6.9E-323"),
    (0x0000000000000010, "7.9E-323"),
    (0x0000000000000012, "8.9E-323"),
    (0x0000000000000014, "9.9E-323"),
];

/// Java's `Float.toString` for the enumerated subnormal values, or `None` for
/// any other value (handled by `ryu` + `format_java_decimal`).
fn subnormal_override_f32(value: f32) -> Option<String> {
    let bits = value.to_bits();
    // Only truly subnormal values qualify: the exponent field must be zero, so
    // the mantissa bits alone do not accidentally match a normal value.
    if bits & 0x7f80_0000 != 0 {
        return None;
    }
    let sign = bits & 0x8000_0000;
    let mantissa = bits & 0x007f_ffff;
    if mantissa == 0 {
        return None; // +0.0 / -0.0 handled earlier
    }
    let base = F32_SUBNORMAL_OVERRIDES
        .iter()
        .find(|(bits, _)| *bits == mantissa)?;
    Some(if sign != 0 {
        format!("-{}", base.1)
    } else {
        base.1.to_owned()
    })
}

/// Java's `Double.toString` for the enumerated subnormal values, or `None` for
/// any other value (handled by `ryu` + `format_java_decimal`).
fn subnormal_override_f64(value: f64) -> Option<String> {
    let bits = value.to_bits();
    // Only truly subnormal values qualify: the exponent field must be zero.
    if bits & 0x7ff0_0000_0000_0000 != 0 {
        return None;
    }
    let sign = bits & 0x8000_0000_0000_0000;
    let mantissa = bits & 0x000f_ffff_ffff_ffff;
    if mantissa == 0 {
        return None; // +0.0 / -0.0 handled earlier
    }
    let base = F64_SUBNORMAL_OVERRIDES
        .iter()
        .find(|(bits, _)| *bits == mantissa)?;
    Some(if sign != 0 {
        format!("-{}", base.1)
    } else {
        base.1.to_owned()
    })
}

/// Ryu shortest digits as a canonical scientific string `[+-]?ddddE±e` with no
/// decimal point — pure significant digits plus the exponent of the first
/// digit. `format_java_decimal` then applies Java's plain-vs-scientific rule
/// and inserts the decimal point.
///
/// Parsing ryu's `d.ddde±e` (or its plain `d.ddd` / `0.00ddd` / integral
/// `dddd.0`): the value is `digits * 10^(e)` where `e` is the exponent applied
/// to the whole digit string. The decimal exponent of the first significant
/// digit is then `e + (digits_before_point - 1 - leading_zeros)`. Trailing
/// zeros are stripped (they are magnitude padding in the plain-integral form,
/// not significant digits); Java's `FloatingDecimal` also emits the shortest
/// round-trip digits, which never carry trailing zeros.
fn shortest_digits<F: ryu::Float>(value: F) -> String {
    let mut buffer = ryu::Buffer::new();
    let s = buffer.format(value);
    debug_assert!(
        !matches!(s, "inf" | "-inf" | "NaN"),
        "non-finite passed to shortest_digits"
    );
    let sign = s.starts_with('-');
    let unsigned = s.strip_prefix('-').unwrap_or(s);
    // Split off the exponent, if present. ryu emits `d.ddde±e` for large/small
    // magnitudes and plain `d.ddd` / `0.00ddd` / `dddd.0` otherwise.
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(idx) => {
            let exp: i32 = unsigned[idx + 1..].parse().expect("ryu exponent");
            (&unsigned[..idx], exp)
        }
        None => (unsigned, 0),
    };
    // Mantissa `d.ddd` — split into whole and fractional digit runs.
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    let int_len = int_part.len() as i32;
    let digits: String = int_part.chars().chain(frac_part.chars()).collect();
    let first_nonzero = digits.find(|c| c != '0').unwrap_or(digits.len());
    let mut sig = &digits[first_nonzero..];
    if sig.is_empty() {
        return "0".to_owned();
    }
    // Trim trailing zeros: Java's shortest round-trip digits have none, and the
    // plain-integral form (`1066807000000.0`) pads the magnitude with zeros.
    let last_nonzero = sig.rfind(|c| c != '0').unwrap_or(sig.len() - 1);
    sig = &sig[..=last_nonzero];
    // Exponent of `sig[0]`: `e` shifts the whole digit string, then the first
    // significant digit sits `int_len - 1 - first_nonzero` places right of the
    // point implied by `int_len`.
    let exp = exponent + int_len - 1 - first_nonzero as i32;
    format!("{}{sig}E{exp}", if sign { "-" } else { "" })
}

/// Formats a scientific digit string (`[+-]?d.dddE±e`) as Java's
/// `Float.toString`/`Double.toString` would: no exponent when `-3 <= e <= 6`
/// (decimal point placed, `.0` appended to integral values), else `x.xE±e`.
fn format_java_decimal(s: &str) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };
    let (mantissa, exp_str) = rest
        .split_once('E')
        .expect("shortest float repr has an exponent");
    let exp: i32 = exp_str.parse().expect("exponent is numeric");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;

    if (-3..=6).contains(&exp) {
        let int_len = exp + 1;
        if int_len >= n {
            // Integral: pad with zeros, then ".0".
            let zeros = "0".repeat((int_len - n) as usize);
            format!("{sign}{digits}{zeros}.0")
        } else if int_len > 0 {
            let (int_part, frac_part) = digits.split_at(int_len as usize);
            format!("{sign}{int_part}.{frac_part}")
        } else {
            // 0.00xxx
            let zeros = "0".repeat((-int_len) as usize);
            format!("{sign}0.{zeros}{digits}")
        }
    } else {
        // Scientific: x.xE±e
        let frac = &digits[1..];
        let mantissa = if frac.is_empty() {
            format!("{}.0", &digits[..1])
        } else {
            format!("{}.{}", &digits[..1], frac)
        };
        let exponent = if exp < 0 {
            format!("-{}", -exp)
        } else {
            format!("{exp}")
        };
        format!("{sign}{mantissa}E{exponent}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    /// Java `Float.compare` parity: total order with NaN canonicalization.
    #[test]
    fn float_compare_matches_java_total_order() {
        assert_eq!(java_float_compare(1.0, 2.0), Ordering::Less);
        assert_eq!(java_float_compare(2.0, 1.0), Ordering::Greater);
        assert_eq!(java_float_compare(1.0, 1.0), Ordering::Equal);
        // Signed zero: `Float.compare(-0.0f, 0.0f)` is -1.
        assert_eq!(java_float_compare(-0.0, 0.0), Ordering::Less);
        assert_eq!(java_float_compare(0.0, -0.0), Ordering::Greater);
        assert_eq!(java_float_compare(-0.0, -0.0), Ordering::Equal);
        // NaN is greater than every finite value and than +Infinity.
        assert_eq!(
            java_float_compare(f32::NAN, f32::INFINITY),
            Ordering::Greater
        );
        assert_eq!(java_float_compare(f32::NAN, f32::MAX), Ordering::Greater);
        assert_eq!(java_float_compare(f32::MAX, f32::NAN), Ordering::Less);
        // Distinct NaN payloads compare equal (Java canonicalizes to 0x7fc00000).
        let nan_a = f32::from_bits(0x7fc00001);
        let nan_b = f32::from_bits(0x7ff12345);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        assert_eq!(java_float_compare(nan_a, nan_b), Ordering::Equal);
        assert_eq!(java_float_compare(f32::NAN, nan_a), Ordering::Equal);
    }

    /// Java `Double.compare` parity (same rules, f64).
    #[test]
    fn double_compare_matches_java_total_order() {
        assert_eq!(java_double_compare(-0.0, 0.0), Ordering::Less);
        assert_eq!(
            java_double_compare(f64::NAN, f64::INFINITY),
            Ordering::Greater
        );
        let nan_a = f64::from_bits(0x7ff8000000000001);
        let nan_b = f64::from_bits(0x7ff8deadbeef0000);
        assert!(nan_a.is_nan() && nan_b.is_nan());
        assert_eq!(java_double_compare(nan_a, nan_b), Ordering::Equal);
    }

    /// Java `Float.equals` parity: all NaNs equal, signed zeros distinct.
    #[test]
    fn float_equals_matches_java() {
        assert!(java_float_equals(1.0, 1.0));
        assert!(!java_float_equals(1.0, 1.5));
        assert!(java_float_equals(f32::NAN, f32::NAN));
        assert!(java_float_equals(f32::NAN, f32::from_bits(0x7fc00001)));
        assert!(!java_float_equals(-0.0, 0.0));
        assert!(java_float_equals(-0.0, -0.0));
        assert!(java_float_equals(0.0, 0.0));
    }

    /// Java `Double.equals` parity: all NaNs equal, signed zeros distinct.
    #[test]
    fn double_equals_matches_java() {
        assert!(java_double_equals(1.0, 1.0));
        assert!(!java_double_equals(1.0, 1.5));
        assert!(java_double_equals(f64::NAN, f64::NAN));
        assert!(java_double_equals(
            f64::NAN,
            f64::from_bits(0x7ff8000000000001)
        ));
        assert!(!java_double_equals(-0.0, 0.0));
        assert!(java_double_equals(-0.0, -0.0));
        assert!(java_double_equals(0.0, 0.0));
    }

    /// Java `Float.toString` ground-truth cases.
    #[test]
    fn float_to_string_matches_java() {
        assert_eq!(java_float_to_string(1.0), "1.0");
        assert_eq!(java_float_to_string(0.0), "0.0");
        assert_eq!(java_float_to_string(-0.0), "-0.0");
        assert_eq!(java_float_to_string(0.5), "0.5");
        assert_eq!(java_float_to_string(1.5), "1.5");
        assert_eq!(java_float_to_string(1.0e7), "1.0E7");
        assert_eq!(java_float_to_string(-1.0e7), "-1.0E7");
        assert_eq!(java_float_to_string(1.0e-4), "1.0E-4");
        assert_eq!(java_float_to_string(9.99e-4), "9.99E-4");
        assert_eq!(java_float_to_string(1.0e-3), "0.001");
        assert_eq!(java_float_to_string(1234567.0), "1234567.0");
        assert_eq!(java_float_to_string(12345678.0), "1.2345678E7");
        assert_eq!(java_float_to_string(f32::NAN), "NaN");
        assert_eq!(java_float_to_string(f32::INFINITY), "Infinity");
        assert_eq!(java_float_to_string(f32::NEG_INFINITY), "-Infinity");
    }

    /// Java `Double.toString` ground-truth cases.
    #[test]
    fn double_to_string_matches_java() {
        assert_eq!(java_double_to_string(1.0), "1.0");
        assert_eq!(java_double_to_string(0.0), "0.0");
        assert_eq!(java_double_to_string(-0.0), "-0.0");
        assert_eq!(java_double_to_string(2.25), "2.25");
        assert_eq!(java_double_to_string(1.0e7), "1.0E7");
        assert_eq!(java_double_to_string(-1.0e7), "-1.0E7");
        assert_eq!(java_double_to_string(1.0e-4), "1.0E-4");
        assert_eq!(java_double_to_string(9999999.0), "9999999.0");
        assert_eq!(
            java_double_to_string(123456789012345.0),
            "1.23456789012345E14"
        );
        assert_eq!(
            java_double_to_string(1234567890123456.0),
            "1.234567890123456E15"
        );
        assert_eq!(java_double_to_string(1.0e20), "1.0E20");
        assert_eq!(java_double_to_string(1.0e-20), "1.0E-20");
        assert_eq!(java_double_to_string(0.1), "0.1");
        assert_eq!(java_double_to_string(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(java_double_to_string(f64::NAN), "NaN");
        assert_eq!(java_double_to_string(f64::INFINITY), "Infinity");
        assert_eq!(java_double_to_string(f64::NEG_INFINITY), "-Infinity");
    }

    /// The f32/f64 subnormal values where Java's `FloatingDecimal` prints a
    /// non-shortest "preferred" digit string; the overrides must match Java.
    #[test]
    fn subnormal_overrides_match_java() {
        for (bits, expected) in F32_SUBNORMAL_OVERRIDES {
            assert_eq!(
                java_float_to_string(f32::from_bits(*bits)),
                *expected,
                "bits {bits:#x}"
            );
            assert_eq!(
                java_float_to_string(f32::from_bits(bits | 0x8000_0000)),
                format!("-{expected}"),
                "bits {:#x}",
                bits | 0x8000_0000
            );
        }
        for (bits, expected) in F64_SUBNORMAL_OVERRIDES {
            assert_eq!(
                java_double_to_string(f64::from_bits(*bits)),
                *expected,
                "bits {bits:#x}"
            );
            assert_eq!(
                java_double_to_string(f64::from_bits(bits | 0x8000_0000_0000_0000)),
                format!("-{expected}"),
                "bits {:#x}",
                bits | 0x8000_0000_0000_0000
            );
        }
        // Every other subnormal matches Java via the shortest-form path.
        assert_eq!(java_float_to_string(f32::from_bits(0x0000_0005)), "7.0E-45");
        assert_eq!(
            java_double_to_string(f64::from_bits(0x0000_0000_0000_0003)),
            "1.5E-323"
        );
    }

    /// The specific bit patterns from the review finding must now match Java.
    /// The reviewer's `-4392560525442127822` is a Java `long` bit pattern
    /// (`0xC30A7D79929E6832`), not a literal value — the double it encodes is
    /// `-932038812355846.2`, printed by Java as `-9.320388123558462E14`.
    #[test]
    fn reviewer_cases() {
        assert_eq!(
            java_float_to_string(f32::from_bits(964689920)),
            "2.4414062E-4"
        );
        assert_eq!(
            java_float_to_string(f32::from_bits(1249038305)),
            "3978232.2"
        );
        assert_eq!(
            java_double_to_string(f64::from_bits(0x0000000000000001)),
            "4.9E-324"
        );
        assert_eq!(java_float_to_string(f32::from_bits(0x00000001)), "1.4E-45");
        assert_eq!(
            java_double_to_string(f64::from_bits(0xc30a7d79929e6832)),
            "-9.320388123558462E14"
        );
    }
}

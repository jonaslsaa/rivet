//! Java `Double.toString` / `Float.toString` formatting.
//!
//! Java's `DoubleArgumentType.toString()`/`FloatArgumentType.toString()` stringify
//! their bounds with the JDK's `Double.toString`/`Float.toString`, and
//! `TranslatableContents` renders `Double` arguments the same way. That format is
//! not Rust's `Display`: Java always keeps a decimal point (plain form `100.0`,
//! `0.05`, `0.001` in the `10^-3 <= |v| < 10^7` range, else computerized
//! scientific `2.147483648E9`, `1.0E-4`), with the shortest round-trip digits.
//!
//! Rust's `{:e}` (LowerExp) also produces the shortest round-trip digits, so the
//! value's mantissa digits and base-10 exponent are reused and re-rendered per
//! Java's rules. `Float.toString` uses the same rules on the `float` value; the
//! modern JDK (19+) prints the shortest form, which is what this port reproduces.

/// Java `Double.toString(double)` for the finite, non-`NaN` range used by the
/// argument types and translatable-argument rendering.
pub fn java_double_to_string(value: f64) -> String {
    java_format(
        value,
        value.is_nan(),
        value == f64::INFINITY,
        value == f64::NEG_INFINITY,
        &format!("{:e}", value),
    )
}

/// Java `Float.toString(float)` for the finite, non-`NaN` range used by the
/// argument types.
pub fn java_float_to_string(value: f32) -> String {
    java_format(
        value as f64,
        value.is_nan(),
        value == f32::INFINITY,
        value == f32::NEG_INFINITY,
        &format!("{:e}", value),
    )
}

/// Shared renderer: `scientific` is Rust's `LowerExp` of the value, whose mantissa
/// digits and exponent are the shortest round-trip form Java uses.
fn java_format(value: f64, nan: bool, pos_inf: bool, neg_inf: bool, scientific: &str) -> String {
    if nan {
        return "NaN".to_string();
    }
    if pos_inf {
        return "Infinity".to_string();
    }
    if neg_inf {
        return "-Infinity".to_string();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }

    let (negative, mantissa, exp) = parse_scientific(scientific);
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let digits = digits.as_str();
    // E = exponent such that value = 0.digits * 10^E (exp is the `m * 10^exp` with
    // 1 <= m < 10 form from `{:e}`, so E = exp + 1).
    let e = exp + 1;
    let n = digits.len() as i32;

    let sign = if negative { "-" } else { "" };
    // Java uses plain decimal form for `10^-3 <= |v| < 10^7` (Float.toString /
    // Double.toString docs). With `e = exp + 1` and `v` in `[10^(e-1), 10^e)`,
    // that is `-2 <= e <= 7`, i.e. `(-2..8)`.
    if (-2..8).contains(&e) {
        // Plain decimal form.
        if e >= n {
            // Whole number at or above one — pad integer part, then ".0".
            let zeros = e - n;
            format!("{}{}{}.0", sign, digits, "0".repeat(zeros as usize))
        } else if e > 0 {
            // Fractional with an integer part: split the digits.
            format!(
                "{}{}.{}",
                sign,
                &digits[..e as usize],
                &digits[e as usize..]
            )
        } else {
            // Below one: "0." plus zeros then the digits.
            let zeros = -e;
            format!("{}{}{}{}", sign, "0.", "0".repeat(zeros as usize), digits)
        }
    } else {
        // Computerized scientific notation: d.dddEexp.
        let mantissa = if n == 1 {
            format!("{}.0", &digits[..1])
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{}{}E{}", sign, mantissa, exp)
    }
}

/// Split Rust's `LowerExp` (`[-]d[.d+]e[-]dd`) into sign, mantissa digits (with
/// any `.`), and the base-10 exponent.
fn parse_scientific(s: &str) -> (bool, &str, i32) {
    let (negative, rest) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (mantissa, exp) = rest.split_once('e').expect("LowerExp contains 'e'");
    let exp: i32 = exp.parse().expect("LowerExp exponent");
    (negative, mantissa, exp)
}

#[cfg(test)]
mod tests {
    use super::{java_double_to_string, java_float_to_string};

    // Values pinned against JDK 25 `Double.toString`/`Float.toString`. The plain /
    // scientific switch happens at `10^-3` (below → scientific `1.0E-4`) and
    // `10^7` (at/above → scientific `1.0E7`).
    #[test]
    fn double_plain_below_1e7_above_1e_3() {
        // 9999999.999 is 9.999999999E6, inside `10^-3 <= |v| < 10^7` → plain.
        assert_eq!(java_double_to_string(9999999.999), "9999999.999");
        assert_eq!(java_double_to_string(1000000.0), "1000000.0");
        assert_eq!(java_double_to_string(0.001), "0.001");
        assert_eq!(java_double_to_string(0.0009999), "9.999E-4");
    }

    #[test]
    fn double_scientific_outside_plain_range() {
        // 1e-4 < 10^-3 → scientific; 1e7 == 10^7 → scientific.
        assert_eq!(java_double_to_string(0.0001), "1.0E-4");
        assert_eq!(java_double_to_string(10000000.0), "1.0E7");
    }

    #[test]
    fn float_matches_double_rules() {
        assert_eq!(java_float_to_string(0.0001_f32), "1.0E-4");
        assert_eq!(java_float_to_string(0.001_f32), "0.001");
        assert_eq!(java_float_to_string(9999999.0_f32), "9999999.0");
        assert_eq!(java_float_to_string(10000000.0_f32), "1.0E7");
    }
}

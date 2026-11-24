//! The Guava `com.google.common.math` members used by
//! `net.minecraft.world.phys.shapes`: `DoubleMath.fuzzyEquals` and
//! `IntMath.gcd`.

/// `DoubleMath.fuzzyEquals(double a, double b, double tolerance)` —
/// `IllegalArgumentException` (a panic) for a negative or `NaN` tolerance.
pub fn fuzzy_equals(a: f64, b: f64, tolerance: f64) -> bool {
    assert!(tolerance >= 0.0, "tolerance ({tolerance}) must be >= 0");
    (a - b).abs() <= tolerance || a == b || (a.is_nan() && b.is_nan())
}

/// `IntMath.gcd(int a, int b)` — Euclid's algorithm; `IllegalArgumentException`
/// (a panic) for a negative argument; `gcd(0, 0) == 0`.
pub fn gcd(a: i32, b: i32) -> i32 {
    assert!(a >= 0, "a (-{}) must be >= 0", -(a as i64));
    assert!(b >= 0, "b (-{}) must be >= 0", -(b as i64));
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_equals_matches_guava() {
        assert!(fuzzy_equals(1.0, 1.0 + 1e-8, 1e-7));
        assert!(!fuzzy_equals(1.0, 1.0 + 1e-6, 1e-7));
        assert!(fuzzy_equals(f64::INFINITY, f64::INFINITY, 0.0));
        assert!(fuzzy_equals(f64::NAN, f64::NAN, 0.0));
        assert!(!fuzzy_equals(f64::NAN, 1.0, 1.0));
    }

    #[test]
    fn gcd_matches_guava() {
        assert_eq!(gcd(0, 0), 0);
        assert_eq!(gcd(0, 5), 5);
        assert_eq!(gcd(12, 18), 6);
        assert_eq!(gcd(7, 13), 1);
    }
}

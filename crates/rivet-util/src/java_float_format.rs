//! Java `Double.toString` / `Float.toString` formatting.
//!
//! Java's `DoubleArgumentType.toString()`/`FloatArgumentType.toString()` stringify
//! their bounds with the JDK's `Double.toString`/`Float.toString`, and
//! `TranslatableContents` renders `Double` arguments the same way. That format is
//! not Rust's `Display`: Java always keeps a decimal point (plain form `100.0`,
//! `0.05`, `0.001` in the `10^-3 <= |v| < 10^7` range, else computerized
//! scientific `2.147483648E9`, `1.0E-4`), with the shortest round-trip digits.
//!
//! The canonical implementation lives in `rivet-serialization`'s `float_format`
//! module (ryu digits + Java's plain/scientific rule + the subnormal overrides,
//! shared with the JSON `createFloat` path and `rivet-nbt`'s SNBT visitors);
//! this module re-exports it.

pub use rivet_serialization::float_format::{java_double_to_string, java_float_to_string};

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

//! Java-grounded tests for the typed `Number` narrowing semantics.

use rivet_serialization::number::Number;

#[test]
fn number_narrowing_matches_java_number() {
    // `Double(-1.5).intValue() == -1` (truncate toward zero, not floor).
    assert_eq!(Number::Double(-1.5).int_value(), -1);
    assert_eq!(Number::Double(-1.5).long_value(), -1);
    assert_eq!(Number::Double(-1.5).byte_value(), -1);

    // `Float(-1.5f).intValue() == -1`.
    assert_eq!(Number::Float(-1.5).int_value(), -1);

    // Saturation at range boundaries.
    assert_eq!(Number::Double(3.7e9).int_value(), i32::MAX);
    assert_eq!(Number::Double(-3.7e9).int_value(), i32::MIN);
    assert_eq!(Number::Double(f64::INFINITY).int_value(), i32::MAX);
    assert_eq!(Number::Double(f64::INFINITY).long_value(), i64::MAX);
    assert_eq!(Number::Double(f64::NEG_INFINITY).long_value(), i64::MIN);

    // NaN → 0 for int/long; byte/short via `(byte)(int)` also 0.
    assert_eq!(Number::Double(f64::NAN).int_value(), 0);
    assert_eq!(Number::Double(f64::NAN).long_value(), 0);
    assert_eq!(Number::Float(f32::NAN).int_value(), 0);
    assert_eq!(Number::Double(f64::NAN).byte_value(), 0);

    // Integral variants wrap on narrowing (`Long(5_000_000_000).intValue()`).
    assert_eq!(Number::Long(5_000_000_000).int_value(), 705_032_704);
    assert_eq!(Number::Long(300).byte_value(), 44);
    assert_eq!(Number::Int(300).byte_value(), 44);

    // Float/double byteValue goes through (int) first, then wraps
    // (`Double(300).byteValue() == 44`, NOT the saturated `300 as i8`).
    assert_eq!(Number::Double(300.0).byte_value(), 44);
    assert_eq!(Number::Double(-300.0).byte_value(), -44);
    assert_eq!(Number::Double(300.0).short_value(), 300_i16);

    // 2^53 precision is preserved through the typed Long variant.
    assert_eq!(
        (Number::Long((1i64 << 53) + 1)).long_value(),
        (1i64 << 53) + 1
    );
    assert_eq!(
        (Number::Long((1i64 << 53) - 1)).long_value(),
        (1i64 << 53) - 1
    );
}

#[test]
fn number_double_value_matches_java() {
    assert_eq!(Number::Byte(-1).double_value(), -1.0);
    assert_eq!(Number::Int(7).double_value(), 7.0);
    assert_eq!(Number::Long(9_000_000_000).double_value(), 9_000_000_000.0);
    assert_eq!(Number::Float(1.5).double_value(), 1.5);
    assert_eq!(Number::Double(0.1).double_value(), 0.1);
}

#[test]
fn number_equality_follows_java_wrapper_equals() {
    // Same variant: value equality.
    assert_eq!(Number::Int(5), Number::Int(5));
    assert_ne!(Number::Int(5), Number::Int(6));
    // Mixed variants are unequal (Java `Integer(5) != Long(5)`).
    assert_ne!(Number::Int(5), Number::Long(5));
    assert_ne!(Number::Int(5), Number::Double(5.0));
    // NaN == NaN, 0.0 != -0.0 (Float.compare/Double.compare).
    assert_eq!(Number::Double(f64::NAN), Number::Double(f64::NAN));
    assert_ne!(Number::Double(0.0), Number::Double(-0.0));
    assert_eq!(Number::Float(f32::NAN), Number::Float(f32::NAN));
}

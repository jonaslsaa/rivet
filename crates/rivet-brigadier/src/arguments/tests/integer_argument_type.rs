//! Unit tests ported from the upstream brigadier `IntegerArgumentTypeTest` (MIT),
//! translated against the `IntegerArgumentType` port. Faithful-behavior tests only.

use crate::arguments::integer_argument_type::IntegerArgumentType;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;

/// Java `integer(-100, 100)` — a concrete `ArgumentType<Integer>`.
fn integer() -> std::sync::Arc<dyn crate::arguments::ArgumentType<i32>> {
    IntegerArgumentType::integer()
}

fn integer_bounds(min: i32, max: i32) -> std::sync::Arc<dyn crate::arguments::ArgumentType<i32>> {
    IntegerArgumentType::integer_with_bounds(min, max)
}

/// Java's `CommandContextBuilder` mock is unused by these tests; `getType()` etc.
/// are not exercised. Concrete parsers are tested directly.

#[test]
fn parse() {
    let mut reader = StringReader::new("15");
    assert_eq!(integer().parse(&mut reader).unwrap(), 15);
    assert!(!reader.can_read());
}

#[test]
fn parse_too_small() {
    let mut reader = StringReader::new("-5");
    let err = integer_bounds(0, 100).parse(&mut reader).unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().integer_too_low()
    ));
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn parse_too_big() {
    let mut reader = StringReader::new("5");
    let err = integer_bounds(-100, 0).parse(&mut reader).unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().integer_too_high()
    ));
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn test_equals() {
    // Java's EqualsTester groups: each pair equal, across groups unequal.
    // Equality is exposed as `ArgumentType::type_equals`.
    let a = integer();
    let b = integer();
    let c = integer_bounds(-100, 100);
    let d = integer_bounds(-100, 100);
    let e = integer_bounds(-100, 50);
    let f = integer_bounds(-100, 50);
    let g = integer_bounds(-50, 100);
    let h = integer_bounds(-50, 100);

    assert!(a.type_equals(b.as_ref()));
    assert!(c.type_equals(d.as_ref()));
    assert!(e.type_equals(f.as_ref()));
    assert!(g.type_equals(h.as_ref()));
    // Across groups they are unequal.
    assert!(!a.type_equals(c.as_ref()));
    assert!(!c.type_equals(e.as_ref()));
    assert!(!e.type_equals(g.as_ref()));
}

#[test]
fn test_to_string() {
    assert_eq!(integer().to_string(), "integer()");
    assert_eq!(integer_bounds(-100, i32::MAX).to_string(), "integer(-100)");
    assert_eq!(integer_bounds(-100, 100).to_string(), "integer(-100, 100)");
    assert_eq!(
        integer_bounds(i32::MIN, 100).to_string(),
        "integer(-2147483648, 100)"
    );
}

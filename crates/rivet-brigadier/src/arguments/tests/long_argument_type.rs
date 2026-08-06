//! Unit tests ported from the upstream brigadier `LongArgumentTypeTest` (MIT).

use crate::arguments::long_argument_type::LongArgumentType;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;

fn long_arg() -> std::sync::Arc<dyn crate::arguments::ArgumentType<i64>> {
    LongArgumentType::long_arg()
}

fn long_arg_bounds(min: i64, max: i64) -> std::sync::Arc<dyn crate::arguments::ArgumentType<i64>> {
    LongArgumentType::long_arg_with_bounds(min, max)
}

#[test]
fn parse() {
    let mut reader = StringReader::new("15");
    assert_eq!(long_arg().parse(&mut reader).unwrap(), 15);
    assert!(!reader.can_read());
}

#[test]
fn parse_too_small() {
    let mut reader = StringReader::new("-5");
    let err = long_arg_bounds(0, 100).parse(&mut reader).unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().long_too_low()
    ));
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn parse_too_big() {
    let mut reader = StringReader::new("5");
    let err = long_arg_bounds(-100, 0).parse(&mut reader).unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().long_too_high()
    ));
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn test_equals() {
    let a = long_arg();
    let b = long_arg();
    let c = long_arg_bounds(-100, 100);
    let d = long_arg_bounds(-100, 100);
    let e = long_arg_bounds(-100, 50);
    let f = long_arg_bounds(-100, 50);
    let g = long_arg_bounds(-50, 100);
    let h = long_arg_bounds(-50, 100);

    assert!(a.type_equals(b.as_ref()));
    assert!(c.type_equals(d.as_ref()));
    assert!(e.type_equals(f.as_ref()));
    assert!(g.type_equals(h.as_ref()));
    assert!(!a.type_equals(c.as_ref()));
    assert!(!c.type_equals(e.as_ref()));
    assert!(!e.type_equals(g.as_ref()));
}

#[test]
fn test_to_string() {
    assert_eq!(long_arg().to_string(), "longArg()");
    assert_eq!(long_arg_bounds(-100, i64::MAX).to_string(), "longArg(-100)");
    assert_eq!(long_arg_bounds(-100, 100).to_string(), "longArg(-100, 100)");
    assert_eq!(
        long_arg_bounds(i64::MIN, 100).to_string(),
        "longArg(-9223372036854775808, 100)"
    );
}

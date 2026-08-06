//! Unit tests ported from the upstream brigadier `DoubleArgumentTypeTest` (MIT).

use crate::arguments::double_argument_type::DoubleArgumentType;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;

fn double_arg() -> std::sync::Arc<dyn crate::arguments::ArgumentType<f64>> {
    DoubleArgumentType::double_arg()
}

fn double_arg_bounds(
    min: f64,
    max: f64,
) -> std::sync::Arc<dyn crate::arguments::ArgumentType<f64>> {
    DoubleArgumentType::double_arg_with_bounds(min, max)
}

#[test]
fn parse() {
    let mut reader = StringReader::new("15");
    assert_eq!(double_arg().parse(&mut reader).unwrap(), 15.0);
    assert!(!reader.can_read());
}

#[test]
fn parse_too_small() {
    let mut reader = StringReader::new("-5");
    let err = double_arg_bounds(0.0, 100.0)
        .parse(&mut reader)
        .unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().double_too_low()
    ));
    assert_eq!(err.get_cursor(), 0);
    // Java string-concatenates `Double.toString` for the bound and the found value.
    assert_eq!(
        err.get_message(),
        "Double must not be less than 0.0, found -5.0 at position 0: <--[HERE]"
    );
}

#[test]
fn parse_too_big() {
    let mut reader = StringReader::new("5");
    let err = double_arg_bounds(-100.0, 0.0)
        .parse(&mut reader)
        .unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().double_too_high()
    ));
    assert_eq!(err.get_cursor(), 0);
    assert_eq!(
        err.get_message(),
        "Double must not be more than 0.0, found 5.0 at position 0: <--[HERE]"
    );
}

#[test]
fn test_equals() {
    let a = double_arg();
    let b = double_arg();
    let c = double_arg_bounds(-100.0, 100.0);
    let d = double_arg_bounds(-100.0, 100.0);
    let e = double_arg_bounds(-100.0, 50.0);
    let f = double_arg_bounds(-100.0, 50.0);
    let g = double_arg_bounds(-50.0, 100.0);
    let h = double_arg_bounds(-50.0, 100.0);

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
    assert_eq!(double_arg().to_string(), "double()");
    assert_eq!(
        double_arg_bounds(-100.0, f64::MAX).to_string(),
        "double(-100.0)"
    );
    assert_eq!(
        double_arg_bounds(-100.0, 100.0).to_string(),
        "double(-100.0, 100.0)"
    );
    assert_eq!(
        double_arg_bounds(i32::MIN as f64, 100.0).to_string(),
        "double(-2.147483648E9, 100.0)"
    );
}

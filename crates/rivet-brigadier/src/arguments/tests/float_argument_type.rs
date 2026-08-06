//! Unit tests ported from the upstream brigadier `FloatArgumentTypeTest` (MIT).

use crate::arguments::float_argument_type::FloatArgumentType;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;

fn float_arg() -> std::sync::Arc<dyn crate::arguments::ArgumentType<f32>> {
    FloatArgumentType::float_arg()
}

fn float_arg_bounds(min: f32, max: f32) -> std::sync::Arc<dyn crate::arguments::ArgumentType<f32>> {
    FloatArgumentType::float_arg_with_bounds(min, max)
}

#[test]
fn parse() {
    let mut reader = StringReader::new("15");
    assert_eq!(float_arg().parse(&mut reader).unwrap(), 15.0);
    assert!(!reader.can_read());
}

#[test]
fn parse_too_small() {
    let mut reader = StringReader::new("-5");
    let err = float_arg_bounds(0.0, 100.0).parse(&mut reader).unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().float_too_low()
    ));
    assert_eq!(err.get_cursor(), 0);
    // Java string-concatenates `Float.toString` for the bound and the found value.
    assert_eq!(
        err.get_message(),
        "Float must not be less than 0.0, found -5.0 at position 0: <--[HERE]"
    );
}

#[test]
fn parse_too_big() {
    let mut reader = StringReader::new("5");
    let err = float_arg_bounds(-100.0, 0.0)
        .parse(&mut reader)
        .unwrap_err();
    assert!(crate::exceptions::exception_type_eq(
        err.get_type(),
        CommandSyntaxException::built_in_exceptions().float_too_high()
    ));
    assert_eq!(err.get_cursor(), 0);
    assert_eq!(
        err.get_message(),
        "Float must not be more than 0.0, found 5.0 at position 0: <--[HERE]"
    );
}

#[test]
fn test_equals() {
    let a = float_arg();
    let b = float_arg();
    let c = float_arg_bounds(-100.0, 100.0);
    let d = float_arg_bounds(-100.0, 100.0);
    let e = float_arg_bounds(-100.0, 50.0);
    let f = float_arg_bounds(-100.0, 50.0);
    let g = float_arg_bounds(-50.0, 100.0);
    let h = float_arg_bounds(-50.0, 100.0);

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
    assert_eq!(float_arg().to_string(), "float()");
    assert_eq!(
        float_arg_bounds(-100.0, f32::MAX).to_string(),
        "float(-100.0)"
    );
    assert_eq!(
        float_arg_bounds(-100.0, 100.0).to_string(),
        "float(-100.0, 100.0)"
    );
    // The upstream master test expects `-2.14748365E9` (pre-JDK-19 Float.toString);
    // the modern JDK and this port print `-2.1474836E9` (shortest round-trip).
    assert_eq!(
        float_arg_bounds(i32::MIN as f32, 100.0).to_string(),
        "float(-2.1474836E9, 100.0)"
    );
}

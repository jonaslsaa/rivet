//! Unit tests ported from the upstream brigadier `ParsedArgumentTest` (MIT).

use crate::context::ParsedArgument;
use crate::string_reader::StringReader;

#[test]
fn test_equals() {
    let a = ParsedArgument::new(0, 3, "bar".to_string());
    let b = ParsedArgument::new(0, 3, "bar".to_string());
    let c = ParsedArgument::new(3, 6, "baz".to_string());
    let d = ParsedArgument::new(3, 6, "baz".to_string());
    let e = ParsedArgument::new(6, 9, "baz".to_string());
    let f = ParsedArgument::new(6, 9, "baz".to_string());

    assert_eq!(a, b);
    assert_eq!(c, d);
    assert_eq!(e, f);
    assert_ne!(a, c);
    assert_ne!(c, e);
}

#[test]
fn get_raw() {
    let reader = StringReader::new("0123456789");
    let argument = ParsedArgument::new::<String>(2, 5, String::new());
    assert_eq!(argument.get_range().get_reader(&reader), "234");
}

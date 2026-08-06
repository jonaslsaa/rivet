//! Unit tests ported from the upstream brigadier `BoolArgumentTypeTest` (MIT).

use crate::arguments::bool_argument_type::BoolArgumentType;
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;

#[test]
fn parse() {
    // Upstream mocks `StringReader.readBoolean()` returning `true`; the concrete
    // reader's behavior is exercised instead (the port's `read_boolean` is tested
    // by the StringReader tests).
    let mut reader = StringReader::new("true");
    assert!(BoolArgumentType::bool().parse(&mut reader).unwrap());
    assert!(!reader.can_read());
}

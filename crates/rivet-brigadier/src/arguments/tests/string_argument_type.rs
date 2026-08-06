//! Unit tests ported from the upstream brigadier `StringArgumentTypeTest` (MIT).

use crate::arguments::string_argument_type::{StringArgumentType, escape_if_required};
use crate::immutable_string_reader::ImmutableStringReader;
use crate::string_reader::StringReader;

fn word() -> std::sync::Arc<dyn crate::arguments::ArgumentType<String>> {
    StringArgumentType::word()
}

fn string() -> std::sync::Arc<dyn crate::arguments::ArgumentType<String>> {
    StringArgumentType::string()
}

fn greedy_string() -> std::sync::Arc<dyn crate::arguments::ArgumentType<String>> {
    StringArgumentType::greedy_string()
}

#[test]
fn test_parse_word() {
    let mut reader = StringReader::new("hello world");
    assert_eq!(word().parse(&mut reader).unwrap(), "hello");
}

#[test]
fn test_parse_string() {
    let mut reader = StringReader::new("\"hello world\" rest");
    assert_eq!(string().parse(&mut reader).unwrap(), "hello world");
}

#[test]
fn test_parse_greedy_string() {
    let mut reader = StringReader::new("Hello world! This is a test.");
    assert_eq!(
        greedy_string().parse(&mut reader).unwrap(),
        "Hello world! This is a test."
    );
    assert!(!reader.can_read());
}

#[test]
fn test_to_string() {
    assert_eq!(string().to_string(), "string()");
}

#[test]
fn test_escape_if_required_not_required() {
    assert_eq!(escape_if_required("hello"), "hello");
    assert_eq!(escape_if_required(""), "");
}

#[test]
fn test_escape_if_required_multiple_words() {
    assert_eq!(escape_if_required("hello world"), "\"hello world\"");
}

#[test]
fn test_escape_if_required_quote() {
    assert_eq!(
        escape_if_required("hello \"world\"!"),
        "\"hello \\\"world\\\"!\""
    );
}

#[test]
fn test_escape_if_required_escapes() {
    assert_eq!(escape_if_required("\\"), "\"\\\\\"");
}

#[test]
fn test_escape_if_required_single_quote() {
    assert_eq!(escape_if_required("\""), "\"\\\"\"");
}

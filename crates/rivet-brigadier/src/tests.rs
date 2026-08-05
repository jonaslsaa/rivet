//! Unit tests ported from the upstream brigadier `StringReaderTest` /
//! `CommandSyntaxExceptionTest` semantics, against the translated `StringReader`
//! and `CommandSyntaxException`. Faithful-behavior tests only (no gameplay logic).

use crate::ImmutableStringReader;
use crate::StringReader;
use crate::exceptions::CommandSyntaxException;
use crate::exceptions::built_in_exception_provider::BuiltInExceptionProvider;

fn reader(input: &str) -> StringReader {
    StringReader::new(input)
}

#[test]
fn can_read() {
    let r = reader("abc");
    assert!(r.can_read());
    assert!(r.can_read_with_length(1));
    assert!(r.can_read_with_length(2));
    assert!(r.can_read_with_length(3));
    assert!(!r.can_read_with_length(4));
}

#[test]
fn getters() {
    let mut r = reader("abcdef");
    assert_eq!(r.get_string(), "abcdef");
    assert_eq!(r.get_remaining_length(), 6);
    assert_eq!(r.get_total_length(), 6);
    assert_eq!(r.get_cursor(), 0);
    assert_eq!(r.get_read(), "");
    assert_eq!(r.get_remaining(), "abcdef");
    r.set_cursor(2);
    assert_eq!(r.get_cursor(), 2);
    assert_eq!(r.get_remaining_length(), 4);
    assert_eq!(r.get_read(), "ab");
    assert_eq!(r.get_remaining(), "cdef");
}

#[test]
fn read_and_peek() {
    let mut r = reader("abc");
    assert_eq!(r.peek(), 'a');
    assert_eq!(r.peek_with_offset(1), 'b');
    assert_eq!(r.peek_with_offset(2), 'c');
    assert_eq!(r.read(), 'a');
    assert_eq!(r.get_cursor(), 1);
    r.skip();
    assert_eq!(r.get_cursor(), 2);
    assert_eq!(r.read(), 'c');
}

#[test]
fn read_int_valid() {
    let mut r = reader("12345abc");
    assert_eq!(r.read_int().unwrap(), 12345);
    assert_eq!(r.get_remaining(), "abc");
}

#[test]
fn read_int_negative() {
    let mut r = reader("-5");
    assert_eq!(r.read_int().unwrap(), -5);
}

#[test]
fn read_int_expected() {
    let mut r = reader("abc");
    let err = r.read_int().unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Expected integer");
    assert_eq!(err.get_cursor(), 0);
}

#[test]
fn read_int_invalid_resets_cursor() {
    let mut r = reader("--5");
    let err = r.read_int().unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Invalid integer '--5'");
    // Java resets the cursor to the start on a parse failure.
    assert_eq!(r.get_cursor(), 0);
}

#[test]
fn read_int_overflow_invalid() {
    let mut r = reader("99999999999999999999");
    let err = r.read_int().unwrap_err();
    assert_eq!(
        err.get_raw_message().get_string(),
        "Invalid integer '99999999999999999999'"
    );
    assert_eq!(r.get_cursor(), 0);
}

#[test]
fn read_long_valid() {
    let mut r = reader("9223372036854775807");
    assert_eq!(r.read_long().unwrap(), i64::MAX);
}

#[test]
fn read_long_expected() {
    let mut r = reader("");
    let err = r.read_long().unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Expected long");
}

#[test]
fn read_double_and_float() {
    let mut r = reader("1.5");
    assert_eq!(r.read_double().unwrap(), 1.5);
    let mut r = reader("0.5");
    assert_eq!(r.read_float().unwrap(), 0.5);
}

#[test]
fn read_double_expected() {
    let mut r = reader("abc");
    let err = r.read_double().unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Expected double");
}

#[test]
fn read_float_expected() {
    let mut r = reader("abc");
    let err = r.read_float().unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Expected float");
}

#[test]
fn read_unquoted_string() {
    let mut r = reader("hello world");
    assert_eq!(r.read_unquoted_string(), "hello");
    assert_eq!(r.get_remaining(), " world");
}

#[test]
fn read_quoted_string() {
    let mut r = reader("\"hello world\" rest");
    assert_eq!(r.read_quoted_string().unwrap(), "hello world");
    assert_eq!(r.get_remaining(), " rest");
}

#[test]
fn read_quoted_string_escapes() {
    let mut r = reader(r#""hello \"world\" and \\ backslash""#);
    assert_eq!(
        r.read_quoted_string().unwrap(),
        "hello \"world\" and \\ backslash"
    );
}

#[test]
fn read_quoted_string_unclosed() {
    let mut r = reader("\"hello");
    let err = r.read_quoted_string().unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Unclosed quoted string");
    assert_eq!(err.get_cursor(), 6);
}

#[test]
fn read_quoted_string_expected_start() {
    let mut r = reader("hello");
    let err = r.read_quoted_string().unwrap_err();
    assert_eq!(
        err.get_raw_message().get_string(),
        "Expected quote to start a string"
    );
}

#[test]
fn read_string_until_invalid_escape_rewinds_cursor() {
    let mut r = reader("hello\\q");
    // readStringUntil('x') on "hello\q": h,e,l,l,o appended, then '\\' sets escaped,
    // then 'q' is neither the terminator nor an escape -> invalid escape, cursor
    // rewound one code unit (Java: setCursor(getCursor() - 1)).
    let err = r.read_string_until('x').unwrap_err();
    assert_eq!(
        err.get_raw_message().get_string(),
        "Invalid escape sequence 'q' in quoted string"
    );
    assert_eq!(r.get_cursor(), 6);
}

#[test]
fn read_string_until_invalid_escape_surrogate_rewinds_to_first_unit() {
    // "hi\😀": the supplementary char after the escape is two UTF-16 units (high
    // surrogate at index 4, low at 5). Java read() consumes one unit, then
    // setCursor(getCursor() - 1) lands on index 4 (the high surrogate). Rust
    // read() consumes both units, so the rewind must step back by `consumed` (2)
    // to land on the same first unit of the offending char.
    let mut r = reader("\"hi\\\u{1F600}\"");
    let err = r.read_quoted_string().unwrap_err();
    assert_eq!(err.get_cursor(), 4);
    assert_eq!(
        err.get_raw_message().get_string(),
        "Invalid escape sequence '\u{1F600}' in quoted string"
    );
    // Java would render the lone high surrogate here; Rust renders the full char.
    assert_eq!(
        err.get_message(),
        "Invalid escape sequence '\u{1F600}' in quoted string at position 4: \"hi\\<--[HERE]"
    );
}

#[test]
fn read_string_quoted_and_unquoted() {
    let mut r = reader("\"quoted\"");
    assert_eq!(r.read_string().unwrap(), "quoted");
    let mut r = reader("unquoted");
    assert_eq!(r.read_string().unwrap(), "unquoted");
    let mut r = reader("");
    assert_eq!(r.read_string().unwrap(), "");
}

#[test]
fn read_boolean() {
    let mut r = reader("true");
    assert!(r.read_boolean().unwrap());
    let mut r = reader("false");
    assert!(!r.read_boolean().unwrap());
    let mut r = reader("tru");
    let err = r.read_boolean().unwrap_err();
    assert_eq!(
        err.get_raw_message().get_string(),
        "Invalid bool, expected true or false but found 'tru'"
    );
    assert_eq!(r.get_cursor(), 0);
}

#[test]
fn expect() {
    let mut r = reader("abc");
    r.expect('a').unwrap();
    assert_eq!(r.get_cursor(), 1);
    let err = r.expect('z').unwrap_err();
    assert_eq!(err.get_raw_message().get_string(), "Expected 'z'");
}

#[test]
fn skip_whitespace() {
    let mut r = reader(" \t \n hello");
    r.skip_whitespace();
    assert_eq!(r.get_cursor(), 5);
    assert_eq!(r.peek(), 'h');
}

#[test]
fn is_whitespace_matches_java() {
    use crate::string_reader::is_whitespace;
    assert!(is_whitespace('\u{0020}'));
    assert!(is_whitespace('\t'));
    assert!(is_whitespace('\n'));
    assert!(is_whitespace('\u{3000}'));
    assert!(is_whitespace('\u{1680}'));
    // Character.isWhitespace excludes the non-breaking spaces.
    assert!(!is_whitespace('\u{00A0}'));
    assert!(!is_whitespace('\u{2007}'));
    assert!(!is_whitespace('\u{202F}'));
    assert!(!is_whitespace('a'));
}

#[test]
fn context_formatting() {
    let mut r = reader("hello");
    let err = r.read_string_until('z').unwrap_err();
    assert_eq!(
        err.get_message(),
        "Unclosed quoted string at position 5: hello<--[HERE]"
    );
}

#[test]
fn context_elided() {
    let mut r = reader("a long input string that goes well past the context amount");
    let err = r.read_string_until('z').unwrap_err();
    let context = err.get_context().unwrap();
    assert!(context.starts_with("..."));
    assert!(context.ends_with("<--[HERE]"));
}

#[test]
fn exception_without_context() {
    let err = CommandSyntaxException::built_in_exceptions()
        .reader_expected_int()
        .create();
    assert_eq!(err.get_message(), "Expected integer");
    assert_eq!(err.get_context(), None);
    assert_eq!(err.get_input(), None);
    assert_eq!(err.get_cursor(), -1);
    assert_eq!(err.get_raw_message().get_string(), "Expected integer");
}

#[test]
fn built_in_types_distinct() {
    // Java: each accessor returns the same singleton instance. In Rust the
    // per-object identity is preserved by the LazyLock singleton.
    let a = CommandSyntaxException::built_in_exceptions().reader_expected_int();
    let b = CommandSyntaxException::built_in_exceptions().reader_expected_int();
    assert!(std::ptr::eq(a, b));
}

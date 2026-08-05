//! Port of `com.mojang.brigadier.StringReader` (upstream).

use crate::ImmutableStringReader;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::immutable_string_reader::utf16_units_to_string;

/// Java `StringReader.SYNTAX_ESCAPE` = `'\\'`.
const SYNTAX_ESCAPE: char = '\\';
/// Java `StringReader.SYNTAX_DOUBLE_QUOTE` = `'"'`.
const SYNTAX_DOUBLE_QUOTE: char = '"';
/// Java `StringReader.SYNTAX_SINGLE_QUOTE` = `'\''`.
const SYNTAX_SINGLE_QUOTE: char = '\'';

/// Java `StringReader` — a cursor over a command string.
///
/// The input is stored as UTF-16 code units (`units`), the same representation as
/// Java's `char[]`, so the `cursor` is a UTF-16 code-unit index exactly like Java's.
/// The original `String` is kept alongside for `getString()` to return `&str`.
pub struct StringReader {
    /// Original input string (`getString()`), the Rust view of the same text as `units`.
    string: String,
    /// UTF-16 code units of `string` — the Java `char[]` representation.
    units: Vec<u16>,
    /// Java `cursor` — a UTF-16 code-unit index.
    cursor: i32,
}

impl StringReader {
    /// `StringReader(String)`.
    pub fn new(string: impl Into<String>) -> Self {
        let string = string.into();
        let units: Vec<u16> = string.encode_utf16().collect();
        StringReader {
            string,
            units,
            cursor: 0,
        }
    }

    /// `StringReader(StringReader)` copy constructor.
    pub fn new_with_reader(other: &StringReader) -> Self {
        StringReader {
            string: other.string.clone(),
            units: other.units.clone(),
            cursor: other.cursor,
        }
    }

    /// `setCursor(int)`.
    pub fn set_cursor(&mut self, cursor: i32) {
        self.cursor = cursor;
    }

    /// `read()` — the char at the cursor, advancing past it.
    ///
    /// Java advances exactly one UTF-16 code unit per `read()` and returns a
    /// surrogate half for supplementary-plane chars; Rust decodes a surrogate pair
    /// into one `char` and advances two code units in a single call. The cursor
    /// position after fully consuming a pair matches Java, so command-parsing
    /// control flow is identical — but a caller that interleaves `read()` with
    /// `skip()` or counts calls observes a different per-call advancement for
    /// supplementary chars than the Java API. `peek()`/`peek_with_offset()` share
    /// the same one-call-per-`char` divergence.
    pub fn read(&mut self) -> char {
        let (c, consumed) = decode_char(&self.units, self.cursor as usize);
        self.cursor = self.cursor.wrapping_add(consumed as i32);
        c
    }

    /// `skip()` — advance the cursor by one UTF-16 code unit.
    pub fn skip(&mut self) {
        self.cursor = self.cursor.wrapping_add(1);
    }

    /// Java `StringReader.isAllowedNumber(char)`.
    pub fn is_allowed_number(c: char) -> bool {
        matches!(c, '0'..='9' | '.' | '-')
    }

    /// Java `StringReader.isQuotedStringStart(char)`.
    pub fn is_quoted_string_start(c: char) -> bool {
        c == SYNTAX_DOUBLE_QUOTE || c == SYNTAX_SINGLE_QUOTE
    }

    /// Java `StringReader.skipWhitespace()` — skips `Character.isWhitespace` chars.
    pub fn skip_whitespace(&mut self) {
        while self.can_read() && is_whitespace(self.peek()) {
            self.skip();
        }
    }

    /// Java `StringReader.readInt()`.
    pub fn read_int(&mut self) -> Result<i32, CommandSyntaxException<'static>> {
        let start = self.cursor;
        while self.can_read() && StringReader::is_allowed_number(self.peek()) {
            self.skip();
        }
        let number = self.substring_units(start, self.cursor);
        if number.is_empty() {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_int()
                .create_with_context(self));
        }
        match number.parse::<i32>() {
            Ok(value) => Ok(value),
            Err(_) => {
                self.cursor = start;
                Err(CommandSyntaxException::built_in_exceptions()
                    .reader_invalid_int()
                    .create_with_context(self, &number))
            }
        }
    }

    /// Java `StringReader.readLong()`.
    pub fn read_long(&mut self) -> Result<i64, CommandSyntaxException<'static>> {
        let start = self.cursor;
        while self.can_read() && StringReader::is_allowed_number(self.peek()) {
            self.skip();
        }
        let number = self.substring_units(start, self.cursor);
        if number.is_empty() {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_long()
                .create_with_context(self));
        }
        match number.parse::<i64>() {
            Ok(value) => Ok(value),
            Err(_) => {
                self.cursor = start;
                Err(CommandSyntaxException::built_in_exceptions()
                    .reader_invalid_long()
                    .create_with_context(self, &number))
            }
        }
    }

    /// Java `StringReader.readDouble()`.
    pub fn read_double(&mut self) -> Result<f64, CommandSyntaxException<'static>> {
        let start = self.cursor;
        while self.can_read() && StringReader::is_allowed_number(self.peek()) {
            self.skip();
        }
        let number = self.substring_units(start, self.cursor);
        if number.is_empty() {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_double()
                .create_with_context(self));
        }
        match number.parse::<f64>() {
            Ok(value) => Ok(value),
            Err(_) => {
                self.cursor = start;
                Err(CommandSyntaxException::built_in_exceptions()
                    .reader_invalid_double()
                    .create_with_context(self, &number))
            }
        }
    }

    /// Java `StringReader.readFloat()`.
    pub fn read_float(&mut self) -> Result<f32, CommandSyntaxException<'static>> {
        let start = self.cursor;
        while self.can_read() && StringReader::is_allowed_number(self.peek()) {
            self.skip();
        }
        let number = self.substring_units(start, self.cursor);
        if number.is_empty() {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_float()
                .create_with_context(self));
        }
        match number.parse::<f32>() {
            Ok(value) => Ok(value),
            Err(_) => {
                self.cursor = start;
                Err(CommandSyntaxException::built_in_exceptions()
                    .reader_invalid_float()
                    .create_with_context(self, &number))
            }
        }
    }

    /// Java `StringReader.isAllowedInUnquotedString(char)`.
    pub fn is_allowed_in_unquoted_string(c: char) -> bool {
        matches!(
            c,
            '0'..='9' | 'A'..='Z' | 'a'..='z' | '_' | '-' | '.' | '+'
        )
    }

    /// Java `StringReader.readUnquotedString()`.
    pub fn read_unquoted_string(&mut self) -> String {
        let start = self.cursor;
        while self.can_read() && StringReader::is_allowed_in_unquoted_string(self.peek()) {
            self.skip();
        }
        self.substring_units(start, self.cursor)
    }

    /// Java `StringReader.readQuotedString()`.
    pub fn read_quoted_string(&mut self) -> Result<String, CommandSyntaxException<'static>> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if !StringReader::is_quoted_string_start(next) {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_start_of_quote()
                .create_with_context(self));
        }
        self.skip();
        self.read_string_until(next)
    }

    /// Java `StringReader.readStringUntil(char)`.
    pub fn read_string_until(
        &mut self,
        terminator: char,
    ) -> Result<String, CommandSyntaxException<'static>> {
        let mut result = String::new();
        let mut escaped = false;
        while self.can_read() {
            // Cursor before read() — Java rewinds setCursor(getCursor() - 1), which
            // steps back one code unit onto the first unit of the offending char.
            // Rust read() consumes `consumed` code units (2 for a surrogate pair), so
            // rewinding to the pre-read cursor lands on that same first unit.
            let before = self.cursor;
            let c = self.read();
            if escaped {
                if c == terminator || c == SYNTAX_ESCAPE {
                    result.push(c);
                    escaped = false;
                } else {
                    self.set_cursor(before);
                    // The message arg renders the full Rust char; Java's
                    // String.valueOf(c) would be the lone high surrogate for a
                    // supplementary char (inherent to Rust's char type).
                    return Err(CommandSyntaxException::built_in_exceptions()
                        .reader_invalid_escape()
                        .create_with_context(self, &c.to_string()));
                }
            } else if c == SYNTAX_ESCAPE {
                escaped = true;
            } else if c == terminator {
                return Ok(result);
            } else {
                result.push(c);
            }
        }

        Err(CommandSyntaxException::built_in_exceptions()
            .reader_expected_end_of_quote()
            .create_with_context(self))
    }

    /// Java `StringReader.readString()`.
    pub fn read_string(&mut self) -> Result<String, CommandSyntaxException<'static>> {
        if !self.can_read() {
            return Ok(String::new());
        }
        let next = self.peek();
        if StringReader::is_quoted_string_start(next) {
            self.skip();
            return self.read_string_until(next);
        }
        Ok(self.read_unquoted_string())
    }

    /// Java `StringReader.readBoolean()`.
    pub fn read_boolean(&mut self) -> Result<bool, CommandSyntaxException<'static>> {
        let start = self.cursor;
        let value = self.read_string()?;
        if value.is_empty() {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_bool()
                .create_with_context(self));
        }

        if value == "true" {
            Ok(true)
        } else if value == "false" {
            Ok(false)
        } else {
            self.cursor = start;
            Err(CommandSyntaxException::built_in_exceptions()
                .reader_invalid_bool()
                .create_with_context(self, &value))
        }
    }

    /// Java `StringReader.expect(char)`.
    pub fn expect(&mut self, c: char) -> Result<(), CommandSyntaxException<'static>> {
        if !self.can_read() || self.peek() != c {
            return Err(CommandSyntaxException::built_in_exceptions()
                .reader_expected_symbol()
                .create_with_context(self, &c.to_string()));
        }
        self.skip();
        Ok(())
    }

    /// `input.substring(start, end)` in UTF-16 code units.
    fn substring_units(&self, start: i32, end: i32) -> String {
        utf16_units_to_string(&self.units[start as usize..end as usize])
    }
}

/// Java `Character.isWhitespace(char)`: Unicode White_Space minus the non-breaking
/// spaces (U+00A0, U+2007, U+202F), plus the ASCII and Unicode separators. This is
/// NOT the same as Rust's `char::is_whitespace` (which includes the non-breaking
/// spaces), so it is spelled out for parity.
pub fn is_whitespace(c: char) -> bool {
    matches!(c,
        '\u{0009}' | '\u{000A}' | '\u{000B}' | '\u{000C}' | '\u{000D}'
        | '\u{001C}' | '\u{001D}' | '\u{001E}' | '\u{001F}'
        | '\u{0020}'
        | '\u{1680}'
        | '\u{2000}'..='\u{2006}'
        | '\u{2008}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}'
        | '\u{205F}' | '\u{3000}')
}

/// Decode the UTF-16 code unit(s) at `index` to a Rust `char`, returning the char
/// and how many code units it spans (2 for a valid surrogate pair, else 1).
/// A lone surrogate decodes to U+FFFD; Java would surface the raw code unit, but no
/// parsing predicate (digits, letters, quotes, whitespace) matches surrogates, so
/// control flow is identical.
fn decode_char(units: &[u16], index: usize) -> (char, usize) {
    let unit = units[index];
    if (0xD800..=0xDBFF).contains(&unit)
        && let Some(&low) = units.get(index + 1)
        && (0xDC00..=0xDFFF).contains(&low)
    {
        let scalar = 0x1_0000 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00));
        return (char::from_u32(scalar).unwrap_or('\u{FFFD}'), 2);
    }
    (char::from_u32(unit as u32).unwrap_or('\u{FFFD}'), 1)
}

impl ImmutableStringReader for StringReader {
    fn get_string(&self) -> &str {
        &self.string
    }

    fn get_remaining_length(&self) -> i32 {
        self.units.len() as i32 - self.cursor
    }

    fn get_total_length(&self) -> i32 {
        self.units.len() as i32
    }

    fn get_cursor(&self) -> i32 {
        self.cursor
    }

    /// `getRead()` — `input.substring(0, cursor)`.
    ///
    /// When `cursor` splits a surrogate pair, Java's substring returns the raw lone
    /// surrogate code unit while `from_utf16_lossy` renders U+FFFD (Rust's `String`
    /// cannot hold lone surrogates). Divergence is inherent to Rust's String model;
    /// documented (see also `decode_char`).
    fn get_read(&self) -> String {
        utf16_units_to_string(&self.units[0..self.cursor as usize])
    }

    /// `getRemaining()` — `input.substring(cursor)`.
    ///
    /// Same lone-surrogate -> U+FFFD rendering note as `get_read()` when `cursor`
    /// splits a surrogate pair.
    fn get_remaining(&self) -> String {
        utf16_units_to_string(&self.units[self.cursor as usize..])
    }

    fn can_read_with_length(&self, length: i32) -> bool {
        self.cursor.wrapping_add(length) <= self.units.len() as i32
    }

    fn peek(&self) -> char {
        decode_char(&self.units, self.cursor as usize).0
    }

    fn peek_with_offset(&self, offset: i32) -> char {
        decode_char(&self.units, self.cursor.wrapping_add(offset) as usize).0
    }
}

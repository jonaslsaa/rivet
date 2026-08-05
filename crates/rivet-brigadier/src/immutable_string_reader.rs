//! Port of `com.mojang.brigadier.ImmutableStringReader` (upstream).

/// Java `ImmutableStringReader` interface — the read-only surface of a command
/// string reader, implemented by `StringReader`.
///
/// Java indices are UTF-16 code units (PORTING.md: command parsing counts UTF-16
/// units, not bytes): `get_remaining_length`, `get_cursor`, `get_read`,
/// `get_remaining` and the `peek` variants all operate on UTF-16 code units.
/// `get_read`/`get_remaining` return owned `String`s because a slice of a Rust
/// `&str` by a UTF-16 index cannot be returned as a `&str` view.
pub trait ImmutableStringReader {
    /// `getString()` — the full input string.
    fn get_string(&self) -> &str;
    /// `getRemainingLength()` — UTF-16 code units left after the cursor.
    fn get_remaining_length(&self) -> i32;
    /// `getTotalLength()` — UTF-16 code-unit length of the input.
    fn get_total_length(&self) -> i32;
    /// `getCursor()` — current UTF-16 code-unit position.
    fn get_cursor(&self) -> i32;
    /// `getRead()` — `input.substring(0, cursor)`.
    fn get_read(&self) -> String;
    /// `getRemaining()` — `input.substring(cursor)`.
    fn get_remaining(&self) -> String;
    /// `canRead(int length)` — whether `cursor + length` code units remain.
    fn can_read_with_length(&self, length: i32) -> bool;
    /// `canRead()` — whether at least one code unit remains.
    fn can_read(&self) -> bool {
        self.can_read_with_length(1)
    }
    /// `peek()` — the char at the cursor. A surrogate pair decodes to one char.
    fn peek(&self) -> char;
    /// `peek(int offset)` — the char `offset` code units ahead.
    fn peek_with_offset(&self, offset: i32) -> char;
}

/// Decode UTF-16 code units to a Rust `String`.
///
/// Input produced by `str::encode_utf16` is well-formed UTF-16 (no lone
/// surrogates), so `from_utf16_lossy` reproduces it exactly.
pub(crate) fn utf16_units_to_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

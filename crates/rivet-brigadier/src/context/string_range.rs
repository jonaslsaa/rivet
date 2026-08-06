//! Port of `com.mojang.brigadier.context.StringRange` (upstream brigadier-1.3.10).

use crate::ImmutableStringReader;
use crate::immutable_string_reader::utf16_units_to_string;

/// Java `StringRange` — a half-open `[start, end)` range over UTF-16 code units.
///
/// Java `String.substring` indices are UTF-16 code-unit offsets, so `get`/`get_reader`
/// slice by code units, not bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringRange {
    start: i32,
    end: i32,
}

impl StringRange {
    /// Java `StringRange(int start, int end)`.
    pub fn new(start: i32, end: i32) -> Self {
        StringRange { start, end }
    }

    /// Java `StringRange.at(int pos)`.
    pub fn at(pos: i32) -> Self {
        StringRange {
            start: pos,
            end: pos,
        }
    }

    /// Java `StringRange.between(int start, int end)`.
    pub fn between(start: i32, end: i32) -> Self {
        StringRange { start, end }
    }

    /// Java `StringRange.encompassing(StringRange, StringRange)`.
    pub fn encompassing(a: &StringRange, b: &StringRange) -> Self {
        StringRange {
            start: i32::min(a.get_start(), b.get_start()),
            end: i32::max(a.get_end(), b.get_end()),
        }
    }

    /// Java `getStart()`.
    pub fn get_start(&self) -> i32 {
        self.start
    }

    /// Java `getEnd()`.
    pub fn get_end(&self) -> i32 {
        self.end
    }

    /// Java `get(ImmutableStringReader)` — `reader.getString().substring(start, end)`.
    pub fn get_reader(&self, reader: &dyn ImmutableStringReader) -> String {
        self.get_string(reader.get_string())
    }

    /// Java `get(String)` — `string.substring(start, end)` in UTF-16 code units.
    pub fn get_string(&self, string: &str) -> String {
        let units: Vec<u16> = string.encode_utf16().collect();
        // Java's substring throws on out-of-range indices; here clamp is used for
        // the handful of defensive callers. The indices produced by the parser are
        // always in range.
        let start = i32::max(0, self.start) as usize;
        let end = i32::min(units.len() as i32, self.end) as usize;
        utf16_units_to_string(&units[start..end])
    }

    /// Java `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Java `getLength()`.
    pub fn get_length(&self) -> i32 {
        self.end - self.start
    }

    /// Java `hashCode()` — `Objects.hash(start, end)`.
    pub fn hash_code(&self) -> i32 {
        crate::java_hash::objects_hash(&[self.start, self.end])
    }
}

impl std::fmt::Display for StringRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StringRange{{start={}, end={}}}", self.start, self.end)
    }
}

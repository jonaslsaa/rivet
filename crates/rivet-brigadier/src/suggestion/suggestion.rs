//! Port of `com.mojang.brigadier.suggestion.Suggestion` (upstream brigadier-1.3.10),
//! including the `IntegerSuggestion` subclass.
//!
//! Java has `Suggestion` as a base class and `IntegerSuggestion extends Suggestion`.
//! Rust models both with one struct carrying an optional `Integer(i32)` kind: the
//! two only differ in `equals`/`hashCode` (subclass checks) and the `compareTo`
//! family (numeric vs text). The kind is preserved through `expand` only when the
//! range is unchanged — Java's `expand` returns a plain `Suggestion` (`new
//! Suggestion(...)`) when the range differs, dropping the integer subclass.

use std::sync::Arc;

use crate::Message;
use crate::context::StringRange;

/// Java `Suggestion` / `IntegerSuggestion`.
#[derive(Clone)]
pub struct Suggestion {
    range: StringRange,
    text: String,
    tooltip: Option<Arc<dyn Message>>,
    kind: Kind,
}

impl std::fmt::Debug for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Suggestion")
            .field("range", &self.range)
            .field("text", &self.text)
            .field("kind", &self.kind)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Integer(i32),
}

impl Suggestion {
    /// Java `Suggestion(StringRange, String)`.
    pub fn new(range: StringRange, text: impl Into<String>) -> Self {
        Suggestion {
            range,
            text: text.into(),
            tooltip: None,
            kind: Kind::Text,
        }
    }

    /// Java `Suggestion(StringRange, String, Message)`.
    pub fn new_with_tooltip(
        range: StringRange,
        text: impl Into<String>,
        tooltip: Arc<dyn Message>,
    ) -> Self {
        Suggestion {
            range,
            text: text.into(),
            tooltip: Some(tooltip),
            kind: Kind::Text,
        }
    }

    /// Java `IntegerSuggestion(StringRange, int)`.
    pub fn integer(range: StringRange, value: i32) -> Self {
        Suggestion {
            range,
            text: value.to_string(),
            tooltip: None,
            kind: Kind::Integer(value),
        }
    }

    /// Java `IntegerSuggestion(StringRange, int, Message)`.
    pub fn integer_with_tooltip(range: StringRange, value: i32, tooltip: Arc<dyn Message>) -> Self {
        Suggestion {
            range,
            text: value.to_string(),
            tooltip: Some(tooltip),
            kind: Kind::Integer(value),
        }
    }

    /// Java `IntegerSuggestion.getValue()`.
    pub fn get_value(&self) -> Option<i32> {
        match self.kind {
            Kind::Integer(value) => Some(value),
            Kind::Text => None,
        }
    }

    /// Java `getRange()`.
    pub fn get_range(&self) -> StringRange {
        self.range
    }

    /// Java `getText()`.
    pub fn get_text(&self) -> &str {
        &self.text
    }

    /// Java `getTooltip()`.
    pub fn get_tooltip(&self) -> Option<&dyn Message> {
        self.tooltip.as_deref()
    }

    /// Java `apply(String)` — replace the range in the input with this text.
    ///
    /// Java `String.substring` indices are UTF-16 code units, so the input is
    /// sliced by code units, not bytes.
    pub fn apply(&self, input: &str) -> String {
        let units: Vec<u16> = input.encode_utf16().collect();
        let total = units.len() as i32;
        if self.range.get_start() == 0 && self.range.get_end() == total {
            return self.text.clone();
        }
        let mut result = String::new();
        if self.range.get_start() > 0 {
            result.push_str(&crate::immutable_string_reader::utf16_units_to_string(
                &units[0..self.range.get_start() as usize],
            ));
        }
        result.push_str(&self.text);
        if self.range.get_end() < total {
            result.push_str(&crate::immutable_string_reader::utf16_units_to_string(
                &units[self.range.get_end() as usize..],
            ));
        }
        result
    }

    /// Java `expand(String command, StringRange range)` — widen this suggestion to
    /// cover `range`, pulling the command's intervening text in. When the range
    /// changes, the result is a plain `Suggestion` (Java `new Suggestion(...)`), so
    /// the integer kind is dropped.
    pub fn expand(&self, command: &str, range: StringRange) -> Suggestion {
        if range == self.range {
            return self.clone();
        }
        let units: Vec<u16> = command.encode_utf16().collect();
        let mut result = String::new();
        if range.get_start() < self.range.get_start() {
            result.push_str(&crate::immutable_string_reader::utf16_units_to_string(
                &units[range.get_start() as usize..self.range.get_start() as usize],
            ));
        }
        result.push_str(&self.text);
        if range.get_end() > self.range.get_end() {
            result.push_str(&crate::immutable_string_reader::utf16_units_to_string(
                &units[self.range.get_end() as usize..range.get_end() as usize],
            ));
        }
        Suggestion {
            range,
            text: result,
            tooltip: self.tooltip.clone(),
            kind: Kind::Text,
        }
    }

    /// Java `compareTo(Suggestion)`. For a plain `Suggestion`: text compare. For an
    /// `IntegerSuggestion`: numeric when the other is an integer suggestion, else the
    /// inherited text compare.
    pub fn compare_to(&self, other: &Suggestion) -> std::cmp::Ordering {
        if let (Kind::Integer(self_value), Kind::Integer(other_value)) = (self.kind, other.kind) {
            return self_value.cmp(&other_value);
        }
        utf16_compare(&self.text, &other.text)
    }

    /// Java `compareToIgnoreCase(Suggestion)`. For an `IntegerSuggestion` this
    /// delegates to `compareTo`; a plain `Suggestion` compares text ignoring case.
    pub fn compare_to_ignore_case(&self, other: &Suggestion) -> std::cmp::Ordering {
        if matches!(self.kind, Kind::Integer(_)) {
            return self.compare_to(other);
        }
        compare_ignore_case(&self.text, &other.text)
    }

    /// Java `hashCode()`. Plain: `Objects.hash(range, text, tooltip)`.
    /// `IntegerSuggestion`: `Objects.hash(super.hashCode(), value)`. Tooltips have
    /// Java identity semantics, hashed by pointer.
    pub fn hash_code(&self) -> i32 {
        let tooltip_hash = self
            .tooltip
            .as_ref()
            .map_or(0, |m| Arc::as_ptr(m) as *const () as usize as i32);
        let base = crate::java_hash::objects_hash(&[
            self.range.hash_code(),
            crate::java_hash::string_hash(&self.text),
            tooltip_hash,
        ]);
        match self.kind {
            Kind::Integer(value) => crate::java_hash::objects_hash(&[base, value]),
            Kind::Text => base,
        }
    }
}

/// Java `String.compareTo` over UTF-16 code units. Rust `str::cmp` compares bytes,
/// which diverges for non-ASCII text (a UTF-16 `<` ordering differs from a UTF-8
/// one). Code-unit ordering reproduces Java exactly.
pub fn utf16_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let au: Vec<u16> = a.encode_utf16().collect();
    let bu: Vec<u16> = b.encode_utf16().collect();
    au.cmp(&bu)
}

/// Java `String.compareToIgnoreCase` (the `CASE_INSENSITIVE_ORDER` comparator): per
/// code unit, if the raw units differ, fold each to upper case and then to lower
/// case; only a difference at every fold yields a result, otherwise the compare
/// continues to the next unit, and finally falls back to the length difference.
/// Java folds one UTF-16 unit to one unit via `Character.toUpperCase`/
/// `toLowerCase`; `char::to_lowercase` can expand to multiple chars (e.g. 'İ'), so a
/// multi-char fold falls back to the raw unit. Divergence is confined to
/// supplementary/multifold text, which the ported tests don't reach.
pub fn compare_ignore_case(a: &str, b: &str) -> std::cmp::Ordering {
    let au: Vec<u16> = a.encode_utf16().collect();
    let bu: Vec<u16> = b.encode_utf16().collect();
    let mut i = 0;
    while i < au.len() && i < bu.len() {
        let ac = char::from_u32(au[i] as u32).unwrap_or('\u{FFFD}');
        let bc = char::from_u32(bu[i] as u32).unwrap_or('\u{FFFD}');
        if ac != bc {
            let ac_up = single_unit_fold(ac, false);
            let bc_up = single_unit_fold(bc, false);
            if ac_up != bc_up {
                // Java lower-folds the upper-folded char; round-trip the unit.
                let ac_up_c = char::from_u32(ac_up as u32).unwrap_or('\u{FFFD}');
                let bc_up_c = char::from_u32(bc_up as u32).unwrap_or('\u{FFFD}');
                let ac_low = single_unit_fold(ac_up_c, true);
                let bc_low = single_unit_fold(bc_up_c, true);
                if ac_low != bc_low {
                    return ac_low.cmp(&bc_low);
                }
            }
        }
        i += 1;
    }
    au.len().cmp(&bu.len())
}

/// Fold a char to a single UTF-16 code unit; `lower` selects lower/upper case.
/// A fold that would produce multiple chars (Java folds to exactly one code unit)
/// returns the original char — a per-char fallback, faithful for single-unit chars.
fn single_unit_fold(c: char, lower: bool) -> u16 {
    let folded = if lower {
        c.to_lowercase().collect::<String>()
    } else {
        c.to_uppercase().collect::<String>()
    };
    let units: Vec<u16> = folded.encode_utf16().collect();
    if units.len() == 1 { units[0] } else { c as u16 }
}

/// Java `equals`. Plain `Suggestion.equals` is `o instanceof Suggestion` (any
/// subclass) + `Objects.equals(range, text, tooltip)`. `IntegerSuggestion.equals`
/// additionally requires `o instanceof IntegerSuggestion` — reproduced by `kind`
/// matching (a text suggestion never equals an integer one).
impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        match (self.kind, other.kind) {
            (Kind::Integer(_), Kind::Integer(_)) | (Kind::Text, Kind::Text) => {}
            _ => return false,
        }
        self.range == other.range
            && self.text == other.text
            && match (&self.tooltip, &other.tooltip) {
                (None, None) => true,
                (Some(x), Some(y)) => Arc::ptr_eq(x, y),
                _ => false,
            }
    }
}

impl Eq for Suggestion {}

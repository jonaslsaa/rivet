//! Port of `net.minecraft.nbt.TagParser` — the entry points for parsing SNBT
//! into tags.
//!
//! Java `TagParser` is thin: it delegates to a `Grammar<T>` built by
//! `SnbtGrammar.createParser` over the packrat machinery
//! (`net.minecraft.util.parsing.packrat`). That package is not yet ported, so
//! per the unit notes the grammar is re-expressed here as a hand-written
//! recursive-descent parser that reproduces the packrat grammar's accepted
//! language and its error messages/positions (the packrat layer itself is
//! STUB(mc.nbt.snbt)).
//!
//! Key fidelity points (from `SnbtGrammar.createParser`):
//! - Number dispatch: a numeric lookahead picks the float-vs-integer path; an
//!   integer literal after `0` is committed (leading-zero / hex / binary), with
//!   no fallback to a plain decimal.
//! - `Term.cut()` semantics: a committed alternative does not fall back to a
//!   later branch (e.g. an unterminated double-quoted string never re-parses as
//!   a single-quoted one; after `{`/`[`/quote the unquoted fallback is closed).
//! - `IntegerLiteral.create` uses Java's signed/unsigned parse semantics with
//!   per-type ranges; floats use Java `parseFloat`/`parseDouble` semantics
//!   (overflow → infinity → "Non-finite numbers are not allowed", underflow →
//!   ±0.0).
//! - Brigadier `CommandSyntaxException` is approximated by
//!   `NbtFormatException` carrying the full `getMessage()` text, i.e.
//!   `"<reason> at position <cursor>: <context><--[HERE]"`.
//!
//! STUB(mc.nbt.snbt): the public `Codec<CompoundTag>` surfaces
//! `TagParser.FLATTENED_CODEC` and `TagParser.LENIENT_CODEC`, and the public
//! `char` constants `ELEMENT_SEPARATOR` (`,`) and `NAME_VALUE_SEPARATOR`
//! (`:`), are not ported — they are the DFU `Codec`/`Codec.STRING` /
//! `Codec.withAlternative` surface, which lands with the DFU port. They are
//! called out here so the omission is explicit rather than silent. (The
//! printer defines its own private `ELEMENT_SEPARATOR`/`NAME_VALUE_SEPARATOR`,
//! which are a different class's constants and are ported in
//! `snbt_printer_tag_visitor`.)

use crate::compound_tag::CompoundTag;
use crate::nbt_format_exception::NbtFormatException;
use crate::nbt_ops::NbtOps;
use crate::snbt_operations::{BUILTIN_FALSE, BUILTIN_TRUE, BuiltinKey, find_builtin, run_builtin};
use crate::tag::Tag;
use rivet_serialization::dynamic_ops::{DynamicOps, Pair};

// ---- Unicode helpers (the `StringReader.skipWhitespace` /
// `Character.isWhitespace` surface the grammar needs). Java indices are UTF-16
// code units (observable in SNBT error positions); the parser tracks a cursor
// in UTF-16 units directly. ----

/// `Character.isWhitespace(int)` — Unicode space separators (excluding
/// non-breaking spaces 0x00A0/0x2007/0x202F) plus the ASCII/ISO control runs.
fn java_is_whitespace(cp: u32) -> bool {
    match cp {
        0x09..=0x0D => true,     // \t \n \x0B \f \r
        0x1C..=0x1F => true,     // file/group/record/unit separators
        0x20 => true,            // space
        0x1680 => true,          // OGHAM SPACE MARK
        0x2000..=0x2006 => true, // en..em quad/space
        0x2008..=0x200A => true, // punctuation/hair/line spaces
        0x2028 => true,          // LINE SEPARATOR
        0x2029 => true,          // PARAGRAPH SEPARATOR
        0x205F => true,          // MEDIUM MATHEMATICAL SPACE
        0x3000 => true,          // IDEOGRAPHIC SPACE
        _ => false,
    }
}

/// Decode the code point at UTF-16 `units[i]`; returns `(cp, units_consumed)`.
fn code_point_at(units: &[u16], i: usize) -> (u32, usize) {
    let c = units[i] as u32;
    if (0xD800..=0xDBFF).contains(&c) && i + 1 < units.len() {
        let lo = units[i + 1] as u32;
        if (0xDC00..=0xDFFF).contains(&lo) {
            return (((c - 0xD800) << 10) + (lo - 0xDC00) + 0x10000, 2);
        }
    }
    (c, 1)
}

fn region_to_string(units: &[u16], start: usize, end: usize) -> String {
    String::from_utf16(&units[start..end]).unwrap_or_default()
}

fn char_from_u16(u: u16) -> String {
    char::from_u32(u as u32)
        .map(|c| c.to_string())
        .unwrap_or_default()
}

// ---- SNBT error message text (en_us.json translations for the
// `snbt.parser.*` / `argument.nbt.*` keys). ----

const ERROR_TRAILING_DATA: &str = "Unexpected trailing data";
const ERROR_EXPECTED_COMPOUND: &str = "Expected compound tag";
const ERROR_NUMBER_PARSE_FAILURE_PREFIX: &str = "Failed to parse number: ";
const ERROR_EXPECTED_HEX_ESCAPE_PREFIX: &str = "Expected a character literal of length ";
const ERROR_INVALID_CODEPOINT_PREFIX: &str = "Invalid Unicode character value: ";
const ERROR_NO_SUCH_OPERATION_PREFIX: &str = "No such operation: ";
const ERROR_EXPECTED_INTEGER_TYPE: &str = "Expected an integer number";
const ERROR_EXPECTED_FLOAT_TYPE: &str = "Expected a floating point number";
const ERROR_EXPECTED_NON_NEGATIVE_NUMBER: &str = "Expected a non-negative number";
const ERROR_INVALID_CHARACTER_NAME: &str = "Invalid Unicode character name";
const ERROR_INVALID_ARRAY_ELEMENT_TYPE: &str = "Invalid array element type";
const ERROR_INVALID_UNQUOTED_START: &str = "Unquoted strings can't start with digits 0-9, + or -";
const ERROR_EXPECTED_UNQUOTED_STRING: &str = "Expected a valid unquoted string";
const ERROR_INVALID_STRING_CONTENTS: &str = "Invalid string contents";
const ERROR_EXPECTED_BINARY_NUMERAL: &str = "Expected a binary number";
const ERROR_EXPECTED_DECIMAL_NUMERAL: &str = "Expected a decimal number";
const ERROR_EXPECTED_HEX_NUMERAL: &str = "Expected a hexadecimal number";
const ERROR_EMPTY_KEY: &str = "Key cannot be empty";
const ERROR_LEADING_ZERO_NOT_ALLOWED: &str = "Decimal numbers can't start with 0";
const ERROR_INFINITY_NOT_ALLOWED: &str = "Non-finite numbers are not allowed";
// Java source spells the key `snbt.parser.undescore_not_allowed`.
const ERROR_UNDERSCORE_NOT_ALLOWED: &str =
    "Underscore characters are not allowed at the start or end of a number";
/// `CommandSyntaxException.BUILT_IN_EXCEPTIONS.literalIncorrect()` prefix.
const LITERAL_INCORRECT_PREFIX: &str = "Expected literal ";

// ---- `SnbtGrammar` grammar helpers and value types. ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    Plus,
    Minus,
}

impl Sign {
    /// `Sign.append(StringBuilder)` — only MINUS writes a `-`.
    fn append(self, out: &mut String) {
        if self == Sign::Minus {
            out.push('-');
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
    Binary,
    Decimal,
    Hex,
}

impl Base {
    fn radix(self) -> u32 {
        match self {
            Base::Binary => 2,
            Base::Decimal => 10,
            Base::Hex => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSuffix {
    Float,
    Double,
    Byte,
    Short,
    Int,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignedPrefix {
    Signed,
    Unsigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntegerSuffix {
    signed: Option<SignedPrefix>,
    ty: Option<TypeSuffix>,
}

impl IntegerSuffix {
    const EMPTY: IntegerSuffix = IntegerSuffix {
        signed: None,
        ty: None,
    };
}

#[derive(Debug, Clone)]
struct Signed<T> {
    sign: Sign,
    value: T,
}

#[derive(Debug, Clone)]
struct IntegerLiteral {
    sign: Sign,
    base: Base,
    digits: String,
    suffix: IntegerSuffix,
}

impl IntegerLiteral {
    /// `IntegerLiteral.signedOrDefault()`.
    fn signed_or_default(&self) -> SignedPrefix {
        if let Some(s) = self.suffix.signed {
            return s;
        }
        match self.base {
            Base::Binary | Base::Hex => SignedPrefix::Unsigned,
            Base::Decimal => SignedPrefix::Signed,
        }
    }

    /// `IntegerLiteral.cleanupDigits(Sign)` — leading `-` for MINUS, and
    /// underscores stripped when a `-` or `_` is present.
    fn cleanup_digits(&self) -> String {
        let needs_underscore_removal = self.digits.contains('_');
        if self.sign != Sign::Minus && !needs_underscore_removal {
            return self.digits.clone();
        }
        let mut out = String::new();
        self.sign.append(&mut out);
        for c in self.digits.chars() {
            if c != '_' {
                out.push(c);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayPrefix {
    Byte,
    Int,
    Long,
}

impl ArrayPrefix {
    fn default_type(self) -> TypeSuffix {
        match self {
            ArrayPrefix::Byte => TypeSuffix::Byte,
            ArrayPrefix::Int => TypeSuffix::Int,
            ArrayPrefix::Long => TypeSuffix::Long,
        }
    }

    fn is_allowed(self, ty: TypeSuffix) -> bool {
        match self {
            ArrayPrefix::Byte => ty == TypeSuffix::Byte,
            ArrayPrefix::Int => {
                matches!(ty, TypeSuffix::Int | TypeSuffix::Byte | TypeSuffix::Short)
            }
            ArrayPrefix::Long => matches!(
                ty,
                TypeSuffix::Long | TypeSuffix::Byte | TypeSuffix::Short | TypeSuffix::Int
            ),
        }
    }

    /// `ArrayPrefix.computeType(IntegerSuffix)`.
    fn compute_type(self, lit: &IntegerLiteral) -> Option<TypeSuffix> {
        match lit.suffix.ty {
            None => Some(self.default_type()),
            Some(ty) => {
                if self.is_allowed(ty) {
                    Some(ty)
                } else {
                    None
                }
            }
        }
    }
}

/// A stored parse error, mirroring `ErrorCollector.LongestOnly` (first error
/// at the farthest cursor wins; on a tie the earliest stored wins).
#[derive(Debug)]
struct ErrorInfo {
    cursor: usize,
    message: String,
    /// true → a brigadier `CommandSyntaxException`-style message (position +
    /// context suffix); false → a raw exception message (e.g. "Too deep").
    cmd_syntax: bool,
}

/// The recursive-descent SNBT engine over `NbtOps` (`Tag`).
struct SnbtParser {
    units: Vec<u16>,
    cursor: usize,
    /// Paper depth tracking (`Scope.increaseDepth`/`decreaseDepth`).
    depth: i32,
    ops: NbtOps,
    best: Option<ErrorInfo>,
}

impl SnbtParser {
    fn new(input: &str, ops: NbtOps) -> SnbtParser {
        SnbtParser {
            units: input.encode_utf16().collect(),
            cursor: 0,
            depth: 0,
            ops,
            best: None,
        }
    }

    fn mark(&self) -> usize {
        self.cursor
    }

    fn restore(&mut self, mark: usize) {
        self.cursor = mark;
    }

    fn can_read(&self) -> bool {
        self.cursor < self.units.len()
    }

    fn peek(&self) -> u16 {
        self.units[self.cursor]
    }

    /// `ErrorCollector.store(cursor, reason)` — LongestOnly: replace only when
    /// the cursor is strictly farther than any prior error.
    ///
    /// Fidelity note: Java's `LongestOnly` keeps *all* entries at the farthest
    /// cursor and `Grammar.parseForCommands` throws the *first*
    /// `CommandSyntaxException` among them (and the sole `RuntimeException` if
    /// exactly one). Here the farthest-cursor single error is kept (earlier
    /// tied entries dropped), which can change which error text surfaces when
    /// two errors land at the same cursor. The accepted language is unaffected.
    fn store(&mut self, cursor: usize, message: impl Into<String>, cmd_syntax: bool) {
        let message = message.into();
        match &self.best {
            Some(b) if cursor <= b.cursor => {}
            _ => {
                self.best = Some(ErrorInfo {
                    cursor,
                    message,
                    cmd_syntax,
                });
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.cursor < self.units.len() {
            let (cp, len) = code_point_at(&self.units, self.cursor);
            if java_is_whitespace(cp) {
                self.cursor += len;
            } else {
                break;
            }
        }
    }

    /// `TerminalCharacters.parse` for a single literal char — skips
    /// whitespace, consumes the char if accepted, else stores
    /// "Expected literal <c>" at the post-whitespace cursor.
    fn try_char(&mut self, c: u16) -> bool {
        self.skip_whitespace();
        let cursor = self.mark();
        if self.can_read() && self.peek() == c {
            self.cursor += 1;
            true
        } else {
            self.store(
                cursor,
                format!("{LITERAL_INCORRECT_PREFIX}{}", char_from_u16(c)),
                true,
            );
            false
        }
    }

    /// `StringReaderTerms.characters(v1, v2)` — accepts either char; the
    /// literal error joins them with `|`.
    fn try_chars(&mut self, c1: u16, c2: u16) -> bool {
        self.skip_whitespace();
        let cursor = self.mark();
        if self.can_read() && (self.peek() == c1 || self.peek() == c2) {
            self.cursor += 1;
            true
        } else {
            self.store(
                cursor,
                format!(
                    "{LITERAL_INCORRECT_PREFIX}{}|{}",
                    char_from_u16(c1),
                    char_from_u16(c2)
                ),
                true,
            );
            false
        }
    }

    /// A positive lookahead that restores the cursor and never stores errors
    /// (`Term.positiveLookahead` parses on `state.silent()`).
    fn lookahead_after_whitespace(&mut self, pred: fn(u16) -> bool) -> bool {
        let mark = self.mark();
        self.skip_whitespace();
        let result = self.can_read() && pred(self.peek());
        self.restore(mark);
        result
    }

    // ---- terminal char classes ----

    fn is_number_start(c: u16) -> bool {
        matches!(
            c,
            0x2B | 0x2D | 0x2E | 0x30..=0x39 // + - . 0-9
        )
    }

    fn is_allowed_in_unquoted_string(c: u16) -> bool {
        matches!(
            c,
            0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x2D | 0x2E | 0x2B | 0x5F
            // 0-9 A-Z a-z - . + _
        )
    }

    fn is_plain_string_char(c: u16) -> bool {
        // PLAIN_STRING_CHUNK: anything except " ' \.
        c != 0x22 && c != 0x27 && c != 0x5C
    }

    fn is_hex_digit(c: u16) -> bool {
        matches!(
            c,
            0x30..=0x39 | 0x41..=0x46 | 0x61..=0x66 // 0-9 A-F a-f
        )
    }

    fn is_unicode_name_char(c: u16) -> bool {
        matches!(
            c,
            0x2D | 0x41..=0x5A | 0x61..=0x7A | 0x30..=0x39 | 0x20 // - A-Z a-z 0-9 space
        )
    }

    // ---- rules ----

    /// `sign` rule.
    fn parse_sign(&mut self) -> Option<Sign> {
        let mark = self.mark();
        if self.try_char(0x2B) {
            return Some(Sign::Plus);
        }
        self.restore(mark);
        if self.try_char(0x2D) {
            return Some(Sign::Minus);
        }
        self.restore(mark);
        None
    }

    /// `NumberRunParseRule` — skips whitespace, reads the run of accepted
    /// chars, rejects a leading/trailing underscore.
    fn parse_numeral(
        &mut self,
        no_value_error: &str,
        is_accepted: fn(u16) -> bool,
    ) -> Option<String> {
        self.skip_whitespace();
        let start = self.cursor;
        let mut pos = start;
        while pos < self.units.len() && is_accepted(self.units[pos]) {
            pos += 1;
        }
        let length = pos - start;
        if length == 0 {
            self.store(self.mark(), no_value_error, true);
            return None;
        }
        if self.units[start] != b'_' as u16 && self.units[pos - 1] != b'_' as u16 {
            self.cursor = pos;
            Some(region_to_string(&self.units, start, pos))
        } else {
            self.store(self.mark(), ERROR_UNDERSCORE_NOT_ALLOWED, true);
            None
        }
    }

    fn parse_decimal_numeral(&mut self) -> Option<String> {
        self.parse_numeral(ERROR_EXPECTED_DECIMAL_NUMERAL, |c| {
            matches!(c, 0x30..=0x39 | 0x5F) // 0-9 _
        })
    }

    fn parse_binary_numeral(&mut self) -> Option<String> {
        self.parse_numeral(ERROR_EXPECTED_BINARY_NUMERAL, |c| {
            matches!(c, 0x30 | 0x31 | 0x5F) // 0 1 _
        })
    }

    fn parse_hex_numeral(&mut self) -> Option<String> {
        self.parse_numeral(ERROR_EXPECTED_HEX_NUMERAL, |c| {
            Self::is_hex_digit(c) || c == b'_' as u16
        })
    }

    /// `integerSuffix` rule.
    /// The `characters('b','B')`-style terminal — one of the type letters,
    /// either case (the grammar's `characters(v1, v2)` accepts both).
    fn parse_suffix_type_letter(&mut self) -> Option<TypeSuffix> {
        let mark = self.mark();
        if self.try_chars(b'b' as u16, b'B' as u16) {
            return Some(TypeSuffix::Byte);
        }
        self.restore(mark);
        if self.try_chars(b's' as u16, b'S' as u16) {
            return Some(TypeSuffix::Short);
        }
        self.restore(mark);
        if self.try_chars(b'i' as u16, b'I' as u16) {
            return Some(TypeSuffix::Int);
        }
        self.restore(mark);
        if self.try_chars(b'l' as u16, b'L' as u16) {
            return Some(TypeSuffix::Long);
        }
        self.restore(mark);
        None
    }

    fn parse_integer_suffix(&mut self) -> Option<IntegerSuffix> {
        let mark = self.mark();
        // Branch: u/U then a type letter.
        if self.try_chars(b'u' as u16, b'U' as u16)
            && let Some(ty) = self.parse_suffix_type_letter()
        {
            return Some(IntegerSuffix {
                signed: Some(SignedPrefix::Unsigned),
                ty: Some(ty),
            });
        }
        self.restore(mark);
        // Branch: s/S then a type letter.
        if self.try_chars(b's' as u16, b'S' as u16)
            && let Some(ty) = self.parse_suffix_type_letter()
        {
            return Some(IntegerSuffix {
                signed: Some(SignedPrefix::Signed),
                ty: Some(ty),
            });
        }
        self.restore(mark);
        // Plain type letters (either case).
        if self.try_chars(b'b' as u16, b'B' as u16) {
            return Some(IntegerSuffix {
                signed: None,
                ty: Some(TypeSuffix::Byte),
            });
        }
        self.restore(mark);
        if self.try_chars(b's' as u16, b'S' as u16) {
            return Some(IntegerSuffix {
                signed: None,
                ty: Some(TypeSuffix::Short),
            });
        }
        self.restore(mark);
        if self.try_chars(b'i' as u16, b'I' as u16) {
            return Some(IntegerSuffix {
                signed: None,
                ty: Some(TypeSuffix::Int),
            });
        }
        self.restore(mark);
        if self.try_chars(b'l' as u16, b'L' as u16) {
            return Some(IntegerSuffix {
                signed: None,
                ty: Some(TypeSuffix::Long),
            });
        }
        self.restore(mark);
        None
    }

    /// `integerLiteral` rule — `[sign] ( '0' <hex|bin|leading-zero|"0"> |
    /// decimalNumeral ) [integerSuffix]`. After `0` is consumed the branch is
    /// committed (the grammar's `cut` after `'0'`).
    fn parse_integer_literal(&mut self) -> Option<IntegerLiteral> {
        let sign_mark = self.mark();
        let sign = match self.parse_sign() {
            Some(s) => s,
            None => {
                self.restore(sign_mark);
                Sign::Plus
            }
        };
        let mark = self.mark();
        if self.try_char(b'0' as u16) {
            // Committed to branch A (the grammar cuts after '0').
            let inner_mark = self.mark();
            let mut cut = false;
            let mut result: Option<(Base, String)> = None;
            // Branch 1: x/X hexNumeral.
            {
                let m = self.mark();
                if self.try_chars(b'x' as u16, b'X' as u16) {
                    cut = true;
                    result = self.parse_hex_numeral().map(|h| (Base::Hex, h));
                    if result.is_none() {
                        self.restore(m);
                    }
                } else {
                    self.restore(m);
                }
            }
            // Branch 2: b/B binaryNumeral.
            if result.is_none() && !cut {
                let m = self.mark();
                if self.try_chars(b'b' as u16, b'B' as u16) {
                    result = self.parse_binary_numeral().map(|b| (Base::Binary, b));
                    if result.is_none() {
                        self.restore(m);
                    }
                } else {
                    self.restore(m);
                }
            }
            // Branch 3: decimalNumeral then `cut` + fail(leading zero). The
            // inner alternative breaks on cut → branch A fails (Java mirrors
            // this with `Term.cut` then `Term.fail(ERROR_LEADING_ZERO...)`).
            if result.is_none() && !cut {
                let m = self.mark();
                if self.parse_decimal_numeral().is_some() {
                    self.store(self.mark(), ERROR_LEADING_ZERO_NOT_ALLOWED, true);
                    self.restore(m);
                    self.restore(inner_mark);
                    self.restore(mark);
                    return None;
                }
                self.restore(m);
            }
            // Branch 4: marker(decimalNumeral, "0").
            if result.is_none() && !cut {
                result = Some((Base::Decimal, "0".to_string()));
            }
            let (base, digits) = result?;
            let suffix_mark = self.mark();
            let suffix = match self.parse_integer_suffix() {
                Some(s) => s,
                None => {
                    self.restore(suffix_mark);
                    IntegerSuffix::EMPTY
                }
            };
            Some(IntegerLiteral {
                sign,
                base,
                digits,
                suffix,
            })
        } else {
            self.restore(mark);
            // Branch B: decimalNumeral [integerSuffix].
            let digits = self.parse_decimal_numeral()?;
            let suffix_mark = self.mark();
            let suffix = match self.parse_integer_suffix() {
                Some(s) => s,
                None => {
                    self.restore(suffix_mark);
                    IntegerSuffix::EMPTY
                }
            };
            Some(IntegerLiteral {
                sign,
                base: Base::Decimal,
                digits,
                suffix,
            })
        }
    }

    /// `IntegerLiteral.create(DynamicOps, TypeSuffix, ParseState)` — the typed
    /// numeric value (as `i64`, with unsigned types wrapping), storing the
    /// grammar's errors.
    fn integer_literal_value(&mut self, lit: &IntegerLiteral, ty: TypeSuffix) -> Option<i64> {
        let is_signed = lit.signed_or_default() == SignedPrefix::Signed;
        if !is_signed && lit.sign == Sign::Minus {
            self.store(self.mark(), ERROR_EXPECTED_NON_NEGATIVE_NUMBER, true);
            return None;
        }
        let fixed_digits = lit.cleanup_digits();
        let radix = lit.base.radix();
        let result = if is_signed {
            match ty {
                TypeSuffix::Byte => {
                    parse_checked(&fixed_digits, radix, i8::MIN as i128, i8::MAX as i128)
                        .map(|v| v as i8 as i64)
                }
                TypeSuffix::Short => {
                    parse_checked(&fixed_digits, radix, i16::MIN as i128, i16::MAX as i128)
                        .map(|v| v as i16 as i64)
                }
                TypeSuffix::Int => {
                    parse_checked(&fixed_digits, radix, i32::MIN as i128, i32::MAX as i128)
                        .map(|v| v as i32 as i64)
                }
                TypeSuffix::Long => {
                    parse_checked(&fixed_digits, radix, i64::MIN as i128, i64::MAX as i128)
                        .map(|v| v as i64)
                }
                _ => {
                    self.store(self.mark(), ERROR_EXPECTED_INTEGER_TYPE, true);
                    return None;
                }
            }
        } else {
            match ty {
                TypeSuffix::Byte => {
                    parse_checked(&fixed_digits, radix, 0, 255).map(|v| v as u8 as i8 as i64)
                }
                TypeSuffix::Short => {
                    parse_checked(&fixed_digits, radix, 0, 65_535).map(|v| v as u16 as i16 as i64)
                }
                TypeSuffix::Int => parse_checked(&fixed_digits, radix, 0, u32::MAX as i128)
                    .map(|v| v as u32 as i32 as i64),
                TypeSuffix::Long => parse_checked(&fixed_digits, radix, 0, u64::MAX as i128)
                    .map(|v| v as u64 as i64),
                _ => {
                    self.store(self.mark(), ERROR_EXPECTED_INTEGER_TYPE, true);
                    return None;
                }
            }
        };
        match result {
            Ok(v) => Some(v),
            Err(()) => {
                // Java: `catch (NumberFormatException e)` →
                // `createNumberParseError(e)`. The Java sub-message
                // (`For input string: "..."`) is approximated by the digits.
                self.store(
                    self.mark(),
                    format!("{ERROR_NUMBER_PARSE_FAILURE_PREFIX}{fixed_digits}"),
                    true,
                );
                None
            }
        }
    }

    /// `floatTypeSuffix` rule.
    fn parse_float_type_suffix(&mut self) -> Option<TypeSuffix> {
        let mark = self.mark();
        if self.try_chars(b'f' as u16, b'F' as u16) {
            return Some(TypeSuffix::Float);
        }
        self.restore(mark);
        if self.try_chars(b'd' as u16, b'D' as u16) {
            return Some(TypeSuffix::Double);
        }
        self.restore(mark);
        None
    }

    /// `floatExponentPart` rule — `e/E [sign] decimalNumeral`.
    fn parse_float_exponent_part(&mut self) -> Option<Signed<String>> {
        if !self.try_chars(b'e' as u16, b'E' as u16) {
            return None;
        }
        let sign_mark = self.mark();
        let sign = match self.parse_sign() {
            Some(s) => s,
            None => {
                self.restore(sign_mark);
                Sign::Plus
            }
        };
        let digits = self.parse_decimal_numeral()?;
        Some(Signed {
            sign,
            value: digits,
        })
    }

    /// `createFloat` — build `sign whole[.fraction][e exp]` (underscores
    /// stripped) and parse as f32/f64.
    fn create_float(
        &mut self,
        sign: Sign,
        whole: Option<&str>,
        fraction: Option<&str>,
        exponent: Option<&Signed<String>>,
        type_suffix: Option<TypeSuffix>,
    ) -> Option<Tag> {
        let mut result = String::new();
        sign.append(&mut result);
        if let Some(whole) = whole {
            clean_and_append(&mut result, whole);
        }
        if let Some(fraction) = fraction {
            result.push('.');
            clean_and_append(&mut result, fraction);
        }
        if let Some(exponent) = exponent {
            result.push('e');
            exponent.sign.append(&mut result);
            clean_and_append(&mut result, &exponent.value);
        }
        let contents = result;
        match type_suffix {
            Some(TypeSuffix::Float) => match java_parse_float(&contents) {
                Ok(v) => {
                    if !v.is_finite() {
                        self.store(self.mark(), ERROR_INFINITY_NOT_ALLOWED, true);
                        None
                    } else {
                        Some(self.ops.create_float(v))
                    }
                }
                Err(()) => {
                    self.store(
                        self.mark(),
                        format!("{ERROR_NUMBER_PARSE_FAILURE_PREFIX}{contents}"),
                        true,
                    );
                    None
                }
            },
            Some(TypeSuffix::Double) | None => match java_parse_double(&contents) {
                Ok(v) => {
                    if !v.is_finite() {
                        self.store(self.mark(), ERROR_INFINITY_NOT_ALLOWED, true);
                        None
                    } else {
                        Some(self.ops.create_double(v))
                    }
                }
                Err(()) => {
                    self.store(
                        self.mark(),
                        format!("{ERROR_NUMBER_PARSE_FAILURE_PREFIX}{contents}"),
                        true,
                    );
                    None
                }
            },
            Some(_) => {
                // `SnbtGrammar.createFloat` default branch.
                self.store(self.mark(), ERROR_EXPECTED_FLOAT_TYPE, true);
                None
            }
        }
    }

    /// `floatLiteral` rule — the four `[sign] ...` alternatives with the
    /// grammar's `cut`s (after `.` in branches a/b, after the exponent in c).
    fn parse_float_literal(&mut self) -> Option<Tag> {
        let sign_mark = self.mark();
        let sign = match self.parse_sign() {
            Some(s) => s,
            None => {
                self.restore(sign_mark);
                Sign::Plus
            }
        };
        let alt_mark = self.mark();
        let mut cut = false;
        let mut whole: Option<String> = None;
        let mut fraction: Option<String> = None;
        let mut exponent: Option<Signed<String>> = None;
        let mut type_suffix: Option<TypeSuffix> = None;
        let mut matched = false;

        // Branch a: whole '.' cut [fraction] [exponent] [suffix].
        if !matched {
            let m = self.mark();
            if let Some(w) = self.parse_decimal_numeral()
                && self.try_char(b'.' as u16)
            {
                cut = true;
                let fm = self.mark();
                fraction = match self.parse_decimal_numeral() {
                    Some(f) => Some(f),
                    None => {
                        self.restore(fm);
                        None
                    }
                };
                let em = self.mark();
                exponent = match self.parse_float_exponent_part() {
                    Some(e) => Some(e),
                    None => {
                        self.restore(em);
                        None
                    }
                };
                let sm = self.mark();
                type_suffix = match self.parse_float_type_suffix() {
                    Some(t) => Some(t),
                    None => {
                        self.restore(sm);
                        None
                    }
                };
                whole = Some(w);
                matched = true;
            }
            if !matched {
                self.restore(m);
            }
        }
        // Branch b: '.' cut fraction [exponent] [suffix].
        if !matched && !cut {
            let m = self.mark();
            if self.try_char(b'.' as u16) {
                cut = true;
                let fm = self.mark();
                fraction = match self.parse_decimal_numeral() {
                    Some(f) => Some(f),
                    None => {
                        self.restore(fm);
                        None
                    }
                };
                if fraction.is_some() {
                    let em = self.mark();
                    exponent = match self.parse_float_exponent_part() {
                        Some(e) => Some(e),
                        None => {
                            self.restore(em);
                            None
                        }
                    };
                    let sm = self.mark();
                    type_suffix = match self.parse_float_type_suffix() {
                        Some(t) => Some(t),
                        None => {
                            self.restore(sm);
                            None
                        }
                    };
                    matched = true;
                }
            }
            if !matched {
                self.restore(m);
            }
        }
        // Branch c: whole exponent cut [suffix].
        if !matched && !cut {
            let m = self.mark();
            if let Some(w) = self.parse_decimal_numeral()
                && let Some(e) = self.parse_float_exponent_part()
            {
                cut = true;
                let sm = self.mark();
                type_suffix = match self.parse_float_type_suffix() {
                    Some(t) => Some(t),
                    None => {
                        self.restore(sm);
                        None
                    }
                };
                whole = Some(w);
                exponent = Some(e);
                matched = true;
            }
            if !matched {
                self.restore(m);
            }
        }
        // Branch d: whole [exponent] suffix.
        if !matched && !cut {
            let m = self.mark();
            if let Some(w) = self.parse_decimal_numeral() {
                let em = self.mark();
                exponent = match self.parse_float_exponent_part() {
                    Some(e) => Some(e),
                    None => {
                        self.restore(em);
                        None
                    }
                };
                let sm = self.mark();
                type_suffix = match self.parse_float_type_suffix() {
                    Some(t) => Some(t),
                    None => {
                        self.restore(sm);
                        None
                    }
                };
                if type_suffix.is_some() {
                    whole = Some(w);
                    matched = true;
                }
            }
            if !matched {
                self.restore(m);
            }
        }

        if !matched {
            self.restore(alt_mark);
            return None;
        }
        self.create_float(
            sign,
            whole.as_deref(),
            fraction.as_deref(),
            exponent.as_ref(),
            type_suffix,
        )
    }

    /// `SimpleHexLiteralParseRule(size)` — exactly `size` hex digits.
    fn parse_simple_hex(&mut self, size: usize) -> Option<String> {
        let start = self.cursor;
        let mut pos = start;
        while pos < self.units.len() && pos - start < size && Self::is_hex_digit(self.units[pos]) {
            pos += 1;
        }
        if pos - start < size {
            self.store(
                self.mark(),
                format!("{ERROR_EXPECTED_HEX_ESCAPE_PREFIX}{size}"),
                true,
            );
            None
        } else {
            self.cursor = pos;
            Some(region_to_string(&self.units, start, pos))
        }
    }

    /// `stringEscapeSequence` action — convert `\xHH`/`\uHHHH`/`\UHHHHHHHH`
    /// hex escapes and `\N{name}`. `Character.codePointOf` (the Unicode name
    /// database) is STUB(mc.nbt.snbt): `\N{...}` always reports an invalid
    /// character name until the name table is ported.
    ///
    /// The escape is always 2/4/8 hex digits, so `codepoint` is at most
    /// 0xFFFFFFFF; the only invalid cases are out-of-codepoint (> 0x10FFFF,
    /// only reachable via `\U`) and lone surrogates. Java's `Character.isValid
    /// CodePoint` accepts surrogates and `Character.toString` yields a String
    /// holding the lone surrogate; Rust `char` cannot represent one, so a lone
    /// surrogate is an inherent (documented) divergence from Java — it stores
    /// the same invalid-codepoint error.
    fn apply_hex_escape(&mut self, hex: &str) -> Option<String> {
        let codepoint = u32::from_str_radix(hex, 16).ok()?;
        if codepoint > 0x10FFFF || (0xD800..=0xDFFF).contains(&codepoint) {
            // Java: `String.format(Locale.ROOT, "U+%08X", codepoint)`.
            self.store(
                self.mark(),
                format!("{ERROR_INVALID_CODEPOINT_PREFIX}U+{codepoint:08X}"),
                true,
            );
            None
        } else {
            char::from_u32(codepoint).map(|c| c.to_string())
        }
    }

    /// `stringEscapeSequence` rule.
    fn parse_string_escape_sequence(&mut self) -> Option<String> {
        let mark = self.mark();
        // Plain single-char escapes (marker values are the decoded char).
        const PLAIN: &[(u16, &str)] = &[
            (b'b' as u16, "\u{8}"),
            (b's' as u16, " "),
            (b't' as u16, "\t"),
            (b'n' as u16, "\n"),
            (b'f' as u16, "\u{c}"),
            (b'r' as u16, "\r"),
            (b'\\' as u16, "\\"),
            (b'\'' as u16, "'"),
            (b'"' as u16, "\""),
        ];
        for &(c, v) in PLAIN {
            if self.try_char(c) {
                return Some(v.to_string());
            }
            self.restore(mark);
        }
        // x/u/U hex escapes.
        for (lead, size) in [(b'x' as u16, 2usize), (b'u' as u16, 4), (b'U' as u16, 8)] {
            if self.try_char(lead) {
                if let Some(hex) = self.parse_simple_hex(size) {
                    return self.apply_hex_escape(&hex);
                }
                self.restore(mark);
            }
        }
        // \N{name} — STUB: Character.codePointOf is not ported.
        if self.try_char(b'N' as u16) {
            if self.try_char(b'{' as u16)
                && self.parse_unicode_name().is_some()
                && self.try_char(b'}' as u16)
            {
                self.store(self.mark(), ERROR_INVALID_CHARACTER_NAME, true);
                self.restore(mark);
                return None;
            }
            self.restore(mark);
        }
        None
    }

    /// `GreedyPatternParseRule(UNICODE_NAME, ...)` — `[-a-zA-Z0-9 ]+`.
    fn parse_unicode_name(&mut self) -> Option<String> {
        let start = self.cursor;
        let mut pos = start;
        while pos < self.units.len() && Self::is_unicode_name_char(self.units[pos]) {
            pos += 1;
        }
        if pos == start {
            self.store(self.mark(), ERROR_INVALID_CHARACTER_NAME, true);
            None
        } else {
            self.cursor = pos;
            Some(region_to_string(&self.units, start, pos))
        }
    }

    /// `PLAIN_STRING_CHUNK` — greedy run of non-`"`/`'`/`\` chars (min 1).
    fn parse_plain_string_chunk(&mut self) -> Option<String> {
        let start = self.cursor;
        let mut pos = start;
        while pos < self.units.len() && Self::is_plain_string_char(self.units[pos]) {
            pos += 1;
        }
        if pos - start < 1 {
            self.store(self.mark(), ERROR_INVALID_STRING_CONTENTS, true);
            None
        } else {
            self.cursor = pos;
            Some(region_to_string(&self.units, start, pos))
        }
    }

    /// `singleQuotedStringChunk` — plain | `\` escape | literal `"`.
    fn parse_single_quoted_chunk(&mut self) -> Option<String> {
        let m = self.mark();
        if let Some(plain) = self.parse_plain_string_chunk() {
            return Some(plain);
        }
        self.restore(m);
        if self.try_char(b'\\' as u16) {
            if let Some(e) = self.parse_string_escape_sequence() {
                return Some(e);
            }
            self.restore(m);
            return None;
        }
        self.restore(m);
        if self.try_char(b'"' as u16) {
            return Some("\"".to_string());
        }
        self.restore(m);
        None
    }

    /// `doubleQuotedStringChunk` — plain | `\` escape | literal `'`.
    fn parse_double_quoted_chunk(&mut self) -> Option<String> {
        let m = self.mark();
        if let Some(plain) = self.parse_plain_string_chunk() {
            return Some(plain);
        }
        self.restore(m);
        if self.try_char(b'\\' as u16) {
            if let Some(e) = self.parse_string_escape_sequence() {
                return Some(e);
            }
            self.restore(m);
            return None;
        }
        self.restore(m);
        if self.try_char(b'\'' as u16) {
            return Some("'".to_string());
        }
        self.restore(m);
        None
    }

    /// `Term.repeated(chunkRule, stringChunks)` + `joinList` — concatenates
    /// chunks (empty list → `""`).
    fn parse_quoted_contents(&mut self, chunk: fn(&mut Self) -> Option<String>) -> Option<String> {
        let mut chunks = Vec::new();
        loop {
            let m = self.mark();
            match chunk(self) {
                Some(c) => chunks.push(c),
                None => {
                    self.restore(m);
                    break;
                }
            }
        }
        Some(match chunks.len() {
            0 => String::new(),
            1 => chunks.remove(0),
            _ => chunks.concat(),
        })
    }

    /// `quotedStringLiteral` rule.
    fn parse_quoted_string_literal(&mut self) -> Option<String> {
        let mark = self.mark();
        // Branch 1: " contents " (cut after the opening quote — no fallback to
        // the single-quote branch once a double-quote is seen).
        if self.try_char(b'"' as u16) {
            let contents = self.parse_quoted_contents(Self::parse_double_quoted_chunk);
            if self.try_char(b'"' as u16) {
                return contents;
            }
            self.restore(mark);
            return None;
        }
        self.restore(mark);
        // Branch 2: ' contents '.
        if self.try_char(b'\'' as u16) {
            let contents = self.parse_quoted_contents(Self::parse_single_quoted_chunk);
            if self.try_char(b'\'' as u16) {
                return contents;
            }
            self.restore(mark);
            return None;
        }
        self.restore(mark);
        None
    }

    /// `UnquotedStringParseRule(1, ERROR_EXPECTED_UNQUOTED_STRING)`.
    fn parse_unquoted_string(&mut self) -> Option<String> {
        self.skip_whitespace();
        let cursor = self.mark();
        let start = self.cursor;
        let mut pos = start;
        while pos < self.units.len() && Self::is_allowed_in_unquoted_string(self.units[pos]) {
            pos += 1;
        }
        self.cursor = pos;
        let value = region_to_string(&self.units, start, pos);
        if value.is_empty() {
            self.store(cursor, ERROR_EXPECTED_UNQUOTED_STRING, true);
            None
        } else {
            Some(value)
        }
    }

    /// `unquotedStringOrBuiltIn` rule.
    fn parse_unquoted_string_or_builtin(&mut self) -> Option<Tag> {
        let contents = self.parse_unquoted_string()?;
        // Optional '(' argumentList ')'.
        let args_mark = self.mark();
        let arguments: Option<Vec<Tag>> = if self.try_char(b'(' as u16) {
            let args = self.parse_argument_list();
            if args.is_some() && self.try_char(b')' as u16) {
                args
            } else {
                self.restore(args_mark);
                None
            }
        } else {
            self.restore(args_mark);
            None
        };

        let first = contents.chars().next()?;
        // `isAllowedToStartUnquotedString` = `!canStartNumber`.
        if !Self::is_number_start(first as u16) {
            if let Some(args) = arguments {
                let key = BuiltinKey::new(contents.clone(), args.len());
                match find_builtin(&key) {
                    Some(op) => match run_builtin(op, &self.ops, &args) {
                        Ok(tag) => Some(tag),
                        Err(e) => {
                            self.store(self.mark(), e.message(), true);
                            None
                        }
                    },
                    None => {
                        self.store(
                            self.mark(),
                            format!("{ERROR_NO_SUCH_OPERATION_PREFIX}{key}"),
                            true,
                        );
                        None
                    }
                }
            } else if contents.eq_ignore_ascii_case(BUILTIN_TRUE) {
                Some(self.ops.create_boolean(true))
            } else if contents.eq_ignore_ascii_case(BUILTIN_FALSE) {
                Some(self.ops.create_boolean(false))
            } else {
                Some(self.ops.create_string(contents))
            }
        } else {
            self.store(self.mark(), ERROR_INVALID_UNQUOTED_START, true);
            None
        }
    }

    /// `mapKey` rule — quoted or unquoted string.
    fn parse_map_key(&mut self) -> Option<String> {
        let mark = self.mark();
        if let Some(q) = self.parse_quoted_string_literal() {
            return Some(q);
        }
        self.restore(mark);
        self.parse_unquoted_string()
    }

    /// `mapEntry` rule — `mapKey ':' literal` (empty-key check in the action).
    fn parse_map_entry(&mut self) -> Option<(String, Tag)> {
        let key = self.parse_map_key()?;
        let colon_mark = self.mark();
        if !self.try_char(b':' as u16) {
            self.restore(colon_mark);
            return None;
        }
        let value = self.parse_literal()?;
        if key.is_empty() {
            self.store(self.mark(), ERROR_EMPTY_KEY, true);
            return None;
        }
        Some((key, value))
    }

    /// `repeatedWithTrailingSeparator(mapEntryRule, ',', allowTrailing=true)`.
    fn parse_map_entries(&mut self) -> Option<Vec<(String, Tag)>> {
        let mut elements: Vec<(String, Tag)> = Vec::new();
        loop {
            let before_sep = self.mark();
            if !elements.is_empty() && !self.try_char(b',' as u16) {
                self.restore(before_sep);
                break;
            }
            let after_sep = self.mark();
            match self.parse_map_entry() {
                Some(e) => elements.push(e),
                None => {
                    self.restore(after_sep);
                    break;
                }
            }
        }
        Some(elements)
    }

    /// `mapLiteral` rule — `{ mapEntries }` with Paper depth tracking.
    fn parse_map_literal(&mut self) -> Option<Tag> {
        let mark = self.mark();
        if !self.try_char(b'{' as u16) {
            return None;
        }
        self.depth += 1;
        if self.depth > 512 {
            // `Scope.increaseDepth` — IllegalStateException("Too deep").
            self.store(mark, "Too deep", false);
            // Deviation (documented): Java leaks the incremented depth (513+)
            // for the rest of that parse state; here the counter is restored so
            // the parser recovers. Unobservable for valid inputs (the too-deep
            // input already fails).
            self.depth -= 1;
            return None;
        }
        let entries = self.parse_map_entries().unwrap_or_default();
        self.depth -= 1;
        if !self.try_char(b'}' as u16) {
            self.restore(mark);
            return None;
        }
        if entries.is_empty() {
            Some(self.ops.empty_map())
        } else {
            let pairs: Vec<Pair<Tag, Tag>> = entries
                .into_iter()
                .map(|(k, v)| Pair::of(self.ops.create_string(k), v))
                .collect();
            Some(self.ops.create_map(pairs))
        }
    }

    /// `arrayPrefix` rule.
    fn parse_array_prefix(&mut self) -> Option<ArrayPrefix> {
        let mark = self.mark();
        if self.try_char(b'B' as u16) {
            return Some(ArrayPrefix::Byte);
        }
        self.restore(mark);
        if self.try_char(b'L' as u16) {
            return Some(ArrayPrefix::Long);
        }
        self.restore(mark);
        if self.try_char(b'I' as u16) {
            return Some(ArrayPrefix::Int);
        }
        self.restore(mark);
        None
    }

    /// `intArrayEntries` — repeated `integerLiteral` separated by `,`.
    fn parse_int_array_entries(&mut self) -> Option<Vec<IntegerLiteral>> {
        let mut elements: Vec<IntegerLiteral> = Vec::new();
        loop {
            let before_sep = self.mark();
            if !elements.is_empty() && !self.try_char(b',' as u16) {
                self.restore(before_sep);
                break;
            }
            let after_sep = self.mark();
            match self.parse_integer_literal() {
                Some(e) => elements.push(e),
                None => {
                    self.restore(after_sep);
                    break;
                }
            }
        }
        Some(elements)
    }

    /// `listEntries` — repeated `literal` separated by `,`.
    fn parse_list_entries(&mut self) -> Option<Vec<Tag>> {
        let mut elements: Vec<Tag> = Vec::new();
        loop {
            let before_sep = self.mark();
            if !elements.is_empty() && !self.try_char(b',' as u16) {
                self.restore(before_sep);
                break;
            }
            let after_sep = self.mark();
            match self.parse_literal() {
                Some(e) => elements.push(e),
                None => {
                    self.restore(after_sep);
                    break;
                }
            }
        }
        Some(elements)
    }

    /// `argumentList` — repeated `literal` separated by `,`.
    fn parse_argument_list(&mut self) -> Option<Vec<Tag>> {
        let mut elements: Vec<Tag> = Vec::new();
        loop {
            let before_sep = self.mark();
            if !elements.is_empty() && !self.try_char(b',' as u16) {
                self.restore(before_sep);
                break;
            }
            let after_sep = self.mark();
            match self.parse_literal() {
                Some(e) => elements.push(e),
                None => {
                    self.restore(after_sep);
                    break;
                }
            }
        }
        Some(elements)
    }

    /// `ArrayPrefix.create(ops, entries, state)` — the typed array element
    /// conversion (values parsed at the entry's allowed type, then widened).
    fn build_array(&mut self, prefix: ArrayPrefix, entries: Vec<IntegerLiteral>) -> Option<Tag> {
        if entries.is_empty() {
            return Some(match prefix {
                ArrayPrefix::Byte => self.ops.create_byte_list(&[]),
                ArrayPrefix::Int => self.ops.create_int_list(vec![]),
                ArrayPrefix::Long => self.ops.create_long_list(vec![]),
            });
        }
        let mut values: Vec<i64> = Vec::with_capacity(entries.len());
        for entry in &entries {
            let ty = match prefix.compute_type(entry) {
                Some(t) => t,
                None => {
                    self.store(self.mark(), ERROR_INVALID_ARRAY_ELEMENT_TYPE, true);
                    return None;
                }
            };
            values.push(self.integer_literal_value(entry, ty)?);
        }
        Some(match prefix {
            ArrayPrefix::Byte => self
                .ops
                .create_byte_list(&values.iter().map(|v| *v as i8 as u8).collect::<Vec<_>>()),
            ArrayPrefix::Int => self
                .ops
                .create_int_list(values.iter().map(|v| *v as i32).collect()),
            ArrayPrefix::Long => self.ops.create_long_list(values),
        })
    }

    /// `listLiteral` rule — `[ arrayPrefix ';' intArrayEntries | listEntries ]`
    /// with Paper depth tracking.
    fn parse_list_literal(&mut self) -> Option<Tag> {
        let mark = self.mark();
        if !self.try_char(b'[' as u16) {
            return None;
        }
        self.depth += 1;
        if self.depth > 512 {
            self.store(mark, "Too deep", false);
            // Deviation (documented): Java leaks the incremented depth; here
            // the counter is restored so the parser recovers (see
            // `parse_map_literal`).
            self.depth -= 1;
            return None;
        }
        let alt_mark = self.mark();
        // Branch 1: arrayPrefix ';' intArrayEntries.
        let result = if let Some(prefix) = self.parse_array_prefix() {
            if self.try_char(b';' as u16) {
                let entries = self.parse_int_array_entries().unwrap_or_default();
                Some(self.build_array(prefix, entries))
            } else {
                self.restore(alt_mark);
                None
            }
        } else {
            self.restore(alt_mark);
            None
        };
        // Branch 2: listEntries.
        let result = match result {
            Some(Some(tag)) => Some(tag),
            Some(None) => None, // array element failed — no list fallback (Java action ran)
            None => {
                let entries = self.parse_list_entries().unwrap_or_default();
                if entries.is_empty() {
                    Some(self.ops.empty_list())
                } else {
                    Some(self.ops.create_list(entries))
                }
            }
        };
        self.depth -= 1;
        if !self.try_char(b']' as u16) {
            self.restore(mark);
            return None;
        }
        result
    }

    /// The `literal` top rule — the number/quote/map/list/unquoted dispatch.
    fn parse_literal(&mut self) -> Option<Tag> {
        let mark = self.mark();
        // Branch 1: number lookahead → floatLiteral | integerLiteral.
        if self.lookahead_after_whitespace(Self::is_number_start) {
            if let Some(f) = self.parse_float_literal() {
                return Some(f);
            }
            self.restore(mark);
            if let Some(i) = self.parse_integer_literal() {
                let ty = match i.suffix.ty {
                    None => TypeSuffix::Int,
                    Some(t) => t,
                };
                let value = self.integer_literal_value(&i, ty)?;
                return Some(match ty {
                    TypeSuffix::Byte => self.ops.create_byte(value as i8),
                    TypeSuffix::Short => self.ops.create_short(value as i16),
                    TypeSuffix::Int => self.ops.create_int(value as i32),
                    TypeSuffix::Long => self.ops.create_long(value),
                    _ => unreachable!("integer literal suffix type is always an integer type"),
                });
            }
            self.restore(mark);
        }
        self.restore(mark);
        // Branch 2: quote lookahead → quotedStringLiteral (committed).
        if self.lookahead_after_whitespace(|c| c == b'"' as u16 || c == b'\'' as u16) {
            if let Some(s) = self.parse_quoted_string_literal() {
                return Some(self.ops.create_string(s));
            }
            self.restore(mark);
            return None;
        }
        self.restore(mark);
        // Branch 3: '{' → mapLiteral (committed).
        if self.lookahead_after_whitespace(|c| c == b'{' as u16) {
            if let Some(m) = self.parse_map_literal() {
                return Some(m);
            }
            self.restore(mark);
            return None;
        }
        self.restore(mark);
        // Branch 4: '[' → listLiteral (committed).
        if self.lookahead_after_whitespace(|c| c == b'[' as u16) {
            if let Some(l) = self.parse_list_literal() {
                return Some(l);
            }
            self.restore(mark);
            return None;
        }
        self.restore(mark);
        // Branch 5: unquotedStringOrBuiltIn.
        self.parse_unquoted_string_or_builtin()
    }

    /// Format a `CommandSyntaxException.getMessage()`-style message.
    ///
    /// This duplicates `rivet-brigadier`'s `CommandSyntaxException::get_message`
    /// / `get_context` (both port `CommandSyntaxException.getMessage`, UTF-16
    /// `CONTEXT_AMOUNT = 10`). rivet-nbt does not depend on rivet-brigadier, so
    /// this is a second source of truth for the format; keep the two in sync if
    /// the context format changes.
    fn format_cmd(&self, cursor: usize, raw: &str) -> String {
        let input_len = self.units.len();
        let cursor = cursor.min(input_len);
        let mut context = String::new();
        if cursor > 10 {
            context.push_str("...");
        }
        context.push_str(&region_to_string(
            &self.units,
            cursor.saturating_sub(10),
            cursor,
        ));
        context.push_str("<--[HERE]");
        format!("{raw} at position {cursor}: {context}")
    }

    /// The best stored error as an `NbtFormatException`.
    fn failure(&self) -> NbtFormatException {
        match &self.best {
            Some(e) if e.cmd_syntax => {
                NbtFormatException::new(self.format_cmd(e.cursor, &e.message))
            }
            Some(e) => NbtFormatException::new(e.message.clone()),
            None => NbtFormatException::new("Failed to parse"),
        }
    }
}

// ---- number parsing helpers (Java `Integer`/`Long`/`UnsignedBytes` range
// semantics). ----

/// Parse `s` (optionally `+`/`-`-prefixed) in `radix` and require the value to
/// lie in `[min, max]` (inclusive). Mirrors Java `Integer.parseInt` /
/// `Long.parseLong` / `Integer.parseUnsignedInt` accepted-value sets.
fn parse_checked(s: &str, radix: u32, min: i128, max: i128) -> Result<i128, ()> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(());
    }
    let (negative, start) = match bytes[0] {
        b'+' => (false, 1usize),
        b'-' => (true, 1usize),
        _ => (false, 0usize),
    };
    if start >= bytes.len() {
        return Err(());
    }
    let mut acc: u128 = 0;
    for &b in &bytes[start..] {
        let digit = (b as char).to_digit(radix).ok_or(())? as u128;
        acc = acc
            .checked_mul(radix as u128)
            .and_then(|a| a.checked_add(digit))
            .ok_or(())?;
    }
    let value: i128 = if negative {
        0i128.wrapping_sub(acc as i128)
    } else {
        acc as i128
    };
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(())
    }
}

/// `Float.parseFloat` with Java overflow/underflow semantics (Rust's
/// `f32::from_str` rejects both, Java saturates to ±Infinity / ±0.0).
fn java_parse_float(s: &str) -> Result<f32, ()> {
    if let Ok(v) = s.parse::<f32>() {
        return Ok(v);
    }
    // Out of f32 range: parse as f64 (with the same saturating fallback Java
    // uses), then narrow — ±Inf / ±0.0 stay, finite f64 in f32 range maps
    // exactly.
    java_parse_double(s).map(|v| v as f32)
}

/// `Double.parseDouble` with Java overflow/underflow semantics.
fn java_parse_double(s: &str) -> Result<f64, ()> {
    if let Ok(v) = s.parse::<f64>() {
        return Ok(v);
    }
    parse_double_out_of_range(s)
}

/// Fallback for values Rust's `f64::from_str` rejects (out of range): split
/// the mantissa/exponent and saturate to ±Infinity / ±0.0.
fn parse_double_out_of_range(s: &str) -> Result<f64, ()> {
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let mantissa: f64 = mantissa.parse().map_err(|_| ())?;
    if exp.is_empty() {
        return Err(());
    }
    let exp: i128 = exp.parse().map_err(|_| ())?;
    let scale = if exp > i32::MAX as i128 {
        f64::INFINITY
    } else if exp < i32::MIN as i128 {
        0.0
    } else {
        10f64.powi(exp as i32)
    };
    Ok(mantissa * scale)
}

/// `SnbtGrammar.cleanAndAppend` — appends `contents` with `_` removed when
/// present.
fn clean_and_append(output: &mut String, contents: &str) {
    if contents.contains('_') {
        for c in contents.chars() {
            if c != '_' {
                output.push(c);
            }
        }
    } else {
        output.push_str(contents);
    }
}

// ---- `TagParser` ----

/// `TagParser<T>` specialised to `NbtOps` (the crate's only parser,
/// `NBT_OPS_PARSER = create(NbtOps.INSTANCE)`). Java keeps the ops generic;
/// `getOps()` is preserved.
#[derive(Debug, Clone, Copy)]
pub struct TagParser {
    ops: NbtOps,
}

impl TagParser {
    /// `TagParser.create(DynamicOps)`.
    pub fn create(ops: NbtOps) -> TagParser {
        TagParser { ops }
    }

    /// `TagParser.getOps()`.
    pub fn get_ops(&self) -> NbtOps {
        self.ops
    }

    /// `TagParser.parseFully(String)` — parse a literal, skip trailing
    /// whitespace, reject any remaining data.
    pub fn parse_fully(&self, input: &str) -> Result<Tag, NbtFormatException> {
        let mut parser = SnbtParser::new(input, self.ops);
        match parser.parse_literal() {
            Some(tag) => {
                parser.skip_whitespace();
                if parser.can_read() {
                    Err(NbtFormatException::new(
                        parser.format_cmd(parser.mark(), ERROR_TRAILING_DATA),
                    ))
                } else {
                    Ok(tag)
                }
            }
            None => Err(parser.failure()),
        }
    }

    /// `TagParser.parseAsArgument(StringReader)` — parse a literal, leaving
    /// any trailing input unconsumed.
    pub fn parse_as_argument(&self, input: &str) -> Result<Tag, NbtFormatException> {
        let mut parser = SnbtParser::new(input, self.ops);
        match parser.parse_literal() {
            Some(tag) => Ok(tag),
            None => Err(parser.failure()),
        }
    }
}

/// `TagParser.parseCompoundFully(String)` — parse SNBT and require a compound
/// result.
pub fn parse_compound_fully(input: &str) -> Result<CompoundTag, NbtFormatException> {
    let parser = TagParser::create(NbtOps::instance());
    let mut inner = SnbtParser::new(input, parser.get_ops());
    let result = match inner.parse_literal() {
        Some(tag) => {
            inner.skip_whitespace();
            if inner.can_read() {
                return Err(NbtFormatException::new(
                    inner.format_cmd(inner.mark(), ERROR_TRAILING_DATA),
                ));
            }
            tag
        }
        None => return Err(inner.failure()),
    };
    cast_to_compound_or_throw(&inner, &result)
}

/// `TagParser.parseCompoundAsArgument(StringReader)`.
pub fn parse_compound_as_argument(input: &str) -> Result<CompoundTag, NbtFormatException> {
    let parser = TagParser::create(NbtOps::instance());
    let mut inner = SnbtParser::new(input, parser.get_ops());
    let result = match inner.parse_literal() {
        Some(tag) => tag,
        None => return Err(inner.failure()),
    };
    cast_to_compound_or_throw(&inner, &result)
}

/// `TagParser.castToCompoundOrThrow(StringReader, Tag)` — on a non-compound
/// result, `ERROR_EXPECTED_COMPOUND.createWithContext(reader)` at the reader's
/// cursor. After `parseFully` the reader sits at end of input; after
/// `parseAsArgument` it sits at end of the parsed literal (which may be before
/// the end of the string). `SnbtParser.cursor` is exactly that position.
fn cast_to_compound_or_throw(
    parser: &SnbtParser,
    result: &Tag,
) -> Result<CompoundTag, NbtFormatException> {
    match result {
        Tag::Compound(c) => Ok(c.clone()),
        _other => Err(NbtFormatException::new(
            parser.format_cmd(parser.mark(), ERROR_EXPECTED_COMPOUND),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byte_array_tag::ByteArrayTag;
    use crate::byte_tag::ByteTag;
    use crate::compound_tag::CompoundTag;
    use crate::double_tag::DoubleTag;
    use crate::float_tag::FloatTag;
    use crate::int_array_tag::IntArrayTag;
    use crate::int_tag::IntTag;
    use crate::list_tag::ListTag;
    use crate::long_array_tag::LongArrayTag;
    use crate::long_tag::LongTag;
    use crate::short_tag::ShortTag;
    use crate::string_tag::StringTag;
    use crate::tag::Tag;

    fn parse(s: &str) -> Tag {
        TagParser::create(NbtOps::instance())
            .parse_fully(s)
            .expect(s)
    }

    fn parse_err(s: &str) -> String {
        match TagParser::create(NbtOps::instance()).parse_fully(s) {
            Ok(t) => panic!("expected error for {s:?}, got {t:?}"),
            Err(e) => e.message,
        }
    }

    fn compound(entries: &[(&str, Tag)]) -> CompoundTag {
        let mut c = CompoundTag::new();
        for (k, v) in entries {
            c.put(k.to_string(), v.clone());
        }
        c
    }

    #[test]
    fn parse_numeric_primitives() {
        assert_eq!(parse("5b"), Tag::Byte(ByteTag::new(5)));
        assert_eq!(parse("-128b"), Tag::Byte(ByteTag::new(-128)));
        assert_eq!(parse("3s"), Tag::Short(ShortTag::new(3)));
        assert_eq!(parse("1234"), Tag::Int(IntTag::new(1234)));
        assert_eq!(parse("99L"), Tag::Long(LongTag::new(99)));
        assert_eq!(parse("1.5f"), Tag::Float(FloatTag::new(1.5)));
        assert_eq!(parse("2.25d"), Tag::Double(DoubleTag::new(2.25)));
        assert_eq!(parse("2.25"), Tag::Double(DoubleTag::new(2.25)));
    }

    #[test]
    fn parse_negative_and_signed_suffixes() {
        assert_eq!(parse("-3"), Tag::Int(IntTag::new(-3)));
        assert_eq!(parse("-3L"), Tag::Long(LongTag::new(-3)));
        assert_eq!(parse("-3s"), Tag::Short(ShortTag::new(-3)));
        assert_eq!(parse("-3b"), Tag::Byte(ByteTag::new(-3)));
        // u signed prefixes.
        assert_eq!(parse("1ub"), Tag::Byte(ByteTag::new(1)));
        assert_eq!(parse("1us"), Tag::Short(ShortTag::new(1)));
        assert_eq!(parse("1ui"), Tag::Int(IntTag::new(1)));
        assert_eq!(parse("1ul"), Tag::Long(LongTag::new(1)));
        // s signed prefixes.
        assert_eq!(parse("1sb"), Tag::Byte(ByteTag::new(1)));
        assert_eq!(parse("1ss"), Tag::Short(ShortTag::new(1)));
        assert_eq!(parse("1si"), Tag::Int(IntTag::new(1)));
        assert_eq!(parse("1sl"), Tag::Long(LongTag::new(1)));
    }

    #[test]
    fn parse_hex_and_binary() {
        assert_eq!(parse("0x10"), Tag::Int(IntTag::new(16)));
        assert_eq!(parse("0X10"), Tag::Int(IntTag::new(16)));
        assert_eq!(parse("0x1F"), Tag::Int(IntTag::new(31)));
        assert_eq!(parse("0b101"), Tag::Int(IntTag::new(5)));
        assert_eq!(parse("0B101"), Tag::Int(IntTag::new(5)));
        assert_eq!(parse("0x1_0"), Tag::Int(IntTag::new(16)));
        assert_eq!(parse("0b1_0_1"), Tag::Int(IntTag::new(5)));
        // '0' alone is decimal zero.
        assert_eq!(parse("0"), Tag::Int(IntTag::new(0)));
        // Hex unsigned range: 0xFFFFFFFF → -1 as int (wraps).
        assert_eq!(parse("0xFFFFFFFF"), Tag::Int(IntTag::new(-1)));
        // Without a suffix the default type is INT, so 0xFFFFFFFFFFFFFFFF
        // exceeds the unsigned-int range. Java (verified): the optional
        // `integerSuffix` stores "Expected literal u|U" at cursor 18 (right
        // after the 16 hex digits) before `IntegerLiteral.create` runs, and
        // Java throws the first CSE at the farthest cursor — so "Expected
        // literal u|U" surfaces, not "Failed to parse number".
        assert!(
            parse_err("0xFFFFFFFFFFFFFFFF").contains("Expected literal u|U"),
            "err = {}",
            parse_err("0xFFFFFFFFFFFFFFFF")
        );
        // `0xFFFFFFFFFFFFFFFFL` → long -1 (unsigned long wraps).
        assert_eq!(parse("0xFFFFFFFFFFFFFFFFL"), Tag::Long(LongTag::new(-1)));
        // Hex is unsigned; `0xFF` → Int 255. Note `b` is itself a hex digit, so
        // `0xFFb` = 0xFFB = 4091 (Int), not a byte suffix.
        assert_eq!(parse("0xFF"), Tag::Int(IntTag::new(255)));
        assert_eq!(parse("0xFFb"), Tag::Int(IntTag::new(0xFFB)));
        // Unsigned-byte suffix wraps: `0xFFub` → byte 255 → -1.
        assert_eq!(parse("0xFFub"), Tag::Byte(ByteTag::new(-1)));
    }

    #[test]
    fn parse_leading_zero_is_error() {
        // Java: `0123` fails, but the surfaced error is the floatLiteral
        // branch's `character('.')` failure — "Expected literal ." — NOT
        // "Decimal numbers can't start with 0" (the leading-zero error is
        // stored at the same cursor but later, and Java throws the first CSE
        // at the farthest cursor). Verified against the live Java server.
        let err = parse_err("0123");
        assert!(err.contains("Expected literal ."), "err = {err}");
        assert!(err.contains("at position"), "err = {err}");
    }

    #[test]
    fn parse_underscore_rules() {
        // A leading underscore does not start a number (`canStartNumber('_')`
        // is false), so `_1` parses as the unquoted string "_1" (Java-verified).
        assert_eq!(
            parse("_1"),
            Tag::String(StringTag::value_of("_1".to_string()))
        );
        // `1_`: the number path reads the numeral but rejects the trailing
        // underscore, then the unquoted fallback rejects a digit-starting token
        // — Java surfaces "Expected literal (" (the optional `(` in
        // `unquotedStringOrBuiltIn` is tried first). Verified against the live
        // Java server.
        let err = parse_err("1_");
        assert!(err.contains("Expected literal ("), "err = {err}");
        // Interior underscores accepted.
        assert_eq!(parse("1_000"), Tag::Int(IntTag::new(1000)));
        // In a float.
        assert_eq!(parse("1_0.5"), Tag::Double(DoubleTag::new(10.5)));
    }

    #[test]
    fn parse_float_forms() {
        assert_eq!(parse("1.0f"), Tag::Float(FloatTag::new(1.0)));
        assert_eq!(parse(".5"), Tag::Double(DoubleTag::new(0.5)));
        assert_eq!(parse("1."), Tag::Double(DoubleTag::new(1.0)));
        assert_eq!(parse("1e3"), Tag::Double(DoubleTag::new(1000.0)));
        assert_eq!(parse("1E3"), Tag::Double(DoubleTag::new(1000.0)));
        assert_eq!(parse("1e-3"), Tag::Double(DoubleTag::new(0.001)));
        assert_eq!(parse("1e+3f"), Tag::Float(FloatTag::new(1000.0)));
        assert_eq!(parse("-1.5"), Tag::Double(DoubleTag::new(-1.5)));
        // 1d suffix → double.
        assert_eq!(parse("1d"), Tag::Double(DoubleTag::new(1.0)));
        assert_eq!(parse("1f"), Tag::Float(FloatTag::new(1.0)));
    }

    #[test]
    fn parse_float_overflow_and_infinity_rejected() {
        // Java (verified against the live server): the `floatLiteral` action
        // stores the infinity error and returns null, so the literal
        // alternative falls back to `integerLiteral`, which parses `1`, and
        // `parseFully` then fails on the trailing `e400` — the stored infinity
        // error is discarded when the parse succeeds, so the surfaced error is
        // "Unexpected trailing data". For `.5e400` the integer fallback cannot
        // parse a leading `.`, so the farthest errors are at cursor 6 and Java
        // throws the *first* CSE there — the optional `floatTypeSuffix`'s
        // "Expected literal f|F" (stored before the infinity error).
        let err = parse_err("1e400");
        assert!(err.contains("Unexpected trailing data"), "err = {err}");
        let err = parse_err("1e400f");
        assert!(err.contains("Unexpected trailing data"), "err = {err}");
        let err = parse_err("1.5e400f");
        assert!(err.contains("Unexpected trailing data"), "err = {err}");
        let err = parse_err(".5e400");
        assert!(err.contains("Expected literal f|F"), "err = {err}");
    }

    #[test]
    fn parse_strings_quoted_and_escaped() {
        assert_eq!(
            parse("\"hi\""),
            Tag::String(StringTag::value_of("hi".to_string()))
        );
        assert_eq!(
            parse("'hi'"),
            Tag::String(StringTag::value_of("hi".to_string()))
        );
        assert_eq!(
            parse("\"a\\nb\""),
            Tag::String(StringTag::value_of("a\nb".to_string()))
        );
        assert_eq!(
            parse("\"a\\tb\""),
            Tag::String(StringTag::value_of("a\tb".to_string()))
        );
        assert_eq!(
            parse("\"a\\\\b\""),
            Tag::String(StringTag::value_of("a\\b".to_string()))
        );
        assert_eq!(
            parse("\"a\\\"b\""),
            Tag::String(StringTag::value_of("a\"b".to_string()))
        );
        assert_eq!(
            parse("\"a\\'b\""),
            Tag::String(StringTag::value_of("a'b".to_string()))
        );
        // A double-quoted string may contain literal single quotes and vice-versa.
        assert_eq!(
            parse("\"a'b\""),
            Tag::String(StringTag::value_of("a'b".to_string()))
        );
        assert_eq!(
            parse("'a\"b'"),
            Tag::String(StringTag::value_of("a\"b".to_string()))
        );
        // Hex escapes.
        assert_eq!(
            parse("\"\\x41\""),
            Tag::String(StringTag::value_of("A".to_string()))
        );
        assert_eq!(
            parse("\"\\u0041\""),
            Tag::String(StringTag::value_of("A".to_string()))
        );
        assert_eq!(
            parse("\"\\U00000041\""),
            Tag::String(StringTag::value_of("A".to_string()))
        );
        // Invalid codepoints: a lone surrogate is rejected with the Java
        // "Invalid Unicode character value: U+0000D800" message (a documented
        // divergence — Java would store the lone surrogate, Rust char cannot).
        let err = parse_err("\"\\uD800\"");
        assert!(
            err.contains("Invalid Unicode character value: U+0000D800"),
            "err = {err}"
        );
        // An out-of-range codepoint is rejected too.
        let err = parse_err("\"\\U00110000\"");
        assert!(
            err.contains("Invalid Unicode character value: U+00110000"),
            "err = {err}"
        );
    }

    #[test]
    fn parse_strings_unquoted() {
        assert_eq!(
            parse("hello"),
            Tag::String(StringTag::value_of("hello".to_string()))
        );
        assert_eq!(
            parse("hello_world"),
            Tag::String(StringTag::value_of("hello_world".to_string()))
        );
        assert_eq!(
            parse("a.b"),
            Tag::String(StringTag::value_of("a.b".to_string()))
        );
    }

    #[test]
    fn parse_unquoted_starting_with_number_is_error() {
        // Java (verified against the live server): for "+abc" the number
        // lookahead matches but no float/int parses; the unquoted fallback
        // rejects a number-starting token, but the optional `(` after the
        // unquoted token stores "Expected literal (" first at the same cursor,
        // and Java throws the first CSE at the farthest cursor. So the surfaced
        // error is "Expected literal (" — NOT the invalid-unquoted-start text.
        let err = parse_err("+abc");
        assert!(err.contains("Expected literal ("), "err = {err}");
        let err = parse_err("-abc");
        assert!(err.contains("Expected literal ("), "err = {err}");
    }

    #[test]
    fn parse_digit_prefixed_token_is_trailing_data() {
        // "5abc": the number path parses Int 5, then "abc" is trailing data
        // (NOT the unquoted-start error — the number path committed first).
        let err = parse_err("5abc");
        assert!(err.contains("Unexpected trailing data"), "err = {err}");
        assert_eq!(parse("5"), Tag::Int(IntTag::new(5)));
    }

    #[test]
    fn parse_true_false() {
        assert_eq!(parse("true"), Tag::Byte(ByteTag::value_of_bool(true)));
        assert_eq!(parse("false"), Tag::Byte(ByteTag::value_of_bool(false)));
        assert_eq!(parse("TRUE"), Tag::Byte(ByteTag::value_of_bool(true)));
        // Not true/false → plain string.
        assert_eq!(
            parse("truex"),
            Tag::String(StringTag::value_of("truex".to_string()))
        );
    }

    #[test]
    fn parse_builtin_bool_and_uuid() {
        assert_eq!(parse("bool(1)"), Tag::Byte(ByteTag::value_of_bool(true)));
        assert_eq!(parse("bool(0)"), Tag::Byte(ByteTag::value_of_bool(false)));
        assert_eq!(parse("bool(true)"), Tag::Byte(ByteTag::value_of_bool(true)));
        assert_eq!(
            parse("bool(false)"),
            Tag::Byte(ByteTag::value_of_bool(false))
        );
        // uuid(...) → int array (argument must be a string).
        assert_eq!(
            parse("uuid(\"01020304-0506-0708-090a-0b0c0d0e0f10\")"),
            Tag::IntArray(IntArrayTag::new(vec![
                0x01020304, 0x05060708, 0x090a0b0c, 0x0d0e0f10
            ]))
        );
        // bool on a non-number → error.
        assert!(
            parse_err("bool(\"x\")").contains("Expected a number or a boolean"),
            "err = {}",
            parse_err("bool(\"x\")")
        );
        // Unknown builtin → no such operation.
        assert!(
            parse_err("foo(1)").contains("No such operation: foo/1"),
            "err = {}",
            parse_err("foo(1)")
        );
        // Builtin with wrong arity → no such operation.
        assert!(
            parse_err("bool(1,2)").contains("No such operation: bool/2"),
            "err = {}",
            parse_err("bool(1,2)")
        );
    }

    #[test]
    fn parse_trailing_comma() {
        // `repeatedWithTrailingSeparator(..., allowTrailing=true)` for
        // mapEntries/listEntries: a trailing comma is consumed but the failed
        // following element restores to just after the comma, then the closing
        // brace/bracket is consumed. So "{a:1,}" == "{a:1}" (Java-verified).
        assert_eq!(parse("{a:1,}"), parse("{a:1}"));
        assert_eq!(parse("{a:1, }"), parse("{a:1}"));
        // Lists: "[1,]" == "[1]".
        assert_eq!(parse("[1,]"), parse("[1]"));
        // A double comma: the second comma is consumed as a separator, then the
        // empty-string key fails and the quoted-string branch's `character('"')`
        // stores "Expected literal \"" at the farthest cursor (Java-verified) —
        // NOT "Unexpected trailing data".
        let err = parse_err("{a:1,,}");
        assert!(err.contains("Expected literal \""), "err = {err}");
    }

    #[test]
    fn parse_compound() {
        let c = parse("{a:1,b:two}");
        assert_eq!(
            c,
            Tag::Compound(compound(&[
                ("a", Tag::Int(IntTag::new(1))),
                ("b", Tag::String(StringTag::value_of("two".to_string()))),
            ]))
        );
        // Empty compound.
        assert_eq!(parse("{}"), Tag::Compound(CompoundTag::new()));
        // Nested.
        let c = parse("{outer:{inner:1}}");
        assert_eq!(
            c,
            Tag::Compound(compound(&[(
                "outer",
                Tag::Compound(compound(&[("inner", Tag::Int(IntTag::new(1)))])),
            )]))
        );
    }

    #[test]
    fn parse_list() {
        assert_eq!(parse("[]"), Tag::List(ListTag::new()));
        let mut l = ListTag::new();
        l.add(Tag::Int(IntTag::new(1)));
        l.add(Tag::Int(IntTag::new(2)));
        assert_eq!(parse("[1,2]"), Tag::List(l));
        // Mixed list.
        let mut l2 = ListTag::new();
        l2.add(Tag::Int(IntTag::new(1)));
        l2.add(Tag::String(StringTag::value_of("a".to_string())));
        assert_eq!(parse("[1,\"a\"]"), Tag::List(l2));
    }

    #[test]
    fn parse_typed_arrays() {
        assert_eq!(parse("[B;]"), Tag::ByteArray(ByteArrayTag::new(vec![])));
        assert_eq!(
            parse("[B;1B,-1B,2B]"),
            Tag::ByteArray(ByteArrayTag::new(vec![1, -1, 2]))
        );
        assert_eq!(
            parse("[I;1,2]"),
            Tag::IntArray(IntArrayTag::new(vec![1, 2]))
        );
        assert_eq!(
            parse("[L;1L,2L]"),
            Tag::LongArray(LongArrayTag::new(vec![1, 2]))
        );
        // Type widening within allowed element types.
        assert_eq!(
            parse("[I;1b,2s]"),
            Tag::IntArray(IntArrayTag::new(vec![1, 2]))
        );
        // Invalid element type (`L` is not allowed in a byte array). Java
        // (verified): the `intArrayEntries` element fails after `1`, the
        // trailing-comma restore leaves the cursor at the `L` position, and the
        // stored "Invalid array element type" error (at an earlier cursor) is
        // discarded; the farthest error surfaces as "Expected literal ,".
        assert!(
            parse_err("[B;1L]").contains("Expected literal ,"),
            "err = {}",
            parse_err("[B;1L]")
        );
    }

    #[test]
    fn parse_whitespace_is_skipped() {
        assert_eq!(parse("  { a : 1 }  "), parse("{a:1}"));
        assert_eq!(parse("[ 1 , 2 ]"), parse("[1,2]"));
    }

    #[test]
    fn parse_trailing_data_is_error() {
        let err = parse_err("1 2");
        assert!(err.contains("Unexpected trailing data"), "err = {err}");
        // The context marks the trailing position.
        assert!(err.contains("at position"), "err = {err}");
        assert!(err.contains("<--[HERE]"), "err = {err}");
    }

    #[test]
    fn parse_unclosed_structures_fail() {
        // Java (verified against the live server): the farthest-cursor error is
        // thrown first. For each unclosed structure, the floatLiteral branch's
        // `character('.')` stores "Expected literal ." at the farthest cursor
        // before the closing-brace/bracket check, and the unterminated quoted
        // string stores "Invalid string contents" (the plain chunk fails at the
        // end before the closing quote check).
        assert!(parse_err("{a:1").contains("Expected literal ."));
        assert!(parse_err("[1,2").contains("Expected literal ."));
        assert!(parse_err("\"abc").contains("Invalid string contents"));
    }

    #[test]
    fn parse_round_trip_via_string_visitor() {
        // parse(print(tag)) == tag for a representative compound.
        let c = compound(&[
            (
                "name",
                Tag::String(StringTag::value_of("Rivet".to_string())),
            ),
            ("x", Tag::Int(IntTag::new(42))),
            (
                "list",
                Tag::List({
                    let mut l = ListTag::new();
                    l.add(Tag::Float(FloatTag::new(1.5)));
                    l
                }),
            ),
        ]);
        let snbt =
            crate::string_tag_visitor::StringTagVisitor::to_string(&Tag::Compound(c.clone()));
        assert_eq!(parse(&snbt), Tag::Compound(c));
    }

    #[test]
    fn parse_compound_fully_requires_compound() {
        let ok = parse_compound_fully("{a:1}").unwrap();
        assert_eq!(ok, compound(&[("a", Tag::Int(IntTag::new(1)))]));
        // Non-compound → error.
        assert!(parse_compound_fully("1").is_err());
        assert!(parse_compound_fully("hello").is_err());
    }

    #[test]
    fn parse_compound_as_argument_errors_at_end_of_literal() {
        // Java `parseCompoundAsArgument` leaves the reader at the end of the
        // parsed literal, so "1 2" reports ERROR_EXPECTED_COMPOUND at position
        // 1 (not end of input at 3). The context slice is therefore "1".
        let err = parse_compound_as_argument("1 2").unwrap_err();
        assert!(
            err.message.contains("at position 1"),
            "err = {}",
            err.message
        );
        assert!(err.message.contains("1<--[HERE]"), "err = {}", err.message);
        // Leading whitespace is skipped, so the literal starts at position 1.
        let err = parse_compound_as_argument("  1 2").unwrap_err();
        assert!(
            err.message.contains("at position 3"),
            "err = {}",
            err.message
        );
    }

    #[test]
    fn error_messages_carry_position_and_context() {
        // SimpleCommandExceptionType.createWithContext: the message is
        // "<reason> at position <cursor>: <...><--[HERE]".
        // "nope!" parses `nope` then leaves `!` → trailing data at position 4.
        let err = parse_err("nope!");
        assert!(err.contains("Unexpected trailing data"), "err = {err}");
        assert!(err.contains("at position"), "err = {err}");
        assert!(err.contains("nope<--[HERE]"), "err = {err}");
        // CONTEXT_AMOUNT = 10 chars: cursor 15 > 10 → "..." prefix in the
        // context slice (mid-message).
        let err = parse_err("{averylongkey:1");
        assert!(err.contains("..."), "err = {err}");
        assert!(err.contains("<--[HERE]"), "err = {err}");
    }

    #[test]
    fn parse_empty_string_fails() {
        let err = parse_err("");
        assert!(
            err.contains("Expected a valid unquoted string"),
            "err = {err}"
        );
    }
}

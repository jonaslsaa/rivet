//! `java.util.UUID` — the value type, owned here in `rivet-util` (a JDK type,
//! like `java_hash`/`data_io`; there is no java.util crate). Ported for
//! `NameAndId` (`UUIDUtil.STRING_CODEC`) and the profile/authlib slice; the
//! wire codecs live in `rivet-protocol` on top of this type.
//!
//! Only the surface Minecraft actually uses is ported: the `most`/`least` bit
//! layout (`UUID(long, long)`), `UUID.fromString`, `UUID.toString`, and value
//! equality/hash. The rest of `java.util.UUID` (name-based/random factories,
//! version/variant accessors, serialization) is not ported.
//! RivetTodo(#206): the rest of the `java.util.UUID` surface is deferred.
//!
//! `Hash` is derived so `GameProfile` can derive `Hash` over its record
//! components. Java's `UUID.hashCode()` is `(int)(msb ^ (msb >>> 32)) ^
//! (int)(lsb ^ (lsb >>> 32))`; deriving over the two `i64` fields satisfies the
//! `a == b ⇒ hash(a) == hash(b)` contract with a different spread (hash values
//! are never wire- or equality-visible — only the consistency contract is).

/// `java.util.UUID` — a 128-bit value as the two signed `long` halves Java
/// exposes (`getMostSignificantBits()` / `getLeastSignificantBits()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Uuid {
    /// `mostSignificantBits`.
    pub most: i64,
    /// `leastSignificantBits`.
    pub least: i64,
}

impl Uuid {
    /// `UUID.fromString(String)` — Java's parser, exactly (JDK 25). Accepts
    /// any string of at most 36 UTF-16 code units with exactly 4 dashes whose
    /// groups parse as signed base-16 longs, masking each group to its
    /// 32/16/16/16/48-bit width (short groups pad with zeros, so `1-2-3-4-5`
    /// is valid; a group wider than its field truncates, so
    /// `100000000-2-3-4-5` is valid). Hex digits are `Character.digit(char,
    /// 16)` — ASCII `0-9A-Fa-f`, the BMP decimal digits (Arabic-Indic,
    /// Devanagari, fullwidth, &c.), and fullwidth `Ａ-Ｆ`/`ａ-ｆ`.
    /// Braces, the `urn:uuid:` prefix, the undashed 32-char form, and a group
    /// whose value exceeds `Long.MAX_VALUE` (e.g. `8000000000000000`) are
    /// REJECTED — `UUID.fromString` never accepted them (verified against the
    /// JDK 25 JVM Paper's oracle runs on). No value validation: Java's
    /// `UUID(long, long)` constructor accepts any bit pattern (the all-zero
    /// UUID parses fine).
    ///
    /// `Err` carries Java's exact exception message (`e.getMessage()` from the
    /// `IllegalArgumentException`/`NumberFormatException` `UUID.fromString`
    /// throws), so `UUIDUtil.STRING_CODEC` can reproduce its
    /// `"Invalid UUID <s>: <cause>"` error verbatim.
    pub fn from_string(name: &str) -> Result<Uuid, String> {
        // Java's `UUID.fromString` sees a `String` as UTF-16 code units
        // (`name.length()`, `charAt(i)`, `Character.digit(char, 16)`), so the
        // parser below operates on code units, not bytes — a fullwidth digit
        // is 3 UTF-8 bytes but 1 code unit, and must count as 1.
        let units: Vec<u16> = name.encode_utf16().collect();
        let len = units.len();
        // Java's `UUID.fromString1` rejects strings longer than 36 code units
        // outright ("UUID string too large").
        if len > 36 {
            return Err("UUID string too large".to_string());
        }
        // Locate the dashes exactly as repeated `indexOf('-', ...)` would; a
        // 5th dash (or fewer than 4) is "Invalid UUID string".
        let dash_after = |start: usize| {
            units[start..]
                .iter()
                .position(|&u| u == u16::from(b'-'))
                .map(|i| start + i)
        };
        let d1 = dash_after(0);
        let d2 = d1.and_then(|d| dash_after(d + 1));
        let d3 = d2.and_then(|d| dash_after(d + 1));
        let d4 = d3.and_then(|d| dash_after(d + 1));
        let d5 = d4.and_then(|d| dash_after(d + 1));
        let (d1, d2, d3, d4) = match (d1, d2, d3, d4, d5) {
            (Some(a), Some(b), Some(c), Some(d), None) => (a, b, c, d),
            _ => return Err(format!("Invalid UUID string: {name}")),
        };
        // `Long.parseLong(segment, 16)` masked to the field width. Each group
        // parses as a SIGNED long (values beyond `Long.MAX_VALUE` overflow),
        // then masks — exactly Java's `parseLong(...) & 0x...`.
        let g1 = (parse_long_hex(&units[..d1])? as u64) & 0xffff_ffff;
        let g2 = (parse_long_hex(&units[d1 + 1..d2])? as u64) & 0xffff;
        let g3 = (parse_long_hex(&units[d2 + 1..d3])? as u64) & 0xffff;
        let g4 = (parse_long_hex(&units[d3 + 1..d4])? as u64) & 0xffff;
        let g5 = (parse_long_hex(&units[d4 + 1..len])? as u64) & 0xffff_ffff_ffff;
        Ok(Uuid {
            most: ((g1 << 32) | (g2 << 16) | g3) as i64,
            least: ((g4 << 48) | g5) as i64,
        })
    }
}

/// `Long.parseLong(String, 16)` — Java's signed-base-16 parser, with the exact
/// JDK-25 accept/reject set and `NumberFormatException` messages. Parses a
/// segment (a slice of UTF-16 code units) into a signed `i64` (a value outside
/// `[i64::MIN, i64::MAX]` — e.g. a 16-hex-digit group starting `8..f` — is
/// rejected), accumulating negatively the way Java does so `Long.MIN_VALUE`
/// parses. `Err` is Java's message: `For input string: "" under radix 16` for
/// an empty segment, `Error at index N in: "..."` for a non-hex digit or
/// overflow, and the lone-sign cases.
fn parse_long_hex(segment: &[u16]) -> Result<i64, String> {
    if segment.is_empty() {
        // Java `NumberFormatException.forInputString("", 16)`.
        return Err("For input string: \"\" under radix 16".to_string());
    }
    let first = segment[0];
    let signed = first == u16::from(b'-') || first == u16::from(b'+');
    let negative = first == u16::from(b'-');
    // Java's `digit` sentinel `~0xFF` when the first char is a sign. `i` is 1
    // after the first char was consumed by `s.charAt(i++)`.
    let mut digit: i64 = if signed { -256 } else { hex_digit_value(first) };
    let mut i = 1;
    if digit >= 0 || (digit == -256 && segment.len() > 1) {
        // `limit`: `MIN_VALUE` for a leading '-', else `MIN_VALUE + 1`.
        let limit = if negative { i64::MIN } else { i64::MIN + 1 };
        let multmin = limit / 16;
        // `result = -(digit & 0xFF)`: for a sign, `digit & 0xFF` is 0, so
        // `result` starts at 0; for a hex digit `d`, it starts at `-d`.
        let mut result = -(digit & 0xff);
        let mut in_range = true;
        loop {
            if i >= segment.len() {
                break;
            }
            digit = hex_digit_value(segment[i]);
            i += 1;
            if digit < 0 {
                // Non-hex digit: Java's `digit >= 0` loop condition fails;
                // `inRange` keeps its previous (true) value.
                break;
            }
            // `inRange = result > multmin || result == multmin && digit <= (int)(radix*multmin - limit)`.
            let cast = (16 * multmin - limit) as i32 as i64;
            in_range = result > multmin || (result == multmin && digit <= cast);
            if !in_range {
                break;
            }
            result = 16 * result - digit;
        }
        if in_range && i == segment.len() && digit >= 0 {
            return Ok(if negative { result } else { -result });
        }
    }
    // `NumberFormatException.forCharSequence(s, 0, len, i - (digit < -1 ? 0 : 1))`.
    let error_index = i - if digit < -1 { 0 } else { 1 };
    // `segment` is a slice of `encode_utf16` output over a valid `&str`, so it
    // is well-formed UTF-16 and `from_utf16` cannot fail.
    let seg_str = String::from_utf16(segment).expect("segment is well-formed UTF-16");
    Err(format!("Error at index {error_index} in: \"{seg_str}\""))
}

/// `Character.digit(char, 16)` — the JDK-25 BMP digit set, returning the same
/// value the JVM does (0..=15) or `-1` for a non-digit. `c` is a UTF-16 code
/// unit. The set covers ASCII `0-9A-Fa-f`, the BMP decimal digits (Arabic-Indic
/// `٠-٩`, Devanagari `०-९`, fullwidth `０-９`, &c.), and fullwidth
/// `Ａ-Ｆ`/`ａ-ｆ`. Surrogate code units (U+D800-U+DFFF) are not digits and
/// match no range.
fn hex_digit_value(c: u16) -> i64 {
    let cp = c as u32;
    for &(lo, hi, base) in HEX_DIGIT_RANGES {
        if cp >= lo && cp <= hi {
            return cp as i64 + base as i64;
        }
    }
    -1
}

/// `Character.digit(cp, 16)` over the BMP, collapsed into ranges of contiguous
/// code points. Each entry maps a `cp` in `lo..=hi` to `cp as i64 + base`.
/// Generated from the local JDK 25 (`Character.digit(cp, 16) >= 0` for every
/// BMP code point); regenerate if the pinned JDK changes.
static HEX_DIGIT_RANGES: &[(u32, u32, i32)] = &[
    ('\u{0030}' as u32, '\u{0039}' as u32, -48),
    ('\u{0041}' as u32, '\u{0046}' as u32, -55),
    ('\u{0061}' as u32, '\u{0066}' as u32, -87),
    ('\u{0660}' as u32, '\u{0669}' as u32, -1632),
    ('\u{06F0}' as u32, '\u{06F9}' as u32, -1776),
    ('\u{07C0}' as u32, '\u{07C9}' as u32, -1984),
    ('\u{0966}' as u32, '\u{096F}' as u32, -2406),
    ('\u{09E6}' as u32, '\u{09EF}' as u32, -2534),
    ('\u{0A66}' as u32, '\u{0A6F}' as u32, -2662),
    ('\u{0AE6}' as u32, '\u{0AEF}' as u32, -2790),
    ('\u{0B66}' as u32, '\u{0B6F}' as u32, -2918),
    ('\u{0BE6}' as u32, '\u{0BEF}' as u32, -3046),
    ('\u{0C66}' as u32, '\u{0C6F}' as u32, -3174),
    ('\u{0CE6}' as u32, '\u{0CEF}' as u32, -3302),
    ('\u{0D66}' as u32, '\u{0D6F}' as u32, -3430),
    ('\u{0DE6}' as u32, '\u{0DEF}' as u32, -3558),
    ('\u{0E50}' as u32, '\u{0E59}' as u32, -3664),
    ('\u{0ED0}' as u32, '\u{0ED9}' as u32, -3792),
    ('\u{0F20}' as u32, '\u{0F29}' as u32, -3872),
    ('\u{1040}' as u32, '\u{1049}' as u32, -4160),
    ('\u{1090}' as u32, '\u{1099}' as u32, -4240),
    ('\u{17E0}' as u32, '\u{17E9}' as u32, -6112),
    ('\u{1810}' as u32, '\u{1819}' as u32, -6160),
    ('\u{1946}' as u32, '\u{194F}' as u32, -6470),
    ('\u{19D0}' as u32, '\u{19D9}' as u32, -6608),
    ('\u{1A80}' as u32, '\u{1A89}' as u32, -6784),
    ('\u{1A90}' as u32, '\u{1A99}' as u32, -6800),
    ('\u{1B50}' as u32, '\u{1B59}' as u32, -6992),
    ('\u{1BB0}' as u32, '\u{1BB9}' as u32, -7088),
    ('\u{1C40}' as u32, '\u{1C49}' as u32, -7232),
    ('\u{1C50}' as u32, '\u{1C59}' as u32, -7248),
    ('\u{A620}' as u32, '\u{A629}' as u32, -42528),
    ('\u{A8D0}' as u32, '\u{A8D9}' as u32, -43216),
    ('\u{A900}' as u32, '\u{A909}' as u32, -43264),
    ('\u{A9D0}' as u32, '\u{A9D9}' as u32, -43472),
    ('\u{A9F0}' as u32, '\u{A9F9}' as u32, -43504),
    ('\u{AA50}' as u32, '\u{AA59}' as u32, -43600),
    ('\u{ABF0}' as u32, '\u{ABF9}' as u32, -44016),
    ('\u{FF10}' as u32, '\u{FF19}' as u32, -65296),
    ('\u{FF21}' as u32, '\u{FF26}' as u32, -65303),
    ('\u{FF41}' as u32, '\u{FF46}' as u32, -65335),
];

/// `UUID.toString()` — the canonical `8-4-4-4-12` lowercase-hex form.
impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let most = format!("{:016x}", self.most as u64);
        let least = format!("{:016x}", self.least as u64);
        write!(
            f,
            "{}-{}-{}-{}-{}",
            &most[0..8],
            &most[8..12],
            &most[12..16],
            &least[0..4],
            &least[4..16]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(most: u64, least: u64) -> Uuid {
        Uuid {
            most: most as i64,
            least: least as i64,
        }
    }

    #[test]
    fn from_string_matches_java_accept_set() {
        // Java `UUID.fromString` (verified on the JDK 25 oracle JVM).
        let canonical = "00112233-4455-6677-8899-aabbccddeeff";
        assert_eq!(
            Uuid::from_string(canonical),
            Ok(uuid(0x00112233_44556677, 0x8899_aabbccddeeff))
        );
        // Uppercase hex is accepted.
        assert_eq!(
            Uuid::from_string("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"),
            Ok(uuid(0xffffffff_ffffffff, 0xffff_ffffffffffff))
        );
        // Variable-width dashed groups pad with zeros (`fromString1`).
        assert_eq!(
            Uuid::from_string("1-2-3-4-5"),
            Ok(uuid(0x00000001_00020003, 0x0004_000000000005))
        );
        // `Long.parseLong` accepts a leading `+` in a group.
        assert_eq!(
            Uuid::from_string("1-+2-3-4-5"),
            Ok(uuid(0x00000001_00020003, 0x0004_000000000005))
        );
        // A group wider than its field truncates via the mask (Java
        // `& 0x...`), as long as the raw value fits a signed long.
        assert_eq!(
            Uuid::from_string("100000000-2-3-4-5"),
            Ok(uuid(0x00000000_00020003, 0x0004_000000000005))
        );
        // The all-zero UUID parses fine (no variant validation).
        assert_eq!(
            Uuid::from_string("00000000-0000-0000-0000-000000000000"),
            Ok(uuid(0, 0))
        );
        // A full-width 8-hex-digit leading group fits a signed long
        // (`0xffffffff` < `Long.MAX_VALUE`) and masks to its 32-bit field.
        assert_eq!(
            Uuid::from_string("ffffffff-1-2-3-4"),
            Ok(uuid(0xffffffff_00010002, 0x0003_000000000004))
        );
    }

    #[test]
    fn from_string_rejects_java_rejected_forms() {
        // >36 chars is `IllegalArgumentException("UUID string too large")`;
        // braces, `urn:uuid:`, and a trailing dash all exceed 36 chars.
        assert_eq!(
            Uuid::from_string("{00112233-4455-6677-8899-aabbccddeeff}"),
            Err("UUID string too large".to_string())
        );
        assert_eq!(
            Uuid::from_string("urn:uuid:00112233-4455-6677-8899-aabbccddeeff"),
            Err("UUID string too large".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeefff"),
            Err("UUID string too large".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeeff-"),
            Err("UUID string too large".to_string())
        );
        // The undashed 32-char form has no 4th dash.
        assert_eq!(
            Uuid::from_string("00112233445566778899aabbccddeeff"),
            Err("Invalid UUID string: 00112233445566778899aabbccddeeff".to_string())
        );
        // A 5th dash (≤36 chars) is "Invalid UUID string".
        assert_eq!(
            Uuid::from_string("1-2-3-4-5-6"),
            Err("Invalid UUID string: 1-2-3-4-5-6".to_string())
        );
        // Empty groups: `Long.parseLong("", 16)`.
        assert_eq!(
            Uuid::from_string(""),
            Err("Invalid UUID string: ".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233--6677-8899-aabbccddeeff"),
            Err("For input string: \"\" under radix 16".to_string())
        );
        // Non-hex digits report Java's error index within the failing segment.
        assert_eq!(
            Uuid::from_string("0011223g-4455-6677-8899-aabbccddeeff"),
            Err("Error at index 7 in: \"0011223g\"".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeefg"),
            Err("Error at index 11 in: \"aabbccddeefg\"".to_string())
        );
    }

    #[test]
    fn from_string_rejects_groups_above_long_max() {
        // A 16-hex-digit group starting `8`..`f` exceeds `Long.MAX_VALUE`
        // (`0x7fffffffffffffff`), so Java's `Long.parseLong(segment, 16)`
        // overflows at the last digit (index 15) and `UUID.fromString`
        // rejects the whole string. The old unsigned parse wrongly accepted
        // these (masked to 0).
        assert_eq!(
            Uuid::from_string("8000000000000000-a-b-c-d"),
            Err("Error at index 15 in: \"8000000000000000\"".to_string())
        );
        assert_eq!(
            Uuid::from_string("ffffffffffffffff-a-b-c-d"),
            Err("Error at index 15 in: \"ffffffffffffffff\"".to_string())
        );
        // ...and the boundary just below `Long.MAX_VALUE` still parses.
        assert_eq!(
            Uuid::from_string("7fffffffffffffff-a-b-c-d"),
            Ok(uuid(0xffffffff_000a000b, 0x000c_00000000000d))
        );
    }

    #[test]
    fn from_string_accepts_unicode_hex_digits() {
        // Java's `Character.digit(char, 16)` accepts the full BMP digit set
        // and fullwidth `Ａ-Ｆ`/`ａ-ｆ`; `UUID.fromString` parses them
        // (verified on the local JDK 25.0.2).
        // Fullwidth digits ０-９ (U+FF10-U+FF19).
        assert_eq!(
            Uuid::from_string(
                "００１１２２３３-４４５５-６６７７-８８９９-ａａｂｂｃｃｄｄｅｅｆｆ"
            ),
            Ok(uuid(0x00112233_44556677, 0x8899_aabbccddeeff))
        );
        // Fullwidth letters Ａ-Ｆ / ａ-ｆ (U+FF21-U+FF26, U+FF41-U+FF46).
        assert_eq!(
            Uuid::from_string(
                "ＦＦＦＦＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦＦＦＦＦＦＦＦＦ"
            ),
            Ok(uuid(0xffffffff_ffffffff, 0xffff_ffffffffffff))
        );
        // Arabic-Indic ٠-٩ (U+0660-U+0669).
        assert_eq!(
            Uuid::from_string("٠١٢٣٤٥٦٧-٨٩٠١-٢٣٤٥-٦٧٨٩-٠١٢٣٤٥٦٧٨٩"),
            Ok(uuid(0x01234567_89012345, 0x6789_000123456789))
        );
        // Devanagari ०-९ (U+0966-U+096F).
        assert_eq!(
            Uuid::from_string("०१२३४५६७-८९०१-२३४५-६७८९-०१२३४५६७८९"),
            Ok(uuid(0x01234567_89012345, 0x6789_000123456789))
        );
        // Tibetan ༠-༩ (U+0F20-U+0F29).
        assert_eq!(
            Uuid::from_string("༠༡༢༣༤༥༦༧-༨༩༠༡-༢༣༤༥-༦༧༨༩-༠༡༢༣༤༥༦༧༨༩"),
            Ok(uuid(0x01234567_89012345, 0x6789_000123456789))
        );
        // Fullwidth and ASCII digits mix within one UUID.
        assert_eq!(
            Uuid::from_string("００１１２２３３-4455-6677-8899-aabbccddeeff"),
            Ok(uuid(0x00112233_44556677, 0x8899_aabbccddeeff))
        );
        // Short fullwidth groups pad with zeros like ASCII ones.
        assert_eq!(
            Uuid::from_string("１-２-３-４-５"),
            Ok(uuid(0x00000001_00020003, 0x0004_000000000005))
        );
    }

    #[test]
    fn from_string_rejects_invalid_unicode_neighbors() {
        // Fullwidth `Ｇ` (U+FF27) and `ｇ` (U+FF47) have digit value 16 — not a
        // hex digit — so Java rejects the whole string. The error index is
        // segment-relative, in UTF-16 code units (verified on the JDK 25
        // oracle).
        assert_eq!(
            Uuid::from_string(
                "ＧＦＦＦＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦＦＦＦＦＦＦＦＦ"
            ),
            Err("Error at index 0 in: \"ＧＦＦＦＦＦＦＦ\"".to_string())
        );
        assert_eq!(
            Uuid::from_string(
                "ｇＦＦＦＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦ-ＦＦＦＦＦＦＦＦＦＦＦＦ"
            ),
            Err("Error at index 0 in: \"ｇＦＦＦＦＦＦＦ\"".to_string())
        );
        // A bad digit mid-segment: the index counts code units within the
        // failing segment (segment-relative), matching Java.
        assert_eq!(
            Uuid::from_string(
                "００１１２２３ｇ-４４５５-６６７７-８８９９-ａａｂｂｃｃｄｄｅｅｆｆ"
            ),
            Err("Error at index 7 in: \"００１１２２３ｇ\"".to_string())
        );
        assert_eq!(
            Uuid::from_string(
                "００１１２２３３-４４５５-６６７７-８８９９-ａａｂｂｃｃｄｄｅｅｆｇ"
            ),
            Err("Error at index 11 in: \"ａａｂｂｃｃｄｄｅｅｆｇ\"".to_string())
        );
        // Superscript one U+00B9 is NOT a decimal digit (category No, not Nd),
        // so `Character.digit('¹', 16)` is -1 and Java rejects the string; the
        // index lands on the superscript, segment-relative (verified on JDK
        // 25.0.2).
        assert_eq!(
            Uuid::from_string("0011223¹-4455-6677-8899-aabbccddeeff"),
            Err("Error at index 7 in: \"0011223¹\"".to_string())
        );
    }

    #[test]
    fn to_string_matches_java_canonical_form() {
        // Java `UUID.toString()` (`digits(...)` over the two halves).
        assert_eq!(
            uuid(0x00112233_44556677, 0x8899_aabbccddeeff).to_string(),
            "00112233-4455-6677-8899-aabbccddeeff"
        );
        assert_eq!(
            uuid(0, 0).to_string(),
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(
            uuid(0xffffffff_ffffffff, 0xffff_ffffffffffff).to_string(),
            "ffffffff-ffff-ffff-ffff-ffffffffffff"
        );
        // The most-significant half spills into the least (high bit set in
        // `most` prints as a leading `f`, not a sign).
        assert_eq!(
            uuid(0xffffffff_ffffffff, 0).to_string(),
            "ffffffff-ffff-ffff-0000-000000000000"
        );
    }

    #[test]
    fn parse_to_string_round_trips() {
        for s in [
            "00112233-4455-6677-8899-aabbccddeeff",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "00000000-0000-0000-0000-000000000000",
            "1-2-3-4-5",
        ] {
            let parsed = Uuid::from_string(s).unwrap();
            // Variable-width groups normalize to the canonical form on parse,
            // so round-trip through `Display` for the canonical inputs only.
            if s.len() == 36 {
                assert_eq!(parsed.to_string(), s);
            }
        }
    }

    #[test]
    fn equality_and_hash_are_value_consistent() {
        // Java `UUID.equals` compares the two halves; derived `PartialEq`/
        // `Hash` must satisfy `a == b ⇒ hash(a) == hash(b)`.
        let a = Uuid::from_string("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        let b = Uuid::from_string("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        let c = Uuid::from_string("00112233-4455-6677-8899-aabbccddeef0").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, a);
        // Hashing the two equal values yields equal hashes.
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 1);
    }
}

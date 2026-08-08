//! Port of `Character.codePointOf`'s Unicode character-name resolution.
//!
//! Java resolves `\N{name}` (SNBT, `SnbtGrammar` `stringEscapeSequence`) via
//! `Character.codePointOf`, which consults `CharacterName` — the name table
//! packed into `java.base`'s `java/lang/uniName.dat` at JDK build time. This
//! module reproduces that algorithm (Temurin 25.0.2, Unicode 16.0):
//!
//! `codePointOf` semantics (OpenJDK `Character.codePointOf`):
//! 1. `name.trim().toUpperCase(Locale.ROOT)` — the lookup key is the name
//!    trimmed and upper-cased (the JDK stores only upper-case names, so this is
//!    the case-insensitive lookup);
//! 2. a hit in the name database returns the code point;
//! 3. otherwise a hex fallback parses the token after the last space as a
//!    base-16 code point and accepts it only if it names the very character
//!    (`name.equals(getName(cp))` — i.e. only when the parsed value is exactly
//!    the canonical name from this table, or its algorithmic name
//!    `<UnicodeBlock> <hex>` from `Character.getName(int)`);
//! 4. otherwise `IllegalArgumentException("Unrecognized character name :" +
//!    name)`.
//!
//! Step 3 is essential: assigned code points without a canonical name in the
//! table (CJK ideographs, Hangul syllables, surrogates, private-use, and other
//! `getType != UNASSIGNED` code points) are still resolvable through their
//! algorithmic name — e.g. `CJK UNIFIED IDEOGRAPHS 4E00` → `U+4E00`,
//! `HIGH SURROGATES D800` → `U+D800`. `Character.getName(int)` returns
//! `null` for unassigned code points, so the fallback never resolves those
//! (gated here by the `getType != UNASSIGNED` ranges in `ASSIGNED_RANGES`).
//!
//! Reachability from SNBT: the `\N{...}` name pattern (`[-a-zA-Z0-9 ]+`)
//! admits every algorithmic spelling — every `UnicodeBlock` name in
//! `BLOCK_NAMES` is within that charset, so `<block> <hex>` is always
//! expressible. Four canonical names contain `(`/`)` (`CARRIAGE RETURN (CR)`,
//! `FORM FEED (FF)`, `LINE FEED (LF)`, `NEXT LINE (NEL)`) and are NOT
//! expressible in an SNBT string; those are reachable only through the public
//! [`code_point_of`] helper.

/// The two ways `Character.codePointOf` can fail to produce a usable `char`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePointOfError {
    /// Java's `IllegalArgumentException` — the name is neither in the table nor
    /// an accepted algorithmic spelling. The parser reports
    /// `ERROR_INVALID_CHARACTER_NAME`.
    UnknownName,
    /// The hex fallback resolved a lone surrogate code point (e.g. `HIGH
    /// SURROGATES D800`). Java's `Character.toString` returns a `String`
    /// holding the lone surrogate; Rust `char` cannot represent one, so this
    /// implements the repository's unsupported-surrogate policy (issue #264):
    /// the parser reports the same invalid-codepoint error as the `\uHHHH`
    /// escape path, never panicking or lossy-replacing.
    // RivetTodo(#264): revisit if a surrogate-preserving boundary type is adopted.
    LoneSurrogate(u32),
}

/// `Character.codePointOf(String)` for the SNBT `\N{name}` escape.
///
/// Returns `Ok(cp)` when Java would return the code point, `Err(UnknownName)`
/// for Java's `IllegalArgumentException`, and `Err(LoneSurrogate(cp))` for the
/// surrogate divergence above.
pub fn code_point_of(name: &str) -> Result<u32, CodePointOfError> {
    // Java `String.trim()` strips leading/trailing chars `<= U+0020` (not the
    // broader Unicode-whitespace set that Rust's `trim()` uses), then
    // `toUpperCase(Locale.ROOT)`. The reachable SNBT names are ASCII-only, so
    // the trim difference is unobservable via the parser, but the public helper
    // is kept exactly faithful.
    let key = name.trim_matches(|c: char| c <= '\u{20}').to_uppercase();
    if let Some(cp) = NAME_TO_CODEPOINT.get(&key) {
        return Ok(*cp);
    }
    // Hex fallback: `Integer.parseInt(name, off + 1, name.length(), 16)`,
    // accepted only when the parsed value's canonical name equals the input.
    let Some(off) = key.rfind(' ') else {
        return Err(CodePointOfError::UnknownName);
    };
    let suffix = &key[off + 1..];
    if suffix.is_empty() {
        return Err(CodePointOfError::UnknownName);
    }
    let Ok(cp) = u32::from_str_radix(suffix, 16) else {
        return Err(CodePointOfError::UnknownName);
    };
    if cp > 0x10FFFF {
        return Err(CodePointOfError::UnknownName);
    }
    // `Character.getName(cp)` — must equal the input, or the fallback fails.
    if Some(key.as_str()) != character_name(cp).as_deref() {
        return Err(CodePointOfError::UnknownName);
    }
    if (0xD800..=0xDFFF).contains(&cp) {
        return Err(CodePointOfError::LoneSurrogate(cp));
    }
    Ok(cp)
}

/// `Character.getName(int)` — the canonical table name, else the algorithmic
/// `<UnicodeBlock> <hex>` name for assigned code points, else `None`.
fn character_name(cp: u32) -> Option<String> {
    if let Some(name) = CODEPOINT_TO_NAME.get(&cp) {
        return Some((*name).to_string());
    }
    if !is_assigned(cp) {
        return None;
    }
    let block = block_name(cp)?;
    Some(format!("{block} {cp:X}"))
}

/// `Character.getType(cp) != UNASSIGNED` — binary search over `ASSIGNED_RANGES`.
fn is_assigned(cp: u32) -> bool {
    let idx = ASSIGNED_RANGES.partition_point(|(s, _)| *s <= cp);
    if idx == 0 {
        return false;
    }
    let (s, e) = ASSIGNED_RANGES[idx - 1];
    s <= cp && cp <= e
}

/// `UnicodeBlock.of(cp).toString().replace('_', ' ')` — binary search over the
/// block start table. `None` where `UnicodeBlock.of` is null.
fn block_name(cp: u32) -> Option<&'static str> {
    let idx = BLOCK_STARTS.partition_point(|s| *s <= cp);
    if idx == 0 {
        return None;
    }
    BLOCK_NAMES[idx - 1]
}

include!("unicode_name_table_generated.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_resolve_to_their_code_point() {
        assert_eq!(code_point_of("SNOWMAN"), Ok(0x2603));
        assert_eq!(code_point_of("BLACK HEART SUIT"), Ok(0x2665));
        assert_eq!(code_point_of("LINEAR B SYLLABLE B008 A"), Ok(0x10000));
        assert_eq!(code_point_of("NULL"), Ok(0x0000));
        assert_eq!(code_point_of("CARRIAGE RETURN (CR)"), Ok(0x000D));
    }

    #[test]
    fn algorithmic_names_resolve_via_the_hex_fallback() {
        // JDK 25.0.2 probes (real `Character.codePointOf`).
        assert_eq!(code_point_of("CJK UNIFIED IDEOGRAPHS 4E00"), Ok(0x4E00));
        assert_eq!(code_point_of("HANGUL SYLLABLES AC00"), Ok(0xAC00));
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION A 3400"),
            Ok(0x3400)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION B 20000"),
            Ok(0x20000)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION I 2EBF0"),
            Ok(0x2EBF0)
        );
        assert_eq!(code_point_of("PRIVATE USE AREA E000"), Ok(0xE000));
        // Case-insensitive and trimmed, exactly like `codePointOf`.
        assert_eq!(code_point_of("  cjk unified ideographs 4e00  "), Ok(0x4E00));
    }

    #[test]
    fn surrogate_spellings_are_lone_surrogates() {
        // `codePointOf("HIGH SURROGATES D800")` returns 0xD800; the divergence
        // is reported as `LoneSurrogate` (Rust cannot hold it as a `char`).
        assert_eq!(
            code_point_of("HIGH SURROGATES D800"),
            Err(CodePointOfError::LoneSurrogate(0xD800))
        );
        assert_eq!(
            code_point_of("LOW SURROGATES DC00"),
            Err(CodePointOfError::LoneSurrogate(0xDC00))
        );
        // A non-matching name whose suffix happens to parse as a surrogate is
        // an unknown name, not a surrogate (Java throws `IllegalArgumentException`).
        assert_eq!(
            code_point_of("NONSENSE D800"),
            Err(CodePointOfError::UnknownName)
        );
    }

    #[test]
    fn lookup_is_case_insensitive_and_trims() {
        // `Character.codePointOf` = `name.trim().toUpperCase(Locale.ROOT)`.
        assert_eq!(code_point_of("  snowman "), Ok(0x2603));
        assert_eq!(code_point_of("Black Heart Suit"), Ok(0x2665));
        assert_eq!(code_point_of("\tLINEAR B SYLLABLE B008 A\n"), Ok(0x10000));
    }

    #[test]
    fn unknown_names_are_errors() {
        // The Java-rejected aliases and fabricated names.
        for name in [
            "NUL",
            "TAB",
            "LF",
            "LINE FEED",
            "CARRIAGE RETURN",
            "CJK UNIFIED IDEOGRAPH-4E00",
            "HANGUL SYLLABLE AC00",
            "SNOWMAN EXTRA",
            "A",
            "0",
            "NOT A REAL UNICODE NAME",
        ] {
            assert_eq!(
                code_point_of(name),
                Err(CodePointOfError::UnknownName),
                "name {name:?} must not resolve"
            );
        }
        // Table-named code points are NOT resolvable via an algorithmic
        // spelling (`name.equals(getName(cp))` fails) — JDK probes.
        assert_eq!(
            code_point_of("BASIC LATIN 41"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("GENERAL PUNCTUATION 2028"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("SPECIALS FFFE"),
            Err(CodePointOfError::UnknownName)
        );
        // Unassigned code points never resolve (`getName` returns null).
        assert_eq!(
            code_point_of("GREEK 0378"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("SPECIALS 10FFFE"),
            Err(CodePointOfError::UnknownName)
        );
    }

    #[test]
    fn every_table_entry_is_a_valid_code_point() {
        for (name, cp) in NAME_TO_CODEPOINT.entries() {
            assert!(
                char::from_u32(*cp).is_some(),
                "name {name:?} has invalid code point {cp:#x}"
            );
            // Round-trip through code_point_of (names are already upper-cased).
            assert_eq!(code_point_of(name), Ok(*cp), "name {name:?}");
        }
    }

    // ---- Exhaustive JDK-grounded families and boundaries. ----
    //
    // Every expectation below was verified against the real JDK 25.0.2
    // (`Character.codePointOf` / `Character.getName(int)`) via the
    // differential probe used while porting (scripts/gen_unicode_names.py +
    // the committed `tools/rivet-codegen/data/*.tsv` decoded from that JDK).

    /// Resolves through the algorithmic fallback — assigned, unnamed code
    /// points in the CJK, Hangul, Tangut, private-use, surrogate, and
    /// supplementary blocks (`Character.getName` returns the algorithmic
    /// `<UnicodeBlock> <hex>` spelling).
    #[test]
    fn algorithmic_family_spellings_resolve() {
        // CJK Unified Ideographs (BMP) and Extensions A/B/G/H/I.
        assert_eq!(code_point_of("CJK UNIFIED IDEOGRAPHS 4E00"), Ok(0x4E00));
        assert_eq!(code_point_of("CJK UNIFIED IDEOGRAPHS 9FA5"), Ok(0x9FA5));
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION A 3400"),
            Ok(0x3400)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION A 4DB5"),
            Ok(0x4DB5)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION B 20000"),
            Ok(0x20000)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION G 30000"),
            Ok(0x30000)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION H 31350"),
            Ok(0x31350)
        );
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS EXTENSION I 2EBF0"),
            Ok(0x2EBF0)
        );
        // Hangul Syllables (assigned block, unnamed).
        assert_eq!(code_point_of("HANGUL SYLLABLES AC00"), Ok(0xAC00));
        assert_eq!(code_point_of("HANGUL SYLLABLES D7A3"), Ok(0xD7A3));
        // Tangut block and Tangut Supplement (assigned, unnamed). The
        // 18800-18AFF "Tangut Components" are all canonical-named, so their
        // block never produces an algorithmic spelling.
        assert_eq!(code_point_of("TANGUT 17000"), Ok(0x17000));
        assert_eq!(code_point_of("TANGUT SUPPLEMENT 18D00"), Ok(0x18D00));
        // Private Use Area (BMP) and Supplementary Private Use Area (the JDK
        // splits it into "A" F0000-FFFFD and "B" 100000-10FFFD).
        assert_eq!(code_point_of("PRIVATE USE AREA E000"), Ok(0xE000));
        assert_eq!(code_point_of("PRIVATE USE AREA F8FF"), Ok(0xF8FF));
        assert_eq!(
            code_point_of("SUPPLEMENTARY PRIVATE USE AREA A F0000"),
            Ok(0xF0000)
        );
        assert_eq!(
            code_point_of("SUPPLEMENTARY PRIVATE USE AREA A FFFDD"),
            Ok(0xFFFDD)
        );
        assert_eq!(
            code_point_of("SUPPLEMENTARY PRIVATE USE AREA B 100000"),
            Ok(0x100000)
        );
        // Low/high/private-use surrogate blocks (assigned surrogates).
        assert_eq!(
            code_point_of("HIGH SURROGATES D800"),
            Err(CodePointOfError::LoneSurrogate(0xD800))
        );
        assert_eq!(
            code_point_of("HIGH SURROGATES DB7F"),
            Err(CodePointOfError::LoneSurrogate(0xDB7F))
        );
        assert_eq!(
            code_point_of("HIGH PRIVATE USE SURROGATES DB80"),
            Err(CodePointOfError::LoneSurrogate(0xDB80))
        );
        assert_eq!(
            code_point_of("LOW SURROGATES DC00"),
            Err(CodePointOfError::LoneSurrogate(0xDC00))
        );
        assert_eq!(
            code_point_of("LOW SURROGATES DFFF"),
            Err(CodePointOfError::LoneSurrogate(0xDFFF))
        );
    }

    /// Assigned, unnamed code points at the very edges of each named block
    /// resolve; the unassigned gap immediately outside them does not.
    #[test]
    fn algorithmic_block_boundaries() {
        // CJK Unified Ideographs: block start 4E00, end 9FFF (both assigned).
        assert_eq!(code_point_of("CJK UNIFIED IDEOGRAPHS 4E00"), Ok(0x4E00));
        assert_eq!(code_point_of("CJK UNIFIED IDEOGRAPHS 9FFF"), Ok(0x9FFF));
        // 9FFF+1 = A000 is Yi Syllables (assigned but a different block), so
        // the CJK spelling must NOT resolve it.
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS A000"),
            Err(CodePointOfError::UnknownName)
        );
        // Hangul Syllables block end D7A3 is assigned; D7A4 is UNASSIGNED.
        assert_eq!(code_point_of("HANGUL SYLLABLES D7A3"), Ok(0xD7A3));
        assert_eq!(
            code_point_of("HANGUL SYLLABLES D7A4"),
            Err(CodePointOfError::UnknownName)
        );
        // Unassigned code points never resolve even inside a named block.
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS 3400"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("GREEK 0378"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("SPECIALS 10FFFE"),
            Err(CodePointOfError::UnknownName)
        );
    }

    /// Table-named code points are NOT resolvable via their algorithmic
    /// spelling (`name.equals(getName(cp))` fails), and zero-padded hex never
    /// matches the unpadded algorithmic name.
    #[test]
    fn algorithmic_spelling_of_named_codepoint_rejected() {
        // Canonical names resolve; the same cps' block+hex spelling does not.
        assert_eq!(code_point_of("LATIN CAPITAL LETTER A"), Ok(0x41));
        assert_eq!(
            code_point_of("BASIC LATIN 41"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("GENERAL PUNCTUATION 2028"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("SPECIALS FFFE"),
            Err(CodePointOfError::UnknownName)
        );
        // Zero-padded algorithmic spellings (getName never pads) are rejected.
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS 004E00"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("HIGH SURROGATES 0D800"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("TANGUT IDEOGRAPHS 00017000"),
            Err(CodePointOfError::UnknownName)
        );
        // Lower-case hex in the suffix parses fine (Integer.parseInt radix 16).
        assert_eq!(code_point_of("CJK UNIFIED IDEOGRAPHS 4e0f"), Ok(0x4E0F));
    }

    /// Hex-suffix edge behavior that is reachable from SNBT: only names whose
    /// suffix parses as base-16 and equals the exact `getName` spelling pass;
    /// everything else (overflow, > MAX_CODE_POINT, non-hex, empty suffix) is
    /// an unknown name.
    #[test]
    fn hex_suffix_edge_cases() {
        // Over-range suffixes cannot be valid code points.
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS 110000"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("SPECIALS 10FFFF"),
            Err(CodePointOfError::UnknownName)
        );
        // A suffix longer than the real name's hex never equals getName(cp).
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS 4E000"),
            Err(CodePointOfError::UnknownName)
        );
        // Non-hex suffix chars and whitespace in the suffix fail to parse.
        assert_eq!(
            code_point_of("CJK UNIFIED IDEOGRAPHS 4G"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("BASIC LATIN 4 1"),
            Err(CodePointOfError::UnknownName)
        );
        // No space at all: the whole name is not a table entry, so unknown.
        assert_eq!(code_point_of("D800"), Err(CodePointOfError::UnknownName));
        // A fabricated block spelling that parses a surrogate suffix is an
        // unknown name (getName(cp) mismatch), not a lone surrogate.
        assert_eq!(
            code_point_of("NONSENSE D800"),
            Err(CodePointOfError::UnknownName)
        );
    }

    /// Every `UnicodeBlock` name is within the SNBT `[-a-zA-Z0-9 ]` charset,
    /// so every algorithmic spelling the fallback produces is expressible in
    /// an SNBT string (checked structurally against the committed block table).
    #[test]
    fn all_block_names_are_snbt_reachable() {
        let mut names: Vec<&str> = BLOCK_NAMES.iter().flatten().copied().collect();
        names.sort_unstable();
        names.dedup();
        for name in names {
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-'),
                "block name {name:?} contains a char outside [-a-zA-Z0-9 ]"
            );
        }
    }

    /// The four canonical names whose `(`/`)` chars the SNBT grammar cannot
    /// express are reachable through the public helper but not through SNBT.
    #[test]
    fn parenthesized_names_reachable_only_via_helper() {
        assert_eq!(code_point_of("CARRIAGE RETURN (CR)"), Ok(0x000D));
        assert_eq!(code_point_of("FORM FEED (FF)"), Ok(0x000C));
        assert_eq!(code_point_of("LINE FEED (LF)"), Ok(0x000A));
        assert_eq!(code_point_of("NEXT LINE (NEL)"), Ok(0x0085));
        // The unprefixed aliases Java does NOT recognize.
        assert_eq!(
            code_point_of("CARRIAGE RETURN"),
            Err(CodePointOfError::UnknownName)
        );
        assert_eq!(
            code_point_of("LINE FEED"),
            Err(CodePointOfError::UnknownName)
        );
    }
}

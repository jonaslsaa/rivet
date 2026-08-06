//! Port of `net.minecraft.IdentifierException` (MC 26.2).
//!
//! PROVENANCE: `net/minecraft/IdentifierException.java` is a class of the
//! `net.minecraft` root package, which the Rust port maps to `rivet-core`
//! (`crate_for` in `scripts/analyze_graph.py` returns `rivet-core` for bare
//! `net.minecraft`). Java:
//!
//! ```java
//! public class IdentifierException extends RuntimeException {
//!     public IdentifierException(final String message) {
//!         super(StringEscapeUtils.escapeJava(message));
//!     }
//!     public IdentifierException(final String message, final Throwable cause) {
//!         super(StringEscapeUtils.escapeJava(message), cause);
//!     }
//! }
//! ```
//!
//! The message is escaped with `StringEscapeUtils.escapeJava` (Commons Lang 3,
//! the `ESCAPE_JAVA` translator). `new_with_cause` drops the cause (the Rust
//! `std::error::Error` model does not reproduce Java's chained stack trace);
//! only the escaped message is retained.

/// Java's `IdentifierException extends RuntimeException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierException(String);

impl std::fmt::Display for IdentifierException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for IdentifierException {}

impl IdentifierException {
    /// `IdentifierException(String message)` — message escaped via
    /// `StringEscapeUtils.escapeJava`.
    pub fn new(message: impl AsRef<str>) -> Self {
        IdentifierException(escape_java(message.as_ref()))
    }

    /// `IdentifierException(String message, Throwable cause)` — the escaped
    /// message only; the cause chain is not represented in the Rust error.
    pub fn new_with_cause(
        message: impl AsRef<str>,
        _cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        IdentifierException::new(message)
    }

    /// `getMessage()` — the escaped message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

/// `StringEscapeUtils.escapeJava` (Commons Lang 3.20.0) over Rust `char`s.
///
/// The translator is `LookupTranslator("` + `\`) `.with(`
/// `LookupTranslator(JAVA_CTRL_CHARS_ESCAPE)) `.with(`
/// `JavaUnicodeEscaper.outsideOf(32, 0x7f))`. `outsideOf(32, 0x7f)` keeps
/// code points in `[32, 127]` and escapes everything else (0x7F = 127 is
/// *inside* the range, so it is kept). Iteration is by code point; a kept
/// supplementary code point is written as its two UTF-16 surrogates verbatim,
/// matching the top-level `CharSequenceTranslator` loop. Escaped supplementary
/// code points become the two `\uXXXX` surrogate halves (`JavaUnicodeEscaper.
/// toUtf16Escape`).
fn escape_java(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\u{000C}' => out.push_str("\\f"), // form feed
            '\r' => out.push_str("\\r"),
            _ if (0x20..=0x7F).contains(&(c as u32)) => out.push(c),
            _ if (c as u32) <= 0xFFFF => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            _ => {
                // Supplementary code point -> the two surrogate escapes
                // (`JavaUnicodeEscaper.toUtf16Escape`).
                let cp = c as u32;
                let high = 0xD800 + ((cp - 0x10000) >> 10);
                let low = 0xDC00 + ((cp - 0x10000) & 0x3FF);
                out.push_str(&format!("\\u{:04X}", high));
                out.push_str(&format!("\\u{:04X}", low));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::IdentifierException;

    /// Golden values produced by running `StringEscapeUtils.escapeJava` on
    /// commons-lang3 3.20.0 in the pinned Paper runtime (OpenJDK 25).
    #[test]
    fn escape_java_golden() {
        assert_eq!(IdentifierException::new("hello").message(), "hello");
        assert_eq!(
            IdentifierException::new("He didn't say, \"Stop!\"").message(),
            "He didn't say, \\\"Stop!\\\""
        );
        assert_eq!(
            IdentifierException::new("back\\slash").message(),
            "back\\\\slash"
        );
        assert_eq!(
            IdentifierException::new("a\nb\tc\r\u{000C}e\u{0008}b").message(),
            "a\\nb\\tc\\r\\fe\\bb"
        );
        assert_eq!(IdentifierException::new("café").message(), "caf\\u00E9");
        assert_eq!(IdentifierException::new("😀").message(), "\\uD83D\\uDE00");
        assert_eq!(
            IdentifierException::new("a\u{0000}b").message(),
            "a\\u0000b"
        );
        assert_eq!(
            IdentifierException::new("a\u{0001}b").message(),
            "a\\u0001b"
        );
        assert_eq!(
            IdentifierException::new("minecraft:stone").message(),
            "minecraft:stone"
        );
        // 0x7F is inside the kept range [32, 0x7F]; only < 32 and > 0x7F escape.
        assert_eq!(
            IdentifierException::new("a\u{007F}b").message(),
            "a\u{007F}b"
        );
        assert_eq!(
            IdentifierException::new("a\u{0080}b").message(),
            "a\\u0080b"
        );
        assert_eq!(IdentifierException::new("\u{0000}").message(), "\\u0000");
    }

    #[test]
    fn display_and_message_are_the_escaped_message() {
        let e = IdentifierException::new("x\ny");
        assert_eq!(e.message(), "x\\ny");
        assert_eq!(format!("{}", e), "x\\ny");
    }

    #[test]
    fn new_with_cause_escapes_the_message() {
        let e = IdentifierException::new_with_cause("boom\nline", std::io::Error::other("cause"));
        assert_eq!(e.message(), "boom\\nline");
    }
}

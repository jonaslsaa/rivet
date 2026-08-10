//! Port of `net.minecraft.resources.Identifier` (MC 26.2 `ResourceLocation`).
//!
//! PROVENANCE: leaf of the `mc.resources` manifest unit
//! (`net.minecraft.resources` -> `rivet-registry`). Java source:
//! `net/minecraft/resources/Identifier.java` (282 lines, 26.2).
//!
//! The port preserves Java's exact semantics (PORTING.md):
//!
//! - **Parsing**: `parse`/`by_separator` split on the first `:`. A leading
//!   `:` or a string with no `:` gets the `minecraft` namespace; otherwise the
//!   text before the `:` is the namespace and the rest is the path. Multiple
//!   colons: only the first is the separator — a `:` in the path is invalid.
//! - **Validation**: the `[a-z0-9_.-]` namespace / `[a-z0-9/._-]` path char
//!   sets, with the `..` namespace special-case. Constructor/parse errors are
//!   `IdentifierException` (a Java `RuntimeException` -> Rust panic) with the
//!   exact message, namespace/path normalized via `StringUtils.normalizeSpace`
//!   (Paper's sanitized error logging).
//! - **Length guard** (Paper): the private constructor throws
//!   `"Identifier too long: N"` when `namespace.length() + path.length() + 1`
//!   exceeds `Short.MAX_VALUE`, or its Netty `utf8MaxBytes` (`3 * length`)
//!   exceeds `2 * Short.MAX_VALUE + 1`. `String.length()` counts UTF-16 code
//!   units, so the port uses `encode_utf16().count()`.
//! - **Ordering**: `compareTo` compares **path first, then namespace**.
//! - **Hash**: `31 * namespace.hashCode() + path.hashCode()` via
//!   `rivet_util::java_hash::string_hash` (UTF-16), not `std` hashing.
//! - **`resolve_against`**: `root.resolve(namespace, path)` then normalize +
//!   starts-with escape check (`IllegalStateException` -> panic).
//!
//! Codec boundary: `Identifier::CODEC` (a `Codec<Identifier>` over
//! `codec::string_codec().comap_flat_map(Identifier::read, |i| i.to_string())`,
//! `.stable()`) belongs here; `Identifier::STREAM_CODEC` is `rivet-protocol`
//! (#126 holder codecs), NOT here — `rivet-registry` never depends on
//! `rivet-protocol` (OWNERSHIP.md).
//!
//! `read(StringReader)`/`read_non_empty` need `rivet_brigadier::StringReader` +
//! the `argument.id.invalid` `SimpleCommandExceptionType`, which are not a
//! dependency of `rivet-registry` (the ownership map's dependency list is
//! `rivet-core`/`rivet-serialization`/`rivet-util` only). They are deferred
//! wholesale to the brigadier wiring — no placeholder signature here.

use crate::identifier_exception::IdentifierException;

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::java_hash::string_hash;

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `Identifier.DEFAULT_NAMESPACE` = `"minecraft"`.
pub const DEFAULT_NAMESPACE: &str = "minecraft";
/// `Identifier.REALMS_NAMESPACE` = `"realms"`.
pub const REALMS_NAMESPACE: &str = "realms";
/// `Identifier.NAMESPACE_SEPARATOR` = `':'`.
pub const NAMESPACE_SEPARATOR: char = ':';
/// `Identifier.ALLOWED_NAMESPACE_CHARACTERS` = `"[a-z0-9_.-]"`.
pub const ALLOWED_NAMESPACE_CHARACTERS: &str = "[a-z0-9_.-]";
/// Paper's `Identifier.PAPER_NAMESPACE` = `"paper"`.
pub const PAPER_NAMESPACE: &str = "paper";

/// `Identifier.IdentifierParseError` — the exception thrown by the parsing
/// constructors. Java throws `IdentifierException`.
pub type IdentifierParseError = IdentifierException;

/// `net.minecraft.resources.Identifier` — the immutable (namespace, path) key.
///
/// Value type: two owned strings. `Eq`/`Ord`/`Hash` are hand-written to match
/// Java (`compareTo` path-first; `hashCode` = `31*ns + path` in UTF-16); see
/// the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    namespace: String,
    path: String,
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

/// `Identifier.compareTo` — **path first, then namespace**.
impl Ord for Identifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.namespace.cmp(&other.namespace))
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `Identifier.hashCode` = `31 * namespace.hashCode() + path.hashCode()` over
/// UTF-16 code units (PORTING.md UTF-16 drift checklist).
impl std::hash::Hash for Identifier {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let combined = string_hash(&self.namespace)
            .wrapping_mul(31)
            .wrapping_add(string_hash(&self.path));
        state.write_i32(combined);
    }
}

/// Unwraps an `IdentifierException` into a panic whose message is the escaped
/// Java message (`IdentifierException` is a `RuntimeException`). Generic over
/// the return type so it can satisfy any `unwrap_or_else` expected `T`.
fn panic_on_err<T>(e: IdentifierException) -> T {
    panic!("{}", e)
}

impl Identifier {
    /// The private `Identifier(String, String)` constructor — validates the
    /// Paper length guard only (the char assertions are checked by the
    /// `assert_valid_*` helpers before this is reached).
    fn new(namespace: String, path: String) -> Result<Self, IdentifierException> {
        // Paper: "Check for the max network string length (capped at
        // Short.MAX_VALUE) as well as the max bytes of a StringTag (length
        // written as an unsigned short)". `length` counts UTF-16 code units;
        // Netty's `utf8MaxBytes(n)` is `n * MAX_BYTES_PER_CHAR_UTF8` (3).
        let length = namespace.encode_utf16().count() + path.encode_utf16().count() + 1;
        let too_long = length > i16::MAX as usize;
        let utf8_too_long = length * 3 > 2 * i16::MAX as usize + 1;
        if too_long || utf8_too_long {
            return Err(IdentifierException::new(format!(
                "Identifier too long: {}",
                length
            )));
        }
        Ok(Identifier { namespace, path })
    }

    /// `Identifier.createUntrusted` — `new(assertValidNamespace(...),
    /// assertValidPath(...))`.
    fn create_untrusted(namespace: &str, path: &str) -> Result<Self, IdentifierException> {
        let namespace = assert_valid_namespace(namespace, path)?;
        let path = assert_valid_path(namespace, path)?;
        Self::new(namespace.to_string(), path.to_string())
    }

    /// `Identifier.fromNamespaceAndPath(String, String)`.
    pub fn from_namespace_and_path(namespace: &str, path: &str) -> Self {
        Self::create_untrusted(namespace, path).unwrap_or_else(panic_on_err)
    }

    /// `Identifier.parse(String)` — `bySeparator(identifier, ':')`.
    pub fn parse(identifier: &str) -> Self {
        Self::by_separator(identifier, ':')
    }

    /// `Identifier.bySeparator(String, char)`.
    ///
    /// Splits on the **first** occurrence of `separator`. A leading separator
    /// (or no separator) yields the `minecraft` namespace; otherwise the text
    /// before the separator is the namespace and everything after is the path
    /// (a second separator in the path is an invalid path character).
    pub fn by_separator(identifier: &str, separator: char) -> Self {
        Self::by_separator_result(identifier, separator).unwrap_or_else(panic_on_err)
    }

    /// `Identifier.withDefaultNamespace(String)`.
    pub fn with_default_namespace(path: &str) -> Self {
        let path = assert_valid_path(DEFAULT_NAMESPACE, path).unwrap_or_else(panic_on_err);
        Self::new(DEFAULT_NAMESPACE.to_string(), path.to_string()).unwrap_or_else(panic_on_err)
    }

    /// `Identifier.tryParse(String)` — `@Nullable`.
    pub fn try_parse(identifier: &str) -> Option<Self> {
        Self::try_parse_result(identifier).unwrap_or_else(panic_on_err)
    }

    /// Fallible form of Paper's nullable `Identifier.tryParse(String)`.
    ///
    /// Invalid characters remain `Ok(None)`, while the private constructor's
    /// unchecked Paper length guard is returned as `Err`. This lets callers
    /// preserve that exceptional boundary without relying on a Rust panic.
    pub fn try_parse_result(identifier: &str) -> Result<Option<Self>, IdentifierException> {
        Self::try_by_separator_result(identifier, ':')
    }

    /// `Identifier.tryBuild(String, String)` — `@Nullable`.
    pub fn try_build(namespace: &str, path: &str) -> Option<Self> {
        if Self::is_valid_namespace(namespace) && Self::is_valid_path(path) {
            Some(Self::new(namespace.to_string(), path.to_string()).unwrap_or_else(panic_on_err))
        } else {
            None
        }
    }

    /// `Identifier.tryBySeparator(String, char)` — `@Nullable`.
    pub fn try_by_separator(identifier: &str, separator: char) -> Option<Self> {
        Self::try_by_separator_result(identifier, separator).unwrap_or_else(panic_on_err)
    }

    /// Fallible form of `tryBySeparator`: invalid characters are nullable,
    /// but Paper's constructor length guard remains an error.
    pub fn try_by_separator_result(
        identifier: &str,
        separator: char,
    ) -> Result<Option<Self>, IdentifierException> {
        // Java's nullable variant. Note the Paper length guard in the private
        // constructor still throws; char validity is the only thing that maps
        // to `null`.
        if let Some(separator_index) = identifier.find(separator) {
            let path = &identifier[separator_index + separator.len_utf8()..];
            if !Self::is_valid_path(path) {
                return Ok(None);
            }
            if separator_index != 0 {
                let namespace = &identifier[..separator_index];
                if !Self::is_valid_namespace(namespace) {
                    return Ok(None);
                }
                Self::new(namespace.to_string(), path.to_string()).map(Some)
            } else {
                Self::new(DEFAULT_NAMESPACE.to_string(), path.to_string()).map(Some)
            }
        } else if Self::is_valid_path(identifier) {
            Self::new(DEFAULT_NAMESPACE.to_string(), identifier.to_string()).map(Some)
        } else {
            Ok(None)
        }
    }

    /// `Identifier.bySeparator` as a `Result` — the fallible parse. `read`
    /// uses it to build the `DataResult` error instead of panicking, and
    /// `rivet-protocol`'s `Identifier.STREAM_CODEC` uses it to surface the
    /// `IdentifierException` message as a `CodecError` at the codec boundary.
    pub fn by_separator_result(
        identifier: &str,
        separator: char,
    ) -> Result<Self, IdentifierException> {
        if let Some(separator_index) = identifier.find(separator) {
            let path = &identifier[separator_index + separator.len_utf8()..];
            if separator_index != 0 {
                let namespace = &identifier[..separator_index];
                Self::create_untrusted(namespace, path)
            } else {
                // Java: `withDefaultNamespace(path)`.
                let path = assert_valid_path(DEFAULT_NAMESPACE, path)?;
                Self::new(DEFAULT_NAMESPACE.to_string(), path.to_string())
            }
        } else {
            // Java: `withDefaultNamespace(identifier)`.
            let path = assert_valid_path(DEFAULT_NAMESPACE, identifier)?;
            Self::new(DEFAULT_NAMESPACE.to_string(), path.to_string())
        }
    }

    /// `Identifier.read(String)` — `DataResult<Identifier>`.
    ///
    /// Java catches the `IdentifierException` and errors with
    /// `"Not a valid resource location: <input> <message>"`.
    pub fn read(input: &str) -> DataResult<Identifier> {
        match Self::by_separator_result(input, ':') {
            Ok(identifier) => DataResult::success(identifier),
            Err(e) => DataResult::error(format!(
                "Not a valid resource location: {} {}",
                input,
                e.message()
            )),
        }
    }

    /// `Identifier.getNamespace()`.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// `Identifier.getPath()`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// `Identifier.withPath(String)`.
    pub fn with_path(&self, new_path: &str) -> Self {
        let path = assert_valid_path(&self.namespace, new_path).unwrap_or_else(panic_on_err);
        Self::new(self.namespace.clone(), path.to_string()).unwrap_or_else(panic_on_err)
    }

    /// `Identifier.withPath(UnaryOperator<String>)`.
    pub fn with_path_fn(&self, modifier: &dyn Fn(&str) -> String) -> Self {
        self.with_path(&modifier(&self.path))
    }

    /// `Identifier.withPrefix(String)`.
    pub fn with_prefix(&self, prefix: &str) -> Self {
        self.with_path(&format!("{}{}", prefix, self.path))
    }

    /// `Identifier.withSuffix(String)`.
    pub fn with_suffix(&self, suffix: &str) -> Self {
        self.with_path(&format!("{}{}", self.path, suffix))
    }

    /// `Identifier.resolveAgainst(Path)`.
    ///
    /// `root.resolve(namespace, path)` then `Path.normalize()`; if the
    /// normalized path escapes the normalized root, Java throws
    /// `IllegalStateException` ("tried to access path ... from root ..."). On
    /// success the *unnormalized* resulting path is returned (matching Java —
    /// `resolveAgainst` returns `resultingPath`, not the normalized one).
    pub fn resolve_against(&self, root: &Path) -> PathBuf {
        let resulting = root.join(&self.namespace).join(&self.path);
        let normalized = normalize_path(&resulting);
        let normalized_root = normalize_path(root);
        if !normalized.starts_with(&normalized_root) {
            panic!(
                "Identifier \"{}\" tried to access path \"{}\" from root \"{}\"",
                self,
                normalized.display(),
                normalized_root.display()
            );
        }
        resulting
    }

    /// `Identifier.toDebugFileName()`.
    pub fn to_debug_file_name(&self) -> String {
        self.to_string()
            .chars()
            .map(|c| if c == '/' || c == ':' { '_' } else { c })
            .collect()
    }

    /// `Identifier.toLanguageKey()`.
    pub fn to_language_key(&self) -> String {
        format!("{}.{}", self.namespace, self.path)
    }

    /// `Identifier.toShortLanguageKey()`.
    pub fn to_short_language_key(&self) -> String {
        if self.namespace == DEFAULT_NAMESPACE {
            self.path.clone()
        } else {
            self.to_language_key()
        }
    }

    /// `Identifier.toShortString()`.
    pub fn to_short_string(&self) -> String {
        if self.namespace == DEFAULT_NAMESPACE {
            self.path.clone()
        } else {
            self.to_string()
        }
    }

    /// `Identifier.toLanguageKey(String prefix)`.
    pub fn to_language_key_with_prefix(&self, prefix: &str) -> String {
        format!("{}.{}", prefix, self.to_language_key())
    }

    /// `Identifier.toLanguageKey(String prefix, String suffix)`.
    pub fn to_language_key_with_prefix_suffix(&self, prefix: &str, suffix: &str) -> String {
        format!("{}.{}.{}", prefix, self.to_language_key(), suffix)
    }

    /// `Identifier.isAllowedInIdentifier(char)`.
    // The `c >= '0' && c <= '9'` form mirrors the Java source character-for-
    // character (PORTING.md fidelity); allow the manual-range lint.
    #[allow(clippy::manual_range_contains)]
    pub fn is_allowed_in_identifier(c: char) -> bool {
        (c >= '0' && c <= '9')
            || (c >= 'a' && c <= 'z')
            || c == '_'
            || c == ':'
            || c == '/'
            || c == '.'
            || c == '-'
    }

    /// `Identifier.isValidPath(String)`.
    pub fn is_valid_path(path: &str) -> bool {
        path.chars().all(Self::valid_path_char)
    }

    /// `Identifier.isValidNamespace(String)`.
    pub fn is_valid_namespace(namespace: &str) -> bool {
        if namespace == ".." {
            return false;
        }
        namespace.chars().all(Self::valid_namespace_char)
    }

    /// `Identifier.validPathChar(char)`.
    #[allow(clippy::manual_range_contains)]
    pub fn valid_path_char(c: char) -> bool {
        c == '_'
            || c == '-'
            || (c >= 'a' && c <= 'z')
            || (c >= '0' && c <= '9')
            || c == '/'
            || c == '.'
    }

    /// `Identifier.validNamespaceChar(char)` (private in Java; exposed for the
    /// validation helper the unit needs internally).
    #[allow(clippy::manual_range_contains)]
    pub fn valid_namespace_char(c: char) -> bool {
        c == '_' || c == '-' || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '.'
    }
}

/// `Identifier.assertValidNamespace` — throws with the namespace AND path both
/// `normalizeSpace`d (Paper's sanitized error logging).
fn assert_valid_namespace<'a>(
    namespace: &'a str,
    path: &str,
) -> Result<&'a str, IdentifierException> {
    if Identifier::is_valid_namespace(namespace) {
        Ok(namespace)
    } else {
        Err(IdentifierException::new(format!(
            "Non [a-z0-9_.-] character in namespace of identifier: {}:{}",
            normalize_space(namespace),
            normalize_space(path)
        )))
    }
}

/// `Identifier.assertValidPath` — throws with the raw namespace and the
/// `normalizeSpace`d path.
fn assert_valid_path<'a>(namespace: &str, path: &'a str) -> Result<&'a str, IdentifierException> {
    if Identifier::is_valid_path(path) {
        Ok(path)
    } else {
        Err(IdentifierException::new(format!(
            "Non [a-z0-9/._-] character in path of location: {}:{}",
            namespace,
            normalize_space(path)
        )))
    }
}

/// `StringUtils.normalizeSpace` (Commons Lang 3.20.0) over Rust `char`s.
///
/// Collapses whitespace runs to a single ASCII space, trims leading/trailing
/// whitespace (Java `String.trim()`: chars `<= U+0020`), converts U+00A0
/// (NBSP — not Java whitespace) to a regular space, and returns `""` when the
/// whole string is whitespace. Whitespace is Java's `Character.isWhitespace`
/// (Unicode `White_Space` minus the non-breaking variants), NOT Rust's
/// `char::is_whitespace` — that would misclassify U+00A0.
fn normalize_space(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(input.len());
    let mut prev_was_space = true; // collapses leading whitespace
    for c in input.chars() {
        if is_java_whitespace(c) {
            if !prev_was_space {
                out.push(' ');
            }
            prev_was_space = true;
        } else {
            out.push(if c == '\u{00A0}' { ' ' } else { c });
            prev_was_space = false;
        }
    }
    out.trim_matches(|c| c <= ' ').to_string()
}

/// Java `Path.normalize()` — lexical `.`/`..` resolution without filesystem
/// access (used by `resolve_against`'s escape check). Rust's
/// `components().collect()` does NOT collapse `..`, so the Java algorithm is
/// reproduced: `.` is dropped, `..` pops the previous segment unless it is
/// itself `..` (Java keeps a leading `..`).
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Pop the previous non-`..` segment; a leading/stacked `..` is
                // preserved.
                if normalized
                    .components()
                    .next_back()
                    .map(|c| matches!(c, Component::Normal(_)))
                    == Some(true)
                {
                    normalized.pop();
                } else {
                    normalized.push("..");
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Java `Character.isWhitespace(char)`: the explicit control-char table plus
/// the Unicode space separators (Zs/Zl/Zp), excluding the non-breaking
/// variants U+00A0, U+2007 and U+202F.
fn is_java_whitespace(c: char) -> bool {
    match c as u32 {
        0x0009..=0x000D | 0x001C..=0x001F | 0x0020 => true,
        0x1680 => true,
        0x2000..=0x200A => c as u32 != 0x2007,
        0x2028 | 0x2029 | 0x205F | 0x3000 => true,
        _ => false,
    }
}

/// `Identifier::CODEC` — `Codec.STRING.comapFlatMap(Identifier::read,
/// Identifier::toString).stable()`.
pub fn identifier_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Identifier, Ops>> {
    codec::stable(codec::comap_flat_map::<String, Identifier, Ops>(
        codec::string_codec::<Ops>(),
        Arc::new(|input: &String| Identifier::read(input)),
        Arc::new(|identifier: &Identifier| identifier.to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use rivet_serialization::json_ops::JsonOps;
    use std::hash::{Hash, Hasher};

    /// A `Hasher` that captures the single `write_i32` an `Identifier` emits
    /// (its Java `hashCode`), for golden checks.
    #[derive(Default)]
    struct I32Capture(i32);

    impl Hasher for I32Capture {
        fn finish(&self) -> u64 {
            self.0 as u64
        }
        fn write(&mut self, _bytes: &[u8]) {
            panic!("Identifier hash must be a single write_i32");
        }
        fn write_i32(&mut self, i: i32) {
            self.0 = i;
        }
    }

    fn java_hash(id: &Identifier) -> i32 {
        let mut h = I32Capture::default();
        id.hash(&mut h);
        h.0
    }

    // ------------------------------------------------------------------
    // Parsing
    // ------------------------------------------------------------------

    #[test]
    fn parse_namespace_and_path() {
        let id = Identifier::parse("minecraft:stone");
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "stone");
        assert_eq!(id.to_string(), "minecraft:stone");

        let id = Identifier::parse("foo:bar/baz");
        assert_eq!(id.namespace(), "foo");
        assert_eq!(id.path(), "bar/baz");
    }

    #[test]
    fn parse_leading_separator_uses_default_namespace() {
        let id = Identifier::parse(":stone");
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "stone");
        assert_eq!(id.to_string(), "minecraft:stone");
    }

    #[test]
    fn parse_no_separator_uses_default_namespace() {
        let id = Identifier::parse("stone");
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "stone");
    }

    #[test]
    fn parse_empty_identifier() {
        // Java `bySeparator("", ':')` -> no separator -> `withDefaultNamespace("")`
        // -> an empty path is valid.
        let id = Identifier::parse("");
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "");
        assert_eq!(id.to_string(), "minecraft:");
    }

    #[test]
    fn parse_bare_separator() {
        let id = Identifier::parse(":");
        assert_eq!(id.to_string(), "minecraft:");
    }

    #[test]
    fn parse_trailing_separator() {
        let id = Identifier::parse("minecraft:");
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "");
    }

    #[test]
    fn parse_with_all_valid_characters() {
        let id = Identifier::parse("a.b-c_d:9/8.7");
        assert_eq!(id.namespace(), "a.b-c_d");
        assert_eq!(id.path(), "9/8.7");
    }

    #[test]
    fn from_namespace_and_path() {
        let id = Identifier::from_namespace_and_path("minecraft", "stone");
        assert_eq!(id.to_string(), "minecraft:stone");
    }

    #[test]
    fn with_default_namespace() {
        let id = Identifier::with_default_namespace("path");
        assert_eq!(id.to_string(), "minecraft:path");
    }

    #[test]
    fn by_separator_uses_first_separator() {
        // Splits on the FIRST occurrence of the given separator.
        let id = Identifier::by_separator("a;b", ';');
        assert_eq!(id.namespace(), "a");
        assert_eq!(id.path(), "b");
        // No separator -> default namespace on the whole string (path `ab`).
        let id = Identifier::by_separator("ab", ':');
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "ab");
        // A path containing a ':' is invalid regardless of separator.
        assert!(Identifier::try_by_separator("a:b:c", ';').is_none());
    }

    // ------------------------------------------------------------------
    // Errors (exact messages, Java-grounded)
    // ------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "Non [a-z0-9_.-] character in namespace of identifier: a b:c")]
    fn namespace_with_space_errors() {
        Identifier::from_namespace_and_path("a b", "c");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9_.-] character in namespace of identifier: aA:c")]
    fn namespace_with_uppercase_errors() {
        Identifier::from_namespace_and_path("aA", "c");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9_.-] character in namespace of identifier: ..:c")]
    fn namespace_of_dots_errors() {
        Identifier::from_namespace_and_path("..", "c");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9_.-] character in namespace of identifier: a/b:c")]
    fn namespace_with_slash_errors() {
        Identifier::from_namespace_and_path("a/b", "c");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9/._-] character in path of location: minecraft:b c")]
    fn path_with_space_errors() {
        Identifier::from_namespace_and_path("minecraft", "b c");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9/._-] character in path of location: minecraft:b:c")]
    fn path_with_colon_errors() {
        Identifier::parse("minecraft:b:c");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9/._-] character in path of location: minecraft:aA")]
    fn path_with_uppercase_errors() {
        Identifier::from_namespace_and_path("minecraft", "aA");
    }

    #[test]
    #[should_panic(
        expected = "Non [a-z0-9/._-] character in path of location: minecraft:\\uD83D\\uDE00"
    )]
    fn path_with_emoji_errors() {
        // The panic message is the escaped `IdentifierException` message:
        // the emoji escapes as its two surrogate halves.
        Identifier::parse("minecraft:😀");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9_.-] character in namespace of identifier: a b:c d")]
    fn namespace_and_path_both_normalized_in_namespace_error() {
        // Java normalizes BOTH sides of the namespace error message.
        Identifier::from_namespace_and_path("a b", "c d");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9/._-] character in path of location: minecraft:a  b")]
    fn nb_sp_in_path_normalized_in_path_error() {
        // Paper error-log sanitization: NBSP collapses with the following space
        // (StringUtils.normalizeSpace) in the path error message only.
        Identifier::from_namespace_and_path("minecraft", "a\u{00A0} b");
    }

    #[test]
    #[should_panic(expected = "Identifier too long: 40001")]
    fn over_long_identifier_errors() {
        let ns = "a".repeat(20_000);
        let path = "b".repeat(20_000);
        Identifier::from_namespace_and_path(&ns, &path);
    }

    #[test]
    fn length_guard_utf8_boundary() {
        // Effective bound: `length * 3 > 2 * Short.MAX_VALUE + 1` (65535),
        // i.e. length > 21845. 21845 passes, 21846 throws.
        let ns = "a".repeat(10_922);
        let path = "b".repeat(10_922);
        assert_eq!(
            ns.encode_utf16().count() + path.encode_utf16().count() + 1,
            21_845
        );
        let id = Identifier::from_namespace_and_path(&ns, &path);
        assert_eq!(id.namespace().len(), 10_922);
    }

    #[test]
    #[should_panic(expected = "Identifier too long: 21846")]
    fn length_guard_utf8_boundary_next() {
        let ns = "a".repeat(10_923);
        let path = "b".repeat(10_922);
        Identifier::from_namespace_and_path(&ns, &path);
    }

    // ------------------------------------------------------------------
    // try_* (nullable)
    // ------------------------------------------------------------------

    #[test]
    fn try_parse_returns_some_for_valid() {
        assert_eq!(
            Identifier::try_parse("minecraft:stone")
                .unwrap()
                .to_string(),
            "minecraft:stone"
        );
        assert_eq!(
            Identifier::try_parse("foo:bar/baz").unwrap().to_string(),
            "foo:bar/baz"
        );
        assert_eq!(
            Identifier::try_parse(":path").unwrap().to_string(),
            "minecraft:path"
        );
        assert_eq!(
            Identifier::try_parse("plain").unwrap().to_string(),
            "minecraft:plain"
        );
        assert_eq!(Identifier::try_parse("a:").unwrap().to_string(), "a:");
    }

    #[test]
    fn try_parse_returns_none_for_invalid() {
        assert!(Identifier::try_parse("a b:c").is_none());
        assert!(Identifier::try_parse("..:x").is_none());
        assert!(Identifier::try_parse("aA:b").is_none());
        assert!(Identifier::try_parse("minecraft:😀").is_none());
        assert!(Identifier::try_parse("minecraft:b c").is_none());
        assert!(Identifier::try_parse("a/b:c").is_none());
    }

    #[test]
    fn try_build() {
        assert_eq!(
            Identifier::try_build("minecraft", "stone")
                .unwrap()
                .to_string(),
            "minecraft:stone"
        );
        assert!(Identifier::try_build("..", "x").is_none());
        assert!(Identifier::try_build("aA", "b").is_none());
        assert!(Identifier::try_build("a", "b c").is_none());
        assert!(Identifier::try_build("a", "b:c").is_none());
    }

    // ------------------------------------------------------------------
    // with_* projections
    // ------------------------------------------------------------------

    #[test]
    fn with_path() {
        let id = Identifier::parse("minecraft:stone");
        assert_eq!(id.with_path("dirt").to_string(), "minecraft:dirt");
        assert_eq!(id.with_path("dir/dirt").to_string(), "minecraft:dir/dirt");
    }

    #[test]
    #[should_panic(expected = "Non [a-z0-9/._-] character in path of location: minecraft:b c")]
    fn with_path_validates() {
        Identifier::parse("minecraft:stone").with_path("b c");
    }

    #[test]
    fn with_path_fn() {
        let id = Identifier::parse("minecraft:stone");
        assert_eq!(
            id.with_path_fn(&|p: &str| format!("{}{}", "pre_", p))
                .to_string(),
            "minecraft:pre_stone"
        );
    }

    #[test]
    fn with_prefix() {
        let id = Identifier::parse("minecraft:stone");
        assert_eq!(
            id.with_prefix("block/").to_string(),
            "minecraft:block/stone"
        );
    }

    #[test]
    fn with_suffix() {
        let id = Identifier::parse("minecraft:stone");
        assert_eq!(id.with_suffix("_item").to_string(), "minecraft:stone_item");
    }

    // ------------------------------------------------------------------
    // Equality / hash / ordering
    // ------------------------------------------------------------------

    #[test]
    fn value_equality() {
        let a = Identifier::parse("minecraft:stone");
        let b = Identifier::parse("minecraft:stone");
        let c = Identifier::from_namespace_and_path("minecraft", "stone");
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_ne!(a, Identifier::parse("minecraft:dirt"));
        assert_ne!(a, Identifier::parse("foo:stone"));
    }

    #[test]
    fn hash_matches_java_formula() {
        let id = Identifier::parse("minecraft:stone");
        // Java `hashCode` = 31 * "minecraft".hashCode() + "stone".hashCode(),
        // wrapping i32 arithmetic. Golden values from OpenJDK 25:
        // "minecraft" = 695073197, "stone" = 109770853.
        let expected = 695_073_197_i64.wrapping_mul(31).wrapping_add(109_770_853) as i32;
        assert_eq!(java_hash(&id), expected);
        // The identifier's Java hash is NOT the hash of the joined string.
        assert_ne!(java_hash(&id), string_hash("minecraft:stone"));
        // Equal values hash equal; different values hash differently (here).
        assert_eq!(
            java_hash(&id),
            java_hash(&Identifier::parse("minecraft:stone"))
        );
        assert_ne!(
            java_hash(&id),
            java_hash(&Identifier::parse("minecraft:dirt"))
        );
    }

    #[test]
    fn compare_path_first_then_namespace() {
        // Path is the primary sort key.
        assert!(Identifier::parse("minecraft:z") > Identifier::parse("minecraft:a"));
        // Paths equal -> namespace breaks the tie.
        assert!(Identifier::parse("zzz:b") > Identifier::parse("aaa:b"));
        // Prefix ordering on path.
        assert!(Identifier::parse("minecraft:ab") > Identifier::parse("minecraft:a"));
        // Equality.
        assert_eq!(
            Identifier::parse("minecraft:a").cmp(&Identifier::parse("minecraft:a")),
            Ordering::Equal
        );
    }

    #[test]
    fn sort_orders_by_path_then_namespace() {
        let mut ids = [
            Identifier::parse("minecraft:b"),
            Identifier::parse("aaa:x"),
            Identifier::parse("minecraft:a"),
        ];
        ids.sort();
        let sorted: Vec<String> = ids.iter().map(ToString::to_string).collect();
        assert_eq!(sorted, vec!["minecraft:a", "minecraft:b", "aaa:x"]);
    }

    // ------------------------------------------------------------------
    // Language / debug helpers
    // ------------------------------------------------------------------

    #[test]
    fn language_keys() {
        let id = Identifier::parse("minecraft:stone");
        assert_eq!(id.to_language_key(), "minecraft.stone");
        assert_eq!(id.to_short_language_key(), "stone");
        assert_eq!(id.to_short_string(), "stone");
        assert_eq!(
            id.to_language_key_with_prefix("prefix"),
            "prefix.minecraft.stone"
        );
        assert_eq!(
            id.to_language_key_with_prefix_suffix("prefix", "suffix"),
            "prefix.minecraft.stone.suffix"
        );

        let other = Identifier::parse("foo:bar");
        assert_eq!(other.to_language_key(), "foo.bar");
        assert_eq!(other.to_short_language_key(), "foo.bar");
        assert_eq!(other.to_short_string(), "foo:bar");
    }

    #[test]
    fn debug_file_name() {
        let id = Identifier::parse("abc:b/c");
        assert_eq!(id.to_debug_file_name(), "abc_b_c");
    }

    // ------------------------------------------------------------------
    // Validation
    // ------------------------------------------------------------------

    #[test]
    fn valid_namespace_char_table() {
        assert!(Identifier::valid_namespace_char('a'));
        assert!(Identifier::valid_namespace_char('z'));
        assert!(Identifier::valid_namespace_char('0'));
        assert!(Identifier::valid_namespace_char('9'));
        assert!(Identifier::valid_namespace_char('_'));
        assert!(Identifier::valid_namespace_char('-'));
        assert!(Identifier::valid_namespace_char('.'));
        assert!(!Identifier::valid_namespace_char('/'));
        assert!(!Identifier::valid_namespace_char('A'));
        assert!(!Identifier::valid_namespace_char(' '));
        assert!(!Identifier::valid_namespace_char(':'));
        assert!(!Identifier::valid_namespace_char('+'));
    }

    #[test]
    fn valid_path_char_table() {
        assert!(Identifier::valid_path_char('a'));
        assert!(Identifier::valid_path_char('9'));
        assert!(Identifier::valid_path_char('_'));
        assert!(Identifier::valid_path_char('-'));
        assert!(Identifier::valid_path_char('/'));
        assert!(Identifier::valid_path_char('.'));
        assert!(!Identifier::valid_path_char(':'));
        assert!(!Identifier::valid_path_char('A'));
        assert!(!Identifier::valid_path_char(' '));
    }

    #[test]
    fn is_valid_path_and_namespace() {
        assert!(Identifier::is_valid_path("a/b_c.d-9"));
        assert!(Identifier::is_valid_path(""));
        assert!(!Identifier::is_valid_path("a:b"));
        assert!(!Identifier::is_valid_path("a b"));
        assert!(!Identifier::is_valid_path("A"));

        assert!(Identifier::is_valid_namespace("a.b-c_d"));
        assert!(Identifier::is_valid_namespace("minecraft"));
        assert!(!Identifier::is_valid_namespace(".."));
        assert!(!Identifier::is_valid_namespace("a/b"));
        assert!(!Identifier::is_valid_namespace("a b"));
        // Java's `isValidNamespace("")` iterates zero chars and returns true.
        assert!(Identifier::is_valid_namespace(""));
    }

    #[test]
    fn is_allowed_in_identifier_table() {
        for c in ['0', '9', 'a', 'z', '_', ':', '/', '.', '-'] {
            assert!(Identifier::is_allowed_in_identifier(c), "{:?}", c);
        }
        for c in ['A', ' ', '#', '+', '\\'] {
            assert!(!Identifier::is_allowed_in_identifier(c), "{:?}", c);
        }
    }

    // ------------------------------------------------------------------
    // resolve_against
    // ------------------------------------------------------------------

    #[test]
    fn resolve_against_joins_namespace_and_path() {
        let id = Identifier::parse("minecraft:stone");
        let resolved = id.resolve_against(Path::new("/tmp/root"));
        assert_eq!(resolved, PathBuf::from("/tmp/root/minecraft/stone"));
    }

    #[test]
    fn resolve_against_returns_unnormalized_on_success() {
        // Java returns the unnormalized resulting path when it stays under the
        // root.
        let id = Identifier::parse("minecraft:a/../b");
        let resolved = id.resolve_against(Path::new("/tmp/root"));
        assert_eq!(resolved, PathBuf::from("/tmp/root/minecraft/a/../b"));
    }

    #[test]
    #[should_panic(expected = "tried to access path \"/tmp/etc/passwd\" from root \"/tmp/root\"")]
    fn resolve_against_escaping_root_panics() {
        let id = Identifier::parse("minecraft:../../etc/passwd");
        let _ = id.resolve_against(Path::new("/tmp/root"));
    }

    // ------------------------------------------------------------------
    // read (DataResult)
    // ------------------------------------------------------------------

    #[test]
    fn read_success() {
        let result = Identifier::read("minecraft:stone");
        assert_eq!(
            result.result().unwrap().clone(),
            Identifier::parse("minecraft:stone")
        );
        assert!(result.error_ref().is_none());
    }

    #[test]
    fn read_error_message_for_invalid_namespace() {
        // `bySeparator("a b:c")` splits on the first ':' -> namespace "a b",
        // path "c"; the namespace fails first (`createUntrusted` validates the
        // namespace before the path), so the error is the namespace one.
        let result = Identifier::read("a b:c");
        assert!(result.result().is_none());
        let binding = result.error_ref().unwrap();
        assert_eq!(
            binding.message(),
            "Not a valid resource location: a b:c Non [a-z0-9_.-] character in namespace of identifier: a b:c"
        );
    }

    #[test]
    fn read_error_message_for_invalid_path() {
        let result = Identifier::read("minecraft:b c");
        assert!(result.result().is_none());
        let binding = result.error_ref().unwrap();
        assert_eq!(
            binding.message(),
            "Not a valid resource location: minecraft:b c Non [a-z0-9/._-] character in path of location: minecraft:b c"
        );
    }

    // ------------------------------------------------------------------
    // codec (round-trip through JsonOps)
    // ------------------------------------------------------------------

    #[test]
    fn identifier_codec_roundtrips() {
        let ops = JsonOps::INSTANCE;
        let codec = identifier_codec::<JsonOps>();
        let id = Identifier::parse("minecraft:stone");
        let encoded = codec.encode_start(&ops, &id).get_or_throw("encode").clone();
        assert_eq!(encoded, ops.create_string("minecraft:stone".to_string()));
        let input = ops.create_string("minecraft:stone".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, id);
    }

    #[test]
    fn identifier_codec_decodes_invalid_to_error() {
        let ops = JsonOps::INSTANCE;
        let codec = identifier_codec::<JsonOps>();
        let input = ops.create_string("a b:c".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.result().is_none());
    }
}

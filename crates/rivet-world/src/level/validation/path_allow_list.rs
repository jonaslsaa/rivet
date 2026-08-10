//! `net.minecraft.world.level.validation.PathAllowList`.
//!
//! Paper delegates `glob:` and `regex:` entries to the path's `FileSystem`.
//! Rivet currently has one native filesystem, so Paper's per-provider cache is
//! represented by one lazily compiled matcher list. Glob conversion follows
//! OpenJDK's platform-specific `sun.nio.fs.Globs` rules. Regex entries use
//! a deliberately small Java-compatible subset compiled by PCRE2. Syntax
//! outside that subset is rejected rather than risking a broader rule.

use std::io::{self, BufRead};
use std::path::Path;
use std::sync::OnceLock;

use pcre2::bytes::{Regex, RegexBuilder};

/// An ordered, lazily compiled `allowed_symlinks.txt` matcher list.
#[derive(Debug)]
pub struct PathAllowList {
    entries: Vec<ConfigEntry>,
    compiled_paths: OnceLock<Option<Vec<CompiledMatcher>>>,
}

impl Clone for PathAllowList {
    fn clone(&self) -> Self {
        Self::new(self.entries.clone())
    }
}

impl PathAllowList {
    /// `new PathAllowList(List<ConfigEntry>)`.
    pub fn new(entries: Vec<ConfigEntry>) -> Self {
        Self {
            entries,
            compiled_paths: OnceLock::new(),
        }
    }

    /// `PathAllowList.matches(Path)`.
    ///
    /// Paper compiles every entry before it attempts the first match. If any
    /// entry fails to compile, the cached matcher rejects every path.
    pub fn matches(&self, path: &Path) -> bool {
        let compiled = self.compiled_paths.get_or_init(|| {
            self.entries
                .iter()
                .map(ConfigEntry::compile)
                .collect::<Result<Vec<_>, _>>()
                .ok()
        });
        compiled
            .as_ref()
            .is_some_and(|matchers| matchers.iter().any(|matcher| matcher.matches(path)))
    }

    /// `PathAllowList.readPlain(BufferedReader)`.
    pub fn read_plain(reader: impl BufRead) -> io::Result<Self> {
        let mut entries = Vec::new();
        for line in reader.lines() {
            if let Some(entry) = ConfigEntry::parse(&line?)
                .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?
            {
                entries.push(entry);
            }
        }
        Ok(Self::new(entries))
    }
}

/// One parsed allow-list line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    ty: EntryType,
    pattern: String,
}

impl ConfigEntry {
    /// `ConfigEntry.type()`; named to avoid Rust's `type` keyword.
    pub fn entry_type(&self) -> EntryType {
        self.ty
    }

    /// `ConfigEntry.pattern()`.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    fn compile(&self) -> Result<CompiledMatcher, String> {
        match self.ty {
            EntryType::Prefix => Ok(CompiledMatcher::Prefix(self.pattern.clone())),
            EntryType::Filesystem => {
                let (syntax, pattern) = self.pattern.split_once(':').ok_or_else(|| {
                    "filesystem path matcher must contain a syntax prefix".to_string()
                })?;
                let expression = if syntax.eq_ignore_ascii_case("glob") {
                    glob_to_native_regex(pattern)?
                } else if syntax.eq_ignore_ascii_case("regex") {
                    validate_java_regex(pattern)?;
                    pattern.to_string()
                } else {
                    return Err(format!("Syntax '{syntax}' not recognized"));
                };
                let expression = java_dots_to_pcre2(&expression);
                let mut builder = RegexBuilder::new();
                builder.utf(true).ucp(false);
                #[cfg(windows)]
                builder.caseless(true);
                // Matcher.matches() constrains the engine while it evaluates
                // alternatives; checking the span of an unconstrained find
                // would incorrectly reject `short|longer` after `short` wins.
                let expression = format!(r"\A(?:{expression})\z");
                builder
                    .build(&expression)
                    .map(CompiledMatcher::Regex)
                    .map_err(|error| error.to_string())
            }
        }
    }

    /// `ConfigEntry.parse(String)`.
    pub fn parse(definition: &str) -> Result<Option<Self>, String> {
        if java_is_blank(definition) || definition.starts_with('#') {
            return Ok(None);
        }

        if !definition.starts_with('[') {
            return Ok(Some(Self {
                ty: EntryType::Prefix,
                pattern: definition.to_string(),
            }));
        }

        let Some(split) = definition[1..].find(']').map(|index| index + 1) else {
            return Err(format!("Unterminated type in line '{definition}'"));
        };
        let ty = &definition[1..split];
        let contents = &definition[split + 1..];

        match ty {
            "glob" | "regex" => Ok(Some(Self {
                ty: EntryType::Filesystem,
                pattern: format!("{ty}:{contents}"),
            })),
            "prefix" => Ok(Some(Self {
                ty: EntryType::Prefix,
                pattern: contents.to_string(),
            })),
            _ => Err(format!(
                "Unsupported definition type in line '{definition}'"
            )),
        }
    }

    /// Test/construction equivalent of Paper's `ConfigEntry.glob`.
    #[cfg(test)]
    pub(crate) fn glob(pattern: impl Into<String>) -> Self {
        Self {
            ty: EntryType::Filesystem,
            pattern: format!("glob:{}", pattern.into()),
        }
    }

    /// Test/construction equivalent of Paper's `ConfigEntry.regex`.
    #[cfg(test)]
    pub(crate) fn regex(pattern: impl Into<String>) -> Self {
        Self {
            ty: EntryType::Filesystem,
            pattern: format!("regex:{}", pattern.into()),
        }
    }

    /// Test/construction equivalent of Paper's `ConfigEntry.prefix`.
    #[cfg(test)]
    pub(crate) fn prefix(pattern: impl Into<String>) -> Self {
        Self {
            ty: EntryType::Prefix,
            pattern: pattern.into(),
        }
    }
}

/// Paper's two `EntryType` implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Filesystem,
    Prefix,
}

#[derive(Debug)]
enum CompiledMatcher {
    Prefix(String),
    Regex(Regex),
}

impl CompiledMatcher {
    fn matches(&self, path: &Path) -> bool {
        let path = path.to_string_lossy();
        match self {
            // This is deliberately a string prefix, not Path::starts_with.
            CompiledMatcher::Prefix(prefix) => path.starts_with(prefix),
            CompiledMatcher::Regex(regex) => regex.is_match(path.as_bytes()).unwrap_or(false),
        }
    }
}

fn java_is_blank(value: &str) -> bool {
    value.chars().all(java_is_whitespace)
}

fn java_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{001C}'..='\u{0020}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

#[cfg(windows)]
fn glob_to_native_regex(glob: &str) -> Result<String, String> {
    glob_to_regex(glob, true)
}

#[cfg(not(windows))]
fn glob_to_native_regex(glob: &str) -> Result<String, String> {
    glob_to_regex(glob, false)
}

/// OpenJDK's Unix/Windows glob conversion. A positive class is preceded by a
/// separator-negative assertion, the PCRE2 equivalent of OpenJDK's class
/// intersection. This matters when a range (for example `[.-0]`) contains the
/// separator without spelling it explicitly.
fn glob_to_regex(glob: &str, windows: bool) -> Result<String, String> {
    let chars: Vec<char> = glob.chars().collect();
    let mut regex = String::from("^");
    let mut in_group = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '\\' => {
                let Some(&next) = chars.get(i) else {
                    return Err(format!("No character to escape near index {}", i - 1));
                };
                i += 1;
                if is_glob_meta(next) || is_regex_meta(next) {
                    regex.push('\\');
                }
                regex.push(next);
            }
            '/' if windows => regex.push_str("\\\\"),
            '/' => regex.push('/'),
            '[' => {
                let class_start = i - 1;
                let mut negated = false;
                let mut has_member = false;
                if chars.get(i) == Some(&'^') {
                    regex.push_str(if windows { "(?!\\\\)" } else { "(?!/)" });
                    regex.push_str("[\\^");
                    i += 1;
                    has_member = true;
                } else {
                    if chars.get(i) == Some(&'!') {
                        negated = true;
                        i += 1;
                    }
                    if negated {
                        regex.push_str(if windows { "[^\\\\" } else { "[^/" });
                    } else {
                        regex.push_str(if windows { "(?!\\\\)" } else { "(?!/)" });
                        regex.push('[');
                    }
                    if chars.get(i) == Some(&'-') {
                        regex.push('-');
                        i += 1;
                        has_member = true;
                    }
                }

                let mut has_range_start = false;
                let mut last = '\0';
                let mut closed = false;
                while i < chars.len() {
                    let class_char = chars[i];
                    i += 1;
                    if class_char == ']' {
                        if !has_member {
                            return Err(format!("Empty character class near index {class_start}"));
                        }
                        closed = true;
                        break;
                    }
                    if class_char == '/' || (windows && class_char == '\\') {
                        return Err(format!(
                            "Explicit 'name separator' in class near index {}",
                            i - 1
                        ));
                    }
                    // OpenJDK escapes a doubled '&' so Java's class
                    // intersection cannot reinterpret a literal ampersand.
                    if matches!(class_char, '\\' | '[' | ']')
                        || (class_char == '&' && chars.get(i) == Some(&'&'))
                    {
                        regex.push('\\');
                    }
                    regex.push(class_char);
                    has_member = true;
                    if class_char == '-' {
                        if !has_range_start {
                            return Err(format!("Invalid range near index {}", i - 1));
                        }
                        let Some(&range_end) = chars.get(i) else {
                            return Err(format!("Invalid range near index {}", i - 1));
                        };
                        i += 1;
                        if range_end == ']' {
                            closed = true;
                            break;
                        }
                        if range_end < last {
                            return Err(format!("Invalid range near index {}", i - 3));
                        }
                        // OpenJDK appends range endpoints without applying the
                        // direct-class-member escaping or separator check. Its
                        // generated Java regex is therefore invalid for these
                        // raw endpoints, even when PCRE2 would accept it.
                        if matches!(range_end, '[' | '\\') {
                            return Err(format!("Invalid range endpoint near index {}", i - 1));
                        }
                        regex.push(range_end);
                        has_range_start = false;
                    } else {
                        has_range_start = true;
                        last = class_char;
                    }
                }
                if !closed {
                    return Err(format!("Missing ']' near index {class_start}"));
                }
                regex.push(']');
            }
            '{' => {
                if in_group {
                    return Err(format!("Cannot nest groups near index {}", i - 1));
                }
                regex.push_str("(?:");
                in_group = true;
            }
            '}' if in_group => {
                regex.push(')');
                in_group = false;
            }
            '}' => regex.push('}'),
            ',' if in_group => regex.push('|'),
            ',' => regex.push(','),
            '*' if chars.get(i) == Some(&'*') => {
                regex.push_str(".*");
                i += 1;
            }
            '*' => regex.push_str(if windows { "[^\\\\]*" } else { "[^/]*" }),
            '?' => regex.push_str(if windows { "[^\\\\]" } else { "[^/]" }),
            other => {
                if is_regex_meta(other) || other == '\\' {
                    regex.push('\\');
                }
                regex.push(other);
            }
        }
    }
    if in_group {
        return Err(format!("Missing '}}' near index {}", chars.len() - 1));
    }
    regex.push('$');
    Ok(regex)
}

/// Accept only the Java `Pattern` surface whose PCRE2 behavior this matcher
/// relies on. This is intentionally an allowlist: a new PCRE2 extension cannot
/// silently become authorization syntax. PCRE2 still performs the final
/// structural validation after this compatibility scan.
fn validate_java_regex(pattern: &str) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut groups = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i = scan_java_escape(&chars, i, false, groups.contains(&GroupKind::Lookbehind))?;
            continue;
        }
        match c {
            '[' => i = scan_java_class(&chars, i + 1)?,
            '(' => {
                if groups.contains(&GroupKind::Lookbehind) {
                    return Err("groups inside lookbehind are outside the supported subset".into());
                }
                let (kind, next) = scan_java_group(&chars, i)?;
                groups.push(kind);
                i = next;
            }
            ')' => {
                groups.pop();
                i += 1;
            }
            '*' | '+' | '?' => {
                if groups.contains(&GroupKind::Lookbehind) {
                    return Err("variable-length lookbehind is outside the supported subset".into());
                }
                i += 1;
                if chars
                    .get(i)
                    .is_some_and(|suffix| matches!(suffix, '?' | '+'))
                {
                    i += 1;
                }
            }
            '{' => {
                if groups.contains(&GroupKind::Lookbehind) {
                    return Err("variable-length lookbehind is outside the supported subset".into());
                }
                i = scan_java_quantifier(&chars, i)?;
            }
            '|' if groups.contains(&GroupKind::Lookbehind) => {
                return Err("alternation in lookbehind is outside the supported subset".into());
            }
            '^' | '$' => {
                return Err("line anchors are outside the supported Java-compatible subset".into());
            }
            '}' => return Err("unmatched '}' is outside the supported subset".into()),
            _ => i += 1,
        }
    }
    Ok(())
}

/// Java's dot excludes CR, LF, NEL, line separator, and paragraph separator,
/// but not vertical tab or form feed. No PCRE2 newline mode has exactly that
/// set, so translate unescaped dots and enable DOTALL only for the replacement.
fn java_dots_to_pcre2(pattern: &str) -> String {
    const JAVA_DOT: &str = r"(?s:(?![\n\r\x{85}\x{2028}\x{2029}]).)";
    let chars: Vec<char> = pattern.chars().collect();
    let mut translated = String::with_capacity(pattern.len());
    let mut in_class = false;
    let mut class_has_member = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            translated.push(c);
            i += 1;
            if let Some(&escaped) = chars.get(i) {
                translated.push(escaped);
                if in_class {
                    class_has_member = true;
                }
                i += 1;
            }
            continue;
        }
        match c {
            '[' if !in_class => {
                in_class = true;
                class_has_member = false;
                translated.push(c);
            }
            '^' if in_class && !class_has_member => translated.push(c),
            ']' if in_class && class_has_member => {
                in_class = false;
                translated.push(c);
            }
            '.' if !in_class => translated.push_str(JAVA_DOT),
            _ => {
                translated.push(c);
                if in_class {
                    class_has_member = true;
                }
            }
        }
        i += 1;
    }
    translated
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    Regular,
    Lookbehind,
}

fn scan_java_group(chars: &[char], start: usize) -> Result<(GroupKind, usize), String> {
    if chars.get(start + 1) != Some(&'?') {
        if chars.get(start + 1) == Some(&'*') {
            return Err("group construct is outside the supported Java-compatible subset".into());
        }
        return Ok((GroupKind::Regular, start + 1));
    }
    match (chars.get(start + 2), chars.get(start + 3)) {
        (Some(':' | '=' | '!' | '>'), _) => Ok((GroupKind::Regular, start + 3)),
        (Some('<'), Some('=' | '!')) => Ok((GroupKind::Lookbehind, start + 4)),
        (Some('<'), _) => {
            let mut i = start + 3;
            if !chars.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
                return Err("Java named groups must start with a Latin letter".into());
            }
            i += 1;
            while chars.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
                i += 1;
            }
            if chars.get(i) != Some(&'>') {
                return Err("invalid Java named-group syntax".into());
            }
            Ok((GroupKind::Regular, i + 1))
        }
        _ => Err("group construct is outside the supported Java-compatible subset".into()),
    }
}

fn scan_java_class(chars: &[char], mut i: usize) -> Result<usize, String> {
    if chars.get(i) == Some(&'^') {
        i += 1;
    }
    if chars.get(i) == Some(&']') {
        i += 1;
    }
    while let Some(&c) = chars.get(i) {
        match c {
            ']' => return Ok(i + 1),
            '[' => {
                return Err(
                    "nested Java character classes are outside the supported subset".into(),
                );
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                return Err(
                    "Java character-class intersection is outside the supported subset".into(),
                );
            }
            '\\' => i = scan_java_escape(chars, i, true, false)?,
            _ => i += 1,
        }
    }
    Ok(i) // PCRE2 reports the unclosed class.
}

fn scan_java_escape(
    chars: &[char],
    start: usize,
    in_class: bool,
    in_lookbehind: bool,
) -> Result<usize, String> {
    let Some(&escaped) = chars.get(start + 1) else {
        return Ok(start + 1); // PCRE2 reports the trailing slash.
    };
    if escaped.is_ascii_digit() {
        if in_class
            || in_lookbehind
            || escaped == '0'
            || chars.get(start + 2).is_some_and(char::is_ascii_digit)
        {
            return Err("only unambiguous single-digit Java backreferences are supported".into());
        }
        return Ok(start + 2);
    }
    if escaped == 'k' && !in_class {
        if in_lookbehind {
            return Err("backreferences in lookbehind are outside the supported subset".into());
        }
        let mut i = start + 2;
        if chars.get(i) != Some(&'<') {
            return Err("only Java's \\k<name> named-backreference syntax is supported".into());
        }
        i += 1;
        if !chars.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
            return Err("invalid Java named backreference".into());
        }
        i += 1;
        while chars.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
            i += 1;
        }
        if chars.get(i) != Some(&'>') {
            return Err("invalid Java named backreference".into());
        }
        return Ok(i + 1);
    }
    if escaped == 'x' {
        let end = start + 4;
        if chars
            .get(start + 2..end)
            .is_none_or(|digits| digits.len() != 2 || !digits.iter().all(char::is_ascii_hexdigit))
        {
            return Err("only Java's two-digit \\xhh escape is supported".into());
        }
        return Ok(end);
    }
    let allowed_alpha = if in_class {
        "tnrfaedDsSwW"
    } else {
        "tnrfaedDsSwWAz"
    };
    if escaped.is_alphanumeric()
        && !(escaped.is_ascii_alphabetic() && allowed_alpha.contains(escaped))
    {
        return Err(format!(
            "Java alphabetic escape \\{escaped} is outside the supported subset"
        ));
    }
    Ok(start + 2)
}

fn scan_java_quantifier(chars: &[char], start: usize) -> Result<usize, String> {
    let mut i = start + 1;
    let lower_start = i;
    while chars.get(i).is_some_and(char::is_ascii_digit) {
        i += 1;
    }
    if i == lower_start {
        return Err("Java counted quantifiers require a lower bound".into());
    }
    if chars.get(i) == Some(&',') {
        i += 1;
        while chars.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
    }
    if chars.get(i) != Some(&'}') {
        return Err("invalid Java counted quantifier".into());
    }
    i += 1;
    if chars
        .get(i)
        .is_some_and(|suffix| matches!(suffix, '?' | '+'))
    {
        i += 1;
    }
    Ok(i)
}

fn is_regex_meta(c: char) -> bool {
    ".^$+{[]|()".contains(c)
}

fn is_glob_meta(c: char) -> bool {
    "\\*?[{".contains(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parsing_matches_paper_including_java_blank_rules() {
        assert!(ConfigEntry::parse("").unwrap().is_none());
        assert!(ConfigEntry::parse("\u{2003}").unwrap().is_none());
        assert!(ConfigEntry::parse("#comment").unwrap().is_none());
        assert!(ConfigEntry::parse("\u{00A0}").unwrap().is_some());
        assert_eq!(
            ConfigEntry::parse("[glob]**/*.mca").unwrap().unwrap(),
            ConfigEntry::glob("**/*.mca")
        );
        assert_eq!(
            ConfigEntry::parse("[regex].*/region/.*").unwrap().unwrap(),
            ConfigEntry::regex(".*/region/.*")
        );
        assert_eq!(
            ConfigEntry::parse("[prefix]/srv/world").unwrap().unwrap(),
            ConfigEntry::prefix("/srv/world")
        );
        assert_eq!(
            ConfigEntry::parse("[glob").unwrap_err(),
            "Unterminated type in line '[glob'"
        );
        assert_eq!(
            ConfigEntry::parse("[other]x").unwrap_err(),
            "Unsupported definition type in line '[other]x'"
        );
    }

    #[test]
    fn read_plain_preserves_entries_and_ignores_comments() {
        let list = PathAllowList::read_plain(Cursor::new(
            "# targets\n[glob]/srv/*/world\n[prefix]/opt/world\n",
        ))
        .unwrap();
        assert!(list.matches(Path::new("/srv/alice/world")));
        assert!(list.matches(Path::new("/opt/world/data")));
        assert!(!list.matches(Path::new("/srv/alice/world/data")));
    }

    #[test]
    fn prefix_is_raw_string_prefix_not_component_boundary() {
        let list = PathAllowList::new(vec![ConfigEntry::prefix("/data/world")]);
        assert!(list.matches(Path::new("/data/world")));
        assert!(list.matches(Path::new("/data/world/region")));
        assert!(list.matches(Path::new("/data/worldly")));
        assert!(!list.matches(Path::new("/data/World")));
    }

    #[test]
    fn glob_matches_openjdk_unix_separator_and_group_rules() {
        let list = PathAllowList::new(vec![ConfigEntry::glob("/srv/{red,blue}/**/*.mca")]);
        assert!(list.matches(Path::new("/srv/red/region/r.0.0.mca")));
        assert!(list.matches(Path::new("/srv/blue/a/b/r.1.2.mca")));
        assert!(!list.matches(Path::new("/srv/green/region/r.0.0.mca")));
        assert!(!list.matches(Path::new("/srv/red/r.0.0.dat")));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_glob_wildcards_and_positive_classes_cannot_consume_separator() {
        let star = PathAllowList::new(vec![ConfigEntry::glob("/srv/*/world")]);
        assert!(star.matches(Path::new("/srv/alice/world")));
        assert!(!star.matches(Path::new("/srv/alice/nested/world")));

        let question = PathAllowList::new(vec![ConfigEntry::glob("/srv/?/world")]);
        assert!(question.matches(Path::new("/srv/a/world")));
        assert!(!question.matches(Path::new("/srv///world")));

        // '/' lies inside the '.'..='0' range even though the class does not
        // spell out a separator. OpenJDK intersects every positive class with
        // the separator's complement.
        let ranged_class = PathAllowList::new(vec![ConfigEntry::glob("/srv/safe[.-0]secret")]);
        assert!(ranged_class.matches(Path::new("/srv/safe.secret")));
        assert!(ranged_class.matches(Path::new("/srv/safe0secret")));
        assert!(!ranged_class.matches(Path::new("/srv/safe/secret")));

        // OpenJDK treats a leading '^' as a literal, not negation, and still
        // intersects the positive class with the separator's complement.
        let caret_class = PathAllowList::new(vec![ConfigEntry::glob("/srv/safe[^.-0]secret")]);
        assert!(caret_class.matches(Path::new("/srv/safe^secret")));
        assert!(caret_class.matches(Path::new("/srv/safe.secret")));
        assert!(!caret_class.matches(Path::new("/srv/safe/secret")));
    }

    #[test]
    fn windows_glob_conversion_is_testable_on_every_build_host() {
        let mut builder = RegexBuilder::new();
        builder.utf(true).ucp(false).caseless(true);
        for (glob, matching_path) in [
            ("C:/safe[Z-^]secret", &br"C:\safe[secret"[..]),
            ("C:/safe[^[-^]secret", &br"C:\safe[secret"[..]),
        ] {
            let expression = glob_to_regex(glob, true).unwrap();
            let expression = format!(r"(*ANY)\A(?:{expression})\z");
            let regex = builder.build(&expression).unwrap();

            assert!(regex.is_match(matching_path).unwrap());
            assert!(!regex.is_match(br"C:\safe\secret").unwrap());
        }
    }

    #[test]
    fn glob_class_separator_quirks_match_openjdk_on_every_build_host() {
        for windows in [false, true] {
            assert!(glob_to_regex("[a/b]", windows).is_err());
            assert!(glob_to_regex("[.-/]", windows).is_ok());
            assert!(glob_to_regex("[.-0]", windows).is_ok());
            assert!(glob_to_regex("[Z-^]", windows).is_ok());

            // OpenJDK bypasses the direct-member separator check for a range
            // endpoint, then emits '[' or '\\' verbatim. Java Pattern rejects
            // both generated expressions, so reject them before PCRE2 gets a
            // chance to accept a different language.
            assert!(glob_to_regex("[Z-[]", windows).is_err());
            assert!(glob_to_regex(r"[Z-\]", windows).is_err());

            // ']' closes the range/class in OpenJDK rather than becoming a
            // raw endpoint, while '/' remains a valid raw endpoint.
            assert!(glob_to_regex("[Z-]]", windows).is_ok());
            assert!(glob_to_regex("[.-/]", windows).is_ok());
        }

        assert!(glob_to_regex(r"[a\b]", false).is_ok());
        assert!(glob_to_regex(r"[a\b]", true).is_err());
        assert!(glob_to_regex(r"[\]", false).is_ok());
        assert!(glob_to_regex(r"[\]", true).is_err());
    }

    #[test]
    fn glob_class_doubled_ampersand_is_a_literal_member_like_openjdk() {
        // OpenJDK escapes a doubled '&' inside a glob class, so it is a
        // literal member; without the escape PCRE2 would treat it as class
        // intersection and match nothing.
        for windows in [false, true] {
            assert!(glob_to_regex("[a&&b]", windows).is_ok());
            let expression = glob_to_regex("[a&&b]", windows).unwrap();
            let expression = format!(r"\A(?:{expression})\z");
            let mut builder = RegexBuilder::new();
            builder.utf(true).ucp(false);
            let regex = builder.build(&expression).unwrap();
            for matching in [b"a".as_slice(), b"&".as_slice(), b"b".as_slice()] {
                assert!(
                    regex.is_match(matching).unwrap(),
                    "[a&&b] must match literal '&' member for windows={windows}"
                );
            }
            assert!(!regex.is_match(b"c").unwrap());
        }

        let list = PathAllowList::new(vec![ConfigEntry::glob("[a&&b]")]);
        assert!(list.matches(Path::new("a")));
        assert!(list.matches(Path::new("&")));
        assert!(list.matches(Path::new("b")));
        assert!(!list.matches(Path::new("c")));
    }

    #[test]
    fn memberless_glob_classes_are_rejected_on_unix_and_windows() {
        for windows in [false, true] {
            for glob in ["[!]", "[]*]", "[]?]", "[][]"] {
                assert!(
                    glob_to_regex(glob, windows).is_err(),
                    "memberless class {glob:?} must fail for windows={windows}"
                );
            }

            // A leading caret is a literal class member in OpenJDK globs;
            // leading hyphens, negated members, and ranges remain valid.
            for glob in ["[^]", "[-]", "[!a]", "[a-z]"] {
                assert!(
                    glob_to_regex(glob, windows).is_ok(),
                    "valid class {glob:?} must pass for windows={windows}"
                );
            }
        }
    }

    #[test]
    fn memberless_glob_class_disables_the_whole_allow_list() {
        for invalid in ["[!]", "[]*]", "[]?]", "[][]"] {
            let list = PathAllowList::new(vec![
                ConfigEntry::prefix("otherwise-matching"),
                ConfigEntry::glob(invalid),
            ]);
            assert!(
                !list.matches(Path::new("otherwise-matching")),
                "invalid glob {invalid:?} left the prefix active"
            );
        }
    }

    #[test]
    fn range_end_backslash_regex_failure_disables_the_whole_allow_list() {
        let list = PathAllowList::new(vec![
            ConfigEntry::prefix("otherwise-matching"),
            ConfigEntry::glob(r"[Z-\]"),
        ]);
        assert!(!list.matches(Path::new("otherwise-matching")));
    }

    #[test]
    fn java_pattern_invalid_range_endpoint_disables_the_whole_allow_list() {
        let list =
            PathAllowList::read_plain(Cursor::new("[prefix]otherwise-matching\n[glob][Z-[]\n"))
                .unwrap();

        assert!(!list.matches(Path::new("otherwise-matching")));
        assert!(!list.matches(Path::new("Z")));
        assert!(!list.matches(Path::new("[")));
    }

    #[test]
    fn printable_ascii_range_endpoints_follow_openjdk_compile_results() {
        for windows in [false, true] {
            for endpoint in ' '..='~' {
                let glob = format!("[ -{endpoint}]");
                let result = glob_to_regex(&glob, windows);
                if matches!(endpoint, '[' | '\\') {
                    assert!(result.is_err(), "{glob:?} must fail for windows={windows}");
                } else {
                    assert!(result.is_ok(), "{glob:?} must pass for windows={windows}");
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_globs_use_backslash_and_never_consume_it() {
        let star = PathAllowList::new(vec![ConfigEntry::glob("C:/*/world")]);
        assert!(star.matches(Path::new(r"C:\alice\world")));
        assert!(!star.matches(Path::new(r"C:\alice\nested\world")));

        let question = PathAllowList::new(vec![ConfigEntry::glob("C:/?/world")]);
        assert!(question.matches(Path::new(r"C:\a\world")));
        assert!(!question.matches(Path::new(r"C:\\\world")));

        // '\\' lies inside the 'Z'..='^' range. The positive
        // class still may not consume Windows' native separator.
        let ranged_class = PathAllowList::new(vec![ConfigEntry::glob("C:/safe[Z-^]secret")]);
        assert!(ranged_class.matches(Path::new(r"C:\safe[secret")));
        assert!(!ranged_class.matches(Path::new(r"C:\safe\secret")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_explicit_backslash_class_disables_the_whole_allow_list() {
        let list = PathAllowList::new(vec![
            ConfigEntry::prefix("otherwise-matching"),
            ConfigEntry::glob(r"C:/safe[\]secret"),
        ]);
        assert!(!list.matches(Path::new("otherwise-matching")));
        assert!(!list.matches(Path::new(r"C:\safe\secret")));
    }

    #[test]
    fn regex_uses_full_string_matches() {
        let list = PathAllowList::new(vec![ConfigEntry::regex("/srv/.+/world")]);
        assert!(list.matches(Path::new("/srv/alice/world")));
        assert!(!list.matches(Path::new("prefix/srv/alice/world")));
        assert!(!list.matches(Path::new("/srv/alice/world/suffix")));

        // The engine must evaluate alternatives under the full-match
        // constraint rather than accepting the first partial alternative.
        let alternatives = PathAllowList::new(vec![ConfigEntry::regex("/srv|/srv/world")]);
        assert!(alternatives.matches(Path::new("/srv/world")));
    }

    #[test]
    fn regex_dot_uses_java_line_terminators() {
        let list = PathAllowList::new(vec![ConfigEntry::regex("/srv/.*")]);
        assert!(list.matches(Path::new("/srv/alice")));
        assert!(!list.matches(Path::new("/srv/alice\rbob")));
        assert!(!list.matches(Path::new("/srv/alice\u{2028}bob")));
        assert!(list.matches(Path::new("/srv/alice\u{000b}bob")));
        assert!(list.matches(Path::new("/srv/alice\u{000c}bob")));
    }

    #[test]
    fn regex_supports_java_lookarounds() {
        let lookahead = PathAllowList::new(vec![ConfigEntry::regex(
            r"(?=/srv/approved/)(?!.*forbidden).*/world",
        )]);
        assert!(lookahead.matches(Path::new("/srv/approved/alice/world")));
        assert!(!lookahead.matches(Path::new("/srv/approved/forbidden/world")));

        let lookbehind =
            PathAllowList::new(vec![ConfigEntry::regex(r"/srv/[a-z]+(?<=alice)/world")]);
        assert!(lookbehind.matches(Path::new("/srv/alice/world")));
        assert!(!lookbehind.matches(Path::new("/srv/bob/world")));
    }

    #[test]
    fn regex_supports_java_numeric_and_named_backreferences() {
        let numeric = PathAllowList::new(vec![ConfigEntry::regex(r"/srv/([a-z]+)/\1")]);
        assert!(numeric.matches(Path::new("/srv/alice/alice")));
        assert!(!numeric.matches(Path::new("/srv/alice/bob")));

        let named = PathAllowList::new(vec![ConfigEntry::regex(r"/srv/(?<name>[a-z]+)/\k<name>")]);
        assert!(named.matches(Path::new("/srv/alice/alice")));
        assert!(!named.matches(Path::new("/srv/alice/bob")));
    }

    #[test]
    fn regex_predefined_classes_are_ascii_by_default_like_java() {
        let digit = PathAllowList::new(vec![ConfigEntry::regex(r"/srv/\d+")]);
        assert!(digit.matches(Path::new("/srv/123")));
        assert!(!digit.matches(Path::new("/srv/١٢٣")));

        let word = PathAllowList::new(vec![ConfigEntry::regex(r"/srv/\w+")]);
        assert!(word.matches(Path::new("/srv/alice_123")));
        assert!(!word.matches(Path::new("/srv/élise")));

        let whitespace = PathAllowList::new(vec![ConfigEntry::regex("/srv/\\s")]);
        assert!(whitespace.matches(Path::new("/srv/\t")));
        assert!(!whitespace.matches(Path::new("/srv/\u{00a0}")));
    }

    #[test]
    fn regex_accepts_only_the_supported_java_pattern_subset() {
        for pattern in [
            r"/srv/([a-z]+)/\1",
            r"/srv/(?<name>[a-z]+)/\k<name>",
            r"(?=/srv/)(?!.*forbidden).+",
            r"/srv/[a-z]+(?<=alice)",
            r"/srv/a{1,3}+",
            r"/srv/\x61+",
        ] {
            ConfigEntry::regex(pattern).compile().unwrap();
        }
        assert!(
            PathAllowList::new(vec![ConfigEntry::regex(r"/srv/a{1,3}+")])
                .matches(Path::new("/srv/aaa"))
        );
        assert!(
            PathAllowList::new(vec![ConfigEntry::regex(r"/srv/\x61+")])
                .matches(Path::new("/srv/aaa"))
        );

        for pattern in [
            r"(?#comment)",
            r"a{,3}",
            r"(?i:a)",
            r"(?R)",
            r"(?1)",
            r"(?(1)a|b)",
            r"(?|a|b)",
            r"(?P<name>a)",
            r"(?'name'a)",
            r"(*ACCEPT)",
            r"\C",
            r"\K",
            r"\g{name}",
            r"\o{141}",
            r"\R",
            r"\X",
            r"\Qquoted\E",
            r"\bword\b",
            r"[[:alpha:]]",
            r"[a-z&&[^m-p]]",
            r"\p{javaLowerCase}",
            r"a^b",
            r"a{1, 3}",
            r"(a)\10",
            r"(?<=a+)b",
        ] {
            assert!(
                ConfigEntry::regex(pattern).compile().is_err(),
                "unexpectedly accepted {pattern}"
            );
        }
    }

    #[test]
    fn regex_rejects_backspace_escapes_inside_character_classes() {
        for pattern in [r"[\b]", r"[A\b]", r"[\b-A]"] {
            assert!(
                ConfigEntry::regex(pattern).compile().is_err(),
                "Java-invalid in-class backspace escape was accepted: {pattern}"
            );
        }
    }

    #[test]
    fn in_class_backspace_escape_disables_the_whole_allow_list() {
        for invalid in [r"[\b]", r"[A\b]", r"[\b-A]"] {
            let list = PathAllowList::new(vec![
                ConfigEntry::prefix("otherwise-matching"),
                ConfigEntry::regex(invalid),
            ]);
            assert!(
                !list.matches(Path::new("otherwise-matching")),
                "invalid regex {invalid:?} left the prefix active"
            );
        }
    }

    #[test]
    fn regex_rejections_disable_the_whole_allow_list() {
        for invalid in [r"/srv/ok(?#comment)", r"/srv/a{,3}"] {
            let list = PathAllowList::new(vec![
                ConfigEntry::prefix("/otherwise/allowed"),
                ConfigEntry::regex(invalid),
            ]);
            assert!(!list.matches(Path::new("/otherwise/allowed")));
            assert!(!list.matches(Path::new("/srv/ok")));
            assert!(!list.matches(Path::new("/srv/")));
            assert!(!list.matches(Path::new("/srv/a")));
        }
    }

    #[test]
    fn regex_rejects_java_syntax_with_different_pcre2_meaning() {
        assert!(
            ConfigEntry::regex(r"(?U)\w+")
                .compile()
                .unwrap_err()
                .contains("group construct")
        );
        assert!(
            ConfigEntry::regex(r"[a-z&&[^m-p]]+")
                .compile()
                .unwrap_err()
                .contains("character-class intersection")
        );
        assert!(
            ConfigEntry::regex(r"\p{javaLowerCase}+")
                .compile()
                .unwrap_err()
                .contains("outside the supported subset")
        );
        assert!(
            ConfigEntry::regex(r"(*ACCEPT)")
                .compile()
                .unwrap_err()
                .contains("group construct")
        );
        assert!(
            ConfigEntry::regex(r"\C+")
                .compile()
                .unwrap_err()
                .contains("outside the supported subset")
        );
    }

    #[test]
    fn one_invalid_pattern_disables_whole_list() {
        let list = PathAllowList::new(vec![
            ConfigEntry::prefix("/otherwise/allowed"),
            ConfigEntry::glob("[z-a]"),
        ]);
        assert!(!list.matches(Path::new("/otherwise/allowed")));
        assert!(!list.matches(Path::new("anything")));
    }

    #[test]
    fn raw_path_text_is_not_lexically_normalized() {
        let list = PathAllowList::new(vec![ConfigEntry::prefix("safe/../outside")]);
        assert!(list.matches(Path::new("safe/../outside/file")));
        assert!(!list.matches(Path::new("outside/file")));
    }
}

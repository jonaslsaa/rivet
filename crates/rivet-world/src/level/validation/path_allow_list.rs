//! `net.minecraft.world.level.validation.PathAllowList`.
//!
//! Paper delegates `glob:` and `regex:` entries to the path's `FileSystem`.
//! Rivet currently has one native filesystem, so Paper's per-provider cache is
//! represented by one lazily compiled matcher list. Glob conversion follows
//! OpenJDK's platform-specific `sun.nio.fs.Globs` rules. Regex entries use
//! PCRE2's Java-compatible surface, with incompatible syntax rejected instead
//! of risking a broader authorization match.

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
                let mut builder = RegexBuilder::new();
                builder.utf(true).ucp(false);
                #[cfg(windows)]
                builder.caseless(true);
                // Matcher.matches() constrains the engine while it evaluates
                // alternatives; checking the span of an unconstrained find
                // would incorrectly reject `short|longer` after `short` wins.
                // ANY gives dot/anchors Java's full line-terminator set rather
                // than PCRE2's narrower LF-only default.
                let expression = format!(r"(*ANY)\A(?:{expression})\z");
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
                if chars.get(i) == Some(&'^') {
                    regex.push_str("[\\^");
                    i += 1;
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
                    }
                }

                let mut has_range_start = false;
                let mut last = '\0';
                let mut closed = false;
                while i < chars.len() {
                    let class_char = chars[i];
                    i += 1;
                    if class_char == ']' {
                        closed = true;
                        break;
                    }
                    if class_char == '/' {
                        return Err(format!(
                            "Explicit 'name separator' in class near index {}",
                            i - 1
                        ));
                    }
                    if matches!(class_char, '\\' | '[' | ']') {
                        regex.push('\\');
                    }
                    regex.push(class_char);
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
                        if matches!(range_end, '\\' | '[' | ']') {
                            regex.push('\\');
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

/// Reject Java Pattern syntax that PCRE2 accepts with a different meaning.
/// Unsupported constructs fail compilation, which keeps the entire allow list
/// closed just as a Java `PatternSyntaxException` would. Everything accepted
/// here is then compiled by PCRE2, including lookarounds and backreferences.
fn validate_java_regex(pattern: &str) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut in_class = false;
    let mut quoted = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            let Some(&escaped) = chars.get(i + 1) else {
                break;
            };
            if escaped == 'Q' && !quoted {
                quoted = true;
            } else if escaped == 'E' && quoted {
                quoted = false;
            } else if !quoted && matches!(escaped, 'p' | 'P' | 'N') {
                return Err(format!(
                    "Java regex escape \\{escaped} is unsupported because PCRE2's Unicode property syntax differs"
                ));
            } else if !quoted && matches!(escaped, 'C' | 'K' | 'g' | 'o') {
                return Err(format!(
                    "PCRE2-only regex escape \\{escaped} is not valid Java Pattern syntax"
                ));
            } else if !quoted
                && escaped == 'k'
                && chars.get(i + 2).is_some_and(|delimiter| *delimiter != '<')
            {
                return Err(
                    "only Java's \\k<name> named-backreference syntax is supported".to_string(),
                );
            }
            i += 2;
            continue;
        }
        if quoted {
            i += 1;
            continue;
        }
        if c == '(' && chars.get(i + 1) == Some(&'*') {
            return Err("PCRE2 control verbs are not valid Java Pattern syntax".to_string());
        }
        if c == '[' {
            if in_class {
                return Err(
                    "Java nested/intersection character classes are unsupported".to_string()
                );
            }
            in_class = true;
        } else if c == ']' {
            in_class = false;
        } else if in_class && c == '&' && chars.get(i + 1) == Some(&'&') {
            return Err("Java character-class intersection is unsupported".to_string());
        } else if !in_class && c == '(' && chars.get(i + 1) == Some(&'?') {
            if chars
                .get(i + 2)
                .is_some_and(|marker| matches!(marker, '|' | '(' | '[' | '\''))
            {
                return Err("PCRE2-only group syntax is not valid Java Pattern syntax".to_string());
            }
            let mut j = i + 2;
            if chars.get(j) == Some(&'-') {
                j += 1;
            }
            while let Some(flag) = chars.get(j) {
                if matches!(flag, ':' | ')') {
                    break;
                }
                if !flag.is_ascii_alphabetic() && *flag != '-' {
                    break;
                }
                if matches!(flag, 'd' | 'i' | 'u' | 'U' | 'x') {
                    return Err(format!(
                        "Java embedded regex flag '{flag}' is unsupported because PCRE2's behavior differs"
                    ));
                }
                if !matches!(flag, 'm' | 's' | '-') {
                    return Err(format!(
                        "PCRE2 regex flag or group marker '{flag}' is not valid supported Java Pattern syntax"
                    ));
                }
                j += 1;
            }
        }
        i += 1;
    }
    Ok(())
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
    }

    #[test]
    fn windows_glob_conversion_is_testable_on_every_build_host() {
        let expression = glob_to_regex("C:/safe[!-~]secret", true).unwrap();
        let expression = format!(r"(*ANY)\A(?:{expression})\z");
        let mut builder = RegexBuilder::new();
        builder.utf(true).ucp(false).caseless(true);
        let regex = builder.build(&expression).unwrap();

        assert!(regex.is_match(br"C:\safe.secret").unwrap());
        assert!(regex.is_match(br"c:\SAFE0secret").unwrap());
        assert!(!regex.is_match(br"C:\safe\secret").unwrap());
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

        // '\\' lies inside the broad printable-ASCII range. The positive
        // class still may not consume Windows' native separator.
        let ranged_class = PathAllowList::new(vec![ConfigEntry::glob("C:/safe[!-~]secret")]);
        assert!(ranged_class.matches(Path::new(r"C:\safe.secret")));
        assert!(!ranged_class.matches(Path::new(r"C:\safe\secret")));
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
    fn regex_rejects_java_syntax_with_different_pcre2_meaning() {
        assert!(
            ConfigEntry::regex(r"(?U)\w+")
                .compile()
                .unwrap_err()
                .contains("embedded regex flag 'U'")
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
                .contains("Unicode property syntax differs")
        );
        assert!(
            ConfigEntry::regex(r"(*ACCEPT)")
                .compile()
                .unwrap_err()
                .contains("control verbs")
        );
        assert!(
            ConfigEntry::regex(r"\C+")
                .compile()
                .unwrap_err()
                .contains("not valid Java Pattern syntax")
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

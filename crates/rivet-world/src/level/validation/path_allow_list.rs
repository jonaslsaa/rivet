//! `net.minecraft.world.level.validation.PathAllowList`.
//!
//! Paper delegates `glob:` and `regex:` entries to the path's `FileSystem`.
//! Rivet currently has one native filesystem, so Paper's per-provider cache is
//! represented by one lazily compiled matcher list. The Unix glob conversion
//! follows OpenJDK's `sun.nio.fs.Globs.toUnixRegexPattern`.

use std::io::{self, BufRead};
use std::path::Path;
use std::sync::OnceLock;

use regex_automata::{Anchored, Input, meta::Regex};

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
                    glob_to_unix_regex(pattern)?
                } else if syntax.eq_ignore_ascii_case("regex") {
                    pattern.to_string()
                } else {
                    return Err(format!("Syntax '{syntax}' not recognized"));
                };
                Regex::new(&expression)
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
            CompiledMatcher::Regex(regex) => regex
                .find(Input::new(path.as_bytes()).anchored(Anchored::Yes))
                .is_some_and(|matched| matched.end() == path.len()),
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

/// OpenJDK's Unix glob conversion, expressed in regex-automata's equivalent
/// character-class syntax.
fn glob_to_unix_regex(glob: &str) -> Result<String, String> {
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
                        regex.push_str("[^/");
                    } else {
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
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
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

    #[test]
    fn regex_uses_full_string_matches() {
        let list = PathAllowList::new(vec![ConfigEntry::regex("/srv/.+/world")]);
        assert!(list.matches(Path::new("/srv/alice/world")));
        assert!(!list.matches(Path::new("prefix/srv/alice/world")));
        assert!(!list.matches(Path::new("/srv/alice/world/suffix")));
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

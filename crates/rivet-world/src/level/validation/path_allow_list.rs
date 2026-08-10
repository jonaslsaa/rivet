//! `net.minecraft.world.level.validation.PathAllowList` — a parsed
//! `allowed_symlinks.txt` allow list that decides whether a symlink target
//! path is permitted.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/validation/PathAllowList.java`. It implements Java's
//! `PathMatcher`: `matches(Path)` is true when at least one compiled entry
//! matches the path.
//!
//! # Port notes
//!
//! Java's matcher abstraction keys compiled matchers off the `FileSystem`s
//! provider *scheme*, and two of its three entry types are JVM glob and regex
//! syntax. Rivet's `Path` has one implicit filesystem, so the per-scheme cache
//! collapses to a single matcher; the workspace has no `regex`/`glob` crate
//! (CRATES.md records external deps, and adding one is a deliberate
//! workspace-level decision), so only the [`EntryType::Prefix`] type ports
//! here today. RivetTodo(#323): `EntryType::Filesystem` ("glob:" and "regex:"
//! lines) defers until a regex engine lands in the workspace; the allow-list
//! files Paper ships for normal worlds use prefix lines, so the practical
//! validation surface is complete.

use std::path::Path;

/// `PathAllowList` — an ordered list of allow-list entries.
///
/// Java matches across *all* compiled matchers (any single match permits the
/// path), so Rust evaluates each entry in declaration order and returns true
/// on the first match.
#[derive(Debug, Clone)]
pub struct PathAllowList {
    entries: Vec<ConfigEntry>,
}

impl PathAllowList {
    /// `new PathAllowList(List<ConfigEntry>)` — the constructor.
    pub fn new(entries: Vec<ConfigEntry>) -> Self {
        PathAllowList { entries }
    }

    /// `PathAllowList.matches(Path)` — true if any entry matches the path.
    pub fn matches(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e.matches(path))
    }
}

/// `PathAllowList.ConfigEntry` — a single parsed allow-list line.
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    ty: EntryType,
    pattern: String,
}

impl ConfigEntry {
    /// `ConfigEntry.compile(FileSystem)` — the compiled matcher for this entry.
    pub fn matches(&self, path: &Path) -> bool {
        self.ty.matches(path, &self.pattern)
    }

    /// `ConfigEntry.parse(String)` — pares a single line into an entry, or
    /// `None` for blank/comment lines.
    ///
    /// Faithful to Java:
    /// - blank lines and lines starting with `#` yield nothing;
    /// - a line not starting with `[` is a `PREFIX` entry of the whole line;
    /// - a `[type]contents` line dispatches on `type`; `glob`/`regex` map to
    ///   the `FILESYSTEM` type as `"type:contents"`, `prefix` maps to `PREFIX`.
    ///   Unknown or unterminated types throw.
    pub fn parse(definition: &str) -> Result<Option<ConfigEntry>, String> {
        if definition.is_empty() || definition.trim().is_empty() || definition.starts_with('#') {
            return Ok(None);
        }

        if !definition.starts_with('[') {
            return Ok(Some(ConfigEntry {
                ty: EntryType::Prefix,
                pattern: definition.to_string(),
            }));
        }

        let split = definition.find(']').map(|i| i + 1);
        let split = match split {
            Some(i) => i,
            None => {
                return Err(format!(
                    "Unterminated type in line '{}'",
                    definition
                ));
            }
        };

        let ty = &definition[1..split - 1];
        let contents = &definition[split..];

        match ty {
            "glob" | "regex" => Ok(Some(ConfigEntry {
                ty: EntryType::Filesystem,
                pattern: format!("{}:{}", ty, contents),
            })),
            "prefix" => Ok(Some(ConfigEntry {
                ty: EntryType::Prefix,
                pattern: contents.to_string(),
            })),
            _ => Err(format!(
                "Unsupported definition type in line '{}'",
                definition
            )),
        }
    }
}

/// `PathAllowList.EntryType` — how a pattern decides a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    /// `EntryType.FILESYSTEM` — a JVM glob or regex pattern. Java resolves it
    /// via `FileSystem::getPathMatcher`; deferred (see module doc).
    Filesystem,
    /// `EntryType.PREFIX` — true when the path string starts with the pattern.
    Prefix,
}

impl EntryType {
    fn matches(&self, path: &Path, pattern: &str) -> bool {
        match self {
            EntryType::Filesystem => {
                // RivetTodo(#323): glob/regex matching defers with a workspace
                // regex dependency. Never matches so a partially-ported list
                // cannot accidentally permit a path it should not.
                let _ = (path, pattern);
                false
            }
            EntryType::Prefix => {
                path.to_string_lossy().starts_with(pattern)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_and_comment_lines_are_ignored() {
        assert!(ConfigEntry::parse("").unwrap().is_none());
        assert!(ConfigEntry::parse("   ").unwrap().is_none());
        assert!(ConfigEntry::parse("# a comment").unwrap().is_none());
    }

    #[test]
    fn bare_line_is_a_prefix_entry() {
        let e = ConfigEntry::parse("/data/world").unwrap().unwrap();
        assert_eq!(e.ty, EntryType::Prefix);
        assert!(e.matches(Path::new("/data/world")));
        assert!(e.matches(Path::new("/data/world/region/r.0.0.mca")));
        assert!(!e.matches(Path::new("/data/worldly")));
    }

    #[test]
    fn prefix_type_strips_brackets() {
        let e = ConfigEntry::parse("[prefix]/data/world").unwrap().unwrap();
        assert_eq!(e.ty, EntryType::Prefix);
        assert!(e.matches(Path::new("/data/world")));
    }

    #[test]
    fn glob_and_regex_map_to_filesystem() {
        let g = ConfigEntry::parse("[glob]**/*.mca").unwrap().unwrap();
        assert_eq!(g.ty, EntryType::Filesystem);
        let r = ConfigEntry::parse("[regex]\\.*").unwrap().unwrap();
        assert_eq!(r.ty, EntryType::Filesystem);
    }

    #[test]
    fn unterminated_and_unknown_types_throw() {
        assert!(ConfigEntry::parse("[glob").is_err());
        assert!(ConfigEntry::parse("[bogus]x").is_err());
    }

    #[test]
    fn allow_list_any_match_permits() {
        let list = PathAllowList::new(vec![
            ConfigEntry::parse("/data/public").unwrap().unwrap(),
        ]);
        assert!(list.matches(Path::new("/data/public")));
        assert!(!list.matches(Path::new("/etc/passwd")));
    }
}

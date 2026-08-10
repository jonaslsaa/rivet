//! `net.minecraft.world.level.validation.ContentValidationException` — the
//! exception raised when a directory contains forbidden symlinks.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/validation/ContentValidationException.java`. Java throws it as
//! an `Exception` (checked); Rust models it as a lightweight value with a
//! [`std::fmt::Display`] message matching Java's `getMessage()` exactly.

use std::fmt;
use std::path::Path;

use super::ForbiddenSymlinkInfo;

/// `ContentValidationException` — carries the validated directory and the
/// symlinks found there that failed the allow list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentValidationException {
    directory: std::path::PathBuf,
    entries: Vec<ForbiddenSymlinkInfo>,
}

impl ContentValidationException {
    /// `new ContentValidationException(Path directory, List<ForbiddenSymlinkInfo> entries)`.
    pub fn new(directory: std::path::PathBuf, entries: Vec<ForbiddenSymlinkInfo>) -> Self {
        ContentValidationException { directory, entries }
    }

    /// `ContentValidationException.getMessage(Path, List)` — the static
    /// message formatter, also used by `getMessage()`. Preserves Java's exact
    /// formatting: `"Failed to validate '<dir>'. Found forbidden symlinks:
    /// <link>-><target>, <link>-><target>"` joined with `", "`.
    pub fn format(directory: &Path, entries: &[ForbiddenSymlinkInfo]) -> String {
        let joined = entries
            .iter()
            .map(|e| format!("{}->{}", e.link().display(), e.target().display()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Failed to validate '{}'. Found forbidden symlinks: {}",
            directory.display(),
            joined
        )
    }
}

/// `ContentValidationException.getMessage()` — `getMessage(directory, entries)`.
impl fmt::Display for ContentValidationException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::format(&self.directory, &self.entries))
    }
}

impl std::error::Error for ContentValidationException {}

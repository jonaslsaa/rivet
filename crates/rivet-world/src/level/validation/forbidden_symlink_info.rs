//! `net.minecraft.world.level.validation.ForbiddenSymlinkInfo` — a symlink
//! flagged as disallowed by a [`crate::level::validation::DirectoryValidator`].
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/validation/ForbiddenSymlinkInfo.java`, a two-field `record
//! ForbiddenSymlinkInfo(Path link, Path target)`.

use std::path::PathBuf;

/// `ForbiddenSymlinkInfo` — a `(link, target)` pair for a symlink that did not
/// pass the allow list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForbiddenSymlinkInfo {
    link: PathBuf,
    target: PathBuf,
}

impl ForbiddenSymlinkInfo {
    /// `new ForbiddenSymlinkInfo(Path link, Path target)` — the record
    /// constructor.
    pub fn new(link: PathBuf, target: PathBuf) -> Self {
        ForbiddenSymlinkInfo { link, target }
    }

    /// `ForbiddenSymlinkInfo.link()`.
    pub fn link(&self) -> &PathBuf {
        &self.link
    }

    /// `ForbiddenSymlinkInfo.target()`.
    pub fn target(&self) -> &PathBuf {
        &self.target
    }
}

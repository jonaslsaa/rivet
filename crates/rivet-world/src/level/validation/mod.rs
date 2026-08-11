//! `net.minecraft.world.level.validation` — path/symlink validation primitives
//! (issue #340, the read-only world-loading slice).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/validation/`. This is the smallest coherent slice needed to
//! validate a copied world directory before opening it: an allow list of
//! permitted symlink targets, a directory walker that flags disallowed links,
//! and the exception/record that report them.
//!
//! - `forbidden_symlink_info` — `ForbiddenSymlinkInfo`, the `(link, target)`
//!   record for one flagged symlink.
//! - `path_allow_list` — `PathAllowList`, the parsed `allowed_symlinks.txt`
//!   allow list (Java's `PathMatcher`).
//! - `directory_validator` — `DirectoryValidator`, the `walkFileTree`-driven
//!   validation pass over a world directory.
//! - `content_validation_exception` — `ContentValidationException`, thrown
//!   when a validated directory contains forbidden symlinks.
//!
//! Matching preserves Paper's entry order and OpenJDK Unix path-matcher
//! behavior; walking never follows a nested symbolic link.

mod content_validation_exception;
mod directory_validator;
mod forbidden_symlink_info;
mod path_allow_list;

pub use content_validation_exception::ContentValidationException;
pub use directory_validator::DirectoryValidator;
pub use forbidden_symlink_info::ForbiddenSymlinkInfo;
pub use path_allow_list::{ConfigEntry, EntryType, PathAllowList};

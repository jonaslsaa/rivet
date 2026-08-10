//! `net.minecraft.world.level.validation.DirectoryValidator` — validates a
//! directory tree's symlinks against an allow list.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/validation/DirectoryValidator.java`. Java's `Files.walkFileTree`
//! with a `SimpleFileVisitor` visits each directory and file exactly once; the
//! Rust port recurses with `std::fs::read_dir` in pre-order, which visits the
//! same entries in the same relative order (directory first, then each child).
//!
//! Fidelity notes (mapped in `validate_directory` below):
//! - Java `Files.readAttributes(path, BasicFileAttributes, NOFOLLOW_LINKS)`
//!   returns the attributes of the link itself, so a symlink appears as
//!   `isSymbolicLink()` and the regular-file check (`isRegularFile()`) is
//!   false for a symlink. `std::fs::symlink_metadata` is the equivalent
//!   no-follow read.
//! - Java `NoSuchFileException` on the top path is swallowed (returns an empty
//!   issue list); other read errors, and "path is a regular file", throw.

use std::io;
use std::path::Path;

use super::ForbiddenSymlinkInfo;
use super::PathAllowList;

/// `DirectoryValidator` — validates symlinks under a directory against a
/// [`PathAllowList`].
pub struct DirectoryValidator {
    symlink_target_allow_list: PathAllowList,
}

impl DirectoryValidator {
    /// `new DirectoryValidator(PathMatcher)` — the constructor.
    pub fn new(symlink_target_allow_list: PathAllowList) -> Self {
        DirectoryValidator {
            symlink_target_allow_list,
        }
    }

    /// `validateSymlink(Path, List)` — reads one symbolic link and, if its
    /// target is not allow-listed, appends a [`ForbiddenSymlinkInfo`].
    pub fn validate_symlink(
        &self,
        path: &Path,
        issues: &mut Vec<ForbiddenSymlinkInfo>,
    ) -> io::Result<()> {
        let target = std::fs::read_link(path)?;
        if !self.symlink_target_allow_list.matches(&target) {
            issues.push(ForbiddenSymlinkInfo::new(path.to_path_buf(), target));
        }
        Ok(())
    }

    /// `validateSymlink(Path)` — the variadic convenience form.
    pub fn validate_symlink_owned(&self, path: &Path) -> io::Result<Vec<ForbiddenSymlinkInfo>> {
        let mut result = Vec::new();
        self.validate_symlink(path, &mut result)?;
        Ok(result)
    }

    /// `validateDirectory(Path, boolean)`.
    ///
    /// - A top path that does not exist (`NoSuchFileException`) yields no
    ///   issues.
    /// - A top path that is a regular file throws (`"Path <dir> is not a
    ///   directory"`).
    /// - A top symlink is validated (and not descended into) unless
    ///   `allow_top_symlink` is true, in which case the walk descends through
    ///   the link's target.
    /// - Otherwise the whole known directory tree is walked, validating every
    ///   symlink found within.
    pub fn validate_directory(
        &self,
        directory: &Path,
        allow_top_symlink: bool,
    ) -> io::Result<Vec<ForbiddenSymlinkInfo>> {
        let mut issues = Vec::new();

        let top_is_symlink = match std::fs::symlink_metadata(directory) {
            Ok(meta) => meta.file_type().is_symlink(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(issues),
            Err(e) => return Err(e),
        };

        if !top_is_symlink && directory.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Path {} is not a directory", directory.display()),
            ));
        }

        if top_is_symlink {
            if !allow_top_symlink {
                self.validate_symlink(directory, &mut issues)?;
                return Ok(issues);
            }

            let target = std::fs::read_link(directory)?;
            self.validate_known_directory(&target, &mut issues)?;
            return Ok(issues);
        }

        self.validate_known_directory(directory, &mut issues)?;
        Ok(issues)
    }

    /// `validateKnownDirectory(Path, List)` — walks the directory tree,
    /// validating every symlink (directory or file) encountered.
    pub fn validate_known_directory(
        &self,
        directory: &Path,
        issues: &mut Vec<ForbiddenSymlinkInfo>,
    ) -> io::Result<()> {
        // `Files.walkFileTree(root, visitor)` visits root itself via
        // `preVisitDirectory` (validating it if it is a symlink), then each
        // child in pre-order. A recursive `read_dir` walk visits the same set
        // in the same relative order: the root first, then each child, with a
        // symlink child validated but never descended into (the follow is
        // never entered) — matching Java, which only validates a symlink
        // directory and never visits its contents.
        self.walk_directory(directory, issues, true)
    }

    fn walk_directory(
        &self,
        dir: &Path,
        issues: &mut Vec<ForbiddenSymlinkInfo>,
        is_root: bool,
    ) -> io::Result<()> {
        if is_root {
            // `preVisitDirectory(root, attrs)` — the walk target itself. Java
            // reads the root's no-follow attributes here; the only reachable
            // case where this is a symlink is a top symlink resolved through
            // `allow_top_symlink=true` whose target is itself a symlink.
            let meta = std::fs::symlink_metadata(dir)?;
            if meta.file_type().is_symlink() {
                self.validate_symlink(dir, issues)?;
            }
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = entry.metadata()?;
            if meta.file_type().is_symlink() {
                self.validate_symlink(&path, issues)?;
            } else if meta.is_dir() {
                self.walk_directory(&path, issues, false)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `PathAllowList` matching only the temp root (for prefix matches of the
    /// resolved targets).
    fn allowlist_under(root: &Path) -> PathAllowList {
        let rule = format!("{}", root.display());
        PathAllowList::new(vec![crate::level::validation::path_allow_list::ConfigEntry::parse(&rule)
            .unwrap()
            .unwrap()])
    }

    #[test]
    #[cfg(unix)]
    fn missing_directory_yields_no_issues() {
        let v = DirectoryValidator::new(PathAllowList::new(vec![]));
        let issues = v
            .validate_directory(
                Path::new("/nonexistent/path/for/rivet-test"),
                false,
            )
            .unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn regular_file_is_not_a_directory() {
        let dir = std::env::temp_dir().join(format!("rivet-dv-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("file.txt");
        fs::write(&f, "x").unwrap();

        let v = DirectoryValidator::new(PathAllowList::new(vec![]));
        let err = v.validate_directory(&f, false).unwrap_err();
        assert!(err.to_string().contains("is not a directory"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn forbidden_symlink_in_tree_is_flagged() {
        let dir = std::env::temp_dir().join(format!("rivet-dv-{}", std::process::id()));
        fs::create_dir_all(dir.join("a/b")).unwrap();
        // Symlink target outside the allow list.
        let outside = std::env::temp_dir().join(format!("rivet-dv-out-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        let _ = std::os::unix::fs::symlink(&outside, dir.join("a/evil"));

        let v = DirectoryValidator::new(PathAllowList::new(vec![]));
        let issues = v.validate_directory(&dir, false).unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0].link().ends_with("a/evil"));
        assert_eq!(issues[0].target(), &outside);

        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn allowed_symlink_is_not_flagged() {
        let root = std::env::temp_dir().join(format!("rivet-dv-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("real");
        fs::create_dir_all(&target).unwrap();
        let _ = std::os::unix::fs::symlink(&target, root.join("alias"));

        let v = DirectoryValidator::new(allowlist_under(&target));
        let issues = v.validate_directory(&root, false).unwrap();
        // The target itself is allow-listed (its path starts with the rule), so
        // the alias's target matches and nothing is flagged.
        assert!(issues.is_empty());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn top_symlink_not_allowed_is_not_descended() {
        let dir = std::env::temp_dir().join(format!("rivet-dv-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("real");
        fs::create_dir_all(&target).unwrap();

        let v = DirectoryValidator::new(PathAllowList::new(vec![]));
        // `allow_top_symlink=false`: the top symlink itself is flagged, and the
        // walk does not cross into the target.
        let _ = std::os::unix::fs::symlink(&target, dir.join("top"));
        let issues = v.validate_directory(&dir.join("top"), false).unwrap();
        assert_eq!(issues.len(), 1);

        fs::remove_dir_all(&dir).unwrap();
    }
}

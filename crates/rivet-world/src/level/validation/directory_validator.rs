//! `net.minecraft.world.level.validation.DirectoryValidator`.
//!
//! The walker mirrors `Files.walkFileTree` without `FOLLOW_LINKS`: attributes
//! are read without following links, every symlink is visited as a leaf, and
//! traversal stops on the first I/O error after accumulating earlier issues.

use std::io;
use std::path::Path;

use super::{ForbiddenSymlinkInfo, PathAllowList};

/// Validates every symbolic-link target encountered beneath a path.
pub struct DirectoryValidator {
    symlink_target_allow_list: PathAllowList,
}

impl DirectoryValidator {
    /// `new DirectoryValidator(PathMatcher)`.
    pub fn new(symlink_target_allow_list: PathAllowList) -> Self {
        Self {
            symlink_target_allow_list,
        }
    }

    /// `validateSymlink(Path, List)`.
    pub fn validate_symlink(
        &self,
        path: &Path,
        issues: &mut Vec<ForbiddenSymlinkInfo>,
    ) -> io::Result<()> {
        // `read_link` preserves the stored target. In particular, relative
        // targets and `.`/`..` components are not resolved or normalized.
        let target = std::fs::read_link(path)?;
        if !self.symlink_target_allow_list.matches(&target) {
            issues.push(ForbiddenSymlinkInfo::new(path.to_path_buf(), target));
        }
        Ok(())
    }

    /// `validateSymlink(Path)`.
    pub fn validate_symlink_owned(&self, path: &Path) -> io::Result<Vec<ForbiddenSymlinkInfo>> {
        let mut result = Vec::new();
        self.validate_symlink(path, &mut result)?;
        Ok(result)
    }

    /// `validateDirectory(Path, boolean)`.
    pub fn validate_directory(
        &self,
        directory: &Path,
        allow_top_symlink: bool,
    ) -> io::Result<Vec<ForbiddenSymlinkInfo>> {
        let mut issues = Vec::new();
        let attributes = match std::fs::symlink_metadata(directory) {
            Ok(attributes) => attributes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(issues),
            Err(error) => return Err(error),
        };

        if attributes.is_file() {
            return Err(io::Error::other(format!(
                "Path {} is not a directory",
                directory.display()
            )));
        }

        if attributes.file_type().is_symlink() {
            if !allow_top_symlink {
                self.validate_symlink(directory, &mut issues)?;
                return Ok(issues);
            }

            // This intentionally does not resolve a relative target against
            // the link's parent. Paper assigns Files.readSymbolicLink's raw
            // Path and passes it directly to Files.walkFileTree.
            let target = std::fs::read_link(directory)?;
            self.validate_known_directory(&target, &mut issues)?;
            return Ok(issues);
        }

        self.validate_known_directory(directory, &mut issues)?;
        Ok(issues)
    }

    /// `validateKnownDirectory(Path, List)`.
    pub fn validate_known_directory(
        &self,
        directory: &Path,
        issues: &mut Vec<ForbiddenSymlinkInfo>,
    ) -> io::Result<()> {
        self.walk_path(directory, issues)
    }

    fn walk_path(&self, path: &Path, issues: &mut Vec<ForbiddenSymlinkInfo>) -> io::Result<()> {
        let attributes = std::fs::symlink_metadata(path)?;
        if attributes.file_type().is_symlink() {
            self.validate_symlink(path, issues)?;
            return Ok(());
        }
        if !attributes.is_dir() {
            // `walkFileTree` invokes visitFile for regular and "other" roots;
            // the visitor only acts on symbolic links.
            return Ok(());
        }

        for entry in std::fs::read_dir(path)? {
            self.walk_path(&entry?.path(), issues)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::validation::ConfigEntry;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rivet-directory-validator-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn deny_all() -> DirectoryValidator {
        DirectoryValidator::new(PathAllowList::new(Vec::new()))
    }

    fn allow_prefix(prefix: &Path) -> DirectoryValidator {
        DirectoryValidator::new(PathAllowList::new(vec![ConfigEntry::prefix(
            prefix.to_string_lossy(),
        )]))
    }

    #[test]
    fn missing_top_path_returns_empty_list() {
        let root = TestDir::new("missing");
        let issues = deny_all()
            .validate_directory(&root.path().join("absent"), false)
            .unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn regular_file_uses_papers_exact_error_text() {
        let root = TestDir::new("file");
        let file = root.path().join("level.dat");
        fs::write(&file, b"not relevant to validation").unwrap();
        let error = deny_all().validate_directory(&file, false).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Path {} is not a directory", file.display())
        );
    }

    #[test]
    #[cfg(unix)]
    fn nested_allowed_and_forbidden_links_are_aggregated_without_traversal() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("nested");
        let allowed = root.path().join("allowed-target");
        let outside = TestDir::new("outside");
        fs::create_dir(&allowed).unwrap();
        fs::create_dir_all(root.path().join("world/region/deep")).unwrap();
        symlink(&allowed, root.path().join("world/allowed-link")).unwrap();
        symlink(outside.path(), root.path().join("world/region/hostile-dir")).unwrap();
        symlink(
            "../../../../escape",
            root.path().join("world/region/deep/hostile-file"),
        )
        .unwrap();
        // If directory symlinks were followed, this nested link would add a
        // second issue for the outside tree.
        symlink(
            "still-forbidden",
            outside.path().join("must-not-be-visited"),
        )
        .unwrap();

        let mut issues = allow_prefix(&allowed)
            .validate_directory(&root.path().join("world"), false)
            .unwrap();
        issues.sort_by(|left, right| left.link().cmp(right.link()));

        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].target(), Path::new("../../../../escape"));
        assert_eq!(issues[1].target(), outside.path());
        assert!(
            issues
                .iter()
                .all(|issue| !issue.link().starts_with(outside.path()))
        );
    }

    #[test]
    #[cfg(unix)]
    fn forbidden_top_symlink_is_reported_and_not_followed() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("top-denied");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        symlink("nested-target", target.join("nested-link")).unwrap();
        let top = root.path().join("world");
        symlink(&target, &top).unwrap();

        let issues = deny_all().validate_directory(&top, false).unwrap();
        assert_eq!(issues, vec![ForbiddenSymlinkInfo::new(top, target)]);
    }

    #[test]
    #[cfg(unix)]
    fn allowed_absolute_top_symlink_walks_target_but_not_top_link() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("top-allowed");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let nested = target.join("nested-link");
        symlink("../forbidden", &nested).unwrap();
        let top = root.path().join("world");
        symlink(&target, &top).unwrap();

        let issues = deny_all().validate_directory(&top, true).unwrap();
        assert_eq!(
            issues,
            vec![ForbiddenSymlinkInfo::new(
                nested,
                std::path::PathBuf::from("../forbidden")
            )]
        );
    }

    #[test]
    #[cfg(unix)]
    fn relative_top_target_is_interpreted_from_process_directory() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("relative-top");
        let relative_target = format!(
            "rivet-relative-top-target-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        let process_target = std::env::current_dir().unwrap().join(&relative_target);
        fs::create_dir(&process_target).unwrap();
        let nested = process_target.join("nested-link");
        symlink("forbidden", &nested).unwrap();
        let top = root.path().join("world");
        symlink(&relative_target, &top).unwrap();

        let issues = deny_all().validate_directory(&top, true).unwrap();
        fs::remove_dir_all(&process_target).unwrap();

        assert_eq!(
            issues,
            vec![ForbiddenSymlinkInfo::new(
                Path::new(&relative_target).join("nested-link"),
                std::path::PathBuf::from("forbidden")
            )]
        );
    }

    #[test]
    #[cfg(unix)]
    fn raw_dot_dot_target_is_matched_and_reported_unchanged() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("raw-target");
        let link = root.path().join("raw-link");
        symlink("safe/../outside//payload", &link).unwrap();

        let allowed = DirectoryValidator::new(PathAllowList::new(vec![ConfigEntry::prefix(
            "safe/../outside",
        )]));
        assert!(allowed.validate_symlink_owned(&link).unwrap().is_empty());

        let issues = deny_all().validate_symlink_owned(&link).unwrap();
        assert_eq!(issues[0].target(), Path::new("safe/../outside//payload"));
    }
}

//! Read-only-copy world workflow for the M2 loaded-world acceptance harness
//! (issue #316).
//!
//! This module owns the acceptance-harness machinery that is independent of the
//! server's world-loading capability:
//!
//! - resolving the known local Minecraft 26.2 world (the launcher-created
//!   save, overridable via `RIVET_WORLD_SRC`);
//! - copying it into a deterministic disposable temp world ([`TempWorld`]),
//!   refusing to follow symlinks so the copy can never alias the source;
//! - asserting the copy is byte-faithful to the source and that the source is
//!   not mutated between the pre-run snapshot and the post-run snapshot — the
//!   explicit no-source-mutation guarantee;
//!
//! It deliberately does NOT parse `level.dat`, check `DataVersion`, or read
//! Anvil region files — those belong to #323 (level validation) and #231
//! (region storage). This module only moves bytes and proves they were not
//! touched, so it stays independent of the level/storage slices the harness
//! will one day drive. The server world-path launch probe lives in [`server`]
//! (`crate::server`), which owns the launch interface contract.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Default launcher-created Minecraft 26.2 world name (local-official-minecraft-client).
const LAUNCHER_WORLD_NAME: &str = "New World";

#[derive(Debug)]
pub enum Error {
    /// A prerequisite is missing (source world not found) — maps to UNVERIFIED.
    Unverified(String),
    /// A harness/safety failure (copy not faithful, source mutated, symlink
    /// refused, probe could not launch) — maps to FAIL.
    Gate(String),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unverified(m) => write!(f, "{m}"),
            Error::Gate(m) => write!(f, "{m}"),
            Error::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// The default world path under a launcher home directory.
fn default_world_src(home: &Path) -> PathBuf {
    home.join("Library/Application Support/minecraft/saves")
        .join(LAUNCHER_WORLD_NAME)
}

/// Resolve the source world from an optional override path and a home
/// directory (pure, so tests exercise the override/default/missing branches
/// without touching the process env).
fn resolve_from(override_path: Option<&Path>, home: &Path) -> Result<PathBuf, Error> {
    let p = match override_path {
        Some(p) => p.to_path_buf(),
        None => default_world_src(home),
    };
    if p.is_dir() {
        Ok(p)
    } else {
        Err(Error::Unverified(format!(
            "Minecraft 26.2 world not found at {} — set RIVET_WORLD_SRC to point at a world \
             save directory",
            p.display()
        )))
    }
}

/// Resolve the known local Minecraft 26.2 world: `RIVET_WORLD_SRC` wins, then
/// the default launcher save. A missing world is a prerequisite (UNVERIFIED),
/// not a harness failure.
pub fn resolve_source_world() -> Result<PathBuf, Error> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    resolve_from(
        std::env::var("RIVET_WORLD_SRC")
            .ok()
            .as_deref()
            .map(Path::new),
        &home,
    )
}

/// SHA-256 of a byte buffer (the copy-fidelity and no-mutation fingerprints).
pub fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(bytes));
    out
}

/// The type and, for a file, contents of one entry in a tree fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFingerprint {
    Directory,
    File([u8; 32]),
}

/// One deterministic tree-fingerprint entry: a relative path plus an explicit
/// entry-type marker (and content hash for files).
pub type TreeEntry = (PathBuf, EntryFingerprint);

/// Recursively fingerprint every directory and regular file under `root`,
/// keyed by its path relative to `root` and sorted by relative path. Recording
/// directories explicitly makes empty-directory add/delete/rename observable.
/// Symlinks are refused: a tree walk that follows a link could silently read
/// content outside the tree (and, for the copy, could alias the source), so the
/// harness fails loudly instead.
pub fn hash_tree(root: &Path) -> Result<Vec<TreeEntry>, Error> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<TreeEntry>) -> Result<(), Error> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(Error::Gate(format!(
                    "refusing to follow symlink {} under {}",
                    path.display(),
                    root.display()
                )));
            }
            let rel = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .expect("walked paths stay under root");
            if meta.is_dir() {
                out.push((rel, EntryFingerprint::Directory));
                walk(root, &path, out)?;
            } else if meta.is_file() {
                out.push((rel, EntryFingerprint::File(hash_bytes(&fs::read(&path)?))));
            } else {
                return Err(Error::Gate(format!(
                    "refusing non-regular filesystem entry {} under {}",
                    path.display(),
                    root.display()
                )));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Recursively copy `src` into `dst`, refusing symlinks so the copy is a full
/// materialized mirror and can never alias the source (a link farm would let a
/// write through the "copy" reach the original save).
fn copy_tree(src: &Path, dst: &Path) -> Result<(), Error> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let meta = fs::symlink_metadata(&from)?;
        if meta.file_type().is_symlink() {
            return Err(Error::Gate(format!(
                "refusing to copy symlink {} — the read-only copy must be a full materialized \
                 mirror, not a link into the source",
                from.display()
            )));
        }
        let to = dst.join(entry.file_name());
        if meta.is_dir() {
            fs::create_dir_all(&to)?;
            copy_tree(&from, &to)?;
        } else if meta.is_file() {
            fs::copy(&from, &to)?;
        } else {
            return Err(Error::Gate(format!(
                "refusing non-regular filesystem entry {} while copying the source world",
                from.display()
            )));
        }
    }
    Ok(())
}

/// The copy must be byte-faithful to the source: the same relative files with
/// identical contents, and nothing extra. Any divergence means the copy cannot
/// stand in for the source, so a later server boot would be against a world the
/// harness never validated.
pub fn assert_copy_equals_source(source: &[TreeEntry], copy: &[TreeEntry]) -> Result<(), Error> {
    if source.len() != copy.len() {
        return Err(Error::Gate(format!(
            "temp-world copy has {} entry/entries but the source has {} — the copy is not a faithful \
             mirror",
            copy.len(),
            source.len()
        )));
    }
    for (s, c) in source.iter().zip(copy) {
        if s.0 != c.0 {
            return Err(Error::Gate(format!(
                "temp-world copy names {} where the source names {} — the copy is not a \
                 faithful mirror",
                c.0.display(),
                s.0.display()
            )));
        }
        if s.1 != c.1 {
            return Err(Error::Gate(format!(
                "temp-world copy entry {} differs in type or contents from the source — the copy is not a faithful \
                 mirror",
                c.0.display()
            )));
        }
    }
    Ok(())
}

/// The source world must be byte-identical before and after the run. This is
/// the explicit no-source-mutation guarantee: the harness reads the source
/// (to copy and to hash), and anything that writes the source — the harness
/// itself, a server booted against it, or a concurrently-running client — fails
/// the run loudly rather than silently proceeding on a mutated save.
pub fn assert_source_unchanged(before: &[TreeEntry], after: &[TreeEntry]) -> Result<(), Error> {
    if before == after {
        return Ok(());
    }
    let detail = before
        .iter()
        .zip(after)
        .find(|(b, a)| b != a)
        .map(|(b, a)| {
            if b.0 != a.0 {
                format!("entry set changed ({:?} present/absent)", a.0.display())
            } else {
                format!("entry {} changed type or contents", b.0.display())
            }
        })
        .unwrap_or_else(|| "entry set, types, or contents changed".to_owned());
    Err(Error::Gate(format!(
        "the source world was MUTATED during the run ({detail}) — the harness and any server \
         it boots must never write to the original save; the read-only-copy guarantee is broken"
    )))
}

/// A deterministic disposable copy of the source world.
///
/// [`TempWorld::create`] sets up the copy at a fixed destination path: any
/// stale leftover from a crashed previous run is removed first, then the source
/// is copied fresh, so setup is deterministic and idempotent. [`TempWorld::cleanup`]
/// (or [`Drop`]) removes the copy, so a temp world can never leak a stale tree a
/// later run would mistake for fresh — the file-system analog of the shared
/// child-process kill-on-drop.
#[derive(Debug)]
pub struct TempWorld {
    path: PathBuf,
    removed: bool,
}

impl TempWorld {
    /// Create a fresh disposable copy of `source` at `dest`. Refuses to copy
    /// into itself or into a path nested inside the source, which would alias
    /// the very files being copied.
    pub fn create(source: &Path, dest: &Path) -> Result<Self, Error> {
        if fs::symlink_metadata(source)?.file_type().is_symlink() {
            return Err(Error::Gate(format!(
                "source world {} is a symlink; the harness requires the concrete launcher save \
                 directory so it can prove the server never receives an alias of the source",
                source.display()
            )));
        }
        let source_abs = source.canonicalize().map_err(|e| {
            Error::Gate(format!(
                "source world {} is not readable: {e}",
                source.display()
            ))
        })?;
        let dest_abs = match fs::canonicalize(dest) {
            Ok(path) => path,
            Err(_) => {
                let parent = dest.parent().unwrap_or_else(|| Path::new("."));
                let parent = parent.canonicalize().map_err(|e| {
                    Error::Gate(format!(
                        "temp-world destination parent {} is not accessible: {e}",
                        parent.display()
                    ))
                })?;
                parent.join(dest.file_name().ok_or_else(|| {
                    Error::Gate(format!(
                        "temp-world destination {} has no final path component",
                        dest.display()
                    ))
                })?)
            }
        };
        if source_abs == dest_abs || dest_abs.starts_with(&source_abs) {
            return Err(Error::Gate(format!(
                "temp-world destination {} aliases the source {}",
                dest.display(),
                source.display()
            )));
        }
        if let Ok(meta) = fs::symlink_metadata(dest) {
            if meta.file_type().is_symlink() {
                return Err(Error::Gate(format!(
                    "refusing stale symlink at temp-world destination {}",
                    dest.display()
                )));
            }
            if meta.is_dir() {
                fs::remove_dir_all(dest)?;
            } else {
                fs::remove_file(dest)?;
            }
        }
        fs::create_dir_all(dest)?;
        if let Err(error) = copy_tree(source, dest) {
            if let Err(cleanup_error) = fs::remove_dir_all(dest) {
                return Err(Error::Gate(format!(
                    "world copy failed ({error}) and partial-copy cleanup at {} also failed: \
                     {cleanup_error}",
                    dest.display()
                )));
            }
            return Err(error);
        }
        Ok(Self {
            path: dest.to_path_buf(),
            removed: false,
        })
    }

    /// The copy's root directory (absolute).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Hash the copy with the same walker used on the source, so copy fidelity
    /// is asserted on identical fingerprints.
    pub fn hash_tree(&self) -> Result<Vec<TreeEntry>, Error> {
        hash_tree(&self.path)
    }

    /// Deterministically remove the copy. Idempotent.
    pub fn cleanup(&mut self) -> Result<(), Error> {
        if self.removed {
            return Ok(());
        }
        if self.path.exists() {
            fs::remove_dir_all(&self.path)?;
        }
        self.removed = true;
        Ok(())
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        // Best-effort: cleanup failure on the panic/error path must not mask the
        // original error (mirrors ChildServer's kill-on-drop).
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_world(tag: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("rivet-load-world-{tag}-{}", std::process::id()));
        let region = base.join("dimensions/minecraft/overworld/region");
        fs::create_dir_all(&region).unwrap();
        fs::write(base.join("level.dat"), [0u8, 1, 2, 3]).unwrap();
        fs::write(region.join("r.0.0.mca"), b"region bytes").unwrap();
        fs::write(base.join("session.lock"), b"lock").unwrap();
        base
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    fn sibling(dir: &Path, suffix: &str) -> PathBuf {
        dir.parent().unwrap().join(format!(
            "{}-{suffix}",
            dir.file_name().unwrap().to_string_lossy()
        ))
    }

    #[test]
    fn resolve_from_prefers_the_override_and_rejects_a_missing_world() {
        let world = fixture_world("resolve");
        let home = std::env::temp_dir().join(format!("rivet-load-home-{}", std::process::id()));

        // An explicit override pointing at a real world resolves.
        assert_eq!(
            resolve_from(Some(&world), &home).unwrap(),
            world,
            "the override must win over the home default"
        );
        // An override pointing at nothing is a missing prerequisite (UNVERIFIED).
        let missing = world.join("does-not-exist");
        assert!(matches!(
            resolve_from(Some(&missing), &home),
            Err(Error::Unverified(_))
        ));
        // No override: the default launcher path under the fake home is absent,
        // so the resolution fails as UNVERIFIED rather than guessing a path.
        assert!(matches!(
            resolve_from(None, &home),
            Err(Error::Unverified(_))
        ));

        cleanup(&world);
        cleanup(&home);
    }

    #[test]
    fn copy_tree_mirrors_a_world_and_refuses_symlinks() {
        let src = fixture_world("copy");
        let dst = sibling(&src, "copy");
        cleanup(&dst);
        copy_tree(&src, &dst).unwrap();
        assert_eq!(fs::read(dst.join("level.dat")).unwrap(), [0u8, 1, 2, 3]);
        assert_eq!(
            fs::read(dst.join("dimensions/minecraft/overworld/region/r.0.0.mca")).unwrap(),
            b"region bytes"
        );
        assert_eq!(fs::read(dst.join("session.lock")).unwrap(), b"lock");

        // A symlink in the source must be refused, not followed: following it
        // could pull content from outside the tree or alias the source.
        #[cfg(unix)]
        {
            let link_src = src.join("link-src");
            fs::write(&link_src, b"outside").unwrap();
            let link = src.join("level.dat.link");
            std::os::unix::fs::symlink(&link_src, &link).unwrap();
            let dst2 = sibling(&src, "copy2");
            cleanup(&dst2);
            let err = copy_tree(&src, &dst2).unwrap_err();
            assert!(
                matches!(err, Error::Gate(_)),
                "a symlink must be refused: {err}"
            );
        }

        cleanup(&src);
        cleanup(&dst);
        cleanup(&sibling(&src, "copy2"));
    }

    #[test]
    fn hash_tree_is_deterministic_and_sensitive_to_content() {
        let world = fixture_world("hash");
        let a = hash_tree(&world).unwrap();
        let b = hash_tree(&world).unwrap();
        assert_eq!(a, b, "hashing the same tree twice must be identical");
        assert_eq!(a.len(), 7, "four directories plus three files");

        fs::write(world.join("level.dat"), [9u8, 9]).unwrap();
        let c = hash_tree(&world).unwrap();
        assert_ne!(a, c, "a content change must change the tree hash");

        cleanup(&world);
    }

    #[test]
    fn hash_tree_detects_empty_directory_add_delete_and_rename() {
        let world = fixture_world("empty-dir");
        let baseline = hash_tree(&world).unwrap();

        let empty = world.join("empty");
        fs::create_dir(&empty).unwrap();
        let added = hash_tree(&world).unwrap();
        assert_ne!(baseline, added, "adding an empty directory must be visible");
        assert!(added.contains(&(PathBuf::from("empty"), EntryFingerprint::Directory)));

        let renamed = world.join("renamed-empty");
        fs::rename(&empty, &renamed).unwrap();
        let renamed_fingerprint = hash_tree(&world).unwrap();
        assert_ne!(
            added, renamed_fingerprint,
            "renaming an empty directory must be visible"
        );
        assert!(
            renamed_fingerprint
                .contains(&(PathBuf::from("renamed-empty"), EntryFingerprint::Directory))
        );

        fs::remove_dir(&renamed).unwrap();
        assert_eq!(
            baseline,
            hash_tree(&world).unwrap(),
            "deleting the empty directory must restore the original fingerprint"
        );

        cleanup(&world);
    }

    #[test]
    fn assert_copy_equals_source_detects_tampering() {
        let src = fixture_world("fidelity");
        let dst = sibling(&src, "copy");
        cleanup(&dst);
        copy_tree(&src, &dst).unwrap();
        let source = hash_tree(&src).unwrap();
        let copy = hash_tree(&dst).unwrap();
        assert_copy_equals_source(&source, &copy).expect("an untouched copy is faithful");

        // Tamper a file in the copy: the copy-fidelity assertion must name it.
        fs::write(dst.join("level.dat"), [7u8, 7, 7]).unwrap();
        let copy_t = hash_tree(&dst).unwrap();
        let err = assert_copy_equals_source(&source, &copy_t).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("level.dat"),
            "a tampered copy must be named: {err}"
        );

        // Add an extra file: the copy is no longer a mirror.
        fs::write(dst.join("extra.mca"), b"extra").unwrap();
        let copy_e = hash_tree(&dst).unwrap();
        let err = assert_copy_equals_source(&source, &copy_e).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_))
                && err.to_string().contains("8 entry/entries")
                && err.to_string().contains("source has 7"),
            "an extra file must fail the mirror check: {err}"
        );

        cleanup(&src);
        cleanup(&dst);
    }

    #[test]
    fn assert_source_unchanged_detects_mutation() {
        let src = fixture_world("nomut");
        let before = hash_tree(&src).unwrap();
        assert_source_unchanged(&before, &before).expect("identical snapshots are unchanged");

        // Mutate the source between snapshots: the no-mutation assertion must
        // name the file.
        fs::write(src.join("level.dat"), [5u8, 5]).unwrap();
        let after = hash_tree(&src).unwrap();
        let err = assert_source_unchanged(&before, &after).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_))
                && err.to_string().contains("MUTATED")
                && err.to_string().contains("level.dat"),
            "a mutated source must be named: {err}"
        );

        cleanup(&src);
    }

    #[test]
    fn temp_world_setup_removes_stale_and_cleanup_removes_the_copy() {
        let src = fixture_world("temp");
        let dest = sibling(&src, "temp");
        cleanup(&dest);
        fs::create_dir_all(&dest).unwrap();
        // A stale leftover from a "crashed previous run" must be replaced by a
        // fresh copy, not kept.
        fs::write(dest.join("stale.txt"), b"stale").unwrap();

        let mut temp = TempWorld::create(&src, &dest).unwrap();
        assert_eq!(temp.path(), dest);
        assert!(
            !dest.join("stale.txt").exists(),
            "stale leftover must be cleared before the fresh copy"
        );
        assert_copy_equals_source(&hash_tree(&src).unwrap(), &temp.hash_tree().unwrap())
            .expect("the fresh copy is faithful");

        temp.cleanup().unwrap();
        assert!(!dest.exists(), "cleanup must remove the copy");
        // Idempotent: a second cleanup is a no-op, and Drop is a no-op too.
        temp.cleanup().unwrap();

        cleanup(&src);
    }

    #[test]
    fn temp_world_refuses_aliasing_destinations() {
        let src = fixture_world("alias");
        // The destination may not be the source itself, nor nested inside it.
        let err = TempWorld::create(&src, &src).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)),
            "self-copy must be refused: {err}"
        );
        let nested = src.join("inner");
        fs::create_dir_all(&nested).unwrap();
        let err = TempWorld::create(&src, &nested).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)),
            "a destination nested in the source must be refused: {err}"
        );
        cleanup(&src);
    }

    #[cfg(unix)]
    #[test]
    fn temp_world_refuses_a_symlinked_source_and_cleans_a_partial_copy() {
        let src = fixture_world("source-link");
        let source_link = sibling(&src, "source-link-alias");
        let dest = sibling(&src, "source-link-copy");
        cleanup(&source_link);
        cleanup(&dest);
        std::os::unix::fs::symlink(&src, &source_link).unwrap();

        let err = TempWorld::create(&source_link, &dest).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("is a symlink"),
            "the root source alias must be refused: {err}"
        );
        assert!(!dest.exists(), "a refused source must not leave a copy");

        fs::remove_file(&source_link).unwrap();
        cleanup(&src);
        cleanup(&dest);
    }
}

//! Read-only-copy world workflow for the M2 loaded-world acceptance harness
//! (issue #316).
//!
//! This module owns the acceptance-harness machinery that is independent of the
//! server's world-loading capability:
//!
//! - resolving the known local Minecraft 26.2 world (the launcher-created
//!   save, overridable via `RIVET_WORLD_SRC`);
//! - copying it beneath a fresh private disposable directory ([`TempWorld`]),
//!   refusing symlinks and retaining a directory handle so pathname replacement
//!   cannot redirect the server to the source;
//! - asserting the copy is byte-faithful to the source and that two non-atomic
//!   pre/post source snapshots match. This detects differences visible in
//!   those snapshots; it cannot prove that no transient write occurred;
//!
//! It deliberately does NOT parse `level.dat`, check `DataVersion`, or read
//! Anvil region files — those belong to #323 (level validation) and #231
//! (region storage). This module only moves bytes and proves they were not
//! touched, so it stays independent of the level/storage slices the harness
//! will one day drive. The server world-path launch probe lives in [`server`]
//! (`crate::server`), which owns the launch interface contract.

use std::ffi::{CStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::fd::{AsFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(all(unix, test))]
use std::os::unix::fs::PermissionsExt;

use sha2::{Digest, Sha256};

#[cfg(unix)]
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags};
#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::io::{FdFlags, fcntl_setfd};

/// Default launcher-created Minecraft 26.2 world name (local-official-minecraft-client).
const LAUNCHER_WORLD_NAME: &str = "New World";

/// Fixed upper bound for each file-content read while fingerprinting a world.
const HASH_BUFFER_SIZE: usize = 64 * 1024;

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

/// Original whole-buffer SHA-256 path, retained as the streaming test oracle.
#[cfg(test)]
fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(bytes));
    out
}

/// Stream a reader into SHA-256 without allocating in proportion to its size.
fn hash_reader(reader: impl Read) -> io::Result<[u8; 32]> {
    let mut reader = BufReader::with_capacity(HASH_BUFFER_SIZE, reader);
    let mut hasher = Sha256::new();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        hasher.update(bytes);
        let consumed = bytes.len();
        reader.consume(consumed);
    }

    Ok(hasher.finalize().into())
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
#[cfg(unix)]
fn open_child(dir: &impl AsFd, name: &CStr, flags: OFlags) -> io::Result<OwnedFd> {
    let flags = flags | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    return rustix::fs::openat2(
        dir,
        name,
        flags,
        Mode::empty(),
        rustix::fs::ResolveFlags::BENEATH
            | rustix::fs::ResolveFlags::NO_SYMLINKS
            | rustix::fs::ResolveFlags::NO_MAGICLINKS
            | rustix::fs::ResolveFlags::NO_XDEV,
    )
    .map_err(io::Error::from);

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    rustix::fs::openat(dir, name, flags, Mode::empty()).map_err(io::Error::from)
}

#[cfg(unix)]
fn entry_path(parent: &Path, name: &CStr) -> PathBuf {
    parent.join(OsString::from_vec(name.to_bytes().to_vec()))
}

#[cfg(unix)]
fn file_type(fd: &impl AsFd) -> io::Result<FileType> {
    rustix::fs::fstat(fd)
        .map(|stat| FileType::from_raw_mode(stat.st_mode))
        .map_err(io::Error::from)
}

#[cfg(unix)]
fn open_root(path: &Path) -> Result<OwnedFd, Error> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    if file_type(&fd)? != FileType::Directory {
        return Err(Error::Gate(format!(
            "{} must be a concrete directory",
            path.display()
        )));
    }
    Ok(fd)
}

#[cfg(unix)]
fn hash_tree_fd(root: &OwnedFd) -> Result<Vec<TreeEntry>, Error> {
    fn walk(dir: &OwnedFd, relative: &Path, out: &mut Vec<TreeEntry>) -> Result<(), Error> {
        for entry in Dir::read_from(dir).map_err(io::Error::from)? {
            let entry = entry.map_err(io::Error::from)?;
            let name = entry.file_name();
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            let rel = entry_path(relative, name);
            let fd = open_child(dir, name, OFlags::RDONLY).map_err(|error| {
                Error::Gate(format!(
                    "refusing entry {} that could not be opened without following links: {error}",
                    rel.display()
                ))
            })?;
            match file_type(&fd)? {
                FileType::Directory => {
                    out.push((rel.clone(), EntryFingerprint::Directory));
                    walk(&fd, &rel, out)?;
                }
                FileType::RegularFile => {
                    let hash = hash_reader(fs::File::from(fd)).map_err(|error| {
                        Error::Io(io::Error::new(
                            error.kind(),
                            format!("read {} for fingerprinting: {error}", rel.display()),
                        ))
                    })?;
                    out.push((rel, EntryFingerprint::File(hash)));
                }
                _ => {
                    return Err(Error::Gate(format!(
                        "refusing non-regular filesystem entry {}",
                        rel.display()
                    )));
                }
            }
        }
        Ok(())
    }

    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub fn hash_tree(root: &Path) -> Result<Vec<TreeEntry>, Error> {
    #[cfg(not(unix))]
    return Err(Error::Gate(
        "the loaded-world safety boundary requires Unix directory descriptors".to_owned(),
    ));
    #[cfg(unix)]
    hash_tree_fd(&open_root(root)?)
}

/// Recursively copy `src` into `dst`, refusing symlinks so the copy is a full
/// materialized mirror and can never alias the source (a link farm would let a
/// write through the "copy" reach the original save).
#[cfg(unix)]
fn copy_tree_fd(src: &OwnedFd, dst: &OwnedFd, relative: &Path) -> Result<(), Error> {
    for entry in Dir::read_from(src).map_err(io::Error::from)? {
        let entry = entry.map_err(io::Error::from)?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let rel = entry_path(relative, name);
        let source = open_child(src, name, OFlags::RDONLY).map_err(|error| {
            Error::Gate(format!(
                "refusing to copy entry {} without following links: {error}",
                rel.display()
            ))
        })?;
        match file_type(&source)? {
            FileType::Directory => {
                rustix::fs::mkdirat(dst, name, Mode::from_raw_mode(0o700))
                    .map_err(io::Error::from)?;
                let destination = open_child(dst, name, OFlags::RDONLY | OFlags::DIRECTORY)?;
                copy_tree_fd(&source, &destination, &rel)?;
            }
            FileType::RegularFile => {
                let destination = rustix::fs::openat(
                    dst,
                    name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )
                .map_err(io::Error::from)?;
                io::copy(
                    &mut fs::File::from(source),
                    &mut fs::File::from(destination),
                )
                .map_err(|error| {
                    Error::Io(io::Error::new(
                        error.kind(),
                        format!("copy {}: {error}", rel.display()),
                    ))
                })?;
            }
            _ => {
                return Err(Error::Gate(format!(
                    "refusing non-regular filesystem entry {} while copying the source world",
                    rel.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn copy_tree(src: &Path, dst: &Path) -> Result<(), Error> {
    #[cfg(not(unix))]
    return Err(Error::Gate(
        "the loaded-world safety boundary requires Unix directory descriptors".to_owned(),
    ));
    #[cfg(unix)]
    {
        let source = open_root(src)?;
        fs::create_dir(dst)?;
        let destination = open_root(dst)?;
        copy_tree_fd(&source, &destination, Path::new(""))
    }
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

/// The source world must be byte-identical in the snapshots taken before and
/// after the run. A difference visible in either non-atomic snapshot fails the
/// run loudly; a transient modify-and-restore event may not be observable.
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

/// A disposable copy of the source world with deterministic contents and an
/// unpredictable per-run identity.
///
/// [`TempWorld::create`] creates a fresh private `0700` parent beneath the
/// supplied storage directory and materializes the copy as its `world` child.
/// On Unix, the copy is opened once with `O_NOFOLLOW` and retained through
/// child shutdown. Linux fingerprints and launches through that descriptor;
/// other Unix platforms retain it as ownership evidence but use the private
/// path because their fd namespace is not portably directory-traversable.
/// [`TempWorld::cleanup`] (or [`Drop`]) removes the unique parent only while
/// its retained storage entry still names the exact device/inode created by
/// this instance. Path substitution therefore fails closed and leaks the
/// owned tree instead of recursively deleting an attacker-selected substitute.
#[derive(Debug)]
pub struct TempWorld {
    path: PathBuf,
    #[cfg(unix)]
    storage_dir: OwnedFd,
    #[cfg(unix)]
    parent_dir: OwnedFd,
    #[cfg(unix)]
    parent_name: OsString,
    #[cfg(unix)]
    parent_identity: Identity,
    #[cfg(unix)]
    world_dir: OwnedFd,
    active: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: rustix::fs::Dev,
    inode: u64,
}

#[cfg(unix)]
fn identity(stat: &rustix::fs::Stat) -> Identity {
    Identity {
        device: stat.st_dev,
        inode: stat.st_ino,
    }
}

#[cfg(unix)]
fn fd_identity(fd: &impl AsFd) -> io::Result<Identity> {
    rustix::fs::fstat(fd)
        .map(|stat| identity(&stat))
        .map_err(io::Error::from)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn descriptor_path(fd: &impl AsRawFd) -> PathBuf {
    Path::new("/proc/self/fd").join(fd.as_raw_fd().to_string())
}

#[cfg(unix)]
fn opened_directory_path(fd: &OwnedFd, _original: &Path) -> Result<PathBuf, Error> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    return descriptor_path(fd).canonicalize().map_err(Error::from);

    #[cfg(target_vendor = "apple")]
    return rustix::fs::getpath(fd)
        .map(|path| PathBuf::from(std::ffi::OsString::from_vec(path.into_bytes())))
        .map_err(io::Error::from)
        .map_err(Error::from);

    #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
    _original.canonicalize().map_err(Error::from)
}

fn reject_aliasing(source: &Path, destination: &Path) -> Result<(), Error> {
    if source == destination || destination.starts_with(source) || source.starts_with(destination) {
        return Err(Error::Gate(format!(
            "temp-world storage {} equals, contains, or is contained by source {}",
            destination.display(),
            source.display()
        )));
    }
    Ok(())
}

/// Resolve a not-yet-created storage path through its nearest existing
/// ancestor, then reject equality and containment in either direction. This
/// function is deliberately mutation-free so callers can run it before
/// `create_dir_all`.
pub fn validate_prospective_storage(source: &Path, storage: &Path) -> Result<(), Error> {
    if fs::symlink_metadata(source)?.file_type().is_symlink() {
        return Err(Error::Gate(format!(
            "source world {} is a symlink; the harness requires a concrete directory",
            source.display()
        )));
    }
    let source_abs = source.canonicalize().map_err(|error| {
        Error::Gate(format!(
            "source world {} is not readable: {error}",
            source.display()
        ))
    })?;

    let mut existing = storage;
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            Error::Gate(format!(
                "temp-world storage {} has no existing ancestor",
                storage.display()
            ))
        })?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(|| {
            Error::Gate(format!(
                "temp-world storage {} has no existing ancestor",
                storage.display()
            ))
        })?;
    }
    let mut prospective = existing.canonicalize().map_err(|error| {
        Error::Gate(format!(
            "nearest existing ancestor {} of temp-world storage is not readable: {error}",
            existing.display()
        ))
    })?;
    for component in missing.iter().rev() {
        prospective.push(component);
    }
    reject_aliasing(&source_abs, &prospective)
}

#[cfg(unix)]
fn random_parent_name() -> Result<OsString, Error> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|error| Error::Gate(format!("secure random name generation failed: {error}")))?;
    let mut name = String::with_capacity(5 + random.len() * 2);
    name.push_str("copy-");
    for byte in random {
        use fmt::Write as _;
        write!(&mut name, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(name.into())
}

#[cfg(unix)]
fn create_private_parent(storage: &OwnedFd) -> Result<(OsString, OwnedFd), Error> {
    for _ in 0..128 {
        let name = random_parent_name()?;
        match rustix::fs::mkdirat(storage, &name, Mode::from_raw_mode(0o700)) {
            Ok(()) => {
                // Open the newly-created single component relative to storage;
                // never resolve it again through the storage pathname.
                let c_name = std::ffi::CString::new(name.as_encoded_bytes()).map_err(|_| {
                    Error::Gate("generated private-directory name contained NUL".to_owned())
                })?;
                let fd = open_child(storage, &c_name, OFlags::RDONLY | OFlags::DIRECTORY)?;
                return Ok((name, fd));
            }
            Err(rustix::io::Errno::EXIST) => continue,
            Err(error) => return Err(io::Error::from(error).into()),
        }
    }
    Err(Error::Gate(
        "could not allocate a unique private temp-world parent".to_owned(),
    ))
}

#[cfg(unix)]
fn entry_identity(dir: &OwnedFd, name: &CStr) -> io::Result<(Identity, FileType)> {
    let stat = rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
    Ok((identity(&stat), FileType::from_raw_mode(stat.st_mode)))
}

#[cfg(unix)]
fn remove_owned_tree(dir: &OwnedFd, relative: &Path) -> Result<(), Error> {
    let names = Dir::read_from(dir)
        .map_err(io::Error::from)?
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry.file_name().to_bytes() != b"."
                    && entry.file_name().to_bytes() != b".." =>
            {
                Some(Ok(entry.file_name().to_owned()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(io::Error::from(error))),
        })
        .collect::<io::Result<Vec<_>>>()?;

    for name in names {
        let rel = entry_path(relative, &name);
        let child = open_child(dir, &name, OFlags::RDONLY).map_err(|error| {
            Error::Gate(format!(
                "cleanup refused entry {} that changed or could not be opened safely: {error}",
                rel.display()
            ))
        })?;
        let expected = fd_identity(&child)?;
        let kind = file_type(&child)?;
        if kind == FileType::Directory {
            remove_owned_tree(&child, &rel)?;
        } else if kind != FileType::RegularFile {
            return Err(Error::Gate(format!(
                "cleanup refused non-regular entry {}; leaking the private tree",
                rel.display()
            )));
        }
        let (visible, visible_kind) = entry_identity(dir, &name).map_err(|error| {
            Error::Gate(format!(
                "cleanup could not revalidate {}: {error}; leaking the private tree",
                rel.display()
            ))
        })?;
        if visible != expected || visible_kind != kind {
            return Err(Error::Gate(format!(
                "cleanup identity changed for {}; leaking the private tree",
                rel.display()
            )));
        }
        rustix::fs::unlinkat(
            dir,
            &name,
            if kind == FileType::Directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        )
        .map_err(io::Error::from)?;
    }
    Ok(())
}

impl TempWorld {
    /// Create a fresh disposable copy of `source` beneath `storage`.
    ///
    /// Canonical equality and containment are rejected in both directions
    /// before the first filesystem mutation. This is essential when an
    /// override accidentally points inside the harness storage directory: no
    /// cleanup or setup may then touch the source.
    pub fn create(source: &Path, storage: &Path) -> Result<Self, Error> {
        #[cfg(not(unix))]
        return Err(Error::Gate(
            "the loaded-world safety boundary requires Unix directory descriptors".to_owned(),
        ));

        #[cfg(unix)]
        {
            validate_prospective_storage(source, storage)?;
            let storage_meta = fs::symlink_metadata(storage).map_err(|e| {
                Error::Gate(format!(
                    "temp-world storage {} is not accessible: {e}",
                    storage.display()
                ))
            })?;
            if storage_meta.file_type().is_symlink() || !storage_meta.is_dir() {
                return Err(Error::Gate(format!(
                    "temp-world storage {} must be a concrete directory",
                    storage.display()
                )));
            }
            let storage_dir = rustix::fs::open(
                storage,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?;
            let storage_abs = opened_directory_path(&storage_dir, storage).map_err(|e| {
                Error::Gate(format!(
                    "opened temp-world storage {} cannot be resolved: {e}",
                    storage.display()
                ))
            })?;
            let source_dir = open_root(source)?;
            let opened_source = opened_directory_path(&source_dir, source).map_err(|error| {
                Error::Gate(format!(
                    "opened source world {} cannot be resolved: {error}",
                    source.display()
                ))
            })?;
            reject_aliasing(&opened_source, &storage_abs)?;
            let (parent_name, parent_dir) = create_private_parent(&storage_dir)?;
            let parent_identity = fd_identity(&parent_dir)?;
            rustix::fs::mkdirat(&parent_dir, "world", Mode::from_raw_mode(0o700))
                .map_err(io::Error::from)?;
            let world_dir = open_child(&parent_dir, c"world", OFlags::RDONLY | OFlags::DIRECTORY)?;
            let world_path = storage_abs.join(&parent_name).join("world");
            let mut temp = Self {
                path: world_path,
                storage_dir,
                parent_dir,
                parent_name,
                parent_identity,
                world_dir,
                active: true,
            };
            if let Err(error) = copy_tree_fd(&source_dir, &temp.world_dir, Path::new("")) {
                if let Err(cleanup_error) = temp.cleanup() {
                    return Err(Error::Gate(format!(
                        "world copy failed ({error}) and identity-safe partial-copy cleanup also failed: {cleanup_error}"
                    )));
                }
                return Err(error);
            }
            // Linux resolves /proc/self/fd/<n> in the child. Clearing CLOEXEC
            // intentionally transfers this one capability across exec;
            // TempWorld retains ownership in the harness until the child exits.
            #[cfg(any(target_os = "linux", target_os = "android"))]
            fcntl_setfd(&temp.world_dir, FdFlags::empty()).map_err(io::Error::from)?;
            Ok(temp)
        }
    }

    /// The copy's root directory (absolute).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Path the child server consumes. Linux names the retained directory
    /// descriptor; platforms without a traversable fd namespace use the fresh
    /// private pathname documented in the module threat model.
    pub fn server_path(&self) -> PathBuf {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        return descriptor_path(&self.world_dir);
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        self.path.clone()
    }

    /// Hash the copy with the same walker used on the source, so copy fidelity
    /// is asserted on identical fingerprints.
    pub fn hash_tree(&self) -> Result<Vec<TreeEntry>, Error> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        return hash_tree_fd(&self.world_dir);
        #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
        hash_tree_fd(&self.world_dir)
    }

    /// Deterministically remove the copy. Idempotent.
    pub fn cleanup(&mut self) -> Result<(), Error> {
        if !self.active {
            return Ok(());
        }
        #[cfg(unix)]
        {
            let c_name = std::ffi::CString::new(self.parent_name.as_encoded_bytes())
                .map_err(|_| Error::Gate("private-directory name contained NUL".to_owned()))?;
            let (visible, kind) = entry_identity(&self.storage_dir, &c_name).map_err(|error| {
                Error::Gate(format!(
                    "cleanup cannot revalidate {}: {error}; leaking the private tree",
                    self.path.parent().unwrap().display()
                ))
            })?;
            if visible != self.parent_identity || kind != FileType::Directory {
                return Err(Error::Gate(format!(
                    "cleanup identity changed for {}; leaking the private tree rather than deleting a substitute",
                    self.path.parent().unwrap().display()
                )));
            }
            remove_owned_tree(&self.parent_dir, Path::new(""))?;
            let (visible, kind) = entry_identity(&self.storage_dir, &c_name).map_err(|error| {
                Error::Gate(format!(
                    "cleanup cannot revalidate private parent after emptying it: {error}; leaking it"
                ))
            })?;
            if visible != self.parent_identity || kind != FileType::Directory {
                return Err(Error::Gate(
                    "cleanup private-parent identity changed; leaking it rather than deleting a substitute"
                        .to_owned(),
                ));
            }
            rustix::fs::unlinkat(&self.storage_dir, &c_name, AtFlags::REMOVEDIR)
                .map_err(io::Error::from)?;
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct InstrumentedReader<'a> {
        inner: io::Cursor<&'a [u8]>,
        largest_request: usize,
        read_count: usize,
    }

    impl Read for InstrumentedReader<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.largest_request = self.largest_request.max(buf.len());
            self.read_count += 1;
            self.inner.read(buf)
        }
    }

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
    fn hash_reader_uses_bounded_chunks_and_matches_the_original_hash() {
        let bytes: Vec<u8> = (0..(HASH_BUFFER_SIZE * 32 + 17))
            .map(|index| (index % 251) as u8)
            .collect();
        let mut reader = InstrumentedReader {
            inner: io::Cursor::new(bytes.as_slice()),
            largest_request: 0,
            read_count: 0,
        };

        let streamed = hash_reader(&mut reader).unwrap();

        assert_eq!(streamed, hash_bytes(&bytes));
        assert!(
            reader.largest_request <= HASH_BUFFER_SIZE,
            "fingerprinting requested {} bytes at once",
            reader.largest_request
        );
        assert!(
            reader.read_count > 2,
            "the multi-buffer input must require chunked reads"
        );
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
    fn temp_world_uses_private_unique_parents_and_cleanup_removes_them() {
        let src = fixture_world("temp");
        let storage = sibling(&src, "storage");
        cleanup(&storage);
        fs::create_dir_all(&storage).unwrap();

        let mut first = TempWorld::create(&src, &storage).unwrap();
        let mut second = TempWorld::create(&src, &storage).unwrap();
        assert_ne!(
            first.path().parent(),
            second.path().parent(),
            "each disposable copy needs an unpredictable per-run parent"
        );
        assert!(
            first
                .path()
                .parent()
                .unwrap()
                .starts_with(storage.canonicalize().unwrap()),
            "the private parent must stay beneath the requested storage root"
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(first.path().parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "the per-run parent must be owner-only"
        );
        assert_copy_equals_source(&hash_tree(&src).unwrap(), &first.hash_tree().unwrap())
            .expect("the fresh copy is faithful");

        let first_parent = first.path().parent().unwrap().to_owned();
        let second_parent = second.path().parent().unwrap().to_owned();
        first.cleanup().unwrap();
        second.cleanup().unwrap();
        assert!(!first_parent.exists(), "cleanup must remove the first copy");
        assert!(
            !second_parent.exists(),
            "cleanup must remove the second copy"
        );
        // Idempotent: a second cleanup is a no-op, and Drop is a no-op too.
        first.cleanup().unwrap();

        cleanup(&src);
        cleanup(&storage);
    }

    #[test]
    fn temp_world_refuses_aliasing_destinations() {
        let src = fixture_world("alias");
        // Storage may not be the source itself, nor nested inside it.
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

    #[test]
    fn prospective_storage_beneath_source_is_rejected_before_creation() {
        let src = fixture_world("prospective-containment");
        let storage = src.join("missing/scenario-loaded-world");
        assert!(!storage.exists());

        let err = validate_prospective_storage(&src, &storage).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("contained by source"),
            "a missing destination beneath RIVET_WORLD_SRC must be rejected: {err}"
        );
        assert!(
            !src.join("missing").exists(),
            "prospective validation must not create any path beneath the configured source"
        );

        cleanup(&src);
    }

    #[test]
    fn temp_world_refuses_source_nested_under_stale_storage_without_mutation() {
        let storage = std::env::temp_dir().join(format!(
            "rivet-load-world-reverse-containment-{}",
            std::process::id()
        ));
        cleanup(&storage);
        let source = storage.join("copied-world/launcher-save");
        fs::create_dir_all(&source).unwrap();
        let sentinel = source.join("source-sentinel.txt");
        fs::write(&sentinel, b"launcher source must survive").unwrap();
        fs::write(storage.join("stale.txt"), b"stale destination evidence").unwrap();

        let err = TempWorld::create(&source, &storage).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("contains"),
            "reverse containment must fail as a safety gate: {err}"
        );
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"launcher source must survive",
            "the nested source sentinel must be byte-identical after rejection"
        );
        assert_eq!(
            fs::read(storage.join("stale.txt")).unwrap(),
            b"stale destination evidence",
            "rejection must happen before any stale-storage cleanup"
        );

        cleanup(&storage);
    }

    #[cfg(unix)]
    #[test]
    fn fresh_private_copy_ignores_stale_predictable_path_alias() {
        let src = fixture_world("stale-predictable-alias");
        let storage = sibling(&src, "stale-predictable-storage");
        cleanup(&storage);
        fs::create_dir_all(&storage).unwrap();
        let stale = storage.join("copied-world");
        std::os::unix::fs::symlink(&src, &stale).unwrap();
        let source_before = hash_tree(&src).unwrap();

        let mut temp = TempWorld::create(&src, &storage).unwrap();
        assert_ne!(
            temp.path(),
            stale,
            "the copy must not reuse the formerly predictable destination"
        );
        assert!(
            fs::symlink_metadata(&stale)
                .unwrap()
                .file_type()
                .is_symlink(),
            "creating the fresh copy must not remove or follow stale caller-owned storage"
        );
        assert_copy_equals_source(&source_before, &temp.hash_tree().unwrap()).unwrap();
        assert_source_unchanged(&source_before, &hash_tree(&src).unwrap()).unwrap();

        temp.cleanup().unwrap();
        fs::remove_file(stale).unwrap();
        cleanup(&src);
        cleanup(&storage);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn retained_descriptor_defeats_visible_world_path_replacement() {
        let src = fixture_world("descriptor-replacement");
        let storage = sibling(&src, "descriptor-storage");
        cleanup(&storage);
        fs::create_dir_all(&storage).unwrap();
        let mut temp = TempWorld::create(&src, &storage).unwrap();
        let visible_world = temp.path().to_owned();
        let displaced_world = visible_world.with_file_name("displaced-world");

        fs::rename(&visible_world, &displaced_world).unwrap();
        std::os::unix::fs::symlink(&src, &visible_world).unwrap();
        fs::write(displaced_world.join("level.dat"), b"retained copy").unwrap();

        assert_eq!(
            fs::read(visible_world.join("level.dat")).unwrap(),
            [0u8, 1, 2, 3],
            "the adversarial visible replacement points at the source"
        );
        let child = std::process::Command::new("/bin/cat")
            .arg(temp.server_path().join("level.dat"))
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "child must consume the inherited fd"
        );
        assert_eq!(
            child.stdout, b"retained copy",
            "the child must read the retained copy, not the replacement pathname"
        );
        assert_eq!(
            fs::read(src.join("level.dat")).unwrap(),
            [0u8, 1, 2, 3],
            "the source must remain unchanged"
        );

        let err = temp.cleanup().unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("cleanup refused"),
            "cleanup must leak rather than follow the substituted world entry: {err}"
        );
        fs::remove_file(&visible_world).unwrap();
        fs::rename(&displaced_world, &visible_world).unwrap();
        temp.cleanup().unwrap();
        cleanup(&src);
        cleanup(&storage);
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_leaks_when_private_parent_path_is_substituted() {
        let src = fixture_world("cleanup-parent-substitution");
        let storage = sibling(&src, "cleanup-parent-storage");
        cleanup(&storage);
        fs::create_dir_all(&storage).unwrap();
        let mut temp = TempWorld::create(&src, &storage).unwrap();
        let visible_parent = temp.path().parent().unwrap().to_owned();
        let displaced_parent = storage.join("displaced-owned-parent");

        fs::rename(&visible_parent, &displaced_parent).unwrap();
        fs::create_dir(&visible_parent).unwrap();
        let substitute_sentinel = visible_parent.join("launcher-source-like-sentinel.dat");
        fs::write(&substitute_sentinel, b"must never be deleted").unwrap();

        let err = temp.cleanup().unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("identity changed"),
            "private-parent substitution must fail closed: {err}"
        );
        assert_eq!(
            fs::read(&substitute_sentinel).unwrap(),
            b"must never be deleted",
            "cleanup must not recurse into or delete a substituted directory"
        );
        assert!(
            displaced_parent.join("world/level.dat").exists(),
            "the original owned tree is intentionally leaked when its visible identity changes"
        );

        drop(temp);
        assert_eq!(
            fs::read(&substitute_sentinel).unwrap(),
            b"must never be deleted",
            "Drop must preserve the same fail-closed identity boundary"
        );
        cleanup(&visible_parent);
        cleanup(&displaced_parent);
        cleanup(&src);
        cleanup(&storage);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_symlink_replacement_is_never_followed_by_hashing() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let src = fixture_world("concurrent-source-entry");
        let outside = sibling(&src, "outside-sentinel");
        cleanup(&outside);
        fs::create_dir(&outside).unwrap();
        let outside_file = outside.join("secret");
        fs::write(&outside_file, b"outside content must never be read").unwrap();
        let target = src.join("racy-entry");
        let spare = src.join("racy-entry.regular");
        let link = src.join("racy-entry.link");
        fs::write(&target, b"safe in-tree content").unwrap();
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_target = target.clone();
        let worker_spare = spare.clone();
        let worker_link = link.clone();
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                let _ = fs::rename(&worker_target, &worker_spare);
                let _ = fs::rename(&worker_link, &worker_target);
                let _ = fs::rename(&worker_target, &worker_link);
                let _ = fs::rename(&worker_spare, &worker_target);
            }
        });

        let outside_hash = hash_reader(fs::File::open(&outside_file).unwrap()).unwrap();
        for _ in 0..200 {
            if let Ok(snapshot) = hash_tree(&src) {
                assert!(
                    snapshot.iter().all(|(_, entry)| {
                        !matches!(entry, EntryFingerprint::File(hash) if *hash == outside_hash)
                    }),
                    "descriptor-relative hashing must never follow the raced symlink outside the source"
                );
            }
        }
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();

        cleanup(&src);
        cleanup(&outside);
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

        fs::create_dir_all(&dest).unwrap();
        let err = TempWorld::create(&source_link, &dest).unwrap_err();
        assert!(
            matches!(err, Error::Gate(_)) && err.to_string().contains("is a symlink"),
            "the root source alias must be refused: {err}"
        );
        assert_eq!(
            fs::read_dir(&dest).unwrap().count(),
            0,
            "a refused source must not create a private copy"
        );

        fs::remove_file(&source_link).unwrap();
        cleanup(&src);
        cleanup(&dest);
    }
}

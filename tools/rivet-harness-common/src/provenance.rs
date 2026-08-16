use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

fn target_dir() -> Result<PathBuf, String> {
    let value = env::var("RIVET_CARGO_TARGET_DIR")
        .map_err(|_| "managed Cargo target directory is not exported".to_owned())?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!(
            "managed Cargo target directory is relative: {}",
            path.display()
        ));
    }
    reject_symlink_path(&path, "managed target")?;
    Ok(path)
}

fn managed_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("managed Cargo namespace variable is not exported: {name}"))
}

fn sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.rivet-provenance", path.display()))
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

fn encoded_path(path: &Path) -> String {
    base64::engine::general_purpose::STANDARD.encode(path_bytes(path))
}

fn reject_symlink_path(path: &Path, label: &str) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => current.push(component.as_os_str()),
            Component::Normal(name) => {
                current.push(name);
                match fs::symlink_metadata(&current) {
                    Ok(metadata) => {
                        if metadata.file_type().is_symlink() {
                            return Err(format!("{label} is a symlink: {}", current.display()));
                        }
                        if !metadata.is_dir() {
                            return Err(format!(
                                "{label} is not a directory: {}",
                                current.display()
                            ));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(format!("{label} {}: {error}", current.display())),
                }
            }
        }
    }
    Ok(())
}

fn managed_path(target: &Path, path: &Path) -> Result<(PathBuf, PathBuf), String> {
    if !path.is_absolute() {
        return Err(format!("deliverable path is relative: {}", path.display()));
    }
    reject_symlink_path(target, "managed target")?;
    let relative = path
        .strip_prefix(target)
        .map_err(|_| format!("deliverable is outside managed target: {}", path.display()))?;
    if relative.as_os_str().is_empty() {
        return Err(format!(
            "deliverable is the managed target directory: {}",
            path.display()
        ));
    }
    let mut parent = target.to_owned();
    let components: Vec<_> = relative.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        match component {
            Component::Normal(name) => parent.push(name),
            _ => {
                return Err(format!(
                    "deliverable path is not normalized: {}",
                    path.display()
                ));
            }
        }
        match fs::symlink_metadata(&parent) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "deliverable parent is a symlink: {}",
                        parent.display()
                    ));
                }
                if !metadata.is_dir() {
                    return Err(format!(
                        "deliverable parent is not a directory: {}",
                        parent.display()
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("deliverable parent {}: {error}", parent.display())),
        }
    }
    let metadata = fs::symlink_metadata(path);
    if let Ok(metadata) = &metadata {
        if metadata.file_type().is_symlink() {
            return Err(format!("deliverable is a symlink: {}", path.display()));
        }
        if !metadata.is_file() {
            return Err(format!(
                "deliverable is not a regular file: {}",
                path.display()
            ));
        }
        let canonical_target = target
            .canonicalize()
            .map_err(|error| format!("managed target {}: {error}", target.display()))?;
        let canonical_path = path
            .canonicalize()
            .map_err(|error| format!("deliverable {}: {error}", path.display()))?;
        if !canonical_path.starts_with(&canonical_target) {
            return Err(format!(
                "deliverable canonical path escapes managed target: {}",
                path.display()
            ));
        }
        return Ok((path.to_owned(), canonical_path));
    }
    if let Err(error) = metadata
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!("deliverable {}: {error}", path.display()));
    }
    Ok((path.to_owned(), path.to_owned()))
}

fn directory_snapshots(target: &Path, path: &Path) -> Result<Vec<(PathBuf, fs::Metadata)>, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{path:?} has no parent"))?;
    let relative = parent.strip_prefix(target).map_err(|_| {
        format!(
            "deliverable parent is outside managed target: {}",
            parent.display()
        )
    })?;
    let mut snapshots = Vec::new();
    let mut current = target.to_owned();
    let target_metadata = fs::symlink_metadata(&current)
        .map_err(|error| format!("managed target {}: {error}", target.display()))?;
    if target_metadata.file_type().is_symlink() || !target_metadata.is_dir() {
        return Err(format!(
            "managed target is not a directory: {}",
            target.display()
        ));
    }
    snapshots.push((current.clone(), target_metadata));
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(format!(
                "deliverable parent is not normalized: {}",
                parent.display()
            ));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("deliverable parent {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "deliverable parent is not a directory: {}",
                current.display()
            ));
        }
        snapshots.push((current.clone(), metadata));
    }
    Ok(snapshots)
}

#[cfg(unix)]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

fn read_regular_file_authenticated(
    path: &Path,
    target: &Path,
    label: &str,
) -> Result<Vec<u8>, String> {
    let snapshots = directory_snapshots(target, path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| format!("{label} {}: {error}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("{label} {}: {error}", path.display()))?;
    if !opened.is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    #[cfg(unix)]
    if opened.nlink() > 1 {
        return Err(format!("{label} is a hardlink: {}", path.display()));
    }
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|error| format!("{label} {}: {error}", path.display()))?;
    let current = fs::symlink_metadata(path)
        .map_err(|error| format!("{label} {}: {error}", path.display()))?;
    if current.file_type().is_symlink() || !current.is_file() || !same_file(&opened, &current) {
        return Err(format!("{label} changed during read: {}", path.display()));
    }
    let canonical_target = target
        .canonicalize()
        .map_err(|error| format!("managed target {}: {error}", target.display()))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("{label} {}: {error}", path.display()))?;
    if !canonical_path.starts_with(&canonical_target) {
        return Err(format!(
            "{label} escapes managed target: {}",
            path.display()
        ));
    }
    for (directory, expected) in snapshots {
        let actual = fs::symlink_metadata(&directory)
            .map_err(|error| format!("managed directory {}: {error}", directory.display()))?;
        if actual.file_type().is_symlink() || !actual.is_dir() || !same_file(&expected, &actual) {
            return Err(format!(
                "managed directory changed during read: {}",
                directory.display()
            ));
        }
    }
    Ok(data)
}

fn fields(path: &Path, target: &Path) -> Result<BTreeMap<String, Value>, String> {
    let value: Value = serde_json::from_slice(&read_regular_file_authenticated(
        path,
        target,
        "provenance sidecar",
    )?)
    .map_err(|error| format!("invalid provenance sidecar {}: {error}", path.display()))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid provenance sidecar object: {}", path.display()))?;
    Ok(object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect())
}

fn sha256(path: &Path, target: &Path) -> Result<String, String> {
    let data = read_regular_file_authenticated(path, target, "binary")?;
    Ok(format!("{:x}", Sha256::digest(data)))
}

pub fn verify(path: &Path) -> Result<(), String> {
    let target = target_dir()?;
    let (path, canonical_path) = managed_path(&target, path)?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("binary is not a regular file: {}", path.display()));
    }
    #[cfg(unix)]
    if metadata.nlink() > 1 {
        return Err(format!("binary is a hardlink: {}", path.display()));
    }
    let sidecar = sidecar(&path);
    let values = fields(&sidecar, &target)?;
    let expected = BTreeMap::from([
        ("version".to_owned(), Value::from(1)),
        (
            "repo_id".to_owned(),
            Value::String(managed_env("RIVET_CARGO_REPO_ID")?),
        ),
        (
            "checkout_id".to_owned(),
            Value::String(managed_env("RIVET_CARGO_CHECKOUT_ID")?),
        ),
        (
            "head".to_owned(),
            Value::String(managed_env("RIVET_CARGO_HEAD")?),
        ),
        (
            "state_digest".to_owned(),
            Value::String(managed_env("RIVET_CARGO_STATE_DIGEST")?),
        ),
        (
            "target_b64".to_owned(),
            Value::String(encoded_path(&target)),
        ),
        (
            "path_b64".to_owned(),
            Value::String(encoded_path(&canonical_path)),
        ),
        ("sha256".to_owned(), Value::String(sha256(&path, &target)?)),
    ]);
    if values != expected {
        return Err(format!("{} has invalid provenance", path.display()));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn authenticated_reads_reject_final_and_ancestor_symlinks() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("rivet-provenance-test-{unique}"));
        let target = root.join("target");
        let debug = target.join("debug");
        let binary = debug.join("binary");
        let outside = root.join("outside");
        let backup = root.join("debug-backup");
        fs::create_dir_all(&debug).expect("create managed directories");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(&binary, b"inside").expect("write managed binary");
        let final_link = debug.join("final-link");
        symlink(&binary, &final_link).expect("create final symlink");
        assert!(read_regular_file_authenticated(&final_link, &target, "binary").is_err());
        fs::remove_file(&final_link).expect("remove final symlink");
        fs::rename(&debug, &backup).expect("move managed directory");
        symlink(&outside, &debug).expect("create ancestor symlink");
        fs::write(outside.join("binary"), b"outside").expect("write outside binary");
        assert!(read_regular_file_authenticated(&debug.join("binary"), &target, "binary").is_err());
        fs::remove_file(&debug).expect("remove ancestor symlink");
        fs::rename(&backup, &debug).expect("restore managed directory");
        fs::remove_dir_all(root).expect("remove test tree");
    }
}

pub fn resolve(name: &str, override_var: &str) -> Result<PathBuf, String> {
    let target = target_dir()?;
    if let Ok(value) = env::var(override_var) {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(format!(
                "{override_var} must be absolute: {}",
                path.display()
            ));
        }
        verify(&path)?;
        return Ok(path);
    }
    let path = target.join("debug").join(name);
    verify(&path)?;
    Ok(path)
}

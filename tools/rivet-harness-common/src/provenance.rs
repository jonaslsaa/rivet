use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

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

fn sha256(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(data)))
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

fn fields(path: &Path) -> Result<BTreeMap<String, Value>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("provenance {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "provenance sidecar is not a regular file: {}",
            path.display()
        ));
    }
    #[cfg(unix)]
    if metadata.nlink() > 1 {
        return Err(format!(
            "provenance sidecar is a hardlink: {}",
            path.display()
        ));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("provenance {}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("invalid provenance sidecar {}: {error}", path.display()))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid provenance sidecar object: {}", path.display()))?;
    Ok(object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect())
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
    let values = fields(&sidecar)?;
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
        ("sha256".to_owned(), Value::String(sha256(&path)?)),
    ]);
    if values != expected {
        return Err(format!("{} has invalid provenance", path.display()));
    }
    Ok(())
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

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

fn fields(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("provenance {}: {error}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect())
}

pub fn verify(path: &Path) -> Result<(), String> {
    let target = target_dir()?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let values = fields(&sidecar(path))?;
    let expected = [
        ("version", "1".to_owned()),
        ("repo_id", managed_env("RIVET_CARGO_REPO_ID")?),
        ("checkout_id", managed_env("RIVET_CARGO_CHECKOUT_ID")?),
        ("head", managed_env("RIVET_CARGO_HEAD")?),
        ("state_digest", managed_env("RIVET_CARGO_STATE_DIGEST")?),
        ("target", target.to_string_lossy().into_owned()),
        ("path", canonical_path.to_string_lossy().into_owned()),
        ("sha256", sha256(path)?),
    ];
    for (key, expected_value) in expected {
        if values.get(key) != Some(&expected_value) {
            return Err(format!("{} has invalid {key} provenance", path.display()));
        }
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
        if !path.is_file() {
            return Err(format!("{override_var} is not a file: {}", path.display()));
        }
        verify(&path)?;
        return Ok(path);
    }
    let path = target.join("debug").join(name);
    if !path.is_file() {
        return Err(format!("managed binary is missing: {}", path.display()));
    }
    verify(&path)?;
    Ok(path)
}

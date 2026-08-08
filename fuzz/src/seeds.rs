//! Committed-seed discovery for the deterministic seed regressions.
//!
//! cargo-fuzz (0.13.x) never reads `fuzz/seeds/` automatically — a plain
//! `cargo fuzz run <target>` only reads (and writes) `fuzz/corpus/<target>/`
//! (see `fuzz/README.md`), which is why `seed_corpus.sh` exists to copy seeds
//! into the corpus. The regressions are the deterministic complement: the seed
//! files are discovered here and fed through the same target bodies the fuzzer
//! invokes, so a seed that stops parsing, changes behavior, or trips a
//! non-faithful panic fails `cargo test -p rivet-fuzz`.

use std::fs;
use std::path::{Path, PathBuf};

/// `fuzz/seeds/<target>`, resolved from `CARGO_MANIFEST_DIR` so the tests run
/// deterministically from any working directory.
pub fn seed_dir(target: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("seeds")
        .join(target)
}

/// The committed seed files for `target`, in deterministic (sorted) order.
pub fn seed_paths(target: &str) -> Vec<PathBuf> {
    let dir = seed_dir(target);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read seeds dir {}: {e}", dir.display()))
        .map(|entry| entry.expect("read seed entry").path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    paths
}

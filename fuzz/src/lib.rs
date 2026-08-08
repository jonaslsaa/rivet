//! Shared fuzz-target logic for the rivet parser crates (`rivet-nbt`,
//! `rivet-serialization`, `rivet-util`).
//!
//! Each `fuzz_targets/*.rs` bin is a thin shim that hands raw bytes to the
//! corresponding body in [`targets`] — so libFuzzer drives the exact same code
//! the deterministic seed regressions exercise — and [`seeds`] plus the
//! `regress` tests feed every committed `fuzz/seeds/<target>/` file through
//! those bodies on `cargo test -p rivet-fuzz`.

pub mod common;
pub mod seeds;
pub mod targets;

#[cfg(test)]
mod regress;

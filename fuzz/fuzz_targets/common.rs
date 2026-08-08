//! Shared helpers for the binary-NBT fuzz targets.
//!
//! Thin re-export of the library's `rivet_fuzz::common::guarded` — the panic
//! filter the binary-NBT and compressed-map targets run under so a faithful
//! Java panic (negative list length, oversized array, accounter quota/depth,
//! compressed-map out-of-bounds) is swallowed instead of aborting the fuzzer.
//! The bodies of those targets live in `rivet_fuzz::targets`, and the
//! deterministic seed regressions classify panics with the same table, so the
//! table has exactly one definition — here, in the library.
//!
//! See the library module for the full doc: `fuzz/src/common.rs`.

pub use rivet_fuzz::common::guarded;

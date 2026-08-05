//! Vanilla data registries for Rivet.
//!
//! Contents are **generated, not hand-typed** (PORTING.md). Run
//! `tools/rivet-codegen generate` and commit the output. The generated tables
//! are compile-time (`phf` maps and `&'static` arrays) and gated behind the
//! `blocks` cargo feature so consumers only pay for the registries they use.

/// Compile-time block registry + block-state tables.
///
/// Gated behind the `blocks` feature; empty when the feature is off.
/// Submodule wiring lives in the generated `generated/mod.rs`.
#[cfg(feature = "blocks")]
pub mod generated;

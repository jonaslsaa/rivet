//! Minecraft protocol layer: `net.minecraft.network`.
//!
//! Packet-ID tables are **generated, not hand-typed** (PORTING.md): run
//! `tools/rivet-codegen generate` and commit the output. The tables live in
//! `src/generated/`, are compile-time (`phf` maps + `&'static` arrays + enums),
//! and are gated behind the `packets` cargo feature so consumers only pay for
//! the protocol data they use.

/// Compile-time packet-ID tables (protocol state -> flow -> packet name -> id).
///
/// Gated behind the `packets` feature; empty when the feature is off.
/// Submodule wiring lives in the generated `generated/mod.rs`.
#[cfg(feature = "packets")]
pub mod generated;

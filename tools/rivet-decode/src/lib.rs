//! Decode/capture harness for the ported serverbound play packets (issue #97).
//!
//! A reusable, deterministic decode/capture tool over the `rivet-protocol`
//! serverbound play slice:
//!   - `protocol` — the vanilla-id dispatch table (69 entries; the nine ported
//!     packets decode with real codecs, the rest are raw passthrough).
//!   - `frame`    — varint21 framing.
//!   - `corpus`   — capture corpus directory + provenance manifest (sha256).
//!   - `mutate`   — the hostile-input mutation matrix.
//!   - `frag`     — fragmentation / coalescing checks.
//!
//! The binary (`main.rs`) exposes the `decode` / `verify` / `mutate` / `frag`
//! subcommands with exit codes 0 / 1 / 3 (matching `rivet-oracle`'s gate
//! contract).

pub mod advancement;
pub mod corpus;
pub mod frag;
pub mod frame;
pub mod mutate;
pub mod protocol;

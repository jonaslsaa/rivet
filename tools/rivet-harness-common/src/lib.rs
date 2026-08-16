//! Shared primitives for Rivet's E2E harness tools.
//!
//! The harness tools each boot real servers, reserve ports, parse
//! machine-readable JSONL transcripts, report a machine-stable exit contract,
//! and verify attested Cargo artifacts. This crate is where those policies
//! live once, so the tools cannot drift apart. The current consumers are
//! `rivet-capture` and `rivet-client`'s `run-scenario` binary, which previously
//! duplicated the boot loop, kill-on-drop, port reservation, JSONL line policy,
//! and artifact checks across their modules. Modules:
//!
//! - [`exit`] — the 0 PASS / 1 FAIL / 3 UNVERIFIED exit contract.
//! - [`server`] — child-process boot lifecycle with kill-on-drop cleanup.
//! - [`port`] — held localhost port reservations (no bind-drop-boot race).
//! - [`transcript`] — strict JSONL parsing (malformed / duplicate-terminal /
//!   missing-terminal failures).
//! - [`negative`] — named-path negative-control helpers.
//! - [`timing`] — wall-clock budgets the client and scenario runner must agree
//!   on (keepalive settle, login headroom, dwell/move timeout reservations).
//!
//! Only the *strict E2E primitives* live here. Consumer-specific orchestration
//! (which command to spawn, Paper's clean-save marker, the rivet-server
//! `RIVET_READY` line, fixture corruption) stays in the calling tool.

pub mod exit;
pub mod negative;
pub mod port;
pub mod provenance;
pub mod server;
pub mod timing;
pub mod transcript;

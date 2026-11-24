//! Shared std-only primitives for Rivet's E2E harness tools.
//!
//! The harness tools each boot real servers, reserve ports, parse
//! machine-readable JSONL transcripts, and report a machine-stable exit
//! contract. This crate is where those primitives live once, so the tools
//! cannot drift apart. The current consumers are `rivet-capture` and
//! `rivet-client`'s `run-scenario` binary, which previously duplicated the
//! boot loop, kill-on-drop, port reservation, and JSONL line policy across
//! their `server.rs`/`main.rs` modules. `rivet-oracle` is a future consumer:
//! it has not been migrated yet, but the surface is std-only, so adopting it
//! adds no external (non-std) packages to the consumer's dependency tree — the
//! only new lockfile entry is the crate itself (serde_json is a dev-dependency,
//! never a runtime dependency of a consumer).
//!
//! The crate is std-only by design — see `Cargo.toml`. Modules:
//!
//! - [`exit`] — the 0 PASS / 1 FAIL / 3 UNVERIFIED exit contract.
//! - [`closure`] — declared-vs-executed counts and the row status they imply
//!   (zero executed is UNVERIFIED; dropped inputs FAIL).
//! - [`oracle_client`] — boot-once JSON-Lines oracle client: id correlation,
//!   deadlock-free request pipelining, and the tamper hook for negatives.
//! - [`canon`] — canonical IEEE-754 bit-hex float encoding (D14 NaN rules).
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

pub mod canon;
pub mod closure;
pub mod exit;
pub mod negative;
pub mod oracle_client;
pub mod port;
pub mod server;
pub mod timing;
pub mod transcript;

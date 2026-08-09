//! The env-gated tick-thread movement trace (issue #53, M1 disjoint
//! server/client foundation).
//!
//! `RIVET_TRACE_MOVEMENT=1` turns on a set of stable, single-message,
//! info-level `tracing` records emitted from the tick-owned session handling
//! path ([`PlayerSessionManager`](crate::server::player::session::PlayerSessionManager)),
//! so a server run with the env var set produces a machine-readable movement
//! audit on stderr without touching stdout (the `RIVET_READY` machine channel)
//! and without adding any shared state to the server.
//!
//! # Gating
//!
//! The gate is a process-wide one-time [`OnceLock`]: [`movement_trace_enabled`]
//! reads `RIVET_TRACE_MOVEMENT` exactly once, treating any value as enabled.
//! Every trace call site evaluates it once and becomes a compile-time-skippable
//! no-op when unset — no per-frame env lookup, no allocation, no observable
//! behavior change on a default boot. There is deliberately no stdout marker
//! and no channel/`Arc`/`Mutex`: the records are plain `tracing::info!` events
//! flowing through the existing subscriber, so zero new shared state exists.
//! Tests pin the gate with [`set_trace_gate_for_tests`] (the one-time read is
//! unchanged in production; the setter only overwrites the cached value). The
//! tracing-subscriber recording machinery the tests assert against lives in the
//! dev-dependency crate `rivet-test-support`, not in this library.
//!
//! # Records
//!
//! Each record is a single event whose message is a stable `SCHEMA`-prefixed
//! tag (parseable by grepping stderr); the fields are the exact values the tick
//! thread owns at the emission point:
//!
//! - `RIVET_TELEPORT_ACK` — one per `accept_teleportation` frame whose body
//!   parsed, with `outcome=accepted|ignored|invalid` and `id` the echoed
//!   `awaitingTeleport` id. `accepted` carries the awaited position snapped
//!   into the player; `invalid` also emits `reason=invalid_player_movement`.
//! - `RIVET_MOVE_ACCEPTED` — only on the full post-ack accepted movement path
//!   (not the teleport-pending rotation-only snap, not the invalid-value gate),
//!   carrying the exact clamped/wrapped `x/y/z/y_rot/x_rot` values
//!   `abs_snap_to` wrote into the tick-owned `ServerPlayer`.
//! - `RIVET_SESSION_END` — from `prune_lost` only for the EOF / Timeout /
//!   InboundOverflow disconnect paths, carrying the final authoritative
//!   position + rotation and the session's move-frame counts.
//!
//! `accepted_frames` is the per-session count of post-ack accepted movements
//! (each accepted move increments it); `move_frames_seen` is the manager-wide
//! parsed-move/ack counter, so a session-end record lets a consumer sum the
//! authoritative displacement from the `RIVET_MOVE_ACCEPTED` trail and compare
//! it against the final position.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use super::network::connection_id::ConnectionId;
use super::network::packet_listener::DisconnectReason;

/// The env var that enables the movement trace (`RIVET_TRACE_MOVEMENT=1`).
pub const TRACE_MOVEMENT_ENV: &str = "RIVET_TRACE_MOVEMENT";

/// The stable message tags each record's single message is prefixed with (the
/// schema is `RIVET_* <key>=<value> ...`; the exact fields are pinned by the
/// `movement_trace` integration test's parser).
pub const TAG_TELEPORT_ACK: &str = "RIVET_TELEPORT_ACK";
pub const TAG_MOVE_ACCEPTED: &str = "RIVET_MOVE_ACCEPTED";
pub const TAG_SESSION_END: &str = "RIVET_SESSION_END";

/// The teleport-ack outcome string embedded in a `RIVET_TELEPORT_ACK` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// `packet.getId() == awaitingTeleport` with a pending position: the player
    /// snapped to the awaited spawn.
    Accepted,
    /// A stale/wrong id: silent no-op.
    Ignored,
    /// A matching id with no pending position: the Paper
    /// `invalid_player_movement` kick.
    Invalid,
}

impl AckOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AckOutcome::Accepted => "accepted",
            AckOutcome::Ignored => "ignored",
            AckOutcome::Invalid => "invalid",
        }
    }
}

/// The cached gate value, shared by the reader and the test setter. The
/// `OnceLock` keeps the production read a true one-time env lookup (every later
/// call is a load, never an env access); the inner `AtomicBool` lets tests pin
/// either state via [`set_trace_gate_for_tests`] without racing the env or the
/// cache.
static ENABLED: OnceLock<AtomicBool> = OnceLock::new();

/// One-time evaluation of the `RIVET_TRACE_MOVEMENT` env gate. Reads the env
/// exactly once per process; any value (including `RIVET_TRACE_MOVEMENT=0`)
/// enables the trace — the env var is a boolean "present or not" switch, not a
/// level knob. Unset means the trace never runs (zero behavior change).
pub fn movement_trace_enabled() -> bool {
    ENABLED
        .get_or_init(|| AtomicBool::new(std::env::var_os(TRACE_MOVEMENT_ENV).is_some()))
        .load(Ordering::Relaxed)
}

/// Pin the gate to a concrete state for tests (unit and live-TCP). The trace is
/// diagnostic infrastructure; a test needs to assert both the enabled records
/// and the zero-behavior-when-unset without depending on the runner's env. The
/// store overwrites whatever a parallel test initialized, so each test controls
/// its own state deterministically. The production path never calls this.
pub fn set_trace_gate_for_tests(enabled: bool) {
    ENABLED
        .get_or_init(|| AtomicBool::new(enabled))
        .store(enabled, Ordering::Relaxed);
}

/// Emit a `RIVET_TELEPORT_ACK` record for one parsed `accept_teleportation`
/// frame. `awaited` is the position snapped on the accepted path (the spawn the
/// player landed on); the ignored/invalid paths never use it. A no-op unless
/// the env gate is on.
pub fn trace_teleport_ack(
    id: ConnectionId,
    ack_id: i32,
    outcome: AckOutcome,
    awaited: Option<[f64; 3]>,
) {
    if !movement_trace_enabled() {
        return;
    }
    match (outcome, awaited) {
        (AckOutcome::Accepted, Some([x, y, z])) => tracing::info!(
            %id, ack_id, outcome = AckOutcome::Accepted.as_str(), x, y, z,
            "{}", TAG_TELEPORT_ACK,
        ),
        (AckOutcome::Ignored, _) => tracing::info!(
            %id, ack_id, outcome = AckOutcome::Ignored.as_str(),
            "{}", TAG_TELEPORT_ACK,
        ),
        (AckOutcome::Invalid, _) => tracing::info!(
            %id, ack_id, outcome = AckOutcome::Invalid.as_str(),
            reason = "invalid_player_movement",
            "{}", TAG_TELEPORT_ACK,
        ),
        // `Accepted` always carries the awaited position from the ack machine;
        // a `None` here is unreachable and emits nothing.
        (AckOutcome::Accepted, None) => {}
    }
}

/// Emit a `RIVET_MOVE_ACCEPTED` record on the post-ack accepted movement path,
/// carrying the exact clamped/wrapped values snapped into the tick-owned
/// `ServerPlayer` and the session's accepted-move frame counter. A no-op unless
/// the env gate is on.
pub fn trace_move_accepted(
    id: ConnectionId,
    x: f64,
    y: f64,
    z: f64,
    y_rot: f32,
    x_rot: f32,
    accepted_frames: usize,
) {
    if !movement_trace_enabled() {
        return;
    }
    tracing::info!(
        %id, x, y, z, y_rot, x_rot, accepted_frames,
        "{}", TAG_MOVE_ACCEPTED,
    );
}

/// Whether a disconnect reason is one the session-end trace reports. Only the
/// EOF / Timeout / InboundOverflow paths — the client going away or being kicked
/// by the liveness/anti-flood machines — carry a meaningful final authoritative
/// position. A deliberate server-side close (malformed frame, overflow, invalid
/// movement, unsupported, request handled, shutdown) is not a movement trace
/// endpoint.
pub fn is_traced_disconnect(reason: &DisconnectReason) -> bool {
    matches!(
        reason,
        DisconnectReason::EndOfStream
            | DisconnectReason::Timeout
            | DisconnectReason::InboundOverflow(_)
    )
}

/// Emit a `RIVET_SESSION_END` record for a session pruned after an EOF / Timeout
/// / InboundOverflow close, carrying the final authoritative position + rotation
/// and the session's movement counts. A no-op unless the env gate is on.
///
/// The nine arguments are the record's schema fields (each maps 1:1 onto a
/// field of the emitted `tracing` event), so the excess over clippy's default
/// limit is inherent to the schema rather than a refactorable arity smell —
/// same as the connection functions in `server_connection_listener.rs`.
#[allow(clippy::too_many_arguments)]
pub fn trace_session_end(
    id: ConnectionId,
    reason: DisconnectReason,
    x: f64,
    y: f64,
    z: f64,
    y_rot: f32,
    x_rot: f32,
    accepted_frames: usize,
    move_frames_seen: usize,
) {
    if !movement_trace_enabled() {
        return;
    }
    tracing::info!(
        %id, %reason, x, y, z, y_rot, x_rot, accepted_frames, move_frames_seen,
        "{}", TAG_SESSION_END,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The setter pins both gate states and the cached value is stable across
    /// calls. The genuine one-time `RIVET_TRACE_MOVEMENT` env read is a
    /// production-only path in `movement_trace_enabled`; this test asserts the
    /// cache snapshot directly because it must not depend on the runner's env.
    #[test]
    fn set_trace_gate_is_stable_across_calls() {
        set_trace_gate_for_tests(true);
        assert!(movement_trace_enabled(), "pinned enabled");
        assert!(movement_trace_enabled(), "the gate is stable across calls");
        set_trace_gate_for_tests(false);
        assert!(!movement_trace_enabled(), "pinned disabled");
        assert!(!movement_trace_enabled(), "the gate is stable across calls");
        set_trace_gate_for_tests(true);
    }

    /// The record tags are the exact stable strings the trace consumer greps
    /// for — a rename here silently breaks the schema, so the constants are
    /// pinned.
    #[test]
    fn record_tags_are_stable() {
        assert_eq!(TAG_TELEPORT_ACK, "RIVET_TELEPORT_ACK");
        assert_eq!(TAG_MOVE_ACCEPTED, "RIVET_MOVE_ACCEPTED");
        assert_eq!(TAG_SESSION_END, "RIVET_SESSION_END");
        assert_eq!(TRACE_MOVEMENT_ENV, "RIVET_TRACE_MOVEMENT");
    }

    /// The `accepted|ignored|invalid` outcome strings are the schema tokens the
    /// consumer matches on.
    #[test]
    fn ack_outcome_strings_are_stable() {
        assert_eq!(AckOutcome::Accepted.as_str(), "accepted");
        assert_eq!(AckOutcome::Ignored.as_str(), "ignored");
        assert_eq!(AckOutcome::Invalid.as_str(), "invalid");
    }

    /// Minimal repro for the cross-test callsite-interest poisoning: installing a
    /// subscriber, emitting a `RIVET_*` event, dropping the subscriber, then
    /// reinstalling and emitting the *same* callsite again must still capture.
    #[test]
    fn reinstall_still_captures_same_callsite() {
        let sub = rivet_test_support::install_for_tests(super::set_trace_gate_for_tests, true);
        trace_teleport_ack(
            ConnectionId(1),
            1,
            AckOutcome::Accepted,
            Some([0.0, -63.0, 0.0]),
        );
        assert_eq!(
            sub.recorder
                .snapshot()
                .iter()
                .filter(|r| r.tag == TAG_TELEPORT_ACK)
                .count(),
            1,
            "first install captures the event"
        );
        drop(sub);
        let sub2 = rivet_test_support::install_for_tests(super::set_trace_gate_for_tests, true);
        trace_teleport_ack(
            ConnectionId(1),
            1,
            AckOutcome::Accepted,
            Some([0.0, -63.0, 0.0]),
        );
        assert_eq!(
            sub2.recorder
                .snapshot()
                .iter()
                .filter(|r| r.tag == TAG_TELEPORT_ACK)
                .count(),
            1,
            "reinstall still captures the same callsite"
        );
    }
}

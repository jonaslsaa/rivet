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
//! unchanged in production; the setter only overwrites the cached value).
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

    /// The gate is a stable snapshot: a call after the first must return the
    /// same bool (the `OnceLock` caches the value; a later `set_var` would be
    /// ignored). The test setter pins both states deterministically — the
    /// enabled records and the zero-behavior-when-unset — without depending on
    /// the runner's env.
    #[test]
    fn gate_reads_env_exactly_once() {
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
        let sub = super::test_support::install_for_tests(true);
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
        let sub2 = super::test_support::install_for_tests(true);
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

/// Test-only tracing subscriber infrastructure shared by the movement-trace
/// unit tests (`player::session`) and the live-TCP integration test
/// (`tests/movement_trace.rs`). Nothing in production reads trace records back —
/// the trace is fire-and-forget `tracing::info!` events — so this is pinned
/// behind a doc-hide: it exists so the tests can assert the schema (fields and
/// their types) instead of scraping rendered log lines.
#[doc(hidden)]
pub mod test_support {
    use std::collections::BTreeMap;
    use std::fmt;
    use std::sync::{Arc, Mutex};

    use tracing::Subscriber;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::Context;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::registry::LookupSpan;

    /// Serializes the trace tests in a binary: the env gate is a process-wide
    /// one-time value, so gate-sensitive tests must not run concurrently with
    /// each other. Production code never touches this lock. The lock is
    /// recovered from poisoning so a single failing trace test does not cascade
    /// into every later trace test.
    pub static TRACE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        TRACE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One captured trace record: the `RIVET_*` message tag and the raw field
    /// name → value pairs (each value stringified by the visitor).
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct TraceRecord {
        pub tag: String,
        pub fields: BTreeMap<String, String>,
    }

    impl TraceRecord {
        pub fn field(&self, name: &str) -> Option<&str> {
            self.fields.get(name).map(String::as_str)
        }
    }

    /// A shared sink the recording layer appends captured records to.
    #[derive(Clone, Default)]
    pub struct Recorder {
        records: Arc<Mutex<Vec<TraceRecord>>>,
    }

    impl Recorder {
        pub fn snapshot(&self) -> Vec<TraceRecord> {
            self.records.lock().expect("recorder lock poisoned").clone()
        }

        pub fn clear(&self) {
            self.records.lock().expect("recorder lock poisoned").clear();
        }
    }

    /// A [`Visit`] that captures every field of a tracing event as a string,
    /// keyed by field name. The typed record methods are all overridden so the
    /// schema's value types are observable (a bare `record_debug` only would
    /// collapse them); `DisplayValue`/`DebugValue` (`%id`, `%reason`) land in
    /// `record_debug` and render as their Display/Debug text.
    struct FieldCollector<'a> {
        fields: &'a mut BTreeMap<String, String>,
    }

    impl Visit for FieldCollector<'_> {
        fn record_f64(&mut self, field: &Field, value: f64) {
            self.fields
                .insert(field.name().to_owned(), value.to_string());
        }

        fn record_i64(&mut self, field: &Field, value: i64) {
            self.fields
                .insert(field.name().to_owned(), value.to_string());
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.fields
                .insert(field.name().to_owned(), value.to_string());
        }

        fn record_bool(&mut self, field: &Field, value: bool) {
            self.fields
                .insert(field.name().to_owned(), value.to_string());
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.fields
                .insert(field.name().to_owned(), value.to_owned());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.fields
                .insert(field.name().to_owned(), format!("{value:?}"));
        }
    }

    /// A [`Layer`] that captures every event whose message is a `RIVET_*` tag
    /// into a [`Recorder`], with its structured fields. The message field (the
    /// tag) is stripped out into `TraceRecord::tag`.
    #[derive(Clone, Default)]
    pub struct RecordingLayer {
        recorder: Recorder,
    }

    impl<S> Layer<S> for RecordingLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut fields = BTreeMap::new();
            event.record(&mut FieldCollector {
                fields: &mut fields,
            });
            let Some(tag) = fields.remove("message") else {
                return;
            };
            if tag.starts_with("RIVET_") {
                self.recorder
                    .records
                    .lock()
                    .expect("recorder lock poisoned")
                    .push(TraceRecord { tag, fields });
            }
        }
    }

    pub fn recording_layer(recorder: &Recorder) -> RecordingLayer {
        RecordingLayer {
            recorder: recorder.clone(),
        }
    }

    /// A handle the in-process trace tests hold for their duration: the
    /// thread-local subscriber guard (keeps the recording + fmt layers
    /// installed) and the gate lock (keeps concurrent trace tests from flipping
    /// the env gate mid-test).
    pub struct TestSubscriber {
        pub recorder: Recorder,
        _guard: tracing::subscriber::DefaultGuard,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    /// Pin the env gate and install a thread-local subscriber — a recording
    /// layer for the structured assertions plus `tracing_subscriber::fmt` with
    /// the test writer, so cargo captures the rendered stderr — for the
    /// duration of the returned guard. The in-process (unit) trace tests use
    /// this; the live-TCP test installs the process-global variant so the tick
    /// OS thread's events are captured too.
    pub fn install_for_tests(enabled: bool) -> TestSubscriber {
        let lock = test_lock();
        force_multi_dispatch();
        super::set_trace_gate_for_tests(enabled);
        let recorder = Recorder::default();
        let guard = tracing::subscriber::set_default(composed_subscriber(&recorder));
        TestSubscriber {
            recorder,
            _guard: guard,
            _lock: lock,
        }
    }

    /// Pin the env gate and install the process-global subscriber, so a server's
    /// tick thread (a separate OS thread) emits into the returned recorder. Only
    /// installed once per test binary; the recorder is shared across calls so
    /// every live test observes every run.
    pub fn install_global_for_tests(enabled: bool) -> Recorder {
        static GLOBAL: std::sync::OnceLock<Recorder> = std::sync::OnceLock::new();
        let _lock = test_lock();
        force_multi_dispatch();
        super::set_trace_gate_for_tests(enabled);
        GLOBAL
            .get_or_init(|| {
                let recorder = Recorder::default();
                tracing::subscriber::set_global_default(composed_subscriber(&recorder))
                    .expect("global default installed once per test binary");
                recorder
            })
            .clone()
    }

    /// Keep tracing-core's dispatch set multi-registered for the process
    /// lifetime, so every callsite-interest rebuild evaluates against the
    /// registered dispatches instead of the single-dispatch `JustOne` fast path.
    ///
    /// That fast path re-evaluates all callsites against `get_default` — the
    /// process-global default (or `NONE` in the lib test binary) when no scoped
    /// dispatch is live on the calling thread. Between trace tests no guard is
    /// held, so `Dispatch::new` inside the next `set_default` would re-cache any
    /// already-registered `RIVET_*` callsite as `Interest::never()` and those
    /// events would be silently dropped for the rest of the process. Leaking one
    /// extra dispatch keeps `has_just_one == false`, so rebuilds iterate the
    /// registered list (which always includes the caller's own composed
    /// subscriber) and never consult the thread-local. The leaked dispatch is a
    /// bare registry — `always` interest, no layers, never observed.
    fn force_multi_dispatch() {
        static LEAKED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        LEAKED.get_or_init(|| {
            // Deliberately leak the dispatch: its registrar must outlive every
            // scoped install so the dispatch set never collapses back to one.
            std::mem::forget(tracing::Dispatch::new(tracing_subscriber::registry()));
        });
    }

    fn composed_subscriber(recorder: &Recorder) -> impl Subscriber + Send + Sync + 'static {
        tracing_subscriber::registry()
            .with(recording_layer(recorder))
            .with(
                tracing_subscriber::fmt::layer()
                    .with_test_writer()
                    .with_level(false)
                    .without_time()
                    .with_target(false),
            )
    }
}

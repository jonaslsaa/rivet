//! Test-only tracing subscriber machinery for asserting the `RIVET_*` movement
//! trace records emitted by `rivet-server` (issue #53).
//!
//! The movement trace is fire-and-forget `tracing::info!` events; nothing in
//! production reads them back. The `rivet-server` tests assert the record
//! *schema* (the message tag and the typed fields) instead of scraping rendered
//! log lines, so they install a recording subscriber. That machinery must not
//! ship in a production binary, so it lives here — `rivet-test-support` is a
//! dev-dependency of `rivet-server` and nothing production links it.
//!
//! The env gate itself is production logic in
//! `rivet_server::server::movement_trace` (a process-wide one-time read), so
//! the installers take a `set_gate` callback (the caller's
//! `movement_trace::set_trace_gate_for_tests`) and the `enabled` value; the
//! gate write happens under the same lock that serializes the gate-sensitive
//! tests, so a parallel trace test cannot flip the gate mid-install.
//!
//! Two install styles:
//! - [`install_for_tests`] installs a thread-local default and returns a guard
//!   that keeps it installed for the duration — the in-process unit tests in
//!   `server::player::session` use this.
//! - [`install_global_for_tests`] installs the process-global default exactly
//!   once per test binary, so a server's tick OS thread emits into the shared
//!   recorder — the live-TCP integration test (`tests/offline_login.rs`) uses
//!   this.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;

/// Serializes the gate-sensitive trace tests in a binary: the env gate is a
/// process-wide one-time value, so trace tests must not run concurrently with
/// each other. The lock is recovered from poisoning so a single failing trace
/// test does not cascade into every later trace test.
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

/// A [`Layer`] that captures every event whose message is a `RIVET_*` tag into
/// a [`Recorder`], with its structured fields. The message field (the tag) is
/// stripped out into `TraceRecord::tag`.
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
/// thread-local subscriber guard (keeps the recording + fmt layers installed)
/// and the gate lock (keeps concurrent trace tests from flipping the env gate
/// mid-test).
pub struct TestSubscriber {
    pub recorder: Recorder,
    _guard: tracing::subscriber::DefaultGuard,
    _lock: std::sync::MutexGuard<'static, ()>,
}

/// Pin the env gate (via the caller's `set_gate`) and install a thread-local
/// subscriber — a recording layer for the structured assertions plus
/// `tracing_subscriber::fmt` with the test writer, so cargo captures the
/// rendered stderr — for the duration of the returned guard. The in-process
/// (unit) trace tests use this.
pub fn install_for_tests(set_gate: fn(bool), enabled: bool) -> TestSubscriber {
    let lock = test_lock();
    force_multi_dispatch();
    set_gate(enabled);
    let recorder = Recorder::default();
    let guard = tracing::subscriber::set_default(composed_subscriber(&recorder));
    TestSubscriber {
        recorder,
        _guard: guard,
        _lock: lock,
    }
}

/// Pin the env gate (via the caller's `set_gate`) and install the
/// process-global subscriber, so a server's tick thread (a separate OS thread)
/// emits into the returned recorder. Only installed once per test binary; the
/// recorder is shared across calls so every live test observes every run.
pub fn install_global_for_tests(set_gate: fn(bool), enabled: bool) -> Recorder {
    static GLOBAL: OnceLock<Recorder> = OnceLock::new();
    let _lock = test_lock();
    force_multi_dispatch();
    set_gate(enabled);
    GLOBAL
        .get_or_init(|| {
            let recorder = Recorder::default();
            tracing::subscriber::set_global_default(composed_subscriber(&recorder))
                .expect("global default installed once per test binary");
            recorder
        })
        .clone()
}

/// Keep tracing-core's dispatch set multi-registered for the process lifetime,
/// so every callsite-interest rebuild evaluates against the registered
/// dispatches instead of the single-dispatch `JustOne` fast path.
///
/// That fast path re-evaluates all callsites against `get_default` — the
/// process-global default (or `NONE` in the lib test binary) when no scoped
/// dispatch is live on the calling thread. Between trace tests no guard is
/// held, so `Dispatch::new` inside the next `set_default` would re-cache any
/// already-registered `RIVET_*` callsite as `Interest::never()` and those
/// events would be silently dropped for the rest of the process. Leaking one
/// extra dispatch keeps `has_just_one == false`, so rebuilds iterate the
/// registered list (which always includes the caller's own composed subscriber)
/// and never consult the thread-local. The leaked dispatch is a bare registry —
/// `always` interest, no layers, never observed.
fn force_multi_dispatch() {
    static LEAKED: OnceLock<()> = OnceLock::new();
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

//! `net.minecraft.gametest.framework` reporters and deterministic execution
//! transcript (Paper 26.2).
//!
//! Mirrors:
//! - `TestReporter.java`
//! - `LogTestReporter.java`
//! - `JUnitLikeTestReporter.java`
//! - `GlobalTestReporter.java` (owned per-runner rather than a process-global
//!   static mutable singleton, per `OWNERSHIP.md`)
//!
//! Also provides [`GameTestExecutionTranscript`], the stable deterministic
//! execution transcript required by issue #430.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use rivet_registry::Identifier;
use rivet_registry::core::{BlockPos, Rotation};

use crate::server::gametest::exception::GameTestException;

/// Immutable snapshot of a `GameTestInfo` passed to [`TestReporter`] callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestReportEntry {
    pub id: Identifier,
    pub structure: Identifier,
    pub required: bool,
    pub rotation: Rotation,
    pub test_block_pos: Option<BlockPos>,
    pub run_time_ms: i64,
    pub tick_count: i32,
    pub error: Option<GameTestException>,
}

impl GameTestReportEntry {
    pub fn short_pos_string(&self) -> String {
        match self.test_block_pos {
            Some(pos) => format!("{}, {}, {}", pos.get_x(), pos.get_y(), pos.get_z()),
            None => "unknown".to_owned(),
        }
    }
}

/// `net.minecraft.gametest.framework.TestReporter` (Paper 26.2).
pub trait TestReporter {
    fn on_test_failed(&mut self, entry: &GameTestReportEntry);

    fn on_test_success(&mut self, entry: &GameTestReportEntry);

    fn finish(&mut self) {}
}

/// `net.minecraft.gametest.framework.LogTestReporter` (Paper 26.2).
///
/// Formats failure lines with Paper's exact required/optional wording and
/// preserves every emitted line for deterministic inspection.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LogTestReporter {
    entries: Vec<LogReportLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogReportLevel {
    Error,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogReportLine {
    pub level: LogReportLevel,
    pub message: String,
}

impl LogTestReporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[LogReportLine] {
        &self.entries
    }
}

impl TestReporter for LogTestReporter {
    fn on_test_failed(&mut self, entry: &GameTestReportEntry) {
        let test_position = entry.short_pos_string();
        let err_desc = entry
            .error
            .as_ref()
            .map(GameTestException::message)
            .unwrap_or_default();
        if entry.required {
            let msg = format!("{} failed at {}! {}", entry.id, test_position, err_desc);
            tracing::error!("{msg}");
            self.entries.push(LogReportLine {
                level: LogReportLevel::Error,
                message: msg,
            });
        } else {
            let msg = format!(
                "(optional) {} failed at {}. {}",
                entry.id, test_position, err_desc
            );
            tracing::warn!("{msg}");
            self.entries.push(LogReportLine {
                level: LogReportLevel::Warn,
                message: msg,
            });
        }
    }

    fn on_test_success(&mut self, _entry: &GameTestReportEntry) {}
}

/// Single `<testcase>` recorded by [`JUnitLikeTestReporter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JUnitTestCase {
    pub name: String,
    pub classname: String,
    pub time_ms: i64,
    pub failure_tag: Option<(&'static str, String)>,
}

/// `net.minecraft.gametest.framework.JUnitLikeTestReporter` (Paper 26.2).
///
/// Produces the XML Paper's DOM + default JDK `Transformer` writes: an XML
/// declaration with `standalone="no"`, no indentation, attributes in the DOM's
/// sorted (by name) order, and childless elements self-closed:
/// ```xml
/// <?xml version="1.0" encoding="UTF-8" standalone="no"?>
/// <testsuite>
///   <testsuite time="..." timestamp="...">
///     <testcase classname="..." name="..." time="...">
///       <failure message="..."/> <!-- or <skipped message="..."/> for optional -->
///     </testcase>
///     <testcase classname="..." name="..." time="..."/> <!-- passed -->
///   </testsuite>
/// </testsuite>
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JUnitLikeTestReporter {
    timestamp: String,
    total_time_ms: i64,
    test_cases: Vec<JUnitTestCase>,
    finished: bool,
}

impl JUnitLikeTestReporter {
    /// Construct a JUnit reporter with an explicit deterministic ISO-8601 timestamp.
    pub fn new(timestamp: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
            total_time_ms: 0,
            test_cases: Vec::new(),
            finished: false,
        }
    }

    pub fn test_cases(&self) -> &[JUnitTestCase] {
        &self.test_cases
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_total_time_ms(&mut self, total_time_ms: i64) {
        self.total_time_ms = total_time_ms;
    }

    /// Render deterministic XML matching Paper's `JUnitLikeTestReporter.java`.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>");
        out.push_str("<testsuite>");
        let _ = write!(
            out,
            "<testsuite time=\"{}\" timestamp=\"{}\"",
            format_seconds(self.total_time_ms),
            escape_xml_attr(&self.timestamp)
        );
        if self.test_cases.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            for tc in &self.test_cases {
                let _ = write!(
                    out,
                    "<testcase classname=\"{}\" name=\"{}\" time=\"{}\"",
                    escape_xml_attr(&tc.classname),
                    escape_xml_attr(&tc.name),
                    format_seconds(tc.time_ms)
                );
                match &tc.failure_tag {
                    Some((tag, msg)) => {
                        let _ = write!(
                            out,
                            "><{tag} message=\"{}\"/></testcase>",
                            escape_xml_attr(msg)
                        );
                    }
                    None => out.push_str("/>"),
                }
            }
            out.push_str("</testsuite>");
        }
        out.push_str("</testsuite>");
        out
    }

    /// Write the XML report to `path`.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        fs::write(path, self.to_xml())
    }
}

impl TestReporter for JUnitLikeTestReporter {
    fn on_test_failed(&mut self, entry: &GameTestReportEntry) {
        let tag = if entry.required { "failure" } else { "skipped" };
        let err_msg = entry
            .error
            .as_ref()
            .map(GameTestException::message)
            .unwrap_or_default();
        let message = format!("({}) {}", entry.short_pos_string(), err_msg);
        self.total_time_ms = self.total_time_ms.wrapping_add(entry.run_time_ms);
        self.test_cases.push(JUnitTestCase {
            name: entry.id.to_string(),
            classname: entry.structure.to_string(),
            time_ms: entry.run_time_ms,
            failure_tag: Some((tag, message)),
        });
    }

    fn on_test_success(&mut self, entry: &GameTestReportEntry) {
        self.total_time_ms = self.total_time_ms.wrapping_add(entry.run_time_ms);
        self.test_cases.push(JUnitTestCase {
            name: entry.id.to_string(),
            classname: entry.structure.to_string(),
            time_ms: entry.run_time_ms,
            failure_tag: None,
        });
    }

    fn finish(&mut self) {
        self.finished = true;
    }
}

/// Paper: `String.valueOf(ms / 1000.0)`.
fn format_seconds(ms: i64) -> String {
    java_double_to_string((ms as f64) / 1000.0)
}

/// Java `Double.toString(double)` for finite values (JDK 19+ shortest-repr):
/// plain decimal with at least one fractional digit for `1e-3 <= |v| < 1e7`,
/// otherwise computerized scientific notation (`1.0E7`).
fn java_double_to_string(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.to_owned();
    }
    if (1e-3..1e7).contains(&v.abs()) {
        let s = format!("{v}");
        if s.contains('.') { s } else { format!("{s}.0") }
    } else {
        let s = format!("{v:e}");
        let (mantissa, exponent) = s.split_once('e').unwrap_or((&s, "0"));
        if mantissa.contains('.') {
            format!("{mantissa}E{exponent}")
        } else {
            format!("{mantissa}.0E{exponent}")
        }
    }
}

fn is_valid_xml_1_0_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

fn escape_xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other if is_valid_xml_1_0_char(other) => out.push(other),
            _ => {}
        }
    }
    out
}

/// Overall run verdict. Zero selected tests is explicitly non-success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTestRunStatus {
    /// At least one test was selected and executed, and zero required tests failed.
    Passed,
    /// At least one required test failed (or runner halted on error).
    Failed,
    /// Selection matched zero tests; Paper exits non-zero (`EXIT_CODE_CRASH_DEFAULT`).
    ZeroSelected,
    /// Runner did not complete within the tick budget.
    Incomplete,
}

/// Outcome of a single logical test (across its retries/flaky attempts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTestOutcome {
    Passed,
    FailedRequired,
    FailedOptional,
    Skipped,
}

/// Per-test record in [`GameTestExecutionTranscript`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestRunRecord {
    pub id: Identifier,
    pub structure: Identifier,
    pub environment: Identifier,
    pub batch_index: i32,
    pub required: bool,
    pub outcome: GameTestOutcome,
    pub setup_tick: Option<i64>,
    pub start_tick: Option<i64>,
    pub end_tick: Option<i64>,
    pub final_test_tick: i32,
    pub attempts: i32,
    pub retry_count: i32,
    pub successes: i32,
    pub first_failure: Option<String>,
    pub final_error: Option<String>,
    pub position: Option<BlockPos>,
}

/// Stable deterministic execution transcript covering all fields required by
/// issue #430: discovered/selected/executed/pass/fail/skip counts,
/// required/optional status, seed, setup/start/end ticks, retry count, first
/// failure, and resource provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTestExecutionTranscript {
    pub status: GameTestRunStatus,
    pub discovered_count: usize,
    pub selected_count: usize,
    pub executed_count: usize,
    pub passed_count: usize,
    pub failed_required_count: usize,
    pub failed_optional_count: usize,
    pub skipped_count: usize,
    pub seed: i64,
    pub resource_provenance: String,
    pub records: Vec<GameTestRunRecord>,
}

impl GameTestExecutionTranscript {
    /// Construct an explicit non-success transcript when selection matched zero tests.
    pub fn zero_selected(
        discovered_count: usize,
        seed: i64,
        resource_provenance: impl Into<String>,
    ) -> Self {
        Self {
            status: GameTestRunStatus::ZeroSelected,
            discovered_count,
            selected_count: 0,
            executed_count: 0,
            passed_count: 0,
            failed_required_count: 0,
            failed_optional_count: 0,
            skipped_count: 0,
            seed,
            resource_provenance: resource_provenance.into(),
            records: Vec::new(),
        }
    }

    /// True ONLY when at least one test was selected and executed, and no required tests failed.
    /// Zero selected tests is ALWAYS non-success (`false`).
    pub const fn is_success(&self) -> bool {
        matches!(self.status, GameTestRunStatus::Passed)
            && self.selected_count > 0
            && self.executed_count > 0
            && self.failed_required_count == 0
    }
}

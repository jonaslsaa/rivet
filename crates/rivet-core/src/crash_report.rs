//! Stub of `net.minecraft.CrashReport` / `ReportedException` for rivet-nbt.
//!
//! STUB(net.minecraft) — owned by the `net.minecraft` package, which lives in
//! `rivet-server`. rivet-nbt must not depend on rivet-server (Cargo cycle:
//! rivet-server → rivet-nbt), so this minimal surface is replicated here and
//! will be replaced by the real port (via `rivet-server`/`CrashReportCategory`)
//! once the cycle is broken (or the real types are promoted into a shared crate).
//!
//! Surface is what `net.minecraft.nbt` calls in the Java source plus what the
//! level-data crash-report defaults need (#398):
//!   CrashReport.forThrowable(e, "Loading NBT data")
//!   report.addCategory("NBT Tag").setDetail("Tag name", value)
//!   ReportedNbtException extends ReportedException
//! The `CrashReportCategory` records its `setDetail` entries so the
//! level-data `fillCrashReportCategory` defaults (#398) are observable and
//! testable. Java's `setDetail` takes a lazy `Supplier<String>` (the detail is
//! formatted only when the report is rendered); the stub formats its
//! `impl Display` value eagerly. Accepted: this is a test-observability stub
//! replaced by the real `rivet-server` `CrashReport` port, and the level-data
//! defaults format their detail strings at call time regardless.

use std::fmt;

/// Port of `net.minecraft.CrashReport`.
#[derive(Debug)]
pub struct CrashReport {
    title: String,
    // Captured for API fidelity; read when the real CrashReport printer lands.
    #[allow(dead_code)]
    exception: String,
}

impl CrashReport {
    /// `CrashReport.forThrowable(e, title)`. The throwable's message is
    /// captured at construction (Java holds the Throwable reference).
    pub fn for_throwable(e: &dyn fmt::Display, title: &str) -> Self {
        CrashReport {
            title: title.to_owned(),
            exception: e.to_string(),
        }
    }

    pub fn add_category(&self, name: &str) -> CrashReportCategory {
        CrashReportCategory::new(name)
    }
}

/// Port of `net.minecraft.CrashReportCategory`.
///
/// Java holds an ordered `List<Entry>` of `(key, value)` detail pairs; this
/// stub records the same pairs (in order) so crash-report callers can append
/// and observers can read them back. The real port (rivet-server) replaces
/// this once the rivet-nbt ↔ rivet-server cycle is broken; the `formatLocation`
/// static (needs `LevelHeightAccessor`/`BlockPos`/`SectionPos`) lives in
/// `rivet-world::level::storage::level_data` (the level crate owns the height
/// accessor) rather than here.
#[derive(Debug, Default)]
pub struct CrashReportCategory {
    // Captured for API fidelity; printed by the real CrashReport printer
    // (`getDetails` prefix), which the stub does not render yet.
    #[allow(dead_code)]
    title: String,
    entries: Vec<(String, String)>,
}

impl CrashReportCategory {
    /// `new CrashReportCategory(String title)`.
    pub fn new(title: impl Into<String>) -> Self {
        CrashReportCategory {
            title: title.into(),
            entries: Vec::new(),
        }
    }

    /// `CrashReportCategory.setDetail(String key, Object value)` — appends
    /// the `(key, value.toString())` entry.
    pub fn set_detail(&mut self, name: &str, value: impl fmt::Display) -> &mut Self {
        self.entries.push((name.to_string(), value.to_string()));
        self
    }

    /// The recorded `(key, value)` entries, in insertion order.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_detail_records_in_order() {
        let mut category = CrashReportCategory::new("NBT Tag");
        category
            .set_detail("Tag name", "level.dat")
            .set_detail("Tag type", 3);
        assert_eq!(
            category.entries(),
            &[
                ("Tag name".to_string(), "level.dat".to_string()),
                ("Tag type".to_string(), "3".to_string()),
            ]
        );
    }

    #[test]
    fn booleans_record_as_java_boolean_strings() {
        // Java `Boolean.toString` / `%b` formats as "true"/"false".
        let mut category = CrashReportCategory::new("test");
        category
            .set_detail("Derived", true)
            .set_detail("wasModded", false);
        assert_eq!(
            category.entries()[0],
            ("Derived".to_string(), "true".to_string())
        );
        assert_eq!(
            category.entries()[1],
            ("wasModded".to_string(), "false".to_string())
        );
    }
}

/// Port of `net.minecraft.ReportedException`.
#[derive(Debug)]
pub struct ReportedException {
    report: CrashReport,
}

impl ReportedException {
    pub fn new(report: CrashReport) -> Self {
        ReportedException { report }
    }
}

impl std::error::Error for ReportedException {}

impl fmt::Display for ReportedException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.report.title)
    }
}

impl std::panic::UnwindSafe for ReportedException {}
impl std::panic::RefUnwindSafe for ReportedException {}

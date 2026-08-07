//! Minimal stub of `net.minecraft.CrashReport` / `ReportedException` for rivet-nbt.
//!
//! STUB(net.minecraft) — owned by the `net.minecraft` package, which lives in
//! `rivet-server`. rivet-nbt must not depend on rivet-server (Cargo cycle:
//! rivet-server → rivet-nbt), so this minimal surface is replicated here and
//! will be replaced by the real port (via `rivet-server`/`CrashReportCategory`)
//! once the cycle is broken (or the real types are promoted into a shared crate).
//!
//! Surface is exactly what `net.minecraft.nbt` calls in the Java source:
//!   CrashReport.forThrowable(e, "Loading NBT data")
//!   report.addCategory("NBT Tag").setDetail("Tag name", value)
//!   ReportedNbtException extends ReportedException

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

    pub fn add_category(&self, _name: &str) -> CrashReportCategory {
        CrashReportCategory
    }
}

/// Port of `net.minecraft.CrashReportCategory` (minimal: only `setDetail`).
#[derive(Debug, Default)]
pub struct CrashReportCategory;

impl CrashReportCategory {
    pub fn set_detail(&self, _name: &str, _value: impl fmt::Display) {}
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

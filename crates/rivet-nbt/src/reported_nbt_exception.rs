//! Port of `net.minecraft.nbt.ReportedNbtException` — `extends ReportedException`.
//!
//! `ReportedException`/`CrashReport` live in `rivet-core` (minimal skeleton;
//! see `crash_report.rs`).

use rivet_core::{CrashReport, ReportedException};

/// `ReportedNbtException`.
#[derive(Debug)]
pub struct ReportedNbtException {
    pub report: CrashReport,
}

impl ReportedNbtException {
    pub fn new(report: CrashReport) -> Self {
        ReportedNbtException { report }
    }
}

impl From<ReportedNbtException> for ReportedException {
    fn from(e: ReportedNbtException) -> Self {
        ReportedException::new(e.report)
    }
}

//! Port of the `net.minecraft` root package — classes owned by no subpackage.
//!
//! STUB(shared) — `CrashReport`/`ReportedException` and `ChatFormatting` were
//! moved here from rivet-nbt/rivet-text (decision 7754455). The real ports live
//! in rivet-server (CrashReport) / rivet-text (ChatFormatting); the minimal
//! surface here breaks the rivet-nbt ↔ rivet-server cycle.

pub mod chat_formatting;
pub mod crash_report;

pub use chat_formatting::ChatFormatting;
pub use crash_report::{CrashReport, CrashReportCategory, ReportedException};

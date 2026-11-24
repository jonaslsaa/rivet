//! Closure accounting shared by every differential row (VERIFICATION §2.4.5).
//!
//! A row declares how many inputs its corpus holds; the run counts how many it
//! actually executed and how they compared. A row can only PASS when every
//! declared input executed, none mismatched, and its tamper negative was
//! detected. Zero executed inputs is UNVERIFIED (nothing was compared), never a
//! vacuous PASS; executed != declared is a FAIL (inputs were silently dropped
//! or invented).

use std::fmt;

use crate::exit::{EXIT_FAIL, EXIT_PASS, EXIT_UNVERIFIED};

/// The verdict of one verification row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Pass,
    Unverified,
    Fail,
}

impl Status {
    /// The machine-stable exit code for this status ([`crate::exit`]).
    pub fn exit_code(self) -> u8 {
        match self {
            Status::Pass => EXIT_PASS,
            Status::Fail => EXIT_FAIL,
            Status::Unverified => EXIT_UNVERIFIED,
        }
    }

    /// Combine row statuses: any FAIL wins, then any UNVERIFIED; an empty set
    /// of rows is UNVERIFIED.
    pub fn combine(statuses: impl IntoIterator<Item = Status>) -> Status {
        statuses.into_iter().max().unwrap_or(Status::Unverified)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Unverified => "UNVERIFIED",
        })
    }
}

/// Declared-vs-executed counts for one row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Closure {
    pub declared: usize,
    pub executed: usize,
    pub matched: usize,
    pub mismatched: usize,
}

impl Closure {
    /// The row status given whether its tamper negative was detected and named.
    pub fn status(&self, negative_detected: bool) -> Status {
        if self.matched + self.mismatched != self.executed {
            // The accounting itself is inconsistent: never green.
            return Status::Fail;
        }
        if self.mismatched > 0 {
            return Status::Fail;
        }
        if self.executed == 0 {
            return Status::Unverified;
        }
        if self.executed != self.declared {
            return Status::Fail;
        }
        if !negative_detected {
            return Status::Unverified;
        }
        Status::Pass
    }
}

impl fmt::Display for Closure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "declared={} executed={} matched={} mismatched={}",
            self.declared, self.executed, self.matched, self.mismatched
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closure(declared: usize, executed: usize, matched: usize, mismatched: usize) -> Closure {
        Closure {
            declared,
            executed,
            matched,
            mismatched,
        }
    }

    #[test]
    fn full_clean_closure_with_detected_negative_passes() {
        assert_eq!(closure(5, 5, 5, 0).status(true), Status::Pass);
    }

    #[test]
    fn zero_executed_is_unverified_never_pass() {
        assert_eq!(closure(0, 0, 0, 0).status(true), Status::Unverified);
        assert_eq!(closure(5, 0, 0, 0).status(true), Status::Unverified);
    }

    #[test]
    fn dropped_or_invented_inputs_fail() {
        assert_eq!(closure(5, 4, 4, 0).status(true), Status::Fail);
        assert_eq!(closure(5, 6, 6, 0).status(true), Status::Fail);
    }

    #[test]
    fn any_mismatch_fails_even_when_the_negative_was_not_run() {
        assert_eq!(closure(5, 5, 4, 1).status(false), Status::Fail);
    }

    #[test]
    fn undetected_negative_is_unverified() {
        assert_eq!(closure(5, 5, 5, 0).status(false), Status::Unverified);
    }

    #[test]
    fn inconsistent_accounting_fails() {
        assert_eq!(closure(5, 5, 3, 0).status(true), Status::Fail);
    }

    #[test]
    fn combine_prefers_fail_then_unverified() {
        use Status::*;
        assert_eq!(Status::combine([Pass, Pass]), Pass);
        assert_eq!(Status::combine([Pass, Unverified]), Unverified);
        assert_eq!(Status::combine([Unverified, Fail, Pass]), Fail);
        assert_eq!(Status::combine([]), Unverified);
        assert_eq!(Fail.exit_code(), 1);
        assert_eq!(Unverified.exit_code(), 3);
        assert_eq!(Pass.exit_code(), 0);
    }
}

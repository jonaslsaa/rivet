//! Machine-stable process exit contract shared by every harness binary.
//!
//! `scripts/gate.sh` classifies each oracle/scenario step by its exit code
//! (`ORACLE_EXIT_UNVERIFIED=3`). The contract, which every harness tool must
//! honor:
//!
//! - `0` PASS — the comparison ran and found no divergence.
//! - `1` FAIL — the comparison ran and diverged (or a hard orchestration
//!   failure, e.g. a panic — a tool crash is FAILED, never green).
//! - `3` UNVERIFIED — the oracle/prerequisite could not run (missing jar,
//!   missing binary, a server that never reached READY), so nothing was
//!   actually compared. Distinct from FAIL so the gate can report "did not
//!   run" instead of "ran and diverged".
//!
//! Any exit code other than 0/1/3 is treated by the gate as a tool failure and
//! maps to FAILED.

/// The comparison ran and found no divergence.
pub const EXIT_PASS: u8 = 0;
/// The comparison ran and diverged, or the harness crashed.
pub const EXIT_FAIL: u8 = 1;
/// A prerequisite was missing or the oracle did not reach READY.
pub const EXIT_UNVERIFIED: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_machine_stable_and_distinct() {
        assert_eq!(EXIT_PASS, 0);
        assert_eq!(EXIT_FAIL, 1);
        assert_eq!(EXIT_UNVERIFIED, 3);
        assert_ne!(
            EXIT_FAIL, EXIT_UNVERIFIED,
            "FAIL and UNVERIFIED are distinct signals"
        );
    }
}

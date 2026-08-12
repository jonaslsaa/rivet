//! Shared wall-clock timing budgets — and the timeout validation that consumes
//! them — for `rivet-client` and its `run-scenario` orchestrator. Keeping them
//! here, rather than duplicating literals and predicates in both tools, is what
//! makes that coupling load-bearing instead of a drift-prone convention.

/// How long the keepalive settle loop (dwell and move modes) waits for the
/// challenge and echo streams to reach 1:1 correspondence before giving up and
/// snapshotting anyway. Bounded so a genuinely missing echo still fails the
/// verdict.
pub const KEEPALIVE_SETTLE_TIMEOUT_SECS: u64 = 1;

/// Reserved client-side headroom (s) beyond the scenario window that
/// `--timeout-seconds` must accommodate: the offline login and configuration
/// time before `Event::Spawn`.
pub const DWELL_LOGIN_HEADROOM_SECONDS: u64 = 5;

/// The headroom (s) a `dwell` `--timeout-seconds` must exceed the dwell window
/// by: the keepalive settle plus the pre-spawn login headroom.
pub const DWELL_TIMEOUT_HEADROOM_SECONDS: u64 =
    KEEPALIVE_SETTLE_TIMEOUT_SECS + DWELL_LOGIN_HEADROOM_SECONDS;

/// Move-mode walk length in game ticks (the client's `MOVE_TICKS`).
pub const MOVE_WALK_TICKS: u32 = 120;
/// Server tick rate (game ticks per second).
pub const TICKS_PER_SECOND: u32 = 20;
/// Move-mode walk wall-clock seconds, ceiled to a whole second (like
/// `MOVE_DRAIN_SECONDS_CEIL`) so a tick count that is not divisible by the tick
/// rate cannot silently truncate the reserved budget below the true walk time.
pub const MOVE_WALK_SECONDS: u64 =
    ((MOVE_WALK_TICKS + TICKS_PER_SECOND - 1) / TICKS_PER_SECOND) as u64;
/// Move-mode post-walk drain in milliseconds (the client's `MOVE_DRAIN`).
pub const MOVE_DRAIN_MS: u64 = 200;
/// Move-mode post-walk drain (s) before the record emits: `MOVE_DRAIN_MS`
/// ceiled to a whole second so the headroom budget is a whole-second floor.
pub const MOVE_DRAIN_SECONDS_CEIL: u64 = (MOVE_DRAIN_MS + 999) / 1000;

/// The headroom (s) a `move` `--timeout-seconds` must exceed: the pre-spawn
/// login/configuration, the fixed walk, the post-walk drain, and the keepalive
/// settle. A timeout at or below this total cuts the client off before it emits
/// `moved`.
///
/// Computed below as `DWELL_LOGIN_HEADROOM_SECONDS + MOVE_WALK_SECONDS +
/// MOVE_DRAIN_SECONDS_CEIL + KEEPALIVE_SETTLE_TIMEOUT_SECS` so the runner and
/// client can never disagree about the move emit budget.
pub const MOVE_TIMEOUT_HEADROOM_SECONDS: u64 = DWELL_LOGIN_HEADROOM_SECONDS
    + MOVE_WALK_SECONDS
    + MOVE_DRAIN_SECONDS_CEIL
    + KEEPALIVE_SETTLE_TIMEOUT_SECS;

/// The move budget is a compile-time constant; a refactor that zeroes every
/// summand would leave no wall-clock budget for the walk, so pin it above zero
/// in every build (not just test builds).
const _: () = assert!(MOVE_TIMEOUT_HEADROOM_SECONDS > 0);

/// Reject a `move` `--timeout-seconds` that cannot outlast the emit path: the
/// `moved` record is emitted only after login/configuration, the fixed walk,
/// the drain, and the keepalive settle. A timeout at or below that total cuts
/// the client off before it emits (ExitCode 2, spurious FAIL). Returns the
/// shared error so both parsers cannot drift.
pub fn validate_move_timeout(timeout_seconds: u64) -> Result<(), String> {
    if timeout_seconds <= MOVE_TIMEOUT_HEADROOM_SECONDS {
        Err(format!(
            "--timeout-seconds must exceed {MOVE_TIMEOUT_HEADROOM_SECONDS}s in move mode (the \
             client spends up to {DWELL_LOGIN_HEADROOM_SECONDS}s on login/configuration, \
             {MOVE_WALK_SECONDS}s walking, {MOVE_DRAIN_SECONDS_CEIL}s draining, and \
             {KEEPALIVE_SETTLE_TIMEOUT_SECS}s settling the keepalive stream before emitting the \
             moved record, and must emit before the timeout fires); got timeout {timeout_seconds}s"
        ))
    } else {
        Ok(())
    }
}

/// Reject a `dwell` `--timeout-seconds` that cannot outlast the dwell emit
/// path: the `dwell` record is emitted only after the dwell window, up to 1 s
/// of keepalive settling, and the pre-spawn login/configuration time. Returns
/// the shared error so both parsers cannot drift.
pub fn validate_dwell_timeout(dwell_seconds: u64, timeout_seconds: u64) -> Result<(), String> {
    if timeout_seconds <= dwell_seconds + DWELL_TIMEOUT_HEADROOM_SECONDS {
        Err(format!(
            "--timeout-seconds must exceed --dwell-seconds by more than \
             {DWELL_TIMEOUT_HEADROOM_SECONDS}s of settle/login headroom (the client spends up to \
             {KEEPALIVE_SETTLE_TIMEOUT_SECS}s settling the keepalive stream after the dwell \
             window, plus {DWELL_LOGIN_HEADROOM_SECONDS}s of login/configuration time before \
             spawn, and must emit the dwell record before the timeout fires); got dwell \
             {dwell_seconds}s timeout {timeout_seconds}s"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_headroom_matches_the_fixed_emit_budget() {
        // The self-sum is implied by the definition; the literals pin it so a
        // refactor that shrinks every summand cannot silently shrink the budget.
        assert_eq!(MOVE_TIMEOUT_HEADROOM_SECONDS, 13);
        assert_eq!(MOVE_DRAIN_SECONDS_CEIL, 1);
        assert_eq!(MOVE_WALK_SECONDS, 6);
    }

    #[test]
    fn dwell_headroom_matches_settle_plus_login() {
        assert_eq!(DWELL_TIMEOUT_HEADROOM_SECONDS, 6);
        assert_eq!(
            KEEPALIVE_SETTLE_TIMEOUT_SECS + DWELL_LOGIN_HEADROOM_SECONDS,
            6
        );
    }

    #[test]
    fn validate_move_timeout_rejects_at_or_below_the_budget() {
        assert!(validate_move_timeout(MOVE_TIMEOUT_HEADROOM_SECONDS).is_err());
        assert!(validate_move_timeout(MOVE_TIMEOUT_HEADROOM_SECONDS + 1).is_ok());
    }

    #[test]
    fn validate_dwell_timeout_rejects_an_insufficient_reservation() {
        assert!(validate_dwell_timeout(41, 47).is_err());
        assert!(validate_dwell_timeout(41, 48).is_ok());
    }
}

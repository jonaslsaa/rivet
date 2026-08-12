//! Shared wall-clock timing budgets that `rivet-client` and its
//! `run-scenario` orchestrator must agree on.
//!
//! The client enforces its `--timeout-seconds` reservation at parse time so a
//! record is always emitted before the outer timeout fires; `run-scenario`
//! mirrors the same validation so an invocation it accepts is one the client
//! can honor. Keeping the budgets here — rather than duplicating literals in
//! both tools — is what makes that coupling load-bearing instead of a
//! drift-prone convention. (The dwell timeout headroom used to be a hardcoded
//! `6` in `run-scenario` that had to match the client's `1 + 5` by hand.)

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
/// Move-mode walk wall-clock seconds.
pub const MOVE_WALK_SECONDS: u64 = (MOVE_WALK_TICKS / TICKS_PER_SECOND) as u64;
/// Move-mode post-walk drain (s) before the record emits (the client's
/// `MOVE_DRAIN`, ceiled to a whole second for the headroom budget).
pub const MOVE_DRAIN_SECONDS_CEIL: u64 = 1;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_headroom_exceeds_the_fixed_emit_budget() {
        // The move record is emitted only after login/configuration, the walk,
        // the drain, and the keepalive settle. The timeout must exceed that
        // total (strict >), so the headroom constant must be at least the sum
        // of its parts — the derived value is exactly that sum by construction,
        // and this pins the arithmetic so a refactor cannot silently shrink it.
        assert_eq!(
            MOVE_TIMEOUT_HEADROOM_SECONDS,
            DWELL_LOGIN_HEADROOM_SECONDS
                + MOVE_WALK_SECONDS
                + MOVE_DRAIN_SECONDS_CEIL
                + KEEPALIVE_SETTLE_TIMEOUT_SECS
        );
        assert!(MOVE_TIMEOUT_HEADROOM_SECONDS > 0);
    }

    #[test]
    fn dwell_headroom_is_settle_plus_login() {
        assert_eq!(
            DWELL_TIMEOUT_HEADROOM_SECONDS,
            KEEPALIVE_SETTLE_TIMEOUT_SECS + DWELL_LOGIN_HEADROOM_SECONDS
        );
    }
}

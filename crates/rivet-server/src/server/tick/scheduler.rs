//! Deterministic tick scheduling, mirroring Paper's `MinecraftServer.runServer`
//! catch-up math (the moonrise `tickSchedule`). Pure — no wall clock — so it is
//! unit-tested with a simulated clock.
//!
//! Observable semantics matched from Paper:
//!   - one tick per loop iteration, `interval` (50 ms → 20 TPS) apart;
//!   - the schedule is *monotonic*: the next deadline advances by exactly one
//!     interval per tick regardless of how long the tick took, so a slow tick
//!     does not push later ticks later — the loop catches up instead;
//!   - catch-up cap (`catchupTicks`, Paper default 5): a backlog beyond the cap
//!     is dropped by advancing the schedule, bounding how many ticks run
//!     back-to-back after a stall before the loop re-syncs to the clock.

/// Nanoseconds per tick: `TimeUtil.NANOSECONDS_PER_SECOND / 20` (50 ms).
pub const NANOS_PER_TICK: u128 = 1_000_000_000 / 20;

/// Paper default `catchupTicks` (`GlobalConfiguration misc.catchupTicks.or(5)`).
pub const DEFAULT_CATCHUP_TICKS: u64 = 5;

/// The pure scheduling state of the tick loop. All times are in the same
/// arbitrary monotonic nanoseconds epoch as the `TickTime` clock feeding it.
#[derive(Debug, Clone)]
pub struct TickScheduler {
    interval_nanos: u128,
    catchup_ticks: u64,
    /// Deadline at which the next tick is due.
    next_deadline_nanos: u128,
    /// Number of ticks run (`MinecraftServer.tickCount`).
    tick_count: u64,
}

impl TickScheduler {
    pub fn new(interval_nanos: u128, catchup_ticks: u64, first_deadline_nanos: u128) -> Self {
        assert!(interval_nanos > 0, "tick interval must be positive");
        TickScheduler {
            interval_nanos,
            catchup_ticks: catchup_ticks.max(1),
            next_deadline_nanos: first_deadline_nanos,
            tick_count: 0,
        }
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    pub fn next_deadline_nanos(&self) -> u128 {
        self.next_deadline_nanos
    }

    pub fn interval_nanos(&self) -> u128 {
        self.interval_nanos
    }

    /// Called once per loop iteration with the current time. Returns `true` when
    /// a tick is due now, advancing the monotonic schedule (dropping backlog
    /// beyond the catch-up cap) and incrementing the tick counter.
    ///
    /// Paper's math (`MinecraftServer.runServer`): `ticksBehind` is computed
    /// against the schedule's `lastPeriod`, which `initTickSchedule` pins one
    /// interval *before* the deadline (`Schedule.setNextPeriod(nextTickTimeNanos,
    /// interval)`), so a full interval past the deadline counts as *two* periods:
    ///   ticksBehind = max(1, (now - deadline) / interval + 1)
    ///   if ticksBehind - catchup > 0: tickSchedule.advanceBy(ticksBehind - catchup)
    ///   tickSchedule.advanceBy(1)
    ///   nextTickTimeNanos = tickSchedule.getDeadline(interval)
    pub fn start_tick(&mut self, now_nanos: u128) -> bool {
        if now_nanos < self.next_deadline_nanos {
            return false;
        }
        // Paper: `Math.max(1, getPeriodsAhead(interval, now))` where
        // `lastPeriod = nextTickTimeNanos - interval`.
        let periods_ahead =
            ((now_nanos - self.next_deadline_nanos) / self.interval_nanos + 1).max(1);
        let ticks_behind = periods_ahead as u64;
        let excess = ticks_behind.saturating_sub(self.catchup_ticks);
        if excess > 0 {
            self.next_deadline_nanos += self.interval_nanos * excess as u128;
        }
        self.next_deadline_nanos += self.interval_nanos;
        self.tick_count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler(interval: u128, catchup: u64) -> TickScheduler {
        TickScheduler::new(interval, catchup, 0)
    }

    #[test]
    fn first_tick_is_due_immediately() {
        let mut s = scheduler(NANOS_PER_TICK, DEFAULT_CATCHUP_TICKS);
        assert!(s.start_tick(0));
        assert_eq!(s.tick_count(), 1);
        assert_eq!(s.next_deadline_nanos(), NANOS_PER_TICK);
    }

    #[test]
    fn steady_state_ticks_every_interval() {
        let mut s = scheduler(NANOS_PER_TICK, DEFAULT_CATCHUP_TICKS);
        for tick in 1..=10u64 {
            let now = (tick as u128 - 1) * NANOS_PER_TICK;
            assert!(s.start_tick(now), "tick {tick} due at {now}");
            assert_eq!(s.tick_count(), tick);
            assert_eq!(s.next_deadline_nanos(), tick as u128 * NANOS_PER_TICK);
        }
        // Nothing is due between deadlines.
        assert!(!s.start_tick(10 * NANOS_PER_TICK - 1));
        assert_eq!(s.tick_count(), 10);
    }

    #[test]
    fn no_tick_before_deadline() {
        let mut s = scheduler(NANOS_PER_TICK, 1);
        s.start_tick(0);
        assert!(!s.start_tick(NANOS_PER_TICK - 1));
        assert!(!s.start_tick(NANOS_PER_TICK / 2));
        assert_eq!(s.tick_count(), 1);
    }

    #[test]
    fn slow_tick_keeps_schedule_monotonic() {
        // A tick that overruns its deadline must not shift later deadlines: the
        // schedule stays on the absolute grid and catches up.
        let mut s = scheduler(NANOS_PER_TICK, DEFAULT_CATCHUP_TICKS);
        s.start_tick(0); // tick 1 at t=0, next deadline t=50ms
        // Tick work overruns by 30ms: the next call is at t=80ms.
        assert!(s.start_tick(80 * 1_000_000));
        assert_eq!(s.tick_count(), 2);
        // The next deadline is t=100ms (grid-aligned), NOT t=130ms.
        assert_eq!(s.next_deadline_nanos(), 100 * 1_000_000);
        assert!(!s.start_tick(90 * 1_000_000));
        assert!(s.start_tick(100 * 1_000_000));
        assert_eq!(s.tick_count(), 3);
    }

    #[test]
    fn catchup_cap_drops_excess_backlog() {
        // interval 10ms, catchup 3. After one tick (deadline 10ms), a 90ms
        // stall (now=100ms). Paper counts the backlog against `lastPeriod` =
        // deadline - interval = 0ms, so ticksBehind = (100 - 0)/10 = 10; it
        // drops 10 - 3 = 7 periods and then runs exactly catchup = 3
        // back-to-back ticks before the schedule passes `now` (deadline 110ms
        // > 100ms) and the loop waits.
        let mut s = scheduler(10 * 1_000_000, 3);
        s.start_tick(0); // tick 1, deadline = 10ms
        assert_eq!(s.next_deadline_nanos(), 10 * 1_000_000);

        let mut ran = 0;
        while s.start_tick(100 * 1_000_000) {
            ran += 1;
        }
        assert_eq!(ran, 3);
        assert_eq!(s.tick_count(), 4);
        // Re-synced: the next deadline is past `now`, so the loop sleeps.
        assert_eq!(s.next_deadline_nanos(), 110 * 1_000_000);
        assert!(!s.start_tick(100 * 1_000_000));
    }

    #[test]
    fn backlog_of_exactly_catchup_runs_catchup_ticks_without_drop() {
        // A backlog exactly at the cap (ticksBehind == catchup == 3) drops
        // nothing and still runs catchup back-to-back ticks: `now = 30ms`
        // counts 3 periods against lastPeriod (0ms), advanceBy(1) per call.
        let mut s = scheduler(10 * 1_000_000, 3);
        s.start_tick(0); // tick 1, deadline = 10ms

        let mut ran = 0;
        while s.start_tick(30 * 1_000_000) {
            ran += 1;
        }
        assert_eq!(ran, 3);
        // No period was dropped: the schedule advanced exactly 3 intervals past
        // the pre-stall deadline (10ms → 40ms) — had a drop fired, this would
        // jump further ahead.
        assert_eq!(s.next_deadline_nanos(), 40 * 1_000_000);
    }

    #[test]
    fn backlog_of_catchup_plus_one_caps_at_catchup_ticks() {
        // One period over the cap (ticksBehind == catchup + 1 == 4) drops the
        // single excess period, then runs catchup = 3 back-to-back ticks — the
        // cap boundary where dropping begins.
        let mut s = scheduler(10 * 1_000_000, 3);
        s.start_tick(0); // tick 1, deadline = 10ms

        let mut ran = 0;
        while s.start_tick(40 * 1_000_000) {
            ran += 1;
        }
        assert_eq!(ran, 3);
        assert_eq!(s.tick_count(), 4);
        // The one dropped period shows up as a deadline past `now` (40ms) that
        // skips the 30ms grid point.
        assert_eq!(s.next_deadline_nanos(), 50 * 1_000_000);
    }

    #[test]
    fn small_overrun_runs_one_tick_then_waits() {
        // A sub-period overrun (now = 15ms, deadline = 10ms) counts one period
        // against lastPeriod (0ms): exactly one tick runs, then the schedule is
        // already past `now` and the loop waits for 20ms.
        let mut s = scheduler(10 * 1_000_000, 3);
        s.start_tick(0); // tick 1, deadline = 10ms

        assert!(s.start_tick(15 * 1_000_000));
        assert_eq!(s.tick_count(), 2);
        assert_eq!(s.next_deadline_nanos(), 20 * 1_000_000);
        assert!(!s.start_tick(15 * 1_000_000));
    }

    #[test]
    fn backlog_within_cap_runs_all_periods() {
        // A backlog below the cap (ticksBehind = 2 < catchup 5) drops nothing
        // and runs each backlogged period (Paper max(1, periodsAhead) then
        // advanceBy(1) per call).
        let mut s = scheduler(NANOS_PER_TICK, DEFAULT_CATCHUP_TICKS);
        s.start_tick(0); // tick 1, deadline = 50ms
        let mut ran = 0;
        while s.start_tick(100 * 1_000_000) {
            ran += 1;
        }
        // Paper counts against lastPeriod = deadline - interval = 0ms, so
        // 100ms is 2 periods behind: exactly 2 back-to-back ticks (no drop).
        assert_eq!(ran, 2);
        assert_eq!(s.next_deadline_nanos(), 150 * 1_000_000);
    }

    #[test]
    fn exact_deadline_counts_as_due() {
        let mut s = scheduler(NANOS_PER_TICK, DEFAULT_CATCHUP_TICKS);
        s.start_tick(0);
        assert!(s.start_tick(NANOS_PER_TICK), "now == deadline is due");
        assert_eq!(s.tick_count(), 2);
    }

    #[test]
    #[should_panic(expected = "tick interval must be positive")]
    fn zero_interval_rejected() {
        TickScheduler::new(0, 1, 0);
    }
}

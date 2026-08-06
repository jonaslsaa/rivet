//! Clock abstraction for the tick loop: a monotonic nanoseconds source plus an
//! interruptible sleep. Production uses [`RealTime`] (backed by `Instant`, woken
//! by [`Shutdown`]); tests substitute the deterministic paused [`SimTime`].

use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::shutdown::Shutdown;

/// A monotonic clock in an arbitrary nanoseconds epoch, with a sleep that can be
/// interrupted by shutdown. Implementors are `Send + Sync` so the loop can live
/// on its own OS thread.
pub trait TickTime: Send + Sync {
    fn now_nanos(&self) -> u128;

    /// Block until `deadline_nanos` (same epoch as `now_nanos`). May return
    /// early if shutdown is requested; the loop re-checks after every wake.
    fn sleep_until(&self, deadline_nanos: u128);
}

/// Real monotonic clock (`Instant`, so scheduling is immune to NTP/clock jumps)
/// and a sleep interrupted by the `Shutdown` condvar.
pub struct RealTime {
    epoch: OnceLock<Instant>,
    shutdown: Arc<Shutdown>,
}

impl RealTime {
    pub fn new(shutdown: Arc<Shutdown>) -> Self {
        RealTime {
            epoch: OnceLock::new(),
            shutdown,
        }
    }
}

impl TickTime for RealTime {
    fn now_nanos(&self) -> u128 {
        let epoch = *self.epoch.get_or_init(Instant::now);
        epoch.elapsed().as_nanos()
    }

    fn sleep_until(&self, deadline_nanos: u128) {
        let now = self.now_nanos();
        if now >= deadline_nanos {
            return;
        }
        let sleep_for = Duration::from_nanos((deadline_nanos - now) as u64);
        self.shutdown.sleep_for(sleep_for);
    }
}

/// Deterministic paused clock for tests: `sleep_until` blocks until the test
/// advances the clock past the deadline, so the tick loop's timing is fully
/// under test control and never touches a wall clock. When constructed with a
/// shutdown handle (`SimTime::with_shutdown`) the sleep also wakes on shutdown —
/// the loop's idle wait must observe both wake sources.
#[derive(Debug)]
pub struct SimTime {
    state: Mutex<SimState>,
    condvar: Condvar,
    shutdown: Option<Arc<Shutdown>>,
}

#[derive(Debug, Default)]
struct SimState {
    now_nanos: u128,
    /// Diagnostic: total `sleep_until` calls (a busy loop would inflate it).
    sleeps: u64,
}

impl Default for SimTime {
    fn default() -> Self {
        SimTime {
            state: Mutex::new(SimState::default()),
            condvar: Condvar::new(),
            shutdown: None,
        }
    }
}

impl SimTime {
    pub fn new() -> Self {
        SimTime::default()
    }

    /// A `SimTime` whose sleep also returns when `shutdown` is requested (the
    /// tick-loop idle sleep needs both wake sources).
    pub fn with_shutdown(shutdown: Arc<Shutdown>) -> Self {
        SimTime {
            shutdown: Some(shutdown),
            ..SimTime::default()
        }
    }

    /// Advance the clock by `by_nanos`, waking any thread sleeping until a
    /// deadline now in the past.
    pub fn advance(&self, by_nanos: u128) {
        let mut st = self.state.lock().unwrap();
        st.now_nanos = st.now_nanos.saturating_add(by_nanos);
        self.condvar.notify_all();
    }

    pub fn now_nanos(&self) -> u128 {
        self.state.lock().unwrap().now_nanos
    }

    pub fn sleeps(&self) -> u64 {
        self.state.lock().unwrap().sleeps
    }
}

impl TickTime for SimTime {
    fn now_nanos(&self) -> u128 {
        SimTime::now_nanos(self)
    }

    fn sleep_until(&self, deadline_nanos: u128) {
        let mut st = self.state.lock().unwrap();
        st.sleeps += 1;
        let shutdown_aware = self.shutdown.is_some();
        loop {
            if st.now_nanos >= deadline_nanos {
                return;
            }
            if let Some(shutdown) = &self.shutdown
                && shutdown.is_requested()
            {
                return;
            }
            if shutdown_aware {
                // Re-check the shutdown flag periodically so a `request()` that
                // does not go through `advance` still ends the sleep promptly.
                let (guard, _) = self
                    .condvar
                    .wait_timeout(st, Duration::from_millis(5))
                    .unwrap_or_else(|e| e.into_inner());
                st = guard;
            } else {
                st = self.condvar.wait(st).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_time_advance_past_deadline_releases_sleep() {
        let sim = Arc::new(SimTime::new());
        let time: Arc<dyn TickTime> = sim.clone();
        let sleeper = {
            let time = time.clone();
            std::thread::spawn(move || time.sleep_until(1_000))
        };
        std::thread::sleep(Duration::from_millis(20));
        assert!(!sleeper.is_finished(), "sleep must block until advanced");
        sim.advance(1_000);
        sleeper.join().unwrap();
    }

    #[test]
    fn sim_time_sleep_returns_immediately_when_deadline_passed() {
        let sim = SimTime::new();
        sim.advance(5_000);
        sim.sleep_until(1_000); // deadline already passed — must not block
        assert_eq!(sim.now_nanos(), 5_000);
    }

    #[test]
    fn sim_time_now_reflects_advance() {
        let sim = SimTime::new();
        assert_eq!(sim.now_nanos(), 0);
        sim.advance(123);
        assert_eq!(sim.now_nanos(), 123);
    }
}

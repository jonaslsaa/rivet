//! `net.minecraft.gametest.framework.GameTestTicker` (Paper 26.2).
//!
//! Mirrors `GameTestTicker.java`: `IDLE`/`RUNNING`/`HALTING` state machine,
//! ordered active test collection, removal of completed tests after each tick,
//! and deferred clear when `clear()` is called mid-tick.

/// Copyable arena handle referencing a [`crate::server::gametest::game_test_info::GameTestInfo`]
/// owned by a runner or test harness (per `OWNERSHIP.md`: arenas + IDs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TestInfoId(pub usize);

/// `GameTestTicker.State` (Paper 26.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickerState {
    #[default]
    Idle,
    Running,
    Halting,
}

/// `net.minecraft.gametest.framework.GameTestTicker` (Paper 26.2).
///
/// Owned by the synchronous tick thread / runner rather than a global static
/// mutable singleton (per `OWNERSHIP.md`).
#[derive(Debug, Default)]
pub struct GameTestTicker {
    test_infos: Vec<TestInfoId>,
    state: TickerState,
}

/// Summary returned by [`GameTestTicker::tick_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickerTickResult {
    pub ticked_count: usize,
    pub removed_count: usize,
    pub halted: bool,
}

impl GameTestTicker {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn state(&self) -> TickerState {
        self.state
    }

    pub fn active_tests(&self) -> &[TestInfoId] {
        &self.test_infos
    }

    pub fn is_empty(&self) -> bool {
        self.test_infos.is_empty()
    }

    pub fn len(&self) -> usize {
        self.test_infos.len()
    }

    /// Paper: `GameTestTicker.add(GameTestInfo testInfo)`.
    pub fn add(&mut self, id: TestInfoId) {
        self.test_infos.push(id);
    }

    /// Paper: `GameTestTicker.clear()`.
    ///
    /// If not `Idle`, transitions to `Halting` so the active tick loop finishes
    /// safely before clearing. Returns `true` if cleared immediately.
    pub fn clear(&mut self) -> bool {
        if self.state != TickerState::Idle {
            self.state = TickerState::Halting;
            false
        } else {
            self.test_infos.clear();
            self.state = TickerState::Idle;
            true
        }
    }

    /// Paper: `GameTestTicker.tick()`, first half: enters `RUNNING` and returns
    /// the snapshot `testInfos.forEach` iterates (tests added mid-tick are not
    /// visited until the next tick, as with Paper's `CopyOnWriteArrayList`).
    pub fn begin_tick(&mut self) -> Vec<TestInfoId> {
        self.state = TickerState::Running;
        self.test_infos.clone()
    }

    /// Paper: `GameTestTicker.tick()`, second half: `removeIf(GameTestInfo::isDone)`,
    /// back to `IDLE`, and the deferred `clear()` if `clear()` was called while
    /// running. Returns `(removed_count, halted)`.
    pub fn end_tick<F>(&mut self, mut is_done: F) -> (usize, bool)
    where
        F: FnMut(TestInfoId) -> bool,
    {
        let before = self.test_infos.len();
        self.test_infos.retain(|&id| !is_done(id));
        let removed_count = before.saturating_sub(self.test_infos.len());
        let finishing_state = self.state;
        self.state = TickerState::Idle;
        let halted = finishing_state == TickerState::Halting;
        if halted {
            self.clear();
        }
        (removed_count, halted)
    }

    /// Paper: `GameTestTicker.tick()`.
    ///
    /// Iterates over a snapshot of active tests in deterministic order, invoking
    /// `tick_fn(id, &mut self)` for each. `tick_fn` returns `true` if the test
    /// is now done and should be removed (matching Paper's `removeIf(GameTestInfo::isDone)`).
    ///
    /// If `clear()` was called during `tick_fn`, defers clearing until the end
    /// of the tick and reports `halted = true`.
    pub fn tick_with<F>(&mut self, mut tick_fn: F) -> TickerTickResult
    where
        F: FnMut(TestInfoId, &mut Self) -> bool,
    {
        if self.test_infos.is_empty() {
            return TickerTickResult {
                ticked_count: 0,
                removed_count: 0,
                halted: false,
            };
        }

        let snapshot = self.begin_tick();
        let ticked_count = snapshot.len();
        let mut done = Vec::new();
        for id in snapshot {
            if tick_fn(id, self) {
                done.push(id);
            }
        }
        let (removed_count, halted) = self.end_tick(|id| done.contains(&id));

        TickerTickResult {
            ticked_count,
            removed_count,
            halted,
        }
    }
}

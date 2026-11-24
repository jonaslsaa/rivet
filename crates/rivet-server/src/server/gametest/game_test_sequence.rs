//! `net.minecraft.gametest.framework.GameTestSequence` (Paper 26.2).
//!
//! Mirrors `GameTestSequence.java`: ordered event execution where each event's
//! assertion either throws (the sequence stops for this tick) or completes, after
//! which the event's minimum/expected delay is checked against the tick delta.
//! Event bodies act on the parent `GameTestInfo` through [`SequenceParent`]
//! exactly where Paper calls `this.parent.fail(..)` / `this.parent.succeed()`.

use std::collections::VecDeque;

use crate::server::gametest::exception::{GameTestEvent, GameTestException};

/// Paper `test.error.sequence.not_completed` (en_us).
const SEQUENCE_NOT_COMPLETED: &str = "Test timed out before sequence completed";

/// The parent `GameTestInfo` operations a sequence event may perform
/// (Paper: the `GameTestInfo parent` field of `GameTestSequence`).
pub trait SequenceParent {
    /// Paper: `parent.getTick()`.
    fn tick(&self) -> i32;
    /// Paper: `parent.fail(GameTestException)`.
    fn fail(&mut self, error: GameTestException);
    /// Paper: `parent.succeed()`.
    fn succeed(&mut self);
}

/// Body of one [`GameTestSequence`] event (Paper: `GameTestEvent.assertion`).
///
/// Receives the parent and the sequence's current `lastTick`. Returning `Err`
/// is Paper's thrown exception: the event stays queued and the sequence stops
/// for this tick.
pub type SequenceAssertion =
    Box<dyn FnMut(&mut dyn SequenceParent, i32) -> Result<(), GameTestException>>;

/// `net.minecraft.gametest.framework.GameTestSequence` (Paper 26.2).
pub struct GameTestSequence {
    events: VecDeque<GameTestEvent<SequenceAssertion>>,
    last_tick: i32,
}

impl Default for GameTestSequence {
    fn default() -> Self {
        Self::new()
    }
}

/// Paper: `GameTestSequence.executeWithoutFail(Runnable)` — a
/// `GameTestAssertException` (or `GameTestAssertPosException`) fails the parent
/// immediately; any other exception propagates.
fn execute_without_fail<F>(
    parent: &mut dyn SequenceParent,
    assertion: &mut F,
) -> Result<(), GameTestException>
where
    F: FnMut(i32) -> Result<(), GameTestException>,
{
    match assertion(parent.tick()) {
        Ok(()) => Ok(()),
        Err(err @ (GameTestException::Assert(_) | GameTestException::AssertPos(_))) => {
            parent.fail(err);
            Ok(())
        }
        Err(other) => Err(other),
    }
}

fn not_completed(parent: &dyn SequenceParent) -> GameTestException {
    GameTestException::assert(SEQUENCE_NOT_COMPLETED, parent.tick())
}

impl GameTestSequence {
    pub fn new() -> Self {
        Self::with_start_tick(0)
    }

    /// Paper: `GameTestSequence(GameTestInfo parent)` sets `this.lastTick = parent.getTick()`.
    pub fn with_start_tick(start_tick: i32) -> Self {
        Self {
            events: VecDeque::new(),
            last_tick: start_tick,
        }
    }

    pub(crate) fn set_start_tick(&mut self, start_tick: i32) {
        self.last_tick = start_tick;
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Paper: `thenWaitUntil(Runnable assertion)`.
    pub fn then_wait_until<F>(mut self, mut assertion: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |parent, _| {
                assertion(parent.tick())
            })));
        self
    }

    /// Paper: `thenWaitUntil(long expectedDelay, Runnable assertion)`.
    pub fn then_wait_until_at<F>(mut self, expected_delay: i64, mut assertion: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events.push_back(GameTestEvent::create_at(
            expected_delay,
            Box::new(move |parent, _| assertion(parent.tick())),
        ));
        self
    }

    /// Paper: `thenWaitAtLeast(long minimumDelay, Runnable assertion)`.
    pub fn then_wait_at_least<F>(mut self, minimum_delay: i64, mut assertion: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events
            .push_back(GameTestEvent::create_with_minimum_delay(
                minimum_delay,
                Box::new(move |parent, _| assertion(parent.tick())),
            ));
        self
    }

    /// Paper: `thenIdle(int delta)` -> `thenExecuteAfter(delta, () -> {})`.
    pub fn then_idle(self, delta: i32) -> Self {
        self.then_execute_after(delta, |_| Ok(()))
    }

    /// Paper: `thenExecute(Runnable assertion)` — wrapped in `executeWithoutFail`.
    pub fn then_execute<F>(mut self, mut assertion: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |parent, _| {
                execute_without_fail(parent, &mut assertion)
            })));
        self
    }

    /// Paper: `thenExecuteAfter(int delta, Runnable after)`: throws
    /// `test.error.sequence.not_completed` while `parent.getTick() < lastTick + delta`.
    pub fn then_execute_after<F>(mut self, delta: i32, mut after: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |parent, last_tick| {
                if parent.tick() < last_tick.wrapping_add(delta) {
                    return Err(not_completed(parent));
                }
                execute_without_fail(parent, &mut after)
            })));
        self
    }

    /// Paper: `thenExecuteFor(int delta, Runnable check)`: runs `check` and throws
    /// `test.error.sequence.not_completed` while `parent.getTick() < lastTick + delta`,
    /// completing (without running `check`) once the delta has elapsed.
    pub fn then_execute_for<F>(mut self, delta: i32, mut check: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |parent, last_tick| {
                if parent.tick() < last_tick.wrapping_add(delta) {
                    execute_without_fail(parent, &mut check)?;
                    return Err(not_completed(parent));
                }
                Ok(())
            })));
        self
    }

    /// Paper: `thenSucceed()` -> `parent.succeed()`.
    pub fn then_succeed(mut self) -> Self {
        self.events
            .push_back(GameTestEvent::create(Box::new(|parent, _| {
                parent.succeed();
                Ok(())
            })));
        self
    }

    /// Paper: `thenFail(Supplier<GameTestException> e)` -> `parent.fail(e.get())`.
    pub fn then_fail<F>(mut self, mut error_fn: F) -> Self
    where
        F: FnMut() -> GameTestException + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |parent, _| {
                parent.fail(error_fn());
                Ok(())
            })));
        self
    }

    /// Paper: `tickAndContinue(int tick)` — swallows `GameTestAssertException`
    /// (incl. `GameTestAssertPosException`) while waiting.
    ///
    /// Any other exception escapes `GameTestInfo.tick` in Paper; Rivet records it
    /// as the parent's failure instead of unwinding the tick thread.
    pub fn tick_and_continue(&mut self, parent: &mut dyn SequenceParent, tick: i32) {
        match self.tick(parent, tick) {
            Ok(()) | Err(GameTestException::Assert(_) | GameTestException::AssertPos(_)) => {}
            Err(other) => parent.fail(other),
        }
    }

    /// Paper: `tickAndFailIfNotComplete(int tick)` — any assertion thrown fails the parent.
    pub fn tick_and_fail_if_not_complete(&mut self, parent: &mut dyn SequenceParent, tick: i32) {
        if let Err(err) = self.tick(parent, tick) {
            parent.fail(err);
        }
    }

    /// Paper: `GameTestSequence.tick(int tick)`.
    fn tick(
        &mut self,
        parent: &mut dyn SequenceParent,
        tick: i32,
    ) -> Result<(), GameTestException> {
        while let Some(event) = self.events.front_mut() {
            (event.assertion)(parent, self.last_tick)?;
            let Some(event) = self.events.pop_front() else {
                break;
            };
            let delay = tick.wrapping_sub(self.last_tick);
            let prev_tick = self.last_tick;
            self.last_tick = tick;
            if let Some(minimum_delay) = event.minimum_delay
                && minimum_delay > i64::from(delay)
            {
                parent.fail(GameTestException::assert(
                    format!(
                        "Succeeded before expected tick: expected to wait {}",
                        i64::from(prev_tick).wrapping_add(minimum_delay)
                    ),
                    tick,
                ));
                break;
            }

            if let Some(expected_delay) = event.expected_delay
                && expected_delay != i64::from(delay)
            {
                parent.fail(GameTestException::assert(
                    format!(
                        "Succeeded in invalid tick: expected {}",
                        i64::from(prev_tick).wrapping_add(expected_delay)
                    ),
                    tick,
                ));
                break;
            }
        }
        Ok(())
    }
}

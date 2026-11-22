//! `net.minecraft.gametest.framework.GameTestSequence` (Paper 26.2).
//!
//! Mirrors `GameTestSequence.java` and `GameTestSequence.Condition`:
//! ordered step execution with exact delay, minimum delay, idle, execute,
//! execute-after, execute-for, wait-until, succeed, fail, and trigger
//! semantics.

use std::collections::VecDeque;

use crate::server::gametest::exception::{GameTestEvent, GameTestException};

/// Callback executed for a single step in a [`GameTestSequence`].
///
/// Receives the current test tick (`tick_count`) and returns `Ok(SequenceStepEffect)`
/// or `Err(GameTestException)`.
pub type SequenceAssertion = Box<dyn FnMut(i32) -> Result<SequenceStepEffect, GameTestException>>;

/// Side effect produced by a sequence step on its parent `GameTestInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceStepEffect {
    None,
    Succeed,
}

/// `net.minecraft.gametest.framework.GameTestSequence` (Paper 26.2).
pub struct GameTestSequence {
    events: VecDeque<GameTestEvent<SequenceAssertion>>,
    last_tick: i64,
    completed_effect: Option<SequenceStepEffect>,
}

impl Default for GameTestSequence {
    fn default() -> Self {
        Self::new()
    }
}

impl GameTestSequence {
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            last_tick: 0,
            completed_effect: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Take any terminal effect (e.g. `Succeed`) produced during ticking.
    pub fn take_completed_effect(&mut self) -> Option<SequenceStepEffect> {
        self.completed_effect.take()
    }

    /// Paper: `thenWaitUntil(Runnable assertion)`.
    pub fn then_wait_until<F>(mut self, mut assertion: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |tick| {
                assertion(tick)?;
                Ok(SequenceStepEffect::None)
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
            Box::new(move |tick| {
                assertion(tick)?;
                Ok(SequenceStepEffect::None)
            }),
        ));
        self
    }

    /// Paper: `thenIdle(int delta)`.
    pub fn then_idle(mut self, delta: i32) -> Self {
        self.events
            .push_back(GameTestEvent::create_with_minimum_delay(
                i64::from(delta),
                Box::new(|_| Ok(SequenceStepEffect::None)),
            ));
        self
    }

    /// Paper: `thenExecute(Runnable task)`.
    pub fn then_execute<F>(self, task: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.then_wait_until(task)
    }

    /// Paper: `thenExecuteAfter(int tick, Runnable task)`.
    pub fn then_execute_after<F>(self, tick: i32, task: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        self.then_idle(tick).then_execute(task)
    }

    /// Paper: `thenExecuteFor(int tick, Runnable task)`.
    pub fn then_execute_for<F>(mut self, ticks: i32, mut task: F) -> Self
    where
        F: FnMut(i32) -> Result<(), GameTestException> + 'static,
    {
        let mut remaining = ticks;
        self.events
            .push_back(GameTestEvent::create(Box::new(move |tick| {
                if remaining > 0 {
                    task(tick)?;
                    remaining -= 1;
                    if remaining > 0 {
                        return Err(GameTestException::assert(
                            "sequence execute_for remaining",
                            tick,
                        ));
                    }
                }
                Ok(SequenceStepEffect::None)
            })));
        self
    }

    /// Paper: `thenSucceed()`.
    pub fn then_succeed(mut self) -> Self {
        self.events.push_back(GameTestEvent::create(Box::new(|_| {
            Ok(SequenceStepEffect::Succeed)
        })));
        self
    }

    /// Paper: `thenFail(Supplier<GameTestException> e)`.
    pub fn then_fail<F>(mut self, mut error_fn: F) -> Self
    where
        F: FnMut() -> GameTestException + 'static,
    {
        self.events
            .push_back(GameTestEvent::create(Box::new(move |_| Err(error_fn()))));
        self
    }

    /// Paper: `tickAndContinue(long tick)`.
    ///
    /// Swallows `GameTestAssertException` / `GameTestAssertPosException` while
    /// waiting before timeout; returns `None` on normal continuation.
    pub fn tick_and_continue(&mut self, tick: i64) -> Option<GameTestException> {
        match self.execute_without_fail(tick) {
            Ok(()) => None,
            Err(GameTestException::Assert(_) | GameTestException::AssertPos(_)) => None,
            Err(other) => Some(other),
        }
    }

    /// Paper: `tickAndFailIfNotComplete(long tick)`.
    ///
    /// Called when `tickCount > timeoutTicks`; returns any failure from the
    /// front unfinished event instead of swallowing it.
    pub fn tick_and_fail_if_not_complete(&mut self, tick: i64) -> Option<GameTestException> {
        self.execute_without_fail(tick).err()
    }

    /// Paper: `executeWithoutFail(long tick)`.
    fn execute_without_fail(&mut self, tick: i64) -> Result<(), GameTestException> {
        let mut last_tick = self.last_tick;
        while let Some(front) = self.events.front_mut() {
            if let Some(min_delay) = front.minimum_delay
                && tick.wrapping_sub(last_tick) < min_delay
            {
                return Ok(());
            }

            let effect = (front.assertion)(tick as i32)?;
            let expected_delay = front.expected_delay;
            let _ = self.events.pop_front();

            let delta = tick.wrapping_sub(last_tick);
            last_tick = tick;
            self.last_tick = tick;

            if effect == SequenceStepEffect::Succeed {
                self.completed_effect = Some(SequenceStepEffect::Succeed);
            }

            if let Some(expected) = expected_delay
                && expected != delta
            {
                return Err(GameTestException::assert(
                    format!(
                        "Succeeded in invalid tick: expected {}, but current tick is {tick}",
                        last_tick.wrapping_sub(delta).wrapping_add(expected)
                    ),
                    tick as i32,
                ));
            }
        }
        Ok(())
    }
}

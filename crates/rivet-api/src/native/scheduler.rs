//! Synchronous tick-thread scheduler for native Rust plugins (#507).
//!
//! Mirrors Paper's synchronous `CraftScheduler.mainThreadHeartbeat()` execution
//! model on the single tick thread (async/Folia schedulers are intentionally
//! out of scope):
//! - Delayed one-shot tasks and fixed-period repeating tasks.
//! - Deterministic execution order: `(next_run_tick, registration_seq)`.
//! - Reentrant scheduling and cancellation from inside running tasks without
//!   corrupting the active tick iteration (tasks scheduled during tick `T`
//!   never execute earlier than tick `T + 1`).
//! - Panic containment for both task execution and task `Drop`; a panicking
//!   task is automatically cancelled so it cannot panic-loop the tick thread.
//! - Automatic plugin-disable and shutdown cleanup.

use std::collections::BTreeSet;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::handle::{Handle, HandleError, HandleTable, PluginId};
use super::lifecycle::extract_panic_message_and_drop;

/// Marker type for handles referring to scheduled synchronous tasks.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct TaskMarker;

/// Generational handle to a scheduled synchronous task.
pub type TaskHandle = Handle<TaskMarker>;

/// Errors produced by [`SyncScheduler`] operations.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ScheduleError {
    ZeroPeriod,
    TickOverflow,
    Handle(HandleError),
}

impl From<HandleError> for ScheduleError {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPeriod => write!(f, "repeating task period must be >= 1 tick"),
            Self::TickOverflow => write!(f, "scheduled tick overflows u64"),
            Self::Handle(err) => write!(f, "task handle error: {err}"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// Panic captured while executing a scheduled task.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct TaskPanic {
    pub owner: PluginId,
    pub handle: TaskHandle,
    pub tick: u64,
    pub message: String,
}

/// Summary returned by [`SyncScheduler::tick`].
#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct TickReport {
    pub tick: u64,
    pub executed: usize,
    pub panics: Vec<TaskPanic>,
}

type TaskCallback = Box<dyn FnMut(&mut TaskContext<'_>)>;

struct PendingTaskSpec {
    owner: PluginId,
    delay_ticks: u64,
    period_ticks: Option<u64>,
    callback: TaskCallback,
}

/// Reentrant handle passed to a running synchronous task.
///
/// Operations performed through `TaskContext` are applied safely after the
/// callback returns, preserving tick-thread determinism and ownership checks.
pub struct TaskContext<'a> {
    owner: PluginId,
    self_handle: TaskHandle,
    current_tick: u64,
    cancel_self: bool,
    cancellations: &'a mut Vec<(PluginId, TaskHandle)>,
    new_tasks: &'a mut Vec<PendingTaskSpec>,
}

impl TaskContext<'_> {
    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.current_tick
    }

    #[must_use]
    pub const fn owner(&self) -> PluginId {
        self.owner
    }

    #[must_use]
    pub const fn task_handle(&self) -> TaskHandle {
        self.self_handle
    }

    /// Cancels the currently executing task (prevents further repeats).
    pub fn cancel_self(&mut self) {
        self.cancel_self = true;
    }

    /// Requests cancellation of another task owned by the same plugin.
    pub fn cancel_owned(&mut self, handle: TaskHandle) {
        if handle == self.self_handle {
            self.cancel_self = true;
        } else {
            self.cancellations.push((self.owner, handle));
        }
    }

    /// Schedules a follow-up delayed task owned by the same plugin.
    ///
    /// A `delay_ticks` of `0` executes on the next tick (`current_tick + 1`),
    /// preventing same-tick infinite reentrancy loops.
    pub fn schedule_delayed<F>(&mut self, delay_ticks: u64, callback: F)
    where
        F: FnMut(&mut TaskContext<'_>) + 'static,
    {
        self.new_tasks.push(PendingTaskSpec {
            owner: self.owner,
            delay_ticks: delay_ticks.max(1),
            period_ticks: None,
            callback: Box::new(callback),
        });
    }

    /// Schedules a follow-up repeating task owned by the same plugin.
    pub fn schedule_repeating<F>(
        &mut self,
        delay_ticks: u64,
        period_ticks: u64,
        callback: F,
    ) -> Result<(), ScheduleError>
    where
        F: FnMut(&mut TaskContext<'_>) + 'static,
    {
        if period_ticks == 0 {
            return Err(ScheduleError::ZeroPeriod);
        }
        self.new_tasks.push(PendingTaskSpec {
            owner: self.owner,
            delay_ticks: delay_ticks.max(1),
            period_ticks: Some(period_ticks),
            callback: Box::new(callback),
        });
        Ok(())
    }
}

struct TaskEntry {
    next_run_tick: u64,
    period_ticks: Option<u64>,
    seq: u64,
    callback: Option<TaskCallback>,
}

/// Deterministic synchronous tick-thread scheduler.
#[derive(Default)]
pub struct SyncScheduler {
    current_tick: u64,
    tasks: HandleTable<TaskMarker, TaskEntry>,
    /// Ordered priority queue: `(next_run_tick, seq, handle)`.
    queue: BTreeSet<(u64, u64, TaskHandle)>,
    next_seq: u64,
}

impl SyncScheduler {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn current_tick(&self) -> u64 {
        self.current_tick
    }

    #[must_use]
    pub const fn pending_task_count(&self) -> usize {
        self.tasks.live_count()
    }

    #[must_use]
    pub fn pending_task_count_for(&self, owner: PluginId) -> usize {
        self.tasks.live_count_for(owner)
    }

    /// Schedules a one-shot task to run after `delay_ticks`.
    ///
    /// A `delay_ticks` of `0` or `1` runs on the very next `tick()` call
    /// (matching Paper `CraftScheduler` where `currentTick` increments at the
    /// start of `mainThreadHeartbeat()`).
    pub fn schedule_delayed<F>(
        &mut self,
        owner: PluginId,
        delay_ticks: u64,
        callback: F,
    ) -> Result<TaskHandle, ScheduleError>
    where
        F: FnMut(&mut TaskContext<'_>) + 'static,
    {
        self.schedule_internal(owner, delay_ticks, None, Box::new(callback))
    }

    /// Schedules a repeating task starting after `delay_ticks` and repeating
    /// every `period_ticks` (`>= 1`).
    pub fn schedule_repeating<F>(
        &mut self,
        owner: PluginId,
        delay_ticks: u64,
        period_ticks: u64,
        callback: F,
    ) -> Result<TaskHandle, ScheduleError>
    where
        F: FnMut(&mut TaskContext<'_>) + 'static,
    {
        if period_ticks == 0 {
            return Err(ScheduleError::ZeroPeriod);
        }
        self.schedule_internal(owner, delay_ticks, Some(period_ticks), Box::new(callback))
    }

    fn schedule_internal(
        &mut self,
        owner: PluginId,
        delay_ticks: u64,
        period_ticks: Option<u64>,
        callback: TaskCallback,
    ) -> Result<TaskHandle, ScheduleError> {
        let next_run_tick = self
            .current_tick
            .checked_add(delay_ticks)
            .ok_or(ScheduleError::TickOverflow)?;
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let entry = TaskEntry {
            next_run_tick,
            period_ticks,
            seq,
            callback: Some(callback),
        };
        let handle = self.tasks.insert(owner, entry)?;
        self.queue.insert((next_run_tick, seq, handle));
        Ok(handle)
    }

    /// Cancels a single task after verifying generational validity and ownership.
    pub fn cancel(&mut self, owner: PluginId, handle: TaskHandle) -> Result<(), ScheduleError> {
        let entry = self.tasks.remove(handle, Some(owner))?;
        self.queue.remove(&(entry.next_run_tick, entry.seq, handle));
        drop_task_contained(entry.callback);
        Ok(())
    }

    /// Cancels and drops all tasks owned by `owner`.
    pub fn cancel_all_for_plugin(&mut self, owner: PluginId) -> usize {
        let removed = self.tasks.revoke_owned_by(owner);
        let count = removed.len();
        for entry in removed {
            self.queue
                .retain(|&(t, s, _)| !(t == entry.next_run_tick && s == entry.seq));
            drop_task_contained(entry.callback);
        }
        count
    }

    /// Advances the scheduler by one server tick and executes all ready tasks
    /// in `(next_run_tick, registration_seq)` order.
    pub fn tick(&mut self) -> TickReport {
        self.current_tick = self.current_tick.saturating_add(1);
        let now = self.current_tick;
        let mut report = TickReport {
            tick: now,
            executed: 0,
            panics: Vec::new(),
        };

        while let Some(&(due_tick, seq, handle)) = self.queue.first() {
            if due_tick > now {
                break;
            }
            self.queue.pop_first();

            let Ok(owner) = self.tasks.owner_of(handle) else {
                continue;
            };
            let Ok(entry) = self.tasks.get_mut(handle, None) else {
                continue;
            };
            let period_ticks = entry.period_ticks;
            let Some(mut callback) = entry.callback.take() else {
                continue;
            };

            let mut cancellations = Vec::new();
            let mut new_tasks = Vec::new();
            let mut cancel_self = false;

            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let mut ctx = TaskContext {
                    owner,
                    self_handle: handle,
                    current_tick: now,
                    cancel_self: false,
                    cancellations: &mut cancellations,
                    new_tasks: &mut new_tasks,
                };
                callback(&mut ctx);
                cancel_self = ctx.cancel_self;
            }));

            report.executed += 1;

            let panicked = match outcome {
                Ok(()) => false,
                Err(payload) => {
                    let message = extract_panic_message_and_drop(payload);
                    report.panics.push(TaskPanic {
                        owner,
                        handle,
                        tick: now,
                        message,
                    });
                    true
                }
            };

            // Reschedule repeating task if it did not panic, did not cancel itself, and still exists.
            if !panicked && !cancel_self {
                if let Some(period) = period_ticks {
                    if let Some(next_run_tick) = now.checked_add(period) {
                        if let Ok(slot) = self.tasks.get_mut(handle, Some(owner)) {
                            slot.next_run_tick = next_run_tick;
                            slot.callback = Some(callback);
                            self.queue.insert((next_run_tick, seq, handle));
                        } else {
                            drop_task_contained(Some(callback));
                        }
                    } else {
                        let _ = self.tasks.remove(handle, Some(owner));
                        drop_task_contained(Some(callback));
                    }
                } else {
                    let _ = self.tasks.remove(handle, Some(owner));
                    drop_task_contained(Some(callback));
                }
            } else {
                let _ = self.tasks.remove(handle, Some(owner));
                drop_task_contained(Some(callback));
            }

            // Apply queued cancellations from the task.
            for (cancel_owner, cancel_handle) in cancellations {
                let _ = self.cancel(cancel_owner, cancel_handle);
            }

            // Enqueue newly scheduled tasks from the task (all have delay >= 1).
            for spec in new_tasks {
                let _ = self.schedule_internal(
                    spec.owner,
                    spec.delay_ticks,
                    spec.period_ticks,
                    spec.callback,
                );
            }
        }

        report
    }

    /// Cancels and drops all pending tasks during server shutdown.
    pub fn shutdown(&mut self) -> usize {
        let handles: Vec<TaskHandle> = self.queue.iter().map(|&(_, _, h)| h).collect();
        self.queue.clear();
        let mut count = 0usize;
        for handle in handles {
            if let Ok(entry) = self.tasks.remove(handle, None) {
                drop_task_contained(entry.callback);
                count += 1;
            }
        }
        count
    }
}

fn drop_task_contained(callback: Option<TaskCallback>) {
    if let Some(cb) = callback
        && let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(cb)))
    {
        let _ = extract_panic_message_and_drop(payload);
    }
}

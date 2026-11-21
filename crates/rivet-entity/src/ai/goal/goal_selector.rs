//! Paper 26.2 `GoalSelector` priority scheduling, flag locking, and EAR 2 inactive ticking.
//!
//! Mirrors `net.minecraft.world.entity.ai.goal.GoalSelector` and the
//! `Mob.serverAiStep` selector cadence (`tick_mob_selectors`).

use super::goal::{Goal, GoalFlag, GoalFlags, GoalId, WrappedGoal};

/// Sentinel priority of Paper's `GoalSelector.NO_GOAL` (`Integer.MAX_VALUE`).
const NO_GOAL_PRIORITY: i32 = i32::MAX;

/// Mirrors `net.minecraft.world.entity.ai.goal.GoalSelector`.
pub struct GoalSelector<C> {
    locked_flags: [Option<GoalId>; GoalFlag::COUNT],
    available_goals: Vec<WrappedGoal<C>>,
    disabled_flags: GoalFlags,
    cur_rate: i32,
    next_id: u64,
}

impl<C> Default for GoalSelector<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> GoalSelector<C> {
    pub fn new() -> Self {
        Self {
            locked_flags: [None; GoalFlag::COUNT],
            available_goals: Vec::new(),
            disabled_flags: GoalFlags::EMPTY,
            cur_rate: 0,
            next_id: 1,
        }
    }

    /// Registers a goal with the given priority (lower integer = higher priority).
    ///
    /// Preserves insertion order, matching Paper's `ObjectLinkedOpenHashSet`.
    pub fn add_goal(&mut self, priority: i32, goal: impl Goal<C> + 'static) -> GoalId {
        self.add_boxed_goal(priority, Box::new(goal))
    }

    /// Registers a pre-boxed goal with the given priority.
    pub fn add_boxed_goal(&mut self, priority: i32, goal: Box<dyn Goal<C>>) -> GoalId {
        let id = GoalId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        self.available_goals
            .push(WrappedGoal::new(id, priority, goal));
        id
    }

    /// Stops and removes a single goal by its [`GoalId`].
    pub fn remove_goal(&mut self, id: GoalId, ctx: &mut C) {
        self.remove_all_goals(|wrapped| wrapped.id() == id, ctx);
    }

    /// Stops every matching running goal and removes all matching goals.
    ///
    /// Mirrors `GoalSelector.removeAllGoals(Predicate<Goal>)`.
    pub fn remove_all_goals<F>(&mut self, mut predicate: F, ctx: &mut C)
    where
        F: FnMut(&WrappedGoal<C>) -> bool,
    {
        for goal in &mut self.available_goals {
            if predicate(goal) && goal.is_running() {
                goal.stop(ctx);
            }
        }
        self.available_goals.retain(|goal| !predicate(goal));
    }

    /// Paper EAR 2 `GoalSelector.inactiveTick()`.
    pub fn inactive_tick(&mut self) -> bool {
        self.cur_rate = self.cur_rate.wrapping_add(1);
        self.cur_rate % 3 == 0
    }

    /// Paper EAR 2 `GoalSelector.hasTasks()`.
    pub fn has_tasks(&self) -> bool {
        self.available_goals.iter().any(WrappedGoal::is_running)
    }

    pub fn available_goals(&self) -> &[WrappedGoal<C>] {
        &self.available_goals
    }

    pub fn disable_control_flag(&mut self, flag: GoalFlag) {
        self.disabled_flags.add_unchecked(flag);
    }

    pub fn enable_control_flag(&mut self, flag: GoalFlag) {
        self.disabled_flags.remove_unchecked(flag);
    }

    pub fn set_control_flag(&mut self, flag: GoalFlag, enabled: bool) {
        if enabled {
            self.enable_control_flag(flag);
        } else {
            self.disable_control_flag(flag);
        }
    }

    pub fn is_control_flag_disabled(&self, flag: GoalFlag) -> bool {
        self.disabled_flags.has_element(flag)
    }

    pub fn disabled_flags(&self) -> GoalFlags {
        self.disabled_flags
    }

    /// Returns the [`GoalId`] currently locking `flag`, if any.
    pub fn locked_flag(&self, flag: GoalFlag) -> Option<GoalId> {
        self.locked_flags[flag.ordinal()]
    }

    fn index_of(&self, id: GoalId) -> Option<usize> {
        self.available_goals.iter().position(|g| g.id() == id)
    }

    fn goal_can_be_replaced_for_all_flags(
        &self,
        flags: GoalFlags,
        candidate_priority: i32,
    ) -> bool {
        for flag in flags.iter() {
            match self.locked_flags[flag.ordinal()] {
                Some(locked_id) => {
                    if let Some(idx) = self.index_of(locked_id) {
                        if !self.available_goals[idx].can_be_replaced_by(candidate_priority) {
                            return false;
                        }
                    } else if candidate_priority == NO_GOAL_PRIORITY {
                        return false;
                    }
                }
                None => {
                    // NO_GOAL has priority Integer.MAX_VALUE and isInterruptable() == true.
                    if candidate_priority == NO_GOAL_PRIORITY {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Runs Paper 26.2 `GoalSelector.tick()`:
    /// 1. `goalCleanup`: stop running goals with disabled flags or `!canContinueToUse()`,
    ///    then clear `lockedFlags` entries whose goal is no longer running.
    /// 2. `goalUpdate`: in insertion order, start eligible non-running goals after stopping
    ///    any conflicting locked goals in `GoalFlag` ordinal order.
    /// 3. `goalTick`: `tickRunningGoals(true)`.
    pub fn tick(&mut self, ctx: &mut C) {
        // Phase 1: goalCleanup
        let disabled = self.disabled_flags;
        for goal in &mut self.available_goals {
            if goal.is_running()
                && (goal.flags().has_common_elements(disabled) || !goal.can_continue_to_use(ctx))
            {
                goal.stop(ctx);
            }
        }

        for slot in &mut self.locked_flags {
            if let Some(locked_id) = *slot {
                let still_running = self
                    .available_goals
                    .iter()
                    .find(|g| g.id() == locked_id)
                    .is_some_and(WrappedGoal::is_running);
                if !still_running {
                    *slot = None;
                }
            }
        }

        // Phase 2: goalUpdate
        let len = self.available_goals.len();
        for idx in 0..len {
            let (is_running, flags, priority, goal_id) = {
                let goal = &self.available_goals[idx];
                (goal.is_running(), goal.flags(), goal.priority(), goal.id())
            };

            if !is_running
                && !flags.has_common_elements(self.disabled_flags)
                && self.goal_can_be_replaced_for_all_flags(flags, priority)
                && self.available_goals[idx].can_use(ctx)
            {
                for flag in flags.iter() {
                    if let Some(current_id) = self.locked_flags[flag.ordinal()]
                        && let Some(current_idx) = self.index_of(current_id)
                    {
                        self.available_goals[current_idx].stop(ctx);
                    }
                    self.locked_flags[flag.ordinal()] = Some(goal_id);
                }

                self.available_goals[idx].start(ctx);
            }
        }

        // Phase 3: goalTick
        self.tick_running_goals(ctx, true);
    }

    /// Mirrors `GoalSelector.tickRunningGoals(boolean forceTickAllRunningGoals)`.
    pub fn tick_running_goals(&mut self, ctx: &mut C, force_tick_all_running_goals: bool) {
        for goal in &mut self.available_goals {
            if goal.is_running()
                && (force_tick_all_running_goals || goal.requires_update_every_tick())
            {
                goal.tick(ctx);
            }
        }
    }
}

/// Mirrors `Mob.serverAiStep` selector scheduling cadence in Paper 26.2 (`Mob.java:825-840`):
/// - `idBasedTickCount = tickCount + entityId` (wrapping `i32`).
/// - If `idBasedTickCount % 2 != 0 && tickCount > 1`, only `tickRunningGoals(false)` runs.
/// - Otherwise full `tick()` runs.
/// - `targetSelector` ALWAYS runs before `goalSelector`.
pub fn tick_mob_selectors<C>(
    target_selector: &mut GoalSelector<C>,
    goal_selector: &mut GoalSelector<C>,
    ctx: &mut C,
    tick_count: i32,
    entity_id: i32,
) {
    let id_based_tick_count = tick_count.wrapping_add(entity_id);
    if id_based_tick_count % 2 != 0 && tick_count > 1 {
        target_selector.tick_running_goals(ctx, false);
        goal_selector.tick_running_goals(ctx, false);
    } else {
        target_selector.tick(ctx);
        goal_selector.tick(ctx);
    }
}

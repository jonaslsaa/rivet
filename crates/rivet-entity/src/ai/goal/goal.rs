//! Core `Goal`, `GoalFlag`, `GoalFlags`, and `WrappedGoal` abstractions.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.entity.ai.goal.Goal`
//! - `net.minecraft.world.entity.ai.goal.WrappedGoal`

use std::fmt;

/// Control flags claimed by a goal (`net.minecraft.world.entity.ai.goal.Goal.Flag`).
///
/// Paper 26.2 adds `UNKNOWN_BEHAVIOR` at ordinal 0 so that no goal ever has an
/// empty flag set (`Goal()` and `Goal.setFlags` both fall back to `UNKNOWN_BEHAVIOR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum GoalFlag {
    UnknownBehavior = 0,
    Move = 1,
    Look = 2,
    Jump = 3,
    Target = 4,
}

impl GoalFlag {
    pub const COUNT: usize = 5;

    pub const ALL: [GoalFlag; Self::COUNT] = [
        GoalFlag::UnknownBehavior,
        GoalFlag::Move,
        GoalFlag::Look,
        GoalFlag::Jump,
        GoalFlag::Target,
    ];

    #[inline]
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn bit(self) -> u8 {
        1u8 << (self as u8)
    }

    #[inline]
    pub const fn from_ordinal(ordinal: usize) -> Option<Self> {
        match ordinal {
            0 => Some(Self::UnknownBehavior),
            1 => Some(Self::Move),
            2 => Some(Self::Look),
            3 => Some(Self::Jump),
            4 => Some(Self::Target),
            _ => None,
        }
    }
}

/// Compact bitset of [`GoalFlag`], mirroring Paper's
/// `ca.spottedleaf.moonrise.common.set.OptimizedSmallEnumSet<Goal.Flag>`.
///
/// Iteration order matches `Goal.Flag.values()` ordinal order (via trailing zeros
/// of the backing bitmask, identical to Paper's `Long.numberOfTrailingZeros`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GoalFlags(u8);

impl GoalFlags {
    pub const EMPTY: Self = Self(0);
    pub const UNKNOWN_BEHAVIOR: Self = Self(GoalFlag::UnknownBehavior.bit());
    pub const MOVE: Self = Self(GoalFlag::Move.bit());
    pub const LOOK: Self = Self(GoalFlag::Look.bit());
    pub const JUMP: Self = Self(GoalFlag::Jump.bit());
    pub const TARGET: Self = Self(GoalFlag::Target.bit());

    /// Creates a normalized goal flag set: if `flags` is empty, inserts
    /// [`GoalFlag::UnknownBehavior`] (Paper 26.2 `Goal()` / `Goal.setFlags`).
    #[inline]
    pub const fn for_goal(flags: Self) -> Self {
        if flags.0 == 0 {
            Self::UNKNOWN_BEHAVIOR
        } else {
            flags
        }
    }

    #[inline]
    pub const fn from_flag(flag: GoalFlag) -> Self {
        Self(flag.bit())
    }

    #[inline]
    pub const fn from_slice(flags: &[GoalFlag]) -> Self {
        let mut mask = 0u8;
        let mut i = 0;
        while i < flags.len() {
            mask |= flags[i].bit();
            i += 1;
        }
        Self(mask)
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    #[inline]
    pub const fn has_element(self, flag: GoalFlag) -> bool {
        (self.0 & flag.bit()) != 0
    }

    #[inline]
    pub const fn has_common_elements(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    #[inline]
    pub fn add_unchecked(&mut self, flag: GoalFlag) {
        self.0 |= flag.bit();
    }

    #[inline]
    pub fn remove_unchecked(&mut self, flag: GoalFlag) {
        self.0 &= !flag.bit();
    }

    #[inline]
    pub fn set_for_goal(&mut self, required_control_flags: Self) {
        *self = Self::for_goal(required_control_flags);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    #[inline]
    pub fn iter(self) -> GoalFlagsIter {
        GoalFlagsIter { bits: self.0 }
    }
}

/// Iterates set [`GoalFlag`]s in ascending enum ordinal order.
pub struct GoalFlagsIter {
    bits: u8,
}

impl Iterator for GoalFlagsIter {
    type Item = GoalFlag;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.bits == 0 {
            return None;
        }
        let tz = self.bits.trailing_zeros() as usize;
        self.bits &= self.bits - 1;
        GoalFlag::from_ordinal(tz)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.bits.count_ones() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for GoalFlagsIter {}

/// Exact Paper 26.2 `Goal.reducedTickDelay(int ticks)` = `Mth.positiveCeilDiv(ticks, 2)`
/// = `-Math.floorDiv(-ticks, 2)`.
#[inline]
pub const fn reduced_tick_delay(ticks: i32) -> i32 {
    ticks.wrapping_neg().div_euclid(2).wrapping_neg()
}

/// Exact Paper 26.2 `Goal.adjustedTickDelay(int ticks)`.
#[inline]
pub const fn adjusted_tick_delay(requires_update_every_tick: bool, ticks: i32) -> i32 {
    if requires_update_every_tick {
        ticks
    } else {
        reduced_tick_delay(ticks)
    }
}

/// Trait mirroring `net.minecraft.world.entity.ai.goal.Goal`.
pub trait Goal<C> {
    fn can_use(&mut self, ctx: &mut C) -> bool;

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        self.can_use(ctx)
    }

    fn is_interruptable(&self) -> bool {
        true
    }

    fn start(&mut self, _ctx: &mut C) {}

    fn stop(&mut self, _ctx: &mut C) {}

    fn requires_update_every_tick(&self) -> bool {
        false
    }

    fn tick(&mut self, _ctx: &mut C) {}

    /// Returns the goal's control flags.
    ///
    /// Per Paper 26.2, a goal's effective flag set is never empty — an empty set
    /// defaults to [`GoalFlags::UNKNOWN_BEHAVIOR`].
    fn flags(&self) -> GoalFlags;

    /// Replaces the goal's control flags.
    ///
    /// Implementors should normalize empty sets via [`GoalFlags::for_goal`] so that
    /// `flags()`, `add_flag()`, and [`WrappedGoal::set_flags`] always observe the
    /// mutated flag state.
    fn set_flags(&mut self, required_control_flags: GoalFlags);

    fn has_flag(&self, flag: GoalFlag) -> bool {
        GoalFlags::for_goal(self.flags()).has_element(flag)
    }

    fn add_flag(&mut self, flag: GoalFlag) {
        let mut current = self.flags();
        current.add_unchecked(flag);
        self.set_flags(current);
    }

    fn adjusted_tick_delay(&self, ticks: i32) -> i32 {
        adjusted_tick_delay(self.requires_update_every_tick(), ticks)
    }

    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Unique handle identifying a registered [`WrappedGoal`] inside a [`crate::ai::goal::GoalSelector`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GoalId(pub u64);

/// Mirrors `net.minecraft.world.entity.ai.goal.WrappedGoal`.
pub struct WrappedGoal<C> {
    id: GoalId,
    priority: i32,
    is_running: bool,
    goal: Box<dyn Goal<C>>,
}

impl<C> fmt::Debug for WrappedGoal<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrappedGoal")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("is_running", &self.is_running)
            .field("name", &self.goal.name())
            .field("flags", &self.flags())
            .finish()
    }
}

impl<C> WrappedGoal<C> {
    pub fn new(id: GoalId, priority: i32, goal: Box<dyn Goal<C>>) -> Self {
        Self {
            id,
            priority,
            is_running: false,
            goal,
        }
    }

    #[inline]
    pub fn id(&self) -> GoalId {
        self.id
    }

    #[inline]
    pub fn priority(&self) -> i32 {
        self.priority
    }

    #[inline]
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    #[inline]
    pub fn goal(&self) -> &dyn Goal<C> {
        &*self.goal
    }

    #[inline]
    pub fn goal_mut(&mut self) -> &mut dyn Goal<C> {
        &mut *self.goal
    }

    /// Mirrors `WrappedGoal.canBeReplacedBy(WrappedGoal)`.
    #[inline]
    pub fn can_be_replaced_by(&self, candidate_priority: i32) -> bool {
        self.is_interruptable() && candidate_priority < self.priority
    }

    #[inline]
    pub fn can_use(&mut self, ctx: &mut C) -> bool {
        self.goal.can_use(ctx)
    }

    #[inline]
    pub fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        self.goal.can_continue_to_use(ctx)
    }

    #[inline]
    pub fn is_interruptable(&self) -> bool {
        self.goal.is_interruptable()
    }

    #[inline]
    pub fn start(&mut self, ctx: &mut C) {
        if !self.is_running {
            self.is_running = true;
            self.goal.start(ctx);
        }
    }

    #[inline]
    pub fn stop(&mut self, ctx: &mut C) {
        if self.is_running {
            self.is_running = false;
            self.goal.stop(ctx);
        }
    }

    #[inline]
    pub fn requires_update_every_tick(&self) -> bool {
        self.goal.requires_update_every_tick()
    }

    #[inline]
    pub fn tick(&mut self, ctx: &mut C) {
        self.goal.tick(ctx);
    }

    #[inline]
    pub fn flags(&self) -> GoalFlags {
        GoalFlags::for_goal(self.goal.flags())
    }

    #[inline]
    pub fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.goal
            .set_flags(GoalFlags::for_goal(required_control_flags));
    }
}

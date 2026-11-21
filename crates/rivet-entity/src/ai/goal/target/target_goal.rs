//! Shared superclass state for Paper 26.2 `net.minecraft.world.entity.ai.goal.target.TargetGoal`.

use crate::ai::goal::context::{
    EntityId, EntitySnapshot, GoalContext, GoalRandom, TargetReason, TargetingConditions,
};
use crate::ai::goal::goal::{GoalFlags, reduced_tick_delay};

/// Embedded superclass state mirroring `TargetGoal` per `PORTING.md`.
#[derive(Debug, Clone)]
pub struct TargetGoalState {
    pub flags: GoalFlags,
    pub must_see: bool,
    pub must_reach: bool,
    pub reach_cache: i32,
    pub reach_cache_time: i32,
    pub unseen_ticks: i32,
    pub target_mob: Option<EntityId>,
    pub unseen_memory_ticks: i32,
}

impl TargetGoalState {
    pub const UNSEEN_MEMORY_TICKS: i32 = 60;
    pub const EMPTY_REACH_CACHE: i32 = 0;
    pub const CAN_REACH_CACHE: i32 = 1;
    pub const CANT_REACH_CACHE: i32 = 2;

    pub fn new(must_see: bool, must_reach: bool) -> Self {
        Self {
            flags: GoalFlags::TARGET,
            must_see,
            must_reach,
            reach_cache: Self::EMPTY_REACH_CACHE,
            reach_cache_time: 0,
            unseen_ticks: 0,
            target_mob: None,
            unseen_memory_ticks: Self::UNSEEN_MEMORY_TICKS,
        }
    }

    pub fn set_unseen_memory_ticks(&mut self, unseen_memory_ticks: i32) {
        self.unseen_memory_ticks = unseen_memory_ticks;
    }

    /// Mirrors `TargetGoal.canContinueToUse()`.
    pub fn can_continue_to_use<C: GoalContext>(&mut self, ctx: &mut C) -> bool {
        let target = ctx
            .target()
            .or_else(|| self.target_mob.and_then(|id| ctx.entity_snapshot(id)));
        let Some(target) = target else {
            return false;
        };
        if !ctx.can_attack(target.id) {
            return false;
        }
        if let (Some(mob_team), Some(target_team)) = (ctx.team(), target.team)
            && mob_team == target_team
        {
            self.target_mob = None;
            return false;
        }

        let follow_distance = ctx.follow_range();
        if ctx.pos().distance_to_sqr(target.pos) > follow_distance * follow_distance {
            return false;
        }

        if self.must_see {
            if ctx.has_line_of_sight(target.id) {
                self.unseen_ticks = 0;
            } else {
                self.unseen_ticks = self.unseen_ticks.wrapping_add(1);
                if self.unseen_ticks > reduced_tick_delay(self.unseen_memory_ticks) {
                    return false;
                }
            }
        }

        ctx.set_target(Some(target.id), TargetReason::ClosestEntity);
        true
    }

    /// Mirrors `TargetGoal.start()`.
    pub fn start(&mut self) {
        self.reach_cache = Self::EMPTY_REACH_CACHE;
        self.reach_cache_time = 0;
        self.unseen_ticks = 0;
    }

    /// Mirrors `TargetGoal.stop()`.
    pub fn stop<C: GoalContext>(&mut self, ctx: &mut C) {
        ctx.set_target(None, TargetReason::ForgotTarget);
        self.target_mob = None;
    }

    /// Mirrors `TargetGoal.canAttack(LivingEntity, TargetingConditions)`.
    pub fn can_attack<C: GoalContext>(
        &mut self,
        ctx: &mut C,
        target: Option<EntitySnapshot>,
        conditions: TargetingConditions,
    ) -> bool {
        let Some(target) = target else {
            return false;
        };
        if !ctx.test_targeting_conditions(target.id, conditions) {
            return false;
        }
        if !ctx.is_within_home(target.block_pos) {
            return false;
        }
        if self.must_reach {
            self.reach_cache_time = self.reach_cache_time.wrapping_sub(1);
            if self.reach_cache_time <= 0 {
                self.reach_cache = Self::EMPTY_REACH_CACHE;
            }
            if self.reach_cache == Self::EMPTY_REACH_CACHE {
                self.reach_cache = if self.can_reach(ctx, target) {
                    Self::CAN_REACH_CACHE
                } else {
                    Self::CANT_REACH_CACHE
                };
            }
            if self.reach_cache == Self::CANT_REACH_CACHE {
                return false;
            }
        }
        true
    }

    /// Mirrors `TargetGoal.canReach(LivingEntity)`.
    fn can_reach<C: GoalContext>(&mut self, ctx: &mut C, target: EntitySnapshot) -> bool {
        self.reach_cache_time = reduced_tick_delay(10_i32.wrapping_add(ctx.random().next_int(5)));
        let Some(path) = ctx.nav_create_path_to_entity(target.id, 0) else {
            return false;
        };
        let Some(end_node) = path.end_node else {
            return false;
        };
        let xx = end_node.x.wrapping_sub(target.block_pos.x);
        let zz = end_node.z.wrapping_sub(target.block_pos.z);
        ((xx * xx + zz * zz) as f64) <= 2.25
    }
}

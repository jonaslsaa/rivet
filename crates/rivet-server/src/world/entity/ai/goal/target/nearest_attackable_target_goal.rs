//! Mirrors `net.minecraft.world.entity.ai.goal.target.NearestAttackableTargetGoal`.

use crate::world::entity::ai::goal::context::{
    EntityTypeId, GoalContext, GoalRandom, TargetReason, TargetingConditions,
};
use crate::world::entity::ai::goal::goal::{Goal, GoalFlags, reduced_tick_delay};

use super::target_goal::TargetGoalState;
use crate::world::entity::entity_id::EntityId;
use rivet_registry::world::phys::AABB;

/// Target filter for [`NearestAttackableTargetGoal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackableTargetType {
    Player,
    EntityType(EntityTypeId),
}

/// Acquires the nearest valid player or entity of a specific type as combat target.
#[derive(Debug, Clone)]
pub struct NearestAttackableTargetGoal {
    base: TargetGoalState,
    target_type: AttackableTargetType,
    random_interval: i32,
    target: Option<EntityId>,
    target_conditions: TargetingConditions,
}

impl NearestAttackableTargetGoal {
    pub const DEFAULT_RANDOM_INTERVAL: i32 = 10;

    pub fn new(target_type: AttackableTargetType, must_see: bool) -> Self {
        Self::with_options(target_type, Self::DEFAULT_RANDOM_INTERVAL, must_see, false)
    }

    pub fn with_options(
        target_type: AttackableTargetType,
        random_interval: i32,
        must_see: bool,
        must_reach: bool,
    ) -> Self {
        Self {
            base: TargetGoalState::new(must_see, must_reach),
            target_type,
            random_interval: reduced_tick_delay(random_interval),
            target: None,
            target_conditions: TargetingConditions::for_combat(),
        }
    }

    pub fn target(&self) -> Option<EntityId> {
        self.target
    }

    pub fn set_target(&mut self, target: Option<EntityId>) {
        self.target = target;
    }

    fn get_target_search_area<C: GoalContext>(ctx: &C, follow_distance: f64) -> AABB {
        ctx.bounding_box()
            .inflate_xyz(follow_distance, follow_distance, follow_distance)
    }

    fn find_target<C: GoalContext>(&mut self, ctx: &C) {
        let follow_distance = ctx.follow_range();
        let conditions = self.target_conditions.range(follow_distance);
        let pos = ctx.pos();
        let eye_y = ctx.eye_y();
        match self.target_type {
            AttackableTargetType::Player => {
                self.target = ctx
                    .nearest_player(conditions, pos.x, eye_y, pos.z)
                    .map(|e| e.id);
            }
            AttackableTargetType::EntityType(entity_type) => {
                let area = Self::get_target_search_area(ctx, follow_distance);
                self.target = ctx
                    .nearest_entity_of_type(entity_type, area, conditions, pos.x, eye_y, pos.z)
                    .map(|e| e.id);
            }
        }
    }
}

impl<C: GoalContext> Goal<C> for NearestAttackableTargetGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        if self.random_interval > 0 && ctx.random().next_int(self.random_interval) != 0 {
            return false;
        }
        self.find_target(ctx);
        self.target.is_some()
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        self.base.can_continue_to_use(ctx)
    }

    fn start(&mut self, ctx: &mut C) {
        let reason = match self.target_type {
            AttackableTargetType::Player => TargetReason::ClosestPlayer,
            AttackableTargetType::EntityType(_) => TargetReason::ClosestEntity,
        };
        ctx.set_target(self.target, reason);
        self.base.start();
    }

    fn stop(&mut self, ctx: &mut C) {
        self.base.stop(ctx);
    }

    fn flags(&self) -> GoalFlags {
        self.base.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.base.flags.set_for_goal(required_control_flags);
    }
}

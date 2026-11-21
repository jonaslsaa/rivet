//! Mirrors `net.minecraft.world.entity.ai.goal.target.OwnerHurtByTargetGoal`.

use crate::ai::goal::context::{EntityId, GoalContext, TargetReason, TargetingConditions};
use crate::ai::goal::goal::{Goal, GoalFlags};

use super::target_goal::TargetGoalState;

/// Defends a tamable mob's owner by targeting whoever last hurt the owner.
#[derive(Debug, Clone)]
pub struct OwnerHurtByTargetGoal {
    base: TargetGoalState,
    owner_last_hurt_by: Option<EntityId>,
    timestamp: i32,
}

impl Default for OwnerHurtByTargetGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnerHurtByTargetGoal {
    pub fn new() -> Self {
        Self {
            base: TargetGoalState::new(false, false),
            owner_last_hurt_by: None,
            timestamp: 0,
        }
    }
}

impl<C: GoalContext> Goal<C> for OwnerHurtByTargetGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        if !ctx.is_tame() || ctx.is_ordered_to_sit() {
            return false;
        }
        let Some(owner_id) = ctx.owner_id() else {
            return false;
        };
        let Some((attacker, hurt_timestamp)) = ctx.owner_last_hurt_by_mob() else {
            self.owner_last_hurt_by = None;
            return false;
        };
        self.owner_last_hurt_by = Some(attacker.id);
        hurt_timestamp != self.timestamp
            && self
                .base
                .can_attack(ctx, Some(attacker), TargetingConditions::DEFAULT)
            && ctx.wants_to_attack(attacker.id, owner_id)
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        self.base.can_continue_to_use(ctx)
    }

    fn start(&mut self, ctx: &mut C) {
        ctx.set_target(self.owner_last_hurt_by, TargetReason::TargetAttackedOwner);
        self.base.target_mob = self.owner_last_hurt_by;
        if let Some((_, hurt_timestamp)) = ctx.owner_last_hurt_by_mob() {
            self.timestamp = hurt_timestamp;
        }
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

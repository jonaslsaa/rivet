//! Mirrors `net.minecraft.world.entity.ai.goal.target.HurtByTargetGoal`.

use crate::world::entity::ai::goal::context::{
    EntityTypeId, GoalContext, TargetReason, TargetingConditions,
};
use crate::world::entity::ai::goal::goal::{Goal, GoalFlags};

use super::target_goal::TargetGoalState;
use rivet_registry::world::phys::AABB;

/// Retaliates against the mob that last hurt this entity, with optional pack alerting.
#[derive(Debug, Clone)]
pub struct HurtByTargetGoal {
    base: TargetGoalState,
    alert_same_type: bool,
    timestamp: i32,
    to_ignore_damage: Vec<EntityTypeId>,
    to_ignore_alert: Vec<EntityTypeId>,
}

impl HurtByTargetGoal {
    pub const HURT_BY_TARGETING: TargetingConditions = TargetingConditions::for_combat()
        .ignore_line_of_sight()
        .ignore_invisibility_testing();
    pub const ALERT_RANGE_Y: f64 = 10.0;
    pub const RETALIATION_UNSEEN_MEMORY_TICKS: i32 = 300;

    pub fn new(to_ignore_damage: Vec<EntityTypeId>) -> Self {
        Self {
            base: TargetGoalState::new(true, false),
            alert_same_type: false,
            timestamp: 0,
            to_ignore_damage,
            to_ignore_alert: Vec::new(),
        }
    }

    pub fn set_alert_others(mut self, to_ignore_alert: Vec<EntityTypeId>) -> Self {
        self.alert_same_type = true;
        self.to_ignore_alert = to_ignore_alert;
        self
    }

    fn alert_others<C: GoalContext>(
        &self,
        ctx: &mut C,
        attacker_id: crate::world::entity::entity_id::EntityId,
    ) {
        let within = ctx.follow_range();
        let search_area = AABB::unit_cube_from_lower_corner(&ctx.pos()).inflate_xyz(
            within,
            Self::ALERT_RANGE_Y,
            within,
        );
        ctx.alert_nearby_same_type(search_area, attacker_id, &self.to_ignore_alert);
    }
}

impl<C: GoalContext> Goal<C> for HurtByTargetGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        let timestamp = ctx.last_hurt_by_mob_timestamp();
        let Some(last_hurt_by_mob) = ctx.last_hurt_by_mob() else {
            return false;
        };
        if timestamp == self.timestamp {
            return false;
        }
        if last_hurt_by_mob.is_player && ctx.universal_anger_enabled() {
            return false;
        }
        if self
            .to_ignore_damage
            .contains(&last_hurt_by_mob.entity_type)
        {
            return false;
        }
        self.base
            .can_attack(ctx, Some(last_hurt_by_mob), Self::HURT_BY_TARGETING)
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        self.base.can_continue_to_use(ctx)
    }

    fn start(&mut self, ctx: &mut C) {
        let attacker = ctx.last_hurt_by_mob().map(|e| e.id);
        ctx.set_target(attacker, TargetReason::TargetAttackedEntity);
        self.base.target_mob = ctx.target().map(|e| e.id);
        self.timestamp = ctx.last_hurt_by_mob_timestamp();
        self.base.unseen_memory_ticks = Self::RETALIATION_UNSEEN_MEMORY_TICKS;
        if self.alert_same_type
            && let Some(attacker_id) = attacker
        {
            self.alert_others(ctx, attacker_id);
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

//! Mirrors `net.minecraft.world.entity.ai.goal.target.ResetUniversalAngerTargetGoal`.

use crate::world::entity::ai::goal::context::GoalContext;
use crate::world::entity::ai::goal::goal::{Goal, GoalFlags};
use rivet_registry::world::phys::AABB;

/// Refreshes universal anger when hurt by a player while the `UNIVERSAL_ANGER`
/// gamerule is enabled.
///
/// Mirrors Paper 26.2: extends `Goal` directly without calling `setFlags`, so its
/// flag set defaults to [`GoalFlags::UNKNOWN_BEHAVIOR`].
#[derive(Debug, Clone)]
pub struct ResetUniversalAngerTargetGoal {
    flags: GoalFlags,
    alert_others_of_same_type: bool,
    last_hurt_by_player_timestamp: i32,
}

impl ResetUniversalAngerTargetGoal {
    pub const ALERT_RANGE_Y: f64 = 10.0;

    pub fn new(alert_others_of_same_type: bool) -> Self {
        Self {
            flags: GoalFlags::UNKNOWN_BEHAVIOR,
            alert_others_of_same_type,
            last_hurt_by_player_timestamp: 0,
        }
    }

    fn was_hurt_by_player<C: GoalContext>(&self, ctx: &C) -> bool {
        ctx.last_hurt_by_mob().is_some_and(|e| e.is_player)
            && ctx.last_hurt_by_mob_timestamp() > self.last_hurt_by_player_timestamp
    }
}

impl<C: GoalContext> Goal<C> for ResetUniversalAngerTargetGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        ctx.universal_anger_enabled() && self.was_hurt_by_player(ctx)
    }

    fn start(&mut self, ctx: &mut C) {
        self.last_hurt_by_player_timestamp = ctx.last_hurt_by_mob_timestamp();
        ctx.forget_current_target_and_refresh_universal_anger();
        if self.alert_others_of_same_type {
            let within = ctx.follow_range();
            let search_area = AABB::unit_cube_from_lower_corner(&ctx.pos()).inflate_xyz(
                within,
                Self::ALERT_RANGE_Y,
                within,
            );
            ctx.alert_nearby_same_type_universal_anger(search_area);
        }
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

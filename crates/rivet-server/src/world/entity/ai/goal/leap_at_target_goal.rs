//! Mirrors `net.minecraft.world.entity.ai.goal.LeapAtTargetGoal`.

use super::context::{EntityId, GoalContext, GoalRandom, Vec3};
use super::goal::{Goal, GoalFlags, reduced_tick_delay};

/// Makes a mob leap horizontally and vertically toward its target when within
/// squared distance `[4.0, 16.0]`.
#[derive(Debug, Clone)]
pub struct LeapAtTargetGoal {
    flags: GoalFlags,
    target: Option<EntityId>,
    yd: f32,
}

impl LeapAtTargetGoal {
    pub fn new(yd: f32) -> Self {
        Self {
            flags: GoalFlags::JUMP.union(GoalFlags::MOVE),
            target: None,
            yd,
        }
    }
}

impl<C: GoalContext> Goal<C> for LeapAtTargetGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        if ctx.has_controlling_passenger() {
            return false;
        }
        let Some(target) = ctx.target() else {
            return false;
        };
        self.target = Some(target.id);
        let d = ctx.pos().distance_to_sqr(target.pos);
        if !(4.0..=16.0).contains(&d) {
            return false;
        }
        ctx.on_ground() && ctx.random().next_int(reduced_tick_delay(5)) == 0
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        !ctx.on_ground()
    }

    fn start(&mut self, ctx: &mut C) {
        let movement = ctx.delta_movement();
        let Some(target_id) = self.target else {
            return;
        };
        let Some(target) = ctx.entity_snapshot(target_id) else {
            return;
        };
        let pos = ctx.pos();
        let mut delta = Vec3::new(target.pos.x - pos.x, 0.0, target.pos.z - pos.z);
        if delta.length_sqr() > 1.0e-7 {
            delta = delta.normalize().scale(0.4) + movement.scale(0.2);
        }
        ctx.set_delta_movement(delta.x, self.yd as f64, delta.z);
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

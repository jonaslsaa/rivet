//! Mirrors `net.minecraft.world.entity.ai.goal.RandomLookAroundGoal`.

use std::f64::consts::PI;

use super::context::{GoalContext, GoalRandom};
use super::goal::{Goal, GoalFlags};

/// Makes a mob periodically pick a random horizontal direction and look toward it
/// for 20..39 ticks.
#[derive(Debug, Clone)]
pub struct RandomLookAroundGoal {
    flags: GoalFlags,
    rel_x: f64,
    rel_z: f64,
    look_time: i32,
}

impl Default for RandomLookAroundGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomLookAroundGoal {
    pub fn new() -> Self {
        Self {
            flags: GoalFlags::MOVE.union(GoalFlags::LOOK),
            rel_x: 0.0,
            rel_z: 0.0,
            look_time: 0,
        }
    }

    pub fn look_time(&self) -> i32 {
        self.look_time
    }
}

impl<C: GoalContext> Goal<C> for RandomLookAroundGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        ctx.random().next_float() < 0.02
    }

    fn can_continue_to_use(&mut self, _ctx: &mut C) -> bool {
        self.look_time >= 0
    }

    fn start(&mut self, ctx: &mut C) {
        let angle = (PI * 2.0) * ctx.random().next_double();
        self.rel_x = angle.cos();
        self.rel_z = angle.sin();
        self.look_time = 20_i32.wrapping_add(ctx.random().next_int(20));
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, ctx: &mut C) {
        self.look_time = self.look_time.wrapping_sub(1);
        let pos = ctx.pos();
        ctx.set_look_at(pos.x + self.rel_x, ctx.eye_y(), pos.z + self.rel_z);
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

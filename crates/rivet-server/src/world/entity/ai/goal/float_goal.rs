//! Mirrors `net.minecraft.world.entity.ai.goal.FloatGoal`.

use super::context::{GoalContext, GoalRandom};
use super::goal::{Goal, GoalFlags};

/// Causes a mob to jump repeatedly when submerged in water (above its fluid jump
/// threshold) or in lava.
#[derive(Debug, Clone)]
pub struct FloatGoal {
    flags: GoalFlags,
}

impl Default for FloatGoal {
    fn default() -> Self {
        Self::new()
    }
}

impl FloatGoal {
    pub fn new() -> Self {
        Self {
            flags: GoalFlags::JUMP,
        }
    }

    /// Mirrors the navigation setup in Paper's `FloatGoal(Mob)` constructor.
    pub fn init_navigation<C: GoalContext>(ctx: &mut C) {
        ctx.set_can_float(true);
    }
}

impl<C: GoalContext> Goal<C> for FloatGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        (ctx.is_in_water() && ctx.water_fluid_height() > ctx.fluid_jump_threshold())
            || ctx.is_in_lava()
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, ctx: &mut C) {
        if ctx.random().next_float() < 0.8 {
            ctx.jump();
        }
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

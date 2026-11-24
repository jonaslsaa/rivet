//! Mirrors `net.minecraft.world.entity.ai.goal.PanicGoal`.

use super::context::{DAMAGE_TYPE_TAG_PANIC_CAUSES, DamageTypeTagId, GoalContext};
use super::goal::{Goal, GoalFlags};
use rivet_registry::world::phys::Vec3;

/// Causes a mob to flee when hurt by a panic-causing damage source (seeking water
/// first if currently on fire).
#[derive(Debug, Clone)]
pub struct PanicGoal {
    flags: GoalFlags,
    speed_modifier: f64,
    pos_x: f64,
    pos_y: f64,
    pos_z: f64,
    is_running: bool,
    panic_causing_damage_types: DamageTypeTagId,
}

impl PanicGoal {
    pub const WATER_CHECK_DISTANCE_VERTICAL: i32 = 1;

    pub fn new(speed_modifier: f64) -> Self {
        Self::with_damage_tag(speed_modifier, DAMAGE_TYPE_TAG_PANIC_CAUSES)
    }

    pub fn with_damage_tag(
        speed_modifier: f64,
        panic_causing_damage_types: DamageTypeTagId,
    ) -> Self {
        Self {
            flags: GoalFlags::MOVE,
            speed_modifier,
            pos_x: 0.0,
            pos_y: 0.0,
            pos_z: 0.0,
            is_running: false,
            panic_causing_damage_types,
        }
    }

    pub fn should_panic<C: GoalContext>(&self, ctx: &C) -> bool {
        ctx.has_panic_causing_damage(self.panic_causing_damage_types)
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    pub fn target_position(&self) -> Vec3 {
        Vec3::new(self.pos_x, self.pos_y, self.pos_z)
    }

    fn find_random_position<C: GoalContext>(&mut self, ctx: &mut C) -> bool {
        let Some(pos) = ctx.default_random_pos(5, 4) else {
            return false;
        };
        self.pos_x = pos.x;
        self.pos_y = pos.y;
        self.pos_z = pos.z;
        true
    }
}

impl<C: GoalContext> Goal<C> for PanicGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        if !self.should_panic(ctx) {
            return false;
        }
        if ctx.is_on_fire()
            && let Some(water_pos) = ctx.find_closest_water(5, Self::WATER_CHECK_DISTANCE_VERTICAL)
        {
            self.pos_x = water_pos.x as f64;
            self.pos_y = water_pos.y as f64;
            self.pos_z = water_pos.z as f64;
            return true;
        }
        self.find_random_position(ctx)
    }

    fn start(&mut self, ctx: &mut C) {
        ctx.nav_move_to_xyz(self.pos_x, self.pos_y, self.pos_z, self.speed_modifier);
        self.is_running = true;
    }

    fn stop(&mut self, _ctx: &mut C) {
        self.is_running = false;
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        !ctx.nav_is_done()
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

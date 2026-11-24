//! Mirrors `net.minecraft.world.entity.ai.goal.RandomStrollGoal` and
//! `net.minecraft.world.entity.ai.goal.WaterAvoidingRandomStrollGoal`.

use super::context::{GoalContext, GoalRandom};
use super::goal::{Goal, GoalFlags, reduced_tick_delay};
use rivet_registry::world::phys::Vec3;

/// Mirrors `net.minecraft.world.entity.ai.goal.RandomStrollGoal`.
#[derive(Debug, Clone)]
pub struct RandomStrollGoal {
    flags: GoalFlags,
    wanted_x: f64,
    wanted_y: f64,
    wanted_z: f64,
    speed_modifier: f64,
    interval: i32,
    force_trigger: bool,
    check_no_action_time: bool,
}

impl RandomStrollGoal {
    pub const DEFAULT_INTERVAL: i32 = 120;

    pub fn new(speed_modifier: f64) -> Self {
        Self::with_interval(speed_modifier, Self::DEFAULT_INTERVAL)
    }

    pub fn with_interval(speed_modifier: f64, interval: i32) -> Self {
        Self::with_options(speed_modifier, interval, true)
    }

    pub fn with_options(speed_modifier: f64, interval: i32, check_no_action_time: bool) -> Self {
        Self {
            flags: GoalFlags::MOVE,
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier,
            interval,
            force_trigger: false,
            check_no_action_time,
        }
    }

    pub fn trigger(&mut self) {
        self.force_trigger = true;
    }

    pub fn set_interval(&mut self, new_chance: i32) {
        self.interval = new_chance;
    }

    pub fn wanted_position(&self) -> Vec3 {
        Vec3::new(self.wanted_x, self.wanted_y, self.wanted_z)
    }

    fn can_use_with_pos<C: GoalContext>(
        &mut self,
        ctx: &mut C,
        get_pos: impl FnOnce(&mut C) -> Option<Vec3>,
    ) -> bool {
        if ctx.has_controlling_passenger() {
            return false;
        }
        if !self.force_trigger {
            if self.check_no_action_time && ctx.no_action_time() >= 100 {
                return false;
            }
            if ctx.random().next_int(reduced_tick_delay(self.interval)) != 0 {
                return false;
            }
        }

        let Some(pos) = get_pos(ctx) else {
            return false;
        };
        self.wanted_x = pos.x;
        self.wanted_y = pos.y;
        self.wanted_z = pos.z;
        self.force_trigger = false;
        true
    }
}

impl<C: GoalContext> Goal<C> for RandomStrollGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        self.can_use_with_pos(ctx, |c| c.default_random_pos(10, 7))
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        !ctx.nav_is_done() && !ctx.has_controlling_passenger()
    }

    fn start(&mut self, ctx: &mut C) {
        ctx.nav_move_to_xyz(
            self.wanted_x,
            self.wanted_y,
            self.wanted_z,
            self.speed_modifier,
        );
    }

    fn stop(&mut self, ctx: &mut C) {
        ctx.nav_stop();
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

/// Mirrors `net.minecraft.world.entity.ai.goal.WaterAvoidingRandomStrollGoal`.
#[derive(Debug, Clone)]
pub struct WaterAvoidingRandomStrollGoal {
    base: RandomStrollGoal,
    probability: f32,
}

impl WaterAvoidingRandomStrollGoal {
    pub const PROBABILITY: f32 = 0.001;

    pub fn new(speed_modifier: f64) -> Self {
        Self::with_probability(speed_modifier, Self::PROBABILITY)
    }

    pub fn with_probability(speed_modifier: f64, probability: f32) -> Self {
        Self {
            base: RandomStrollGoal::new(speed_modifier),
            probability,
        }
    }

    pub fn trigger(&mut self) {
        self.base.trigger();
    }

    pub fn set_interval(&mut self, new_chance: i32) {
        self.base.set_interval(new_chance);
    }

    pub fn wanted_position(&self) -> Vec3 {
        self.base.wanted_position()
    }
}

impl<C: GoalContext> Goal<C> for WaterAvoidingRandomStrollGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        let probability = self.probability;
        self.base.can_use_with_pos(ctx, |c| {
            if c.is_in_water() {
                c.land_random_pos(15, 7)
                    .or_else(|| c.default_random_pos(10, 7))
            } else if c.random().next_float() >= probability {
                c.land_random_pos(10, 7)
            } else {
                c.default_random_pos(10, 7)
            }
        })
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        self.base.can_continue_to_use(ctx)
    }

    fn start(&mut self, ctx: &mut C) {
        self.base.start(ctx);
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

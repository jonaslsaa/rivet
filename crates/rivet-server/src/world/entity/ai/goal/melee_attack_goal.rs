//! Mirrors `net.minecraft.world.entity.ai.goal.MeleeAttackGoal`.

use super::context::{
    EntityId, GoalContext, GoalRandom, InteractionHand, PathHandle, TargetReason, Vec3,
};
use super::goal::{Goal, GoalFlags};

/// Drives a mob to pathfind toward and melee-attack its current target.
#[derive(Debug, Clone)]
pub struct MeleeAttackGoal {
    flags: GoalFlags,
    speed_modifier: f64,
    following_target_even_if_not_seen: bool,
    path: Option<PathHandle>,
    pathed_target_x: f64,
    pathed_target_y: f64,
    pathed_target_z: f64,
    ticks_until_next_path_recalculation: i32,
    ticks_until_next_attack: i32,
    attack_interval: i32,
    last_can_use_check: i64,
}

impl MeleeAttackGoal {
    pub const COOLDOWN_BETWEEN_CAN_USE_CHECKS: i64 = 20;

    pub fn new(speed_modifier: f64, following_target_even_if_not_seen: bool) -> Self {
        Self {
            flags: GoalFlags::MOVE.union(GoalFlags::LOOK),
            speed_modifier,
            following_target_even_if_not_seen,
            path: None,
            pathed_target_x: 0.0,
            pathed_target_y: 0.0,
            pathed_target_z: 0.0,
            ticks_until_next_path_recalculation: 0,
            ticks_until_next_attack: 0,
            attack_interval: 20,
            last_can_use_check: 0,
        }
    }

    pub fn ticks_until_next_attack(&self) -> i32 {
        self.ticks_until_next_attack
    }

    pub fn is_time_to_attack(&self) -> bool {
        self.ticks_until_next_attack <= 0
    }

    pub fn attack_interval<C: GoalContext>(&self) -> i32 {
        <Self as Goal<C>>::adjusted_tick_delay(self, self.attack_interval)
    }

    fn reset_attack_cooldown<C: GoalContext>(&mut self) {
        self.ticks_until_next_attack =
            <Self as Goal<C>>::adjusted_tick_delay(self, self.attack_interval);
    }

    fn can_perform_attack<C: GoalContext>(&self, ctx: &mut C, target: EntityId) -> bool {
        self.is_time_to_attack()
            && ctx.is_within_melee_attack_range(target)
            && ctx.has_line_of_sight(target)
    }
}

impl<C: GoalContext> Goal<C> for MeleeAttackGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        let time = ctx.game_time();
        if time.wrapping_sub(self.last_can_use_check) < Self::COOLDOWN_BETWEEN_CAN_USE_CHECKS {
            return false;
        }
        self.last_can_use_check = time;
        let Some(target) = ctx.target() else {
            return false;
        };
        if !target.is_alive {
            return false;
        }
        self.path = ctx.nav_create_path_to_entity(target.id, 0);
        self.path.is_some() || ctx.is_within_melee_attack_range(target.id)
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        let Some(target) = ctx.target() else {
            return false;
        };
        if !target.is_alive {
            return false;
        }
        if !self.following_target_even_if_not_seen {
            !ctx.nav_is_done()
        } else if ctx.is_within_home(target.block_pos) {
            !target.is_player || !target.is_creative_or_spectator
        } else {
            false
        }
    }

    fn start(&mut self, ctx: &mut C) {
        ctx.nav_move_to_path(self.path, self.speed_modifier);
        ctx.set_aggressive(true);
        self.ticks_until_next_path_recalculation = 0;
        self.ticks_until_next_attack = 0;
    }

    fn stop(&mut self, ctx: &mut C) {
        if let Some(target) = ctx.target()
            && target.is_creative_or_spectator
        {
            ctx.set_target(None, TargetReason::ForgotTarget);
        }
        ctx.set_aggressive(false);
        ctx.nav_stop();
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, ctx: &mut C) {
        let Some(target) = ctx.target() else {
            return;
        };
        ctx.set_look_at_entity(target.id, 30.0, 30.0);
        self.ticks_until_next_path_recalculation = self
            .ticks_until_next_path_recalculation
            .wrapping_sub(1)
            .max(0);

        let can_see = self.following_target_even_if_not_seen || ctx.has_line_of_sight(target.id);
        let pathed_pos = Vec3::new(
            self.pathed_target_x,
            self.pathed_target_y,
            self.pathed_target_z,
        );

        if can_see
            && self.ticks_until_next_path_recalculation <= 0
            && ((self.pathed_target_x == 0.0
                && self.pathed_target_y == 0.0
                && self.pathed_target_z == 0.0)
                || target.pos.distance_to_sqr(pathed_pos) >= 1.0
                || ctx.random().next_float() < 0.05)
        {
            self.pathed_target_x = target.pos.x;
            self.pathed_target_y = target.pos.y;
            self.pathed_target_z = target.pos.z;
            self.ticks_until_next_path_recalculation = 4_i32.wrapping_add(ctx.random().next_int(7));
            let dist_sqr = ctx.pos().distance_to_sqr(target.pos);
            if dist_sqr > 1024.0 {
                self.ticks_until_next_path_recalculation =
                    self.ticks_until_next_path_recalculation.wrapping_add(10);
            } else if dist_sqr > 256.0 {
                self.ticks_until_next_path_recalculation =
                    self.ticks_until_next_path_recalculation.wrapping_add(5);
            }
            if !ctx.nav_move_to_entity(target.id, self.speed_modifier) {
                self.ticks_until_next_path_recalculation =
                    self.ticks_until_next_path_recalculation.wrapping_add(15);
            }
            self.ticks_until_next_path_recalculation = <Self as Goal<C>>::adjusted_tick_delay(
                self,
                self.ticks_until_next_path_recalculation,
            );
        }

        self.ticks_until_next_attack = self.ticks_until_next_attack.wrapping_sub(1).max(0);
        if self.can_perform_attack(ctx, target.id) {
            self.reset_attack_cooldown::<C>();
            ctx.swing(InteractionHand::MainHand);
            ctx.do_hurt_target(target.id);
        }
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

//! Mirrors `net.minecraft.world.entity.ai.goal.LookAtPlayerGoal`.

use super::context::{EntityTypeId, GoalContext, GoalRandom, TargetingConditions};
use super::goal::{Goal, GoalFlags};
use crate::world::entity::entity_id::EntityId;

/// Target entity category watched by [`LookAtPlayerGoal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookAtTargetType {
    Player,
    EntityType(EntityTypeId),
}

/// Makes a mob turn its head toward a nearby player or entity of a specific type.
#[derive(Debug, Clone)]
pub struct LookAtPlayerGoal {
    flags: GoalFlags,
    look_at: Option<EntityId>,
    look_distance: f32,
    look_time: i32,
    probability: f32,
    only_horizontal: bool,
    look_at_type: LookAtTargetType,
    look_at_context: TargetingConditions,
}

impl LookAtPlayerGoal {
    pub const DEFAULT_PROBABILITY: f32 = 0.02;

    pub fn new(look_at_type: LookAtTargetType, look_distance: f32) -> Self {
        Self::with_options(
            look_at_type,
            look_distance,
            Self::DEFAULT_PROBABILITY,
            false,
        )
    }

    pub fn with_probability(
        look_at_type: LookAtTargetType,
        look_distance: f32,
        probability: f32,
    ) -> Self {
        Self::with_options(look_at_type, look_distance, probability, false)
    }

    pub fn with_options(
        look_at_type: LookAtTargetType,
        look_distance: f32,
        probability: f32,
        only_horizontal: bool,
    ) -> Self {
        let mut look_at_context = TargetingConditions::for_non_combat().range(look_distance as f64);
        if matches!(look_at_type, LookAtTargetType::Player) {
            look_at_context = look_at_context.exclude_riding_mob();
        }
        Self {
            flags: GoalFlags::LOOK,
            look_at: None,
            look_distance,
            look_time: 0,
            probability,
            only_horizontal,
            look_at_type,
            look_at_context,
        }
    }

    pub fn look_at_context(&self) -> TargetingConditions {
        self.look_at_context
    }

    pub fn look_at(&self) -> Option<EntityId> {
        self.look_at
    }

    pub fn look_time(&self) -> i32 {
        self.look_time
    }
}

impl<C: GoalContext> Goal<C> for LookAtPlayerGoal {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        if ctx.random().next_float() >= self.probability {
            return false;
        }

        if let Some(target) = ctx.target() {
            self.look_at = Some(target.id);
        }

        let pos = ctx.pos();
        let eye_y = ctx.eye_y();
        match self.look_at_type {
            LookAtTargetType::Player => {
                self.look_at = ctx
                    .nearest_player(self.look_at_context, pos.x, eye_y, pos.z)
                    .map(|e| e.id);
            }
            LookAtTargetType::EntityType(entity_type) => {
                let area = ctx.bounding_box().inflate_xyz(
                    self.look_distance as f64,
                    3.0,
                    self.look_distance as f64,
                );
                self.look_at = ctx
                    .nearest_entity_of_type(
                        entity_type,
                        area,
                        self.look_at_context,
                        pos.x,
                        eye_y,
                        pos.z,
                    )
                    .map(|e| e.id);
            }
        }

        self.look_at.is_some()
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        let Some(id) = self.look_at else {
            return false;
        };
        let Some(target) = ctx.entity_snapshot(id) else {
            return false;
        };
        if !target.is_alive {
            return false;
        }
        let max_dist_sqr = (self.look_distance * self.look_distance) as f64;
        if ctx.pos().distance_to_sqr(&target.pos) > max_dist_sqr {
            return false;
        }
        self.look_time > 0
    }

    fn start(&mut self, ctx: &mut C) {
        let raw = 40_i32.wrapping_add(ctx.random().next_int(40));
        self.look_time = <Self as Goal<C>>::adjusted_tick_delay(self, raw);
    }

    fn stop(&mut self, _ctx: &mut C) {
        self.look_at = None;
    }

    fn tick(&mut self, ctx: &mut C) {
        let Some(id) = self.look_at else {
            return;
        };
        let Some(target) = ctx.entity_snapshot(id) else {
            return;
        };
        if target.is_alive {
            let target_y = if self.only_horizontal {
                ctx.eye_y()
            } else {
                target.eye_y
            };
            ctx.set_look_at(target.pos.x, target_y, target.pos.z);
            self.look_time = self.look_time.wrapping_sub(1);
        }
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}

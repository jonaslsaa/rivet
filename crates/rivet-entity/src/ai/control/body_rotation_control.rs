//! Port of `net.minecraft.world.entity.ai.control.BodyRotationControl`
//! (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use crate::lifecycle::LivingEntity;
use rivet_util::mth;

/// `BodyRotationControl.HEAD_STABLE_ANGLE`.
const HEAD_STABLE_ANGLE: f32 = 15.0;
/// `BodyRotationControl.DELAY_UNTIL_STARTING_TO_FACE_FORWARD`.
const DELAY_UNTIL_STARTING_TO_FACE_FORWARD: i32 = 10;
/// `BodyRotationControl.HOW_LONG_IT_TAKES_TO_FACE_FORWARD`.
const HOW_LONG_IT_TAKES_TO_FACE_FORWARD: f32 = 10.0;

/// Port of `BodyRotationControl`; `Mob#tickHeadTurn` delegates to [`Self::client_tick`].
///
/// The owning mob is passed per call instead of stored (`OWNERSHIP.md`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BodyRotationControl {
    head_stable_time: i32,
    last_stable_y_head_rot: f32,
}

impl BodyRotationControl {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head_stable_time: 0,
            last_stable_y_head_rot: 0.0,
        }
    }

    /// `BodyRotationControl#clientTick()`.
    ///
    /// `max_head_y_rot` is `Mob#getMaxHeadYRot()`; `carrying_mob_passenger` is
    /// `mob.getFirstPassenger() instanceof Mob`.
    pub fn client_tick(
        &mut self,
        mob: &mut LivingEntity,
        max_head_y_rot: i32,
        carrying_mob_passenger: bool,
    ) {
        if Self::is_moving(mob) {
            mob.y_body_rot = mob.entity.y_rot;
            Self::rotate_head_if_necessary(mob, max_head_y_rot);
            self.last_stable_y_head_rot = mob.y_head_rot;
            self.head_stable_time = 0;
        } else if !carrying_mob_passenger {
            if (mob.y_head_rot - self.last_stable_y_head_rot).abs() > HEAD_STABLE_ANGLE {
                self.head_stable_time = 0;
                self.last_stable_y_head_rot = mob.y_head_rot;
                Self::rotate_body_if_necessary(mob, max_head_y_rot);
            } else {
                self.head_stable_time = self.head_stable_time.wrapping_add(1);
                if self.head_stable_time > DELAY_UNTIL_STARTING_TO_FACE_FORWARD {
                    self.rotate_head_towards_front(mob, max_head_y_rot);
                }
            }
        }
    }

    fn rotate_body_if_necessary(mob: &mut LivingEntity, max_head_y_rot: i32) {
        mob.y_body_rot =
            mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, max_head_y_rot as f32);
    }

    fn rotate_head_if_necessary(mob: &mut LivingEntity, max_head_y_rot: i32) {
        mob.y_head_rot =
            mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, max_head_y_rot as f32);
    }

    fn rotate_head_towards_front(&self, mob: &mut LivingEntity, max_head_y_rot: i32) {
        let time_since_starting_to_face_forward = self
            .head_stable_time
            .wrapping_sub(DELAY_UNTIL_STARTING_TO_FACE_FORWARD);
        let face_forward_fraction = mth::clamp_f32(
            time_since_starting_to_face_forward as f32 / HOW_LONG_IT_TAKES_TO_FACE_FORWARD,
            0.0,
            1.0,
        );
        let angle_remaining_until_facing_forward =
            max_head_y_rot as f32 * (1.0_f32 - face_forward_fraction);
        mob.y_body_rot = mth::rotate_if_necessary(
            mob.y_body_rot,
            mob.y_head_rot,
            angle_remaining_until_facing_forward,
        );
    }

    fn is_moving(mob: &LivingEntity) -> bool {
        let xd = mob.entity.pos.x - mob.entity.xo;
        let zd = mob.entity.pos.z - mob.entity.zo;
        xd * xd + zd * zd > f64::from(2.500_000_3e-7_f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::EntityTypeMeta;

    fn mob() -> LivingEntity {
        LivingEntity::new(1, EntityTypeMeta::new("minecraft:pig", 0.9, 0.9), 10.0, 1)
    }

    #[test]
    fn moving_snaps_body_to_y_rot_and_clamps_head() {
        let mut control = BodyRotationControl::new();
        let mut living = mob();
        living.entity.xo = 0.0;
        living.entity.pos.x = 1.0;
        living.entity.y_rot = 90.0;
        living.y_head_rot = 0.0;
        control.client_tick(&mut living, 75, false);
        assert_eq!(living.y_body_rot, 90.0);
        // rotateIfNecessary(0, 90, 75) = 90 - clamp(90, -75, 75) = 15
        assert_eq!(living.y_head_rot, 15.0);
        assert_eq!(control.last_stable_y_head_rot, 15.0);
        assert_eq!(control.head_stable_time, 0);
    }

    #[test]
    fn stationary_head_turn_rotates_body_then_faces_forward() {
        let mut control = BodyRotationControl::new();
        let mut living = mob();
        living.y_head_rot = 100.0;
        control.client_tick(&mut living, 75, false);
        // |100 - 0| > 15: body follows head to within 75 degrees.
        assert_eq!(living.y_body_rot, 25.0);
        assert_eq!(control.head_stable_time, 0);

        for _ in 0..11 {
            control.client_tick(&mut living, 75, false);
        }
        // headStableTime == 11 -> fraction 0.1 -> remaining 67.5: body stays within 67.5.
        assert_eq!(control.head_stable_time, 11);
        assert_eq!(living.y_body_rot, 32.5);
    }

    #[test]
    fn carrying_mob_passenger_freezes_stationary_body() {
        let mut control = BodyRotationControl::new();
        let mut living = mob();
        living.y_head_rot = 100.0;
        control.client_tick(&mut living, 75, true);
        assert_eq!(living.y_body_rot, 0.0);
        assert_eq!(control.head_stable_time, 0);
    }
}

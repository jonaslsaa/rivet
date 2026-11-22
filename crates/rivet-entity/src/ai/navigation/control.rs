//! Port of Paper 26.2 `net.minecraft.world.entity.ai.control.*`:
//! - `Control` (`rotate_towards`)
//! - `MoveControl`, `FlyingMoveControl`, `SmoothSwimmingMoveControl`
//! - `LookControl`, `SmoothSwimmingLookControl`
//! - `JumpControl`
//! - `BodyRotationControl`

use rivet_util::mth;

use crate::ai::goal::context::{BlockPos, Vec3};
use crate::ai::navigation::path_type::PathType;
use crate::ai::navigation::world_view::{MobPathingState, PathWorldView};

/// Exact port of `Control.rotateTowards(float fromAngle, float toAngle, float maxRot)`.
#[must_use]
pub fn rotate_towards(from_angle: f32, to_angle: f32, max_rot: f32) -> f32 {
    let diff = mth::degrees_difference(from_angle, to_angle);
    let diff_clamped = mth::clamp_f32(diff, -max_rot, max_rot);
    from_angle + diff_clamped
}

/// Exact port of `MoveControl.rotlerp(float sourceAngle, float targetAngle, float maximumChange)`.
#[must_use]
pub fn rotlerp(source_angle: f32, target_angle: f32, maximum_change: f32) -> f32 {
    let mut f = mth::wrap_degrees_f32(target_angle - source_angle);
    if f > maximum_change {
        f = maximum_change;
    }
    if f < -maximum_change {
        f = -maximum_change;
    }
    let mut f1 = source_angle + f;
    if f1 < 0.0 {
        f1 += 360.0;
    } else if f1 > 360.0 {
        f1 -= 360.0;
    }
    f1
}

/// Port of `net.minecraft.world.entity.ai.control.JumpControl` in Paper 26.2.
#[derive(Debug, Default, Clone, Copy)]
pub struct JumpControl {
    jump: bool,
}

impl JumpControl {
    #[must_use]
    pub const fn new() -> Self {
        Self { jump: false }
    }

    pub fn jump(&mut self) {
        self.jump = true;
    }

    pub fn tick(&mut self, mob: &mut MobPathingState) {
        mob.jumping = self.jump;
        self.jump = false;
    }

    #[must_use]
    pub const fn is_jump_queued(&self) -> bool {
        self.jump
    }
}

/// Port of `MoveControl.Operation` in Paper 26.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MoveOperation {
    #[default]
    Wait,
    MoveTo,
    Strafe,
    Jumping,
}

/// Port of `net.minecraft.world.entity.ai.control.MoveControl` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct MoveControl {
    pub wanted_x: f64,
    pub wanted_y: f64,
    pub wanted_z: f64,
    pub speed_modifier: f64,
    pub strafe_forwards: f32,
    pub strafe_right: f32,
    pub operation: MoveOperation,
}

impl Default for MoveControl {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveControl {
    pub const MIN_SPEED: f32 = 5.0e-4;
    pub const MIN_SPEED_SQR: f32 = 2.500_000_3e-7;
    pub const MAX_TURN: f32 = 90.0;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
            speed_modifier: 0.0,
            strafe_forwards: 0.0,
            strafe_right: 0.0,
            operation: MoveOperation::Wait,
        }
    }

    #[must_use]
    pub const fn has_wanted(&self) -> bool {
        matches!(self.operation, MoveOperation::MoveTo)
    }

    #[must_use]
    pub const fn speed_modifier(&self) -> f64 {
        self.speed_modifier
    }

    /// Mirrors `MoveControl.setWantedPosition(double x, double y, double z, double speedModifier)`.
    pub fn set_wanted_position(&mut self, x: f64, y: f64, z: f64, speed_modifier: f64) {
        self.wanted_x = x;
        self.wanted_y = y;
        self.wanted_z = z;
        self.speed_modifier = speed_modifier;
        if self.operation != MoveOperation::Jumping {
            self.operation = MoveOperation::MoveTo;
        }
    }

    /// Mirrors `MoveControl.strafe(float forwards, float right)`.
    pub fn strafe(&mut self, forwards: f32, right: f32) {
        self.operation = MoveOperation::Strafe;
        self.strafe_forwards = forwards;
        self.strafe_right = right;
        self.speed_modifier = 0.25;
    }

    pub fn set_wait(&mut self) {
        self.operation = MoveOperation::Wait;
    }

    /// Mirrors `MoveControl.tick()` in Paper 26.2.
    ///
    /// `eval_path_type` is called only in `MoveOperation::Strafe` to check
    /// `isWalkable` (matching `nodeEvaluator.getPathType(mob, x, y, z) == PathType.WALKABLE`).
    pub fn tick<W, F>(
        &mut self,
        mob: &mut MobPathingState,
        world: &W,
        jump_control: &mut JumpControl,
        mut eval_path_type: F,
    ) where
        W: PathWorldView + ?Sized,
        F: FnMut(i32, i32, i32) -> PathType,
    {
        match self.operation {
            MoveOperation::Strafe => {
                let speed = (self.speed_modifier * mob.movement_speed) as f32;
                let mut forwards = self.strafe_forwards;
                let mut right = self.strafe_right;
                let mut mag = mth::sqrt(forwards * forwards + right * right);
                if mag < 1.0 {
                    mag = 1.0;
                }
                let scale = speed / mag;
                forwards *= scale;
                right *= scale;

                let sin = mth::sin(f64::from(mob.y_rot * mth::DEG_TO_RAD));
                let cos = mth::cos(f64::from(mob.y_rot * mth::DEG_TO_RAD));
                let dx = forwards * cos - right * sin;
                let dz = right * cos + forwards * sin;

                let probe_x = mth::floor_d(mob.pos.x + f64::from(dx));
                let probe_y = mth::floor_d(mob.pos.y);
                let probe_z = mth::floor_d(mob.pos.z + f64::from(dz));
                if eval_path_type(probe_x, probe_y, probe_z) != PathType::Walkable {
                    self.strafe_forwards = 1.0;
                    self.strafe_right = 0.0;
                }

                mob.set_speed(speed);
                mob.zza = self.strafe_forwards;
                mob.xxa = self.strafe_right;
                self.operation = MoveOperation::Wait;
            }
            MoveOperation::MoveTo => {
                self.operation = MoveOperation::Wait;
                let xd = self.wanted_x - mob.pos.x;
                let zd = self.wanted_z - mob.pos.z;
                let yd = self.wanted_y - mob.pos.y;
                let dd = xd * xd + yd * yd + zd * zd;
                if dd < f64::from(Self::MIN_SPEED_SQR) {
                    mob.zza = 0.0;
                    return;
                }

                let y_rot_d = (mth::atan2(zd, xd) * (180.0 / std::f64::consts::PI)) as f32 - 90.0;
                mob.y_rot = rotlerp(mob.y_rot, y_rot_d, Self::MAX_TURN);
                mob.set_speed((self.speed_modifier * mob.movement_speed) as f32);

                let pos = mob.block_position();
                let info = world.block_info(pos);
                let shape_max_y = info.collision_max_y.unwrap_or(0.0);
                let horizontal_threshold = mth::max_f64(f64::from(mob.bb_width), 1.0);
                if (yd > f64::from(mob.max_up_step)
                    && xd * xd + zd * zd < horizontal_threshold * horizontal_threshold)
                    || (info.has_collision()
                        && mob.pos.y < pos.y as f64 + shape_max_y
                        && !info.is_door_open
                        && !info.is_wood_door_closed
                        && !info.is_iron_door_closed
                        && !info.is_fence_or_wall)
                {
                    jump_control.jump();
                    self.operation = MoveOperation::Jumping;
                }
            }
            MoveOperation::Jumping => {
                mob.set_speed((self.speed_modifier * mob.movement_speed) as f32);
                if mob.on_ground || mob.is_in_liquid() {
                    self.operation = MoveOperation::Wait;
                }
            }
            MoveOperation::Wait => {
                mob.zza = 0.0;
            }
        }
    }
}

/// Port of `net.minecraft.world.entity.ai.control.FlyingMoveControl` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct FlyingMoveControl {
    pub base: MoveControl,
    pub max_turn: i32,
    pub hovers_in_place: bool,
}

impl FlyingMoveControl {
    #[must_use]
    pub const fn new(max_turn: i32, hovers_in_place: bool) -> Self {
        Self {
            base: MoveControl::new(),
            max_turn,
            hovers_in_place,
        }
    }

    pub fn set_wanted_position(&mut self, x: f64, y: f64, z: f64, speed_modifier: f64) {
        self.base.set_wanted_position(x, y, z, speed_modifier);
    }

    /// Mirrors `FlyingMoveControl.tick()` in Paper 26.2.
    pub fn tick(&mut self, mob: &mut MobPathingState) {
        if self.base.operation == MoveOperation::MoveTo {
            self.base.operation = MoveOperation::Wait;
            mob.no_gravity = true;
            let xd = self.base.wanted_x - mob.pos.x;
            let yd = self.base.wanted_y - mob.pos.y;
            let zd = self.base.wanted_z - mob.pos.z;
            let dd = xd * xd + yd * yd + zd * zd;
            if dd < f64::from(MoveControl::MIN_SPEED_SQR) {
                mob.yya = 0.0;
                mob.zza = 0.0;
                return;
            }

            let y_rot_d = (mth::atan2(zd, xd) * (180.0 / std::f64::consts::PI)) as f32 - 90.0;
            mob.y_rot = rotlerp(mob.y_rot, y_rot_d, 90.0);

            let speed = if mob.on_ground {
                (self.base.speed_modifier * mob.movement_speed) as f32
            } else {
                (self.base.speed_modifier * mob.flying_speed) as f32
            };
            mob.set_speed(speed);

            let sd = (xd * xd + zd * zd).sqrt();
            if yd.abs() > 1.0e-5 || sd.abs() > 1.0e-5 {
                let x_rot_d = -((mth::atan2(yd, sd) * (180.0 / std::f64::consts::PI)) as f32);
                mob.x_rot = rotlerp(mob.x_rot, x_rot_d, self.max_turn as f32);
                mob.yya = if yd > 0.0 { speed } else { -speed };
            }
        } else {
            if !self.hovers_in_place {
                mob.no_gravity = false;
            }
            mob.yya = 0.0;
            mob.zza = 0.0;
        }
    }
}

/// Port of `net.minecraft.world.entity.ai.control.SmoothSwimmingMoveControl` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct SmoothSwimmingMoveControl {
    pub base: MoveControl,
    pub max_turn_x: i32,
    pub max_turn_y: i32,
    pub in_water_speed_modifier: f32,
    pub outside_water_speed_modifier: f32,
    pub apply_gravity: bool,
}

impl SmoothSwimmingMoveControl {
    const FULL_SPEED_TURN_THRESHOLD: f32 = 10.0;
    const STOP_TURN_THRESHOLD: f32 = 60.0;

    #[must_use]
    pub const fn new(
        max_turn_x: i32,
        max_turn_y: i32,
        in_water_speed_modifier: f32,
        outside_water_speed_modifier: f32,
        apply_gravity: bool,
    ) -> Self {
        Self {
            base: MoveControl::new(),
            max_turn_x,
            max_turn_y,
            in_water_speed_modifier,
            outside_water_speed_modifier,
            apply_gravity,
        }
    }

    pub fn set_wanted_position(&mut self, x: f64, y: f64, z: f64, speed_modifier: f64) {
        self.base.set_wanted_position(x, y, z, speed_modifier);
    }

    /// Mirrors `SmoothSwimmingMoveControl.getTurningSpeedFactor(float leftToTurn)`.
    #[must_use]
    pub fn turning_speed_factor(left_to_turn: f32) -> f32 {
        1.0 - mth::clamp_f32(
            (left_to_turn - Self::FULL_SPEED_TURN_THRESHOLD)
                / (Self::STOP_TURN_THRESHOLD - Self::FULL_SPEED_TURN_THRESHOLD),
            0.0,
            1.0,
        )
    }

    /// Mirrors `SmoothSwimmingMoveControl.tick()` in Paper 26.2.
    pub fn tick(&mut self, mob: &mut MobPathingState, navigation_is_done: bool) {
        if self.apply_gravity && mob.in_water {
            mob.delta_movement = Vec3::new(
                mob.delta_movement.x,
                mob.delta_movement.y + 0.005,
                mob.delta_movement.z,
            );
        }

        if self.base.operation == MoveOperation::MoveTo && !navigation_is_done {
            let xd = self.base.wanted_x - mob.pos.x;
            let yd = self.base.wanted_y - mob.pos.y;
            let zd = self.base.wanted_z - mob.pos.z;
            let dd = xd * xd + yd * yd + zd * zd;
            if dd < f64::from(MoveControl::MIN_SPEED_SQR) {
                mob.zza = 0.0;
                return;
            }

            let y_rot_d = (mth::atan2(zd, xd) * (180.0 / std::f64::consts::PI)) as f32 - 90.0;
            mob.y_rot = rotlerp(mob.y_rot, y_rot_d, self.max_turn_y as f32);
            mob.y_body_rot = mob.y_rot;
            mob.y_head_rot = mob.y_rot;

            let speed = (self.base.speed_modifier * mob.movement_speed) as f32;
            if mob.in_water {
                mob.set_speed(speed * self.in_water_speed_modifier);
                let sd = (xd * xd + zd * zd).sqrt();
                if yd.abs() > 1.0e-5 || sd.abs() > 1.0e-5 {
                    let mut x_rot_d =
                        -((mth::atan2(yd, sd) * (180.0 / std::f64::consts::PI)) as f32);
                    x_rot_d = mth::clamp_f32(
                        mth::wrap_degrees_f32(x_rot_d),
                        -(self.max_turn_x as f32),
                        self.max_turn_x as f32,
                    );
                    mob.x_rot = rotate_towards(mob.x_rot, x_rot_d, 5.0);
                }
                let cos = mth::cos(f64::from(mob.x_rot * mth::DEG_TO_RAD));
                let sin = mth::sin(f64::from(mob.x_rot * mth::DEG_TO_RAD));
                mob.zza = cos * speed;
                mob.yya = -sin * speed;
            } else {
                let left_to_turn = mth::wrap_degrees_f32(mob.y_rot - y_rot_d).abs();
                let turning_speed_factor = Self::turning_speed_factor(left_to_turn);
                mob.set_speed(speed * self.outside_water_speed_modifier * turning_speed_factor);
            }
        } else {
            mob.set_speed(0.0);
            mob.xxa = 0.0;
            mob.yya = 0.0;
            mob.zza = 0.0;
        }
    }
}

/// Port of `net.minecraft.world.entity.ai.control.LookControl` in Paper 26.2.
#[derive(Debug, Clone, Default)]
pub struct LookControl {
    pub y_max_rot_speed: f32,
    pub x_max_rot_angle: f32,
    pub look_at_cooldown: i32,
    pub wanted_x: f64,
    pub wanted_y: f64,
    pub wanted_z: f64,
}

impl LookControl {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            y_max_rot_speed: 0.0,
            x_max_rot_angle: 0.0,
            look_at_cooldown: 0,
            wanted_x: 0.0,
            wanted_y: 0.0,
            wanted_z: 0.0,
        }
    }

    pub fn set_look_at_vec(&mut self, pos: Vec3, mob: &MobPathingState) {
        self.set_look_at(pos.x, pos.y, pos.z, mob.head_rot_speed, mob.max_head_x_rot);
    }

    /// Mirrors `LookControl.setLookAt(double x, double y, double z, float yMaxRotSpeed, float xMaxRotAngle)`.
    pub fn set_look_at(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        y_max_rot_speed: f32,
        x_max_rot_angle: f32,
    ) {
        self.wanted_x = x;
        self.wanted_y = y;
        self.wanted_z = z;
        self.y_max_rot_speed = y_max_rot_speed;
        self.x_max_rot_angle = x_max_rot_angle;
        self.look_at_cooldown = 2;
    }

    #[must_use]
    pub const fn is_looking_at_target(&self) -> bool {
        self.look_at_cooldown > 0
    }

    /// Mirrors `LookControl.getXRotD()`.
    #[must_use]
    pub fn get_x_rot_d(&self, mob: &MobPathingState) -> Option<f32> {
        let xd = self.wanted_x - mob.pos.x;
        let yd = self.wanted_y - mob.eye_y();
        let zd = self.wanted_z - mob.pos.z;
        let sd = (xd * xd + zd * zd).sqrt();
        if yd.abs() <= 1.0e-5 && sd.abs() <= 1.0e-5 {
            None
        } else {
            Some(-((mth::atan2(yd, sd) * (180.0 / std::f64::consts::PI)) as f32))
        }
    }

    /// Mirrors `LookControl.getYRotD()`.
    #[must_use]
    pub fn get_y_rot_d(&self, mob: &MobPathingState) -> Option<f32> {
        let xd = self.wanted_x - mob.pos.x;
        let zd = self.wanted_z - mob.pos.z;
        if zd.abs() <= 1.0e-5 && xd.abs() <= 1.0e-5 {
            None
        } else {
            Some((mth::atan2(zd, xd) * (180.0 / std::f64::consts::PI)) as f32 - 90.0)
        }
    }

    /// Mirrors `LookControl.tick()` in Paper 26.2.
    pub fn tick(
        &mut self,
        mob: &mut MobPathingState,
        navigation_is_done: bool,
        reset_x_rot_on_tick: bool,
    ) {
        if reset_x_rot_on_tick {
            mob.x_rot = 0.0;
        }

        if self.look_at_cooldown > 0 {
            self.look_at_cooldown -= 1;
            if let Some(y_rot_d) = self.get_y_rot_d(mob) {
                mob.y_head_rot = rotate_towards(mob.y_head_rot, y_rot_d, self.y_max_rot_speed);
            }
            if let Some(x_rot_d) = self.get_x_rot_d(mob) {
                mob.x_rot = rotate_towards(mob.x_rot, x_rot_d, self.x_max_rot_angle);
            }
        } else {
            mob.y_head_rot = rotate_towards(mob.y_head_rot, mob.y_body_rot, 10.0);
        }

        self.clamp_head_rotation_to_body(mob, navigation_is_done);
    }

    pub fn clamp_head_rotation_to_body(&self, mob: &mut MobPathingState, navigation_is_done: bool) {
        if !navigation_is_done {
            mob.y_head_rot =
                mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, mob.max_head_y_rot);
        }
    }
}

/// Port of `net.minecraft.world.entity.ai.control.SmoothSwimmingLookControl` in Paper 26.2.
#[derive(Debug, Clone)]
pub struct SmoothSwimmingLookControl {
    pub base: LookControl,
    pub max_y_rot_from_center: i32,
}

impl SmoothSwimmingLookControl {
    const HEAD_TILT_X: f32 = 10.0;
    const HEAD_TILT_Y: f32 = 20.0;

    #[must_use]
    pub const fn new(max_y_rot_from_center: i32) -> Self {
        Self {
            base: LookControl::new(),
            max_y_rot_from_center,
        }
    }

    /// Mirrors `SmoothSwimmingLookControl.tick()` in Paper 26.2.
    pub fn tick(&mut self, mob: &mut MobPathingState, navigation_is_done: bool) {
        if self.base.look_at_cooldown > 0 {
            self.base.look_at_cooldown -= 1;
            if let Some(y_rot_d) = self.base.get_y_rot_d(mob) {
                mob.y_head_rot = rotate_towards(
                    mob.y_head_rot,
                    y_rot_d + Self::HEAD_TILT_Y,
                    self.base.y_max_rot_speed,
                );
            }
            if let Some(x_rot_d) = self.base.get_x_rot_d(mob) {
                mob.x_rot = rotate_towards(
                    mob.x_rot,
                    x_rot_d + Self::HEAD_TILT_X,
                    self.base.x_max_rot_angle,
                );
            }
        } else {
            if navigation_is_done {
                mob.x_rot = rotate_towards(mob.x_rot, 0.0, 5.0);
            }
            mob.y_head_rot =
                rotate_towards(mob.y_head_rot, mob.y_body_rot, self.base.y_max_rot_speed);
        }

        let head_diff = mth::wrap_degrees_f32(mob.y_head_rot - mob.y_body_rot);
        let limit = self.max_y_rot_from_center as f32;
        if head_diff < -limit {
            mob.y_body_rot -= 4.0;
        } else if head_diff > limit {
            mob.y_body_rot += 4.0;
        }
    }
}

/// Port of `net.minecraft.world.entity.ai.control.BodyRotationControl` in Paper 26.2.
#[derive(Debug, Default, Clone)]
pub struct BodyRotationControl {
    head_stable_time: i32,
    last_stable_y_head_rot: f32,
}

impl BodyRotationControl {
    const HEAD_STABLE_ANGLE: f32 = 15.0;
    const DELAY_UNTIL_STARTING_TO_FACE_FORWARD: i32 = 10;
    const HOW_LONG_IT_TAKES_TO_FACE_FORWARD: i32 = 10;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            head_stable_time: 0,
            last_stable_y_head_rot: 0.0,
        }
    }

    /// Mirrors `BodyRotationControl.clientTick()` in Paper 26.2.
    pub fn client_tick(&mut self, mob: &mut MobPathingState) {
        if Self::is_moving(mob) {
            mob.y_body_rot = mob.y_rot;
            Self::rotate_head_if_necessary(mob);
            self.last_stable_y_head_rot = mob.y_head_rot;
            self.head_stable_time = 0;
        } else if !mob.has_mob_passenger {
            if (mob.y_head_rot - self.last_stable_y_head_rot).abs() > Self::HEAD_STABLE_ANGLE {
                self.head_stable_time = 0;
                self.last_stable_y_head_rot = mob.y_head_rot;
                Self::rotate_body_if_necessary(mob);
            } else {
                self.head_stable_time += 1;
                self.rotate_head_towards_front(mob);
            }
        }
    }

    fn rotate_body_if_necessary(mob: &mut MobPathingState) {
        mob.y_body_rot =
            mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, mob.max_head_y_rot);
    }

    fn rotate_head_if_necessary(mob: &mut MobPathingState) {
        mob.y_head_rot =
            mth::rotate_if_necessary(mob.y_head_rot, mob.y_body_rot, mob.max_head_y_rot);
    }

    fn rotate_head_towards_front(&self, mob: &mut MobPathingState) {
        let time_since_start = self.head_stable_time - Self::DELAY_UNTIL_STARTING_TO_FACE_FORWARD;
        let ratio = mth::clamp_f32(
            time_since_start as f32 / Self::HOW_LONG_IT_TAKES_TO_FACE_FORWARD as f32,
            0.0,
            1.0,
        );
        let max_angle = mob.max_head_y_rot * (1.0 - ratio);
        mob.y_body_rot = mth::rotate_if_necessary(mob.y_body_rot, mob.y_head_rot, max_angle);
    }

    fn is_moving(mob: &MobPathingState) -> bool {
        let xd = mob.pos.x - mob.prev_pos.x;
        let zd = mob.pos.z - mob.prev_pos.z;
        xd * xd + zd * zd > 2.500_000_3e-7
    }
}

/// Convenience helper for `BlockPos` center-bottom Vec3 used by controls/navigation.
#[must_use]
pub fn block_bottom_center(pos: BlockPos) -> Vec3 {
    Vec3::new(pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5)
}

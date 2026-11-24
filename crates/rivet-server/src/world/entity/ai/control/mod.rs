//! Port of `net.minecraft.world.entity.ai.control`.

pub mod body_rotation_control;
#[allow(clippy::module_inception)]
pub mod control;

pub use body_rotation_control::BodyRotationControl;

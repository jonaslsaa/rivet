//! Authoritative Paper 26.2 `Entity`, `LivingEntity`, `Mob`, and passenger lifecycle
//! foundation (`net.minecraft.world.entity.*` + `ServerLevel` entity tick traversal).
//!
//! Uses struct embedding (`Mob { living: LivingEntity { entity: Entity } }`), explicit
//! parent calls, and single-threaded take-tick-putback via [`LevelCtx`] with no
//! `Arc<RwLock>` game state.

pub mod dimensions;
pub mod entity;
pub mod equipment;
pub mod level_ctx;
pub mod living;
pub mod mob;

#[cfg(test)]
mod tests;

pub use dimensions::{
    EntityAttachment, EntityAttachments, EntityAttachmentsBuilder, EntityDimensions, Pose,
};
pub use entity::{
    BASE_TICKS_REQUIRED_TO_FREEZE, BOARDING_COOLDOWN, Entity, EntityTypeMeta, FLAG_FALL_FLYING,
    FLAG_GLOWING, FLAG_INVISIBLE, FLAG_ONFIRE, FLAG_SHIFT_KEY_DOWN, FLAG_SPRINTING, FLAG_SWIMMING,
    TOTAL_AIR_SUPPLY,
};
pub use equipment::{
    EntityEquipment, EquipmentItem, EquipmentSlot, EquipmentSlotChange, EquipmentSlotType,
    HumanoidArm,
};
pub use level_ctx::{LevelCtx, LifecycleEntity, LifecycleEvent};
pub use living::{
    LAST_HURT_BY_MOB_EXPIRY_TICKS, LIVING_ENTITY_FLAG_IS_USING, LIVING_ENTITY_FLAG_OFF_HAND,
    LIVING_ENTITY_FLAG_SPIN_ATTACK, LivingEntity, MAX_HEAD_ROTATION_RELATIVE_TO_BODY,
};
pub use mob::{
    DEFAULT_AMBIENT_SOUND_INTERVAL, DEFAULT_MAX_HEAD_Y_ROT, HARD_DESPAWN_DISTANCE_SQR,
    MOB_FLAG_AGGRESSIVE, MOB_FLAG_LEFTHANDED, MOB_FLAG_NO_AI, Mob, MobControlFlags,
    SOFT_DESPAWN_DISTANCE_SQR,
};

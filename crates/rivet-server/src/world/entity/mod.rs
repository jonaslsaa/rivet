//! `net.minecraft.world.entity`: `Entity`, `LivingEntity`, `Mob` and their value types.
//!
//! Uses struct embedding (`Mob { living: LivingEntity { entity: Entity } }`), explicit
//! parent calls, and single-threaded take-tick-putback via
//! [`LevelCtx`](crate::server::level::level_ctx::LevelCtx) with no `Arc<RwLock>` game state.

pub mod ai;
#[allow(clippy::module_inception)]
pub mod entity;
pub mod entity_attachments;
pub mod entity_dimensions;
pub mod entity_equipment;
pub mod entity_id;
pub mod entity_record;
pub mod equipment_slot;
pub mod humanoid_arm;
pub mod living_entity;
pub mod mob;
pub mod player;
pub mod pose;
pub mod projectile;

#[cfg(test)]
mod tests;

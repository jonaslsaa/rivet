//! Port of `net.minecraft.world.entity.projectile` (Paper 26.2).
//!
//! Covers projectile base state/ownership/movement/shooting/deflection,
//! deterministic collision ordering (`ProjectileUtil` + `AbstractArrow::step_move_and_hit`),
//! arrows (`AbstractArrow`, `Arrow`, `SpectralArrow`), and tridents (`ThrownTrident`),
//! including pickup, damage handoff, and NBT persistence.

pub mod arrow;
pub mod phys;
#[allow(clippy::module_inception)]
pub mod projectile;
pub mod projectile_deflection;
pub mod projectile_util;
pub mod world_view;

pub use arrow::{
    AbstractArrow, Arrow, Pickup, PotionContents, PotionEffect, SpectralArrow, ThrownTrident,
};
pub use phys::{
    Aabb, BlockHitResult, BlockPos, ClipBlock, Direction, EntityHitResult, EntityId, EntityUuid,
    HitResult, Vec3,
};
pub use projectile::{EntityReference, ProjectileState};
pub use projectile_deflection::ProjectileDeflection;
pub use world_view::{
    DamageHandoff, DamageKind, EntitySnapshot, HurtOutcome, PickupItem, PostHurtEffect,
    ProjectileGameEvent, ProjectileWorld,
};

#[cfg(test)]
mod tests;

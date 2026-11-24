//! `net.minecraft.world.phys.shapes.CollisionContext` — the live members
//! (D15 split rule). The interface and its live-free members are
//! `rivet_world::phys::shapes::collision_context`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/CollisionContext.java`.
//!
//! STUB(mc.world.level.block.state) `CollisionContext.getCollisionShape(BlockState, CollisionGetter, BlockPos)` is `state.getCollisionShape(collisionGetter, pos, this)`; `BlockBehaviour.getCollisionShape` is not ported.
//! STUB(mc.world.level.material) `CollisionContext.canStandOnFluid(FluidState, FluidState)` needs `FluidState.getType().isSame` and `LivingEntity.canStandOnFluid`.
//! STUB(mc.world.entity) `CollisionContext.of(Entity)`, `of(Entity, boolean)`, `placementContext(Player)` and `withPosition(Entity, double)` need `Entity.isDescending`, `getY` and `LivingEntity.getMainHandItem` on the entity arena.

use std::sync::{Arc, LazyLock};

use rivet_world::phys::shapes::collision_context::CollisionContext;
use rivet_world::phys::shapes::position_collision_context::PositionCollisionContext;

use super::entity_collision_context::{Empty, EntityCollisionContext};

/// `CollisionContext`'s members that need live types (D15 split rule,
/// OWNERSHIP.md §Level interfaces): `canStandOnFluid(FluidState, FluidState)`
/// and `getCollisionShape(BlockState, CollisionGetter, BlockPos)`. Every
/// implementor implements it with its own Java body.
pub trait CollisionContextLive: CollisionContext {
    // STUB(mc.world.level.material) `canStandOnFluid(FluidState, FluidState)` needs `FluidState` (`getType().isSame`) and `LivingEntity.canStandOnFluid`.
    // STUB(mc.world.level.block.state) `getCollisionShape(BlockState, CollisionGetter, BlockPos)` needs `BlockBehaviour.getCollisionShape`; every Java implementor's body is `state.getCollisionShape(collisionGetter, pos, this)`, `MinecartCollisionContext` overriding it.
}

impl CollisionContextLive for EntityCollisionContext {}
impl CollisionContextLive for Empty {}
impl CollisionContextLive for PositionCollisionContext {}

/// `EntityCollisionContext.Empty.WITHOUT_FLUID_COLLISIONS`.
static WITHOUT_FLUID_COLLISIONS: LazyLock<Arc<dyn CollisionContext>> =
    LazyLock::new(|| Arc::new(Empty::new(false)));

/// `EntityCollisionContext.Empty.WITH_FLUID_COLLISIONS`.
static WITH_FLUID_COLLISIONS: LazyLock<Arc<dyn CollisionContext>> =
    LazyLock::new(|| Arc::new(Empty::new(true)));

/// `CollisionContext.empty()`.
pub fn empty() -> Arc<dyn CollisionContext> {
    Arc::clone(&WITHOUT_FLUID_COLLISIONS)
}

/// `CollisionContext.emptyWithFluidCollisions()`.
pub fn empty_with_fluid_collisions() -> Arc<dyn CollisionContext> {
    Arc::clone(&WITH_FLUID_COLLISIONS)
}

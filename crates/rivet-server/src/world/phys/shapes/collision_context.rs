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

use super::entity_collision_context::EntityCollisionContext;

/// `EntityCollisionContext.Empty.WITHOUT_FLUID_COLLISIONS`.
static WITHOUT_FLUID_COLLISIONS: LazyLock<Arc<dyn CollisionContext>> =
    LazyLock::new(|| Arc::new(EntityCollisionContext::empty(false)));

/// `EntityCollisionContext.Empty.WITH_FLUID_COLLISIONS`.
static WITH_FLUID_COLLISIONS: LazyLock<Arc<dyn CollisionContext>> =
    LazyLock::new(|| Arc::new(EntityCollisionContext::empty(true)));

/// `CollisionContext.empty()`.
pub fn empty() -> Arc<dyn CollisionContext> {
    Arc::clone(&WITHOUT_FLUID_COLLISIONS)
}

/// `CollisionContext.emptyWithFluidCollisions()`.
pub fn empty_with_fluid_collisions() -> Arc<dyn CollisionContext> {
    Arc::clone(&WITH_FLUID_COLLISIONS)
}

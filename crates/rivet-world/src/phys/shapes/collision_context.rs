//! `net.minecraft.world.phys.shapes.CollisionContext` — the live-free members
//! of the interface (D15 split rule).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/CollisionContext.java`.
//!
//! The members that need live types go up to `rivet-server`'s
//! `CollisionContextLive: CollisionContext` subtrait (the same mirrored path,
//! implemented by every Java implementor; its two members are still STUBs
//! there):
//! `canStandOnFluid(FluidState, FluidState)`, `getCollisionShape(BlockState,
//! CollisionGetter, BlockPos)` and the static factories `empty()`,
//! `emptyWithFluidCollisions()`, `of(Entity)`, `of(Entity, boolean)`,
//! `placementContext(Player)` and `withPosition(Entity, double)`, whose
//! results are `EntityCollisionContext`s (which holds an `Entity`).
//! `positionContext(double)` is live-free and is [`position_context`].

use std::sync::Arc;

use rivet_registry::core::BlockPos;

use super::position_collision_context::PositionCollisionContext;
use super::voxel_shape::VoxelShape;
use crate::item::item_id::ItemId;

/// `CollisionContext`.
pub trait CollisionContext: Send + Sync {
    /// `CollisionContext.isDescending()`.
    fn is_descending(&self) -> bool;

    /// `CollisionContext.isAbove(VoxelShape, BlockPos, boolean)`.
    fn is_above(&self, shape: &VoxelShape, pos: &BlockPos, default_value: bool) -> bool;

    /// `CollisionContext.isHoldingItem(Item)`.
    fn is_holding_item(&self, item: ItemId) -> bool;

    /// `CollisionContext.alwaysCollideWithFluid()`.
    fn always_collide_with_fluid(&self) -> bool;

    /// `CollisionContext.isPlacement()`.
    fn is_placement(&self) -> bool {
        false
    }
}

/// `CollisionContext.positionContext(double)`.
pub fn position_context(y: f64) -> Arc<dyn CollisionContext> {
    Arc::new(PositionCollisionContext::new(y))
}

//! `net.minecraft.world.phys.shapes.PositionCollisionContext` — the
//! live-free members (D15 split rule); `canStandOnFluid` and
//! `getCollisionShape` are `rivet-server`'s `CollisionContextLive` members.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/PositionCollisionContext.java`.

use rivet_registry::core::{Axis, BlockPos};

use super::collision_context::CollisionContext;
use super::voxel_shape::VoxelShape;
use crate::item::item_id::ItemId;

/// `PositionCollisionContext`.
#[derive(Clone, Copy, Debug)]
pub struct PositionCollisionContext {
    y: f64,
}

impl PositionCollisionContext {
    /// `PositionCollisionContext(double)`.
    pub fn new(y: f64) -> PositionCollisionContext {
        PositionCollisionContext { y }
    }
}

impl CollisionContext for PositionCollisionContext {
    fn is_descending(&self) -> bool {
        false
    }

    /// `pos.getY() + shape.max(Y) - 1.0E-5F` — the `float` literal widens.
    fn is_above(&self, shape: &VoxelShape, pos: &BlockPos, _default_value: bool) -> bool {
        self.y > pos.get_y() as f64 + shape.max(Axis::Y) - 1.0E-5f32 as f64
    }

    fn is_holding_item(&self, _item: ItemId) -> bool {
        false
    }

    fn always_collide_with_fluid(&self) -> bool {
        false
    }
}

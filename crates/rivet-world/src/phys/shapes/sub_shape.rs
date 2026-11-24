//! `net.minecraft.world.phys.shapes.SubShape` — a window onto a parent
//! `DiscreteVoxelShape`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/SubShape.java`.

use std::sync::Arc;

use rivet_registry::core::Axis;
use rivet_util::mth;

use super::discrete_voxel_shape::{DiscreteVoxelShape, DiscreteVoxelShapeBase};

/// `SubShape extends DiscreteVoxelShape`.
#[derive(Debug)]
pub struct SubShape {
    discrete_voxel_shape: DiscreteVoxelShapeBase,
    parent: Arc<dyn DiscreteVoxelShape>,
    start_x: i32,
    start_y: i32,
    start_z: i32,
    end_x: i32,
    end_y: i32,
    end_z: i32,
}

impl SubShape {
    /// `SubShape(DiscreteVoxelShape, int x6)`.
    pub fn new(
        parent: Arc<dyn DiscreteVoxelShape>,
        start_x: i32,
        start_y: i32,
        start_z: i32,
        end_x: i32,
        end_y: i32,
        end_z: i32,
    ) -> SubShape {
        SubShape {
            discrete_voxel_shape: DiscreteVoxelShapeBase::new(
                end_x - start_x,
                end_y - start_y,
                end_z - start_z,
            ),
            parent,
            start_x,
            start_y,
            start_z,
            end_x,
            end_y,
            end_z,
        }
    }

    fn clamp_to_shape(&self, axis: Axis, parent_result: i32) -> i32 {
        let start = axis.choose(self.start_x, self.start_y, self.start_z);
        let end = axis.choose(self.end_x, self.end_y, self.end_z);
        mth::clamp(parent_result, start, end) - start
    }
}

impl DiscreteVoxelShape for SubShape {
    fn discrete_voxel_shape(&self) -> &DiscreteVoxelShapeBase {
        &self.discrete_voxel_shape
    }

    /// `SubShape.isFull(int, int, int)`.
    fn is_full(&self, x: i32, y: i32, z: i32) -> bool {
        self.parent
            .is_full(self.start_x + x, self.start_y + y, self.start_z + z)
    }

    /// `SubShape.fill(int, int, int)` — forwards to the parent, so it needs
    /// the parent to be uniquely owned (no Paper caller fills a `SubShape`;
    /// a shared parent is a bug, not a Java-reachable state).
    fn fill(&mut self, x: i32, y: i32, z: i32) {
        Arc::get_mut(&mut self.parent)
            .expect("SubShape.fill on a shared parent")
            .fill(self.start_x + x, self.start_y + y, self.start_z + z);
    }

    /// `SubShape.firstFull(Direction.Axis)`.
    fn first_full(&self, axis: Axis) -> i32 {
        self.clamp_to_shape(axis, self.parent.first_full(axis))
    }

    /// `SubShape.lastFull(Direction.Axis)`.
    fn last_full(&self, axis: Axis) -> i32 {
        self.clamp_to_shape(axis, self.parent.last_full(axis))
    }
}

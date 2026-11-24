//! `net.minecraft.world.phys.shapes.CubeVoxelShape` — a `VoxelShape` whose
//! coordinates are the even `CubePointRange` subdivisions of the unit cube.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/CubeVoxelShape.java`.

use std::any::Any;
use std::sync::Arc;

use rivet_registry::core::Axis;
use rivet_util::double_list::DoubleList;
use rivet_util::mth;

use super::cube_point_range::CubePointRange;
use super::discrete_voxel_shape::DiscreteVoxelShape;
use super::voxel_shape::{VoxelShape, VoxelShapeBehavior};

/// `CubeVoxelShape` (no fields of its own).
#[derive(Debug)]
pub struct CubeVoxelShape;

/// `new CubeVoxelShape(DiscreteVoxelShape)`.
pub fn new(shape: Arc<dyn DiscreteVoxelShape>) -> VoxelShape {
    VoxelShape::new(shape, Box::new(CubeVoxelShape))
}

impl VoxelShapeBehavior for CubeVoxelShape {
    /// `CubeVoxelShape.getCoords(Direction.Axis)`.
    fn get_coords(&self, shape: &dyn DiscreteVoxelShape, axis: Axis) -> Arc<dyn DoubleList> {
        Arc::new(CubePointRange::new(shape.get_size(axis)))
    }

    /// `CubeVoxelShape.findIndex(Direction.Axis, double)`.
    fn find_index(&self, shape: &dyn DiscreteVoxelShape, axis: Axis, coord: f64) -> Option<i32> {
        let size = shape.get_size(axis);
        Some(mth::floor_d(mth::clamp_f64(
            coord * size as f64,
            -1.0,
            size as f64,
        )))
    }

    fn is_cube_voxel_shape(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

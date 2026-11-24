//! `net.minecraft.world.phys.shapes.ArrayVoxelShape` — a `VoxelShape` with
//! explicit coordinate lists.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/ArrayVoxelShape.java`
//! (Paper: the `DoubleList` constructor is public).
//!
//! The package-private `ArrayVoxelShape(DiscreteVoxelShape, double[] x3)`
//! constructor has no caller in Paper 26.2 and is not ported.

use std::any::Any;
use std::sync::Arc;

use rivet_registry::core::Axis;
use rivet_util::double_list::DoubleList;

use super::discrete_voxel_shape::DiscreteVoxelShape;
use super::voxel_shape::{VoxelShape, VoxelShapeBehavior};

/// `ArrayVoxelShape`'s own fields.
#[derive(Debug)]
pub struct ArrayVoxelShape {
    xs: Arc<dyn DoubleList>,
    ys: Arc<dyn DoubleList>,
    zs: Arc<dyn DoubleList>,
}

/// `new ArrayVoxelShape(DiscreteVoxelShape, DoubleList, DoubleList,
/// DoubleList)` — `IllegalArgumentException` (a panic) when the list lengths
/// disagree with the grid size.
pub fn new(
    shape: Arc<dyn DiscreteVoxelShape>,
    xs: Arc<dyn DoubleList>,
    ys: Arc<dyn DoubleList>,
    zs: Arc<dyn DoubleList>,
) -> VoxelShape {
    let x_size = shape.get_x_size() + 1;
    let y_size = shape.get_y_size() + 1;
    let z_size = shape.get_z_size() + 1;
    if x_size as usize == xs.size() && y_size as usize == ys.size() && z_size as usize == zs.size()
    {
        VoxelShape::new(shape, Box::new(ArrayVoxelShape { xs, ys, zs }))
    } else {
        panic!("Lengths of point arrays must be consistent with the size of the VoxelShape.");
    }
}

impl VoxelShapeBehavior for ArrayVoxelShape {
    /// `ArrayVoxelShape.getCoords(Direction.Axis)`.
    fn get_coords(&self, _shape: &dyn DiscreteVoxelShape, axis: Axis) -> Arc<dyn DoubleList> {
        match axis {
            Axis::X => Arc::clone(&self.xs),
            Axis::Y => Arc::clone(&self.ys),
            Axis::Z => Arc::clone(&self.zs),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

//! `net.minecraft.world.phys.shapes.SliceShape` — a one-voxel-thick slice of
//! another shape along an axis.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/SliceShape.java`.

use std::any::Any;
use std::sync::{Arc, LazyLock};

use rivet_registry::core::Axis;
use rivet_util::double_list::DoubleList;

use super::cube_point_range::CubePointRange;
use super::discrete_voxel_shape::DiscreteVoxelShape;
use super::sub_shape::SubShape;
use super::voxel_shape::{VoxelShape, VoxelShapeBehavior};

/// `SliceShape.SLICE_COORDS`.
static SLICE_COORDS: LazyLock<Arc<dyn DoubleList>> =
    LazyLock::new(|| Arc::new(CubePointRange::new(1)));

/// `SliceShape`'s own fields.
#[derive(Debug)]
pub struct SliceShape {
    delegate: VoxelShape,
    axis: Axis,
}

/// `new SliceShape(VoxelShape, Direction.Axis, int)`.
pub fn new(delegate: &VoxelShape, axis: Axis, point: i32) -> VoxelShape {
    let shape = make_slice(delegate.shape(), axis, point);
    VoxelShape::new(
        shape,
        Box::new(SliceShape {
            delegate: delegate.clone(),
            axis,
        }),
    )
}

/// `SliceShape.makeSlice(DiscreteVoxelShape, Direction.Axis, int)`.
fn make_slice(
    delegate: &Arc<dyn DiscreteVoxelShape>,
    axis: Axis,
    point: i32,
) -> Arc<dyn DiscreteVoxelShape> {
    let base = delegate.discrete_voxel_shape();
    Arc::new(SubShape::new(
        Arc::clone(delegate),
        axis.choose(point, 0, 0),
        axis.choose(0, point, 0),
        axis.choose(0, 0, point),
        axis.choose(point + 1, base.x_size, base.x_size),
        axis.choose(base.y_size, point + 1, base.y_size),
        axis.choose(base.z_size, base.z_size, point + 1),
    ))
}

impl VoxelShapeBehavior for SliceShape {
    /// `SliceShape.getCoords(Direction.Axis)`.
    fn get_coords(&self, _shape: &dyn DiscreteVoxelShape, axis: Axis) -> Arc<dyn DoubleList> {
        if axis == self.axis {
            Arc::clone(&SLICE_COORDS)
        } else {
            self.delegate.get_coords(axis)
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

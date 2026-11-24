//! `net.minecraft.world.phys.shapes.DiscreteVoxelShape` — the abstract voxel
//! grid under a `VoxelShape` (Paper: implements `CollisionDiscreteVoxelShape`).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/DiscreteVoxelShape.java`.
//!
//! `DiscreteVoxelShape` has subclasses (`BitSetDiscreteVoxelShape`,
//! `SubShape`), so it follows OWNERSHIP.md §Class hierarchies: the struct
//! [`DiscreteVoxelShapeBase`] holds the abstract class's own fields and is
//! embedded in each subclass; the trait holds the abstract members and the
//! overridable ones; the non-virtual members are inherent methods on
//! `dyn DiscreteVoxelShape`. `rotate(OctahedralGroup)` takes
//! `com.mojang.math.OctahedralGroup`, which is not ported:
//! STUB(com.mojang.math) below.

use std::fmt;
use std::sync::{Arc, OnceLock};

use rivet_registry::core::{Axis, AxisCycle, AxisDirection, Direction};

use super::bit_set_discrete_voxel_shape::BitSetDiscreteVoxelShape;
use crate::spottedleaf::moonrise::patches::collisions::shape::cached_shape_data::CachedShapeData;

// STUB(com.mojang.math): `DiscreteVoxelShape.rotate(OctahedralGroup)` and its
// private `fixupCoordinate(Vector3i, int)` need `OctahedralGroup` and JOML
// `Vector3i`, whose units are not ported; they land with that unit.

/// `DiscreteVoxelShape.AXIS_VALUES`.
const AXIS_VALUES: [Axis; 3] = Axis::VALUES;

/// The fields of the abstract `DiscreteVoxelShape`: `xSize`, `ySize`, `zSize`
/// and Paper's lazily created `cachedShapeData` ("ignore race conditions on
/// field read/write: the shape is static").
#[derive(Debug)]
pub struct DiscreteVoxelShapeBase {
    pub x_size: i32,
    pub y_size: i32,
    pub z_size: i32,
    cached_shape_data: OnceLock<Arc<CachedShapeData>>,
}

impl DiscreteVoxelShapeBase {
    /// `DiscreteVoxelShape(int, int, int)` — `IllegalArgumentException` on a
    /// negative size.
    pub fn new(x_size: i32, y_size: i32, z_size: i32) -> DiscreteVoxelShapeBase {
        if x_size >= 0 && y_size >= 0 && z_size >= 0 {
            DiscreteVoxelShapeBase {
                x_size,
                y_size,
                z_size,
                cached_shape_data: OnceLock::new(),
            }
        } else {
            panic!("Need all positive sizes: x: {x_size}, y: {y_size}, z: {z_size}");
        }
    }
}

/// The virtual members of `DiscreteVoxelShape`.
pub trait DiscreteVoxelShape: Send + Sync + fmt::Debug {
    /// The embedded abstract-class state.
    fn discrete_voxel_shape(&self) -> &DiscreteVoxelShapeBase;

    /// `instanceof BitSetDiscreteVoxelShape`.
    fn as_bit_set_discrete_voxel_shape(&self) -> Option<&BitSetDiscreteVoxelShape> {
        None
    }

    /// `DiscreteVoxelShape.isFull(int, int, int)` — abstract.
    fn is_full(&self, x: i32, y: i32, z: i32) -> bool;

    /// `DiscreteVoxelShape.fill(int, int, int)` — abstract.
    fn fill(&mut self, x: i32, y: i32, z: i32);

    /// `DiscreteVoxelShape.isEmpty()`.
    fn is_empty(&self) -> bool {
        for axis in AXIS_VALUES {
            if self.first_full(axis) >= self.last_full(axis) {
                return true;
            }
        }

        false
    }

    /// `DiscreteVoxelShape.firstFull(Direction.Axis)` — abstract.
    fn first_full(&self, axis: Axis) -> i32;

    /// `DiscreteVoxelShape.lastFull(Direction.Axis)` — abstract.
    fn last_full(&self, axis: Axis) -> i32;
}

impl<'a> dyn DiscreteVoxelShape + 'a {
    /// `moonrise$getOrCreateCachedShapeData()`.
    pub fn moonrise_get_or_create_cached_shape_data(&self) -> Arc<CachedShapeData> {
        let base = self.discrete_voxel_shape();
        Arc::clone(base.cached_shape_data.get_or_init(|| {
            let size_x = self.get_x_size();
            let size_y = self.get_y_size();
            let size_z = self.get_z_size();

            let max_index = size_x.wrapping_mul(size_y).wrapping_mul(size_z); // exclusive

            let longs_required = ((max_index.wrapping_add(63)) as u32 >> 6) as usize;
            let mut voxel_set: Vec<u64>;

            let is_empty = self.is_empty();

            if let Some(bitset_shape) = self.as_bit_set_discrete_voxel_shape() {
                voxel_set = bitset_shape.storage.to_long_array();
                if voxel_set.len() < longs_required {
                    // happens when the later long values are 0L, so we need to resize
                    voxel_set.resize(longs_required, 0);
                }
            } else {
                voxel_set = vec![0; longs_required];
                if !is_empty {
                    let mul_x = size_z.wrapping_mul(size_y);
                    for x in 0..size_x {
                        for y in 0..size_y {
                            for z in 0..size_z {
                                if self.is_full(x, y, z) {
                                    // index = z + y*size_z + x*(size_z*size_y)
                                    let index = z
                                        .wrapping_add(y.wrapping_mul(size_z))
                                        .wrapping_add(x.wrapping_mul(mul_x));

                                    voxel_set[(index as u32 >> 6) as usize] |=
                                        1u64.wrapping_shl(index as u32);
                                }
                            }
                        }
                    }
                }
            }

            let has_single_aabb =
                size_x == 1 && size_y == 1 && size_z == 1 && !is_empty && (voxel_set[0] & 1) != 0;

            let min_full_x = self.first_full(Axis::X);
            let min_full_y = self.first_full(Axis::Y);
            let min_full_z = self.first_full(Axis::Z);

            let max_full_x = self.last_full(Axis::X);
            let max_full_y = self.last_full(Axis::Y);
            let max_full_z = self.last_full(Axis::Z);

            Arc::new(CachedShapeData {
                size_x,
                size_y,
                size_z,
                voxel_set,
                min_full_x,
                min_full_y,
                min_full_z,
                max_full_x,
                max_full_y,
                max_full_z,
                is_empty,
                has_single_aabb,
            })
        }))
    }

    /// `DiscreteVoxelShape.isFullWide(AxisCycle, int, int, int)`.
    pub fn is_full_wide_cycle(&self, transform: AxisCycle, x: i32, y: i32, z: i32) -> bool {
        self.is_full_wide(
            transform.cycle(x, y, z, Axis::X),
            transform.cycle(x, y, z, Axis::Y),
            transform.cycle(x, y, z, Axis::Z),
        )
    }

    /// `DiscreteVoxelShape.isFullWide(int, int, int)`.
    pub fn is_full_wide(&self, x: i32, y: i32, z: i32) -> bool {
        let base = self.discrete_voxel_shape();
        x >= 0
            && y >= 0
            && z >= 0
            && x < base.x_size
            && y < base.y_size
            && z < base.z_size
            && self.is_full(x, y, z)
    }

    /// `DiscreteVoxelShape.isFull(AxisCycle, int, int, int)`.
    pub fn is_full_cycle(&self, transform: AxisCycle, x: i32, y: i32, z: i32) -> bool {
        self.is_full(
            transform.cycle(x, y, z, Axis::X),
            transform.cycle(x, y, z, Axis::Y),
            transform.cycle(x, y, z, Axis::Z),
        )
    }

    /// `DiscreteVoxelShape.firstFull(Direction.Axis, int, int)`.
    pub fn first_full_bc(&self, a_axis: Axis, b: i32, c: i32) -> i32 {
        let a_size = self.get_size(a_axis);
        if b >= 0 && c >= 0 {
            let b_axis = AxisCycle::Forward.cycle_axis(a_axis);
            let c_axis = AxisCycle::Backward.cycle_axis(a_axis);
            if b < self.get_size(b_axis) && c < self.get_size(c_axis) {
                let transform = AxisCycle::between(Axis::X, a_axis);

                for a in 0..a_size {
                    if self.is_full_cycle(transform, a, b, c) {
                        return a;
                    }
                }

                a_size
            } else {
                a_size
            }
        } else {
            a_size
        }
    }

    /// `DiscreteVoxelShape.lastFull(Direction.Axis, int, int)`.
    pub fn last_full_bc(&self, a_axis: Axis, b: i32, c: i32) -> i32 {
        if b >= 0 && c >= 0 {
            let b_axis = AxisCycle::Forward.cycle_axis(a_axis);
            let c_axis = AxisCycle::Backward.cycle_axis(a_axis);
            if b < self.get_size(b_axis) && c < self.get_size(c_axis) {
                let a_size = self.get_size(a_axis);
                let transform = AxisCycle::between(Axis::X, a_axis);

                let mut a = a_size - 1;
                while a >= 0 {
                    if self.is_full_cycle(transform, a, b, c) {
                        return a + 1;
                    }
                    a -= 1;
                }

                0
            } else {
                0
            }
        } else {
            0
        }
    }

    /// `DiscreteVoxelShape.getSize(Direction.Axis)`.
    pub fn get_size(&self, axis: Axis) -> i32 {
        let base = self.discrete_voxel_shape();
        axis.choose(base.x_size, base.y_size, base.z_size)
    }

    /// `DiscreteVoxelShape.getXSize()`.
    pub fn get_x_size(&self) -> i32 {
        self.get_size(Axis::X)
    }

    /// `DiscreteVoxelShape.getYSize()`.
    pub fn get_y_size(&self) -> i32 {
        self.get_size(Axis::Y)
    }

    /// `DiscreteVoxelShape.getZSize()`.
    pub fn get_z_size(&self) -> i32 {
        self.get_size(Axis::Z)
    }

    /// `DiscreteVoxelShape.forAllEdges(IntLineConsumer, boolean)`.
    pub fn for_all_edges(
        &self,
        consumer: &mut dyn FnMut(i32, i32, i32, i32, i32, i32),
        merge_neighbors: bool,
    ) {
        self.for_all_axis_edges(consumer, AxisCycle::None, merge_neighbors);
        self.for_all_axis_edges(consumer, AxisCycle::Forward, merge_neighbors);
        self.for_all_axis_edges(consumer, AxisCycle::Backward, merge_neighbors);
    }

    fn for_all_axis_edges(
        &self,
        consumer: &mut dyn FnMut(i32, i32, i32, i32, i32, i32),
        transform: AxisCycle,
        merge_neighbors: bool,
    ) {
        let inverse = transform.inverse();
        let a_size = self.get_size(inverse.cycle_axis(Axis::X));
        let b_size = self.get_size(inverse.cycle_axis(Axis::Y));
        let c_size = self.get_size(inverse.cycle_axis(Axis::Z));

        for a in 0..=a_size {
            for b in 0..=b_size {
                let mut last_start = -1;

                for c in 0..=c_size {
                    let mut full_sectors = 0;
                    let mut odd_sectors = 0;

                    for da in 0..=1 {
                        for db in 0..=1 {
                            if self.is_full_wide_cycle(inverse, a + da - 1, b + db - 1, c) {
                                full_sectors += 1;
                                odd_sectors ^= da ^ db;
                            }
                        }
                    }

                    if full_sectors == 1
                        || full_sectors == 3
                        || (full_sectors == 2 && (odd_sectors & 1) == 0)
                    {
                        if merge_neighbors {
                            if last_start == -1 {
                                last_start = c;
                            }
                        } else {
                            consumer(
                                inverse.cycle(a, b, c, Axis::X),
                                inverse.cycle(a, b, c, Axis::Y),
                                inverse.cycle(a, b, c, Axis::Z),
                                inverse.cycle(a, b, c + 1, Axis::X),
                                inverse.cycle(a, b, c + 1, Axis::Y),
                                inverse.cycle(a, b, c + 1, Axis::Z),
                            );
                        }
                    } else if last_start != -1 {
                        consumer(
                            inverse.cycle(a, b, last_start, Axis::X),
                            inverse.cycle(a, b, last_start, Axis::Y),
                            inverse.cycle(a, b, last_start, Axis::Z),
                            inverse.cycle(a, b, c, Axis::X),
                            inverse.cycle(a, b, c, Axis::Y),
                            inverse.cycle(a, b, c, Axis::Z),
                        );
                        last_start = -1;
                    }
                }
            }
        }
    }

    /// `DiscreteVoxelShape.forAllBoxes(IntLineConsumer, boolean)`.
    pub fn for_all_boxes(
        &self,
        consumer: &mut dyn FnMut(i32, i32, i32, i32, i32, i32),
        merge_neighbors: bool,
    ) {
        BitSetDiscreteVoxelShape::for_all_boxes(self, consumer, merge_neighbors);
    }

    /// `DiscreteVoxelShape.forAllFaces(IntFaceConsumer)`.
    pub fn for_all_faces(&self, consumer: &mut dyn FnMut(Direction, i32, i32, i32)) {
        self.for_all_axis_faces(consumer, AxisCycle::None);
        self.for_all_axis_faces(consumer, AxisCycle::Forward);
        self.for_all_axis_faces(consumer, AxisCycle::Backward);
    }

    fn for_all_axis_faces(
        &self,
        consumer: &mut dyn FnMut(Direction, i32, i32, i32),
        transform: AxisCycle,
    ) {
        let inverse = transform.inverse();
        let c_axis = inverse.cycle_axis(Axis::Z);
        let a_size = self.get_size(inverse.cycle_axis(Axis::X));
        let b_size = self.get_size(inverse.cycle_axis(Axis::Y));
        let c_size = self.get_size(c_axis);
        let negative = Direction::from_axis_and_direction(c_axis, AxisDirection::Negative);
        let positive = Direction::from_axis_and_direction(c_axis, AxisDirection::Positive);

        for a in 0..a_size {
            for b in 0..b_size {
                let mut last_full = false;

                for c in 0..=c_size {
                    let full = c != c_size && self.is_full_cycle(inverse, a, b, c);
                    if !last_full && full {
                        consumer(
                            negative,
                            inverse.cycle(a, b, c, Axis::X),
                            inverse.cycle(a, b, c, Axis::Y),
                            inverse.cycle(a, b, c, Axis::Z),
                        );
                    }

                    if last_full && !full {
                        consumer(
                            positive,
                            inverse.cycle(a, b, c - 1, Axis::X),
                            inverse.cycle(a, b, c - 1, Axis::Y),
                            inverse.cycle(a, b, c - 1, Axis::Z),
                        );
                    }

                    last_full = full;
                }
            }
        }
    }
}

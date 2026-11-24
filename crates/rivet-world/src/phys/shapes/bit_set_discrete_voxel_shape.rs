//! `net.minecraft.world.phys.shapes.BitSetDiscreteVoxelShape` — a voxel grid
//! backed by a `java.util.BitSet`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/BitSetDiscreteVoxelShape.java`
//! (Paper: public `storage`/bounds fields, optimised `forAllBoxes`).
//!
//! The private `isZStripFull`/`isXZRectangleFull`/`clearZStrip` are unused in
//! Paper (vanilla's `forAllBoxes` was replaced) and are kept for fidelity.

use rivet_registry::core::Axis;
use rivet_util::java_bit_set::BitSet;

use super::boolean_op::BooleanOp;
use super::discrete_voxel_shape::{DiscreteVoxelShape, DiscreteVoxelShapeBase};
use super::index_merger::IndexMerger;
use crate::spottedleaf::moonrise::common::util::{flat_bitset_util, mixin_workarounds};

/// `BitSetDiscreteVoxelShape extends DiscreteVoxelShape`.
#[derive(Debug)]
pub struct BitSetDiscreteVoxelShape {
    discrete_voxel_shape: DiscreteVoxelShapeBase,
    pub storage: BitSet,
    pub x_min: i32,
    pub y_min: i32,
    pub z_min: i32,
    pub x_max: i32,
    pub y_max: i32,
    pub z_max: i32,
}

impl BitSetDiscreteVoxelShape {
    /// `BitSetDiscreteVoxelShape(int, int, int)`.
    pub fn new(x_size: i32, y_size: i32, z_size: i32) -> BitSetDiscreteVoxelShape {
        let discrete_voxel_shape = DiscreteVoxelShapeBase::new(x_size, y_size, z_size);
        BitSetDiscreteVoxelShape {
            storage: BitSet::new(x_size.wrapping_mul(y_size).wrapping_mul(z_size) as usize),
            x_min: x_size,
            y_min: y_size,
            z_min: z_size,
            x_max: 0,
            y_max: 0,
            z_max: 0,
            discrete_voxel_shape,
        }
    }

    /// `BitSetDiscreteVoxelShape.withFilledBounds(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn with_filled_bounds(
        x_size: i32,
        y_size: i32,
        z_size: i32,
        x_min: i32,
        y_min: i32,
        z_min: i32,
        x_max: i32,
        y_max: i32,
        z_max: i32,
    ) -> BitSetDiscreteVoxelShape {
        let mut shape = BitSetDiscreteVoxelShape::new(x_size, y_size, z_size);
        shape.x_min = x_min;
        shape.y_min = y_min;
        shape.z_min = z_min;
        shape.x_max = x_max;
        shape.y_max = y_max;
        shape.z_max = z_max;

        for x in x_min..x_max {
            for y in y_min..y_max {
                for z in z_min..z_max {
                    shape.fill_update_bounds(x, y, z, false);
                }
            }
        }

        shape
    }

    /// `BitSetDiscreteVoxelShape(DiscreteVoxelShape)`.
    pub fn from_discrete(voxel_shape: &dyn DiscreteVoxelShape) -> BitSetDiscreteVoxelShape {
        let base = voxel_shape.discrete_voxel_shape();
        let discrete_voxel_shape =
            DiscreteVoxelShapeBase::new(base.x_size, base.y_size, base.z_size);
        let storage;
        if let Some(bit_set) = voxel_shape.as_bit_set_discrete_voxel_shape() {
            storage = bit_set.storage.clone();
        } else {
            let mut bits = BitSet::new(
                discrete_voxel_shape
                    .x_size
                    .wrapping_mul(discrete_voxel_shape.y_size)
                    .wrapping_mul(discrete_voxel_shape.z_size) as usize,
            );

            for x in 0..discrete_voxel_shape.x_size {
                for y in 0..discrete_voxel_shape.y_size {
                    for z in 0..discrete_voxel_shape.z_size {
                        if voxel_shape.is_full(x, y, z) {
                            let index = (x * discrete_voxel_shape.y_size + y)
                                * discrete_voxel_shape.z_size
                                + z;
                            bits.set(index as usize);
                        }
                    }
                }
            }
            storage = bits;
        }

        BitSetDiscreteVoxelShape {
            storage,
            x_min: voxel_shape.first_full(Axis::X),
            y_min: voxel_shape.first_full(Axis::Y),
            z_min: voxel_shape.first_full(Axis::Z),
            x_max: voxel_shape.last_full(Axis::X),
            y_max: voxel_shape.last_full(Axis::Y),
            z_max: voxel_shape.last_full(Axis::Z),
            discrete_voxel_shape,
        }
    }

    /// `getIndex(int, int, int)` — Java `int` arithmetic; a negative index
    /// throws `IndexOutOfBoundsException` at the `BitSet` call.
    fn get_index(&self, x: i32, y: i32, z: i32) -> usize {
        let base = &self.discrete_voxel_shape;
        let index = x
            .wrapping_mul(base.y_size)
            .wrapping_add(y)
            .wrapping_mul(base.z_size)
            .wrapping_add(z);
        usize::try_from(index).expect("bitIndex < 0")
    }

    fn fill_update_bounds(&mut self, x: i32, y: i32, z: i32, update_bounds: bool) {
        let index = self.get_index(x, y, z);
        self.storage.set(index);
        if update_bounds {
            self.x_min = self.x_min.min(x);
            self.y_min = self.y_min.min(y);
            self.z_min = self.z_min.min(z);
            self.x_max = self.x_max.max(x + 1);
            self.y_max = self.y_max.max(y + 1);
            self.z_max = self.z_max.max(z + 1);
        }
    }

    /// `BitSetDiscreteVoxelShape.join(first, second, xMerger, yMerger, zMerger,
    /// op)`.
    pub fn join(
        first: &dyn DiscreteVoxelShape,
        second: &dyn DiscreteVoxelShape,
        x_merger: &dyn IndexMerger,
        y_merger: &dyn IndexMerger,
        z_merger: &dyn IndexMerger,
        op: BooleanOp,
    ) -> BitSetDiscreteVoxelShape {
        let mut shape = BitSetDiscreteVoxelShape::new(
            x_merger.size() as i32 - 1,
            y_merger.size() as i32 - 1,
            z_merger.size() as i32 - 1,
        );
        let mut bounds = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
        x_merger.for_merged_indexes(&mut |x1, x2, xr| {
            let mut updated_slice = false;
            y_merger.for_merged_indexes(&mut |y1, y2, yr| {
                let mut updated_column = false;
                z_merger.for_merged_indexes(&mut |z1, z2, zr| {
                    if op.apply(
                        first.is_full_wide(x1, y1, z1),
                        second.is_full_wide(x2, y2, z2),
                    ) {
                        let index = shape.get_index(xr, yr, zr);
                        shape.storage.set(index);
                        bounds[2] = bounds[2].min(zr);
                        bounds[5] = bounds[5].max(zr);
                        updated_column = true;
                    }

                    true
                });
                if updated_column {
                    bounds[1] = bounds[1].min(yr);
                    bounds[4] = bounds[4].max(yr);
                    updated_slice = true;
                }

                true
            });
            if updated_slice {
                bounds[0] = bounds[0].min(xr);
                bounds[3] = bounds[3].max(xr);
            }

            true
        });
        shape.x_min = bounds[0];
        shape.y_min = bounds[1];
        shape.z_min = bounds[2];
        shape.x_max = bounds[3].wrapping_add(1);
        shape.y_max = bounds[4].wrapping_add(1);
        shape.z_max = bounds[5].wrapping_add(1);
        shape
    }

    /// `BitSetDiscreteVoxelShape.forAllBoxes(DiscreteVoxelShape,
    /// IntLineConsumer, boolean)` — Paper's optimised version.
    pub fn for_all_boxes(
        shape: &dyn DiscreteVoxelShape,
        consumer: &mut dyn FnMut(i32, i32, i32, i32, i32, i32),
        merge_adjacent: bool,
    ) {
        // called with the shape of a VoxelShape, so we can expect the cache to exist
        let cache = shape.moonrise_get_or_create_cached_shape_data();

        let size_x = cache.size_x;
        let size_y = cache.size_y;
        let size_z = cache.size_z;

        let mut index_x: i32;
        let mut index_y: i32 = 0;
        let mut index_z: i32;

        let inc_y = size_z;
        let inc_x = size_z * size_y;

        let bitset: &[u64] = &cache.voxel_set;

        // index = z + y*size_z + x*(size_z*size_y)

        if !merge_adjacent {
            // due to the odd selection of loop order (which does affect behavior,
            // unfortunately) we can't simply increment an index in the Z loop, and
            // have to keep track of 3 counters to avoid the multiplication
            for y in 0..size_y {
                index_x = index_y;
                for x in 0..size_x {
                    index_z = index_x;
                    for z in 0..size_z {
                        if (bitset[(index_z as u32 >> 6) as usize]
                            & 1u64.wrapping_shl(index_z as u32))
                            != 0
                        {
                            consumer(x, y, z, x + 1, y + 1, z + 1);
                        }
                        index_z += 1;
                    }
                    index_x += inc_x;
                }
                index_y += inc_y;
            }
        } else {
            // same notes about loop order as the above
            // this branch is actually important to optimise, as it affects
            // uncached toAabbs() (which affects optimize())

            // only clone when we may write to it
            let mut bitset = mixin_workarounds::clone(bitset);

            for y in 0..size_y {
                index_x = index_y;
                for x in 0..size_x {
                    let mut z_idx = index_x;
                    let end_index = index_x + size_z;
                    while z_idx < end_index {
                        let first_set_z = flat_bitset_util::first_set(&bitset, z_idx, end_index);

                        if first_set_z == -1 {
                            break;
                        }

                        let mut last_set_z =
                            flat_bitset_util::first_clear(&bitset, first_set_z, end_index);
                        if last_set_z == -1 {
                            last_set_z = end_index;
                        }

                        flat_bitset_util::clear_range(&mut bitset, first_set_z, last_set_z);

                        // try to merge neighbouring on the X axis
                        let mut end_x = x + 1; // exclusive
                        let mut neighbour_idx_start = first_set_z + inc_x;
                        let mut neighbour_idx_end = last_set_z + inc_x;
                        while end_x < size_x
                            && flat_bitset_util::is_range_set(
                                &bitset,
                                neighbour_idx_start,
                                neighbour_idx_end,
                            )
                        {
                            end_x += 1;
                            flat_bitset_util::clear_range(
                                &mut bitset,
                                neighbour_idx_start,
                                neighbour_idx_end,
                            );
                            neighbour_idx_start += inc_x;
                            neighbour_idx_end += inc_x;
                        }

                        // try to merge neighbouring on the Y axis

                        let mut end_y = y + 1; // exclusive
                        let mut first_set_zy = first_set_z + inc_y;
                        let mut last_set_zy = last_set_z + inc_y;
                        'y_merge: while end_y < size_y {
                            // test the whole XZ range
                            let mut test_x = x;
                            let mut start = first_set_zy;
                            let mut end = last_set_zy;
                            while test_x < end_x {
                                if !flat_bitset_util::is_range_set(&bitset, start, end) {
                                    break 'y_merge;
                                }
                                test_x += 1;
                                start += inc_x;
                                end += inc_x;
                            }

                            end_y += 1;

                            // passed, so we can clear it
                            let mut test_x = x;
                            let mut start = first_set_zy;
                            let mut end = last_set_zy;
                            while test_x < end_x {
                                flat_bitset_util::clear_range(&mut bitset, start, end);
                                test_x += 1;
                                start += inc_x;
                                end += inc_x;
                            }

                            first_set_zy += inc_y;
                            last_set_zy += inc_y;
                        }

                        consumer(
                            x,
                            y,
                            first_set_z - index_x,
                            end_x,
                            end_y,
                            last_set_z - index_x,
                        );
                        z_idx = last_set_z;
                    }
                    index_x += inc_x;
                }
                index_y += inc_y;
            }
        }
    }

    #[allow(dead_code)] // unused in Paper (vanilla `forAllBoxes` path)
    fn is_z_strip_full(&self, start_z: i32, end_z: i32, x: i32, y: i32) -> bool {
        let base = &self.discrete_voxel_shape;
        x < base.x_size
            && y < base.y_size
            && self.storage.next_clear_bit(self.get_index(x, y, start_z))
                >= self.get_index(x, y, end_z)
    }

    #[allow(dead_code)] // unused in Paper (vanilla `forAllBoxes` path)
    fn is_xz_rectangle_full(
        &self,
        start_x: i32,
        end_x: i32,
        start_z: i32,
        end_z: i32,
        y: i32,
    ) -> bool {
        for x in start_x..end_x {
            if !self.is_z_strip_full(start_z, end_z, x, y) {
                return false;
            }
        }

        true
    }

    #[allow(dead_code)] // unused in Paper (vanilla `forAllBoxes` path)
    fn clear_z_strip(&mut self, start_z: i32, end_z: i32, x: i32, y: i32) {
        let from = self.get_index(x, y, start_z);
        let to = self.get_index(x, y, end_z);
        self.storage.clear_range(from, to);
    }

    /// `BitSetDiscreteVoxelShape.isInterior(int, int, int)`.
    pub fn is_interior(&self, x: i32, y: i32, z: i32) -> bool {
        let base = &self.discrete_voxel_shape;
        let is_interior = x > 0
            && x < base.x_size - 1
            && y > 0
            && y < base.y_size - 1
            && z > 0
            && z < base.z_size - 1;
        is_interior
            && self.is_full(x, y, z)
            && self.is_full(x - 1, y, z)
            && self.is_full(x + 1, y, z)
            && self.is_full(x, y - 1, z)
            && self.is_full(x, y + 1, z)
            && self.is_full(x, y, z - 1)
            && self.is_full(x, y, z + 1)
    }
}

impl DiscreteVoxelShape for BitSetDiscreteVoxelShape {
    fn discrete_voxel_shape(&self) -> &DiscreteVoxelShapeBase {
        &self.discrete_voxel_shape
    }

    fn as_bit_set_discrete_voxel_shape(&self) -> Option<&BitSetDiscreteVoxelShape> {
        Some(self)
    }

    /// `BitSetDiscreteVoxelShape.isFull(int, int, int)`.
    fn is_full(&self, x: i32, y: i32, z: i32) -> bool {
        self.storage.get(self.get_index(x, y, z))
    }

    /// `BitSetDiscreteVoxelShape.fill(int, int, int)`.
    fn fill(&mut self, x: i32, y: i32, z: i32) {
        self.fill_update_bounds(x, y, z, true);
    }

    /// `BitSetDiscreteVoxelShape.isEmpty()`.
    fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    fn first_full(&self, axis: Axis) -> i32 {
        axis.choose(self.x_min, self.y_min, self.z_min)
    }

    fn last_full(&self, axis: Axis) -> i32 {
        axis.choose(self.x_max, self.y_max, self.z_max)
    }
}

//! `ca.spottedleaf.moonrise.patches.collisions.CollisionUtil` — the shape
//! members. D15 split class, assigned to `rivet-world` (COVERAGE.tsv; the
//! shape caches `VoxelShape` holds and calls cannot sit above it): this file
//! is the live-free half. The members that take `Entity`/`Level`/`BlockState`/
//! `WorldBorder` (`isSpecialCollidingBlock`, `getCollisionsForBlocksOrWorldBorder`
//! and later, and the `perform*Collisions` family from `offsetX` on) are
//! `CollisionUtilLive`-side members at the same mirrored path in `rivet-server`
//! with the Moonrise unit (the ledger lists that half as `(pending)`).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/moonrise/patches/collisions/CollisionUtil.java`
//! (the members from `COLLISION_EPSILON` through `shapeMismatch`).
//!
//! `VoxelShape.moonrise$*` accessors are `VoxelShape::moonrise_*`; array
//! identity (`firstCoordinates == secondCoordinates`) is `Arc::ptr_eq` on the
//! shared coordinate arrays.

use std::sync::{Arc, LazyLock};

use rivet_registry::core::Axis;
use rivet_registry::world::phys::{AABB, Vec3};
use rivet_util::double_list::{DoubleArrayList, DoubleList, double_list_equals};

use crate::phys::shapes::array_voxel_shape;
use crate::phys::shapes::bit_set_discrete_voxel_shape::BitSetDiscreteVoxelShape;
use crate::phys::shapes::boolean_op::BooleanOp;
use crate::phys::shapes::discrete_voxel_shape::DiscreteVoxelShape;
use crate::phys::shapes::offset_double_list::OffsetDoubleList;
use crate::phys::shapes::shapes;
use crate::phys::shapes::slice_shape as slice_shape_class;
use crate::phys::shapes::voxel_shape::VoxelShape;
use crate::spottedleaf::moonrise::patches::collisions::shape::cached_shape_data::CachedShapeData;

/// `CollisionUtil.COLLISION_EPSILON`.
pub const COLLISION_EPSILON: f64 = 1.0E-7;

/// `CollisionUtil.ZERO_ONE` — one shared `DoubleArrayList` `{0.0, 1.0}`.
pub static ZERO_ONE: LazyLock<Arc<DoubleArrayList>> =
    LazyLock::new(|| Arc::new(DoubleArrayList::wrap(Arc::from([0.0, 1.0]))));

/// `CollisionUtil.ZERO_ONE` as the `DoubleList` handle `ArrayVoxelShape`
/// takes.
pub fn zero_one() -> Arc<dyn DoubleList> {
    let list: Arc<dyn DoubleList> = ZERO_ONE.clone();
    list
}

/// `CollisionUtil.isEmpty(AABB)`.
pub fn is_empty(aabb: &AABB) -> bool {
    (aabb.max_x - aabb.min_x) < COLLISION_EPSILON
        || (aabb.max_y - aabb.min_y) < COLLISION_EPSILON
        || (aabb.max_z - aabb.min_z) < COLLISION_EPSILON
}

/// `CollisionUtil.isEmpty(double x6)`.
pub fn is_empty_xyz(
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
) -> bool {
    (max_x - min_x) < COLLISION_EPSILON
        || (max_y - min_y) < COLLISION_EPSILON
        || (max_z - min_z) < COLLISION_EPSILON
}

/// `CollisionUtil.getBoxForChunk(int, int)`.
pub fn get_box_for_chunk(chunk_x: i32, chunk_z: i32) -> AABB {
    let x = chunk_x.wrapping_shl(4) as f64;
    let z = chunk_z.wrapping_shl(4) as f64;
    // use a bounding box bigger than the chunk to prevent entities from entering it on move
    AABB::new(
        x - 3.0 * COLLISION_EPSILON,
        f64::NEG_INFINITY,
        z - 3.0 * COLLISION_EPSILON,
        x + (16.0 + 3.0 * COLLISION_EPSILON),
        f64::INFINITY,
        z + (16.0 + 3.0 * COLLISION_EPSILON),
    )
}

/// `CollisionUtil.voxelShapeIntersect(double x12)`.
#[allow(clippy::too_many_arguments)]
pub fn voxel_shape_intersect_xyz12(
    min_x1: f64,
    min_y1: f64,
    min_z1: f64,
    max_x1: f64,
    max_y1: f64,
    max_z1: f64,
    min_x2: f64,
    min_y2: f64,
    min_z2: f64,
    max_x2: f64,
    max_y2: f64,
    max_z2: f64,
) -> bool {
    (min_x1 - max_x2) < -COLLISION_EPSILON
        && (max_x1 - min_x2) > COLLISION_EPSILON
        && (min_y1 - max_y2) < -COLLISION_EPSILON
        && (max_y1 - min_y2) > COLLISION_EPSILON
        && (min_z1 - max_z2) < -COLLISION_EPSILON
        && (max_z1 - min_z2) > COLLISION_EPSILON
}

/// `CollisionUtil.voxelShapeIntersect(AABB, double x6)`.
pub fn voxel_shape_intersect_box_xyz(
    box_: &AABB,
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
) -> bool {
    (box_.min_x - max_x) < -COLLISION_EPSILON
        && (box_.max_x - min_x) > COLLISION_EPSILON
        && (box_.min_y - max_y) < -COLLISION_EPSILON
        && (box_.max_y - min_y) > COLLISION_EPSILON
        && (box_.min_z - max_z) < -COLLISION_EPSILON
        && (box_.max_z - min_z) > COLLISION_EPSILON
}

/// `CollisionUtil.voxelShapeIntersect(AABB, AABB)`.
pub fn voxel_shape_intersect(box1: &AABB, box2: &AABB) -> bool {
    (box1.min_x - box2.max_x) < -COLLISION_EPSILON
        && (box1.max_x - box2.min_x) > COLLISION_EPSILON
        && (box1.min_y - box2.max_y) < -COLLISION_EPSILON
        && (box1.max_y - box2.min_y) > COLLISION_EPSILON
        && (box1.min_z - box2.max_z) < -COLLISION_EPSILON
        && (box1.max_z - box2.min_z) > COLLISION_EPSILON
}

/// `CollisionUtil.collideX(AABB target, AABB source, double source_move)` —
/// assumes `!isEmpty(target) && abs(source_move) >= COLLISION_EPSILON`.
pub fn collide_x_aabb(target: &AABB, source: &AABB, source_move: f64) -> f64 {
    if (source.min_y - target.max_y) < -COLLISION_EPSILON
        && (source.max_y - target.min_y) > COLLISION_EPSILON
        && (source.min_z - target.max_z) < -COLLISION_EPSILON
        && (source.max_z - target.min_z) > COLLISION_EPSILON
    {
        if source_move >= 0.0 {
            let max_move = target.min_x - source.max_x; // < 0.0 if no strict collision
            if max_move < -COLLISION_EPSILON {
                return source_move;
            }
            return rivet_util::mth::min_f64(max_move, source_move);
        } else {
            let max_move = target.max_x - source.min_x; // > 0.0 if no strict collision
            if max_move > COLLISION_EPSILON {
                return source_move;
            }
            return rivet_util::mth::max_f64(max_move, source_move);
        }
    }
    source_move
}

/// `CollisionUtil.collideY(AABB target, AABB source, double source_move)`.
pub fn collide_y_aabb(target: &AABB, source: &AABB, source_move: f64) -> f64 {
    if (source.min_x - target.max_x) < -COLLISION_EPSILON
        && (source.max_x - target.min_x) > COLLISION_EPSILON
        && (source.min_z - target.max_z) < -COLLISION_EPSILON
        && (source.max_z - target.min_z) > COLLISION_EPSILON
    {
        if source_move >= 0.0 {
            let max_move = target.min_y - source.max_y; // < 0.0 if no strict collision
            if max_move < -COLLISION_EPSILON {
                return source_move;
            }
            return rivet_util::mth::min_f64(max_move, source_move);
        } else {
            let max_move = target.max_y - source.min_y; // > 0.0 if no strict collision
            if max_move > COLLISION_EPSILON {
                return source_move;
            }
            return rivet_util::mth::max_f64(max_move, source_move);
        }
    }
    source_move
}

/// `CollisionUtil.collideZ(AABB target, AABB source, double source_move)`.
pub fn collide_z_aabb(target: &AABB, source: &AABB, source_move: f64) -> f64 {
    if (source.min_x - target.max_x) < -COLLISION_EPSILON
        && (source.max_x - target.min_x) > COLLISION_EPSILON
        && (source.min_y - target.max_y) < -COLLISION_EPSILON
        && (source.max_y - target.min_y) > COLLISION_EPSILON
    {
        if source_move >= 0.0 {
            let max_move = target.min_z - source.max_z; // < 0.0 if no strict collision
            if max_move < -COLLISION_EPSILON {
                return source_move;
            }
            return rivet_util::mth::min_f64(max_move, source_move);
        } else {
            let max_move = target.max_z - source.min_z; // > 0.0 if no strict collision
            if max_move > COLLISION_EPSILON {
                return source_move;
            }
            return rivet_util::mth::max_f64(max_move, source_move);
        }
    }
    source_move
}

/// `CollisionUtil.findFloor(double[], double, double, int, int)` —
/// `startIndex` and `endIndex` inclusive; the indices must be in range of the
/// array (`Objects.checkFromToIndex`).
pub fn find_floor(
    values: &[f64],
    offset: f64,
    value: f64,
    mut start_index: i32,
    mut end_index: i32,
) -> i32 {
    // Objects.checkFromToIndex(startIndex, endIndex + 1, values.length)
    let to = end_index.wrapping_add(1);
    assert!(
        start_index >= 0 && start_index <= to && (to as i64) <= values.len() as i64,
        "Range [{start_index}, {to}) out of bounds for length {}",
        values.len()
    );
    loop {
        let middle = (start_index.wrapping_add(end_index) as u32 >> 1) as i32;
        let middle_val = values[middle as usize] + offset;

        if value < middle_val {
            end_index = middle - 1;
        } else {
            start_index = middle + 1;
        }
        if start_index > end_index {
            break;
        }
    }

    start_index - 1
}

/// `CollisionUtil.sliceShapeVanilla(VoxelShape, Direction.Axis, int)`.
#[allow(dead_code)] // only reached through the `DEBUG_SLICE_SHAPE` branch
fn slice_shape_vanilla(src: &VoxelShape, axis: Axis, index: i32) -> VoxelShape {
    slice_shape_class::new(src, axis, index)
}

/// `CollisionUtil.offsetList(double[], double)`.
fn offset_list(src: &Arc<[f64]>, by: f64) -> Arc<dyn DoubleList> {
    let wrap: Arc<dyn DoubleList> = Arc::new(DoubleArrayList::wrap(Arc::clone(src)));
    if by == 0.0 {
        return wrap;
    }
    Arc::new(OffsetDoubleList::new(wrap, by))
}

/// `(bitset[idx >>> 6] & (1L << idx)) != 0L` — JLS: a `long` shift masks the
/// distance with 63.
fn bit_set_at(bitset: &[u64], index: i32) -> bool {
    (bitset[(index as u32 >> 6) as usize] & 1u64.wrapping_shl(index as u32)) != 0
}

/// `CollisionUtil.sliceShapeOptimised(VoxelShape, Direction.Axis, int)`.
fn slice_shape_optimised(src: &VoxelShape, axis: Axis, index: i32) -> VoxelShape {
    // assume index in range
    let off_x = src.moonrise_offset_x();
    let off_y = src.moonrise_offset_y();
    let off_z = src.moonrise_offset_z();

    let coords_x = src.moonrise_root_coordinates_x();
    let coords_y = src.moonrise_root_coordinates_y();
    let coords_z = src.moonrise_root_coordinates_z();

    let cached_shape_data = src.moonrise_get_cached_voxel_data();

    // note: size = coords.length - 1
    let size_x = cached_shape_data.size_x;
    let size_y = cached_shape_data.size_y;
    let size_z = cached_shape_data.size_z;

    let bitset: &[u64] = &cached_shape_data.voxel_set;

    let list_x: Arc<dyn DoubleList>;
    let list_y: Arc<dyn DoubleList>;
    let list_z: Arc<dyn DoubleList>;
    let shape_sx: i32;
    let shape_ex: i32;
    let shape_sy: i32;
    let shape_ey: i32;
    let shape_sz: i32;
    let shape_ez: i32;

    match axis {
        Axis::X => {
            // validate index
            if index < 0 || index >= size_x {
                return shapes::empty();
            }

            // test if input is already "sliced"
            if coords_x.len() == 2 && (coords_x[0] + off_x) == 0.0 && (coords_x[1] + off_x) == 1.0 {
                return src.clone();
            }

            // test if result would be full box
            if coords_y.len() == 2
                && coords_z.len() == 2
                && (coords_y[0] + off_y) == 0.0
                && (coords_y[1] + off_y) == 1.0
                && (coords_z[0] + off_z) == 0.0
                && (coords_z[1] + off_z) == 1.0
            {
                // note: size_y == size_z == 1
                let bit_idx = index * (size_z * size_y);
                return if !bit_set_at(bitset, bit_idx) {
                    shapes::empty()
                } else {
                    shapes::block()
                };
            }

            list_x = zero_one();
            list_y = offset_list(coords_y, off_y);
            list_z = offset_list(coords_z, off_z);
            shape_sx = index;
            shape_ex = index + 1;
            shape_sy = 0;
            shape_ey = size_y;
            shape_sz = 0;
            shape_ez = size_z;
        }
        Axis::Y => {
            // validate index
            if index < 0 || index >= size_y {
                return shapes::empty();
            }

            // test if input is already "sliced"
            if coords_y.len() == 2 && (coords_y[0] + off_y) == 0.0 && (coords_y[1] + off_y) == 1.0 {
                return src.clone();
            }

            // test if result would be full box
            if coords_x.len() == 2
                && coords_z.len() == 2
                && (coords_x[0] + off_x) == 0.0
                && (coords_x[1] + off_x) == 1.0
                && (coords_z[0] + off_z) == 0.0
                && (coords_z[1] + off_z) == 1.0
            {
                // note: size_x == size_z == 1
                let bit_idx = index * size_z;
                return if !bit_set_at(bitset, bit_idx) {
                    shapes::empty()
                } else {
                    shapes::block()
                };
            }

            list_x = offset_list(coords_x, off_x);
            list_y = zero_one();
            list_z = offset_list(coords_z, off_z);
            shape_sx = 0;
            shape_ex = size_x;
            shape_sy = index;
            shape_ey = index + 1;
            shape_sz = 0;
            shape_ez = size_z;
        }
        Axis::Z => {
            // validate index
            if index < 0 || index >= size_z {
                return shapes::empty();
            }

            // test if input is already "sliced"
            if coords_z.len() == 2 && (coords_z[0] + off_z) == 0.0 && (coords_z[1] + off_z) == 1.0 {
                return src.clone();
            }

            // test if result would be full box
            if coords_x.len() == 2
                && coords_y.len() == 2
                && (coords_x[0] + off_x) == 0.0
                && (coords_x[1] + off_x) == 1.0
                && (coords_y[0] + off_y) == 0.0
                && (coords_y[1] + off_y) == 1.0
            {
                // note: size_x == size_y == 1
                let bit_idx = index;
                return if !bit_set_at(bitset, bit_idx) {
                    shapes::empty()
                } else {
                    shapes::block()
                };
            }

            list_x = offset_list(coords_x, off_x);
            list_y = offset_list(coords_y, off_y);
            list_z = zero_one();
            shape_sx = 0;
            shape_ex = size_x;
            shape_sy = 0;
            shape_ey = size_y;
            shape_sz = index;
            shape_ez = index + 1;
        }
    }

    let local_len_x = shape_ex - shape_sx;
    let local_len_y = shape_ey - shape_sy;
    let local_len_z = shape_ez - shape_sz;

    let mut shape = BitSetDiscreteVoxelShape::new(local_len_x, local_len_y, local_len_z);

    let bitset_mul_x = size_z * size_y;
    let idx_off = shape_sz + shape_sy * size_z + shape_sx * bitset_mul_x;
    let shape_mul_x = local_len_y * local_len_z;
    for x in 0..local_len_x {
        let mut set_x = false;
        for y in 0..local_len_y {
            let mut set_y = false;
            for z in 0..local_len_z {
                let unsliced_idx = idx_off + z + y * size_z + x * bitset_mul_x;
                if !bit_set_at(bitset, unsliced_idx) {
                    continue;
                }

                set_y = true;
                set_x = true;
                shape.z_min = shape.z_min.min(z);
                shape.z_max = shape.z_max.max(z + 1);

                shape
                    .storage
                    .set((z + y * local_len_z + x * shape_mul_x) as usize);
            }

            if set_y {
                shape.y_min = shape.y_min.min(y);
                shape.y_max = shape.y_max.max(y + 1);
            }
        }
        if set_x {
            shape.x_min = shape.x_min.min(x);
            shape.x_max = shape.x_max.max(x + 1);
        }
    }

    if DiscreteVoxelShape::is_empty(&shape) {
        shapes::empty()
    } else {
        array_voxel_shape::new(Arc::new(shape), list_x, list_y, list_z)
    }
}

/// `CollisionUtil.DEBUG_SLICE_SHAPE`.
const DEBUG_SLICE_SHAPE: bool = false;

/// `CollisionUtil.sliceShape(VoxelShape, Direction.Axis, int)`.
pub fn slice_shape(src: &VoxelShape, axis: Axis, index: i32) -> VoxelShape {
    let ret = slice_shape_optimised(src, axis, index);
    if DEBUG_SLICE_SHAPE {
        let vanilla = slice_shape_vanilla(src, axis, index);
        if !equals_shape(&ret, &vanilla) {
            // special case: SliceShape is not empty when it should be!
            if are_any_full(&**ret.shape()) || are_any_full(&**vanilla.shape()) {
                equals_shape(&ret, &vanilla);
                slice_shape_optimised(src, axis, index);
                panic!("Slice shape mismatch");
            }
        }
    }

    ret
}

/// `CollisionUtil.voxelShapeIntersectNoEmpty(VoxelShape, AABB)`.
pub fn voxel_shape_intersect_no_empty(voxel: &VoxelShape, aabb: &AABB) -> bool {
    if voxel.is_empty() {
        return false;
    }

    // note: this function assumes that for any i in coords that coord[i + 1] - coord[i] > COLLISION_EPSILON is true

    // offsets that should be applied to coords
    let off_x = voxel.moonrise_offset_x();
    let off_y = voxel.moonrise_offset_y();
    let off_z = voxel.moonrise_offset_z();

    let coords_x = voxel.moonrise_root_coordinates_x();
    let coords_y = voxel.moonrise_root_coordinates_y();
    let coords_z = voxel.moonrise_root_coordinates_z();

    let cached_shape_data = voxel.moonrise_get_cached_voxel_data();

    // note: size = coords.length - 1
    let size_x = cached_shape_data.size_x;
    let size_y = cached_shape_data.size_y;
    let size_z = cached_shape_data.size_z;

    // note: voxel bitset with set index (x, y, z) indicates that
    //       an AABB(coords_x[x], coords_y[y], coords_z[z], coords_x[x + 1], coords_y[y + 1], coords_z[z + 1])
    //       is collidable. this is the fundamental principle of operation for the voxel collision operation

    let floor_min_x = 0.max(find_floor(
        coords_x,
        off_x,
        aabb.min_x + COLLISION_EPSILON,
        0,
        size_x,
    ));
    if floor_min_x >= size_x {
        // cannot intersect
        return false;
    }

    let ceil_max_x = size_x.min(
        find_floor(
            coords_x,
            off_x,
            aabb.max_x - COLLISION_EPSILON,
            floor_min_x,
            size_x,
        ) + 1,
    );
    if floor_min_x >= ceil_max_x {
        // cannot intersect
        return false;
    }

    let floor_min_y = 0.max(find_floor(
        coords_y,
        off_y,
        aabb.min_y + COLLISION_EPSILON,
        0,
        size_y,
    ));
    if floor_min_y >= size_y {
        // cannot intersect
        return false;
    }

    let ceil_max_y = size_y.min(
        find_floor(
            coords_y,
            off_y,
            aabb.max_y - COLLISION_EPSILON,
            floor_min_y,
            size_y,
        ) + 1,
    );
    if floor_min_y >= ceil_max_y {
        // cannot intersect
        return false;
    }

    let floor_min_z = 0.max(find_floor(
        coords_z,
        off_z,
        aabb.min_z + COLLISION_EPSILON,
        0,
        size_z,
    ));
    if floor_min_z >= size_z {
        // cannot intersect
        return false;
    }

    let ceil_max_z = size_z.min(
        find_floor(
            coords_z,
            off_z,
            aabb.max_z - COLLISION_EPSILON,
            floor_min_z,
            size_z,
        ) + 1,
    );
    if floor_min_z >= ceil_max_z {
        // cannot intersect
        return false;
    }

    let bitset: &[u64] = &cached_shape_data.voxel_set;

    // check bitset to check if any shapes in range are full

    let mul_x = size_y * size_z;
    for curr_x in floor_min_x..ceil_max_x {
        for curr_y in floor_min_y..ceil_max_y {
            for curr_z in floor_min_z..ceil_max_z {
                let index = curr_z + curr_y * size_z + curr_x * mul_x;
                // note: JLS states long shift operators ANDS shift by 63
                if bit_set_at(bitset, index) {
                    return true;
                }
            }
        }
    }

    false
}

/// `CollisionUtil.collideX(VoxelShape target, AABB source, double
/// source_move)` — assumes `!target.isEmpty() && abs(source_move) >=
/// COLLISION_EPSILON`.
pub fn collide_x(target: &VoxelShape, source: &AABB, source_move: f64) -> f64 {
    if let Some(single_aabb) = target.moonrise_get_single_aabb_representation() {
        return collide_x_aabb(single_aabb, source, source_move);
    }
    // note: this function assumes that for any i in coords that coord[i + 1] - coord[i] > COLLISION_EPSILON is true

    // offsets that should be applied to coords
    let off_x = target.moonrise_offset_x();
    let off_y = target.moonrise_offset_y();
    let off_z = target.moonrise_offset_z();

    let coords_x = target.moonrise_root_coordinates_x();
    let coords_y = target.moonrise_root_coordinates_y();
    let coords_z = target.moonrise_root_coordinates_z();

    let cached_shape_data = target.moonrise_get_cached_voxel_data();

    // note: size = coords.length - 1
    let size_x = cached_shape_data.size_x;
    let size_y = cached_shape_data.size_y;
    let size_z = cached_shape_data.size_z;

    // note: for intersection, one we find the floor of the min we can use that as the start index
    //       for the next check as source max >= source min
    // note: we can fast check intersection on the two other axis by seeing if the min index is >= size,
    //       as this implies that coords[coords.length - 1] < source min
    //       we can also fast check by seeing if max index is < 0, as this implies that coords[0] > source max

    let floor_min_y = 0.max(find_floor(
        coords_y,
        off_y,
        source.min_y + COLLISION_EPSILON,
        0,
        size_y,
    ));
    if floor_min_y >= size_y {
        // cannot intersect
        return source_move;
    }

    let ceil_max_y = size_y.min(
        find_floor(
            coords_y,
            off_y,
            source.max_y - COLLISION_EPSILON,
            floor_min_y,
            size_y,
        ) + 1,
    );
    if floor_min_y >= ceil_max_y {
        // cannot intersect
        return source_move;
    }

    let floor_min_z = 0.max(find_floor(
        coords_z,
        off_z,
        source.min_z + COLLISION_EPSILON,
        0,
        size_z,
    ));
    if floor_min_z >= size_z {
        // cannot intersect
        return source_move;
    }

    let ceil_max_z = size_z.min(
        find_floor(
            coords_z,
            off_z,
            source.max_z - COLLISION_EPSILON,
            floor_min_z,
            size_z,
        ) + 1,
    );
    if floor_min_z >= ceil_max_z {
        // cannot intersect
        return source_move;
    }

    // index = z + y*size_z + x*(size_z*size_y)

    let bitset: &[u64] = &cached_shape_data.voxel_set;

    if source_move > 0.0 {
        let source_max = source.max_x;
        let ceil_max_x = find_floor(coords_x, off_x, source_max - COLLISION_EPSILON, 0, size_x) + 1; // add one, we are not interested in (coords[i] + COLLISION_EPSILON) < max

        // note: only the order of the first loop matters

        // note: we cannot collide with the face at index size on the collision axis for forward movement

        let mul_x = size_y * size_z;
        for curr_x in ceil_max_x..size_x {
            let mut max_dist = (coords_x[curr_x as usize] + off_x) - source_max;
            if max_dist >= source_move {
                // if we reach here, then we will never have a case where
                // coords[curr + n] - source_max < source_move, as coords[curr + n] < coords[curr + n + 1]
                // thus, we can return immediately

                // this optimization is important since this loop is bounded by size, and _not_ by
                // a calculated max index based off of source_move - so it would be possible to check
                // the whole intersected shape for collisions when we didn't need to!
                return source_move;
            }
            if max_dist >= -COLLISION_EPSILON {
                // only push out by up to COLLISION_EPSILON
                max_dist = rivet_util::mth::min_f64(max_dist, source_move);
            }
            for curr_y in floor_min_y..ceil_max_y {
                for curr_z in floor_min_z..ceil_max_z {
                    let index = curr_z + curr_y * size_z + curr_x * mul_x;
                    // note: JLS states long shift operators ANDS shift by 63
                    if bit_set_at(bitset, index) {
                        return max_dist;
                    }
                }
            }
        }

        source_move
    } else {
        let source_min = source.min_x;
        let floor_min_x = find_floor(coords_x, off_x, source_min + COLLISION_EPSILON, 0, size_x);

        // note: only the order of the first loop matters

        // note: we cannot collide with the face at index 0 on the collision axis for backwards movement

        // note: we offset the collision axis by - 1 for the voxel bitset index, but use + 1 for the
        //       coordinate index as the voxelset stores whether the shape is solid for [index, index + 1]
        //       thus, we need to use the voxel index i-1 if we want to check that the face at index i is solid
        let mul_x = size_y * size_z;
        let mut curr_x = floor_min_x - 1;
        while curr_x >= 0 {
            let mut max_dist = (coords_x[(curr_x + 1) as usize] + off_x) - source_min;
            if max_dist <= source_move {
                // if we reach here, then we will never have a case where
                // coords[curr + n] - source_max > source_move, as coords[curr + n] > coords[curr + n - 1]
                // thus, we can return immediately

                // this optimization is important since this loop is possibly bounded by size, and _not_ by
                // a calculated max index based off of source_move - so it would be possible to check
                // the whole intersected shape for collisions when we didn't need to!
                return source_move;
            }
            if max_dist <= COLLISION_EPSILON {
                // only push out by up to COLLISION_EPSILON
                max_dist = rivet_util::mth::max_f64(max_dist, source_move);
            }
            for curr_y in floor_min_y..ceil_max_y {
                for curr_z in floor_min_z..ceil_max_z {
                    let index = curr_z + curr_y * size_z + curr_x * mul_x;
                    // note: JLS states long shift operators ANDS shift by 63
                    if bit_set_at(bitset, index) {
                        return max_dist;
                    }
                }
            }
            curr_x -= 1;
        }

        source_move
    }
}

/// `CollisionUtil.collideY(VoxelShape target, AABB source, double
/// source_move)`.
pub fn collide_y(target: &VoxelShape, source: &AABB, source_move: f64) -> f64 {
    if let Some(single_aabb) = target.moonrise_get_single_aabb_representation() {
        return collide_y_aabb(single_aabb, source, source_move);
    }
    // note: this function assumes that for any i in coords that coord[i + 1] - coord[i] > COLLISION_EPSILON is true

    // offsets that should be applied to coords
    let off_x = target.moonrise_offset_x();
    let off_y = target.moonrise_offset_y();
    let off_z = target.moonrise_offset_z();

    let coords_x = target.moonrise_root_coordinates_x();
    let coords_y = target.moonrise_root_coordinates_y();
    let coords_z = target.moonrise_root_coordinates_z();

    let cached_shape_data = target.moonrise_get_cached_voxel_data();

    // note: size = coords.length - 1
    let size_x = cached_shape_data.size_x;
    let size_y = cached_shape_data.size_y;
    let size_z = cached_shape_data.size_z;

    let floor_min_x = 0.max(find_floor(
        coords_x,
        off_x,
        source.min_x + COLLISION_EPSILON,
        0,
        size_x,
    ));
    if floor_min_x >= size_x {
        // cannot intersect
        return source_move;
    }

    let ceil_max_x = size_x.min(
        find_floor(
            coords_x,
            off_x,
            source.max_x - COLLISION_EPSILON,
            floor_min_x,
            size_x,
        ) + 1,
    );
    if floor_min_x >= ceil_max_x {
        // cannot intersect
        return source_move;
    }

    let floor_min_z = 0.max(find_floor(
        coords_z,
        off_z,
        source.min_z + COLLISION_EPSILON,
        0,
        size_z,
    ));
    if floor_min_z >= size_z {
        // cannot intersect
        return source_move;
    }

    let ceil_max_z = size_z.min(
        find_floor(
            coords_z,
            off_z,
            source.max_z - COLLISION_EPSILON,
            floor_min_z,
            size_z,
        ) + 1,
    );
    if floor_min_z >= ceil_max_z {
        // cannot intersect
        return source_move;
    }

    // index = z + y*size_z + x*(size_z*size_y)

    let bitset: &[u64] = &cached_shape_data.voxel_set;

    if source_move > 0.0 {
        let source_max = source.max_y;
        let ceil_max_y = find_floor(coords_y, off_y, source_max - COLLISION_EPSILON, 0, size_y) + 1; // add one, we are not interested in (coords[i] + COLLISION_EPSILON) < max

        // note: only the order of the first loop matters

        // note: we cannot collide with the face at index size on the collision axis for forward movement

        let mul_x = size_y * size_z;
        for curr_y in ceil_max_y..size_y {
            let mut max_dist = (coords_y[curr_y as usize] + off_y) - source_max;
            if max_dist >= source_move {
                return source_move;
            }
            if max_dist >= -COLLISION_EPSILON {
                // only push out by up to COLLISION_EPSILON
                max_dist = rivet_util::mth::min_f64(max_dist, source_move);
            }
            for curr_x in floor_min_x..ceil_max_x {
                for curr_z in floor_min_z..ceil_max_z {
                    let index = curr_z + curr_y * size_z + curr_x * mul_x;
                    // note: JLS states long shift operators ANDS shift by 63
                    if bit_set_at(bitset, index) {
                        return max_dist;
                    }
                }
            }
        }

        source_move
    } else {
        let source_min = source.min_y;
        let floor_min_y = find_floor(coords_y, off_y, source_min + COLLISION_EPSILON, 0, size_y);

        // note: only the order of the first loop matters

        // note: we cannot collide with the face at index 0 on the collision axis for backwards movement

        let mul_x = size_y * size_z;
        let mut curr_y = floor_min_y - 1;
        while curr_y >= 0 {
            let mut max_dist = (coords_y[(curr_y + 1) as usize] + off_y) - source_min;
            if max_dist <= source_move {
                return source_move;
            }
            if max_dist <= COLLISION_EPSILON {
                // only push out by up to COLLISION_EPSILON
                max_dist = rivet_util::mth::max_f64(max_dist, source_move);
            }
            for curr_x in floor_min_x..ceil_max_x {
                for curr_z in floor_min_z..ceil_max_z {
                    let index = curr_z + curr_y * size_z + curr_x * mul_x;
                    // note: JLS states long shift operators ANDS shift by 63
                    if bit_set_at(bitset, index) {
                        return max_dist;
                    }
                }
            }
            curr_y -= 1;
        }

        source_move
    }
}

/// `CollisionUtil.collideZ(VoxelShape target, AABB source, double
/// source_move)`.
pub fn collide_z(target: &VoxelShape, source: &AABB, source_move: f64) -> f64 {
    if let Some(single_aabb) = target.moonrise_get_single_aabb_representation() {
        return collide_z_aabb(single_aabb, source, source_move);
    }
    // note: this function assumes that for any i in coords that coord[i + 1] - coord[i] > COLLISION_EPSILON is true

    // offsets that should be applied to coords
    let off_x = target.moonrise_offset_x();
    let off_y = target.moonrise_offset_y();
    let off_z = target.moonrise_offset_z();

    let coords_x = target.moonrise_root_coordinates_x();
    let coords_y = target.moonrise_root_coordinates_y();
    let coords_z = target.moonrise_root_coordinates_z();

    let cached_shape_data = target.moonrise_get_cached_voxel_data();

    // note: size = coords.length - 1
    let size_x = cached_shape_data.size_x;
    let size_y = cached_shape_data.size_y;
    let size_z = cached_shape_data.size_z;

    let floor_min_x = 0.max(find_floor(
        coords_x,
        off_x,
        source.min_x + COLLISION_EPSILON,
        0,
        size_x,
    ));
    if floor_min_x >= size_x {
        // cannot intersect
        return source_move;
    }

    let ceil_max_x = size_x.min(
        find_floor(
            coords_x,
            off_x,
            source.max_x - COLLISION_EPSILON,
            floor_min_x,
            size_x,
        ) + 1,
    );
    if floor_min_x >= ceil_max_x {
        // cannot intersect
        return source_move;
    }

    let floor_min_y = 0.max(find_floor(
        coords_y,
        off_y,
        source.min_y + COLLISION_EPSILON,
        0,
        size_y,
    ));
    if floor_min_y >= size_y {
        // cannot intersect
        return source_move;
    }

    let ceil_max_y = size_y.min(
        find_floor(
            coords_y,
            off_y,
            source.max_y - COLLISION_EPSILON,
            floor_min_y,
            size_y,
        ) + 1,
    );
    if floor_min_y >= ceil_max_y {
        // cannot intersect
        return source_move;
    }

    // index = z + y*size_z + x*(size_z*size_y)

    let bitset: &[u64] = &cached_shape_data.voxel_set;

    if source_move > 0.0 {
        let source_max = source.max_z;
        let ceil_max_z = find_floor(coords_z, off_z, source_max - COLLISION_EPSILON, 0, size_z) + 1; // add one, we are not interested in (coords[i] + COLLISION_EPSILON) < max

        // note: only the order of the first loop matters

        // note: we cannot collide with the face at index size on the collision axis for forward movement

        let mul_x = size_y * size_z;
        for curr_z in ceil_max_z..size_z {
            let mut max_dist = (coords_z[curr_z as usize] + off_z) - source_max;
            if max_dist >= source_move {
                return source_move;
            }
            if max_dist >= -COLLISION_EPSILON {
                // only push out by up to COLLISION_EPSILON
                max_dist = rivet_util::mth::min_f64(max_dist, source_move);
            }
            for curr_x in floor_min_x..ceil_max_x {
                for curr_y in floor_min_y..ceil_max_y {
                    let index = curr_z + curr_y * size_z + curr_x * mul_x;
                    // note: JLS states long shift operators ANDS shift by 63
                    if bit_set_at(bitset, index) {
                        return max_dist;
                    }
                }
            }
        }

        source_move
    } else {
        let source_min = source.min_z;
        let floor_min_z = find_floor(coords_z, off_z, source_min + COLLISION_EPSILON, 0, size_z);

        // note: only the order of the first loop matters

        // note: we cannot collide with the face at index 0 on the collision axis for backwards movement

        let mul_x = size_y * size_z;
        let mut curr_z = floor_min_z - 1;
        while curr_z >= 0 {
            let mut max_dist = (coords_z[(curr_z + 1) as usize] + off_z) - source_min;
            if max_dist <= source_move {
                return source_move;
            }
            if max_dist <= COLLISION_EPSILON {
                // only push out by up to COLLISION_EPSILON
                max_dist = rivet_util::mth::max_f64(max_dist, source_move);
            }
            for curr_x in floor_min_x..ceil_max_x {
                for curr_y in floor_min_y..ceil_max_y {
                    let index = curr_z + curr_y * size_z + curr_x * mul_x;
                    // note: JLS states long shift operators ANDS shift by 63
                    if bit_set_at(bitset, index) {
                        return max_dist;
                    }
                }
            }
            curr_z -= 1;
        }

        source_move
    }
}

/// `CollisionUtil.strictlyContains(VoxelShape, Vec3)`.
pub fn strictly_contains_vec3(voxel: &VoxelShape, point: &Vec3) -> bool {
    strictly_contains(voxel, point.x, point.y, point.z)
}

/// `CollisionUtil.strictlyContains(VoxelShape, double, double, double)`.
pub fn strictly_contains(voxel: &VoxelShape, x: f64, y: f64, z: f64) -> bool {
    if let Some(single_aabb) = voxel.moonrise_get_single_aabb_representation() {
        return single_aabb.contains_xyz(x, y, z);
    }

    if voxel.is_empty() {
        return false;
    }

    // offsets that should be applied to coords
    let off_x = voxel.moonrise_offset_x();
    let off_y = voxel.moonrise_offset_y();
    let off_z = voxel.moonrise_offset_z();

    let coords_x = voxel.moonrise_root_coordinates_x();
    let coords_y = voxel.moonrise_root_coordinates_y();
    let coords_z = voxel.moonrise_root_coordinates_z();

    let cached_shape_data = voxel.moonrise_get_cached_voxel_data();

    // note: size = coords.length - 1
    let size_x = cached_shape_data.size_x;
    let size_y = cached_shape_data.size_y;
    let size_z = cached_shape_data.size_z;

    // note: should mirror AABB#contains, which is that for any point X that X >= min and X < max.
    //       specifically, it cannot collide on the max bounds of the shape

    let index_x = find_floor(coords_x, off_x, x, 0, size_x);
    if index_x < 0 || index_x >= size_x {
        return false;
    }

    let index_y = find_floor(coords_y, off_y, y, 0, size_y);
    if index_y < 0 || index_y >= size_y {
        return false;
    }

    let index_z = find_floor(coords_z, off_z, z, 0, size_z);
    if index_z < 0 || index_z >= size_z {
        return false;
    }

    // index = z + y*size_z + x*(size_z*size_y)

    let index = index_z + index_y * size_z + index_x * (size_z * size_y);

    let bitset: &[u64] = &cached_shape_data.voxel_set;

    bit_set_at(bitset, index)
}

/// `CollisionUtil.makeBitset(boolean ft, boolean tf, boolean tt)`.
fn make_bitset(ft: bool, tf: bool, tt: bool) -> i32 {
    // idx ff -> 0
    // idx ft -> 1
    // idx tf -> 2
    // idx tt -> 3
    ((ft as i32) << 1) | ((tf as i32) << 2) | ((tt as i32) << 3)
}

/// `(int)((voxels[(idx = ...) >>> 6] >>> idx) & 1L)`, or 0 when any of the
/// three indices is negative (`-1` marks an index past the shape).
fn voxel_full(voxels: &[u64], x: i32, y: i32, z: i32, mul1: i32, mul2: i32) -> i32 {
    if (x | y | z) < 0 {
        0
    } else {
        let idx = z + y * mul1 + x * mul2;
        ((voxels[(idx as u32 >> 6) as usize].wrapping_shr(idx as u32)) & 1) as i32
    }
}

/// `CollisionUtil.merge(...)` — `null` when the merged shape is empty.
fn merge(
    shape_data_first: &CachedShapeData,
    shape_data_second: &CachedShapeData,
    merged_x: &MergedVoxelCoordinateList,
    merged_y: &MergedVoxelCoordinateList,
    merged_z: &MergedVoxelCoordinateList,
    boolean_op: i32,
) -> Option<BitSetDiscreteVoxelShape> {
    let size_x = merged_x.voxels;
    let size_y = merged_y.voxels;
    let size_z = merged_z.voxels;

    let s1_voxels: &[u64] = &shape_data_first.voxel_set;
    let s2_voxels: &[u64] = &shape_data_second.voxel_set;

    let s1_mul1 = shape_data_first.size_z;
    let s1_mul2 = s1_mul1 * shape_data_first.size_y;

    let s2_mul1 = shape_data_second.size_z;
    let s2_mul2 = s2_mul1 * shape_data_second.size_y;

    // note: indices may contain -1, but nothing > size
    let mut ret = BitSetDiscreteVoxelShape::new(size_x, size_y, size_z);

    let mut empty = true;

    let mut merged_idx: usize = 0;
    for idx_x in 0..size_x {
        let s1x = merged_x.first_indices[idx_x as usize];
        let s2x = merged_x.second_indices[idx_x as usize];
        let mut set_x = false;
        for idx_y in 0..size_y {
            let s1y = merged_y.first_indices[idx_y as usize];
            let s2y = merged_y.second_indices[idx_y as usize];
            let mut set_y = false;
            for idx_z in 0..size_z {
                let s1z = merged_z.first_indices[idx_z as usize];
                let s2z = merged_z.second_indices[idx_z as usize];

                let is_s1_full = voxel_full(s1_voxels, s1x, s1y, s1z, s1_mul1, s1_mul2);
                let is_s2_full = voxel_full(s2_voxels, s2x, s2y, s2z, s2_mul1, s2_mul2);

                // idx ff -> 0
                // idx ft -> 1
                // idx tf -> 2
                // idx tt -> 3

                let res = (boolean_op & (1 << (is_s2_full | (is_s1_full << 1)))) != 0;
                set_y |= res;
                set_x |= res;

                if res {
                    empty = false;
                    // inline and optimize fill operation
                    ret.z_min = ret.z_min.min(idx_z);
                    ret.z_max = ret.z_max.max(idx_z + 1);
                    ret.storage.set(merged_idx);
                }

                merged_idx += 1;
            }
            if set_y {
                ret.y_min = ret.y_min.min(idx_y);
                ret.y_max = ret.y_max.max(idx_y + 1);
            }
        }
        if set_x {
            ret.x_min = ret.x_min.min(idx_x);
            ret.x_max = ret.x_max.max(idx_x + 1);
        }
    }

    if empty { None } else { Some(ret) }
}

/// `CollisionUtil.isMergeEmpty(...)`.
fn is_merge_empty(
    shape_data_first: &CachedShapeData,
    shape_data_second: &CachedShapeData,
    merged_x: &MergedVoxelCoordinateList,
    merged_y: &MergedVoxelCoordinateList,
    merged_z: &MergedVoxelCoordinateList,
    boolean_op: i32,
) -> bool {
    let size_x = merged_x.voxels;
    let size_y = merged_y.voxels;
    let size_z = merged_z.voxels;

    let s1_voxels: &[u64] = &shape_data_first.voxel_set;
    let s2_voxels: &[u64] = &shape_data_second.voxel_set;

    let s1_mul1 = shape_data_first.size_z;
    let s1_mul2 = s1_mul1 * shape_data_first.size_y;

    let s2_mul1 = shape_data_second.size_z;
    let s2_mul2 = s2_mul1 * shape_data_second.size_y;

    // note: indices may contain -1, but nothing > size
    for idx_x in 0..size_x {
        let s1x = merged_x.first_indices[idx_x as usize];
        let s2x = merged_x.second_indices[idx_x as usize];
        for idx_y in 0..size_y {
            let s1y = merged_y.first_indices[idx_y as usize];
            let s2y = merged_y.second_indices[idx_y as usize];
            for idx_z in 0..size_z {
                let s1z = merged_z.first_indices[idx_z as usize];
                let s2z = merged_z.second_indices[idx_z as usize];

                let is_s1_full = voxel_full(s1_voxels, s1x, s1y, s1z, s1_mul1, s1_mul2);
                let is_s2_full = voxel_full(s2_voxels, s2x, s2y, s2z, s2_mul1, s2_mul2);

                // idx ff -> 0
                // idx ft -> 1
                // idx tf -> 2
                // idx tt -> 3

                let res = (boolean_op & (1 << (is_s2_full | (is_s1_full << 1)))) != 0;

                if res {
                    return false;
                }
            }
        }
    }

    true
}

/// `CollisionUtil.joinOptimized(VoxelShape, VoxelShape, BooleanOp)`.
pub fn join_optimized(first: &VoxelShape, second: &VoxelShape, operator: BooleanOp) -> VoxelShape {
    join_unoptimized(first, second, operator).optimize()
}

/// `CollisionUtil.joinUnoptimized(VoxelShape, VoxelShape, BooleanOp)` —
/// `UnsupportedOperationException` (a panic here) for an operator with
/// `(false, false) -> true`.
pub fn join_unoptimized(
    first: &VoxelShape,
    second: &VoxelShape,
    operator: BooleanOp,
) -> VoxelShape {
    let ff = operator.apply(false, false);
    if ff {
        // technically, should be an infinite box but that's clearly an error
        panic!("Ambiguous operator: (false, false) -> true");
    }

    let tt = operator.apply(true, true);

    if first.ptr_eq(second) {
        return if tt { first.clone() } else { shapes::empty() };
    }

    let ft = operator.apply(false, true);
    let tf = operator.apply(true, false);

    if first.is_empty() {
        return if ft { second.clone() } else { shapes::empty() };
    }
    if second.is_empty() {
        return if tf { first.clone() } else { shapes::empty() };
    }

    if !tt {
        // try to check for no intersection, since tt = false
        let aabb_f = first.moonrise_get_single_aabb_representation();
        let aabb_s = second.moonrise_get_single_aabb_representation();

        let intersect;

        let has_aabb_f = aabb_f.is_some();
        let has_aabb_s = aabb_s.is_some();
        if has_aabb_f | has_aabb_s {
            if has_aabb_f & has_aabb_s {
                intersect = voxel_shape_intersect(aabb_f.unwrap(), aabb_s.unwrap());
            } else if has_aabb_f {
                intersect = voxel_shape_intersect_no_empty(second, aabb_f.unwrap());
            } else {
                intersect = voxel_shape_intersect_no_empty(first, aabb_s.unwrap());
            }
        } else {
            // expect cached bounds
            intersect = voxel_shape_intersect(&first.bounds(), &second.bounds());
        }

        if !intersect {
            if !tf & !ft {
                return shapes::empty();
            }
            if !tf | !ft {
                return if tf { first.clone() } else { second.clone() };
            }
        }
    }

    let Some(merged_x) = MergedVoxelCoordinateList::merge(
        first.moonrise_root_coordinates_x(),
        first.moonrise_offset_x(),
        second.moonrise_root_coordinates_x(),
        second.moonrise_offset_x(),
        ft,
        tf,
    ) else {
        return shapes::empty();
    };
    let Some(merged_y) = MergedVoxelCoordinateList::merge(
        first.moonrise_root_coordinates_y(),
        first.moonrise_offset_y(),
        second.moonrise_root_coordinates_y(),
        second.moonrise_offset_y(),
        ft,
        tf,
    ) else {
        return shapes::empty();
    };
    let Some(merged_z) = MergedVoxelCoordinateList::merge(
        first.moonrise_root_coordinates_z(),
        first.moonrise_offset_z(),
        second.moonrise_root_coordinates_z(),
        second.moonrise_offset_z(),
        ft,
        tf,
    ) else {
        return shapes::empty();
    };

    let shape_data_first = first.moonrise_get_cached_voxel_data();
    let shape_data_second = second.moonrise_get_cached_voxel_data();

    let Some(merged_shape) = merge(
        shape_data_first,
        shape_data_second,
        &merged_x,
        &merged_y,
        &merged_z,
        make_bitset(ft, tf, tt),
    ) else {
        return shapes::empty();
    };

    array_voxel_shape::new(
        Arc::new(merged_shape),
        merged_x.wrap_coords(),
        merged_y.wrap_coords(),
        merged_z.wrap_coords(),
    )
}

/// `CollisionUtil.isJoinNonEmpty(VoxelShape, VoxelShape, BooleanOp)`.
pub fn is_join_non_empty(first: &VoxelShape, second: &VoxelShape, operator: BooleanOp) -> bool {
    let ff = operator.apply(false, false);
    if ff {
        // technically, should be an infinite box but that's clearly an error
        panic!("Ambiguous operator: (false, false) -> true");
    }

    let first_empty = first.is_empty();
    let second_empty = second.is_empty();
    if first_empty | second_empty {
        return operator.apply(!first_empty, !second_empty);
    }

    let tt = operator.apply(true, true);

    if first.ptr_eq(second) {
        return tt;
    }

    let ft = operator.apply(false, true);
    let tf = operator.apply(true, false);

    // try to check intersection
    let aabb_f = first.moonrise_get_single_aabb_representation();
    let aabb_s = second.moonrise_get_single_aabb_representation();

    let intersect;

    let has_aabb_f = aabb_f.is_some();
    let has_aabb_s = aabb_s.is_some();
    if has_aabb_f | has_aabb_s {
        if has_aabb_f & has_aabb_s {
            intersect = voxel_shape_intersect(aabb_f.unwrap(), aabb_s.unwrap());
        } else if has_aabb_f {
            intersect = voxel_shape_intersect_no_empty(second, aabb_f.unwrap());
        } else {
            // hasAABBS -> true
            intersect = voxel_shape_intersect_no_empty(first, aabb_s.unwrap());
        }

        if !intersect {
            // is only non-empty if we take from first or second, as there is no overlap AND both shapes are non-empty
            return tf | ft;
        } else if tt {
            // intersect = true && tt = true -> non-empty merged shape
            return true;
        }
    } else {
        // expect cached bounds
        intersect = voxel_shape_intersect(&first.bounds(), &second.bounds());
        if !intersect {
            // is only non-empty if we take from first or second, as there is no intersection
            return tf | ft;
        }
    }

    let Some(merged_x) = MergedVoxelCoordinateList::merge(
        first.moonrise_root_coordinates_x(),
        first.moonrise_offset_x(),
        second.moonrise_root_coordinates_x(),
        second.moonrise_offset_x(),
        ft,
        tf,
    ) else {
        return false;
    };
    let Some(merged_y) = MergedVoxelCoordinateList::merge(
        first.moonrise_root_coordinates_y(),
        first.moonrise_offset_y(),
        second.moonrise_root_coordinates_y(),
        second.moonrise_offset_y(),
        ft,
        tf,
    ) else {
        return false;
    };
    let Some(merged_z) = MergedVoxelCoordinateList::merge(
        first.moonrise_root_coordinates_z(),
        first.moonrise_offset_z(),
        second.moonrise_root_coordinates_z(),
        second.moonrise_offset_z(),
        ft,
        tf,
    ) else {
        return false;
    };

    let shape_data_first = first.moonrise_get_cached_voxel_data();
    let shape_data_second = second.moonrise_get_cached_voxel_data();

    !is_merge_empty(
        shape_data_first,
        shape_data_second,
        &merged_x,
        &merged_y,
        &merged_z,
        make_bitset(ft, tf, tt),
    )
}

/// `MergedVoxelCoordinateList.SIMPLE_INDICES_CACHE`.
static SIMPLE_INDICES_CACHE: LazyLock<Vec<Arc<[i32]>>> = LazyLock::new(|| {
    (0..64)
        .map(MergedVoxelCoordinateList::get_indices)
        .collect()
});

/// `CollisionUtil.MergedVoxelCoordinateList`.
struct MergedVoxelCoordinateList {
    // indices above voxel size are always set to -1
    coordinates: Arc<[f64]>,
    coordinate_offset: f64,
    first_indices: Arc<[i32]>,
    second_indices: Arc<[i32]>,
    voxels: i32,
}

impl MergedVoxelCoordinateList {
    fn get_indices(length: i32) -> Arc<[i32]> {
        let mut ret = vec![0; length as usize];

        for i in 1..length {
            ret[i as usize] = i;
        }

        Arc::from(ret)
    }

    fn wrap_coords(&self) -> Arc<dyn DoubleList> {
        let wrapped: Arc<dyn DoubleList> = Arc::new(DoubleArrayList::wrap_with_length(
            Arc::clone(&self.coordinates),
            (self.voxels + 1) as usize,
        ));
        if self.coordinate_offset == 0.0 {
            return wrapped;
        }
        Arc::new(OffsetDoubleList::new(wrapped, self.coordinate_offset))
    }

    // assume coordinates.length > 1
    fn get_for_single(coordinates: &Arc<[f64]>, offset: f64) -> MergedVoxelCoordinateList {
        let voxels = coordinates.len() as i32 - 1;
        let indices = if (voxels as usize) < SIMPLE_INDICES_CACHE.len() {
            Arc::clone(&SIMPLE_INDICES_CACHE[voxels as usize])
        } else {
            MergedVoxelCoordinateList::get_indices(voxels)
        };

        MergedVoxelCoordinateList {
            coordinates: Arc::clone(coordinates),
            coordinate_offset: offset,
            first_indices: Arc::clone(&indices),
            second_indices: indices,
            voxels,
        }
    }

    // assume coordinates.length > 1
    fn merge(
        first_coordinates: &Arc<[f64]>,
        first_offset: f64,
        second_coordinates: &Arc<[f64]>,
        second_offset: f64,
        ft: bool,
        tf: bool,
    ) -> Option<MergedVoxelCoordinateList> {
        if Arc::ptr_eq(first_coordinates, second_coordinates) && first_offset == second_offset {
            return Some(MergedVoxelCoordinateList::get_for_single(
                first_coordinates,
                first_offset,
            ));
        }

        let first_count = first_coordinates.len() as i32;
        let second_count = second_coordinates.len() as i32;

        let voxels_first = first_count - 1;
        let voxels_second = second_count - 1;

        let max_count = (first_count + second_count) as usize;

        let mut coordinates = vec![0.0f64; max_count];
        let mut first_indices = vec![0i32; max_count];
        let mut second_indices = vec![0i32; max_count];

        let not_tf = !tf;
        let not_ft = !ft;

        let mut first_index: i32 = 0;
        let mut second_index: i32 = 0;
        let mut result_size: i32 = 0;

        // note: operations on NaN are false
        let mut last = f64::NAN;

        loop {
            let none_left_first = first_index >= first_count;
            let none_left_second = second_index >= second_count;

            if (none_left_first & none_left_second)
                | (none_left_second & not_tf)
                | (none_left_first & not_ft)
            {
                break;
            }

            let first_zero = first_index == 0;
            let second_zero = second_index == 0;

            let select: f64;

            if none_left_first {
                // noneLeftSecond -> false
                // notFT -> false
                select = second_coordinates[second_index as usize] + second_offset;
                second_index += 1;
            } else if none_left_second {
                // noneLeftFirst -> false
                // notTF -> false
                select = first_coordinates[first_index as usize] + first_offset;
                first_index += 1;
            } else {
                // noneLeftFirst | noneLeftSecond -> false
                // notTF -> ??
                // notFT -> ??
                let break_first = not_tf & second_zero;
                let break_second = not_ft & first_zero;

                let first = first_coordinates[first_index as usize] + first_offset;
                let second = second_coordinates[second_index as usize] + second_offset;
                let use_first = first < (second + COLLISION_EPSILON);
                let cont = (use_first & break_first) | (!use_first & break_second);

                select = if use_first { first } else { second };
                first_index += use_first as i32;
                second_index += 1 ^ (use_first as i32);

                if cont {
                    continue;
                }
            }

            let mut prev_first = first_index - 1;
            prev_first = if prev_first >= voxels_first {
                -1
            } else {
                prev_first
            };
            let mut prev_second = second_index - 1;
            prev_second = if prev_second >= voxels_second {
                -1
            } else {
                prev_second
            };

            if last >= (select - COLLISION_EPSILON) {
                // note: any operations on NaN is false
                first_indices[(result_size - 1) as usize] = prev_first;
                second_indices[(result_size - 1) as usize] = prev_second;
            } else {
                first_indices[result_size as usize] = prev_first;
                second_indices[result_size as usize] = prev_second;
                coordinates[result_size as usize] = select;

                result_size += 1;
                last = select;
            }
        }

        if result_size <= 1 {
            None
        } else {
            Some(MergedVoxelCoordinateList {
                coordinates: Arc::from(coordinates),
                coordinate_offset: 0.0,
                first_indices: Arc::from(first_indices),
                second_indices: Arc::from(second_indices),
                voxels: result_size - 1,
            })
        }
    }
}

/// `CollisionUtil.equals(DiscreteVoxelShape, DiscreteVoxelShape)`.
pub fn equals_discrete(shape1: &dyn DiscreteVoxelShape, shape2: &dyn DiscreteVoxelShape) -> bool {
    let cached_shape_data1 = shape1.moonrise_get_or_create_cached_shape_data();
    let cached_shape_data2 = shape2.moonrise_get_or_create_cached_shape_data();

    let is_empty1 = cached_shape_data1.is_empty;
    let is_empty2 = cached_shape_data2.is_empty;

    if is_empty1 & is_empty2 {
        return true;
    } else if is_empty1 ^ is_empty2 {
        return false;
    } // else: isEmpty1 = isEmpty2 = false

    if cached_shape_data1.has_single_aabb != cached_shape_data2.has_single_aabb {
        return false;
    }

    if cached_shape_data1.size_x != cached_shape_data2.size_x {
        return false;
    }
    if cached_shape_data1.size_y != cached_shape_data2.size_y {
        return false;
    }
    if cached_shape_data1.size_z != cached_shape_data2.size_z {
        return false;
    }

    cached_shape_data1.voxel_set == cached_shape_data2.voxel_set
}

/// `CollisionUtil.equals(VoxelShape, VoxelShape)` — useful only for testing.
pub fn equals_shape(shape1: &VoxelShape, shape2: &VoxelShape) -> bool {
    if shape1.is_empty() & shape2.is_empty() {
        return true;
    } else if shape1.is_empty() ^ shape2.is_empty() {
        return false;
    }

    if !equals_discrete(&**shape1.shape(), &**shape2.shape()) {
        return false;
    }

    double_list_equals(&*shape1.get_coords(Axis::X), &*shape2.get_coords(Axis::X))
        && double_list_equals(&*shape1.get_coords(Axis::Y), &*shape2.get_coords(Axis::Y))
        && double_list_equals(&*shape1.get_coords(Axis::Z), &*shape2.get_coords(Axis::Z))
}

/// `CollisionUtil.areAnyFull(DiscreteVoxelShape)`.
pub fn are_any_full(shape: &dyn DiscreteVoxelShape) -> bool {
    if shape.is_empty() {
        return false;
    }

    let size_x = shape.get_x_size();
    let size_y = shape.get_y_size();
    let size_z = shape.get_z_size();

    for x in 0..size_x {
        for y in 0..size_y {
            for z in 0..size_z {
                if shape.is_full(x, y, z) {
                    return true;
                }
            }
        }
    }

    false
}

/// `CollisionUtil.shapeMismatch(DiscreteVoxelShape, DiscreteVoxelShape)` —
/// `null` is `None`.
pub fn shape_mismatch(
    shape1: &dyn DiscreteVoxelShape,
    shape2: &dyn DiscreteVoxelShape,
) -> Option<String> {
    let cached_shape_data1 = shape1.moonrise_get_or_create_cached_shape_data();
    let cached_shape_data2 = shape2.moonrise_get_or_create_cached_shape_data();

    let is_empty1 = cached_shape_data1.is_empty;
    let is_empty2 = cached_shape_data2.is_empty;

    // Java's `isEmpty1 & isEmpty2` and `isEmpty1 ^ isEmpty2` branches both return null.
    if is_empty1 || is_empty2 {
        return None;
    } // else: isEmpty1 = isEmpty2 = false

    if cached_shape_data1.size_x != cached_shape_data2.size_x {
        return Some(format!(
            "size x: {} != {}",
            cached_shape_data1.size_x, cached_shape_data2.size_x
        ));
    }
    if cached_shape_data1.size_y != cached_shape_data2.size_y {
        return Some(format!(
            "size y: {} != {}",
            cached_shape_data1.size_y, cached_shape_data2.size_y
        ));
    }
    if cached_shape_data1.size_z != cached_shape_data2.size_z {
        return Some(format!(
            "size z: {} != {}",
            cached_shape_data1.size_z, cached_shape_data2.size_z
        ));
    }

    let mut ret = String::new();

    let size_x = cached_shape_data1.size_x;
    let size_y = cached_shape_data1.size_y;
    let size_z = cached_shape_data1.size_z;

    let mut first = true;

    for x in 0..size_x {
        for y in 0..size_y {
            for z in 0..size_z {
                let is_full1 = shape1.is_full(x, y, z);
                let is_full2 = shape2.is_full(x, y, z);

                if is_full1 == is_full2 {
                    continue;
                }

                if first {
                    first = false;
                } else {
                    ret.push_str(", ");
                }

                ret.push_str(&format!(
                    "({x},{y},{z}): shape1: {is_full1}, shape2: {is_full2}"
                ));
            }
        }
    }

    if ret.is_empty() { None } else { Some(ret) }
}

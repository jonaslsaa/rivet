//! `net.minecraft.world.phys.shapes.Shapes` — the `VoxelShape` factory and
//! boolean-algebra entry points (Paper: the join/occlusion members delegate to
//! Moonrise's `CollisionUtil`).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/Shapes.java`.
//!
//! STUB(com.mojang.math): `rotate(VoxelShape, OctahedralGroup[, Vec3])`,
//! `rotateHorizontalAxis`, `rotateAllAxis`, `rotateHorizontal`, `rotateAll`
//! and `rotateAttachFace` need `OctahedralGroup` (and `AttachFace` for the
//! last), whose units are not ported; they and `BLOCK_CENTER` land with them.
//! [`flip_axis_if_needed`], their `OctahedralGroup`-free helper, is ported.

use std::sync::{Arc, LazyLock};

use rivet_registry::core::{Axis, Direction};
use rivet_registry::world::phys::AABB;
use rivet_util::double_list::{DoubleArrayList, DoubleList, double_list_equals};
use rivet_util::java_math;
use rivet_util::mth;

pub use rivet_util::guava_math::gcd;

use super::array_voxel_shape;
use super::bit_set_discrete_voxel_shape::BitSetDiscreteVoxelShape;
use super::boolean_op::BooleanOp;
use super::cube_point_range::CubePointRange;
use super::discrete_cube_merger::DiscreteCubeMerger;
use super::discrete_voxel_shape::DiscreteVoxelShape;
use super::identical_merger::IdenticalMerger;
use super::index_merger::IndexMerger;
use super::indirect_merger::IndirectMerger;
use super::non_overlapping_merger::NonOverlappingMerger;
use super::voxel_shape::VoxelShape;
use crate::spottedleaf::moonrise::patches::collisions::collision_util::{
    self, COLLISION_EPSILON, zero_one,
};

/// `Shapes.EPSILON`.
pub const EPSILON: f64 = 1.0E-7;
/// `Shapes.BIG_EPSILON`.
pub const BIG_EPSILON: f64 = 1.0E-6;

/// `Shapes.DoubleLineConsumer.consume(double x6)`.
pub type DoubleLineConsumer<'a> = dyn FnMut(f64, f64, f64, f64, f64, f64) + 'a;

// Paper start - optimise collisions
/// `Shapes.BLOCK`.
static BLOCK: LazyLock<VoxelShape> = LazyLock::new(|| {
    let mut shape = BitSetDiscreteVoxelShape::new(1, 1, 1);
    shape.fill(0, 0, 0);

    array_voxel_shape::new(Arc::new(shape), zero_one(), zero_one(), zero_one())
});
// Paper end - optimise collisions

/// `Shapes.INFINITY`.
pub static INFINITY: LazyLock<VoxelShape> = LazyLock::new(|| {
    box_(
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
    )
});

/// `Shapes.EMPTY`.
static EMPTY: LazyLock<VoxelShape> = LazyLock::new(|| {
    array_voxel_shape::new(
        Arc::new(BitSetDiscreteVoxelShape::new(0, 0, 0)),
        Arc::new(DoubleArrayList::from_slice(&[0.0])),
        Arc::new(DoubleArrayList::from_slice(&[0.0])),
        Arc::new(DoubleArrayList::from_slice(&[0.0])),
    )
});

/// `Shapes.empty()`.
pub fn empty() -> VoxelShape {
    EMPTY.clone()
}

/// `Shapes.block()`.
pub fn block() -> VoxelShape {
    BLOCK.clone()
}

// Paper start - optimise collisions
/// `Shapes.PARTS_BY_BITS`.
static PARTS_BY_BITS: LazyLock<[Arc<dyn DoubleList>; 4]> =
    LazyLock::new(|| [parts(1 << 0), parts(1 << 1), parts(1 << 2), parts(1 << 3)]);

fn parts(parts: i32) -> Arc<dyn DoubleList> {
    Arc::new(DoubleArrayList::from_vec(generate_cube_parts(parts)))
}

/// `Shapes.generateCubeParts(int)`.
fn generate_cube_parts(parts: i32) -> Vec<f64> {
    // note: parts is a power of two, so we do not need to worry about loss of precision here
    // note: parts is from [2^0, 2^3]
    let inc = 1.0 / parts as f64;

    let mut ret = vec![0.0; (parts + 1) as usize];
    let mut val = 0.0;
    for slot in ret.iter_mut() {
        *slot = val;
        val += inc;
    }

    ret
}

/// `Shapes.mergedMayOccludeBlock(VoxelShape, VoxelShape)`.
fn merged_may_occlude_block(shape1: &VoxelShape, shape2: &VoxelShape) -> bool {
    // if the combined bounds of the two shapes cannot occlude, then neither can the merged
    let bounds1 = shape1.bounds();
    let bounds2 = shape2.bounds();

    let min_x = mth::min_f64(bounds1.min_x, bounds2.min_x);
    let min_y = mth::min_f64(bounds1.min_y, bounds2.min_y);
    let min_z = mth::min_f64(bounds1.min_z, bounds2.min_z);

    let max_x = mth::max_f64(bounds1.max_x, bounds2.max_x);
    let max_y = mth::max_f64(bounds1.max_y, bounds2.max_y);
    let max_z = mth::max_f64(bounds1.max_z, bounds2.max_z);

    (min_x <= COLLISION_EPSILON && max_x >= (1.0 - COLLISION_EPSILON))
        && (min_y <= COLLISION_EPSILON && max_y >= (1.0 - COLLISION_EPSILON))
        && (min_z <= COLLISION_EPSILON && max_z >= (1.0 - COLLISION_EPSILON))
}
// Paper end - optimise collisions

/// `Shapes.box(double x6)` — `IllegalArgumentException` (a panic) when a min
/// exceeds its max.
pub fn box_(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> VoxelShape {
    if !(min_x > max_x) && !(min_y > max_y) && !(min_z > max_z) {
        create(min_x, min_y, min_z, max_x, max_y, max_z)
    } else {
        panic!("The min values need to be smaller or equals to the max values");
    }
}

/// `Shapes.create(double x6)`.
pub fn create(
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
) -> VoxelShape {
    // Paper start - optimise collisions
    if !(max_x - min_x < 1.0E-7) && !(max_y - min_y < 1.0E-7) && !(max_z - min_z < 1.0E-7) {
        let bits_x = find_bits(min_x, max_x);
        let bits_y = find_bits(min_y, max_y);
        let bits_z = find_bits(min_z, max_z);
        if bits_x >= 0 && bits_y >= 0 && bits_z >= 0 {
            if bits_x == 0 && bits_y == 0 && bits_z == 0 {
                block()
            } else {
                let size_x = 1 << bits_x;
                let size_y = 1 << bits_y;
                let size_z = 1 << bits_z;
                let shape = BitSetDiscreteVoxelShape::with_filled_bounds(
                    size_x,
                    size_y,
                    size_z,
                    java_math::round(min_x * size_x as f64) as i32,
                    java_math::round(min_y * size_y as f64) as i32,
                    java_math::round(min_z * size_z as f64) as i32,
                    java_math::round(max_x * size_x as f64) as i32,
                    java_math::round(max_y * size_y as f64) as i32,
                    java_math::round(max_z * size_z as f64) as i32,
                );
                array_voxel_shape::new(
                    Arc::new(shape),
                    Arc::clone(&PARTS_BY_BITS[bits_x as usize]),
                    Arc::clone(&PARTS_BY_BITS[bits_y as usize]),
                    Arc::clone(&PARTS_BY_BITS[bits_z as usize]),
                )
            }
        } else {
            let coords = |min: f64, max: f64| -> Arc<dyn DoubleList> {
                if min == 0.0 && max == 1.0 {
                    zero_one()
                } else {
                    Arc::new(DoubleArrayList::wrap(Arc::from([min, max])))
                }
            };
            array_voxel_shape::new(
                Arc::clone(block().shape()),
                coords(min_x, max_x),
                coords(min_y, max_y),
                coords(min_z, max_z),
            )
        }
    } else {
        empty()
    }
    // Paper end - optimise collisions
}

/// `Shapes.create(AABB)`.
pub fn create_aabb(aabb: &AABB) -> VoxelShape {
    create(
        aabb.min_x, aabb.min_y, aabb.min_z, aabb.max_x, aabb.max_y, aabb.max_z,
    )
}

/// `Shapes.findBits(double, double)` (`@VisibleForTesting`).
pub fn find_bits(min: f64, max: f64) -> i32 {
    if !(min < -1.0E-7) && !(max > 1.0000001) {
        for bits in 0..=3 {
            let intervals = 1 << bits;
            let sh_min = min * intervals as f64;
            let sh_max = max * intervals as f64;
            let found_min =
                (sh_min - java_math::round(sh_min) as f64).abs() < 1.0E-7 * intervals as f64;
            let found_max =
                (sh_max - java_math::round(sh_max) as f64).abs() < 1.0E-7 * intervals as f64;
            if found_min && found_max {
                return bits;
            }
        }

        -1
    } else {
        -1
    }
}

/// `Shapes.lcm(int, int)` (`@VisibleForTesting`).
pub fn lcm(first: i32, second: i32) -> i64 {
    first as i64 * (second / gcd(first, second)) as i64
}

/// `Shapes.or(VoxelShape, VoxelShape)`.
pub fn or(first: &VoxelShape, second: &VoxelShape) -> VoxelShape {
    join(first, second, BooleanOp::OR)
}

// Paper start - optimise collisions
/// `Shapes.or(VoxelShape, VoxelShape...)`.
pub fn or_many(shape1: &VoxelShape, others: &[VoxelShape]) -> VoxelShape {
    let mut size = others.len();
    if size == 0 {
        return shape1.clone();
    }

    // reduce complexity of joins by splitting the merges

    // add extra slot for first shape
    size += 1;
    let mut tmp: Vec<VoxelShape> = others.to_vec();
    // insert first shape
    tmp.push(shape1.clone());

    while size > 1 {
        let mut new_size = 0;
        let mut i = 0;
        while i < size {
            let next = i + 1;
            if next >= size {
                // nothing to merge with, so leave it for next iteration
                tmp[new_size] = tmp[i].clone();
                new_size += 1;
                break;
            } else {
                // merge with adjacent
                let first = tmp[i].clone();
                let second = tmp[next].clone();

                tmp[new_size] = join_unoptimized(&first, &second, BooleanOp::OR);
                new_size += 1;
            }
            i += 2;
        }
        size = new_size;
    }

    tmp[0].optimize()
    // Paper end - optimise collisions
}

/// `Shapes.join(VoxelShape, VoxelShape, BooleanOp)`.
pub fn join(first: &VoxelShape, second: &VoxelShape, op: BooleanOp) -> VoxelShape {
    collision_util::join_optimized(first, second, op) // Paper - optimise collisions
}

/// `Shapes.joinUnoptimized(VoxelShape, VoxelShape, BooleanOp)`.
pub fn join_unoptimized(first: &VoxelShape, second: &VoxelShape, op: BooleanOp) -> VoxelShape {
    collision_util::join_unoptimized(first, second, op) // Paper - optimise collisions
}

/// `Shapes.joinIsNotEmpty(VoxelShape, VoxelShape, BooleanOp)`.
pub fn join_is_not_empty(first: &VoxelShape, second: &VoxelShape, op: BooleanOp) -> bool {
    collision_util::is_join_non_empty(first, second, op) // Paper - optimise collisions
}

/// `Shapes.joinIsNotEmpty(IndexMerger x3, DiscreteVoxelShape x2, BooleanOp)`
/// (private; unused since Paper's optimised path).
#[allow(dead_code)]
fn join_is_not_empty_mergers(
    x_merger: &dyn IndexMerger,
    y_merger: &dyn IndexMerger,
    z_merger: &dyn IndexMerger,
    first: &dyn DiscreteVoxelShape,
    second: &dyn DiscreteVoxelShape,
    op: BooleanOp,
) -> bool {
    !x_merger.for_merged_indexes(&mut |x1, x2, _xr| {
        y_merger.for_merged_indexes(&mut |y1, y2, _yr| {
            z_merger.for_merged_indexes(&mut |z1, z2, _zr| {
                !op.apply(
                    first.is_full_wide(x1, y1, z1),
                    second.is_full_wide(x2, y2, z2),
                )
            })
        })
    })
}

/// `Shapes.collide(Direction.Axis, AABB, Iterable<VoxelShape>, double)`.
pub fn collide(axis: Axis, moving: &AABB, shapes: &[VoxelShape], mut distance: f64) -> f64 {
    for shape in shapes {
        if distance.abs() < 1.0E-7 {
            return 0.0;
        }

        distance = shape.collide(axis, moving, distance);
    }

    distance
}

// Paper start - optimise collisions
/// `Shapes.blockOccludes(VoxelShape, VoxelShape, Direction)`.
pub fn block_occludes(first: &VoxelShape, second: &VoxelShape, direction: Direction) -> bool {
    let block = block();
    if first.ptr_eq(&block) & second.ptr_eq(&block) {
        return true;
    }

    if first.is_empty() | second.is_empty() {
        return false;
    }

    // we optimise getOpposite, so we can use it
    // secondly, use our cache to retrieve sliced shape
    let new_first = first.moonrise_get_face_shape_clamped(direction);
    if new_first.is_empty() {
        return false;
    }
    let new_second = second.moonrise_get_face_shape_clamped(direction.get_opposite());
    if new_second.is_empty() {
        return false;
    }

    !join_is_not_empty(&new_first, &new_second, BooleanOp::ONLY_FIRST)
}

/// `Shapes.mergedFaceOccludes(VoxelShape, VoxelShape, Direction)`.
pub fn merged_face_occludes(first: &VoxelShape, second: &VoxelShape, direction: Direction) -> bool {
    // see if any of the shapes on their own occludes, only if cached
    if first.moonrise_occludes_full_block_if_cached()
        || second.moonrise_occludes_full_block_if_cached()
    {
        return true;
    }

    if first.is_empty() & second.is_empty() {
        return false;
    }

    // we optimise getOpposite, so we can use it
    // secondly, use our cache to retrieve sliced shape
    let new_first = first.moonrise_get_face_shape_clamped(direction);
    let new_second = second.moonrise_get_face_shape_clamped(direction.get_opposite());

    // see if any of the shapes on their own occludes, only if cached
    if new_first.moonrise_occludes_full_block_if_cached()
        || new_second.moonrise_occludes_full_block_if_cached()
    {
        return true;
    }

    let first_empty = new_first.is_empty();
    let second_empty = new_second.is_empty();

    if first_empty & second_empty {
        return false;
    }

    if first_empty | second_empty {
        return if second_empty {
            new_first.moonrise_occludes_full_block()
        } else {
            new_second.moonrise_occludes_full_block()
        };
    }

    if new_first.ptr_eq(&new_second) {
        return new_first.moonrise_occludes_full_block();
    }

    merged_may_occlude_block(&new_first, &new_second)
        && new_first
            .moonrise_or_unoptimized(&new_second)
            .moonrise_occludes_full_block()
}

/// `Shapes.faceShapeOccludes(VoxelShape, VoxelShape)`.
pub fn face_shape_occludes(shape1: &VoxelShape, shape2: &VoxelShape) -> bool {
    if shape1.moonrise_occludes_full_block_if_cached()
        || shape2.moonrise_occludes_full_block_if_cached()
    {
        return true;
    }

    let s1_empty = shape1.is_empty();
    let s2_empty = shape2.is_empty();
    if s1_empty & s2_empty {
        return false;
    }

    if s1_empty | s2_empty {
        return if s2_empty {
            shape1.moonrise_occludes_full_block()
        } else {
            shape2.moonrise_occludes_full_block()
        };
    }

    if shape1.ptr_eq(shape2) {
        return shape1.moonrise_occludes_full_block();
    }

    merged_may_occlude_block(shape1, shape2)
        && shape1
            .moonrise_or_unoptimized(shape2)
            .moonrise_occludes_full_block()
    // Paper end - optimise collisions
}

/// `Shapes.createIndexMerger(int, DoubleList, DoubleList, boolean, boolean)`
/// (`@VisibleForTesting`; Paper: private).
pub fn create_index_merger(
    cost: i32,
    first: &Arc<dyn DoubleList>,
    second: &Arc<dyn DoubleList>,
    first_only_matters: bool,
    second_only_matters: bool,
) -> Box<dyn IndexMerger> {
    // Paper start - fast track the most common scenario
    // first is usually a DoubleArrayList with Infinite head/tails that falls to the final else clause
    // This is actually the most common path, so jump to it straight away
    if first.get_double(0) == f64::NEG_INFINITY
        && first.get_double(first.size() - 1) == f64::INFINITY
    {
        return Box::new(IndirectMerger::new(
            &**first,
            &**second,
            first_only_matters,
            second_only_matters,
        ));
    }
    // Split out rest to hopefully inline the above
    less_common_merge(cost, first, second, first_only_matters, second_only_matters)
}

/// `Objects.equals(DoubleList, DoubleList)`.
fn objects_equals(first: &Arc<dyn DoubleList>, second: &Arc<dyn DoubleList>) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(first), Arc::as_ptr(second))
        || double_list_equals(&**first, &**second)
}

/// `Shapes.lessCommonMerge(...)`.
fn less_common_merge(
    cost: i32,
    first: &Arc<dyn DoubleList>,
    second: &Arc<dyn DoubleList>,
    first_only_matters: bool,
    second_only_matters: bool,
) -> Box<dyn IndexMerger> {
    // Paper end - fast track the most common scenario
    let first_size = first.size() as i32 - 1;
    let second_size = second.size() as i32 - 1;
    // Paper note - Rewrite below as optimized order if instead of nasty ternary
    if first.as_any().is::<CubePointRange>() && second.as_any().is::<CubePointRange>() {
        let size = lcm(first_size, second_size);
        if cost as i64 * size <= 256 {
            return Box::new(DiscreteCubeMerger::new(first_size, second_size));
        }
    }

    // Paper start - Identical happens more often than Disjoint
    if first_size == second_size && objects_equals(first, second) {
        // `first instanceof IdenticalMerger` / `second instanceof
        // IdenticalMerger` can never hold: `IdenticalMerger` is an
        // `IndexMerger`, not a `DoubleList`.
        Box::new(IdenticalMerger::new(Arc::clone(first)))
    } else if first.get_double(first_size as usize) < second.get_double(0) - 1.0E-7 {
        // Paper end - Identical happens more often than Disjoint
        Box::new(NonOverlappingMerger::new(
            Arc::clone(first),
            Arc::clone(second),
            false,
        ))
    } else if second.get_double(second_size as usize) < first.get_double(0) - 1.0E-7 {
        Box::new(NonOverlappingMerger::new(
            Arc::clone(second),
            Arc::clone(first),
            true,
        ))
    } else {
        Box::new(IndirectMerger::new(
            &**first,
            &**second,
            first_only_matters,
            second_only_matters,
        )) // Paper - Identical happens more often than Disjoint
    }
}

/// `Shapes.flipAxisIfNeeded(DoubleList, boolean, double, double)`
/// (`@VisibleForTesting`).
pub fn flip_axis_if_needed(
    new_axis: &Arc<dyn DoubleList>,
    flip: bool,
    new_relative: f64,
    old_relative: f64,
) -> Arc<dyn DoubleList> {
    if !flip && new_relative == old_relative {
        return Arc::clone(new_axis);
    }

    let size = new_axis.size();
    let mut new_list = Vec::with_capacity(size);
    if flip {
        for i in (0..size).rev() {
            new_list.push(-(new_axis.get_double(i) - new_relative) + old_relative);
        }
    } else {
        for i in 0..size {
            new_list.push(new_axis.get_double(i) - new_relative + old_relative);
        }
    }

    Arc::new(DoubleArrayList::from_vec(new_list))
}

/// `Shapes.equal(VoxelShape, VoxelShape)`.
pub fn equal(first: &VoxelShape, second: &VoxelShape) -> bool {
    !join_is_not_empty(first, second, BooleanOp::NOT_SAME)
}

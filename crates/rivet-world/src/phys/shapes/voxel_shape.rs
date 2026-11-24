//! `net.minecraft.world.phys.shapes.VoxelShape` — an immutable voxel-grid
//! shape with real-valued grid coordinates (Paper: implements
//! `CollisionVoxelShape`, "optimise collisions").
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/VoxelShape.java`.
//!
//! `VoxelShape` is abstract with three subclasses (`ArrayVoxelShape`,
//! `CubeVoxelShape`, `SliceShape`), so it follows OWNERSHIP.md §Class
//! hierarchies: [`VoxelShapeData`] holds the abstract class's fields and is
//! embedded in a shared handle, and the subclass-specific members
//! (`getCoords`, the `findIndex` override) are a [`VoxelShapeBehavior`] value.
//! A Java `VoxelShape` reference is a [`VoxelShape`] (`Arc`); reference
//! identity (`==`, `System.identityHashCode`) is [`VoxelShape::ptr_eq`].
//!
//! The lazily filled Moonrise caches (`cachedBounds`, `cachedToAABBs`,
//! `isFullBlock`, `occludesFullBlock`, the OR cache, the face caches) are
//! write-once or small mutexed slots: a shape's value never changes, only
//! which derived values it has memoised ("ignore race conditions on
//! field read/write: the shape is static"). No lock is held across a call
//! into another shape. A face cache that would hold the shape itself stores
//! [`FaceSlot::This`] instead of a strong self-reference (Java's GC reclaims
//! the cycle; an `Arc` cycle would leak).
//!
//! `Shapes.rotate` and the `OctahedralGroup`-taking members are
//! STUB(com.mojang.math) in `shapes.rs`.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use rivet_registry::core::{Axis, AxisCycle, AxisDirection, BlockPos, Direction, Vec3i};
use rivet_registry::world::phys::{AABB, BlockHitResult, Vec3};
use rivet_util::double_list::{DoubleArrayList, DoubleList};
use rivet_util::guava_math::fuzzy_equals;
use rivet_util::mth;

use super::array_voxel_shape;
use super::boolean_op::BooleanOp;
use super::discrete_voxel_shape::DiscreteVoxelShape;
use super::offset_double_list::OffsetDoubleList;
use super::shapes::{self, DoubleLineConsumer};
use crate::spottedleaf::moonrise::common::util::flat_bitset_util;
use crate::spottedleaf::moonrise::patches::collisions::collision_util::{self, COLLISION_EPSILON};
use crate::spottedleaf::moonrise::patches::collisions::shape::cached_shape_data::CachedShapeData;
use crate::spottedleaf::moonrise::patches::collisions::shape::cached_to_aabbs::CachedToAABBs;
use crate::spottedleaf::moonrise::patches::collisions::shape::merged_or_cache::MergedORCache;

/// `VoxelShape.MERGED_CACHE_SIZE` — must be a power of two.
const MERGED_CACHE_SIZE: usize = 16;

/// The subclass part of a `VoxelShape`.
pub trait VoxelShapeBehavior: Send + Sync + fmt::Debug {
    /// `VoxelShape.getCoords(Direction.Axis)` — abstract. `shape` is the
    /// shape's discrete grid.
    fn get_coords(&self, shape: &dyn DiscreteVoxelShape, axis: Axis) -> Arc<dyn DoubleList>;

    /// A subclass override of `VoxelShape.findIndex(Direction.Axis, double)`
    /// (`CubeVoxelShape`); `None` keeps Paper's `findFloor` implementation.
    fn find_index(&self, _shape: &dyn DiscreteVoxelShape, _axis: Axis, _coord: f64) -> Option<i32> {
        None
    }

    /// `instanceof CubeVoxelShape`.
    fn is_cube_voxel_shape(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any;
}

/// A face-cache entry: the shape itself, or another shape.
#[derive(Clone, Debug)]
enum FaceSlot {
    This,
    Shape(VoxelShape),
}

type FaceCache = Mutex<[Option<FaceSlot>; 6]>;

/// The fields of the abstract `VoxelShape`.
pub struct VoxelShapeData {
    /// `VoxelShape.shape`.
    shape: Arc<dyn DiscreteVoxelShape>,
    behavior: Box<dyn VoxelShapeBehavior>,
    /// `faces`.
    faces: FaceCache,
    // Paper start - optimise collisions
    offset_x: f64,
    offset_y: f64,
    offset_z: f64,
    single_aabb_representation: Option<AABB>,
    root_coordinates_x: Arc<[f64]>,
    root_coordinates_y: Arc<[f64]>,
    root_coordinates_z: Arc<[f64]>,
    cached_shape_data: Arc<CachedShapeData>,
    is_empty: bool,
    cached_to_aabbs: Mutex<Option<CachedToAABBs>>,
    cached_bounds: OnceLock<AABB>,
    is_full_block: OnceLock<bool>,
    occludes_full_block: OnceLock<bool>,
    merged_or_cache: Mutex<Option<Vec<Option<MergedORCache>>>>,
    face_shape_clamped_cache: FaceCache,
    // Paper end - optimise collisions
}

impl fmt::Debug for VoxelShapeData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoxelShape")
            .field("behavior", &self.behavior)
            .field("shape", &self.shape)
            .finish()
    }
}

/// A `VoxelShape` reference.
#[derive(Clone, Debug)]
pub struct VoxelShape(Arc<VoxelShapeData>);

/// `VoxelShape.extractRawArray(DoubleList)`.
fn extract_raw_array(list: &Arc<dyn DoubleList>) -> Arc<[f64]> {
    if let Some(raw_list) = list.as_any().downcast_ref::<DoubleArrayList>() {
        let raw = raw_list.elements();
        let expected = raw_list.size();
        if raw.len() == expected {
            Arc::clone(raw)
        } else {
            Arc::from(&raw[..expected])
        }
    } else {
        Arc::from(list.to_double_array())
    }
}

/// `VoxelShape.offsetList(double[], double)`.
fn offset_list(src: &Arc<[f64]>, by: f64) -> Arc<dyn DoubleList> {
    let wrap: Arc<dyn DoubleList> = Arc::new(DoubleArrayList::wrap(Arc::clone(src)));
    if by == 0.0 {
        return wrap;
    }
    Arc::new(OffsetDoubleList::new(wrap, by))
}

/// `HashCommon.mix(System.identityHashCode(key))`.
fn hash(key: &VoxelShape) -> usize {
    let h = ((Arc::as_ptr(&key.0) as *const () as usize >> 4) as u32).wrapping_mul(0x9E37_79B9);
    (h ^ (h >> 16)) as usize
}

impl VoxelShape {
    /// `VoxelShape(DiscreteVoxelShape)` followed by the subclass constructor's
    /// `moonrise$initCache()`.
    pub fn new(
        shape: Arc<dyn DiscreteVoxelShape>,
        behavior: Box<dyn VoxelShapeBehavior>,
    ) -> VoxelShape {
        let cached_shape_data = shape.moonrise_get_or_create_cached_shape_data();
        let is_empty = cached_shape_data.is_empty;

        let x_list = behavior.get_coords(&*shape, Axis::X);
        let y_list = behavior.get_coords(&*shape, Axis::Y);
        let z_list = behavior.get_coords(&*shape, Axis::Z);

        // `xList instanceof OffsetDoubleList`
        let split = |list: &Arc<dyn DoubleList>| -> (f64, Arc<[f64]>) {
            if let Some(offset_double_list) = list.as_any().downcast_ref::<OffsetDoubleList>() {
                (
                    offset_double_list.offset,
                    extract_raw_array(&offset_double_list.delegate),
                )
            } else {
                (0.0, extract_raw_array(list))
            }
        };
        let (offset_x, root_coordinates_x) = split(&x_list);
        let (offset_y, root_coordinates_y) = split(&y_list);
        let (offset_z, root_coordinates_z) = split(&z_list);

        let mut single_aabb_representation = None;
        let cached_bounds = OnceLock::new();
        if cached_shape_data.has_single_aabb {
            let aabb = AABB::new(
                root_coordinates_x[0] + offset_x,
                root_coordinates_y[0] + offset_y,
                root_coordinates_z[0] + offset_z,
                root_coordinates_x[1] + offset_x,
                root_coordinates_y[1] + offset_y,
                root_coordinates_z[1] + offset_z,
            );
            let _ = cached_bounds.set(aabb);
            single_aabb_representation = Some(aabb);
        }

        VoxelShape(Arc::new(VoxelShapeData {
            shape,
            behavior,
            faces: Mutex::new(Default::default()),
            offset_x,
            offset_y,
            offset_z,
            single_aabb_representation,
            root_coordinates_x,
            root_coordinates_y,
            root_coordinates_z,
            cached_shape_data,
            is_empty,
            cached_to_aabbs: Mutex::new(None),
            cached_bounds,
            is_full_block: OnceLock::new(),
            occludes_full_block: OnceLock::new(),
            merged_or_cache: Mutex::new(None),
            face_shape_clamped_cache: Mutex::new(Default::default()),
        }))
    }

    /// Java reference equality (`a == b`, `Object.equals`).
    pub fn ptr_eq(&self, other: &VoxelShape) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// A weak reference (identity-keyed caches).
    pub fn downgrade(&self) -> Weak<VoxelShapeData> {
        Arc::downgrade(&self.0)
    }

    /// The subclass behavior.
    pub fn behavior(&self) -> &dyn VoxelShapeBehavior {
        &*self.0.behavior
    }

    /// `VoxelShape.shape`.
    pub fn shape(&self) -> &Arc<dyn DiscreteVoxelShape> {
        &self.0.shape
    }

    // Paper start - optimise collisions

    /// `moonrise$offsetX()`.
    pub fn moonrise_offset_x(&self) -> f64 {
        self.0.offset_x
    }

    /// `moonrise$offsetY()`.
    pub fn moonrise_offset_y(&self) -> f64 {
        self.0.offset_y
    }

    /// `moonrise$offsetZ()`.
    pub fn moonrise_offset_z(&self) -> f64 {
        self.0.offset_z
    }

    /// `moonrise$getSingleAABBRepresentation()`.
    pub fn moonrise_get_single_aabb_representation(&self) -> Option<&AABB> {
        self.0.single_aabb_representation.as_ref()
    }

    /// `moonrise$rootCoordinatesX()`.
    pub fn moonrise_root_coordinates_x(&self) -> &Arc<[f64]> {
        &self.0.root_coordinates_x
    }

    /// `moonrise$rootCoordinatesY()`.
    pub fn moonrise_root_coordinates_y(&self) -> &Arc<[f64]> {
        &self.0.root_coordinates_y
    }

    /// `moonrise$rootCoordinatesZ()`.
    pub fn moonrise_root_coordinates_z(&self) -> &Arc<[f64]> {
        &self.0.root_coordinates_z
    }

    /// `moonrise$getCachedVoxelData()`.
    pub fn moonrise_get_cached_voxel_data(&self) -> &CachedShapeData {
        &self.0.cached_shape_data
    }

    fn face_slot(&self, slot: Option<FaceSlot>) -> Option<VoxelShape> {
        slot.map(|slot| match slot {
            FaceSlot::This => self.clone(),
            FaceSlot::Shape(shape) => shape,
        })
    }

    fn make_face_slot(&self, shape: &VoxelShape) -> FaceSlot {
        if shape.ptr_eq(self) {
            FaceSlot::This
        } else {
            FaceSlot::Shape(shape.clone())
        }
    }

    /// `moonrise$getFaceShapeClamped(Direction)`.
    pub fn moonrise_get_face_shape_clamped(&self, direction: Direction) -> VoxelShape {
        if self.0.is_empty {
            return self.clone();
        }
        if self.ptr_eq(&shapes::block()) {
            return self.clone();
        }

        let cached = self.0.face_shape_clamped_cache.lock().unwrap()[direction as usize].clone();
        if let Some(ret) = self.face_slot(cached) {
            return ret;
        }

        let axis = direction.get_axis();

        let ret = if direction.get_axis_direction() == AxisDirection::Positive {
            if fuzzy_equals(self.max(axis), 1.0, COLLISION_EPSILON) {
                collision_util::slice_shape(self, axis, self.0.shape.get_size(axis) - 1)
            } else {
                shapes::empty()
            }
        } else if fuzzy_equals(self.min(axis), 0.0, COLLISION_EPSILON) {
            collision_util::slice_shape(self, axis, 0)
        } else {
            shapes::empty()
        };

        let slot = self.make_face_slot(&ret);
        self.0.face_shape_clamped_cache.lock().unwrap()[direction as usize] = Some(slot);

        ret
    }

    fn compute_occludes_full_block(&self) -> bool {
        if self.0.is_empty {
            let _ = self.0.occludes_full_block.set(false);
            return false;
        }

        if self.moonrise_is_full_block() {
            let _ = self.0.occludes_full_block.set(true);
            return true;
        }

        if let Some(single_aabb) = &self.0.single_aabb_representation {
            // check if the bounding box encloses the full cube
            let ret = (single_aabb.min_y <= COLLISION_EPSILON
                && single_aabb.max_y >= (1.0 - COLLISION_EPSILON))
                && (single_aabb.min_x <= COLLISION_EPSILON
                    && single_aabb.max_x >= (1.0 - COLLISION_EPSILON))
                && (single_aabb.min_z <= COLLISION_EPSILON
                    && single_aabb.max_z >= (1.0 - COLLISION_EPSILON));
            let _ = self.0.occludes_full_block.set(ret);
            return ret;
        }

        let ret = !shapes::join_is_not_empty(&shapes::block(), self, BooleanOp::ONLY_FIRST);
        let _ = self.0.occludes_full_block.set(ret);
        ret
    }

    /// `moonrise$occludesFullBlock()`.
    pub fn moonrise_occludes_full_block(&self) -> bool {
        if let Some(ret) = self.0.occludes_full_block.get() {
            return *ret;
        }

        self.compute_occludes_full_block()
    }

    /// `moonrise$occludesFullBlockIfCached()`.
    pub fn moonrise_occludes_full_block_if_cached(&self) -> bool {
        self.0.occludes_full_block.get().copied().unwrap_or(false)
    }

    /// `moonrise$orUnoptimized(VoxelShape)`.
    pub fn moonrise_or_unoptimized(&self, other: &VoxelShape) -> VoxelShape {
        // don't cache simple cases
        if self.ptr_eq(other) {
            return other.clone();
        }

        if self.0.is_empty {
            return other.clone();
        }

        if other.is_empty() {
            return self.clone();
        }

        // try this cache first
        let this_cache_key = hash(other) & (MERGED_CACHE_SIZE - 1);
        let cached = self
            .0
            .merged_or_cache
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|cache| cache[this_cache_key].clone());
        if let Some(cached) = &cached
            && cached.key_is(other)
        {
            return cached.result();
        }

        // try other cache
        let other_cache_key = hash(self) & (MERGED_CACHE_SIZE - 1);
        let other_cache = other
            .0
            .merged_or_cache
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|cache| cache[other_cache_key].clone());
        if let Some(other_cache) = &other_cache
            && other_cache.key_is(self)
        {
            return other_cache.result();
        }

        // note: unsure if joinUnoptimized(1, 2, OR) == joinUnoptimized(2, 1, OR) for all cases
        let result = shapes::join_unoptimized(self, other, BooleanOp::OR);

        if cached.is_some() && other_cache.is_none() {
            // try to use second cache instead of replacing an entry in this cache
            let mut guard = other.0.merged_or_cache.lock().unwrap();
            let cache = guard.get_or_insert_with(|| vec![None; MERGED_CACHE_SIZE]);
            cache[other_cache_key] = Some(MergedORCache::new(self, result.clone()));
        } else {
            // line is not occupied or other cache line is full
            // always bias to replace this cache, as this cache is the first we check
            let mut guard = self.0.merged_or_cache.lock().unwrap();
            let cache = guard.get_or_insert_with(|| vec![None; MERGED_CACHE_SIZE]);
            cache[this_cache_key] = Some(MergedORCache::new(other, result.clone()));
        }

        result
    }

    fn to_aabbs_uncached(&self) -> Arc<Vec<AABB>> {
        let ret: Vec<AABB> = if let Some(single_aabb) = &self.0.single_aabb_representation {
            vec![*single_aabb]
        } else {
            let mut ret = Vec::new();
            let coords_x = &self.0.root_coordinates_x;
            let coords_y = &self.0.root_coordinates_y;
            let coords_z = &self.0.root_coordinates_z;

            let off_x = self.0.offset_x;
            let off_y = self.0.offset_y;
            let off_z = self.0.offset_z;

            self.0.shape.for_all_boxes(
                &mut |min_x, min_y, min_z, max_x, max_y, max_z| {
                    ret.push(AABB::new(
                        coords_x[min_x as usize] + off_x,
                        coords_y[min_y as usize] + off_y,
                        coords_z[min_z as usize] + off_z,
                        coords_x[max_x as usize] + off_x,
                        coords_y[max_y as usize] + off_y,
                        coords_z[max_z as usize] + off_z,
                    ));
                },
                true,
            );
            ret
        };

        let ret = Arc::new(ret);
        // cache result
        *self.0.cached_to_aabbs.lock().unwrap() = Some(CachedToAABBs {
            aabbs: Arc::clone(&ret),
            is_offset: false,
            off_x: 0.0,
            off_y: 0.0,
            off_z: 0.0,
        });

        ret
    }

    fn compute_full_block(&self) -> bool {
        let ret = if self.0.is_empty {
            false
        } else if self.ptr_eq(&shapes::block()) {
            true
        } else if let Some(single_aabb) = &self.0.single_aabb_representation {
            single_aabb.min_x.abs() <= COLLISION_EPSILON
                && single_aabb.min_y.abs() <= COLLISION_EPSILON
                && single_aabb.min_z.abs() <= COLLISION_EPSILON
                && (1.0 - single_aabb.max_x).abs() <= COLLISION_EPSILON
                && (1.0 - single_aabb.max_y).abs() <= COLLISION_EPSILON
                && (1.0 - single_aabb.max_z).abs() <= COLLISION_EPSILON
        } else {
            let shape_data = &self.0.cached_shape_data;
            let s_min_x = shape_data.min_full_x;
            let s_min_y = shape_data.min_full_y;
            let s_min_z = shape_data.min_full_z;

            let s_max_x = shape_data.max_full_x;
            let s_max_y = shape_data.max_full_y;
            let s_max_z = shape_data.max_full_z;

            if (self.0.root_coordinates_x[s_min_x as usize] + self.0.offset_x).abs()
                <= COLLISION_EPSILON
                && (self.0.root_coordinates_y[s_min_y as usize] + self.0.offset_y).abs()
                    <= COLLISION_EPSILON
                && (self.0.root_coordinates_z[s_min_z as usize] + self.0.offset_z).abs()
                    <= COLLISION_EPSILON
                && (1.0 - (self.0.root_coordinates_x[s_max_x as usize] + self.0.offset_x)).abs()
                    <= COLLISION_EPSILON
                && (1.0 - (self.0.root_coordinates_y[s_max_y as usize] + self.0.offset_y)).abs()
                    <= COLLISION_EPSILON
                && (1.0 - (self.0.root_coordinates_z[s_max_z as usize] + self.0.offset_z)).abs()
                    <= COLLISION_EPSILON
            {
                // index = z + y*sizeZ + x*(sizeZ*sizeY)

                let size_y = shape_data.size_y;
                let size_z = shape_data.size_z;

                let bitset: &[u64] = &shape_data.voxel_set;

                let mut ret = true;

                'check_full: for x in s_min_x..s_max_x {
                    for y in s_min_y..s_max_y {
                        let base_index = y * size_z + x * (size_z * size_y);
                        if !flat_bitset_util::is_range_set(
                            bitset,
                            base_index + s_min_z,
                            base_index + s_max_z,
                        ) {
                            ret = false;
                            break 'check_full;
                        }
                    }
                }
                ret
            } else {
                false
            }
        };

        let _ = self.0.is_full_block.set(ret);

        ret
    }

    /// `moonrise$isFullBlock()`.
    pub fn moonrise_is_full_block(&self) -> bool {
        if let Some(ret) = self.0.is_full_block.get() {
            return *ret;
        }

        self.compute_full_block()
    }

    /// `VoxelShape.clip(AABB, Vec3, Vec3, BlockPos)` (private static).
    fn clip_aabb(aabb: &AABB, from: &Vec3, to: &Vec3, offset: &BlockPos) -> Option<BlockHitResult> {
        let mut min_distance = 1.0;
        let diff_x = to.x - from.x;
        let diff_y = to.y - from.y;
        let diff_z = to.z - from.z;

        let direction = AABB::get_direction(
            &aabb.move_block_pos(offset),
            from,
            &mut min_distance,
            None,
            diff_x,
            diff_y,
            diff_z,
        )?;

        Some(BlockHitResult::new(
            from.add_xyz(
                min_distance * diff_x,
                min_distance * diff_y,
                min_distance * diff_z,
            ),
            direction,
            *offset,
            false,
        ))
    }

    fn calculate_face_direct(
        &self,
        direction: Direction,
        axis: Axis,
        coords: &[f64],
        offset: f64,
    ) -> VoxelShape {
        if coords.len() == 2
            && fuzzy_equals(coords[0] + offset, 0.0, COLLISION_EPSILON)
            && fuzzy_equals(coords[1] + offset, 1.0, COLLISION_EPSILON)
        {
            return self.clone();
        }

        let positive_dir = direction.get_axis_direction() == AxisDirection::Positive;

        // see findIndex
        let index = collision_util::find_floor(
            coords,
            offset,
            if positive_dir {
                1.0 - COLLISION_EPSILON
            } else {
                0.0 + COLLISION_EPSILON
            },
            0,
            coords.len() as i32 - 1,
        );

        collision_util::slice_shape(self, axis, index)
    }
    // Paper end - optimise collisions

    /// `VoxelShape.min(Direction.Axis)`.
    pub fn min(&self, axis: Axis) -> f64 {
        let shape_data = &self.0.cached_shape_data;
        match axis {
            Axis::X => {
                let idx = shape_data.min_full_x;
                if idx >= shape_data.size_x {
                    f64::INFINITY
                } else {
                    self.0.root_coordinates_x[idx as usize] + self.0.offset_x
                }
            }
            Axis::Y => {
                let idx = shape_data.min_full_y;
                if idx >= shape_data.size_y {
                    f64::INFINITY
                } else {
                    self.0.root_coordinates_y[idx as usize] + self.0.offset_y
                }
            }
            Axis::Z => {
                let idx = shape_data.min_full_z;
                if idx >= shape_data.size_z {
                    f64::INFINITY
                } else {
                    self.0.root_coordinates_z[idx as usize] + self.0.offset_z
                }
            }
        }
    }

    /// `VoxelShape.max(Direction.Axis)`.
    pub fn max(&self, axis: Axis) -> f64 {
        let shape_data = &self.0.cached_shape_data;
        match axis {
            Axis::X => {
                let idx = shape_data.max_full_x;
                if idx <= 0 {
                    f64::NEG_INFINITY
                } else {
                    self.0.root_coordinates_x[idx as usize] + self.0.offset_x
                }
            }
            Axis::Y => {
                let idx = shape_data.max_full_y;
                if idx <= 0 {
                    f64::NEG_INFINITY
                } else {
                    self.0.root_coordinates_y[idx as usize] + self.0.offset_y
                }
            }
            Axis::Z => {
                let idx = shape_data.max_full_z;
                if idx <= 0 {
                    f64::NEG_INFINITY
                } else {
                    self.0.root_coordinates_z[idx as usize] + self.0.offset_z
                }
            }
        }
    }

    /// `VoxelShape.bounds()` — `UnsupportedOperationException` (a panic) for an
    /// empty shape.
    pub fn bounds(&self) -> AABB {
        if self.0.is_empty {
            panic!("No bounds for empty shape.");
        }
        if let Some(cached) = self.0.cached_bounds.get() {
            return *cached;
        }

        let shape_data = &self.0.cached_shape_data;

        let coords_x = &self.0.root_coordinates_x;
        let coords_y = &self.0.root_coordinates_y;
        let coords_z = &self.0.root_coordinates_z;

        let off_x = self.0.offset_x;
        let off_y = self.0.offset_y;
        let off_z = self.0.offset_z;

        // note: if not empty, then there is one full AABB so no bounds checks are needed on the minFull/maxFull indices
        let cached = AABB::new(
            coords_x[shape_data.min_full_x as usize] + off_x,
            coords_y[shape_data.min_full_y as usize] + off_y,
            coords_z[shape_data.min_full_z as usize] + off_z,
            coords_x[shape_data.max_full_x as usize] + off_x,
            coords_y[shape_data.max_full_y as usize] + off_y,
            coords_z[shape_data.max_full_z as usize] + off_z,
        );

        let _ = self.0.cached_bounds.set(cached);
        cached
    }

    /// `VoxelShape.singleEncompassing()`.
    pub fn single_encompassing(&self) -> VoxelShape {
        if self.0.is_empty {
            return shapes::empty();
        }
        shapes::create_aabb(&self.bounds())
    }

    /// `VoxelShape.get(Direction.Axis, int)` (protected).
    pub fn get(&self, axis: Axis, i: i32) -> f64 {
        let idx = i as usize;
        match axis {
            Axis::X => self.0.root_coordinates_x[idx] + self.0.offset_x,
            Axis::Y => self.0.root_coordinates_y[idx] + self.0.offset_y,
            Axis::Z => self.0.root_coordinates_z[idx] + self.0.offset_z,
        }
    }

    /// `VoxelShape.getCoords(Direction.Axis)`.
    pub fn get_coords(&self, axis: Axis) -> Arc<dyn DoubleList> {
        self.0.behavior.get_coords(&*self.0.shape, axis)
    }

    /// `VoxelShape.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty
    }

    /// `VoxelShape.move(Vec3)`.
    pub fn move_vec3(&self, delta: &Vec3) -> VoxelShape {
        self.move_xyz(delta.x, delta.y, delta.z)
    }

    /// `VoxelShape.move(Vec3i)`.
    pub fn move_vec3i(&self, delta: &Vec3i) -> VoxelShape {
        self.move_xyz(
            delta.get_x() as f64,
            delta.get_y() as f64,
            delta.get_z() as f64,
        )
    }

    /// `VoxelShape.move(double, double, double)`.
    pub fn move_xyz(&self, dx: f64, dy: f64, dz: f64) -> VoxelShape {
        if self.0.is_empty {
            return shapes::empty();
        }

        let ret = array_voxel_shape::new(
            Arc::clone(&self.0.shape),
            offset_list(&self.0.root_coordinates_x, self.0.offset_x + dx),
            offset_list(&self.0.root_coordinates_y, self.0.offset_y + dy),
            offset_list(&self.0.root_coordinates_z, self.0.offset_z + dz),
        );

        let cached_to_aabbs = self.0.cached_to_aabbs.lock().unwrap().clone();
        if let Some(cached_to_aabbs) = cached_to_aabbs {
            *ret.0.cached_to_aabbs.lock().unwrap() =
                Some(CachedToAABBs::offset(&cached_to_aabbs, dx, dy, dz));
        }

        ret
    }

    /// `ret.cachedToAABBs == null -> this.cachedToAABBs` — "forward AABB cache".
    fn forward_aabb_cache(&self, ret: &VoxelShape) {
        let mut ret_cache = ret.0.cached_to_aabbs.lock().unwrap();
        if ret_cache.is_none() {
            let own = self.0.cached_to_aabbs.lock().unwrap().clone();
            *ret_cache = own;
        }
    }

    /// `VoxelShape.optimize()`.
    pub fn optimize(&self) -> VoxelShape {
        if self.0.is_empty {
            return shapes::empty();
        }

        if self.0.single_aabb_representation.is_some() {
            // note: the isFullBlock() is fuzzy, and Shapes.create() is also fuzzy which would return block()
            return if self.moonrise_is_full_block() {
                shapes::block()
            } else {
                self.clone()
            };
        }

        let aabbs = self.to_aabbs();

        if aabbs.is_empty() {
            // We are a SliceShape, which does not properly fill isEmpty for every case
            return shapes::empty();
        }

        if aabbs.len() == 1 {
            let single_aabb = aabbs[0];
            let ret = shapes::create_aabb(&single_aabb);

            // forward AABB cache
            self.forward_aabb_cache(&ret);

            ret
        } else {
            // reduce complexity of joins by splitting the merges (old complexity: n^2, new: nlogn)

            // set up flat array so that this merge is done in-place
            // initialise as unmerged
            let mut tmp: Vec<VoxelShape> = aabbs.iter().map(shapes::create_aabb).collect();

            let mut size = aabbs.len();
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

                        tmp[new_size] = shapes::join_unoptimized(&first, &second, BooleanOp::OR);
                        new_size += 1;
                    }
                    i += 2;
                }
                size = new_size;
            }

            let ret = tmp[0].clone();

            // forward AABB cache
            self.forward_aabb_cache(&ret);

            ret
        }
    }

    /// `VoxelShape.forAllEdges(Shapes.DoubleLineConsumer)`.
    pub fn for_all_edges(&self, consumer: &mut DoubleLineConsumer<'_>) {
        self.0.shape.for_all_edges(
            &mut |xi1, yi1, zi1, xi2, yi2, zi2| {
                consumer(
                    self.get(Axis::X, xi1),
                    self.get(Axis::Y, yi1),
                    self.get(Axis::Z, zi1),
                    self.get(Axis::X, xi2),
                    self.get(Axis::Y, yi2),
                    self.get(Axis::Z, zi2),
                )
            },
            true,
        );
    }

    /// `VoxelShape.forAllBoxes(Shapes.DoubleLineConsumer)`.
    pub fn for_all_boxes(&self, consumer: &mut DoubleLineConsumer<'_>) {
        let x_coords = self.get_coords(Axis::X);
        let y_coords = self.get_coords(Axis::Y);
        let z_coords = self.get_coords(Axis::Z);
        self.0.shape.for_all_boxes(
            &mut |xi1, yi1, zi1, xi2, yi2, zi2| {
                consumer(
                    x_coords.get_double(xi1 as usize),
                    y_coords.get_double(yi1 as usize),
                    z_coords.get_double(zi1 as usize),
                    x_coords.get_double(xi2 as usize),
                    y_coords.get_double(yi2 as usize),
                    z_coords.get_double(zi2 as usize),
                )
            },
            true,
        );
    }

    /// `VoxelShape.toAabbs()`.
    pub fn to_aabbs(&self) -> Arc<Vec<AABB>> {
        let cached_to_aabbs = self.0.cached_to_aabbs.lock().unwrap().clone();
        if let Some(cached_to_aabbs) = cached_to_aabbs {
            if !cached_to_aabbs.is_offset {
                return cached_to_aabbs.aabbs;
            }

            // all we need to do is offset the cache
            let cached_to_aabbs = cached_to_aabbs.remove_offset();
            // update cache
            *self.0.cached_to_aabbs.lock().unwrap() = Some(cached_to_aabbs.clone());

            return cached_to_aabbs.aabbs;
        }

        // make new cache
        self.to_aabbs_uncached()
    }

    /// `VoxelShape.min(Direction.Axis, double, double)`.
    pub fn min_bc(&self, a_axis: Axis, b: f64, c: f64) -> f64 {
        let b_axis = AxisCycle::Forward.cycle_axis(a_axis);
        let c_axis = AxisCycle::Backward.cycle_axis(a_axis);
        let bi = self.find_index(b_axis, b);
        let ci = self.find_index(c_axis, c);
        let i = self.0.shape.first_full_bc(a_axis, bi, ci);
        if i >= self.0.shape.get_size(a_axis) {
            f64::INFINITY
        } else {
            self.get(a_axis, i)
        }
    }

    /// `VoxelShape.max(Direction.Axis, double, double)`.
    pub fn max_bc(&self, a_axis: Axis, b: f64, c: f64) -> f64 {
        let b_axis = AxisCycle::Forward.cycle_axis(a_axis);
        let c_axis = AxisCycle::Backward.cycle_axis(a_axis);
        let bi = self.find_index(b_axis, b);
        let ci = self.find_index(c_axis, c);
        let i = self.0.shape.last_full_bc(a_axis, bi, ci);
        if i <= 0 {
            f64::NEG_INFINITY
        } else {
            self.get(a_axis, i)
        }
    }

    /// `VoxelShape.findIndex(Direction.Axis, double)` (protected; overridden
    /// by `CubeVoxelShape`).
    pub fn find_index(&self, axis: Axis, coord: f64) -> i32 {
        if let Some(index) = self.0.behavior.find_index(&*self.0.shape, axis, coord) {
            return index;
        }

        let value = coord;
        let (values, offset) = match axis {
            Axis::X => (&self.0.root_coordinates_x, self.0.offset_x),
            Axis::Y => (&self.0.root_coordinates_y, self.0.offset_y),
            Axis::Z => (&self.0.root_coordinates_z, self.0.offset_z),
        };
        collision_util::find_floor(values, offset, value, 0, values.len() as i32 - 1)
    }

    /// `VoxelShape.clip(Vec3, Vec3, BlockPos)`.
    pub fn clip(&self, from: &Vec3, to: &Vec3, pos: &BlockPos) -> Option<BlockHitResult> {
        if self.0.is_empty {
            return None;
        }

        let direction_opposite = to.subtract(from);
        if direction_opposite.length_sqr() < COLLISION_EPSILON {
            return None;
        }

        let from_behind = from.add(&direction_opposite.scale(0.001));
        let from_behind_offset_x = from_behind.x - pos.get_x() as f64;
        let from_behind_offset_y = from_behind.y - pos.get_y() as f64;
        let from_behind_offset_z = from_behind.z - pos.get_z() as f64;

        if let Some(single_aabb) = &self.0.single_aabb_representation {
            if single_aabb.contains_xyz(
                from_behind_offset_x,
                from_behind_offset_y,
                from_behind_offset_z,
            ) {
                return Some(BlockHitResult::new(
                    from_behind,
                    Direction::get_approximate_nearest(
                        direction_opposite.x,
                        direction_opposite.y,
                        direction_opposite.z,
                    )
                    .get_opposite(),
                    *pos,
                    true,
                ));
            }
            return VoxelShape::clip_aabb(single_aabb, from, to, pos);
        }

        if collision_util::strictly_contains(
            self,
            from_behind_offset_x,
            from_behind_offset_y,
            from_behind_offset_z,
        ) {
            return Some(BlockHitResult::new(
                from_behind,
                Direction::get_approximate_nearest(
                    direction_opposite.x,
                    direction_opposite.y,
                    direction_opposite.z,
                )
                .get_opposite(),
                *pos,
                true,
            ));
        }

        AABB::clip_all(&self.to_aabbs(), from, to, pos)
    }

    /// `VoxelShape.closestPointTo(Vec3)`.
    pub fn closest_point_to(&self, point: &Vec3) -> Option<Vec3> {
        if self.0.is_empty {
            return None;
        }

        let mut ret = None;
        let mut ret_distance = f64::MAX;

        let aabbs = self.to_aabbs();
        for aabb in aabbs.iter() {
            let x = mth::clamp_f64(point.x, aabb.min_x, aabb.max_x);
            let y = mth::clamp_f64(point.y, aabb.min_y, aabb.max_y);
            let z = mth::clamp_f64(point.z, aabb.min_z, aabb.max_z);

            let dist = point.distance_to_sqr_xyz(x, y, z);
            if dist < ret_distance {
                ret = Some(Vec3::new(x, y, z));
                ret_distance = dist;
            }
        }

        ret
    }

    /// `VoxelShape.getFaceShape(Direction)`.
    pub fn get_face_shape(&self, direction: Direction) -> VoxelShape {
        if !self.is_empty() && !self.ptr_eq(&shapes::block()) {
            let cached = self.0.faces.lock().unwrap()[direction as usize].clone();
            if let Some(face) = self.face_slot(cached) {
                return face;
            }

            let face = self.calculate_face(direction);
            let slot = self.make_face_slot(&face);
            self.0.faces.lock().unwrap()[direction as usize] = Some(slot);
            face
        } else {
            self.clone()
        }
    }

    fn calculate_face(&self, direction: Direction) -> VoxelShape {
        let axis = direction.get_axis();
        match axis {
            Axis::X => self.calculate_face_direct(
                direction,
                axis,
                &self.0.root_coordinates_x,
                self.0.offset_x,
            ),
            Axis::Y => self.calculate_face_direct(
                direction,
                axis,
                &self.0.root_coordinates_y,
                self.0.offset_y,
            ),
            Axis::Z => self.calculate_face_direct(
                direction,
                axis,
                &self.0.root_coordinates_z,
                self.0.offset_z,
            ),
        }
    }

    /// `VoxelShape.isCubeLike()` (protected).
    pub fn is_cube_like(&self) -> bool {
        for axis in Axis::VALUES {
            if !self.is_cube_like_along(axis) {
                return false;
            }
        }

        true
    }

    fn is_cube_like_along(&self, axis: Axis) -> bool {
        let coords = self.get_coords(axis);
        coords.size() == 2
            && fuzzy_equals(coords.get_double(0), 0.0, 1.0E-7)
            && fuzzy_equals(coords.get_double(1), 1.0, 1.0E-7)
    }

    // Paper start - optimise collisions
    /// `VoxelShape.collide(Direction.Axis, AABB, double)`.
    pub fn collide(&self, axis: Axis, source: &AABB, source_move: f64) -> f64 {
        if self.0.is_empty {
            return source_move;
        }
        if source_move.abs() < COLLISION_EPSILON {
            return 0.0;
        }
        match axis {
            Axis::X => collision_util::collide_x(self, source, source_move),
            Axis::Y => collision_util::collide_y(self, source, source_move),
            Axis::Z => collision_util::collide_z(self, source, source_move),
        }
    }
    // Paper end - optimise collisions

    /// `VoxelShape.collideX(AxisCycle, AABB, double)` (protected).
    pub fn collide_x_cycle(&self, transform: AxisCycle, moving: &AABB, mut distance: f64) -> f64 {
        if self.is_empty() {
            return distance;
        }

        if distance.abs() < 1.0E-7 {
            return 0.0;
        }

        let inverse = transform.inverse();
        let a_axis = inverse.cycle_axis(Axis::X);
        let b_axis = inverse.cycle_axis(Axis::Y);
        let c_axis = inverse.cycle_axis(Axis::Z);
        let max_a = moving.max(a_axis);
        let min_a = moving.min(a_axis);
        let a_min = self.find_index(a_axis, min_a + 1.0E-7);
        let a_max = self.find_index(a_axis, max_a - 1.0E-7);
        let b_min = 0.max(self.find_index(b_axis, moving.min(b_axis) + 1.0E-7));
        let b_max = self
            .0
            .shape
            .get_size(b_axis)
            .min(self.find_index(b_axis, moving.max(b_axis) - 1.0E-7) + 1);
        let c_min = 0.max(self.find_index(c_axis, moving.min(c_axis) + 1.0E-7));
        let c_max = self
            .0
            .shape
            .get_size(c_axis)
            .min(self.find_index(c_axis, moving.max(c_axis) - 1.0E-7) + 1);
        let a_size = self.0.shape.get_size(a_axis);
        if distance > 0.0 {
            for a in (a_max + 1)..a_size {
                for b in b_min..b_max {
                    for c in c_min..c_max {
                        if self.0.shape.is_full_wide_cycle(inverse, a, b, c) {
                            let new_distance = self.get(a_axis, a) - max_a;
                            if new_distance >= -1.0E-7 {
                                distance = mth::min_f64(distance, new_distance);
                            }

                            return distance;
                        }
                    }
                }
            }
        } else if distance < 0.0 {
            let mut a = a_min - 1;
            while a >= 0 {
                for b in b_min..b_max {
                    for c in c_min..c_max {
                        if self.0.shape.is_full_wide_cycle(inverse, a, b, c) {
                            let new_distance = self.get(a_axis, a + 1) - min_a;
                            if new_distance <= 1.0E-7 {
                                distance = mth::max_f64(distance, new_distance);
                            }

                            return distance;
                        }
                    }
                }
                a -= 1;
            }
        }

        distance
    }
}

impl fmt::Display for VoxelShape {
    /// `VoxelShape.toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            f.write_str("EMPTY")
        } else {
            write!(f, "VoxelShape[{}]", self.bounds())
        }
    }
}

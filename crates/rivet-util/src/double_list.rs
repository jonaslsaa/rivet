//! `it.unimi.dsi.fastutil.doubles.DoubleList` — the subset of fastutil's
//! primitive double list that `net.minecraft.world.phys.shapes` uses, plus
//! `DoubleArrayList` with the sharing semantics the Moonrise collision code
//! relies on.
//!
//! `DoubleArrayList.wrap(double[])` shares the caller's array, and
//! `VoxelShape.extractRawArray`/`CollisionUtil.MergedVoxelCoordinateList.merge`
//! compare those backing arrays by *reference identity* (`firstCoordinates ==
//! secondCoordinates`). The backing array is therefore an `Arc<[f64]>` and
//! identity is `Arc::ptr_eq`.
//!
//! Java's `DoubleLists.unmodifiable(list)` (`IndirectMerger.EMPTY`) has no
//! observable difference from the wrapped list here, so it is the list itself.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// `DoubleList` — a read-only random-access list of primitive doubles.
///
/// `getDouble(int)` of an out-of-range index throws in Java; here it panics.
pub trait DoubleList: Send + Sync + fmt::Debug {
    /// `DoubleList.getDouble(int)`.
    fn get_double(&self, index: usize) -> f64;

    /// `DoubleList.size()`.
    fn size(&self) -> usize;

    /// `DoubleList.toDoubleArray()`.
    fn to_double_array(&self) -> Vec<f64> {
        (0..self.size()).map(|i| self.get_double(i)).collect()
    }

    /// `instanceof` support.
    fn as_any(&self) -> &dyn Any;
}

/// A shared `DoubleList` reference (Java object reference).
pub type DoubleListRef = Arc<dyn DoubleList>;

/// `AbstractDoubleList.equals(Object)` for two `DoubleList`s: equal sizes and
/// `==` on every primitive pair (so `NaN` never equals and `0.0 == -0.0`).
/// Callers that mean Java `Objects.equals` check reference identity first.
pub fn double_list_equals(a: &dyn DoubleList, b: &dyn DoubleList) -> bool {
    let size = a.size();
    if size != b.size() {
        return false;
    }
    for i in 0..size {
        if a.get_double(i) != b.get_double(i) {
            return false;
        }
    }
    true
}

/// `DoubleArrayList` — a double list over a (possibly shared) backing array
/// and a logical size.
#[derive(Clone, Debug)]
pub struct DoubleArrayList {
    a: Arc<[f64]>,
    size: usize,
}

impl DoubleArrayList {
    /// `new DoubleArrayList(double[])` — copies the array.
    pub fn from_slice(a: &[f64]) -> DoubleArrayList {
        DoubleArrayList {
            a: Arc::from(a),
            size: a.len(),
        }
    }

    /// The list a `new DoubleArrayList(n)` + `add(double)` sequence builds.
    pub fn from_vec(values: Vec<f64>) -> DoubleArrayList {
        let size = values.len();
        DoubleArrayList {
            a: Arc::from(values),
            size,
        }
    }

    /// `DoubleArrayList.wrap(double[])` — shares the array.
    pub fn wrap(a: Arc<[f64]>) -> DoubleArrayList {
        let size = a.len();
        DoubleArrayList { a, size }
    }

    /// `DoubleArrayList.wrap(double[], int length)` — shares the array.
    pub fn wrap_with_length(a: Arc<[f64]>, length: usize) -> DoubleArrayList {
        assert!(
            length <= a.len(),
            "The specified length ({length}) is greater than the array size ({})",
            a.len()
        );
        DoubleArrayList { a, size: length }
    }

    /// `DoubleArrayList.elements()` — the backing array (which can be longer
    /// than `size()`).
    pub fn elements(&self) -> &Arc<[f64]> {
        &self.a
    }
}

impl DoubleList for DoubleArrayList {
    fn get_double(&self, index: usize) -> f64 {
        assert!(
            index < self.size,
            "Index ({index}) is greater than or equal to list size ({})",
            self.size
        );
        self.a[index]
    }

    fn size(&self) -> usize {
        self.size
    }

    fn to_double_array(&self) -> Vec<f64> {
        self.a[..self.size].to_vec()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

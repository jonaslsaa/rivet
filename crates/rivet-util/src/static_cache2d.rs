//! `net.minecraft.util.StaticCache2D` — a fixed-size 2D cache built once at
//! construction.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/util/StaticCache2D.java`.
//! A square (or, via the private constructor, rectangular) grid of
//! pre-initialized entries over an `(x, z)` coordinate window. Reads are
//! O(1) array indexing; out-of-window reads throw (Java
//! `IllegalArgumentException`), because the window is fixed at construction
//! and there is no on-demand initialization path.
//!
//! [`get_mut`](StaticCache2D::get_mut) is a Rust-only addition: Java's
//! `StaticCache2D` is only ever read after construction, but `WorldGenRegion`
//! mutates a cached `GenerationChunkHolder`'s chunk through `setBlock`, and
//! the port cannot express Java's aliasing (a shared reference plus an
//! in-place write) without a mutable accessor. The window/`get` semantics are
//! unchanged.

/// A fixed, pre-initialized 2D cache over a coordinate window.
///
/// `T` is stored by value; the cache owns every entry. `create` fills the
/// window by calling `initializer` for every `(x, z)` in `[center - range,
/// center + range]` inclusive (a `(2 * range + 1)`-square).
pub struct StaticCache2D<T> {
    /// The inclusive window's minimum x (chunk/coordinate units).
    min_x: i32,
    /// The inclusive window's minimum z.
    min_z: i32,
    /// The number of x steps in the window.
    size_x: i32,
    /// The number of z steps in the window.
    size_z: i32,
    /// The entries, row-major over `(x, z)`.
    cache: Vec<T>,
}

impl<T> StaticCache2D<T> {
    /// `StaticCache2D.create(int centerX, int centerZ, int range,
    /// Initializer<T>)` — a square cache centered on `(centerX, centerZ)` with
    /// the given radius.
    pub fn create(
        center_x: i32,
        center_z: i32,
        range: i32,
        initializer: &dyn Fn(i32, i32) -> T,
    ) -> Self {
        let min_x = center_x - range;
        let min_z = center_z - range;
        let size = 2 * range + 1;
        Self::new(min_x, min_z, size, size, initializer)
    }

    /// A `StaticCache2D` over a pre-built window — the `create`-equivalent for
    /// entries that cannot be produced by an `&dyn Fn` initializer.
    ///
    /// The `&dyn Fn` initializer [`create`](Self::create) forces every entry to
    /// be produced inside a closure that cannot escape a captured borrow, so a
    /// window holding borrow-carrying entries (the worldgen region's
    /// `Box<dyn View + 'a>` holders) has no `create` spelling. `entries` is
    /// stored verbatim in the same row-major `(x, z)` order `create` fills
    /// (x-outer, z-inner), and the window bounds are supplied directly.
    pub fn from_entries(min_x: i32, min_z: i32, size_x: i32, size_z: i32, entries: Vec<T>) -> Self {
        assert_eq!(
            entries.len() as i32,
            size_x * size_z,
            "StaticCache2D window {size_x}x{size_z} requires {} entries, got {}",
            size_x * size_z,
            entries.len()
        );
        StaticCache2D {
            min_x,
            min_z,
            size_x,
            size_z,
            cache: entries,
        }
    }

    /// The private Java constructor — a rectangular window `[minX, minX +
    /// sizeX)` by `[minZ, minZ + sizeZ)`, filled eagerly.
    fn new(
        min_x: i32,
        min_z: i32,
        size_x: i32,
        size_z: i32,
        initializer: &dyn Fn(i32, i32) -> T,
    ) -> Self {
        let mut cache = Vec::with_capacity((size_x * size_z) as usize);
        for x in min_x..min_x + size_x {
            for z in min_z..min_z + size_z {
                cache.push(initializer(x, z));
            }
        }
        StaticCache2D {
            min_x,
            min_z,
            size_x,
            size_z,
            cache,
        }
    }

    /// `StaticCache2D.get(int x, int z)` — the entry at `(x, z)`. Throws
    /// (`IllegalArgumentException` panic) when the coordinate is outside the
    /// fixed window.
    pub fn get(&self, x: i32, z: i32) -> &T {
        if !self.contains(x, z) {
            panic!("Requested out of range value ({},{}) from {}", x, z, self);
        }
        &self.cache[self.get_index(x, z)]
    }

    /// Rust-only mutable half of [`get`](StaticCache2D::get) — the same
    /// window guard and indexing, returning `&mut T` for in-place writes (see
    /// the module doc).
    pub fn get_mut(&mut self, x: i32, z: i32) -> &mut T {
        if !self.contains(x, z) {
            panic!("Requested out of range value ({},{}) from {}", x, z, self);
        }
        let index = self.get_index(x, z);
        &mut self.cache[index]
    }

    /// Consume the cache and return its entries in deterministic X-major,
    /// Z-minor storage order. Rust-only ownership seam for callers that move
    /// borrow-carrying cache entries back into their owner after a bounded
    /// operation (for example, the generated FEATURES workspace).
    pub fn into_entries(self) -> Vec<T> {
        self.cache
    }

    /// `StaticCache2D.contains(int x, int z)` — whether the coordinate is
    /// inside the window.
    pub fn contains(&self, x: i32, z: i32) -> bool {
        let delta_x = x.wrapping_sub(self.min_x);
        let delta_z = z.wrapping_sub(self.min_z);
        delta_x >= 0 && delta_x < self.size_x && delta_z >= 0 && delta_z < self.size_z
    }

    /// `StaticCache2D.forEach(Consumer<T>)` — every entry in row-major order.
    pub fn for_each(&self, mut consumer: impl FnMut(&T)) {
        for entry in &self.cache {
            consumer(entry);
        }
    }

    /// Consume the cache and return its entries in the canonical x-major,
    /// z-inner order used by `create` and `from_entries`.
    pub fn into_entries(self) -> Vec<T> {
        self.cache
    }

    /// `getIndex(int x, int z)` — the row-major index `(x - minX) * sizeZ +
    /// (z - minZ)` (Java's `deltaX * this.sizeZ + deltaZ`; the port iterates
    /// x-major so the index formula matches Java's column-major storage).
    fn get_index(&self, x: i32, z: i32) -> usize {
        let delta_x = x.wrapping_sub(self.min_x);
        let delta_z = z.wrapping_sub(self.min_z);
        (delta_x * self.size_z + delta_z) as usize
    }
}

/// `StaticCache2D.toString()` — `StaticCache2D[minX, minZ, maxX, maxZ]`
/// (the window's exclusive max, matching Java's `minX + sizeX`).
impl<T> std::fmt::Display for StaticCache2D<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StaticCache2D[{}, {}, {}, {}]",
            self.min_x,
            self.min_z,
            self.min_x + self.size_x,
            self.min_z + self.size_z
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_square_window_and_reads() {
        // `StaticCache2D.create(0, 0, 2, ...)` → [-2, 2] square.
        let cache = StaticCache2D::create(0, 0, 2, &|x, z| (x, z));
        assert!(cache.contains(0, 0));
        assert!(cache.contains(-2, -2));
        assert!(cache.contains(2, 2));
        assert!(!cache.contains(-3, 0));
        assert!(!cache.contains(0, 3));
        assert_eq!(*cache.get(2, 2), (2, 2));
        assert_eq!(*cache.get(-2, -2), (-2, -2));
        assert_eq!(*cache.get(0, 0), (0, 0));
    }

    #[test]
    fn contains_uses_wrapping_coordinate_deltas() {
        let cache = StaticCache2D::from_entries(i32::MIN, i32::MIN, 1, 1, vec![()]);
        assert!(cache.contains(i32::MIN, i32::MIN));
        assert!(!cache.contains(i32::MAX, i32::MAX));
        let message = std::panic::catch_unwind(|| cache.get(i32::MAX, i32::MAX))
            .expect_err("out-of-range get must use the Java-style bounds failure")
            .downcast::<String>()
            .unwrap();
        assert!(message.contains("Requested out of range value"));
    }

    #[test]
    fn get_out_of_window_panics_like_java() {
        let cache = StaticCache2D::create(0, 0, 1, &|x, z| (x, z));
        let message = std::panic::catch_unwind(|| cache.get(2, 0))
            .expect_err("out-of-range get must panic")
            .downcast::<String>()
            .unwrap();
        assert!(message.contains("Requested out of range value (2,0)"));
        assert!(message.contains("StaticCache2D[-1, -1, 2, 2]"));
    }

    #[test]
    fn for_each_yields_row_major_order() {
        // A 2x1 window (minX 0, sizeX 2; minZ 0, sizeZ 1).
        let cache = StaticCache2D::new(0, 0, 2, 1, &|x, z| (x, z));
        let mut seen = Vec::new();
        cache.for_each(|entry| seen.push(*entry));
        // Java fills x-outer, z-inner, so (0,0) then (1,0).
        assert_eq!(seen, vec![(0, 0), (1, 0)]);
    }

    #[test]
    fn get_mut_updates_in_place() {
        let mut cache = StaticCache2D::create(0, 0, 1, &|_x, _z| 0);
        *cache.get_mut(1, 1) = 7;
        assert_eq!(*cache.get(1, 1), 7);
        assert_eq!(*cache.get(0, 0), 0);
        let message = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = cache.get_mut(2, 0);
        }))
        .expect_err("out-of-range get_mut must panic")
        .downcast::<String>()
        .unwrap();
        assert!(message.contains("Requested out of range value (2,0)"));
    }

    #[test]
    fn to_string_matches_java_layout() {
        let cache = StaticCache2D::create(5, -3, 2, &|_x, _z| ());
        // minX 3, minZ -5, maxX 8, maxZ 0 (`minZ + size = -5 + 5`).
        assert_eq!(cache.to_string(), "StaticCache2D[3, -5, 8, 0]");
        // Display renders the same string.
        assert_eq!(format!("{cache}"), "StaticCache2D[3, -5, 8, 0]");
    }
}

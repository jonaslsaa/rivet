//! Port of `net.minecraft.server.level.ChunkTrackingView` (MC 26.2, Paper) — the
//! square view-distance containment contract.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/ChunkTrackingView.java`.
//!
//! Owned by the `mc.server.level.pipeline.view` manifest unit (#185). Ported
//! ahead of that unit because issue #100 needs the exact Moonrise
//! view-distance-4 shape: center `(0,0)`, radius 4 → the 11×11 square minus the
//! four corners = **117 chunks** (`isWithinDistance` with `includeNeighbors`,
//! where `bufferRange = 2` shaves the corners: `(-5,-5)`, `(5,-5)`, `(-5,5)`,
//! `(5,5)` fall at distance² = 3² + 3² = 18 ≥ 16). This is the fixed chunk square
//! `Event::ReceiveChunk` lands with (issue #150 DoD).
//!
//! RivetTodo(#185): the EMPTY view, the `Positioned.difference` enter/leave
//! diff walker (Moonrise recenter), and the `isInViewDistance` overloads are
//! deferred to the owning pipeline.view unit. This slice ports only the
//! `Positioned` containment + `forEach` shape the M1 chunk square needs.

use rivet_registry::core::ChunkPos;

/// `ChunkTrackingView.Positioned(ChunkPos center, int viewDistance)` — the
/// square view-distance containment value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkTrackingView {
    center: ChunkPos,
    view_distance: i32,
}

impl ChunkTrackingView {
    /// `ChunkTrackingView.of(ChunkPos center, int radius)`.
    pub fn of(center: ChunkPos, view_distance: i32) -> Self {
        ChunkTrackingView {
            center,
            view_distance,
        }
    }

    /// `Positioned.center()`.
    pub fn center(&self) -> ChunkPos {
        self.center
    }

    /// `Positioned.viewDistance()`.
    pub fn view_distance(&self) -> i32 {
        self.view_distance
    }

    /// `Positioned.minX()` — `center.x() - viewDistance - 1` in wrapping int
    /// arithmetic (PORTING.md: Java `int` arithmetic wraps on overflow).
    fn min_x(&self) -> i32 {
        self.center
            .x()
            .wrapping_sub(self.view_distance)
            .wrapping_sub(1)
    }

    /// `Positioned.minZ()`.
    fn min_z(&self) -> i32 {
        self.center
            .z()
            .wrapping_sub(self.view_distance)
            .wrapping_sub(1)
    }

    /// `Positioned.maxX()`.
    fn max_x(&self) -> i32 {
        self.center
            .x()
            .wrapping_add(self.view_distance)
            .wrapping_add(1)
    }

    /// `Positioned.maxZ()`.
    fn max_z(&self) -> i32 {
        self.center
            .z()
            .wrapping_add(self.view_distance)
            .wrapping_add(1)
    }

    /// `ChunkTrackingView.isWithinDistance(centerX, centerZ, viewDistance,
    /// chunkX, chunkZ, includeNeighbors)` — the only containment formula the
    /// whole class reduces to.
    ///
    /// Java:
    /// ```java
    /// int bufferRange = includeNeighbors ? 2 : 1;
    /// long deltaX = Math.max(0, Math.abs(chunkX - centerX) - bufferRange);
    /// long deltaZ = Math.max(0, Math.abs(chunkZ - centerZ) - bufferRange);
    /// long distanceSquared = deltaX * deltaX + deltaZ * deltaZ;
    /// int radiusSquared = viewDistance * viewDistance;
    /// return distanceSquared < radiusSquared;
    /// ```
    ///
    /// With `viewDistance = 4` and `includeNeighbors = true` (the `contains`
    /// two-arg default) the corners `(-5,-5)` etc. are excluded: their
    /// `deltaX/deltaZ = |5| - 2 = 3`, `3² + 3² = 18 ≥ 16`. The `viewDistance`
    /// bounds (with the neighbor margin) are `-5..5` in both axes, so the shape
    /// is exactly 11×11 minus the four corners = 117 chunks.
    pub fn is_within_distance(
        center_x: i32,
        center_z: i32,
        view_distance: i32,
        chunk_x: i32,
        chunk_z: i32,
        include_neighbors: bool,
    ) -> bool {
        let buffer_range = if include_neighbors { 2 } else { 1 };
        // Java:
        //   long deltaX = Math.max(0, Math.abs(chunkX - centerX) - bufferRange);
        //   long distanceSquared = deltaX * deltaX + deltaZ * deltaZ;
        //   int radiusSquared = viewDistance * viewDistance;
        // All three int steps (`chunk - center`, `Math.abs` of MIN_VALUE, the
        // `- bufferRange`) wrap like Java ints; the `max(0, …)` clamps the
        // possibly-negative delta to 0; then the delta widens to long for the
        // distance square. `viewDistance * viewDistance` also wraps as an int
        // before promoting to long for the comparison.
        let delta_x = i64::from(
            chunk_x
                .wrapping_sub(center_x)
                .wrapping_abs()
                .wrapping_sub(buffer_range)
                .max(0),
        );
        let delta_z = i64::from(
            chunk_z
                .wrapping_sub(center_z)
                .wrapping_abs()
                .wrapping_sub(buffer_range)
                .max(0),
        );
        let distance_squared = delta_x * delta_x + delta_z * delta_z;
        let radius_squared = i64::from(view_distance.wrapping_mul(view_distance));
        distance_squared < radius_squared
    }

    /// `contains(int chunkX, int chunkZ, boolean includeNeighbors)`.
    pub fn contains(&self, chunk_x: i32, chunk_z: i32, include_neighbors: bool) -> bool {
        Self::is_within_distance(
            self.center.x(),
            self.center.z(),
            self.view_distance,
            chunk_x,
            chunk_z,
            include_neighbors,
        )
    }

    /// `contains(ChunkPos)` — the two-arg default, `includeNeighbors = true`.
    pub fn contains_pos(&self, pos: &ChunkPos) -> bool {
        self.contains(pos.x(), pos.z(), true)
    }

    /// `isInViewDistance(int, int)` — `contains(x, z, false)`.
    pub fn is_in_view_distance(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.contains(chunk_x, chunk_z, false)
    }

    /// `Positioned.forEach(Consumer<ChunkPos>)` — the X-major, Z-minor walk
    /// over the inclusive `min..max` bounds, emitting only contained chunks.
    /// Deterministic order: the `-5..5` × `-5..5` raster skips the four
    /// corners, yielding the exact 117-chunk square the M1 send-set needs.
    ///
    /// **Termination deviation (deliberate):** Rust's `RangeInclusive` always
    /// terminates. Java's `for (int x = minX; x <= maxX; x++)` can loop forever
    /// when a bound is `i32::MAX` — the increment wraps to `i32::MIN`, which is
    /// still `<= maxX`. That edge is unreachable on the M1 join path (world view
    /// distances clamp to `[2, 32]`, so `±(view + 1)` never approaches
    /// `i32::MAX`); this port deliberately does not reproduce the hang.
    pub fn for_each(&self, mut f: impl FnMut(ChunkPos)) {
        for x in self.min_x()..=self.max_x() {
            for z in self.min_z()..=self.max_z() {
                if self.contains(x, z, true) {
                    f(ChunkPos::new(x, z));
                }
            }
        }
    }

    /// The number of chunks `for_each` emits — the fixed view square size (117
    /// for the M1 view-distance-4 send-set).
    pub fn chunk_count(&self) -> usize {
        let mut count = 0;
        self.for_each(|_| count += 1);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view-distance-4 square centered on the spawn chunk `(0,0)`.
    fn spawn_view() -> ChunkTrackingView {
        ChunkTrackingView::of(ChunkPos::ZERO, 4)
    }

    #[test]
    fn view_distance_4_center_zero_has_exactly_117_chunks() {
        // The issue #100/#150 DoD shape: 11×11 minus the four corners.
        let view = spawn_view();
        let mut count = 0;
        view.for_each(|_| count += 1);
        assert_eq!(count, 117);
    }

    #[test]
    fn view_distance_4_shape_is_eleven_by_eleven_minus_corners() {
        let view = spawn_view();
        // Every chunk in -5..5 x -5..5 is contained except the four corners.
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                let in_corner = x.abs() == 5 && z.abs() == 5;
                assert_eq!(view.contains(x, z, true), !in_corner, "chunk ({x},{z})");
            }
        }
        // `isInViewDistance` (`includeNeighbors = false`, bufferRange = 1) is the
        // one-chunk-in margin: a ±4 square minus the four corners — `(±4,±4)` has
        // delta = |4| - 1 = 3, 3² + 3² = 18 ≥ 16. Only those four are cut.
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                let in_margin = x.abs() > 4 || z.abs() > 4 || (x.abs() == 4 && z.abs() == 4);
                assert_eq!(
                    view.is_in_view_distance(x, z),
                    !in_margin,
                    "in-view ({x},{z})"
                );
            }
        }
    }

    #[test]
    fn contains_pos_is_the_include_neighbors_default() {
        // `contains(ChunkPos)` — the two-arg default `contains(x, z, true)`.
        let view = spawn_view();
        for x in -5i32..=5 {
            for z in -5i32..=5 {
                let in_corner = x.abs() == 5 && z.abs() == 5;
                assert_eq!(
                    view.contains_pos(&ChunkPos::new(x, z)),
                    !in_corner,
                    "contains_pos ({x},{z})"
                );
            }
        }
    }

    #[test]
    fn for_each_iterates_x_major_then_z_minor() {
        let view = spawn_view();
        let mut chunks = Vec::new();
        view.for_each(|pos| chunks.push(pos));
        assert_eq!(chunks.len(), 117);
        for window in chunks.windows(2) {
            let (a, b) = (window[0], window[1]);
            if a.x() == b.x() {
                // Same column: the contained z are contiguous, so consecutive.
                assert_eq!(a.z() + 1, b.z(), "bad order {a} -> {b}");
            } else {
                // New column to the right; z restarts at the first contained z.
                assert_eq!(a.x() + 1, b.x(), "bad order {a} -> {b}");
                // The ±5 columns start at -4 (the (5,-5) corner is cut); every
                // other column spans the full -5..5.
                let first_z = if b.x().abs() == 5 { -4 } else { -5 };
                assert_eq!(b.z(), first_z, "bad z-restart {a} -> {b}");
            }
        }
        assert_eq!(chunks[0], ChunkPos::new(-5, -5 + 1)); // (-5,-4): the first non-corner
        assert_eq!(*chunks.last().unwrap(), ChunkPos::new(5, 4)); // (5,4): last non-corner
    }

    #[test]
    fn zero_radius_view_contains_no_chunks() {
        // `isWithinDistance` with viewDistance 0 and includeNeighbors
        // (bufferRange = 2) is degenerate: even the center chunk has
        // delta = 0 → distance² = 0, and `0 < radius² = 0` is false, so nothing
        // is contained. Paper never uses radius 0 (`setServerViewDistance`
        // clamps to ≥ 2); this pins the faithful formula boundary.
        let view = ChunkTrackingView::of(ChunkPos::ZERO, 0);
        assert!(!view.contains(0, 0, true));
        assert!(!view.contains(1, 0, true));
        assert!(!view.contains(0, 1, true));
        assert_eq!(view.chunk_count(), 0);
    }

    #[test]
    fn extreme_coordinates_wrap_like_java_ints_not_panic() {
        // Java computes `Math.abs(chunk - center) - bufferRange` in wrapping int
        // arithmetic (so `Math.abs(i32::MIN)` stays negative, and the subtract
        // can wrap). This view is centered on (0,0) with radius 32, so every
        // i32-extreme coordinate wraps to a huge delta and `distance² < radius²`
        // is false — `contains` returns false without panicking on the
        // debug-build overflow. The "always false at extremes" reading is
        // specific to this near-zero center; a view centered on an extreme
        // coordinate could wrap into a small delta and return true.
        let view = ChunkTrackingView::of(ChunkPos::ZERO, 32);
        // Extreme coordinates on both axes: single-axis X, single-axis Z, and
        // both axes together.
        assert!(!view.contains(i32::MAX, 0, true));
        assert!(!view.contains(i32::MIN, 0, true));
        assert!(!view.contains(0, i32::MAX, true));
        assert!(!view.contains(0, i32::MIN, true));
        assert!(!view.contains(i32::MIN, i32::MIN, true));
        assert!(!view.contains(i32::MAX, i32::MAX, true));
        assert!(!view.contains(i32::MIN, i32::MAX, true));
        assert!(!view.contains(i32::MAX, i32::MIN, true));
        // Near center: contained.
        assert!(view.contains(0, 0, true));
        assert!(view.contains(-1, 0, true));
    }

    #[test]
    fn min_max_bounds_wrap_like_java_ints() {
        // Calibration: these exact wrapped values are the wrapping regression
        // guard. In debug builds a revert to plain `-`/`+` panics on i32
        // overflow, so the normal debug gate catches it. Release builds wrap
        // silently (like Java), so release tests would NOT distinguish
        // `wrapping_sub` from `-` — the debug gate (not release) pins the
        // wrapping.
        //
        // `Positioned.minX()` is `center.x() - viewDistance - 1` and `maxX()` is
        // `center.x() + viewDistance + 1`, each step a wrapping int
        // operation. At i32 extremes the subtraction/addition wraps.
        // Center = i32::MIN, view 1: minX = (i32::MIN - 1) - 1 = i32::MAX - 1;
        // maxX = (i32::MIN + 1) + 1 = i32::MIN + 2.
        let min_center = ChunkTrackingView::of(ChunkPos::new(i32::MIN, 0), 1);
        assert_eq!(min_center.min_x(), i32::MAX - 1);
        assert_eq!(min_center.max_x(), i32::MIN + 2);
        assert_eq!(min_center.min_z(), -2);
        assert_eq!(min_center.max_z(), 2);
        // Center = i32::MAX, view 1: minX = (i32::MAX - 1) - 1 = i32::MAX - 2;
        // maxX = (i32::MAX + 1) + 1 = i32::MIN + 1.
        let max_center = ChunkTrackingView::of(ChunkPos::new(i32::MAX, 0), 1);
        assert_eq!(max_center.min_x(), i32::MAX - 2);
        assert_eq!(max_center.max_x(), i32::MIN + 1);
        assert_eq!(max_center.min_z(), -2);
        assert_eq!(max_center.max_z(), 2);
        // View 0 stays exact (no overflow): (-1, 1) both axes.
        let zero = ChunkTrackingView::of(ChunkPos::ZERO, 0);
        assert_eq!(zero.min_x(), -1);
        assert_eq!(zero.max_x(), 1);
        assert_eq!(zero.min_z(), -1);
        assert_eq!(zero.max_z(), 1);
    }

    #[test]
    fn wrapped_empty_view_iterates_nothing() {
        // When the wrapped bounds put minX > maxX, Rust's `RangeInclusive` is an
        // empty iteration — zero chunks, no panic, no off-by-one. (The
        // infinite-loop hazard of Java's `for (int x = minX; x <= maxX; x++)`
        // is a separate, deliberately-not-reproduced edge — see `for_each`.)
        // Both i32::MIN and i32::MAX centers with view 1 wrap to an empty X
        // range.
        let min_center = ChunkTrackingView::of(ChunkPos::new(i32::MIN, 0), 1);
        assert_eq!(min_center.chunk_count(), 0);
        let max_center = ChunkTrackingView::of(ChunkPos::new(i32::MAX, 0), 1);
        assert_eq!(max_center.chunk_count(), 0);
    }
}

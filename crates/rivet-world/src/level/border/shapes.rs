//! STUB(mc.world.phys.shapes) — minimal `VoxelShape`/`Shapes`/`BooleanOp`
//! seams for the pending `net.minecraft.world.phys.shapes` unit, created only
//! so `WorldBorder`'s collision shape compiles. The real port (with the full
//! `VoxelShape` bitset/octree model and `Shapes.join`/`Shapes.box`) replaces
//! these when the unit lands.

/// `net.minecraft.world.phys.shapes.VoxelShape` — the border wall shape.
///
/// STUB(mc.world.phys.shapes): the full voxel shape is not ported. The border
/// only *holds* the shape (the collision consumer — `Level.isUnobstructed`
/// etc. — is a separate seam).
///
/// The four corners stored here are the corners of the REMOVED inner box (the
/// hole), NOT the wall's extent. Java's collision shape is
/// `Shapes.join(Shapes.INFINITY, Shapes.box(floor(minX), NEG_INF, floor(minZ),
/// ceil(maxX), POS_INF, ceil(maxZ)), BooleanOp.ONLY_FIRST)` — the infinite
/// solid MINUS that box (a hollow wall; `ONLY_FIRST = (first, second) -> first
/// && !second`). A future real `Shapes` port must reconstruct the wall from
/// `INFINITY + box + ONLY_FIRST` — never a solid box built from these corners,
/// which would be the interior hole, the exact inverse of the Java wall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelShape {
    /// The removed box's x0 corner (Java `Shapes.box(...)` `minX`).
    pub(crate) min_x: f64,
    /// The removed box's z0 corner (Java `Shapes.box(...)` `minZ`).
    pub(crate) min_z: f64,
    /// The removed box's x1 corner (Java `Shapes.box(...)` `maxX`).
    pub(crate) max_x: f64,
    /// The removed box's z1 corner (Java `Shapes.box(...)` `maxZ`).
    pub(crate) max_z: f64,
}

impl VoxelShape {
    /// `Shapes.INFINITY` — `Shapes.box(NEG_INF, NEG_INF, NEG_INF, POS_INF,
    /// POS_INF, POS_INF)`, the full infinite solid. The border wall derived
    /// from it is `INFINITY AND NOT box` (see [`Shapes::border_wall`]), not
    /// this shape itself.
    pub const INFINITY: VoxelShape = VoxelShape {
        min_x: f64::NEG_INFINITY,
        min_z: f64::NEG_INFINITY,
        max_x: f64::INFINITY,
        max_z: f64::INFINITY,
    };
}

/// `net.minecraft.world.phys.shapes.Shapes` — the shape helpers.
pub struct Shapes;

impl Shapes {
    /// `Shapes.join(Shapes.INFINITY, Shapes.box(x0, NEG_INF, z0, x1,
    /// POS_INF, z1), BooleanOp.ONLY_FIRST)` — the border wall. Java's
    /// `ONLY_FIRST = (first, second) -> first && !second`, so the result is
    /// the infinite solid with the inner box carved out (a hollow wall). The
    /// port keeps the `ONLY_FIRST` box corners (Java floors/ceils the min/max
    /// and spans the full y range) — see [`VoxelShape`]: these corners are the
    /// removed box (the hole), not the wall's extent.
    ///
    /// Stub divergence: Java's `Shapes.box` throws `IllegalArgumentException`
    /// when a min coordinate exceeds its max, so a negative border size (of
    /// sufficient magnitude that `floor(minX) > ceil(maxX)`) throws inside
    /// `StaticBorderExtent.updateBox()`. This stub stores the degenerate shape
    /// silently; the real `Shapes.box` port must replicate that validation.
    pub fn border_wall(min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> VoxelShape {
        VoxelShape {
            min_x,
            min_z,
            max_x,
            max_z,
        }
    }
}

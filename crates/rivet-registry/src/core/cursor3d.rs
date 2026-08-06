//! `net.minecraft.core.Cursor3D` — iterates the cells of an axis-aligned box
//! in X/Y/Z-major order.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/Cursor3D.java`.
//! Used by `SectionPos.betweenClosedStream`; the `getNextType` (INSIDE/FACE/
//! EDGE/CORNER) classification is part of the value surface.

/// `Cursor3D` — iterator over the cells of `[minX..maxX] × [minY..maxY] ×
/// [minZ..maxZ]` (inclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cursor3D {
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    width: i32,
    height: i32,
    depth: i32,
    end: i32,
    index: i32,
    x: i32,
    y: i32,
    z: i32,
}

impl Cursor3D {
    /// `Cursor3D.TYPE_INSIDE`.
    pub const TYPE_INSIDE: i32 = 0;
    /// `Cursor3D.TYPE_FACE`.
    pub const TYPE_FACE: i32 = 1;
    /// `Cursor3D.TYPE_EDGE`.
    pub const TYPE_EDGE: i32 = 2;
    /// `Cursor3D.TYPE_CORNER`.
    pub const TYPE_CORNER: i32 = 3;

    /// `new Cursor3D(minX, minY, minZ, maxX, maxY, maxZ)`.
    pub fn new(min_x: i32, min_y: i32, min_z: i32, max_x: i32, max_y: i32, max_z: i32) -> Self {
        let width = max_x.wrapping_sub(min_x).wrapping_add(1);
        let height = max_y.wrapping_sub(min_y).wrapping_add(1);
        let depth = max_z.wrapping_sub(min_z).wrapping_add(1);
        Self {
            origin_x: min_x,
            origin_y: min_y,
            origin_z: min_z,
            width,
            height,
            depth,
            // Java `width * height * depth` wraps (int overflow).
            end: width.wrapping_mul(height).wrapping_mul(depth),
            index: 0,
            x: 0,
            y: 0,
            z: 0,
        }
    }

    /// `Cursor3D.advance()` — false once the iteration is exhausted.
    pub fn advance(&mut self) -> bool {
        if self.index == self.end {
            return false;
        }
        self.x = self.index % self.width;
        let slice = self.index / self.width;
        self.y = slice % self.height;
        self.z = slice / self.height;
        self.index += 1;
        true
    }

    /// `Cursor3D.nextX()`.
    pub fn next_x(&self) -> i32 {
        self.origin_x.wrapping_add(self.x)
    }

    /// `Cursor3D.nextY()`.
    pub fn next_y(&self) -> i32 {
        self.origin_y.wrapping_add(self.y)
    }

    /// `Cursor3D.nextZ()`.
    pub fn next_z(&self) -> i32 {
        self.origin_z.wrapping_add(self.z)
    }

    /// `Cursor3D.getNextType()` — count of coordinates on a box boundary.
    pub fn get_next_type(&self) -> i32 {
        let mut ty = 0;
        if self.x == 0 || self.x == self.width - 1 {
            ty += 1;
        }
        if self.y == 0 || self.y == self.height - 1 {
            ty += 1;
        }
        if self.z == 0 || self.z == self.depth - 1 {
            ty += 1;
        }
        ty
    }
}

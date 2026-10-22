//! Inclusive integer structure bounds, including Java's wrapping arithmetic.

use rivet_registry::core::BlockPos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundingBox {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

impl BoundingBox {
    /// Paper normalizes inverted bounds (and emits a diagnostic).
    pub fn new(bounds: [i32; 6]) -> Self {
        let [x0, y0, z0, x1, y1, z1] = bounds;
        Self {
            min_x: x0.min(x1),
            min_y: y0.min(y1),
            min_z: z0.min(z1),
            max_x: x0.max(x1),
            max_y: y0.max(y1),
            max_z: z0.max(z1),
        }
    }

    pub fn at(pos: BlockPos) -> Self {
        Self::new([
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
        ])
    }

    pub fn to_array(self) -> [i32; 6] {
        [
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        ]
    }

    pub fn is_inside(self, pos: &BlockPos) -> bool {
        pos.get_x() >= self.min_x
            && pos.get_x() <= self.max_x
            && pos.get_y() >= self.min_y
            && pos.get_y() <= self.max_y
            && pos.get_z() >= self.min_z
            && pos.get_z() <= self.max_z
    }

    pub fn intersects(self, other: Self) -> bool {
        self.max_x >= other.min_x
            && self.min_x <= other.max_x
            && self.max_y >= other.min_y
            && self.min_y <= other.max_y
            && self.max_z >= other.min_z
            && self.min_z <= other.max_z
    }

    pub fn center(self) -> BlockPos {
        fn axis(min: i32, max: i32) -> i32 {
            min.wrapping_add(max.wrapping_sub(min).wrapping_add(1) / 2)
        }
        BlockPos::new(
            axis(self.min_x, self.max_x),
            axis(self.min_y, self.max_y),
            axis(self.min_z, self.max_z),
        )
    }

    pub fn inflated_by(self, amount: i32) -> Self {
        Self::new([
            self.min_x.wrapping_sub(amount),
            self.min_y.wrapping_sub(amount),
            self.min_z.wrapping_sub(amount),
            self.max_x.wrapping_add(amount),
            self.max_y.wrapping_add(amount),
            self.max_z.wrapping_add(amount),
        ])
    }

    pub fn encapsulate(&mut self, other: Self) {
        self.min_x = self.min_x.min(other.min_x);
        self.min_y = self.min_y.min(other.min_y);
        self.min_z = self.min_z.min(other.min_z);
        self.max_x = self.max_x.max(other.max_x);
        self.max_y = self.max_y.max(other.max_y);
        self.max_z = self.max_z.max(other.max_z);
    }
}

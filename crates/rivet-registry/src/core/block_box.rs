//! `net.minecraft.core.BlockBox` — an axis-aligned inclusive block box (a
//! record of two `BlockPos`), plus the `Iterable<BlockPos>` surface.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/BlockBox.java`.
//! The record canonical constructor normalizes `min`/`max` to the
//! component-wise `BlockPos.min`/`BlockPos.max`, so `BlockBox::new`
//! canonicalizes out-of-order corners and the accessors return the stored
//! (normalized) values. `iterator()` yields the cells of
//! `BlockPos.betweenClosed(min, max)` in X/Y/Z-major order — materialized as a
//! `Vec` like the other Java re-iterable surfaces in this crate.
//!
//! RivetTodo(#206): `aabb()` (`AABB.encapsulatingFullBlocks(min, max)`) returns
//! the `world.phys.AABB` value type, deferred with the JOML/math value types
//! in #206. RivetTodo(#126): `STREAM_CODEC` (two `BlockPos` stream codecs)
//! lives in `rivet-protocol`.

use super::block_pos::BlockPos;
use super::direction::{AxisDirection, Direction};
use super::vec3i::Vec3i;

/// `BlockBox` — an axis-aligned inclusive block box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockBox {
    min: BlockPos,
    max: BlockPos,
}

impl BlockBox {
    /// The record canonical constructor — normalizes the corners with
    /// `BlockPos.min`/`BlockPos.max`.
    pub fn new(min: BlockPos, max: BlockPos) -> BlockBox {
        BlockBox {
            min: BlockPos::min(&min, &max),
            max: BlockPos::max(&min, &max),
        }
    }

    /// `BlockBox.of(BlockPos)`.
    pub fn of(pos: BlockPos) -> BlockBox {
        BlockBox::new(pos, pos)
    }

    /// `BlockBox.of(BlockPos, BlockPos)`.
    pub fn of_two(a: BlockPos, b: BlockPos) -> BlockBox {
        BlockBox::new(a, b)
    }

    /// `BlockBox.min()` — the record accessor (already normalized).
    pub fn min(&self) -> BlockPos {
        self.min
    }

    /// `BlockBox.max()` — the record accessor (already normalized).
    pub fn max(&self) -> BlockPos {
        self.max
    }

    /// `BlockBox.include(BlockPos)` — the smallest box containing `this` and
    /// `pos`.
    pub fn include(&self, pos: BlockPos) -> BlockBox {
        BlockBox::new(
            BlockPos::min(&self.min, &pos),
            BlockPos::max(&self.max, &pos),
        )
    }

    /// `BlockBox.isBlock()` — `min.equals(max)`.
    pub fn is_block(&self) -> bool {
        self.min == self.max
    }

    /// `BlockBox.contains(BlockPos)` — inclusive on every corner.
    pub fn contains(&self, pos: &BlockPos) -> bool {
        pos.get_x() >= self.min.get_x()
            && pos.get_y() >= self.min.get_y()
            && pos.get_z() >= self.min.get_z()
            && pos.get_x() <= self.max.get_x()
            && pos.get_y() <= self.max.get_y()
            && pos.get_z() <= self.max.get_z()
    }

    /// `BlockBox.iterator()` — the cells of `BlockPos.betweenClosed(min, max)`.
    pub fn iterator(&self) -> Vec<BlockPos> {
        BlockPos::between_closed_pos(&self.min, &self.max)
    }

    /// `BlockBox.sizeX()` — `max.getX() - min.getX() + 1`.
    pub fn size_x(&self) -> i32 {
        self.max
            .get_x()
            .wrapping_sub(self.min.get_x())
            .wrapping_add(1)
    }

    /// `BlockBox.sizeY()` — `max.getY() - min.getY() + 1`.
    pub fn size_y(&self) -> i32 {
        self.max
            .get_y()
            .wrapping_sub(self.min.get_y())
            .wrapping_add(1)
    }

    /// `BlockBox.sizeZ()` — `max.getZ() - min.getZ() + 1`.
    pub fn size_z(&self) -> i32 {
        self.max
            .get_z()
            .wrapping_sub(self.min.get_z())
            .wrapping_add(1)
    }

    /// `BlockBox.extend(Direction, int)` — grows the box in one direction;
    /// `amount == 0` returns `this` unchanged. A positive-axis-direction grow
    /// extends the `max` corner, a negative one the `min` corner.
    pub fn extend(&self, direction: &Direction, amount: i32) -> BlockBox {
        if amount == 0 {
            return *self;
        }
        if direction.get_axis_direction() == AxisDirection::Positive {
            BlockBox::of_two(
                self.min,
                BlockPos::max(&self.min, &self.max.relative_steps(direction, amount)),
            )
        } else {
            BlockBox::of_two(
                BlockPos::min(&self.min.relative_steps(direction, amount), &self.max),
                self.max,
            )
        }
    }

    /// `BlockBox.move(Direction, int)` — shifts both corners; `amount == 0`
    /// returns `this` unchanged. (Rust `move` is a keyword, hence `moved`.)
    pub fn moved(&self, direction: &Direction, amount: i32) -> BlockBox {
        if amount == 0 {
            return *self;
        }
        BlockBox::new(
            self.min.relative_steps(direction, amount),
            self.max.relative_steps(direction, amount),
        )
    }

    /// `BlockBox.offset(Vec3i)`.
    pub fn offset(&self, offset: &Vec3i) -> BlockBox {
        BlockBox::new(self.min.offset_vec(offset), self.max.offset_vec(offset))
    }

    /// The Java record `hashCode` — `Objects.hash(min, max)`, which is
    /// `31 * (31 * 1 + min.hashCode()) + max.hashCode()` in wrapping int
    /// arithmetic (the record spec starts the accumulator at `1`, so the seed
    /// contributes `31^2 = 961`).
    pub fn hash_code(&self) -> i32 {
        961i32
            .wrapping_add(self.min.hash_code().wrapping_mul(31))
            .wrapping_add(self.max.hash_code())
    }
}

impl std::hash::Hash for BlockBox {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl std::fmt::Display for BlockBox {
    /// The Java record `toString()` — `BlockBox[min=BlockPos{x=…, y=…, z=…},
    /// max=BlockPos{x=…, y=…, z=…}]`, the components rendering via their own
    /// `BlockPos.toString`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlockBox[min={}, max={}]", self.min, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::BlockPos;

    fn bp(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(x, y, z)
    }

    #[test]
    fn new_normalizes_out_of_order_corners() {
        let b = BlockBox::new(bp(5, 10, 7), bp(-3, -4, 2));
        assert_eq!(b.min(), bp(-3, -4, 2));
        assert_eq!(b.max(), bp(5, 10, 7));
    }

    #[test]
    fn of_single_pos_is_a_block() {
        let b = BlockBox::of(bp(1, 2, 3));
        assert!(b.is_block());
        assert_eq!(b.min(), bp(1, 2, 3));
        assert_eq!(b.max(), bp(1, 2, 3));
    }

    #[test]
    fn of_two_accepts_swapped_corners() {
        let b = BlockBox::of_two(bp(9, 9, 9), bp(-9, -9, -9));
        assert_eq!(b.min(), bp(-9, -9, -9));
        assert_eq!(b.max(), bp(9, 9, 9));
    }

    #[test]
    fn include_expands_to_bounding_box() {
        let b = BlockBox::of(bp(0, 0, 0))
            .include(bp(3, -2, 5))
            .include(bp(-1, 4, 2));
        assert_eq!(b.min(), bp(-1, -2, 0));
        assert_eq!(b.max(), bp(3, 4, 5));
        assert!(!b.is_block());
    }

    #[test]
    fn contains_is_inclusive_on_all_faces() {
        let b = BlockBox::of_two(bp(0, 0, 0), bp(3, 3, 3));
        assert!(b.contains(&bp(0, 0, 0)));
        assert!(b.contains(&bp(3, 3, 3)));
        assert!(b.contains(&bp(1, 2, 3)));
        assert!(!b.contains(&bp(-1, 0, 0)));
        assert!(!b.contains(&bp(4, 0, 0)));
        assert!(!b.contains(&bp(0, -1, 0)));
        assert!(!b.contains(&bp(0, 4, 0)));
        assert!(!b.contains(&bp(0, 0, -1)));
        assert!(!b.contains(&bp(0, 0, 4)));
    }

    #[test]
    fn iterator_yields_between_closed_cells_x_fastest() {
        let b = BlockBox::of_two(bp(1, 2, 3), bp(2, 3, 4));
        let cells = b.iterator();
        assert_eq!(cells.len(), 2 * 2 * 2);
        // X-major (x varies fastest), then Y, then Z (BlockPos.betweenClosed order).
        assert_eq!(cells[0], bp(1, 2, 3));
        assert_eq!(cells[1], bp(2, 2, 3));
        assert_eq!(cells[2], bp(1, 3, 3));
        assert_eq!(cells[7], bp(2, 3, 4));
    }

    #[test]
    fn size_is_inclusive_extent() {
        let b = BlockBox::of_two(bp(0, 0, 0), bp(2, 4, 6));
        assert_eq!(b.size_x(), 3);
        assert_eq!(b.size_y(), 5);
        assert_eq!(b.size_z(), 7);
        let block = BlockBox::of(bp(5, 5, 5));
        assert_eq!((block.size_x(), block.size_y(), block.size_z()), (1, 1, 1));
    }

    #[test]
    fn hash_code_matches_java_record() {
        // Hand-replicated `Objects.hash(min, max)` = `31*(31*1 + h(min)) +
        // h(max)`, where `h(BlockPos)` is `Vec3i.hashCode` =
        // `(y + z*31)*31 + x`. For min=(0,0,0): h=0; max=(1,2,3):
        // h = (2 + 3*31)*31 + 1 = 95*31 + 1 = 2946.
        assert_eq!(
            BlockBox::of_two(bp(0, 0, 0), bp(1, 2, 3)).hash_code(),
            31 * 31 + 2946
        );
        // Both corners zero: seed only.
        assert_eq!(BlockBox::of(bp(0, 0, 0)).hash_code(), 961);
        // Negative coordinates wrap through Vec3i.hashCode.
        let b = BlockBox::of_two(bp(-1, -1, -1), bp(1, 1, 1));
        assert_eq!(
            b.hash_code(),
            961i32
                .wrapping_add(b.min().hash_code().wrapping_mul(31))
                .wrapping_add(b.max().hash_code())
        );
    }

    #[test]
    fn extend_positive_grows_max_negative_grows_min() {
        let b = BlockBox::of(bp(0, 0, 0));
        let east = b.extend(&Direction::East, 3);
        assert_eq!(east.min(), bp(0, 0, 0));
        assert_eq!(east.max(), bp(3, 0, 0));
        let north = b.extend(&Direction::North, 4);
        assert_eq!(north.min(), bp(0, 0, -4));
        assert_eq!(north.max(), bp(0, 0, 0));
        let down = b.extend(&Direction::Down, 2);
        assert_eq!(down.min(), bp(0, -2, 0));
        assert_eq!(down.max(), bp(0, 0, 0));
    }

    #[test]
    fn extend_zero_returns_identity() {
        let b = BlockBox::of_two(bp(1, 2, 3), bp(4, 5, 6));
        let e = b.extend(&Direction::North, 0);
        assert_eq!(e, b);
        let e = b.extend(&Direction::East, 0);
        assert_eq!(e, b);
    }

    #[test]
    fn extend_rebases_box_against_the_other_corner() {
        // `extend(SOUTH)` on a one-block box must keep min and only push max;
        // on a box where min.z is already larger than the grown corner the
        // `of(BlockPos.min(...), this.max)` branch normalizes again.
        let b = BlockBox::of_two(bp(0, 0, 5), bp(2, 2, 6));
        let south = b.extend(&Direction::South, 4);
        assert_eq!(south.min(), bp(0, 0, 5));
        assert_eq!(south.max(), bp(2, 2, 10));
        let north = b.extend(&Direction::North, 3);
        assert_eq!(north.min(), bp(0, 0, 2));
        assert_eq!(north.max(), bp(2, 2, 6));
    }

    #[test]
    fn moved_shifts_both_corners() {
        let b = BlockBox::of_two(bp(1, 2, 3), bp(4, 5, 6));
        let m = b.moved(&Direction::East, 3);
        assert_eq!(m.min(), bp(4, 2, 3));
        assert_eq!(m.max(), bp(7, 5, 6));
        let m = b.moved(&Direction::Down, 2);
        assert_eq!(m.min(), bp(1, 0, 3));
        assert_eq!(m.max(), bp(4, 3, 6));
    }

    #[test]
    fn moved_zero_returns_identity() {
        let b = BlockBox::of_two(bp(1, 2, 3), bp(4, 5, 6));
        assert_eq!(b.moved(&Direction::North, 0), b);
    }

    #[test]
    fn offset_shifts_by_vec3i() {
        let b = BlockBox::of_two(bp(1, 2, 3), bp(4, 5, 6));
        let o = b.offset(&Vec3i::new(-1, 10, 0));
        assert_eq!(o.min(), bp(0, 12, 3));
        assert_eq!(o.max(), bp(3, 15, 6));
    }

    #[test]
    fn display_matches_java_record_to_string() {
        let b = BlockBox::of_two(bp(1, 2, 3), bp(4, 5, 6));
        assert_eq!(
            b.to_string(),
            "BlockBox[min=BlockPos{x=1, y=2, z=3}, max=BlockPos{x=4, y=5, z=6}]"
        );
    }
}

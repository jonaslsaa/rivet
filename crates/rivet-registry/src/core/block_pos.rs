//! `net.minecraft.core.BlockPos` — an immutable block position, plus the
//! mutable `MutableBlockPos` and `TraversalNodeStatus`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/BlockPos.java`.
//! Preserves the packed-long bit layout (26-bit X/Z, 12-bit Y) and the wrapping
//! block arithmetic exactly.
//!
//! Java iterator-style surfaces (`withinManhattan`, `betweenClosed`,
//! `neighborColumn`, `spiralAround`, `randomBetweenClosed`) are ported as
//! materialized `Vec`s with the same iteration order (Java re-iterable
//! `Iterable`s can be pulled again; Rivet returns one pass). `Rotation`,
//! `TraversalNodeStatus`, and `RandomSource` are in `core`/`rivet-util`.
//!
//! `CODEC` landed here (`Codec.INT_STREAM.comapFlatMap(Util::fixedSize(…, 3))
//! .stable()`).
//! RivetTodo(#126): `STREAM_CODEC` (codec surface → rivet-protocol).
//! `betweenCornersInDirection`/`clampLocationWithin` (JOML
//! `Vec3`) defer with the JOML unit.

use super::axis_cycle::AxisCycle;
use super::direction::{Axis, Direction};
use super::rotation::Rotation;
use super::vec3i::{Vec3i, Vec3iLike, compare_coords};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::mth;
use std::sync::Arc;

/// Block-position packing constants (`BlockPos` Paper-inlined bit operations).
const PACKED_X_MASK: i64 = 67108863; // 26 bits
const PACKED_Y_MASK: i64 = 4095; // 12 bits
const PACKED_Z_MASK: i64 = 67108863; // 26 bits

/// `BlockPos` — an immutable block position.
///
/// Implements the `(x, y, z)` value-equality projection so it compares equal
/// with any `Vec3i`/`MutableBlockPos`/`SectionPos` with matching coordinates
/// (Java `Vec3i.equals` checks `o instanceof Vec3i`).
#[derive(Clone, Copy, Debug)]
pub struct BlockPos {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

impl Vec3iLike for BlockPos {
    fn coords(&self) -> (i32, i32, i32) {
        (self.x, self.y, self.z)
    }
}

impl PartialEq for BlockPos {
    fn eq(&self, other: &Self) -> bool {
        self.coords() == other.coords()
    }
}

impl Eq for BlockPos {}

impl PartialOrd for BlockPos {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BlockPos {
    /// Lexicographic `(y, z, x)` ordering (see `vec3i::cmp_lexicographic_yzx`):
    /// a total order consistent with `Eq`, equal to the sign of Java
    /// `Vec3i.compareTo` whenever the coordinate subtractions do not overflow.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        super::vec3i::cmp_lexicographic_yzx((self.x, self.y, self.z), (other.x, other.y, other.z))
    }
}

impl std::hash::Hash for BlockPos {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_i32(self.hash_code());
    }
}

impl BlockPos {
    /// `BlockPos.ZERO`.
    pub const ZERO: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

    /// `BlockPos.PACKED_Y_LENGTH` — the bit width of the packed Y field (12),
    /// the source of `DimensionType.BITS_FOR_Y`.
    pub const PACKED_Y_LENGTH: i32 = 12;

    /// `BlockPos.MAX_HORIZONTAL_COORDINATE`.
    pub const MAX_HORIZONTAL_COORDINATE: i32 = 33554431;

    /// `new BlockPos(x, y, z)`.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// `BlockPos.getX()`.
    pub fn get_x(&self) -> i32 {
        self.x
    }

    /// `BlockPos.getY()`.
    pub fn get_y(&self) -> i32 {
        self.y
    }

    /// `BlockPos.getZ()`.
    pub fn get_z(&self) -> i32 {
        self.z
    }

    /// `Vec3i.hashCode()` — `(y + z * 31) * 31 + x` (wrapping).
    pub fn hash_code(&self) -> i32 {
        (self.y.wrapping_add(self.z.wrapping_mul(31)))
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }

    /// `Vec3i.compareTo(Vec3i)` — the exact int result (`y`, then `z`, then `x`
    /// wrapping int subtraction).
    pub fn compare_to(&self, pos: &BlockPos) -> i32 {
        compare_coords(self.x, self.y, self.z, pos.x, pos.y, pos.z)
    }

    /// `Vec3i.distChessboard(Vec3i)` — the max of the axis deltas (`Chebyshev`).
    ///
    /// Re-declared on `BlockPos` like the other Java-inherited `Vec3i` methods
    /// so `GlobalPos::is_close_enough` (Java `GlobalPos.isCloseEnough`) can call
    /// `pos.distChessboard(pos)` on a `BlockPos`.
    pub fn dist_chessboard(&self, pos: &BlockPos) -> i32 {
        let xd = self.x.wrapping_sub(pos.x).wrapping_abs();
        let yd = self.y.wrapping_sub(pos.y).wrapping_abs();
        let zd = self.z.wrapping_sub(pos.z).wrapping_abs();
        xd.max(yd).max(zd)
    }

    /// `Vec3i.distManhattan(Vec3i)` — float absolute values summed then cast
    /// to int (Java: `(int)(xd + yd + zd)`; the float sum truncates).
    ///
    /// Re-declared on `BlockPos` like the other Java-inherited `Vec3i` methods
    /// so `LinearPosTest` (Java `LinearPosTest.test` calling
    /// `worldPos.distManhattan(worldReference)`) can call it on a `BlockPos`.
    pub fn dist_manhattan(&self, pos: &BlockPos) -> i32 {
        let xd = self.x.wrapping_sub(pos.x).wrapping_abs() as f32;
        let yd = self.y.wrapping_sub(pos.y).wrapping_abs() as f32;
        let zd = self.z.wrapping_sub(pos.z).wrapping_abs() as f32;
        (xd + yd + zd) as i32
    }

    /// `BlockPos.asLong()`.
    pub fn as_long(&self) -> i64 {
        Self::as_long_coords(self.x, self.y, self.z)
    }

    /// `BlockPos.asLong(int, int, int)`.
    pub const fn as_long_coords(x: i32, y: i32, z: i32) -> i64 {
        ((x as i64 & PACKED_X_MASK) << 38)
            | (y as i64 & PACKED_Y_MASK)
            | ((z as i64 & PACKED_Z_MASK) << 12)
    }

    /// `BlockPos.getX(long)`.
    pub fn get_x_long(block_node: i64) -> i32 {
        (block_node >> 38) as i32
    }

    /// `BlockPos.getY(long)`.
    pub fn get_y_long(block_node: i64) -> i32 {
        ((block_node << 52) >> 52) as i32
    }

    /// `BlockPos.getZ(long)`.
    pub fn get_z_long(block_node: i64) -> i32 {
        ((block_node << 26) >> 38) as i32
    }

    /// `BlockPos.of(long)`.
    pub fn of_long(block_node: i64) -> BlockPos {
        BlockPos::new(
            Self::get_x_long(block_node),
            Self::get_y_long(block_node),
            Self::get_z_long(block_node),
        )
    }

    /// `BlockPos.offset(long, int, int, int)`.
    pub fn offset_long(block_node: i64, step_x: i32, step_y: i32, step_z: i32) -> i64 {
        Self::as_long_coords(
            Self::get_x_long(block_node).wrapping_add(step_x),
            Self::get_y_long(block_node).wrapping_add(step_y),
            Self::get_z_long(block_node).wrapping_add(step_z),
        )
    }

    /// `BlockPos.offset(long, Direction)`.
    pub fn offset_long_dir(block_node: i64, offset: &Direction) -> i64 {
        Self::offset_long(
            block_node,
            offset.step_x(),
            offset.step_y(),
            offset.step_z(),
        )
    }

    /// `BlockPos.getAdjacent(int, int, int, Direction)` (Paper).
    pub fn get_adjacent(base_x: i32, base_y: i32, base_z: i32, direction: &Direction) -> i64 {
        Self::as_long_coords(
            base_x.wrapping_add(direction.step_x()),
            base_y.wrapping_add(direction.step_y()),
            base_z.wrapping_add(direction.step_z()),
        )
    }

    /// `BlockPos.getFlatIndex(long)`.
    pub fn get_flat_index(neighbor_block_node: i64) -> i64 {
        neighbor_block_node & -16
    }

    /// `BlockPos.containing(double, double, double)`.
    pub fn containing(x: f64, y: f64, z: f64) -> BlockPos {
        BlockPos::new(mth::floor_d(x), mth::floor_d(y), mth::floor_d(z))
    }

    /// `BlockPos.containing(Position)`.
    pub fn containing_pos(pos: &dyn super::Position) -> BlockPos {
        Self::containing(pos.x(), pos.y(), pos.z())
    }

    /// `BlockPos.min(BlockPos, BlockPos)`.
    pub fn min(a: &BlockPos, b: &BlockPos) -> BlockPos {
        BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
    }

    /// `BlockPos.max(BlockPos, BlockPos)`.
    pub fn max(a: &BlockPos, b: &BlockPos) -> BlockPos {
        BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
    }

    /// `BlockPos.offset(int, int, int)`.
    pub fn offset(&self, x: i32, y: i32, z: i32) -> BlockPos {
        if x == 0 && y == 0 && z == 0 {
            *self
        } else {
            BlockPos::new(
                self.x.wrapping_add(x),
                self.y.wrapping_add(y),
                self.z.wrapping_add(z),
            )
        }
    }

    /// `BlockPos.offset(Vec3i)`.
    pub fn offset_vec(&self, vec: &Vec3i) -> BlockPos {
        self.offset(vec.get_x(), vec.get_y(), vec.get_z())
    }

    /// `BlockPos.subtract(Vec3i)`.
    pub fn subtract(&self, vec: &Vec3i) -> BlockPos {
        self.offset(
            vec.get_x().wrapping_neg(),
            vec.get_y().wrapping_neg(),
            vec.get_z().wrapping_neg(),
        )
    }

    /// `BlockPos.multiply(int)`.
    pub fn multiply(&self, scale: i32) -> BlockPos {
        if scale == 1 {
            *self
        } else if scale == 0 {
            Self::ZERO
        } else {
            BlockPos::new(
                self.x.wrapping_mul(scale),
                self.y.wrapping_mul(scale),
                self.z.wrapping_mul(scale),
            )
        }
    }

    /// `BlockPos.multiply(int, int, int)`.
    pub fn multiply_xyz(&self, x_scale: i32, y_scale: i32, z_scale: i32) -> BlockPos {
        BlockPos::new(
            self.x.wrapping_mul(x_scale),
            self.y.wrapping_mul(y_scale),
            self.z.wrapping_mul(z_scale),
        )
    }

    /// `BlockPos.above()`.
    pub fn above(&self) -> BlockPos {
        BlockPos::new(self.x, self.y.wrapping_add(1), self.z)
    }

    /// `BlockPos.above(int)`.
    pub fn above_steps(&self, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(self.x, self.y.wrapping_add(steps), self.z)
        }
    }

    /// `BlockPos.below()`.
    pub fn below(&self) -> BlockPos {
        BlockPos::new(self.x, self.y.wrapping_sub(1), self.z)
    }

    /// `BlockPos.below(int)`.
    pub fn below_steps(&self, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(self.x, self.y.wrapping_sub(steps), self.z)
        }
    }

    /// `BlockPos.north()`.
    pub fn north(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z.wrapping_sub(1))
    }

    /// `BlockPos.north(int)`.
    pub fn north_steps(&self, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(self.x, self.y, self.z.wrapping_sub(steps))
        }
    }

    /// `BlockPos.south()`.
    pub fn south(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z.wrapping_add(1))
    }

    /// `BlockPos.south(int)`.
    pub fn south_steps(&self, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(self.x, self.y, self.z.wrapping_add(steps))
        }
    }

    /// `BlockPos.west()`.
    pub fn west(&self) -> BlockPos {
        BlockPos::new(self.x.wrapping_sub(1), self.y, self.z)
    }

    /// `BlockPos.west(int)`.
    pub fn west_steps(&self, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(self.x.wrapping_sub(steps), self.y, self.z)
        }
    }

    /// `BlockPos.east()`.
    pub fn east(&self) -> BlockPos {
        BlockPos::new(self.x.wrapping_add(1), self.y, self.z)
    }

    /// `BlockPos.east(int)`.
    pub fn east_steps(&self, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(self.x.wrapping_add(steps), self.y, self.z)
        }
    }

    /// `BlockPos.relative(Direction)`.
    pub fn relative(&self, direction: &Direction) -> BlockPos {
        match direction {
            Direction::Up => BlockPos::new(self.x, self.y.wrapping_add(1), self.z),
            Direction::Down => BlockPos::new(self.x, self.y.wrapping_sub(1), self.z),
            Direction::North => BlockPos::new(self.x, self.y, self.z.wrapping_sub(1)),
            Direction::South => BlockPos::new(self.x, self.y, self.z.wrapping_add(1)),
            Direction::West => BlockPos::new(self.x.wrapping_sub(1), self.y, self.z),
            Direction::East => BlockPos::new(self.x.wrapping_add(1), self.y, self.z),
        }
    }

    /// `BlockPos.relative(Direction, int)`.
    pub fn relative_steps(&self, direction: &Direction, steps: i32) -> BlockPos {
        if steps == 0 {
            *self
        } else {
            BlockPos::new(
                self.x.wrapping_add(direction.step_x().wrapping_mul(steps)),
                self.y.wrapping_add(direction.step_y().wrapping_mul(steps)),
                self.z.wrapping_add(direction.step_z().wrapping_mul(steps)),
            )
        }
    }

    /// `BlockPos.relative(Direction.Axis, int)`.
    pub fn relative_axis(&self, axis: &Axis, steps: i32) -> BlockPos {
        if steps == 0 {
            return *self;
        }
        let (x_step, y_step, z_step) = match axis {
            Axis::X => (steps, 0, 0),
            Axis::Y => (0, steps, 0),
            Axis::Z => (0, 0, steps),
        };
        BlockPos::new(
            self.x.wrapping_add(x_step),
            self.y.wrapping_add(y_step),
            self.z.wrapping_add(z_step),
        )
    }

    /// `BlockPos.cross(Vec3i)`.
    pub fn cross(&self, up_vector: &Vec3i) -> BlockPos {
        BlockPos::new(
            self.y
                .wrapping_mul(up_vector.get_z())
                .wrapping_sub(self.z.wrapping_mul(up_vector.get_y())),
            self.z
                .wrapping_mul(up_vector.get_x())
                .wrapping_sub(self.x.wrapping_mul(up_vector.get_z())),
            self.x
                .wrapping_mul(up_vector.get_y())
                .wrapping_sub(self.y.wrapping_mul(up_vector.get_x())),
        )
    }

    /// `BlockPos.rotate(Rotation)` — quarter-turn around the Y axis (wrapping
    /// negation, Java `-this.getZ()` etc.).
    pub fn rotate(&self, rotation: &Rotation) -> BlockPos {
        match rotation {
            Rotation::Clockwise90 => BlockPos::new(self.z.wrapping_neg(), self.y, self.x),
            Rotation::Clockwise180 => {
                BlockPos::new(self.x.wrapping_neg(), self.y, self.z.wrapping_neg())
            }
            Rotation::Counterclockwise90 => BlockPos::new(self.z, self.y, self.x.wrapping_neg()),
            Rotation::None => *self,
        }
    }

    /// `BlockPos.atY(int)`.
    pub fn at_y(&self, y: i32) -> BlockPos {
        BlockPos::new(self.x, y, self.z)
    }

    /// `BlockPos.immutable()`.
    pub fn immutable(&self) -> BlockPos {
        *self
    }

    /// `BlockPos.mutable()`.
    pub fn mutable(&self) -> MutableBlockPos {
        MutableBlockPos::new(self.x, self.y, self.z)
    }

    /// `BlockPos.squareOutSouthEast(BlockPos)` — `Stream.of(from, from.south(),
    /// from.east(), from.south().east())`.
    pub fn square_out_south_east(from: &BlockPos) -> [BlockPos; 4] {
        [*from, from.south(), from.east(), from.south().east()]
    }

    /// `BlockPos.betweenClosed(int, int, int, int, int, int)` — the cells of
    /// the inclusive axis-aligned box in X/Y/Z-major order.
    pub fn between_closed(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Vec<BlockPos> {
        let width = max_x.wrapping_sub(min_x).wrapping_add(1);
        let height = max_y.wrapping_sub(min_y).wrapping_add(1);
        let depth = max_z.wrapping_sub(min_z).wrapping_add(1);
        // Java computes `int end = width * height * depth` (wrapping). Its
        // iterator checks `this.index == this.end`; with a negative `end` the
        // `index` counter keeps yielding positions until int wraparound brings
        // it back to `end` (~2^31 steps). That is unreachable for real boxes;
        // the `.max(0)` guard makes Rivet return an empty `Vec` instead — a
        // deliberate divergence from Java's degenerate loop.
        let end = width.wrapping_mul(height).wrapping_mul(depth).max(0);
        let mut out = Vec::with_capacity(end as usize);
        for index in 0..end {
            let x = index % width;
            let slice = index / width;
            let y = slice % height;
            let z = slice / height;
            out.push(BlockPos::new(
                min_x.wrapping_add(x),
                min_y.wrapping_add(y),
                min_z.wrapping_add(z),
            ));
        }
        out
    }

    /// `BlockPos.betweenClosed(BlockPos, BlockPos)`.
    pub fn between_closed_pos(a: &BlockPos, b: &BlockPos) -> Vec<BlockPos> {
        Self::between_closed(
            a.x.min(b.x),
            a.y.min(b.y),
            a.z.min(b.z),
            a.x.max(b.x),
            a.y.max(b.y),
            a.z.max(b.z),
        )
    }

    /// `BlockPos.betweenClosedStream(BlockPos, BlockPos)`.
    pub fn between_closed_stream(a: &BlockPos, b: &BlockPos) -> Vec<BlockPos> {
        Self::between_closed_pos(a, b)
    }

    /// `BlockPos.betweenClosedStream(int, int, int, int, int, int)`.
    pub fn between_closed_stream_ints(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Vec<BlockPos> {
        Self::between_closed(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `BlockPos.withinManhattan(BlockPos, int, int, int)` — Java's
    /// `withinManhattan` iterator order, including the z-mirror doubling
    /// (`originZ + zz` then `originZ - zz`) for cells with `zz != 0`.
    pub fn within_manhattan(
        origin: &BlockPos,
        reach_x: i32,
        reach_y: i32,
        reach_z: i32,
    ) -> Vec<BlockPos> {
        let max_depth = reach_x.wrapping_add(reach_y).wrapping_add(reach_z);
        let origin_x = origin.x;
        let origin_y = origin.y;
        let origin_z = origin.z;
        let mut out = Vec::new();
        let mut current_depth = 0i32;
        let mut max_x = 0i32;
        let mut max_y = 0i32;
        let mut x = 0i32;
        let mut y = 0i32;
        let mut z_mirror = false;
        let mut cx = 0i32;
        let mut cy = 0i32;
        let mut cz = 0i32;
        loop {
            if z_mirror {
                z_mirror = false;
                cz = origin_z.wrapping_sub(cz.wrapping_sub(origin_z));
                out.push(BlockPos::new(cx, cy, cz));
                continue;
            }
            let mut found: Option<BlockPos> = None;
            while found.is_none() {
                if y > max_y {
                    x = x.wrapping_add(1);
                    if x > max_x {
                        current_depth = current_depth.wrapping_add(1);
                        if current_depth > max_depth {
                            return out;
                        }
                        max_x = reach_x.min(current_depth);
                        x = max_x.wrapping_neg();
                    }
                    max_y = reach_y.min(current_depth.wrapping_sub(x.wrapping_abs()));
                    y = max_y.wrapping_neg();
                }
                let xx = x;
                let yy = y;
                let zz = current_depth
                    .wrapping_sub(xx.wrapping_abs())
                    .wrapping_sub(yy.wrapping_abs());
                if zz <= reach_z {
                    z_mirror = zz != 0;
                    cx = origin_x.wrapping_add(xx);
                    cy = origin_y.wrapping_add(yy);
                    cz = origin_z.wrapping_add(zz);
                    found = Some(BlockPos::new(cx, cy, cz));
                }
                y = y.wrapping_add(1);
            }
            out.push(found.unwrap());
        }
    }

    /// `BlockPos.withinManhattanStream(BlockPos, int, int, int)`.
    pub fn within_manhattan_stream(
        origin: &BlockPos,
        reach_x: i32,
        reach_y: i32,
        reach_z: i32,
    ) -> Vec<BlockPos> {
        Self::within_manhattan(origin, reach_x, reach_y, reach_z)
    }

    /// `BlockPos.findClosestMatch(BlockPos, int, int, Predicate<BlockPos>)` —
    /// the first `withinManhattan` cell satisfying `predicate`, in iterator
    /// order.
    pub fn find_closest_match(
        start_pos: &BlockPos,
        horizontal_search_radius: i32,
        vertical_search_radius: i32,
        predicate: &mut impl FnMut(&BlockPos) -> bool,
    ) -> Option<BlockPos> {
        Self::within_manhattan(
            start_pos,
            horizontal_search_radius,
            vertical_search_radius,
            horizontal_search_radius,
        )
        .into_iter()
        .find(|pos| predicate(pos))
    }

    /// `BlockPos.breadthFirstTraversal(BlockPos, int, int, BiConsumer, Function)`
    /// — BFS from `start_pos`; `node_processor` may `Skip`/`Stop` a node;
    /// returns the number of `Accept`ed nodes processed (capped at `max_count`).
    pub fn breadth_first_traversal(
        start_pos: &BlockPos,
        max_depth: i32,
        max_count: i32,
        neighbor_provider: &mut impl FnMut(BlockPos, &mut dyn FnMut(BlockPos)),
        node_processor: &mut impl FnMut(BlockPos) -> TraversalNodeStatus,
    ) -> i32 {
        let mut nodes = std::collections::VecDeque::new();
        let mut visited = std::collections::HashSet::new();
        nodes.push_back((*start_pos, 0i32));
        let mut count = 0i32;
        while let Some((current_pos, depth)) = nodes.pop_front() {
            let current_pos_long = current_pos.as_long();
            if visited.insert(current_pos_long) {
                let next = node_processor(current_pos);
                if next != TraversalNodeStatus::Skip {
                    if next == TraversalNodeStatus::Stop {
                        break;
                    }
                    count += 1;
                    if count >= max_count {
                        return count;
                    }
                    if depth < max_depth {
                        neighbor_provider(current_pos, &mut |pos| {
                            nodes.push_back((pos, depth + 1))
                        });
                    }
                }
            }
        }
        count
    }

    /// `BlockPos.spiralAround(BlockPos, int, Direction, Direction)` — Java
    /// yields the *same* `MutableBlockPos` instance mutated each step; Rivet
    /// returns the immutable values in the same order.
    pub fn spiral_around(
        center: &BlockPos,
        radius: i32,
        first_direction: &Direction,
        second_direction: &Direction,
    ) -> Vec<BlockPos> {
        // Java `Validate.validState(..., "The two directions cannot be on the
        // same axis")` throws in every build; an unconditional panic mirrors
        // that, so same-axis inputs fail loudly in release too.
        assert!(
            first_direction.get_axis() != second_direction.get_axis(),
            "The two directions cannot be on the same axis"
        );
        let directions = [
            *first_direction,
            *second_direction,
            first_direction.get_opposite(),
            second_direction.get_opposite(),
        ];
        let mut cursor = center.mutable();
        cursor.move_dir(second_direction);
        let legs = 4i32.wrapping_mul(radius);
        let mut leg = -1i32;
        let mut leg_size = 0i32;
        let mut leg_index = 0i32;
        let mut last_x = cursor.get_x();
        let mut last_y = cursor.get_y();
        let mut last_z = cursor.get_z();
        let mut out = Vec::new();
        loop {
            cursor.set(last_x, last_y, last_z);
            cursor.move_dir(&directions[((leg + 4) % 4) as usize]);
            last_x = cursor.get_x();
            last_y = cursor.get_y();
            last_z = cursor.get_z();
            if leg_index >= leg_size {
                if leg >= legs {
                    break;
                }
                leg += 1;
                leg_index = 0;
                leg_size = leg / 2 + 1;
            }
            leg_index += 1;
            out.push(cursor.immutable());
        }
        out
    }

    /// `BlockPos.randomBetweenClosed(RandomSource, int, int, int, int, int, int, int)`
    /// — `limit` uniformly random cells in the box. Java returns a lazily
    /// re-iterable `Iterable`; Rivet materializes one draw as a `Vec`.
    #[allow(clippy::too_many_arguments)] // mirrors the 8-arg Java signature
    pub fn random_between_closed(
        random: &mut impl rivet_util::RandomSource,
        limit: i32,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Vec<BlockPos> {
        let width = max_x.wrapping_sub(min_x).wrapping_add(1);
        let height = max_y.wrapping_sub(min_y).wrapping_add(1);
        let depth = max_z.wrapping_sub(min_z).wrapping_add(1);
        let mut out = Vec::with_capacity(limit.max(0) as usize);
        let mut counter = limit;
        while counter > 0 {
            out.push(BlockPos::new(
                min_x.wrapping_add(random.next_int_bound(width)),
                min_y.wrapping_add(random.next_int_bound(height)),
                min_z.wrapping_add(random.next_int_bound(depth)),
            ));
            counter -= 1;
        }
        out
    }

    /// `BlockPos.randomInCube(RandomSource, int, BlockPos, int)`.
    pub fn random_in_cube(
        random: &mut impl rivet_util::RandomSource,
        limit: i32,
        center: &BlockPos,
        size_to_scan_in_all_directions: i32,
    ) -> Vec<BlockPos> {
        let size = size_to_scan_in_all_directions;
        Self::random_between_closed(
            random,
            limit,
            center.x.wrapping_sub(size),
            center.y.wrapping_sub(size),
            center.z.wrapping_sub(size),
            center.x.wrapping_add(size),
            center.y.wrapping_add(size),
            center.z.wrapping_add(size),
        )
    }

    /// `BlockPos.neighborColumn(int, int, int, int)` — the vertical column plus
    /// the four horizontal neighbor columns.
    pub fn neighbor_column(start_x: i32, start_y: i32, start_z: i32, end_y: i32) -> Vec<BlockPos> {
        let y_direction = if end_y > start_y { 1 } else { -1 };
        let height = (end_y.wrapping_sub(start_y)).wrapping_abs() + 1;
        let steps: [Vec3i; 5] = [
            Vec3i::new(0, 0, 0),
            Direction::North.get_unit_vec3i(),
            Direction::East.get_unit_vec3i(),
            Direction::South.get_unit_vec3i(),
            Direction::West.get_unit_vec3i(),
        ];
        let step_count = steps.len() * height as usize;
        let mut out = Vec::with_capacity(step_count);
        for index in 0..step_count {
            let y = (index % height as usize) as i32;
            let step_index = index / height as usize;
            let step = steps[step_index];
            out.push(BlockPos::new(
                start_x.wrapping_add(step.get_x()),
                start_y.wrapping_add(y.wrapping_mul(y_direction)),
                start_z.wrapping_add(step.get_z()),
            ));
        }
        out
    }
}

impl std::fmt::Display for BlockPos {
    /// `BlockPos.toString()` — Guava `MoreObjects.toStringHelper`, using the
    /// concrete `BlockPos` class name.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            super::vec3i::format_helper("BlockPos", self.x, self.y, self.z)
        )
    }
}

/// `BlockPos.CODEC` — `Codec.INT_STREAM.comapFlatMap(Util::fixedSize, ...)
/// .stable()` as the ops-generic `block_pos_codec::<Ops>()` factory.
///
/// Java: `Codec.INT_STREAM.<BlockPos>comapFlatMap(input ->
/// Util.fixedSize(input, 3).map(ints -> new BlockPos(ints[0], ints[1],
/// ints[2])), pos -> IntStream.of(pos.getX(), pos.getY(), pos.getZ()))` then
/// `.stable()`. The int stream is a `Vec<i32>` here (`get_int_stream`);
/// `Util.fixedSize(input, 3)` (rivet-util `fixed_size_i32`) returns a
/// `DataResult<Vec<i32>>` with the same "Input is not a list of 3 ints"
/// error/partial semantics, mapped to a `BlockPos`; `codec::stable` applies
/// the stable lifecycle like Java's `.stable()`. The `RivetTodo(#126)` on this
/// module's header tracks the remaining `STREAM_CODEC`.
pub fn block_pos_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BlockPos, Ops>> {
    codec::stable(codec::comap_flat_map::<Vec<i32>, BlockPos, Ops>(
        codec::int_stream_codec::<Ops>(),
        Arc::new(|input: &Vec<i32>| {
            rivet_util::fixed_size_i32(input, 3)
                .map(|ints| BlockPos::new(ints[0], ints[1], ints[2]))
        }),
        Arc::new(|pos: &BlockPos| vec![pos.get_x(), pos.get_y(), pos.get_z()]),
    ))
}

/// `BlockPos.MutableBlockPos` — a mutable block position.
#[derive(Clone, Copy, Debug)]
pub struct MutableBlockPos {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

impl Vec3iLike for MutableBlockPos {
    fn coords(&self) -> (i32, i32, i32) {
        (self.x, self.y, self.z)
    }
}

impl PartialEq for MutableBlockPos {
    fn eq(&self, other: &Self) -> bool {
        self.coords() == other.coords()
    }
}

impl Eq for MutableBlockPos {}

impl std::hash::Hash for MutableBlockPos {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Java `MutableBlockPos` inherits `Vec3i.hashCode` — same as
        // `BlockPos`, so equal coordinates hash identically across the
        // hierarchy.
        state.write_i32(self.hash_code());
    }
}

impl MutableBlockPos {
    /// `new MutableBlockPos(x, y, z)`.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// `MutableBlockPos.getX()`.
    pub fn get_x(&self) -> i32 {
        self.x
    }

    /// `MutableBlockPos.getY()`.
    pub fn get_y(&self) -> i32 {
        self.y
    }

    /// `MutableBlockPos.getZ()`.
    pub fn get_z(&self) -> i32 {
        self.z
    }

    /// `Vec3i.hashCode()` — inherited by `MutableBlockPos` from `Vec3i`.
    pub fn hash_code(&self) -> i32 {
        (self.y.wrapping_add(self.z.wrapping_mul(31)))
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }

    /// `Vec3i.compareTo(Vec3i)` — inherited by `MutableBlockPos` from `Vec3i`:
    /// the exact int result (`y`, then `z`, then `x` wrapping int subtraction).
    ///
    /// `MutableBlockPos` deliberately does **not** implement `Ord`. Java
    /// exposes `compareTo` as a plain method, so `Ord::clamp` (a default trait
    /// method) would collide in Rust method resolution with the inherent
    /// `MutableBlockPos::clamp(Direction.Axis, int, int)` setter and silently
    /// hijack every `pos.clamp(...)` call; `compare_to` mirrors the inherited
    /// Java semantics without that breakage.
    pub fn compare_to(&self, pos: &MutableBlockPos) -> i32 {
        compare_coords(self.x, self.y, self.z, pos.x, pos.y, pos.z)
    }

    /// `MutableBlockPos.set(int, int, int)`.
    pub fn set(&mut self, x: i32, y: i32, z: i32) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        self
    }

    /// `MutableBlockPos.set(double, double, double)`.
    pub fn set_f64(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.set(mth::floor_d(x), mth::floor_d(y), mth::floor_d(z))
    }

    /// `MutableBlockPos.set(Vec3i)`.
    pub fn set_vec(&mut self, vec: &Vec3i) -> &mut Self {
        self.set(vec.get_x(), vec.get_y(), vec.get_z())
    }

    /// `MutableBlockPos.set(long)`.
    pub fn set_long(&mut self, pos: i64) -> &mut Self {
        self.set(
            BlockPos::get_x_long(pos),
            BlockPos::get_y_long(pos),
            BlockPos::get_z_long(pos),
        )
    }

    /// `MutableBlockPos.set(AxisCycle, int, int, int)`.
    pub fn set_axis_cycle(&mut self, transform: &AxisCycle, x: i32, y: i32, z: i32) -> &mut Self {
        self.set(
            transform.cycle(x, y, z, Axis::X),
            transform.cycle(x, y, z, Axis::Y),
            transform.cycle(x, y, z, Axis::Z),
        )
    }

    /// `MutableBlockPos.setWithOffset(Vec3i, Direction)`.
    pub fn set_with_offset(&mut self, pos: &Vec3i, direction: &Direction) -> &mut Self {
        self.set(
            pos.get_x().wrapping_add(direction.step_x()),
            pos.get_y().wrapping_add(direction.step_y()),
            pos.get_z().wrapping_add(direction.step_z()),
        )
    }

    /// `MutableBlockPos.setWithOffset(Vec3i, int, int, int)`.
    pub fn set_with_offset_xyz(&mut self, pos: &Vec3i, x: i32, y: i32, z: i32) -> &mut Self {
        self.set(
            pos.get_x().wrapping_add(x),
            pos.get_y().wrapping_add(y),
            pos.get_z().wrapping_add(z),
        )
    }

    /// `MutableBlockPos.setWithOffset(Vec3i, Vec3i)`.
    pub fn set_with_offset_vec(&mut self, pos: &Vec3i, offset: &Vec3i) -> &mut Self {
        self.set(
            pos.get_x().wrapping_add(offset.get_x()),
            pos.get_y().wrapping_add(offset.get_y()),
            pos.get_z().wrapping_add(offset.get_z()),
        )
    }

    /// `MutableBlockPos.move(Direction)`.
    pub fn move_dir(&mut self, direction: &Direction) -> &mut Self {
        self.move_dir_steps(direction, 1)
    }

    /// `MutableBlockPos.move(Direction, int)`.
    pub fn move_dir_steps(&mut self, direction: &Direction, steps: i32) -> &mut Self {
        self.set(
            self.x.wrapping_add(direction.step_x().wrapping_mul(steps)),
            self.y.wrapping_add(direction.step_y().wrapping_mul(steps)),
            self.z.wrapping_add(direction.step_z().wrapping_mul(steps)),
        )
    }

    /// `MutableBlockPos.move(int, int, int)`.
    pub fn move_xyz(&mut self, x: i32, y: i32, z: i32) -> &mut Self {
        self.set(
            self.x.wrapping_add(x),
            self.y.wrapping_add(y),
            self.z.wrapping_add(z),
        )
    }

    /// `MutableBlockPos.move(Vec3i)`.
    pub fn move_vec(&mut self, pos: &Vec3i) -> &mut Self {
        self.set(
            self.x.wrapping_add(pos.get_x()),
            self.y.wrapping_add(pos.get_y()),
            self.z.wrapping_add(pos.get_z()),
        )
    }

    /// `MutableBlockPos.clamp(Direction.Axis, int, int)`.
    pub fn clamp(&mut self, axis: &Axis, minimum: i32, maximum: i32) -> &mut Self {
        match axis {
            Axis::X => self.set(mth::clamp(self.x, minimum, maximum), self.y, self.z),
            Axis::Y => self.set(self.x, mth::clamp(self.y, minimum, maximum), self.z),
            Axis::Z => self.set(self.x, self.y, mth::clamp(self.z, minimum, maximum)),
        }
    }

    /// `MutableBlockPos.setX(int)`.
    pub fn set_x(&mut self, x: i32) -> &mut Self {
        self.x = x;
        self
    }

    /// `MutableBlockPos.setY(int)`.
    pub fn set_y(&mut self, y: i32) -> &mut Self {
        self.y = y;
        self
    }

    /// `MutableBlockPos.setZ(int)`.
    pub fn set_z(&mut self, z: i32) -> &mut Self {
        self.z = z;
        self
    }

    /// `MutableBlockPos.immutable()`.
    pub fn immutable(&self) -> BlockPos {
        BlockPos::new(self.x, self.y, self.z)
    }

    /// `MutableBlockPos.offset(int, int, int)` — Java override returns an
    /// immutable `BlockPos` (`super.offset(x, y, z).immutable()`).
    pub fn offset(&self, x: i32, y: i32, z: i32) -> BlockPos {
        self.immutable().offset(x, y, z)
    }

    /// `MutableBlockPos.multiply(int)` — Java override returns an immutable
    /// `BlockPos` (`super.multiply(scale).immutable()`).
    pub fn multiply(&self, scale: i32) -> BlockPos {
        self.immutable().multiply(scale)
    }

    /// `MutableBlockPos.relative(Direction, int)` — Java override returns an
    /// immutable `BlockPos` (`super.relative(direction, steps).immutable()`).
    pub fn relative_steps(&self, direction: &Direction, steps: i32) -> BlockPos {
        self.immutable().relative_steps(direction, steps)
    }

    /// `MutableBlockPos.relative(Direction.Axis, int)` — Java override returns
    /// an immutable `BlockPos` (`super.relative(axis, steps).immutable()`).
    pub fn relative_axis(&self, axis: &Axis, steps: i32) -> BlockPos {
        self.immutable().relative_axis(axis, steps)
    }

    /// `MutableBlockPos.rotate(Rotation)` — Java override returns an immutable
    /// `BlockPos` (`super.rotate(rotation).immutable()`).
    pub fn rotate(&self, rotation: &Rotation) -> BlockPos {
        self.immutable().rotate(rotation)
    }
}

impl std::fmt::Display for MutableBlockPos {
    /// `MutableBlockPos.toString()` uses the concrete `MutableBlockPos` name
    /// (Java `MoreObjects.toStringHelper(this)`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            super::vec3i::format_helper("MutableBlockPos", self.x, self.y, self.z)
        )
    }
}

/// `BlockPos.TraversalNodeStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraversalNodeStatus {
    Accept,
    Skip,
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::lifecycle::Lifecycle;
    use serde_json::json;

    #[test]
    fn block_pos_codec_round_trips_as_stable() {
        // Paper `BlockPos.CODEC = Codec.INT_STREAM.comapFlatMap(...).stable()`:
        // the wire shape is a 3-int array and the lifecycle is Stable on both
        // encode and decode.
        let ops = JsonOps::INSTANCE;
        let codec = block_pos_codec::<JsonOps>();
        let decoded = codec.decode(&ops, &json!([1, -60, 3]));
        assert_eq!(decoded.lifecycle(), Lifecycle::Stable);
        assert_eq!(decoded.get_or_throw("decode").0, BlockPos::new(1, -60, 3));
        let encoded = codec.encode_start(&ops, &BlockPos::new(1, -60, 3));
        assert_eq!(encoded.lifecycle(), Lifecycle::Stable);
        assert_eq!(encoded.get_or_throw("encode"), &json!([1, -60, 3]));
    }

    #[test]
    fn mutable_block_pos_override_delegates_to_immutable_copy() {
        // The MutableBlockPos overrides call the immutable overload on the
        // `immutable()` copy (Java `super.offset(...).immutable()`), so each
        // result equals the same transform applied to a plain `BlockPos`.
        let m = MutableBlockPos::new(1, 2, 3);
        let expected = BlockPos::new(1, 2, 3);
        assert_eq!(m.offset(4, -5, 6), expected.offset(4, -5, 6));
        assert_eq!(m.multiply(3), expected.multiply(3));
        assert_eq!(
            m.relative_steps(&Direction::Up, 7),
            expected.relative_steps(&Direction::Up, 7)
        );
        assert_eq!(
            m.relative_axis(&Axis::X, -2),
            expected.relative_axis(&Axis::X, -2)
        );
        assert_eq!(
            m.rotate(&Rotation::Clockwise90),
            expected.rotate(&Rotation::Clockwise90)
        );
    }

    #[test]
    fn mutable_block_pos_override_does_not_mutate_receiver() {
        // `MutableBlockPos` overrides return a fresh immutable `BlockPos` and
        // leave the receiver untouched (Java overrides construct from
        // `super.offset(...)` without mutating `this`).
        let m = MutableBlockPos::new(1, 2, 3);
        let before = (m.get_x(), m.get_y(), m.get_z());
        m.offset(4, -5, 6);
        m.multiply(3);
        m.relative_steps(&Direction::Up, 7);
        m.relative_axis(&Axis::X, -2);
        m.rotate(&Rotation::Clockwise90);
        assert_eq!((m.get_x(), m.get_y(), m.get_z()), before);
    }

    #[test]
    fn mutable_block_pos_override_returns_immutable_block_pos() {
        // The overrides return `BlockPos`, not `MutableBlockPos` (Java return
        // type of the overrides); the returned value is a distinct immutable
        // copy equal to the receiver.
        let m = MutableBlockPos::new(1, 2, 3);
        let result: BlockPos = m.offset(0, 0, 0);
        assert_eq!(result, m);
        // A zero offset still yields a value equal to the immutable copy.
        assert_eq!(result, m.immutable());
    }
}

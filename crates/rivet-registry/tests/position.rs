//! Java-grounded tests for the position/value SCC port (issue #125).
//!
//! Expected values are derived by hand-replicating the exact Java
//! bit/arithmetic operations from the pinned Paper 26.2 sources
//! (`working/Paper/paper-server/src/minecraft/java/net/minecraft/core/*.java`
//! and `net/minecraft/world/level/ChunkPos.java`), the same transcription the
//! port itself follows. Where a value depends on runtime-initialized
//! constants (`ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE`), the derivation is
//! cross-checked against the *algorithm* — see `chunk_pos_valid_bound`. Paper
//! ships no vanilla JUnit tests for these value types, so there is no upstream
//! test suite to port; these are direct per-method parity assertions instead.

use rivet_registry::core::MAX_CHUNK_COORDINATE_VALUE;
use rivet_registry::core::{
    Axis, AxisCycle, AxisDirection, BlockPos, ChunkPos, Direction, GlobalPos, MutableBlockPos,
    Plane, Position, Rotation, SectionPos, TraversalNodeStatus, Vec3i,
};
use rivet_registry::registries::DIMENSION;
use rivet_registry::{Identifier, ResourceKey};
use rivet_util::random::LegacyRandomSource;

// ---------------------------------------------------------------------------
// Vec3i / BlockPos long packing
// ---------------------------------------------------------------------------

#[test]
fn block_pos_as_long_roundtrip() {
    let l = BlockPos::as_long_coords(1, 2, 3);
    assert_eq!(l, 274877919234);
    assert_eq!(
        (
            BlockPos::get_x_long(l),
            BlockPos::get_y_long(l),
            BlockPos::get_z_long(l)
        ),
        (1, 2, 3)
    );
    let p = BlockPos::of_long(l);
    assert_eq!((p.get_x(), p.get_y(), p.get_z()), (1, 2, 3));
}

#[test]
fn block_pos_as_long_negative_wraps() {
    // x=-1,z=-3 wrap to 26-bit two's-complement, y=-2 wraps to 12-bit.
    let l = BlockPos::as_long_coords(-1, -2, -3);
    assert_eq!(l, -8194);
    assert_eq!(
        (
            BlockPos::get_x_long(l),
            BlockPos::get_y_long(l),
            BlockPos::get_z_long(l)
        ),
        (-1, -2, -3)
    );
}

#[test]
fn block_pos_as_long_extreme() {
    // y=4095 is the max 12-bit field value, so its top bit (bit 11) makes the
    // arithmetic-shift unpacking of getY sign-extend to -1; z=33554431 keeps
    // its top bit below the sign position and unpacks cleanly.
    let l = BlockPos::as_long_coords(33554431, 4095, 33554431);
    assert_eq!(l, 9223371899415822335);
    assert_eq!(
        (
            BlockPos::get_x_long(l),
            BlockPos::get_y_long(l),
            BlockPos::get_z_long(l)
        ),
        (33554431, -1, 33554431)
    );
    let l2 = BlockPos::as_long_coords(33554431, -4096, 33554431);
    assert_eq!(l2, 9223371899415818240);
    assert_eq!(
        (
            BlockPos::get_x_long(l2),
            BlockPos::get_y_long(l2),
            BlockPos::get_z_long(l2)
        ),
        (33554431, 0, 33554431)
    );
}

#[test]
fn block_pos_as_long_overflow_inputs() {
    let l = BlockPos::as_long_coords(2147483647, -2147483648, 1234567);
    assert_eq!(l, -269821120512);
    assert_eq!(
        (
            BlockPos::get_x_long(l),
            BlockPos::get_y_long(l),
            BlockPos::get_z_long(l)
        ),
        (-1, 0, 1234567)
    );
}

#[test]
fn block_pos_offset_long() {
    let l = BlockPos::as_long_coords(1, 2, 3);
    let off = BlockPos::offset_long(l, 1, 2, 3);
    assert_eq!(off, 549755838468);
    assert_eq!(
        BlockPos::get_flat_index(BlockPos::as_long_coords(1, 2, 3)),
        274877919232
    );
}

#[test]
fn block_pos_flat_index() {
    let l = BlockPos::as_long_coords(1, 2, 3);
    assert_eq!(BlockPos::get_flat_index(l), l & -16);
}

// ---------------------------------------------------------------------------
// Vec3i compare / hash
// ---------------------------------------------------------------------------

#[test]
fn vec3i_compare_wrapping() {
    let a = Vec3i::new(1, 2, 3);
    // y differs first: 5 - 2 = 3.
    assert_eq!(a.compare_to(&Vec3i::new(1, 5, 3)), -3);
    // y equal, z differs: 9 - 3 = 6.
    assert_eq!(a.compare_to(&Vec3i::new(1, 2, 9)), -6);
    // y, z equal, x differs: 9 - 1 = 8.
    assert_eq!(a.compare_to(&Vec3i::new(9, 2, 3)), -8);
    // equal.
    assert_eq!(a.compare_to(&Vec3i::new(1, 2, 3)), 0);
    // Ord matches the sign.
    assert!(a < Vec3i::new(1, 5, 3));
    assert!(Vec3i::new(9, 2, 3) > a);
}

#[test]
fn vec3i_hash_java_parity() {
    assert_eq!(Vec3i::new(1, 2, 3).hash_code(), 2946);
    assert_eq!(Vec3i::new(0, 0, 0).hash_code(), 0);
    assert_eq!(Vec3i::new(-1, 100, 300).hash_code(), 291399);
    assert_eq!(Vec3i::new(i32::MIN, i32::MAX, -7).hash_code(), -6758);
    // Java equality is cross-type (`o instanceof Vec3i`); Rust `Hash` must
    // match so equal types hash identically (Java equals/hashCode contract).
    use std::hash::{Hash, Hasher};
    /// Captures the exact sequence of `write_i32` calls a type feeds a hasher,
    /// so cross-type equal Java values can be asserted to hash identically.
    #[derive(Default, PartialEq, Debug)]
    struct Capture(Vec<i32>);
    impl Hasher for Capture {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, _bytes: &[u8]) {}
        fn write_i32(&mut self, i: i32) {
            self.0.push(i);
        }
    }
    fn hash<V: Hash>(v: &V) -> Capture {
        let mut c = Capture::default();
        v.hash(&mut c);
        c
    }
    assert_eq!(hash(&Vec3i::new(1, 2, 3)), hash(&BlockPos::new(1, 2, 3)));
    assert_eq!(
        hash(&BlockPos::new(1, 2, 3)),
        hash(&SectionPos::of(1, 2, 3))
    );
    assert_eq!(
        hash(&Vec3i::new(1, 2, 3)),
        hash(&MutableBlockPos::new(1, 2, 3))
    );
}

#[test]
fn vec3i_ord_is_total_and_consistent() {
    // Ord must be a law-abiding total order consistent with Eq. Java's
    // compareTo uses wrapping subtraction and is NOT transitive for overflow
    // inputs; the Rust Ord here is the lexicographic (y, z, x) order, which is
    // transitive and equals the sign of compareTo for non-overflow inputs.
    let coords = [
        Vec3i::new(0, 0, 0),
        Vec3i::new(1, 0, 0),
        Vec3i::new(-5, 3, 2),
        Vec3i::new(100, -100, 0),
        Vec3i::new(i32::MIN, i32::MAX, 0),
        Vec3i::new(i32::MAX, i32::MIN, 1),
        Vec3i::new(7, 7, 7),
    ];
    // Consistency with Eq.
    for &a in &coords {
        for &b in &coords {
            assert_eq!(a == b, a.cmp(&b) == std::cmp::Ordering::Equal);
            assert_eq!(a.cmp(&b), a.partial_cmp(&b).unwrap());
        }
    }
    // Transitivity over triples.
    for &a in &coords {
        for &b in &coords {
            for &c in &coords {
                if a <= b && b <= c {
                    assert!(a <= c, "{a} <= {b} <= {c} but {a} > {c}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cross-type equality (Java `o instanceof Vec3i`)
// ---------------------------------------------------------------------------

#[test]
fn cross_type_eq() {
    let bp = BlockPos::new(1, 2, 3);
    let sp = SectionPos::of(1, 2, 3);
    let v3 = Vec3i::new(1, 2, 3);
    assert!(bp == v3);
    assert!(v3 == bp);
    assert!(bp == sp);
    assert!(sp == bp);
    assert!(bp == MutableBlockPos::new(1, 2, 3));
    assert!(MutableBlockPos::new(1, 2, 3) == bp);
}

// ---------------------------------------------------------------------------
// BlockPos
// ---------------------------------------------------------------------------

#[test]
fn block_pos_basic() {
    let p = BlockPos::new(1, 2, 3);
    assert_eq!(p.to_string(), "BlockPos{x=1, y=2, z=3}");
    assert_eq!(p.offset(10, -5, 3), BlockPos::new(11, -3, 6));
    assert_eq!(p.multiply(-2), BlockPos::new(-2, -4, -6));
    assert_eq!(p.multiply_xyz(2, 3, 4), BlockPos::new(2, 6, 12));
    assert_eq!(p.cross(&Vec3i::new(4, 5, 6)), BlockPos::new(-3, 6, -3));
}

#[test]
fn block_pos_direction_ops() {
    let p = BlockPos::new(1, 2, 3);
    assert_eq!(p.above(), BlockPos::new(1, 3, 3));
    assert_eq!(p.relative(&Direction::East), BlockPos::new(2, 2, 3));
    assert_eq!(p.relative_axis(&Axis::X, -4), BlockPos::new(-3, 2, 3));
    assert_eq!(p.relative_axis(&Axis::Z, 2), BlockPos::new(1, 2, 5));
    assert_eq!(p.north().north().south(), BlockPos::new(1, 2, 2));
}

#[test]
fn block_pos_min_max_containing() {
    let a = BlockPos::new(-3, -4, 7);
    let b = BlockPos::new(5, 6, 9);
    assert_eq!(BlockPos::min(&a, &b), BlockPos::new(-3, -4, 7));
    assert_eq!(BlockPos::max(&a, &b), BlockPos::new(5, 6, 9));
    // Mth.floor(-3.7) == -4.
    assert_eq!(
        BlockPos::containing(1.2, -3.7, 100.0),
        BlockPos::new(1, -4, 100)
    );
    assert_eq!(
        BlockPos::containing(1.2, -3.7, 100.0).at_y(-10),
        BlockPos::new(1, -10, 100)
    );
}

#[test]
fn block_pos_square_out_south_east() {
    let from = BlockPos::new(1, 1, 1);
    assert_eq!(
        BlockPos::square_out_south_east(&from),
        [
            BlockPos::new(1, 1, 1),
            BlockPos::new(1, 1, 2),
            BlockPos::new(2, 1, 1),
            BlockPos::new(2, 1, 2),
        ]
    );
}

#[test]
fn block_pos_between_closed() {
    // betweenClosed(0,0,0, 2,1,1) in X/Y/Z-major order.
    let cells = BlockPos::between_closed(0, 0, 0, 2, 1, 1);
    let expected: Vec<BlockPos> = vec![
        BlockPos::new(0, 0, 0),
        BlockPos::new(1, 0, 0),
        BlockPos::new(2, 0, 0),
        BlockPos::new(0, 1, 0),
        BlockPos::new(1, 1, 0),
        BlockPos::new(2, 1, 0),
        BlockPos::new(0, 0, 1),
        BlockPos::new(1, 0, 1),
        BlockPos::new(2, 0, 1),
        BlockPos::new(0, 1, 1),
        BlockPos::new(1, 1, 1),
        BlockPos::new(2, 1, 1),
    ];
    assert_eq!(cells, expected);
}

#[test]
fn block_pos_between_closed_degenerate_overflow_is_empty() {
    // width=height=46341 (box (0,0,0)..(46340,46340,0)) wraps
    // width*height*depth to a negative i32, so Java's `index == end` check
    // would loop ~2^31 steps. Rivet's `.max(0)` guard returns an empty
    // iterator instead. Lock both the lazy iterator and the materialized
    // `between_closed` to that deliberate divergence.
    let a = BlockPos::new(0, 0, 0);
    let b = BlockPos::new(46340, 46340, 0);
    assert_eq!(
        BlockPos::between_closed_iter(&a, &b).count(),
        0,
        "lazy iterator must be empty on a negative wrapping end"
    );
    let materialized: Vec<BlockPos> = BlockPos::between_closed(
        a.get_x(),
        a.get_y(),
        a.get_z(),
        b.get_x(),
        b.get_y(),
        b.get_z(),
    );
    assert_eq!(
        materialized,
        Vec::<BlockPos>::new(),
        "materialized between_closed must also be empty on a negative wrapping end"
    );
}

#[test]
fn block_pos_neighbor_column() {
    // neighborColumn(0,0,0, 3): the column plus N/E/S/W neighbor columns.
    let cols = BlockPos::neighbor_column(0, 0, 0, 3);
    let expected: Vec<BlockPos> = vec![
        BlockPos::new(0, 0, 0),
        BlockPos::new(0, 1, 0),
        BlockPos::new(0, 2, 0),
        BlockPos::new(0, 3, 0),
        BlockPos::new(0, 0, -1),
        BlockPos::new(0, 1, -1),
        BlockPos::new(0, 2, -1),
        BlockPos::new(0, 3, -1),
        BlockPos::new(1, 0, 0),
        BlockPos::new(1, 1, 0),
        BlockPos::new(1, 2, 0),
        BlockPos::new(1, 3, 0),
        BlockPos::new(0, 0, 1),
        BlockPos::new(0, 1, 1),
        BlockPos::new(0, 2, 1),
        BlockPos::new(0, 3, 1),
        BlockPos::new(-1, 0, 0),
        BlockPos::new(-1, 1, 0),
        BlockPos::new(-1, 2, 0),
        BlockPos::new(-1, 3, 0),
    ];
    assert_eq!(cols, expected);
}

#[test]
fn block_pos_within_manhattan_mirror_order() {
    // Java `withinManhattan(origin=0,0,0, reach=1,1,1)` — the z-mirror doubling
    // yields 27 cells in depth/leg order. Replicated from the Java iterator.
    let cells = BlockPos::within_manhattan(&BlockPos::ZERO, 1, 1, 1);
    let expected: Vec<BlockPos> = vec![
        BlockPos::new(0, 0, 0),
        BlockPos::new(-1, 0, 0),
        BlockPos::new(0, -1, 0),
        BlockPos::new(0, 0, 1),
        BlockPos::new(0, 0, -1),
        BlockPos::new(0, 1, 0),
        BlockPos::new(1, 0, 0),
        BlockPos::new(-1, -1, 0),
        BlockPos::new(-1, 0, 1),
        BlockPos::new(-1, 0, -1),
        BlockPos::new(-1, 1, 0),
        BlockPos::new(0, -1, 1),
        BlockPos::new(0, -1, -1),
        BlockPos::new(0, 1, 1),
        BlockPos::new(0, 1, -1),
        BlockPos::new(1, -1, 0),
        BlockPos::new(1, 0, 1),
        BlockPos::new(1, 0, -1),
        BlockPos::new(1, 1, 0),
        BlockPos::new(-1, -1, 1),
        BlockPos::new(-1, -1, -1),
        BlockPos::new(-1, 1, 1),
        BlockPos::new(-1, 1, -1),
        BlockPos::new(1, -1, 1),
        BlockPos::new(1, -1, -1),
        BlockPos::new(1, 1, 1),
        BlockPos::new(1, 1, -1),
    ];
    assert_eq!(cells, expected);
    // reach=2: the first 12 cells match the Java `withinManhattan(0,0,0,2,2,2)`
    // iterator (verified against the pinned source).
    let cells2 = BlockPos::within_manhattan(&BlockPos::ZERO, 2, 2, 2);
    let first12 = &cells2[..12];
    assert_eq!(
        first12,
        &[
            BlockPos::new(0, 0, 0),
            BlockPos::new(-1, 0, 0),
            BlockPos::new(0, -1, 0),
            BlockPos::new(0, 0, 1),
            BlockPos::new(0, 0, -1),
            BlockPos::new(0, 1, 0),
            BlockPos::new(1, 0, 0),
            BlockPos::new(-2, 0, 0),
            BlockPos::new(-1, -1, 0),
            BlockPos::new(-1, 0, 1),
            BlockPos::new(-1, 0, -1),
            BlockPos::new(-1, 1, 0),
        ][..]
    );
}

// ---------------------------------------------------------------------------
// BlockPos traversal / rotation / random
// ---------------------------------------------------------------------------

#[test]
fn block_pos_rotate() {
    let p = BlockPos::new(1, 2, 3);
    // Java BlockPos.rotate: CW90 (-z, y, x); 180 (-x, y, -z); CCW90 (z, y, -x).
    assert_eq!(p.rotate(&Rotation::Clockwise90), BlockPos::new(-3, 2, 1));
    assert_eq!(p.rotate(&Rotation::Clockwise180), BlockPos::new(-1, 2, -3));
    assert_eq!(
        p.rotate(&Rotation::Counterclockwise90),
        BlockPos::new(3, 2, -1)
    );
    assert_eq!(p.rotate(&Rotation::None), p);
}

#[test]
fn block_pos_within_manhattan_stream() {
    let cells = BlockPos::within_manhattan_stream(&BlockPos::ZERO, 1, 0, 0);
    // 3 cells along the X line, mirror order from the Java iterator.
    assert_eq!(
        cells,
        vec![
            BlockPos::new(0, 0, 0),
            BlockPos::new(-1, 0, 0),
            BlockPos::new(1, 0, 0),
        ]
    );
}

#[test]
fn block_pos_find_closest_match() {
    let mut predicate = |p: &BlockPos| p.get_x() == 2;
    let found = BlockPos::find_closest_match(&BlockPos::ZERO, 2, 0, &mut predicate);
    // withinManhattan(0,0,0, reach 2,0,2): first cell with x==2.
    assert_eq!(found, Some(BlockPos::new(2, 0, 0)));
}

#[test]
fn block_pos_breadth_first_traversal() {
    let start = BlockPos::ZERO;
    let mut processor = |p: BlockPos| -> TraversalNodeStatus {
        if p.get_x() > 1 || p.get_y() > 1 || p.get_z() > 1 {
            TraversalNodeStatus::Skip
        } else {
            TraversalNodeStatus::Accept
        }
    };
    let mut provider = |pos: BlockPos, push: &mut dyn FnMut(BlockPos)| {
        for d in [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            push(pos.relative(&d));
        }
    };
    let count = BlockPos::breadth_first_traversal(&start, 3, 100, &mut provider, &mut processor);
    // Re-derived by running the Java `BlockPos.breadthFirstTraversal` iterator
    // (same predicate/provider) in a JVM simulation: BFS over the XZ grid
    // reaches the diamond |x|+|z|<=3, of which the cells with coords <= 1 are
    // accepted — 17 cells, well under max_count (100).
    assert_eq!(count, 17);
}

#[test]
fn block_pos_spiral_around() {
    // Java spiralAround: cursor starts at center.move(EAST) = (1,0,0), then the
    // first computeNext() sets it back to last=(1,0,0) and moves by
    // directions[(-1+4)%4]=WEST, so the first yielded cell is the origin.
    let cells = BlockPos::spiral_around(&BlockPos::ZERO, 2, &Direction::North, &Direction::East);
    assert_eq!(cells.first().copied(), Some(BlockPos::new(0, 0, 0)));
    // Radius 2 winds a 5×5 spiral = 25 cells (8 legs; leg sizes 1,1,2,2,3,3,4,4).
    assert_eq!(cells.len(), 25);
    // Second cell: from origin move NORTH → (0,0,-1).
    assert_eq!(cells.get(1).copied(), Some(BlockPos::new(0, 0, -1)));
}

#[test]
fn block_pos_random_between_closed() {
    // `LegacyRandomSource(99)` draws `nextInt(3)` x, y, z per cell; a JVM
    // simulation of `BlockPos.randomBetweenClosed(random, 5, -1,-1,-1, 1,1,1)`
    // (the same 48-bit LCG, the same bounded-`nextInt` rejection loop, arg
    // order x/y/z) yields these exact cells, pinning both the RNG coupling and
    // the `random_in_cube` expansion.
    let expected = [
        BlockPos::new(0, 1, -1),
        BlockPos::new(-1, -1, 1),
        BlockPos::new(0, 1, 1),
        BlockPos::new(1, 0, -1),
        BlockPos::new(1, 1, 1),
    ];
    let mut r = LegacyRandomSource::new(99);
    let cells = BlockPos::random_in_cube(&mut r, 5, &BlockPos::new(0, 0, 0), 1);
    assert_eq!(cells, expected);
    for p in &cells {
        let (x, y, z) = (p.get_x(), p.get_y(), p.get_z());
        assert!((-1..=1).contains(&x) && (-1..=1).contains(&y) && (-1..=1).contains(&z));
    }
    // The direct `random_between_closed` entry point is the same call the
    // `random_in_cube` wrapper forwards to.
    let mut r2 = LegacyRandomSource::new(99);
    let cells2 = BlockPos::random_between_closed(&mut r2, 5, -1, -1, -1, 1, 1, 1);
    assert_eq!(cells2, expected);
}

// `Position` — the Java interface has no in-crate implementors (its only
// implementors are the deferred JOML `Vec3`/`Vector3d`), so this test supplies
// a local impl to exercise the `Position`-taking overloads.
#[derive(Clone, Copy)]
struct TestPos(f64, f64, f64);

impl Position for TestPos {
    fn x(&self) -> f64 {
        self.0
    }
    fn y(&self) -> f64 {
        self.1
    }
    fn z(&self) -> f64 {
        self.2
    }
}

#[test]
fn position_taking_overloads() {
    let pos = TestPos(1.7, -0.3, 5.0);
    // `BlockPos.containing(Position)` — `Mth.floor` of each coordinate.
    assert_eq!(BlockPos::containing_pos(&pos), BlockPos::new(1, -1, 5));
    // `SectionPos.of(Position)` — `floor(coord) >> 4` per axis.
    assert_eq!(SectionPos::of_position(&pos), SectionPos::of(0, -1, 0));
    // `Vec3i.distToCenterSqr(Position)` on (1,2,3): dx=-0.2, dy=2.8, dz=-1.5.
    let v = Vec3i::new(1, 2, 3);
    assert!((v.dist_to_center_sqr_pos(&pos) - 10.13).abs() < 1e-9);
    // `Vec3i.closerToCenterThan(Position, double)` — `distSqr < square(d)`.
    assert!(v.closer_to_center_than(&pos, 4.0));
    assert!(!v.closer_to_center_than(&pos, 3.0));
}

#[test]
fn chunk_pos_chessboard_coords() {
    let c = ChunkPos::new(3, -5);
    assert_eq!(c.get_chessboard_distance_coords(7, -7), 4);
}

#[test]
fn section_pos_around_chunk() {
    let c = ChunkPos::new(0, 0);
    let sects = SectionPos::around_chunk(&c, 1, 0, 1);
    // (x,z) in -1..=1, y in 0..=1 → 3*3*2 = 18 sections, X/Y/Z-major.
    assert_eq!(sects.len(), 18);
    assert_eq!(sects[0], SectionPos::of(-1, 0, -1));
    assert_eq!(sects[17], SectionPos::of(1, 1, 1));
}

// ---------------------------------------------------------------------------
// MutableBlockPos
// ---------------------------------------------------------------------------

#[test]
fn mutable_block_pos_set_and_move() {
    let mut p = MutableBlockPos::new(0, 0, 0);
    p.set(9, 8, 7);
    assert_eq!((p.get_x(), p.get_y(), p.get_z()), (9, 8, 7));
    p.move_xyz(1, 0, -1);
    assert_eq!((p.get_x(), p.get_y(), p.get_z()), (10, 8, 6));
    p.move_dir(&Direction::Up);
    assert_eq!(p.get_y(), 9);
    p.set_with_offset_xyz(&Vec3i::new(1, 2, 3), 10, 9, 8);
    assert_eq!((p.get_x(), p.get_y(), p.get_z()), (11, 11, 11));
}

#[test]
fn mutable_block_pos_axis_cycle_set() {
    // set(AxisCycle.FORWARD, 1,2,3) cycles (1,2,3) -> (3,1,2).
    let mut p = MutableBlockPos::new(0, 0, 0);
    p.set_axis_cycle(&AxisCycle::Forward, 1, 2, 3);
    assert_eq!((p.get_x(), p.get_y(), p.get_z()), (3, 1, 2));
    let mut p2 = MutableBlockPos::new(0, 0, 0);
    p2.set_axis_cycle(&AxisCycle::Backward, 1, 2, 3);
    assert_eq!((p2.get_x(), p2.get_y(), p2.get_z()), (2, 3, 1));
}

#[test]
fn mutable_block_pos_clamp() {
    let mut p = MutableBlockPos::new(-5, 100, 5);
    p.clamp(&Axis::Y, -10, 10);
    assert_eq!(p.get_y(), 10);
    p.clamp(&Axis::X, 0, 0);
    assert_eq!(p.get_x(), 0);
}

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

#[test]
fn direction_values_and_tables() {
    assert_eq!(Direction::Down.get_3d_data_value(), 0);
    assert_eq!(Direction::East.get_3d_data_value(), 5);
    assert_eq!(Direction::North.get_2d_data_value(), 2);
    assert_eq!(Direction::South.get_2d_data_value(), 0);
    assert_eq!(Direction::West.get_2d_data_value(), 1);
    assert_eq!(Direction::East.get_2d_data_value(), 3);
    assert_eq!(Direction::Up.get_2d_data_value(), -1);
}

#[test]
fn direction_from_data_values() {
    // BY_3D_DATA[Mth.abs(-1 % 6)] = BY_3D_DATA[1] = UP.
    assert_eq!(Direction::from_3d_data_value(-1), Direction::Up);
    assert_eq!(Direction::from_3d_data_value(8), Direction::North);
    assert_eq!(Direction::from_2d_data_value(-1), Direction::West);
    assert_eq!(Direction::from_2d_data_value(6), Direction::North);
    assert_eq!(Direction::from_y_rot(45.0), Direction::West);
    assert_eq!(Direction::from_y_rot(-90.0), Direction::East);
}

#[test]
fn direction_opposite_and_axis() {
    assert_eq!(Direction::North.get_opposite(), Direction::South);
    assert_eq!(Direction::East.get_axis(), Axis::X);
    assert_eq!(Direction::Up.get_axis_direction(), AxisDirection::Positive);
    assert_eq!(
        Direction::West.get_axis_direction(),
        AxisDirection::Negative
    );
    assert_eq!(
        Direction::from_axis_and_direction(Axis::X, AxisDirection::Positive),
        Direction::East
    );
    assert_eq!(
        Direction::from_axis_and_direction(Axis::Y, AxisDirection::Negative),
        Direction::Down
    );
}

#[test]
fn direction_rotation_around_y() {
    assert_eq!(Direction::North.get_clock_wise(), Direction::East);
    assert_eq!(Direction::North.get_counter_clock_wise(), Direction::West);
    assert_eq!(Direction::North.get_clock_wise_x(), Direction::Down);
    assert_eq!(Direction::Down.get_clock_wise_x(), Direction::South);
    assert_eq!(Direction::Down.get_clock_wise_z(), Direction::West);
    assert_eq!(
        Direction::North.get_clock_wise_axis(Axis::Y),
        Direction::East
    );
}

#[test]
fn direction_y_rot() {
    // `toYRot()` = `(data2d & 3) * 90` — East's data2d is 3, so 270.
    assert_eq!(Direction::North.to_y_rot(), 180.0);
    assert_eq!(Direction::East.to_y_rot(), 270.0);
    assert_eq!(Direction::South.to_y_rot(), 0.0);
    // `getYRot(Direction)` is a separate static table.
    assert_eq!(Direction::get_y_rot(Direction::East), -90.0);
    assert_eq!(Direction::get_y_rot(Direction::South), 0.0);
}

#[test]
fn direction_nearest() {
    assert_eq!(Direction::get_nearest(1, 0, 0, None), Some(Direction::East));
    assert_eq!(
        Direction::get_nearest(0, 0, -1, None),
        Some(Direction::North)
    );
    assert_eq!(Direction::get_nearest(1, 5, 0, None), Some(Direction::Up));
    // Tie (1,1,0) — no axis dominates strictly; returns `or_else`.
    assert_eq!(Direction::get_nearest(1, 1, 0, None), None);
}

#[test]
fn direction_approximate_nearest() {
    assert_eq!(
        Direction::get_approximate_nearest_f32(1.0, 0.2, -0.1),
        Direction::East
    );
    assert_eq!(
        Direction::get_approximate_nearest(-1.0, 0.0, 0.0),
        Direction::West
    );
}

#[test]
fn direction_unit_and_steps() {
    assert_eq!(Direction::East.get_unit_vec3i(), Vec3i::new(1, 0, 0));
    assert_eq!(Direction::East.step_x(), 1);
    assert_eq!(Direction::Up.step_y(), 1);
    assert_eq!(Direction::North.step_z(), -1);
}

#[test]
fn axis_and_plane() {
    assert!(Axis::Y.is_vertical());
    assert!(Axis::X.is_horizontal());
    assert_eq!(Axis::X.get_positive(), Direction::East);
    assert_eq!(Axis::X.get_negative(), Direction::West);
    assert_eq!(Axis::Y.get_plane(), Plane::Vertical);
    assert_eq!(Axis::X.get_plane(), Plane::Horizontal);
    assert_eq!(Axis::X.choose(1, 2, 3), 1);
}

#[test]
fn axis_cycle_permutes() {
    assert_eq!(AxisCycle::Forward.cycle_xyz(1, 2, 3), (3, 1, 2));
    assert_eq!(AxisCycle::Backward.cycle_xyz(1, 2, 3), (2, 3, 1));
    assert_eq!(AxisCycle::Forward.cycle_axis(Axis::X), Axis::Y);
    assert_eq!(AxisCycle::Backward.cycle_axis(Axis::X), Axis::Z);
    assert_eq!(AxisCycle::between(Axis::X, Axis::Z), AxisCycle::Backward);
    assert_eq!(AxisCycle::Forward.inverse(), AxisCycle::Backward);
}

// ---------------------------------------------------------------------------
// SectionPos
// ---------------------------------------------------------------------------

#[test]
fn section_pos_long_roundtrip() {
    let l = SectionPos::as_long(3, 4, 5);
    assert_eq!(l, 13194144776196);
    assert_eq!(SectionPos::of_long(l), SectionPos::of(3, 4, 5));
    assert_eq!(
        (
            SectionPos::x_of(l),
            SectionPos::y_of(l),
            SectionPos::z_of(l)
        ),
        (3, 4, 5)
    );
}

#[test]
fn section_pos_coords() {
    assert_eq!(SectionPos::block_to_section_coord(35), 2);
    assert_eq!(SectionPos::block_to_section_coord_f64(-35.7), -3);
    assert_eq!(SectionPos::pos_to_section_coord(-35.7), -3);
    assert_eq!(SectionPos::section_relative(35), 3);
    assert_eq!(SectionPos::section_to_block_coord(3), 48);
    assert_eq!(SectionPos::section_to_block_coord_offset(3, 15), 63);
    let s = SectionPos::of(3, 4, 5);
    assert_eq!(s.min_block_x(), 48);
    assert_eq!(s.min_block_y(), 64);
    assert_eq!(s.max_block_z(), 95);
    assert_eq!(s.origin(), BlockPos::new(48, 64, 80));
    assert_eq!(s.center(), BlockPos::new(56, 72, 88));
}

#[test]
fn section_pos_relative() {
    let p = BlockPos::new(3, 6, 6);
    assert_eq!(SectionPos::section_relative_pos(&p), 870);
    assert_eq!(SectionPos::section_relative_x(870), 3);
    assert_eq!(SectionPos::section_relative_y(870), 6);
    assert_eq!(SectionPos::section_relative_z(870), 6);
    let s = SectionPos::of(3, 4, 5);
    assert_eq!(s.relative_to_block_pos(870), BlockPos::new(51, 70, 86));
}

#[test]
fn section_pos_offset_and_adjacent() {
    let sec = SectionPos::as_long(3, 4, 5);
    assert_eq!(SectionPos::offset(sec, 0, 1, 0), 13194144776197);
    assert_eq!(
        SectionPos::offset_dir(sec, &Direction::East),
        17592191287300
    );
    assert_eq!(
        SectionPos::get_adjacent_from_block_pos(33, 16777215, 68, &Direction::East),
        13194144776191
    );
    assert_eq!(
        SectionPos::get_adjacent_from_section_pos(3, 4, 5, &Direction::East),
        17592191287300
    );
}

#[test]
fn section_pos_zero_node_and_chunk() {
    let sec = SectionPos::as_long(3, 4, 5);
    assert_eq!(SectionPos::get_zero_node_long(sec), 13194144776192);
    assert_eq!(SectionPos::section_to_chunk(sec), 21474836483);
}

#[test]
fn section_pos_block_to_section() {
    // block(33, 63, 68) — section (2, 3, 4).
    let blk = BlockPos::as_long_coords(33, 63, 68);
    let sec = SectionPos::block_to_section(blk);
    assert_eq!(
        (
            SectionPos::x_of(sec),
            SectionPos::y_of(sec),
            SectionPos::z_of(sec)
        ),
        (2, 3, 4)
    );
    // y=4095 is the max 12-bit block y, so `(blockNode<<52)>>56` carries the
    // 0xFFF pattern into the arithmetic shift and sign-extends to -1.
    let blk2 = BlockPos::as_long_coords(33, 4095, 68);
    let sec2 = SectionPos::block_to_section(blk2);
    assert_eq!(
        (
            SectionPos::x_of(sec2),
            SectionPos::y_of(sec2),
            SectionPos::z_of(sec2)
        ),
        (2, -1, 4)
    );
    // blockPosAsSectionLong is the direct (x>>4, y>>4, z>>4) form.
    let l = SectionPos::block_pos_as_section_long(33, 63, 68);
    assert_eq!(
        (
            SectionPos::x_of(l),
            SectionPos::y_of(l),
            SectionPos::z_of(l)
        ),
        (2, 3, 4)
    );
}

#[test]
fn section_pos_blocks_inside_first_five() {
    let s = SectionPos::of(1, 2, 3);
    let blocks = s.blocks_inside();
    assert_eq!(
        &blocks[..5],
        &[
            BlockPos::new(16, 32, 48),
            BlockPos::new(17, 32, 48),
            BlockPos::new(18, 32, 48),
            BlockPos::new(19, 32, 48),
            BlockPos::new(20, 32, 48),
        ]
    );
    assert_eq!(blocks.len(), 4096);
}

#[test]
fn section_pos_cube() {
    let center = SectionPos::of(1, 2, 1);
    let cube = SectionPos::cube(&center, 1);
    // (0..=2) x (1..=3) x (0..=2) in X/Y/Z-major order = 27 sections.
    assert_eq!(cube.len(), 27);
    assert_eq!(cube[0], SectionPos::of(0, 1, 0));
    assert_eq!(cube[26], SectionPos::of(2, 3, 2));
}

#[test]
fn section_pos_around_and_at_block_pos() {
    // block(0,0,0) touches the corner of the 8 sections x∈[-1,0] etc.
    let corners = SectionPos::around_and_at_block_pos(&BlockPos::new(0, 0, 0));
    assert_eq!(corners.len(), 8);
    // A block with `x±1`, `y±1`, `z±1` all in the same section (e.g. x=8)
    // yields a single long.
    let single = SectionPos::around_and_at_block_pos(&BlockPos::new(8, 8, 8));
    assert_eq!(single, vec![SectionPos::as_long(0, 0, 0)]);
}

// ---------------------------------------------------------------------------
// ChunkPos
// ---------------------------------------------------------------------------

#[test]
fn chunk_pos_pack() {
    let p = ChunkPos::pack_coords(123456, -789012);
    assert_eq!(p, -3388780736028096);
    assert_eq!((ChunkPos::get_x(p), ChunkPos::get_z(p)), (123456, -789012));
    let n = ChunkPos::pack_coords(-5, -7);
    assert_eq!(n, -25769803781);
    assert_eq!(ChunkPos::unpack(n), ChunkPos::new(-5, -7));
    assert_eq!(ChunkPos::pack_coords(3, -5), -21474836477);
}

#[test]
fn chunk_pos_hash() {
    assert_eq!(ChunkPos::hash_coords(0, 0), 1455762653);
    assert_eq!(ChunkPos::hash_coords(1, 2), 1458653700);
    assert_eq!(ChunkPos::hash_coords(-5, -7), 895414371);
    assert_eq!(ChunkPos::hash_coords(i32::MIN, i32::MAX), 846615152);
    // Rust `Hash` must feed Java `hashCode` (`hash(x, z)`) into the hasher —
    // verified with a capturing hasher — while staying consistent with `Eq`.
    use std::hash::{Hash, Hasher};
    #[derive(Default)]
    struct Capture;
    impl Hasher for Capture {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, _bytes: &[u8]) {}
        fn write_i32(&mut self, i: i32) {
            assert_eq!(i, ChunkPos::hash_coords(1, 2));
        }
    }
    ChunkPos::new(1, 2).hash(&mut Capture);
}

#[test]
fn chunk_pos_valid_bound() {
    // `ChunkPyramid.MAX_CHUNK_COORDINATE_VALUE` is
    //   SectionPos.blockToSectionCoord(BlockPos.MAX_HORIZONTAL_COORDINATE)
    //     - SAFETY_MARGIN_CHUNKS
    // with SAFETY_MARGIN_CHUNKS = (32 + FULL.accumulatedDependencies().size() + 1) * 2.
    // Replaying the GENERATION_PYRAMID builder (ChunkStep.Builder +
    // ChunkPyramid.GENERATION_PYRAMID in working/Paper) gives
    // accumulatedDependencies().size() == 12, so:
    assert_eq!(
        MAX_CHUNK_COORDINATE_VALUE,
        (33554431 >> 4) - (32 + 12 + 1) * 2
    );
    assert_eq!(MAX_CHUNK_COORDINATE_VALUE, 2097061);
    assert!(ChunkPos::new(0, 0).is_valid());
    assert!(ChunkPos::new(2097061, 0).is_valid());
    assert!(!ChunkPos::new(2097062, 0).is_valid());
    assert!(ChunkPos::new(-2097061, 2097061).is_valid());
}

#[test]
fn chunk_pos_coords() {
    let c = ChunkPos::new(3, -5);
    assert_eq!(c.to_string(), "[3, -5]");
    assert_eq!(c.pack(), -21474836477);
    assert_eq!(c.get_min_block_x(), 48);
    assert_eq!(c.get_max_block_z(), -65);
    assert_eq!(c.get_middle_block_x(), 56);
    assert_eq!(c.get_block_at(1, 2, 3), BlockPos::new(49, 2, -77));
    assert_eq!(c.get_region_x(), 0);
    assert_eq!(c.get_region_local_x(), 3);
}

#[test]
fn chunk_pos_contains_and_distance() {
    let c = ChunkPos::new(3, -5);
    assert!(c.contains(&BlockPos::new(49, 2, -77)));
    assert!(c.contains(&BlockPos::new(48, 2, -80)));
    assert!(!c.contains(&BlockPos::new(47, 2, -77)));
    assert_eq!(c.distance_squared(&ChunkPos::new(5, -7)), 8);
    assert_eq!(c.get_chessboard_distance(&ChunkPos::new(7, -7)), 4);
    assert_eq!(c.get_world_position(), BlockPos::new(48, 0, -80));
}

#[test]
fn chunk_pos_range_closed() {
    let r = ChunkPos::range_closed(&ChunkPos::new(1, 1), 1);
    let expected: Vec<ChunkPos> = vec![
        ChunkPos::new(0, 0),
        ChunkPos::new(1, 0),
        ChunkPos::new(2, 0),
        ChunkPos::new(0, 1),
        ChunkPos::new(1, 1),
        ChunkPos::new(2, 1),
        ChunkPos::new(0, 2),
        ChunkPos::new(1, 2),
        ChunkPos::new(2, 2),
    ];
    assert_eq!(r, expected);
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

#[test]
fn rotation_get_rotated() {
    assert_eq!(
        Rotation::Clockwise90.get_rotated(Rotation::Clockwise90),
        Rotation::Clockwise180
    );
    assert_eq!(
        Rotation::Clockwise180.get_rotated(Rotation::Clockwise180),
        Rotation::None
    );
    assert_eq!(
        Rotation::Clockwise90.get_rotated(Rotation::Counterclockwise90),
        Rotation::None
    );
}

#[test]
fn rotation_display_matches_java_to_string() {
    // Java `Rotation` does not override `toString()`, so `Display` mirrors the
    // enum constant names (distinct from `getSerializedName()`).
    assert_eq!(Rotation::None.to_string(), "NONE");
    assert_eq!(Rotation::Clockwise90.to_string(), "CLOCKWISE_90");
    assert_eq!(Rotation::Clockwise180.to_string(), "CLOCKWISE_180");
    assert_eq!(
        Rotation::Counterclockwise90.to_string(),
        "COUNTERCLOCKWISE_90"
    );
    // The serialized names remain separately exposed.
    assert_eq!(Rotation::Clockwise180.get_serialized_name(), "180");
}

#[test]
fn rotation_rotate_direction() {
    assert_eq!(
        Rotation::Clockwise90.rotate(&Direction::North),
        Direction::East
    );
    assert_eq!(
        Rotation::Clockwise180.rotate(&Direction::North),
        Direction::South
    );
    assert_eq!(
        Rotation::Counterclockwise90.rotate(&Direction::North),
        Direction::West
    );
    assert_eq!(Rotation::Clockwise90.rotate(&Direction::Up), Direction::Up);
}

// ---------------------------------------------------------------------------
// GlobalPos
// ---------------------------------------------------------------------------

fn overworld() -> ResourceKey<rivet_registry::registries::Level> {
    ResourceKey::create(&DIMENSION, Identifier::parse("minecraft:overworld"))
}

#[test]
fn global_pos_of_and_accessors() {
    let dim = overworld();
    let pos = BlockPos::new(1, 2, 3);
    let g = GlobalPos::of(dim.clone(), pos);
    assert_eq!(g.dimension(), &dim);
    assert_eq!(g.pos(), pos);
}

#[test]
fn global_pos_is_close_enough() {
    // Java `GlobalPos.isCloseEnough`: dimensions equal AND
    // `pos.distChessboard(pos) <= maxDistance`.
    let dim = overworld();
    let g = GlobalPos::of(dim.clone(), BlockPos::new(0, 0, 0));
    // Chessboard distance max(|dx|,|dy|,|dz|) = max(3, 2, 1) = 3 <= 3.
    assert!(g.is_close_enough(&dim, &BlockPos::new(3, -2, 1), 3));
    // max(4, 0, 0) = 4 > 3.
    assert!(!g.is_close_enough(&dim, &BlockPos::new(4, 0, 0), 3));
    // Dimension mismatch never matches, regardless of distance.
    let other_dim = ResourceKey::create(&DIMENSION, Identifier::parse("minecraft:the_nether"));
    assert!(!g.is_close_enough(&other_dim, &BlockPos::new(0, 0, 0), 100));
}

#[test]
fn global_pos_to_string_matches_java() {
    // Java `GlobalPos.toString()` = `dimension + " " + pos`; `ResourceKey`
    // renders `ResourceKey[registry / identifier]`, `BlockPos` renders
    // `BlockPos{x=…, y=…, z=…}`.
    let dim = overworld();
    let g = GlobalPos::of(dim, BlockPos::new(1, 2, 3));
    assert_eq!(
        g.to_string(),
        "ResourceKey[minecraft:dimension / minecraft:overworld] BlockPos{x=1, y=2, z=3}"
    );
}

#[test]
fn global_pos_value_semantics() {
    // Java record value semantics: `equals` compares both components.
    let dim = overworld();
    assert_eq!(
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 3)),
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 3))
    );
    // A different `BlockPos` is unequal.
    assert_ne!(
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 3)),
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 4))
    );
    // A different dimension key is unequal even for the same position.
    let nether = ResourceKey::create(&DIMENSION, Identifier::parse("minecraft:the_nether"));
    assert_ne!(
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 3)),
        GlobalPos::of(nether, BlockPos::new(1, 2, 3))
    );
    // `Hash` agrees with `Eq` (equal values hash identically).
    use std::collections::HashSet;
    let set: HashSet<GlobalPos> = [
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 3)),
        GlobalPos::of(dim.clone(), BlockPos::new(1, 2, 3)),
        GlobalPos::of(dim.clone(), BlockPos::new(9, 9, 9)),
    ]
    .into_iter()
    .collect();
    assert_eq!(set.len(), 2);
}

#[test]
fn block_pos_dist_chessboard() {
    // `Vec3i.distChessboard` on a `BlockPos` — max of the axis deltas.
    let a = BlockPos::new(0, 0, 0);
    assert_eq!(a.dist_chessboard(&BlockPos::new(3, -2, 1)), 3);
    assert_eq!(a.dist_chessboard(&BlockPos::new(0, 0, 0)), 0);
    // Wrapping subtraction matches Java: `Integer.MIN_VALUE -
    // Integer.MAX_VALUE` wraps to 1 in Java's int arithmetic (and in the
    // Rust port), so `distChessboard` = max(1, 0, 0) = 1.
    assert_eq!(
        BlockPos::new(i32::MIN, 0, 0).dist_chessboard(&BlockPos::new(i32::MAX, 0, 0)),
        1
    );
}

//! Unit tests for the shape port. The expectations are derived from the Java
//! source's own semantics; the byte-level differential against Paper is the
//! `shapes.eval` oracle op (`tools/rivet-parity`).

use rivet_registry::core::{Axis, BlockPos, Direction};
use rivet_registry::world::phys::{AABB, Vec3};

use super::boolean_op::BooleanOp;
use super::shapes;
use crate::spottedleaf::moonrise::patches::collisions::collision_util;

#[test]
fn block_is_the_unit_cube() {
    let block = shapes::block();
    assert!(!block.is_empty());
    assert_eq!(block.bounds(), AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0));
    assert!(block.moonrise_is_full_block());
    assert!(block.moonrise_occludes_full_block());
    assert!(shapes::empty().is_empty());
}

#[test]
fn find_bits_finds_the_smallest_grid() {
    assert_eq!(shapes::find_bits(0.0, 1.0), 0);
    assert_eq!(shapes::find_bits(0.0, 0.5), 1);
    assert_eq!(shapes::find_bits(0.25, 0.75), 2);
    assert_eq!(shapes::find_bits(0.125, 0.875), 3);
    assert_eq!(shapes::find_bits(0.1, 0.7), -1);
    assert_eq!(shapes::find_bits(-0.5, 0.5), -1);
    assert_eq!(shapes::find_bits(0.0, 1.5), -1);
}

#[test]
fn box_rejects_min_above_max() {
    let result = std::panic::catch_unwind(|| shapes::box_(1.0, 0.0, 0.0, 0.0, 1.0, 1.0));
    assert!(result.is_err());
}

#[test]
fn degenerate_boxes_are_empty() {
    assert!(shapes::create(0.0, 0.0, 0.0, 1.0E-8, 1.0, 1.0).is_empty());
}

#[test]
fn collide_stops_flush_against_the_face() {
    let half = shapes::box_(0.0, 0.0, 0.0, 0.5, 1.0, 1.0);
    let source = AABB::new(0.5, 0.0, 0.0, 1.5, 1.0, 1.0);
    // The source starts touching the target's +X face: it cannot move into it.
    assert_eq!(half.collide(Axis::X, &source, -1.0), 0.0);
    // Away from it, the move is free.
    assert_eq!(half.collide(Axis::X, &source, 1.0), 1.0);
    // Starting 0.25 away it stops at the face.
    let away = AABB::new(0.75, 0.0, 0.0, 1.75, 1.0, 1.0);
    assert_eq!(half.collide(Axis::X, &away, -1.0), -0.25);
}

#[test]
fn or_of_two_halves_optimizes_to_the_block() {
    let left = shapes::box_(0.0, 0.0, 0.0, 0.5, 1.0, 1.0);
    let right = shapes::box_(0.5, 0.0, 0.0, 1.0, 1.0, 1.0);
    let joined = shapes::or(&left, &right);
    assert!(joined.ptr_eq(&shapes::block()));
    let unoptimized = shapes::join_unoptimized(&left, &right, BooleanOp::OR);
    assert!(!unoptimized.ptr_eq(&shapes::block()));
    assert!(unoptimized.moonrise_is_full_block());
}

#[test]
fn only_first_carves_the_second_out() {
    let cauldron = shapes::join(
        &shapes::block(),
        &shapes::box_(0.125, 0.25, 0.125, 0.875, 1.0, 0.875),
        BooleanOp::ONLY_FIRST,
    );
    assert!(!cauldron.is_empty());
    assert!(collision_util::strictly_contains(&cauldron, 0.05, 0.5, 0.5));
    assert!(!collision_util::strictly_contains(&cauldron, 0.5, 0.5, 0.5));
    assert!(collision_util::strictly_contains(&cauldron, 0.5, 0.1, 0.5));
}

#[test]
fn a_join_operator_true_on_false_false_is_unsupported() {
    let result = std::panic::catch_unwind(|| {
        shapes::join_unoptimized(&shapes::block(), &shapes::block(), BooleanOp::TRUE)
    });
    assert!(result.is_err());
}

#[test]
fn move_offsets_the_shape_and_its_aabbs() {
    let post = shapes::box_(0.375, 0.0, 0.375, 0.625, 1.0, 0.625);
    let moved = post.move_xyz(1.0, 2.0, 3.0);
    assert_eq!(
        moved.bounds(),
        AABB::new(1.375, 2.0, 3.375, 1.625, 3.0, 3.625)
    );
    assert_eq!(
        moved.to_aabbs().as_slice(),
        &[AABB::new(1.375, 2.0, 3.375, 1.625, 3.0, 3.625)]
    );
}

#[test]
fn face_shape_of_a_slab_is_the_empty_or_the_full_face() {
    let slab = shapes::box_(0.0, 0.0, 0.0, 1.0, 0.5, 1.0);
    assert!(slab.get_face_shape(Direction::Up).is_empty());
    let down = slab.get_face_shape(Direction::Down);
    assert!(down.moonrise_is_full_block() || !down.is_empty());
    assert!(slab.get_face_shape(Direction::North).bounds().max_y <= 0.5);
}

#[test]
fn clip_hits_the_near_face() {
    let block = shapes::block();
    let hit = block
        .clip(
            &Vec3::new(-1.0, 0.5, 0.5),
            &Vec3::new(2.0, 0.5, 0.5),
            &BlockPos::new(0, 0, 0),
        )
        .expect("the ray crosses the block");
    assert_eq!(hit.get_direction(), Direction::West);
    assert!(!hit.is_inside());
}

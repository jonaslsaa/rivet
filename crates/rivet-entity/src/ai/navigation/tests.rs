//! Comprehensive unit tests for Paper 26.2 pathfinding, navigation,
//! movement/look/jump/body controls, and targeting conditions.

use std::collections::HashMap;

use crate::ai::goal::context::{BlockPos, EntityId, Vec3};
use crate::ai::navigation::{
    AmphibiousNodeEvaluator, BinaryHeap, BlockPathInfo, BodyRotationControl, Difficulty,
    FlyNodeEvaluator, FlyingMoveControl, JumpControl, LookControl, MobPathingState, MoveControl,
    MoveOperation, Node, NodeArena, Path, PathFinder, PathNavigation, PathType, PathTypeCache,
    PathWorldView, PathfindingContext, SmoothSwimmingLookControl, SmoothSwimmingMoveControl,
    SwimNodeEvaluator, Target, TargetEntityView, TargetingConditions, WalkNodeEvaluator,
    check_neighbour_blocks, get_path_type_static, rotate_towards, rotlerp,
};

struct TestWorld {
    blocks: HashMap<BlockPos, BlockPathInfo>,
    default_floor_y: Option<i32>,
    bright_outside: bool,
    sea_level: i32,
}

impl TestWorld {
    fn flat(floor_y: i32) -> Self {
        Self {
            blocks: HashMap::new(),
            default_floor_y: Some(floor_y),
            bright_outside: false,
            sea_level: 63,
        }
    }

    fn empty() -> Self {
        Self {
            blocks: HashMap::new(),
            default_floor_y: None,
            bright_outside: false,
            sea_level: 63,
        }
    }

    fn set(&mut self, x: i32, y: i32, z: i32, info: BlockPathInfo) {
        self.blocks.insert(BlockPos::new(x, y, z), info);
    }

    fn fill_water(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    self.set(x, y, z, BlockPathInfo::WATER);
                }
            }
        }
    }
}

impl PathWorldView for TestWorld {
    fn sea_level(&self) -> i32 {
        self.sea_level
    }

    fn is_bright_outside(&self) -> bool {
        self.bright_outside
    }

    fn block_info(&self, pos: BlockPos) -> BlockPathInfo {
        if let Some(&info) = self.blocks.get(&pos) {
            return info;
        }
        if let Some(floor_y) = self.default_floor_y
            && pos.y <= floor_y
        {
            return BlockPathInfo::SOLID;
        }
        BlockPathInfo::AIR
    }
}

#[derive(Debug, Clone)]
struct TestEntity {
    id: EntityId,
    pos: Vec3,
    can_be_seen: bool,
    can_be_seen_as_enemy: bool,
    can_attack: bool,
    allied: bool,
    visibility: f64,
    los: bool,
    is_mob: bool,
}

impl TestEntity {
    fn new(id: u64, pos: Vec3) -> Self {
        Self {
            id: EntityId(id),
            pos,
            can_be_seen: true,
            can_be_seen_as_enemy: true,
            can_attack: true,
            allied: false,
            visibility: 1.0,
            los: true,
            is_mob: true,
        }
    }
}

impl TargetEntityView for TestEntity {
    fn entity_id(&self) -> EntityId {
        self.id
    }

    fn pos(&self) -> Vec3 {
        self.pos
    }

    fn can_be_seen_by_anyone(&self) -> bool {
        self.can_be_seen
    }

    fn can_be_seen_as_enemy(&self) -> bool {
        self.can_be_seen_as_enemy
    }

    fn can_attack(&self, _target_id: EntityId) -> bool {
        self.can_attack
    }

    fn is_allied_to(&self, _target_id: EntityId) -> bool {
        self.allied
    }

    fn visibility_percent(&self, _targeter: Option<EntityId>) -> f64 {
        self.visibility
    }

    fn has_line_of_sight(&self, _target_id: EntityId) -> bool {
        self.los
    }

    fn is_mob(&self) -> bool {
        self.is_mob
    }
}

// ---------------------------------------------------------------------------
// Node, Target, BinaryHeap, PathTypeCache
// ---------------------------------------------------------------------------

#[test]
fn node_create_hash_matches_paper_java_bit_layout() {
    // Verified directly against Java:
    // y & 0xFF | (x & 32767) << 8 | (z & 32767) << 24 | (x < 0 ? Integer.MIN_VALUE : 0) | (z < 0 ? 32768 : 0)
    assert_eq!(Node::create_hash(0, 0, 0), 0);
    assert_eq!(Node::create_hash(1, 64, 2), 64 | (1 << 8) | (2 << 24));
    let (x, y, z) = (-1_i32, -64_i32, -1_i32);
    assert_eq!(
        Node::create_hash(x, y, z),
        (y & 0xFF) | ((x & 32767) << 8) | ((z & 32767) << 24) | i32::MIN | 32768
    );
}

#[test]
fn target_update_best_strict_less_preserves_first_on_tie() {
    let mut target = Target::new(10, 0, 0);
    target.update_best(5.0, 1);
    target.update_best(5.0, 2); // tie must keep node 1
    assert_eq!(target.best_node(), Some(1));
    target.update_best(4.99, 3);
    assert_eq!(target.best_node(), Some(3));
}

#[test]
fn binary_heap_ordering_change_cost_remove_and_resize() {
    let mut arena = NodeArena::new();
    let mut heap = BinaryHeap::new();

    // Insert > 128 nodes to exercise doubling resize
    for i in 0..160_i32 {
        let idx = arena.get_or_create(i, 0, 0);
        arena.get_mut(idx).f = (160 - i) as f32;
        heap.insert(&mut arena, idx);
    }
    assert_eq!(heap.size(), 160);

    // Lower cost of node 0 (initially f=160) to 0.5 -> becomes root
    let n0 = arena.get_or_create(0, 0, 0);
    heap.change_cost(&mut arena, n0, 0.5);
    assert_eq!(heap.peek(), n0);

    // Remove root explicitly
    heap.remove(&mut arena, n0);
    assert_eq!(arena.get(n0).heap_idx, -1);
    assert_eq!(heap.size(), 159);

    // Pop remaining in non-decreasing f order
    let mut prev = f32::NEG_INFINITY;
    while !heap.is_empty() {
        let idx = heap.pop(&mut arena);
        let f = arena.get(idx).f;
        assert!(f >= prev, "heap violated min-order: {f} < {prev}");
        assert_eq!(arena.get(idx).heap_idx, -1);
        prev = f;
    }
}

#[test]
#[should_panic(expected = "already in BinaryHeap")]
fn binary_heap_rejects_duplicate_insert() {
    let mut arena = NodeArena::new();
    let mut heap = BinaryHeap::new();
    let idx = arena.get_or_create(1, 2, 3);
    heap.insert(&mut arena, idx);
    heap.insert(&mut arena, idx);
}

#[test]
fn path_type_cache_hits_and_invalidates() {
    let mut cache = PathTypeCache::new();
    let pos = BlockPos::new(12, 64, -34);
    let mut calls = 0;

    let first = cache.get_or_compute(pos, |_| {
        calls += 1;
        PathType::Walkable
    });
    let second = cache.get_or_compute(pos, |_| {
        calls += 1;
        PathType::Blocked
    });
    assert_eq!(first, PathType::Walkable);
    assert_eq!(second, PathType::Walkable);
    assert_eq!(calls, 1);

    cache.invalidate(pos);
    let third = cache.get_or_compute(pos, |_| {
        calls += 1;
        PathType::Blocked
    });
    assert_eq!(third, PathType::Blocked);
    assert_eq!(calls, 2);
}

// ---------------------------------------------------------------------------
// WalkNodeEvaluator & PathFinder
// ---------------------------------------------------------------------------

#[test]
fn walk_path_type_static_and_neighbor_priority() {
    let mut world = TestWorld::flat(0);
    world.set(1, 1, 0, BlockPathInfo::FIRE);
    world.set(-1, 1, 0, BlockPathInfo::DAMAGING);

    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
    // Cactus (DAMAGING) beats FIRE in neighbor scan
    assert_eq!(
        get_path_type_static(&mut ctx, 0, 1, 0),
        PathType::DangerOther
    );

    // Without cactus, fire produces DangerFire
    let mut world2 = TestWorld::flat(0);
    world2.set(1, 1, 0, BlockPathInfo::FIRE);
    let mut ctx2 = PathfindingContext::new(&world2, None, BlockPos::new(0, 1, 0));
    assert_eq!(
        get_path_type_static(&mut ctx2, 0, 1, 0),
        PathType::DangerFire
    );

    // Adjacent water produces WaterBorder
    let mut world3 = TestWorld::flat(0);
    world3.set(0, 1, 1, BlockPathInfo::WATER);
    let mut ctx3 = PathfindingContext::new(&world3, None, BlockPos::new(0, 1, 0));
    assert_eq!(
        check_neighbour_blocks(&mut ctx3, 0, 1, 0, PathType::Walkable),
        PathType::WaterBorder
    );
}

#[test]
fn walk_path_finder_navigates_around_wall_and_respects_unloaded() {
    let mut world = TestWorld::flat(0);
    // Wall at x=3, z in -2..=2, height 2
    for z in -2..=2 {
        world.set(3, 1, z, BlockPathInfo::SOLID);
        world.set(3, 2, z, BlockPathInfo::SOLID);
    }

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut eval = WalkNodeEvaluator::new();
    let mut finder = PathFinder::new(200).with_debug(true);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut mob,
            &[BlockPos::new(6, 1, 0)],
            32.0,
            0,
            1.0,
        )
        .expect("should find path around wall");

    assert!(path.can_reach());
    assert_eq!(
        path.get_end_node().unwrap().as_block_pos(),
        BlockPos::new(6, 1, 0)
    );
    // Must not step through the wall
    for n in path.nodes() {
        assert!(
            !(n.x == 3 && (-2..=2).contains(&n.z)),
            "path cut through wall at {n:?}"
        );
    }
    assert!(path.debug_data().is_some());
}

#[test]
fn walk_path_finder_controlled_negative_blocked_by_enclosure() {
    let mut world = TestWorld::flat(0);
    // Completely enclose target at (4, 1, 0) with 2-high solid walls
    for &(x, z) in &[
        (3, 0),
        (5, 0),
        (4, -1),
        (4, 1),
        (3, -1),
        (3, 1),
        (5, -1),
        (5, 1),
    ] {
        world.set(x, 1, z, BlockPathInfo::SOLID);
        world.set(x, 2, z, BlockPathInfo::SOLID);
    }

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut eval = WalkNodeEvaluator::new();
    let mut finder = PathFinder::new(100);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut mob,
            &[BlockPos::new(4, 1, 0)],
            16.0,
            0,
            1.0,
        )
        .expect("returns partial best path even when unreachable");

    assert!(!path.can_reach());
    assert_ne!(
        path.get_end_node().unwrap().as_block_pos(),
        BlockPos::new(4, 1, 0)
    );
}

#[test]
fn walk_path_finder_step_up_and_door_opening() {
    let mut world = TestWorld::flat(0);
    // Step-up ledge at x=2 (floor raised to y=1, standing at y=2)
    world.set(2, 1, 0, BlockPathInfo::SOLID);
    // Closed wooden door at (1, 1, 0)
    world.set(1, 1, 0, BlockPathInfo::WOOD_DOOR_CLOSED);
    // Block side bypasses so mob must use door
    for z in [-1, 1] {
        world.set(1, 1, z, BlockPathInfo::SOLID);
        world.set(1, 2, z, BlockPathInfo::SOLID);
        world.set(2, 1, z, BlockPathInfo::SOLID);
        world.set(2, 2, z, BlockPathInfo::SOLID);
    }

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut eval = WalkNodeEvaluator::new();
    eval.config.can_open_doors = true;
    eval.config.can_pass_doors = true;

    let mut finder = PathFinder::new(200);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut mob,
            &[BlockPos::new(2, 2, 0)],
            16.0,
            0,
            1.0,
        )
        .expect("should pass wooden door and step up");
    assert!(path.can_reach());
    assert_eq!(
        path.get_end_node().unwrap().as_block_pos(),
        BlockPos::new(2, 2, 0)
    );
}

#[test]
fn walk_path_finder_unloaded_block_is_blocked() {
    let mut world = TestWorld::flat(0);
    world.set(1, 1, 0, BlockPathInfo::UNLOADED);
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
    assert_eq!(get_path_type_static(&mut ctx, 1, 1, 0), PathType::Blocked);
}

// ---------------------------------------------------------------------------
// FlyNodeEvaluator, SwimNodeEvaluator, AmphibiousNodeEvaluator
// ---------------------------------------------------------------------------

#[test]
fn fly_evaluator_paths_over_wall_in_3d() {
    let mut world = TestWorld::flat(0);
    for z in -3..=3 {
        world.set(2, 1, z, BlockPathInfo::SOLID);
        world.set(2, 2, z, BlockPathInfo::SOLID);
    }
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.6);
    mob.on_ground = false;

    let mut eval = FlyNodeEvaluator::new();
    let mut finder = PathFinder::new(200);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut mob,
            &[BlockPos::new(4, 1, 0)],
            16.0,
            0,
            1.0,
        )
        .expect("flying mob should fly over wall");
    assert!(path.can_reach());
    assert!(
        path.nodes().iter().any(|n| n.y >= 3),
        "expected flight above wall"
    );
}

#[test]
fn swim_evaluator_navigates_underwater_and_handles_breach() {
    let mut world = TestWorld::empty();
    world.fill_water(-2, 0, -2, 6, 3, 2);

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.6);
    mob.in_water = true;
    mob.on_ground = false;
    mob.set_pathfinding_malus(PathType::Water, 0.0);

    let mut eval = SwimNodeEvaluator::new(false);
    let mut finder = PathFinder::new(200);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut mob,
            &[BlockPos::new(4, 2, 0)],
            16.0,
            0,
            1.0,
        )
        .expect("should swim underwater");
    assert!(path.can_reach());
    assert_eq!(
        path.get_end_node().unwrap().as_block_pos(),
        BlockPos::new(4, 2, 0)
    );

    // Controlled negative: without breaching, air at y=4 is blocked
    let path_air = finder
        .find_path(
            &mut eval,
            &world,
            &mut mob,
            &[BlockPos::new(0, 4, 0)],
            16.0,
            0,
            1.0,
        )
        .unwrap();
    assert!(!path_air.can_reach());
}

#[test]
fn amphibious_evaluator_restores_malus_on_done() {
    let mut world = TestWorld::flat(0);
    world.fill_water(2, 1, -1, 5, 2, 1);

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.6);
    mob.set_pathfinding_malus(PathType::Walkable, 0.0);
    mob.set_pathfinding_malus(PathType::WaterBorder, 8.0);

    let mut eval = AmphibiousNodeEvaluator::new(true);
    let mut finder = PathFinder::new(200);
    let _ = finder.find_path(
        &mut eval,
        &world,
        &mut mob,
        &[BlockPos::new(4, 1, 0)],
        16.0,
        0,
        1.0,
    );

    // Paper restores Walkable & WaterBorder maluses in done()
    assert_eq!(mob.get_pathfinding_malus(PathType::Walkable), 0.0);
    assert_eq!(mob.get_pathfinding_malus(PathType::WaterBorder), 8.0);
}

// ---------------------------------------------------------------------------
// PathNavigation variants
// ---------------------------------------------------------------------------

#[test]
fn ground_navigation_follows_path_and_detects_stuck() {
    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    mob.speed = 0.25;

    let mut nav = PathNavigation::ground(200);
    let mut move_control = MoveControl::new();

    assert!(nav.move_to_xyz(&mut mob, &world, 4.5, 1.0, 0.5, 1.0, 0));
    assert!(nav.is_in_progress());

    // One tick sets MoveControl wanted position
    nav.tick(&mut mob, &world, &mut move_control, 1);
    assert!(move_control.has_wanted());

    // Standing still for > 100 ticks triggers stuck detection and stops path
    for t in 2..=105 {
        nav.tick(&mut mob, &world, &mut move_control, t);
    }
    assert!(nav.is_stuck());
    assert!(nav.is_done());
}

#[test]
fn ground_navigation_avoid_sun_trims_sky_exposed_nodes() {
    let mut world = TestWorld::flat(0);
    world.bright_outside = true;
    // Shaded start at (0, 1, 0) and (1, 1, 0), sun-exposed at (2, 1, 0) onward
    let mut shaded_air = BlockPathInfo::AIR;
    shaded_air.can_see_sky = false;
    world.set(0, 1, 0, shaded_air);
    world.set(1, 1, 0, shaded_air);

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut nav = PathNavigation::ground(200);
    nav.set_avoid_sun(true);

    let path = nav
        .create_path(&mut mob, &world, BlockPos::new(4, 1, 0), 0, 0)
        .expect("path created then trimmed");
    assert!(path.get_node_count() <= 2);
}

#[test]
fn wall_climber_navigation_falls_back_to_direct_move_control() {
    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 1.4, 0.9);
    let mut nav = PathNavigation::wall_climber(200);
    let mut move_control = MoveControl::new();

    // Create path records path_to_position; stopping path leaves fallback active
    let _ = nav.create_path(&mut mob, &world, BlockPos::new(5, 1, 0), 0, 0);
    nav.stop();
    nav.tick(&mut mob, &world, &mut move_control, 1);
    assert!(move_control.has_wanted());
    assert_eq!(move_control.wanted_x, 5.0);
}

#[test]
fn navigation_corner_cutting_rules_match_paper() {
    assert!(PathNavigation::can_cut_corner(PathType::Walkable));
    assert!(!PathNavigation::can_cut_corner(PathType::DangerFire));
    assert!(!PathNavigation::can_cut_corner(PathType::DangerOther));
    assert!(!PathNavigation::can_cut_corner(PathType::WalkableDoor));
}

#[test]
fn path_same_as_and_cursor_helpers() {
    let n0 = Node::new(0, 1, 0);
    let n1 = Node::new(1, 1, 0);
    let mut p1 = Path::new(vec![n0.clone(), n1.clone()], BlockPos::new(1, 1, 0), true);
    let p2 = Path::new(vec![n0, n1], BlockPos::new(1, 1, 0), true);
    assert!(p1.same_as(Some(&p2)));
    assert!(!p1.same_as(None));
    assert!(p1.not_started());
    assert!(p1.get_previous_node().is_none());
    p1.advance();
    assert!(!p1.not_started());
    assert_eq!(p1.get_previous_node().unwrap().x, 0);
    p1.advance();
    assert!(p1.is_done());
}

// ---------------------------------------------------------------------------
// Controls: Move, FlyingMove, SmoothSwimmingMove, Look, Jump, BodyRotation
// ---------------------------------------------------------------------------

#[test]
fn move_control_moves_rotates_jumps_and_strafes() {
    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.0, 1.0, 0.0), 0.6, 1.8);
    let mut mc = MoveControl::new();
    let mut jc = JumpControl::new();

    // Move upward step triggers JumpControl + Jumping operation
    mc.set_wanted_position(0.5, 2.0, 0.0, 1.0);
    mc.tick(&mut mob, &world, &mut jc, |_, _, _| PathType::Walkable);
    assert_eq!(mc.operation, MoveOperation::Jumping);
    assert!(jc.is_jump_queued());

    jc.tick(&mut mob);
    assert!(mob.jumping);
    jc.tick(&mut mob);
    assert!(!mob.jumping);

    // Strafe into non-walkable falls back to forwards=1.0, right=0.0
    mc.strafe(0.0, 1.0);
    mc.tick(&mut mob, &world, &mut jc, |_, _, _| PathType::Blocked);
    assert_eq!(mc.strafe_forwards, 1.0);
    assert_eq!(mc.strafe_right, 0.0);
}

#[test]
fn flying_and_smooth_swimming_move_controls() {
    let mut mob = MobPathingState::new(Vec3::new(0.0, 5.0, 0.0), 0.6, 0.6);
    mob.on_ground = false;
    let mut fmc = FlyingMoveControl::new(20, false);
    fmc.set_wanted_position(5.0, 8.0, 0.0, 1.0);
    fmc.tick(&mut mob);
    assert!(mob.no_gravity);
    assert!(mob.yya > 0.0);

    // Idle re-enables gravity when not hovering in place
    fmc.tick(&mut mob);
    assert!(!mob.no_gravity);
    assert_eq!(mob.yya, 0.0);

    // SmoothSwimmingMoveControl turning speed factor curve
    assert_eq!(SmoothSwimmingMoveControl::turning_speed_factor(5.0), 1.0);
    assert_eq!(SmoothSwimmingMoveControl::turning_speed_factor(10.0), 1.0);
    assert!((SmoothSwimmingMoveControl::turning_speed_factor(35.0) - 0.5).abs() < 1e-5);
    assert_eq!(SmoothSwimmingMoveControl::turning_speed_factor(60.0), 0.0);
    assert_eq!(SmoothSwimmingMoveControl::turning_speed_factor(90.0), 0.0);

    let mut swimmer = MobPathingState::new(Vec3::new(0.0, 10.0, 0.0), 0.9, 0.6);
    swimmer.in_water = true;
    let mut smc = SmoothSwimmingMoveControl::new(85, 10, 0.02, 0.1, true);
    smc.set_wanted_position(0.0, 12.0, 5.0, 1.0);
    smc.tick(&mut swimmer, false);
    assert!((swimmer.delta_movement.y - 0.005).abs() < 1e-9);
    assert!(swimmer.speed > 0.0);
}

#[test]
fn look_smooth_look_and_body_rotation_controls() {
    let mut mob = MobPathingState::new(Vec3::new(0.0, 0.0, 0.0), 0.6, 1.8);
    let mut lc = LookControl::new();
    lc.set_look_at(10.0, mob.eye_y(), 0.0, 30.0, 30.0);
    assert!(lc.is_looking_at_target());
    lc.tick(&mut mob, true, true);
    // Looking towards +X is yaw -90; capped by 30 deg step from 0 -> -30
    assert!((mob.y_head_rot - (-30.0)).abs() < 1e-3);

    // SmoothSwimmingLookControl rotates body by ±4 when head exceeds max_y_rot_from_center
    let mut swimmer = MobPathingState::new(Vec3::ZERO, 0.9, 0.6);
    swimmer.y_head_rot = 40.0;
    swimmer.y_body_rot = 0.0;
    let mut slc = SmoothSwimmingLookControl::new(10);
    slc.base.y_max_rot_speed = 0.0;
    slc.tick(&mut swimmer, true);
    assert_eq!(swimmer.y_body_rot, 4.0);

    // BodyRotationControl gradually faces head after 10 stable ticks
    let mut brc = BodyRotationControl::new();
    let mut standing = MobPathingState::new(Vec3::ZERO, 0.6, 1.8);
    standing.y_head_rot = 60.0;
    standing.y_body_rot = 0.0;
    for _ in 0..25 {
        brc.client_tick(&mut standing);
    }
    assert!((standing.y_body_rot - 60.0).abs() < 1e-3);

    // Helper sanity
    assert_eq!(rotate_towards(0.0, 90.0, 15.0), 15.0);
    assert_eq!(rotlerp(350.0, 10.0, 30.0), 10.0);
}

// ---------------------------------------------------------------------------
// TargetingConditions
// ---------------------------------------------------------------------------

#[test]
fn targeting_conditions_combat_non_combat_invisibility_and_negatives() {
    let targeter = TestEntity::new(1, Vec3::new(0.0, 0.0, 0.0));
    let mut target = TestEntity::new(2, Vec3::new(5.0, 0.0, 0.0));

    let combat = TargetingConditions::for_combat().range(10.0);
    assert!(combat.test(Difficulty::Normal, Some(&targeter), &target, None));

    // Self-target rejected
    assert!(!combat.test(Difficulty::Normal, Some(&targeter), &targeter, None));

    // Peaceful difficulty rejects combat targets (including null targeter)
    assert!(!combat.test(Difficulty::Peaceful, Some(&targeter), &target, None));
    assert!(!combat.test(Difficulty::Peaceful, None, &target, None));

    // Non-combat allows peaceful
    let non_combat = TargetingConditions::for_non_combat().range(10.0);
    assert!(non_combat.test(Difficulty::Peaceful, Some(&targeter), &target, None));

    // Invisibility shrinks effective range, clamped to minimum 2.0
    target.visibility = 0.05; // 10.0 * 0.05 = 0.5 -> clamped to 2.0
    assert!(!combat.test(Difficulty::Normal, Some(&targeter), &target, None));
    target.pos = Vec3::new(1.5, 0.0, 0.0);
    assert!(combat.test(Difficulty::Normal, Some(&targeter), &target, None));

    // ignore_invisibility_testing restores full range
    target.pos = Vec3::new(9.0, 0.0, 0.0);
    let ignore_invis = combat.copy().ignore_invisibility_testing();
    assert!(ignore_invis.test(Difficulty::Normal, Some(&targeter), &target, None));

    // Custom selector & line-of-sight
    let mut blocked_targeter = targeter.clone();
    blocked_targeter.los = false;
    assert!(!ignore_invis.test(Difficulty::Normal, Some(&blocked_targeter), &target, None));
    assert!(ignore_invis.ignore_line_of_sight().test(
        Difficulty::Normal,
        Some(&blocked_targeter),
        &target,
        None
    ));
    let reject_all = |_: &TestEntity| false;
    assert!(!non_combat.test(
        Difficulty::Normal,
        Some(&targeter),
        &target,
        Some(&reject_all),
    ));
}

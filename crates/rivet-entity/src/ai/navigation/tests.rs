//! Comprehensive unit tests for Paper 26.2 pathfinding, navigation,
//! movement/look/jump/body controls, and targeting conditions.

use std::collections::HashMap;

use rivet_util::random::{LegacyRandomSource, RandomSource};

use crate::ai::goal::context::{BlockPos, EntityId, GoalRandom, Vec3};
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
    sea_level: i32,
    /// `(minX, minZ, maxX, maxZ)` of the world border; unbounded when `None`.
    border: Option<(i32, i32, i32, i32)>,
    /// Every `block_info` read, in order.
    reads: std::cell::RefCell<Vec<BlockPos>>,
}

impl TestWorld {
    fn flat(floor_y: i32) -> Self {
        Self {
            blocks: HashMap::new(),
            default_floor_y: Some(floor_y),
            sea_level: 63,
            border: None,
            reads: std::cell::RefCell::default(),
        }
    }

    fn empty() -> Self {
        Self {
            blocks: HashMap::new(),
            default_floor_y: None,
            sea_level: 63,
            border: None,
            reads: std::cell::RefCell::default(),
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

    fn block_info(&self, pos: BlockPos) -> BlockPathInfo {
        self.reads.borrow_mut().push(pos);
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

    fn is_within_world_border(&self, pos: BlockPos) -> bool {
        self.border.is_none_or(|(min_x, min_z, max_x, max_z)| {
            pos.x >= min_x && pos.x < max_x && pos.z >= min_z && pos.z < max_z
        })
    }

    /// Block shapes are `[0, 1] x [0, collision_max_y] x [0, 1]` boxes. The
    /// fixture's world border filters targets only and has no collision shape.
    fn is_aabb_collision_free(&self, aabb: crate::ai::goal::context::Aabb) -> bool {
        use rivet_util::mth;

        let min_x = mth::floor_d(aabb.min_x + 1.0e-7);
        let max_x = mth::floor_d(aabb.max_x - 1.0e-7);
        let min_y = mth::floor_d(aabb.min_y + 1.0e-7);
        let max_y = mth::floor_d(aabb.max_y - 1.0e-7);
        let min_z = mth::floor_d(aabb.min_z + 1.0e-7);
        let max_z = mth::floor_d(aabb.max_z - 1.0e-7);
        // One cell below `min_y` so shapes taller than a block (fences) count.
        for x in min_x..=max_x {
            for y in (min_y - 1)..=max_y {
                for z in min_z..=max_z {
                    if let Some(height) = self.block_info(BlockPos::new(x, y, z)).collision_max_y {
                        let y = f64::from(y);
                        if aabb.min_y < y + height - 1.0e-7 && aabb.max_y > y + 1.0e-7 {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// Block shapes are `[0, 1] x [0, collision_max_y] x [0, 1]` boxes; a water or
    /// lava cell's fluid shape is a source's `[0, 1] x [0, 8/9] x [0, 1]` box.
    fn clip_hits_block(
        &self,
        pos: BlockPos,
        from: Vec3,
        to: Vec3,
        blocked_by_fluids: bool,
    ) -> bool {
        let info = self.block_info(pos);
        if info.is_air {
            return false;
        }
        let box_clips = |height: f64| voxel_box_clips(pos, height, from, to);
        info.collision_max_y.is_some_and(box_clips)
            || (blocked_by_fluids && (info.is_water || info.is_lava) && box_clips(8.0 / 9.0))
    }
}

/// `VoxelShape.clip(from, to, pos) != null` for the single box
/// `[0, 1] x [0, height] x [0, 1]` offset by `pos`.
fn voxel_box_clips(pos: BlockPos, height: f64, from: Vec3, to: Vec3) -> bool {
    use crate::projectile::phys::{Aabb as PhysAabb, Vec3 as PhysVec3};

    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, to.z - from.z);
    if dx * dx + dy * dy + dz * dz < 1.0e-7 {
        return false;
    }
    let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
    let shape = PhysAabb::new(x, y, z, x + 1.0, y + height, z + 1.0);
    let test_point = PhysVec3::new(
        from.x + dx * 0.001,
        from.y + dy * 0.001,
        from.z + dz * 0.001,
    );
    shape.contains(test_point)
        || shape
            .clip(
                PhysVec3::new(from.x, from.y, from.z),
                PhysVec3::new(to.x, to.y, to.z),
            )
            .is_some()
}

/// Mob `RandomSource` double backed by Java's LCG.
struct TestRandom(LegacyRandomSource);

impl TestRandom {
    fn new() -> Self {
        Self(LegacyRandomSource::new(0))
    }
}

impl GoalRandom for TestRandom {
    fn next_float(&mut self) -> f32 {
        self.0.next_float()
    }

    fn next_double(&mut self) -> f64 {
        self.0.next_double()
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        self.0.next_int_bound(bound)
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
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
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
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
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
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
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
    // A typical flyer follow range. Paper overwrites `walkedDistance` on every
    // neighbour visit (even rejected ones), which inflates it inside the dense
    // 26-neighbour fly search, so the budget must leave headroom.
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
            &[BlockPos::new(4, 1, 0)],
            32.0,
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
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
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
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
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
        &mut PathTypeCache::new(),
        &mut mob,
        &mut TestRandom::new(),
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
    // Slow enough that the per-node timeout (3 * dist / speed * 20 ticks = 1500)
    // cannot fire before the 100-tick stuck check.
    mob.speed = 0.04;

    let mut nav = PathNavigation::ground(16.0);
    let mut move_control = MoveControl::new();

    assert!(nav.move_to_xyz(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        4.5,
        1.0,
        0.5,
        1.0
    ));
    assert!(nav.is_in_progress());

    // One tick sets MoveControl wanted position
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut move_control,
        1,
    );
    assert!(move_control.has_wanted());

    // Paper checks `tick - lastStuckCheck > 100`: still in progress at tick 100.
    for t in 2..=100 {
        nav.tick(
            &mut mob,
            &world,
            &mut PathTypeCache::new(),
            &mut TestRandom::new(),
            &mut move_control,
            t,
        );
    }
    assert!(!nav.is_stuck());
    assert!(nav.is_in_progress());
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut move_control,
        101,
    );
    assert!(nav.is_stuck());
    assert!(nav.is_done());

    // The node timeout counts raw game ticks against `dist / speed * 20.0`.
    // Next node (1, 1, 0) is 1.0 from the mob, so the limit is 10 ticks and the
    // path times out once the timer exceeds 30, i.e. on game tick 32.
    let mut fast_mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    fast_mob.speed = 2.0;
    let mut fast_nav = PathNavigation::ground(16.0);
    let mut fast_mc = MoveControl::new();
    assert!(fast_nav.move_to_xyz(
        &mut fast_mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        4.5,
        1.0,
        0.5,
        1.0
    ));
    for t in 1..=31 {
        fast_nav.tick(
            &mut fast_mob,
            &world,
            &mut PathTypeCache::new(),
            &mut TestRandom::new(),
            &mut fast_mc,
            t,
        );
    }
    assert!(fast_nav.is_in_progress());
    fast_nav.tick(
        &mut fast_mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut fast_mc,
        32,
    );
    assert!(fast_nav.is_done());
    assert!(!fast_nav.is_stuck());
}

#[test]
fn ground_navigation_avoid_sun_trims_sky_exposed_nodes_on_move_to() {
    let mut world = TestWorld::flat(0);
    // Shaded start at (0, 1, 0) and (1, 1, 0), sun-exposed at (2, 1, 0) onward
    let mut shaded_air = BlockPathInfo::AIR;
    shaded_air.can_see_sky = false;
    world.set(0, 1, 0, shaded_air);
    world.set(1, 1, 0, shaded_air);

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut nav = PathNavigation::ground(16.0);
    nav.set_avoid_sun(true);

    // createPath does not trim; moveTo(Path) does.
    let path = nav
        .create_path(
            &mut mob,
            &world,
            &mut PathTypeCache::new(),
            &mut TestRandom::new(),
            BlockPos::new(4, 1, 0),
            0,
        )
        .expect("path created");
    assert_eq!(path.get_node_count(), 5);
    assert!(nav.move_to_path(Some(path), 1.0, &mob, &world));
    assert_eq!(nav.path.as_ref().map(Path::get_node_count), Some(2));

    // A mob already under open sky (at `y + 0.5`) does not trim.
    let open_world = TestWorld::flat(0);
    let mut nav = PathNavigation::ground(16.0);
    nav.set_avoid_sun(true);
    let path = nav.create_path(
        &mut mob,
        &open_world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        BlockPos::new(4, 1, 0),
        0,
    );
    assert!(nav.move_to_path(path, 1.0, &mob, &open_world));
    assert_eq!(nav.path.as_ref().map(Path::get_node_count), Some(5));
}

#[test]
fn move_to_path_lifts_cauldron_nodes() {
    let mut world = TestWorld::flat(0);
    let mut cauldron = BlockPathInfo::SOLID;
    cauldron.is_cauldron = true;
    world.set(1, 1, 0, cauldron);

    let nodes = vec![Node::new(0, 1, 0), Node::new(1, 1, 0), Node::new(2, 1, 0)];
    let path = Path::new(nodes, BlockPos::new(2, 1, 0), true);
    let mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut nav = PathNavigation::ground(16.0);
    assert!(nav.move_to_path(Some(path), 1.0, &mob, &world));

    let trimmed = nav.path.as_ref().unwrap();
    assert_eq!(trimmed.get_node_pos(0), BlockPos::new(0, 1, 0));
    assert_eq!(trimmed.get_node_pos(1), BlockPos::new(1, 2, 0));
    // The following node is lifted to the cauldron node's y + 1.
    assert_eq!(trimmed.get_node_pos(2), BlockPos::new(2, 2, 0));
}

#[test]
fn wall_climber_navigation_falls_back_to_direct_move_control() {
    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 1.4, 0.9);
    let mut nav = PathNavigation::wall_climber(16.0);
    let mut move_control = MoveControl::new();

    // Create path records path_to_position; stopping path leaves fallback active
    let _ = nav.create_path(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        BlockPos::new(5, 1, 0),
        0,
    );
    nav.stop();
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut move_control,
        1,
    );
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

    // With a targeter Paper defers to `canAttack` / `isAlliedTo` only; difficulty
    // and `canBeSeenAsEnemy` are not checked directly.
    assert!(combat.test(Difficulty::Peaceful, Some(&targeter), &target, None));
    let mut unseen_enemy = target.clone();
    unseen_enemy.can_be_seen_as_enemy = false;
    assert!(combat.test(Difficulty::Normal, Some(&targeter), &unseen_enemy, None));
    let mut cannot_attack = targeter.clone();
    cannot_attack.can_attack = false;
    assert!(!combat.test(Difficulty::Normal, Some(&cannot_attack), &target, None));
    let mut allied = targeter.clone();
    allied.allied = true;
    assert!(!combat.test(Difficulty::Normal, Some(&allied), &target, None));
    // Without a targeter, peaceful difficulty and `canBeSeenAsEnemy` reject combat targets.
    assert!(!combat.test(Difficulty::Peaceful, None, &target, None));
    assert!(!combat.test(Difficulty::Normal, None, &unseen_enemy, None));

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
#[test]
fn amphibious_evaluator_preserves_water_height_and_water_border() {
    use super::evaluator::NodeEvaluator;

    let mut world = TestWorld::flat(0);
    // 5-block deep water basin from y=1..=5 at x=0..=4, z=0
    for x in 0..=4 {
        for y in 1..=5 {
            world.set(x, y, 0, BlockPathInfo::WATER);
        }
    }
    // Solid wall adjacent to (2, 4, 0) at (2, 4, 1) so (2, 4, 0) is WaterBorder
    world.set(2, 4, 1, BlockPathInfo::SOLID);

    let mut mob = MobPathingState::new(Vec3::new(1.5, 4.0, 0.5), 0.6, 0.8);
    mob.in_water = true;

    let mut eval = AmphibiousNodeEvaluator::new(false);
    let mut arena = NodeArena::new();
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(1, 4, 0));
    eval.prepare(&mut mob, &world);

    let start_idx = arena.get_or_create(1, 4, 0);
    let mut out = [0usize; 32];
    let count = eval.get_neighbors(&mut ctx, &mob, &mut arena, start_idx, &mut out);

    // Neighbor at (2, 4, 0) must stay at y=4 (not sink to y=1) and have PathType::WaterBorder
    let border_neighbor = out[..count]
        .iter()
        .map(|&idx| arena.get(idx))
        .find(|n| n.x == 2 && n.z == 0);
    let border_node = border_neighbor.expect("amphibious neighbor at (2, *, 0) must exist");
    assert_eq!(border_node.y, 4);
    assert_eq!(border_node.path_type, PathType::WaterBorder);

    eval.done(&mut mob);
}

#[test]
fn wall_climber_fallback_checks_3d_distance_when_below_target() {
    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 1.4, 0.9);
    let mut nav = PathNavigation::wall_climber(16.0);
    let mut move_control = MoveControl::new();

    // Target is directly above mob (horiz_dist == 0 < bb_width, but dy == 5.0)
    let _ = nav.create_path(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        BlockPos::new(0, 6, 0),
        0,
    );
    nav.stop();

    // Tick while mob is directly below target: 3D distance >= bb_width, so fallback must NOT clear
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut move_control,
        1,
    );
    assert!(move_control.has_wanted());
    assert_eq!(move_control.wanted_y, 6.0);

    // Move mob above target with same X/Z: second disjunct (mob.pos.y > target.y && horiz < width) clears fallback
    mob.pos.y = 7.0;
    move_control = MoveControl::new();
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut move_control,
        2,
    );
    assert!(!move_control.has_wanted());
}

#[test]
fn amphibious_path_finder_swims_upward_underwater() {
    let mut world = TestWorld::flat(0);
    world.fill_water(-3, 1, -3, 3, 6, 3);

    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.8);
    mob.in_water = true;
    mob.on_ground = false;

    let mut eval = AmphibiousNodeEvaluator::new(false);
    let mut finder = PathFinder::new(64);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
            &[BlockPos::new(0, 4, 0)],
            16.0,
            0,
            1.0,
        )
        .expect("amphibious mob should find upward underwater path");
    assert!(path.can_reach());
    assert_eq!(path.get_end_node().map(|n| n.y), Some(4));
}

#[test]
fn fly_node_evaluator_checks_full_mob_bounding_box() {
    use super::NodeEvaluator;

    let mut world = TestWorld::empty();
    // Place a solid ceiling block at (1, 3, 0) that blocks a 2-tall flying mob at (1, 2, 0)
    world.set(1, 3, 0, BlockPathInfo::SOLID);
    // Place a solid side block at (4, 2, 1) that blocks a 2-wide flying mob at (3, 2, 0)
    world.set(4, 2, 1, BlockPathInfo::SOLID);

    let mut mob = MobPathingState::new(Vec3::new(0.5, 2.0, 0.5), 1.6, 1.8);
    let mut eval = FlyNodeEvaluator::new();
    eval.prepare(&mut mob, &world);
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 2, 0));

    // Origin cell (1, 2, 0) is air, but (1, 3, 0) is solid inside the mob's 2x2x2 volume
    assert_eq!(
        eval.get_path_type_of_mob(&mut ctx, &mob, 1, 2, 0),
        PathType::Blocked
    );
    // Origin cell (3, 2, 0) is air, but (4, 2, 1) is solid inside the mob's 2x2x2 volume
    assert_eq!(
        eval.get_path_type_of_mob(&mut ctx, &mob, 3, 2, 0),
        PathType::Blocked
    );
    // Clear cell at (8, 5, 8) is Open
    assert_eq!(
        eval.get_path_type_of_mob(&mut ctx, &mob, 8, 5, 8),
        PathType::Open
    );
}

#[test]
fn ground_navigation_does_not_prematurely_cut_corner_around_wall() {
    let mut world = TestWorld::flat(0);
    // Wall corner at (1, 1, 1)
    world.set(1, 1, 1, BlockPathInfo::SOLID);

    // Path goes (0,1,0) -> (1,1,0) -> (1,1,2) around a corner; mob is at (0.5, 1.0, 1.4),
    // which is closer to next node? No, mob at (0.2, 1.0, 1.2) is farther than 0.5 sqr from
    // current node (1.5, 1.0, 0.5) and closer to current than next (2.5, 1.0, 2.5),
    // yet previously had positive projection onto segment (1,1,0)->(2,1,2).
    let mut n0 = Node::new(1, 1, 0);
    n0.path_type = PathType::Walkable;
    let mut n1 = Node::new(1, 1, 2);
    n1.path_type = PathType::Walkable;
    let path = Path::new(vec![n0, n1], BlockPos::new(1, 1, 2), true);

    let mut mob = MobPathingState::new(Vec3::new(0.6, 1.0, 1.3), 0.6, 1.8);
    let mut nav = PathNavigation::ground(16.0);
    let mut mc = MoveControl::new();
    assert!(nav.move_to_path(Some(path), 1.0, &mob, &world));
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut mc,
        1,
    );

    // Mob has not passed (1,1,0) and cannot move directly on ground; must not skip node 0
    assert_eq!(nav.path.as_ref().map(Path::get_next_node_index), Some(0));
}

#[test]
fn walk_collision_cache_distinguishes_aabbs_with_same_min_corner() {
    use crate::ai::goal::Aabb;

    let mut world = TestWorld::flat(0);
    // Low ceiling block at (0, 3, 0)
    world.set(0, 3, 0, BlockPathInfo::SOLID);

    let mut eval = WalkNodeEvaluator::new();
    let short_box = Aabb::new(0.2, 1.0, 0.2, 0.8, 2.8, 0.8);
    let tall_box = Aabb::new(0.2, 1.0, 0.2, 0.8, 3.5, 0.8);

    assert!(!eval.has_collisions(&world, short_box));
    assert!(eval.has_collisions(&world, tall_box));
}

#[test]
fn wall_climber_fallback_keeps_climbing_toward_elevated_target() {
    let world = TestWorld::flat(0);
    // Spider at (0.5, 1.0, 0.5) directly underneath elevated target (0, 6, 0)
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 1.4, 0.9);
    let mut nav = PathNavigation::wall_climber(16.0);
    let mut mc = MoveControl::new();

    // Create path to elevated target (0, 6, 0) which sets WallClimber's path_to_position,
    // then clear the path so WallClimber's direct climbing fallback runs.
    let _ = nav.move_to_xyz(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        0.5,
        6.0,
        0.5,
        1.0,
    );
    nav.path = None;
    nav.speed_modifier = 1.0;
    nav.tick(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        &mut mc,
        1,
    );

    // Must still want to move toward (0, 6, 0) instead of clearing path_to_position
    assert!(mc.has_wanted());
    assert_eq!(mc.wanted_y, 6.0);
}

#[test]
fn aabb_collision_free_detects_protruding_fence_top_from_cell_below() {
    use crate::ai::goal::Aabb;

    let mut world = TestWorld::flat(0);
    // Fence at (0, 1, 0) protrudes to y = 2.5
    world.set(0, 1, 0, BlockPathInfo::FENCE);

    // Box starts at y = 2.1 (in cell y = 2, above fence's cell y = 1) and overlaps 2.1..2.5
    let overlapping = Aabb::new(0.2, 2.1, 0.2, 0.8, 3.0, 0.8);
    assert!(!world.is_aabb_collision_free(overlapping));

    // Box starting at y = 2.5 is clear of the fence top
    let clear = Aabb::new(0.2, 2.5, 0.2, 0.8, 3.5, 0.8);
    assert!(world.is_aabb_collision_free(clear));
}

#[test]
fn direct_movement_dda_detects_clipped_diagonal_block_corner() {
    let mut world = TestWorld::flat(0);
    // Solid pillar at (1, 1, 0)
    world.set(1, 1, 0, BlockPathInfo::SOLID);

    // Crosses x=1.0 at z=0.95 (entering (1, 1, 0)) before reaching (1, 1, 1)
    let from = Vec3::new(0.8, 1.5, 0.85);
    let to = Vec3::new(1.2, 1.5, 1.05);
    assert!(!world.is_clear_for_movement_between(from, to, false));
}

// ---------------------------------------------------------------------------
// Paper 26.2 ordering / semantics regressions
// ---------------------------------------------------------------------------

fn positions(arena: &NodeArena, out: &[usize]) -> Vec<(i32, i32, i32)> {
    out.iter()
        .map(|&idx| {
            let n = arena.get(idx);
            (n.x, n.y, n.z)
        })
        .collect()
}

#[test]
fn walk_neighbors_follow_horizontal_plane_order() {
    use super::NodeEvaluator;

    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut eval = WalkNodeEvaluator::new();
    eval.prepare(&mut mob, &world);
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
    let mut arena = NodeArena::new();
    let start = arena.get_or_create(0, 1, 0);
    let mut out = [0usize; 32];
    let count = eval.get_neighbors(&mut ctx, &mob, &mut arena, start, &mut out);

    // NORTH, EAST, SOUTH, WEST, then (N,E), (E,S), (S,W), (W,N) diagonals.
    assert_eq!(
        positions(&arena, &out[..count]),
        vec![
            (0, 1, -1),
            (1, 1, 0),
            (0, 1, 1),
            (-1, 1, 0),
            (1, 1, -1),
            (1, 1, 1),
            (-1, 1, 1),
            (-1, 1, -1),
        ]
    );
}

#[test]
fn walk_diagonals_rejected_for_wide_mob_past_positive_malus() {
    use super::NodeEvaluator;

    let world = TestWorld::flat(0);
    let neighbor_count = |bb_width: f32| {
        let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), bb_width, 1.8);
        mob.set_pathfinding_malus(PathType::Walkable, 1.0);
        let mut eval = WalkNodeEvaluator::new();
        eval.prepare(&mut mob, &world);
        let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
        let mut arena = NodeArena::new();
        let start = arena.get_or_create(0, 1, 0);
        let mut out = [0usize; 32];
        eval.get_neighbors(&mut ctx, &mob, &mut arena, start, &mut out)
    };
    assert_eq!(neighbor_count(0.6), 8);
    // `getBbWidth() > 1.0F` with positive-malus cardinals: cardinals only.
    assert_eq!(neighbor_count(1.4), 4);
}

#[test]
fn walk_ground_search_blocks_at_fall_limit_and_original_y_at_world_bottom() {
    use super::NodeEvaluator;

    let mut world = TestWorld::empty();
    world.set(0, 0, 0, BlockPathInfo::SOLID);
    let run = |max_fall_distance: i32| {
        let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
        mob.max_fall_distance = max_fall_distance;
        let mut eval = WalkNodeEvaluator::new();
        eval.prepare(&mut mob, &world);
        let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
        let mut arena = NodeArena::new();
        let start = arena.get_or_create(0, 1, 0);
        let mut out = [0usize; 32];
        let count = eval.get_neighbors(&mut ctx, &mob, &mut arena, start, &mut out);
        let blocked: Vec<(i32, i32, i32)> = arena
            .iter()
            .filter(|n| n.x == 1 && n.z == 0 && n.path_type == PathType::Blocked)
            .map(|n| (n.x, n.y, n.z))
            .collect();
        (count, blocked)
    };

    // Falling more than 3 blocks: blocked node at `y - 4`.
    assert_eq!(run(3), (0, vec![(1, -3, 0)]));
    // No ground down to the world bottom: blocked node at the original y.
    assert_eq!(run(1000), (0, vec![(1, 1, 0)]));
}

#[test]
fn fly_neighbors_match_paper_order_and_visit_every_candidate() {
    use super::NodeEvaluator;

    let mut world = TestWorld::flat(0);
    // Block SOUTH so every south-dependent candidate is guarded out.
    world.set(0, 1, 1, BlockPathInfo::SOLID);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.6);
    mob.on_ground = false;
    let mut eval = FlyNodeEvaluator::new();
    eval.prepare(&mut mob, &world);
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
    let mut arena = NodeArena::new();
    let start = arena.get_or_create(0, 1, 0);
    let mut out = [0usize; 32];
    let count = eval.get_neighbors(&mut ctx, &mob, &mut arena, start, &mut out);

    assert_eq!(
        positions(&arena, &out[..count]),
        vec![
            (-1, 1, 0),  // west
            (1, 1, 0),   // east
            (0, 1, -1),  // north
            (0, 2, 0),   // up
            (-1, 2, 0),  // westUp
            (1, 2, 0),   // eastUp
            (0, 2, -1),  // northUp
            (1, 1, -1),  // northEast
            (-1, 1, -1), // northWest
            (1, 2, -1),  // northEastUp
            (-1, 2, -1), // northWestUp
        ]
    );
    // southEast is guarded out (south has no malus) but Paper still calls
    // findAcceptedNode for it, which bumps WALKABLE costMalus.
    let south_east = arena
        .iter()
        .find(|n| (n.x, n.y, n.z) == (1, 1, 1))
        .expect("southEast visited");
    assert_eq!(south_east.path_type, PathType::Walkable);
    assert_eq!(south_east.cost_malus, 1.0);
}

#[test]
fn fly_fence_below_mob_position_stays_open() {
    use super::NodeEvaluator;

    let mut world = TestWorld::empty();
    world.set(0, 1, 0, BlockPathInfo::FENCE);
    world.set(5, 1, 0, BlockPathInfo::FENCE);
    let mut eval = FlyNodeEvaluator::new();
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
    assert_eq!(eval.get_path_type(&mut ctx, 0, 2, 0), PathType::Open);
    assert_eq!(eval.get_path_type(&mut ctx, 5, 2, 0), PathType::Fence);
}

#[test]
fn fly_start_falls_back_to_random_candidates_for_small_mobs() {
    use super::NodeEvaluator;

    let mut world = TestWorld::flat(0);
    world.set(0, 1, 0, BlockPathInfo::SOLID);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.6);
    mob.on_ground = false;
    let mut eval = FlyNodeEvaluator::new();
    eval.prepare(&mut mob, &world);
    let mut ctx = PathfindingContext::new(&world, None, mob.block_position());
    let mut arena = NodeArena::new();
    let start = eval.get_start(&mut ctx, &mob, &mut arena, &mut TestRandom::new());

    // Replay `BlockPos.randomBetweenClosed` over the inflated box
    // x,z in -1..=1 and y in 0..=2 (x, then y, then z per candidate).
    let mut replay = LegacyRandomSource::new(0);
    let expected = (0..10)
        .map(|_| {
            let x = -1 + replay.next_int_bound(3);
            let y = replay.next_int_bound(3);
            let z = -1 + replay.next_int_bound(3);
            (x, y, z)
        })
        .find(|&(x, y, z)| y >= 1 && (x, y, z) != (0, 1, 0))
        .expect("a free candidate within 10 draws");
    let node = arena.get(start);
    assert_eq!((node.x, node.y, node.z), expected);
    assert_ne!((node.x, node.y, node.z), (0, 1, 0));
}

#[test]
fn amphibious_path_type_scans_full_mob_volume_in_water() {
    use super::NodeEvaluator;

    let mut world = TestWorld::flat(0);
    world.set(0, 1, 0, BlockPathInfo::WATER);
    world.set(1, 1, 0, BlockPathInfo::SOLID);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 1.6, 0.8);
    mob.in_water = true;
    let mut eval = AmphibiousNodeEvaluator::new(false);
    eval.prepare(&mut mob, &world);
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 1, 0));
    // Anchor cell is WATER_BORDER, but the 2-wide volume overlaps the solid block.
    assert_eq!(
        eval.get_path_type_of_mob(&mut ctx, &mob, 0, 1, 0),
        PathType::Blocked
    );
    eval.done(&mut mob);
}

#[test]
fn path_finder_candidate_selection_uses_paper_comparators() {
    let world = TestWorld::flat(0);

    // Unreached: all targets compete on node count only (ties keep the first),
    // even when a later target is much closer.
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut eval = WalkNodeEvaluator::new();
    let mut finder = PathFinder::new(2);
    let far = BlockPos::new(10, 1, 0);
    let near = BlockPos::new(-3, 1, 0);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
            &[far, near],
            16.0,
            0,
            1.0,
        )
        .unwrap();
    assert!(!path.can_reach());
    assert_eq!(path.get_node_count(), 2);
    assert_eq!(path.get_target(), far);

    // Reached: reached targets compete on distance to target first.
    let a = BlockPos::new(0, 1, 4);
    let b = BlockPos::new(3, 1, 0);
    let mut finder = PathFinder::new(200);
    let path = finder
        .find_path(
            &mut eval,
            &world,
            &mut PathTypeCache::new(),
            &mut mob,
            &mut TestRandom::new(),
            &[a, b],
            16.0,
            5,
            1.0,
        )
        .unwrap();
    assert!(path.can_reach());
    assert_eq!(path.get_target(), b);
}

#[test]
fn ground_create_path_surface_search_scans_up_and_rejects_unloaded_targets() {
    let world = TestWorld::empty();
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut nav = PathNavigation::ground(16.0);
    // No ground below an air target: Paper scans upward from y + 1 to max_y + 1.
    let _ = nav.create_path(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        BlockPos::new(3, 1, 0),
        0,
    );
    assert_eq!(
        nav.target_pos(),
        Some(BlockPos::new(3, world.max_y() + 1, 0))
    );

    let mut unloaded = TestWorld::flat(0);
    unloaded.set(3, 1, 0, BlockPathInfo::UNLOADED);
    let mut nav = PathNavigation::ground(16.0);
    assert!(
        nav.create_path(
            &mut mob,
            &unloaded,
            &mut PathTypeCache::new(),
            &mut TestRandom::new(),
            BlockPos::new(3, 1, 0),
            0
        )
        .is_none()
    );
}

#[test]
fn move_control_jump_rules_match_paper() {
    let world = TestWorld::flat(0);
    let jumps = |world: &TestWorld, width: f32, wanted: (f64, f64, f64)| {
        let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), width, 1.8);
        let mut mc = MoveControl::new();
        let mut jc = JumpControl::new();
        mc.set_wanted_position(wanted.0, wanted.1, wanted.2, 1.0);
        mc.tick(&mut mob, world, &mut jc, |_, _, _| PathType::Walkable);
        jc.is_jump_queued()
    };
    // `xd*xd + zd*zd < max(1.0F, bbWidth)` is not squared: 1.44 vs 1.4.
    assert!(!jumps(&world, 1.4, (1.7, 2.0, 0.5)));
    assert!(jumps(&world, 1.4, (1.6, 2.0, 0.5)));

    // Only DOORS and FENCES are exempt from the in-block jump; walls are not.
    let mut walled = TestWorld::flat(0);
    walled.set(
        0,
        1,
        0,
        BlockPathInfo {
            is_fence: false,
            is_wall: true,
            ..BlockPathInfo::FENCE
        },
    );
    assert!(jumps(&walled, 0.6, (0.6, 1.0, 0.5)));
    let mut fenced = TestWorld::flat(0);
    fenced.set(0, 1, 0, BlockPathInfo::FENCE);
    assert!(!jumps(&fenced, 0.6, (0.6, 1.0, 0.5)));

    // JUMPING only ends in liquid when the mob is affected by fluids.
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut mc = MoveControl::new();
    let mut jc = JumpControl::new();
    mc.operation = MoveOperation::Jumping;
    mob.on_ground = false;
    mob.in_water = true;
    mob.affected_by_fluids = false;
    mc.tick(&mut mob, &world, &mut jc, |_, _, _| PathType::Walkable);
    assert_eq!(mc.operation, MoveOperation::Jumping);
    mob.affected_by_fluids = true;
    mc.tick(&mut mob, &world, &mut jc, |_, _, _| PathType::Walkable);
    assert_eq!(mc.operation, MoveOperation::Wait);
}

#[test]
fn body_rotation_waits_ten_stable_ticks_before_facing_front() {
    let mut brc = BodyRotationControl::new();
    let mut mob = MobPathingState::new(Vec3::ZERO, 0.6, 1.8);
    mob.max_head_y_rot = 5.0;
    mob.y_head_rot = 10.0;
    mob.y_body_rot = 0.0;
    for _ in 0..10 {
        brc.client_tick(&mut mob);
    }
    assert_eq!(mob.y_body_rot, 0.0);
    // headStableTime 11: fraction 0.1, remaining angle 4.5.
    brc.client_tick(&mut mob);
    assert!((mob.y_body_rot - 5.5).abs() < 1e-5);
}

#[test]
fn can_move_directly_clips_to_mob_mid_height_above_stop() {
    let mut world = TestWorld::empty();
    // Ceiling cell above the flyer's next node.
    world.set(0, 13, 0, BlockPathInfo::SOLID);
    let mob = MobPathingState::new(Vec3::new(0.5, 10.0, 0.5), 0.6, 2.6);
    let nav = PathNavigation::flying(16.0);
    let start = Vec3::new(0.5, 10.0, 0.5);
    let stop = Vec3::new(0.5, 12.0, 0.5);
    // A foot-level ray (10.0 -> 12.0) never reaches y = 13; Paper's ray ends at
    // `stop.y + bbHeight * 0.5 = 13.3` and hits the ceiling.
    assert!(world.is_clear_for_movement_between(start, stop, true));
    assert!(!nav.can_move_directly(&mob, &world, start, stop));

    // Going down, the offset keeps the ray above a block a foot-level ray would hit.
    let mut floor = TestWorld::empty();
    floor.set(0, 8, 0, BlockPathInfo::SOLID);
    let tall = MobPathingState::new(Vec3::new(0.5, 10.0, 0.5), 0.6, 2.0);
    let swim = PathNavigation::water_bound(16.0, false);
    let down = Vec3::new(0.5, 8.0, 0.5);
    assert!(!floor.is_clear_for_movement_between(start, down, false));
    assert!(swim.can_move_directly(&tall, &floor, start, down));
}

#[test]
fn clear_for_movement_clips_block_and_fluid_shapes_like_level_clip() {
    let mut world = TestWorld::empty();
    // `from == to` is an immediate miss, even inside a solid block.
    world.set(0, 5, 0, BlockPathInfo::SOLID);
    let inside = Vec3::new(0.5, 5.5, 0.5);
    assert!(world.is_clear_for_movement_between(inside, inside, true));

    // A ray above a bottom slab's collision box misses it; one through it hits.
    let slab = BlockPathInfo {
        collision_max_y: Some(0.5),
        ..BlockPathInfo::SOLID
    };
    world.set(2, 5, 0, slab);
    assert!(world.is_clear_for_movement_between(
        Vec3::new(1.5, 5.75, 0.5),
        Vec3::new(3.5, 5.75, 0.5),
        false
    ));
    assert!(!world.is_clear_for_movement_between(
        Vec3::new(1.5, 5.25, 0.5),
        Vec3::new(3.5, 5.25, 0.5),
        false
    ));

    // Fluids only block with `Fluid.ANY`, and only through the fluid shape
    // (a source is 8/9 of a block tall).
    world.set(2, 7, 0, BlockPathInfo::WATER);
    let low = (Vec3::new(1.5, 7.5, 0.5), Vec3::new(3.5, 7.5, 0.5));
    let high = (Vec3::new(1.5, 7.95, 0.5), Vec3::new(3.5, 7.95, 0.5));
    assert!(world.is_clear_for_movement_between(low.0, low.1, false));
    assert!(!world.is_clear_for_movement_between(low.0, low.1, true));
    assert!(world.is_clear_for_movement_between(high.0, high.1, true));
}

#[test]
fn max_path_length_combines_follow_range_and_required_path_length() {
    // The constructor seeds the node budget from the base follow range only.
    let mut nav = PathNavigation::ground(10.0);
    assert_eq!(nav.max_visited_nodes(), 160);
    // `max((float) FOLLOW_RANGE, requiredPathLength = 16)`.
    assert_eq!(nav.max_path_length(), 16.0);

    nav.on_follow_range_updated(10.0);
    assert_eq!(nav.max_visited_nodes(), 256);

    nav.set_required_path_length(48.0);
    assert_eq!(nav.max_path_length(), 48.0);
    assert_eq!(nav.max_visited_nodes(), 768);

    nav.on_follow_range_updated(64.0);
    assert_eq!(nav.max_path_length(), 64.0);
    assert_eq!(nav.max_visited_nodes(), 1024);
}

#[test]
fn create_path_drops_targets_outside_world_border() {
    let mut world = TestWorld::flat(0);
    world.border = Some((-8, -8, 4, 8));
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut nav = PathNavigation::ground(16.0);
    let outside = BlockPos::new(4, 1, 0);
    let inside = BlockPos::new(3, 1, 0);

    assert!(
        nav.create_path_targets(
            &mut mob,
            &world,
            &mut PathTypeCache::new(),
            &mut TestRandom::new(),
            &[outside],
            0,
            16.0
        )
        .is_none()
    );
    let path = nav
        .create_path_targets(
            &mut mob,
            &world,
            &mut PathTypeCache::new(),
            &mut TestRandom::new(),
            &[outside, inside],
            0,
            16.0,
        )
        .expect("path to the in-border target");
    assert_eq!(path.get_target(), inside);
}

#[test]
fn should_recompute_path_uses_remaining_nodes_around_midpoint() {
    let world = TestWorld::flat(0);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
    let mut nav = PathNavigation::ground(16.0);
    assert!(!nav.should_recompute_path(&mob, BlockPos::new(2, 1, 0)));

    let path = nav.create_path(
        &mut mob,
        &world,
        &mut PathTypeCache::new(),
        &mut TestRandom::new(),
        BlockPos::new(4, 1, 0),
        0,
    );
    assert!(nav.move_to_path(path, 1.0, &mob, &world));
    // End node (4, 1, 0), mob (0.5, 1.0, 0.5): midpoint (2.25, 1.0, 0.25),
    // radius = 5 remaining nodes.
    assert!(nav.should_recompute_path(&mob, BlockPos::new(2, 1, 0)));
    // (6, 1, 3) centre is (6.5, 1.5, 3.5): 4.25^2 + 0.5^2 + 3.25^2 = 28.875 > 25.
    assert!(!nav.should_recompute_path(&mob, BlockPos::new(6, 1, 3)));
    // (5, 1, 3) centre is (5.5, 1.5, 3.5): 3.25^2 + 0.5^2 + 3.25^2 = 21.375 < 25.
    assert!(nav.should_recompute_path(&mob, BlockPos::new(5, 1, 3)));

    nav.has_delayed_recomputation = true;
    assert!(!nav.should_recompute_path(&mob, BlockPos::new(2, 1, 0)));
}

#[test]
fn stable_destination_uses_solid_render_and_entity_can_stand_on() {
    let mut world = TestWorld::empty();
    // Glass-like: full collision, not solid-render, full top face.
    let glass = BlockPathInfo {
        is_solid_render: false,
        ..BlockPathInfo::SOLID
    };
    // Bottom slab: has collision, neither solid-render nor a full top face.
    let slab = BlockPathInfo {
        is_solid_render: false,
        entity_can_stand_on: false,
        collision_max_y: Some(0.5),
        ..BlockPathInfo::SOLID
    };
    world.set(0, 4, 0, glass);
    world.set(1, 4, 0, slab);
    world.set(2, 4, 0, BlockPathInfo::SOLID);

    let ground = PathNavigation::ground(16.0);
    assert!(!ground.is_stable_destination(&world, BlockPos::new(0, 5, 0)));
    assert!(!ground.is_stable_destination(&world, BlockPos::new(1, 5, 0)));
    assert!(ground.is_stable_destination(&world, BlockPos::new(2, 5, 0)));

    let water = PathNavigation::water_bound(16.0, false);
    assert!(water.is_stable_destination(&world, BlockPos::new(0, 4, 0)));
    assert!(water.is_stable_destination(&world, BlockPos::new(1, 4, 0)));
    assert!(!water.is_stable_destination(&world, BlockPos::new(2, 4, 0)));

    let flying = PathNavigation::flying(16.0);
    assert!(flying.is_stable_destination(&world, BlockPos::new(0, 4, 0)));
    assert!(!flying.is_stable_destination(&world, BlockPos::new(1, 4, 0)));
}

#[test]
fn controls_store_x_rot_through_entity_set_x_rot() {
    // FlyingMoveControl: rotlerp(0, xRotD < 0, 20) = -20 + 360 = 340, which
    // `setXRot` stores as clamp(340 % 360, -90, 90) = 90.
    let mut flyer = MobPathingState::new(Vec3::new(0.0, 5.0, 0.0), 0.6, 0.6);
    flyer.on_ground = false;
    let mut fmc = FlyingMoveControl::new(20, false);
    fmc.set_wanted_position(5.0, 8.0, 0.0, 1.0);
    fmc.tick(&mut flyer);
    assert_eq!(flyer.x_rot, 90.0);

    // SmoothSwimmingLookControl aims at xRotD + 10 = 100 for a target directly
    // below; `setXRot` clamps it to 90.
    let mut swimmer = MobPathingState::new(Vec3::ZERO, 0.9, 0.6);
    let mut slc = SmoothSwimmingLookControl::new(10);
    slc.base
        .set_look_at(0.0, swimmer.eye_y() - 5.0, 0.0, 10.0, 180.0);
    slc.tick(&mut swimmer, false);
    assert_eq!(swimmer.x_rot, 90.0);

    // Non-finite values are discarded.
    swimmer.set_x_rot(f32::NAN);
    swimmer.set_y_rot(f32::INFINITY);
    assert_eq!(swimmer.x_rot, 90.0);
    assert_eq!(swimmer.y_rot, 0.0);
}

#[test]
fn unloaded_region_cells_read_as_empty_chunk_air_outside_path_type() {
    use crate::ai::goal::Aabb;

    let mut world = TestWorld::flat(0);
    world.set(1, 1, 0, BlockPathInfo::UNLOADED);
    // `EmptyLevelChunk` adds no collision.
    assert!(world.is_aabb_collision_free(Aabb::new(1.2, 1.0, 0.2, 1.8, 1.9, 0.8)));

    // SwimNodeEvaluator: an unloaded (air) cell above water is BREACH.
    use crate::ai::navigation::NodeEvaluator;
    let mut pool = TestWorld::flat(0);
    pool.fill_water(-2, 1, -2, 2, 3, 2);
    pool.set(0, 4, 0, BlockPathInfo::UNLOADED);
    let mut swimmer = MobPathingState::new(Vec3::new(0.5, 2.0, 0.5), 0.6, 0.6);
    swimmer.in_water = true;
    let mut eval = SwimNodeEvaluator::new(true);
    eval.prepare(&mut swimmer, &pool);
    let mut ctx = PathfindingContext::new(&pool, None, BlockPos::new(0, 2, 0));
    assert_eq!(
        eval.get_path_type_of_mob(&mut ctx, &swimmer, 0, 4, 0),
        PathType::Breach
    );
}

#[test]
fn level_path_type_cache_keeps_unloaded_blocked_across_searches() {
    // A wall of cells missing from the first mob's region.
    let mut partial = TestWorld::flat(0);
    for z in -30..=30 {
        for y in 1..=3 {
            partial.set(2, y, z, BlockPathInfo::UNLOADED);
        }
    }
    let loaded = TestWorld::flat(0);
    let target = BlockPos::new(4, 1, 0);
    let search = |world: &TestWorld, cache: &mut PathTypeCache| {
        let mut mob = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
        PathFinder::new(400)
            .find_path(
                &mut WalkNodeEvaluator::new(),
                world,
                cache,
                &mut mob,
                &mut TestRandom::new(),
                &[target],
                8.0,
                0,
                1.0,
            )
            .is_some_and(|path| path.can_reach())
    };

    let mut level_cache = PathTypeCache::new();
    assert!(!search(&partial, &mut level_cache));
    // `ServerLevel.getPathTypeCache()` still holds the BLOCKED entries after the
    // chunk loads, so a later search sharing it stays blocked.
    assert!(!search(&loaded, &mut level_cache));
    assert!(search(&loaded, &mut PathTypeCache::new()));

    // `sendBlockUpdated` invalidates per position.
    for z in -30..=30 {
        for y in 0..=3 {
            level_cache.invalidate(BlockPos::new(2, y, z));
        }
    }
    assert!(search(&loaded, &mut level_cache));
}

#[test]
fn swim_get_path_type_scans_the_mob_box() {
    use crate::ai::navigation::NodeEvaluator;

    let mut pool = TestWorld::flat(0);
    pool.fill_water(-3, 1, -3, 3, 3, 3);
    pool.set(1, 2, 0, BlockPathInfo::SOLID);
    let mut swimmer = MobPathingState::new(Vec3::new(0.5, 2.0, 0.5), 0.6, 0.6);
    let mut eval = SwimNodeEvaluator::new(true);
    eval.prepare(&mut swimmer, &pool);
    let mut ctx = PathfindingContext::new(&pool, None, BlockPos::new(0, 2, 0));
    // Air above water-pathfindable water is BREACH.
    assert_eq!(eval.get_path_type(&mut ctx, 0, 4, 0), PathType::Breach);
    assert_eq!(eval.get_path_type(&mut ctx, 0, 2, 0), PathType::Water);

    // A two-wide swimmer also reads the neighbouring column.
    let mut wide = MobPathingState::new(Vec3::new(0.5, 2.0, 0.5), 1.2, 0.6);
    let mut wide_eval = SwimNodeEvaluator::new(true);
    wide_eval.prepare(&mut wide, &pool);
    let mut ctx = PathfindingContext::new(&pool, None, BlockPos::new(0, 2, 0));
    assert_eq!(
        wide_eval.get_path_type(&mut ctx, 0, 2, 0),
        PathType::Blocked
    );
    assert_eq!(wide_eval.get_path_type(&mut ctx, -2, 2, 0), PathType::Water);
}

thread_local! {
    static PATHFINDING_HOOK_LOG: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// `Sniffer.onPathfindingStart()` with `isInWater()` standing in for the
/// fire-or-water check.
fn sniffer_like_start(mob: &mut MobPathingState) {
    PATHFINDING_HOOK_LOG.with(|log| log.borrow_mut().push("start"));
    if mob.in_water {
        mob.set_pathfinding_malus(PathType::Water, 0.0);
    }
}

/// `Sniffer.onPathfindingDone()`.
fn sniffer_like_done(mob: &mut MobPathingState) {
    PATHFINDING_HOOK_LOG.with(|log| log.borrow_mut().push("done"));
    mob.set_pathfinding_malus(PathType::Water, -1.0);
}

#[test]
fn evaluators_run_mob_pathfinding_hooks_at_paper_points() {
    let take_log = || PATHFINDING_HOOK_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()));
    let mut world = TestWorld::flat(0);
    for z in -30..=30 {
        world.set(2, 1, z, BlockPathInfo::WATER);
        world.set(2, 2, z, BlockPathInfo::WATER);
    }
    let target = BlockPos::new(4, 1, 0);
    let search = |mob: &mut MobPathingState| {
        PathFinder::new(400)
            .find_path(
                &mut WalkNodeEvaluator::new(),
                &world,
                &mut PathTypeCache::new(),
                mob,
                &mut TestRandom::new(),
                &[target],
                8.0,
                0,
                1.0,
            )
            .is_some_and(|path| path.can_reach())
    };

    let mut sniffer = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 1.9, 1.75);
    sniffer.on_pathfinding_start = sniffer_like_start;
    sniffer.on_pathfinding_done = sniffer_like_done;
    sniffer.set_pathfinding_malus(PathType::Water, -1.0);
    assert!(!search(&mut sniffer));
    assert_eq!(take_log(), ["start", "done"]);

    sniffer.in_water = true;
    assert!(search(&mut sniffer));
    assert_eq!(take_log(), ["start", "done"]);
    assert_eq!(sniffer.get_pathfinding_malus(PathType::Water), -1.0);

    // FlyNodeEvaluator calls each hook once more around `super`.
    let mut flyer = MobPathingState::new(Vec3::new(0.5, 1.0, 0.5), 0.6, 0.6);
    flyer.on_pathfinding_start = sniffer_like_start;
    flyer.on_pathfinding_done = sniffer_like_done;
    let _ = PathFinder::new(400).find_path(
        &mut FlyNodeEvaluator::new(),
        &world,
        &mut PathTypeCache::new(),
        &mut flyer,
        &mut TestRandom::new(),
        &[target],
        8.0,
        0,
        1.0,
    );
    assert_eq!(take_log(), ["start", "start", "done", "done"]);
}

#[test]
fn get_target_registers_cell_and_takes_hash_colliding_node() {
    use super::NodeEvaluator;

    // Java `getTargetNodeAt` = `new Target(getNode(floor x, y, z))`; `Node.createHash`
    // keeps only `y & 0xFF`, so (5, 10, 5) and (5, 266, 5) share the key 266 - 10 = 256.
    let mut arena = NodeArena::new();
    let start = arena.get_or_create(5, 10, 5);
    let walk = WalkNodeEvaluator::new();
    let t = walk.get_target(&mut arena, 5.0, 266.0, 5.0);
    assert_eq!((t.x, t.y, t.z), (5, 10, 5));
    assert_eq!(arena.len(), 1);
    assert_eq!(start, 0);

    // Amphibious uses `getTargetNodeAt(x, y + 0.5, z)`: floor(265.5 + 0.5) = 266, same collision.
    let amph = AmphibiousNodeEvaluator::new(true);
    let t = amph.get_target(&mut arena, 5.0, 265.5, 5.0);
    assert_eq!((t.x, t.y, t.z), (5, 10, 5));

    // A fresh cell is registered in the arena (Java's computeIfAbsent side effect).
    let t = walk.get_target(&mut arena, 7.0, 20.0, 7.0);
    assert_eq!((t.x, t.y, t.z), (7, 20, 7));
    assert_eq!(arena.len(), 2);
}

#[test]
fn walk_find_accepted_node_rejects_by_floor_level_before_scanning_path_type() {
    use super::NodeEvaluator;

    // Start (0,2,0) over air: node height = (2-1) + 0 = 1.0. Candidate (1,2,0) sits on a
    // fence (collision_max_y 1.5): maxYTarget = 1 + 1.5 = 2.5; 2.5 - 1.0 = 1.5 > jump
    // height 1.0, so Java `findAcceptedNode` returns null before `getCachedPathType`
    // reads any cell of the candidate's mob box (e.g. (1,2,0)).
    let mut world = TestWorld::empty();
    world.set(1, 1, 0, BlockPathInfo::FENCE);
    let mut mob = MobPathingState::new(Vec3::new(0.5, 2.0, 0.5), 0.6, 1.8);
    let mut eval = WalkNodeEvaluator::new();
    eval.prepare(&mut mob, &world);
    let mut ctx = PathfindingContext::new(&world, None, BlockPos::new(0, 2, 0));
    let mut arena = NodeArena::new();
    let start = arena.get_or_create(0, 2, 0);
    world.reads.borrow_mut().clear();
    let mut out = [0usize; 32];
    eval.get_neighbors(&mut ctx, &mob, &mut arena, start, &mut out);
    let reads = world.reads.borrow();
    assert!(
        !reads.contains(&BlockPos::new(1, 2, 0)),
        "rejected candidate was scanned: {reads:?}"
    );
}

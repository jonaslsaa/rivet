//! Port of Paper 26.2 `NodeEvaluator` and its four concrete evaluators:
//! - `WalkNodeEvaluator`
//! - `FlyNodeEvaluator`
//! - `SwimNodeEvaluator`
//! - `AmphibiousNodeEvaluator`

use std::collections::HashMap;

use rivet_util::mth;

use crate::ai::goal::context::{Aabb, BlockPos};
use crate::ai::navigation::node::{NodeArena, Target};
use crate::ai::navigation::path_type::{Direction, PathType, block_pos_as_long};
use crate::ai::navigation::world_view::{MobPathingState, PathWorldView, PathfindingContext};

/// Shared configuration and prepared dimensions from `NodeEvaluator`.
#[derive(Debug, Clone, Copy)]
pub struct EvaluatorConfig {
    pub can_pass_doors: bool,
    pub can_open_doors: bool,
    pub can_float: bool,
    pub can_walk_over_fences: bool,
    pub entity_width: i32,
    pub entity_height: i32,
    pub entity_depth: i32,
}

impl Default for EvaluatorConfig {
    fn default() -> Self {
        Self {
            can_pass_doors: true,
            can_open_doors: false,
            can_float: false,
            can_walk_over_fences: false,
            entity_width: 1,
            entity_height: 1,
            entity_depth: 1,
        }
    }
}

impl EvaluatorConfig {
    /// Mirrors `NodeEvaluator.prepare(PathNavigationRegion, Mob)` dimension math.
    pub fn prepare_dimensions(&mut self, mob: &MobPathingState) {
        self.entity_width = mth::floor(mob.bb_width + 1.0);
        self.entity_height = mth::floor(mob.bb_height + 1.0);
        self.entity_depth = mth::floor(mob.bb_width + 1.0);
    }
}

/// Trait mirroring `net.minecraft.world.level.pathfinder.NodeEvaluator`.
pub trait NodeEvaluator {
    fn config(&self) -> &EvaluatorConfig;
    fn config_mut(&mut self) -> &mut EvaluatorConfig;

    fn prepare<W: PathWorldView + ?Sized>(&mut self, mob: &mut MobPathingState, _world: &W) {
        self.config_mut().prepare_dimensions(mob);
    }

    fn done(&mut self, _mob: &mut MobPathingState) {}

    fn get_start<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
    ) -> usize;

    fn get_target(&self, x: f64, y: f64, z: f64) -> Target {
        Target::new(mth::floor_d(x), mth::floor_d(y), mth::floor_d(z))
    }

    fn get_neighbors<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        node_idx: usize,
        out: &mut [usize; 32],
    ) -> usize;

    fn get_path_type_of_mob<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType;

    fn get_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType;
}

/// Exact port of `WalkNodeEvaluator.getPathTypeStatic(PathfindingContext, MutableBlockPos)`.
pub fn get_path_type_static<W: PathWorldView + ?Sized>(
    ctx: &mut PathfindingContext<'_, W>,
    x: i32,
    y: i32,
    z: i32,
) -> PathType {
    let block_path_type = ctx.get_path_type_from_state(x, y, z);
    if block_path_type == PathType::Open && y > ctx.level().min_y() {
        match ctx.get_path_type_from_state(x, y - 1, z) {
            PathType::Open | PathType::Water | PathType::Lava | PathType::Walkable => {
                PathType::Open
            }
            PathType::DamageFire => PathType::DamageFire,
            PathType::DamageOther => PathType::DamageOther,
            PathType::StickyHoney => PathType::StickyHoney,
            PathType::PowderSnow => PathType::DangerPowderSnow,
            PathType::DamageCautious => PathType::DamageCautious,
            PathType::Trapdoor => PathType::DangerTrapdoor,
            _ => check_neighbour_blocks(ctx, x, y, z, PathType::Walkable),
        }
    } else {
        block_path_type
    }
}

/// Exact port of `WalkNodeEvaluator.checkNeighbourBlocks(PathfindingContext, int, int, int, PathType)`.
pub fn check_neighbour_blocks<W: PathWorldView + ?Sized>(
    ctx: &mut PathfindingContext<'_, W>,
    x: i32,
    y: i32,
    z: i32,
    block_path_type: PathType,
) -> PathType {
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if dx != 0 || dz != 0 {
                    let path_type = ctx.get_path_type_from_state(x + dx, y + dy, z + dz);
                    if path_type == PathType::DamageOther {
                        return PathType::DangerOther;
                    }
                    if path_type == PathType::DamageFire || path_type == PathType::Lava {
                        return PathType::DangerFire;
                    }
                    if path_type == PathType::Water {
                        return PathType::WaterBorder;
                    }
                    if path_type == PathType::DamageCautious {
                        return PathType::DamageCautious;
                    }
                }
            }
        }
    }
    block_path_type
}

/// Exact port of `WalkNodeEvaluator.getFloorLevel(BlockGetter, BlockPos)`.
pub fn get_floor_level_static<W: PathWorldView + ?Sized>(world: &W, pos: BlockPos) -> f64 {
    let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
    let info = world.block_info(below);
    let max_y = info.collision_max_y.unwrap_or(0.0);
    below.y as f64 + max_y
}

/// Port of `net.minecraft.world.level.pathfinder.WalkNodeEvaluator` in Paper 26.2.
#[derive(Debug, Default)]
pub struct WalkNodeEvaluator {
    pub config: EvaluatorConfig,
    pub(crate) amphibious: bool,
    path_types_by_pos_cache: HashMap<i64, PathType>,
    collision_cache: HashMap<i64, bool>,
}

impl WalkNodeEvaluator {
    pub const SPACE_BETWEEN_WALL_POSTS: f64 = 0.5;
    const DEFAULT_MOB_JUMP_HEIGHT: f64 = 1.125;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_caches(&mut self) {
        self.path_types_by_pos_cache.clear();
        self.collision_cache.clear();
    }

    fn get_cached_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let key = block_pos_as_long(x, y, z);
        if let Some(&pt) = self.path_types_by_pos_cache.get(&key) {
            return pt;
        }
        let pt = self.get_path_type_of_mob(ctx, mob, x, y, z);
        self.path_types_by_pos_cache.insert(key, pt);
        pt
    }

    fn has_collisions<W: PathWorldView + ?Sized>(&mut self, world: &W, aabb: Aabb) -> bool {
        // Quantize AABB key by its integer min corner + dimensions for cache lookup
        let key = block_pos_as_long(
            mth::floor_d(aabb.min_x * 16.0),
            mth::floor_d(aabb.min_y * 16.0),
            mth::floor_d(aabb.min_z * 16.0),
        );
        if let Some(&cached) = self.collision_cache.get(&key) {
            return cached;
        }
        let collides = !world.is_aabb_collision_free(aabb);
        self.collision_cache.insert(key, collides);
        collides
    }

    fn get_mob_jump_height(&self, mob: &MobPathingState) -> f64 {
        f64::from(mob.max_up_step).max(Self::DEFAULT_MOB_JUMP_HEIGHT)
    }

    fn get_floor_level<W: PathWorldView + ?Sized>(
        &self,
        world: &W,
        mob: &MobPathingState,
        pos: BlockPos,
    ) -> f64 {
        if (self.config.can_float || self.is_amphibious()) && world.block_info(pos).is_water {
            pos.y as f64 + 0.5
        } else {
            let _ = mob;
            get_floor_level_static(world, pos)
        }
    }

    fn is_amphibious(&self) -> bool {
        self.amphibious
    }

    fn amphibious_path_type<W: PathWorldView + ?Sized>(
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let block_path_type = ctx.get_path_type_from_state(x, y, z);
        if block_path_type == PathType::Water {
            for dir in Direction::ALL {
                let neighbor_type = ctx.get_path_type_from_state(
                    x + dir.step_x(),
                    y + dir.step_y(),
                    z + dir.step_z(),
                );
                if neighbor_type == PathType::Blocked {
                    return PathType::WaterBorder;
                }
            }
            return PathType::Water;
        }
        get_path_type_static(ctx, x, y, z)
    }

    fn can_start_at<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        pos: BlockPos,
    ) -> bool {
        let pt = self.get_cached_path_type(ctx, mob, pos.x, pos.y, pos.z);
        pt != PathType::Open && mob.get_pathfinding_malus(pt) >= 0.0
    }

    fn get_start_node<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        pos: BlockPos,
    ) -> usize {
        let node_idx = arena.get_or_create(pos.x, pos.y, pos.z);
        let pt = self.get_cached_path_type(ctx, mob, pos.x, pos.y, pos.z);
        let malus = mob.get_pathfinding_malus(pt);
        let node = arena.get_mut(node_idx);
        node.path_type = pt;
        node.cost_malus = malus;
        node_idx
    }

    fn is_neighbor_valid(arena: &NodeArena, neighbor: Option<usize>, current: usize) -> bool {
        let Some(n_idx) = neighbor else {
            return false;
        };
        let n = arena.get(n_idx);
        let c = arena.get(current);
        !n.closed && (n.cost_malus >= 0.0 || c.cost_malus < 0.0)
    }

    fn is_diagonal_valid(
        arena: &NodeArena,
        mob: &MobPathingState,
        current: usize,
        adj1: Option<usize>,
        adj2: Option<usize>,
    ) -> bool {
        let (Some(a1_idx), Some(a2_idx)) = (adj1, adj2) else {
            return false;
        };
        let cur = arena.get(current);
        let a1 = arena.get(a1_idx);
        let a2 = arena.get(a2_idx);
        if a2.y > cur.y || a1.y > cur.y {
            return false;
        }
        if a1.path_type == PathType::WalkableDoor || a2.path_type == PathType::WalkableDoor {
            return false;
        }
        let is_fence_gap = a2.path_type == PathType::Fence
            && a1.path_type == PathType::Fence
            && mob.bb_width < 0.5;
        (a2.y < cur.y || a2.cost_malus >= 0.0 || is_fence_gap)
            && (a1.y < cur.y || a1.cost_malus >= 0.0 || is_fence_gap)
    }

    fn is_diagonal_valid_node(arena: &NodeArena, diag: Option<usize>) -> bool {
        let Some(d_idx) = diag else {
            return false;
        };
        let d = arena.get(d_idx);
        if d.closed {
            return false;
        }
        if d.path_type == PathType::WalkableDoor {
            return false;
        }
        d.cost_malus >= 0.0
    }

    fn does_block_have_partial_collision(path_type: PathType) -> bool {
        matches!(
            path_type,
            PathType::Fence | PathType::DoorWoodClosed | PathType::DoorIronClosed
        )
    }

    fn can_reach_without_collision<W: PathWorldView + ?Sized>(
        &mut self,
        world: &W,
        mob: &MobPathingState,
        node: &crate::ai::navigation::node::Node,
    ) -> bool {
        let bb = mob.bounding_box();
        let dx = node.x as f64 - bb.min_x + (bb.max_x - bb.min_x) / 2.0;
        let dy = node.y as f64 - bb.min_y + (bb.max_y - bb.min_y) / 2.0;
        let dz = node.z as f64 - bb.min_z + (bb.max_z - bb.min_z) / 2.0;
        let step_count =
            mth::ceil_d((dx * dx + dy * dy + dz * dz).sqrt() / f64::from(mob.bb_width.max(0.1)));
        if step_count <= 0 {
            return true;
        }
        let sx = dx / step_count as f64;
        let sy = dy / step_count as f64;
        let sz = dz / step_count as f64;
        for i in 1..=step_count {
            let shifted = Aabb::new(
                bb.min_x + sx * i as f64,
                bb.min_y + sy * i as f64,
                bb.min_z + sz * i as f64,
                bb.max_x + sx * i as f64,
                bb.max_y + sy * i as f64,
                bb.max_z + sz * i as f64,
            );
            if self.has_collisions(world, shifted) {
                return false;
            }
        }
        true
    }

    fn get_node_and_update_cost_to_max(
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
        path_type: PathType,
        cost_malus: f32,
    ) -> usize {
        let idx = arena.get_or_create(x, y, z);
        let node = arena.get_mut(idx);
        node.path_type = path_type;
        node.cost_malus = node.cost_malus.max(cost_malus);
        idx
    }

    fn get_blocked_node(arena: &mut NodeArena, x: i32, y: i32, z: i32) -> usize {
        let idx = arena.get_or_create(x, y, z);
        let node = arena.get_mut(idx);
        node.path_type = PathType::Blocked;
        node.cost_malus = -1.0;
        idx
    }

    fn get_closed_node(
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
        path_type: PathType,
    ) -> usize {
        let idx = arena.get_or_create(x, y, z);
        let node = arena.get_mut(idx);
        node.path_type = path_type;
        node.cost_malus = path_type.malus();
        node.closed = true;
        idx
    }

    #[allow(clippy::too_many_arguments)]
    fn find_accepted_node<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
        vertical_delta_limit: i32,
        node_floor_level: f64,
        dir: Direction,
        path_type_at_current: PathType,
    ) -> Option<usize> {
        let mut path_type = self.get_cached_path_type(ctx, mob, x, y, z);
        let floor_level = self.get_floor_level(ctx.level(), mob, BlockPos::new(x, y, z));
        if floor_level - node_floor_level > self.get_mob_jump_height(mob) {
            return None;
        }

        let mut cost_malus = mob.get_pathfinding_malus(path_type);
        let mut best = if cost_malus >= 0.0 {
            Some(Self::get_node_and_update_cost_to_max(
                arena, x, y, z, path_type, cost_malus,
            ))
        } else {
            None
        };

        if Self::does_block_have_partial_collision(path_type_at_current)
            && best.is_some()
            && !self.can_reach_without_collision(ctx.level(), mob, arena.get(best.unwrap()))
        {
            best = None;
        }

        if path_type != PathType::Walkable
            && (!self.is_amphibious() || path_type != PathType::Water)
        {
            if (best.is_none() || arena.get(best.unwrap()).cost_malus < 0.0)
                && vertical_delta_limit > 0
                && (path_type != PathType::Fence || self.config.can_walk_over_fences)
                && path_type != PathType::UnpassableRail
                && path_type != PathType::Trapdoor
                && path_type != PathType::PowderSnow
            {
                best = self.try_jump_on(
                    ctx,
                    mob,
                    arena,
                    x,
                    y,
                    z,
                    vertical_delta_limit,
                    node_floor_level,
                    dir,
                    path_type_at_current,
                );
            } else if !self.is_amphibious()
                && path_type == PathType::Water
                && !self.config.can_float
            {
                best = self.try_find_first_non_water_below(ctx, mob, arena, x, y, z, best);
            } else if path_type == PathType::Open {
                best = self.try_find_first_ground_node_below(ctx, mob, arena, x, y, z);
            } else if Self::does_block_have_partial_collision(path_type) && best.is_none() {
                best = Some(Self::get_closed_node(arena, x, y, z, path_type));
            }
            let _ = (&mut path_type, &mut cost_malus);
        }

        best
    }

    #[allow(clippy::too_many_arguments)]
    fn try_jump_on<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
        vertical_delta_limit: i32,
        node_floor_level: f64,
        dir: Direction,
        path_type_at_current: PathType,
    ) -> Option<usize> {
        let node_above = self.find_accepted_node(
            ctx,
            mob,
            arena,
            x,
            y + 1,
            z,
            vertical_delta_limit - 1,
            node_floor_level,
            dir,
            path_type_at_current,
        )?;
        if mob.bb_width >= 1.0 {
            return Some(node_above);
        }
        let above = arena.get(node_above);
        if above.path_type != PathType::Open && above.path_type != PathType::Walkable {
            return Some(node_above);
        }

        let center_x = (x - dir.step_x()) as f64 + 0.5;
        let center_z = (z - dir.step_z()) as f64 + 0.5;
        let half_width = f64::from(mob.bb_width) / 2.0;
        let above_pos = BlockPos::new(above.x, above.y, above.z);
        let min_y = self.get_floor_level(
            ctx.level(),
            mob,
            BlockPos::new(mth::floor_d(center_x), y + 1, mth::floor_d(center_z)),
        ) + 0.001;
        let max_y =
            f64::from(mob.bb_height) + self.get_floor_level(ctx.level(), mob, above_pos) - 0.002;
        let grow = Aabb::new(
            center_x - half_width,
            min_y,
            center_z - half_width,
            center_x + half_width,
            max_y,
            center_z + half_width,
        );
        if self.has_collisions(ctx.level(), grow) {
            None
        } else {
            Some(node_above)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn try_find_first_non_water_below<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        x: i32,
        mut y: i32,
        z: i32,
        mut best: Option<usize>,
    ) -> Option<usize> {
        y -= 1;
        while y > ctx.level().min_y() {
            let pt = self.get_cached_path_type(ctx, mob, x, y, z);
            if pt != PathType::Water {
                return best;
            }
            let malus = mob.get_pathfinding_malus(pt);
            best = Some(Self::get_node_and_update_cost_to_max(
                arena, x, y, z, pt, malus,
            ));
            y -= 1;
        }
        best
    }

    fn try_find_first_ground_node_below<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<usize> {
        let mut cur_y = y;
        let max_fall = mob.max_fall_distance.max(1);
        let min_y = ctx.level().min_y();

        while cur_y > min_y {
            cur_y -= 1;
            if y - cur_y > max_fall {
                return Some(Self::get_blocked_node(arena, x, cur_y, z));
            }
            let pt = self.get_cached_path_type(ctx, mob, x, cur_y, z);
            let malus = mob.get_pathfinding_malus(pt);
            if pt != PathType::Open {
                if malus >= 0.0 {
                    return Some(Self::get_node_and_update_cost_to_max(
                        arena, x, cur_y, z, pt, malus,
                    ));
                }
                return Some(Self::get_blocked_node(arena, x, cur_y, z));
            }
        }
        Some(Self::get_blocked_node(arena, x, cur_y, z))
    }

    fn evaluate_path_type_at_pos<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let amphibious = self.amphibious;
        self.evaluate_path_type_with(ctx, mob, x, y, z, |ctx, bx, by, bz| {
            if amphibious {
                Self::amphibious_path_type(ctx, bx, by, bz)
            } else {
                get_path_type_static(ctx, bx, by, bz)
            }
        })
    }

    pub(crate) fn evaluate_path_type_with<W, F>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
        mut get_type: F,
    ) -> PathType
    where
        W: PathWorldView + ?Sized,
        F: FnMut(&mut PathfindingContext<'_, W>, i32, i32, i32) -> PathType,
    {
        let mut mask: u32 = 0;
        let mob_pos = ctx.mob_position();

        for dx in 0..self.config.entity_width {
            for dy in 0..self.config.entity_height {
                for dz in 0..self.config.entity_depth {
                    let mut pt = get_type(ctx, x + dx, y + dy, z + dz);
                    pt = self.evaluate_block_path_type(ctx, mob, mob_pos, pt);
                    mask |= 1_u32 << pt.index();
                }
            }
        }

        if mask.count_ones() == 1 {
            return PathType::ALL[mask.trailing_zeros() as usize];
        }
        if (mask & (1_u32 << PathType::Fence.index())) != 0 {
            return PathType::Fence;
        }
        if (mask & (1_u32 << PathType::UnpassableRail.index())) != 0 {
            return PathType::UnpassableRail;
        }

        let mut highest_type = PathType::Blocked;
        let mut highest_malus = mob.get_pathfinding_malus(highest_type);

        // Iterate in Paper EnumSet<PathType> ordinal order
        for pt in PathType::ALL {
            if (mask & (1_u32 << pt.index())) == 0 {
                continue;
            }
            let malus = mob.get_pathfinding_malus(pt);
            if malus < 0.0 {
                return pt;
            }
            if malus >= highest_malus {
                highest_malus = malus;
                highest_type = pt;
            }
        }

        let current_node_type = get_type(ctx, x, y, z);
        if self.config.entity_width > 1 {
            let is_current_cheaper = mob.get_pathfinding_malus(current_node_type) < highest_malus;
            let cap_malus = is_current_cheaper
                && mob.get_pathfinding_malus(PathType::BigMobsCloseToDanger) < highest_malus;
            if cap_malus {
                PathType::BigMobsCloseToDanger
            } else {
                highest_type
            }
        } else if current_node_type == PathType::Open
            && highest_type != PathType::Open
            && highest_malus == 0.0
        {
            PathType::Open
        } else {
            highest_type
        }
    }

    fn evaluate_block_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        mob_pos: BlockPos,
        mut path_type: PathType,
    ) -> PathType {
        if path_type == PathType::DoorWoodClosed
            && self.config.can_open_doors
            && self.config.can_pass_doors
        {
            path_type = PathType::WalkableDoor;
        }
        if path_type == PathType::DoorOpen && !self.config.can_pass_doors {
            path_type = PathType::Blocked;
        }
        if path_type == PathType::Rail
            && self.get_path_type(ctx, mob_pos.x, mob_pos.y, mob_pos.z) != PathType::Rail
            && self.get_path_type(ctx, mob_pos.x, mob_pos.y - 1, mob_pos.z) != PathType::Rail
        {
            path_type = PathType::UnpassableRail;
        }
        if path_type == PathType::PowderSnow && mob.can_stand_on_powder_snow {
            path_type = PathType::Walkable;
        }
        path_type
    }
}

impl NodeEvaluator for WalkNodeEvaluator {
    fn config(&self) -> &EvaluatorConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut EvaluatorConfig {
        &mut self.config
    }

    fn prepare<W: PathWorldView + ?Sized>(&mut self, mob: &mut MobPathingState, _world: &W) {
        self.config.prepare_dimensions(mob);
        self.clear_caches();
    }

    fn done(&mut self, _mob: &mut MobPathingState) {
        self.clear_caches();
    }

    fn get_start<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
    ) -> usize {
        let mut y = mth::floor_d(mob.pos.y + 0.5);
        let base_pos = BlockPos::new(mth::floor_d(mob.pos.x), y, mth::floor_d(mob.pos.z));
        if self.config.can_float && mob.in_water {
            let mut probe_y = mth::floor_d(mob.pos.y);
            while ctx
                .get_block_info(BlockPos::new(base_pos.x, probe_y, base_pos.z))
                .is_water
            {
                probe_y += 1;
            }
            y = probe_y - 1;
        } else if !mob.on_ground {
            let mut probe_y = mth::floor_d(mob.pos.y);
            while probe_y > ctx.level().min_y()
                && !ctx
                    .get_block_info(BlockPos::new(base_pos.x, probe_y - 1, base_pos.z))
                    .has_collision()
            {
                probe_y -= 1;
            }
            y = probe_y;
        }

        let start = BlockPos::new(base_pos.x, y, base_pos.z);
        if self.can_start_at(ctx, mob, start) {
            return self.get_start_node(ctx, mob, arena, start);
        }

        // Try the 4 corners of the mob's bounding box in deterministic order
        let bb = mob.bounding_box();
        let corners = [
            BlockPos::new(mth::floor_d(bb.min_x), y, mth::floor_d(bb.min_z)),
            BlockPos::new(mth::floor_d(bb.min_x), y, mth::floor_d(bb.max_z)),
            BlockPos::new(mth::floor_d(bb.max_x), y, mth::floor_d(bb.min_z)),
            BlockPos::new(mth::floor_d(bb.max_x), y, mth::floor_d(bb.max_z)),
        ];
        for corner in corners {
            if self.can_start_at(ctx, mob, corner) {
                return self.get_start_node(ctx, mob, arena, corner);
            }
        }

        self.get_start_node(ctx, mob, arena, start)
    }

    fn get_neighbors<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        node_idx: usize,
        out: &mut [usize; 32],
    ) -> usize {
        let (nx, ny, nz) = {
            let n = arena.get(node_idx);
            (n.x, n.y, n.z)
        };
        let mut count = 0;

        let path_type_above = self.get_cached_path_type(ctx, mob, nx, ny + 1, nz);
        let path_type_current = self.get_cached_path_type(ctx, mob, nx, ny, nz);
        let vertical_delta_limit = if mob.get_pathfinding_malus(path_type_above) >= 0.0
            && path_type_current != PathType::StickyHoney
        {
            mth::floor(mob.max_up_step.max(1.0))
        } else {
            0
        };

        let floor_level = self.get_floor_level(ctx.level(), mob, BlockPos::new(nx, ny, nz));

        // Exact Paper 26.2 order: SOUTH, WEST, EAST, NORTH
        let mut reusable = [None; 4];
        let dirs = [
            Direction::South,
            Direction::West,
            Direction::East,
            Direction::North,
        ];
        for (i, dir) in dirs.iter().copied().enumerate() {
            let neighbor = self.find_accepted_node(
                ctx,
                mob,
                arena,
                nx + dir.step_x(),
                ny,
                nz + dir.step_z(),
                vertical_delta_limit,
                floor_level,
                dir,
                path_type_current,
            );
            reusable[i] = neighbor;
            if Self::is_neighbor_valid(arena, neighbor, node_idx) {
                out[count] = neighbor.unwrap();
                count += 1;
            }
        }

        // Exact Paper 26.2 diagonal order:
        // (North, East), (East, South), (South, West), (West, North)
        // Mapped into `reusable` indices: South=0, West=1, East=2, North=3
        let diag_pairs = [
            (Direction::North, Direction::East, 3_usize, 2_usize),
            (Direction::East, Direction::South, 2_usize, 0_usize),
            (Direction::South, Direction::West, 0_usize, 1_usize),
            (Direction::West, Direction::North, 1_usize, 3_usize),
        ];
        for (d1, d2, i1, i2) in diag_pairs {
            if Self::is_diagonal_valid(arena, mob, node_idx, reusable[i1], reusable[i2]) {
                let diag = self.find_accepted_node(
                    ctx,
                    mob,
                    arena,
                    nx + d1.step_x() + d2.step_x(),
                    ny,
                    nz + d1.step_z() + d2.step_z(),
                    vertical_delta_limit,
                    floor_level,
                    d1,
                    path_type_current,
                );
                if Self::is_diagonal_valid_node(arena, diag) {
                    out[count] = diag.unwrap();
                    count += 1;
                }
            }
        }

        count
    }

    fn get_path_type_of_mob<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        self.evaluate_path_type_at_pos(ctx, mob, x, y, z)
    }

    fn get_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        if self.amphibious {
            Self::amphibious_path_type(ctx, x, y, z)
        } else {
            get_path_type_static(ctx, x, y, z)
        }
    }
}

/// Port of `net.minecraft.world.level.pathfinder.FlyNodeEvaluator` in Paper 26.2.
#[derive(Debug, Default)]
pub struct FlyNodeEvaluator {
    walk: WalkNodeEvaluator,
    path_type_by_pos_cache: HashMap<i64, PathType>,
}

impl FlyNodeEvaluator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn fly_path_type<W: PathWorldView + ?Sized>(
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let mut pt = ctx.get_path_type_from_state(x, y, z);
        if pt == PathType::Open && y > ctx.level().min_y() {
            let below = ctx.get_path_type_from_state(x, y - 1, z);
            if below == PathType::DamageFire || below == PathType::Lava {
                pt = PathType::DamageFire;
            } else if below == PathType::DamageOther {
                pt = PathType::DamageOther;
            } else if below == PathType::Cocoa {
                pt = PathType::Cocoa;
            } else if below == PathType::Fence {
                pt = PathType::Fence;
            } else {
                pt = if below != PathType::Walkable
                    && below != PathType::Open
                    && below != PathType::Water
                {
                    PathType::Walkable
                } else {
                    PathType::Open
                };
            }
        }
        if pt == PathType::Walkable || pt == PathType::Open {
            pt = check_neighbour_blocks(ctx, x, y, z, pt);
        }
        pt
    }

    fn get_cached_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let key = block_pos_as_long(x, y, z);
        if let Some(&pt) = self.path_type_by_pos_cache.get(&key) {
            return pt;
        }
        let pt = self.get_path_type_of_mob(ctx, mob, x, y, z);
        self.path_type_by_pos_cache.insert(key, pt);
        pt
    }

    fn find_accepted_node<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<usize> {
        let pt = self.get_cached_path_type(ctx, mob, x, y, z);
        let malus = mob.get_pathfinding_malus(pt);
        if malus >= 0.0 {
            let idx = arena.get_or_create(x, y, z);
            let node = arena.get_mut(idx);
            node.path_type = pt;
            node.cost_malus = node.cost_malus.max(malus);
            if pt == PathType::Walkable {
                node.cost_malus += 1.0;
            }
            Some(idx)
        } else {
            None
        }
    }

    fn is_open(arena: &NodeArena, node: Option<usize>) -> bool {
        node.is_some_and(|idx| !arena.get(idx).closed)
    }

    fn has_malus(arena: &NodeArena, node: Option<usize>) -> bool {
        node.is_some_and(|idx| arena.get(idx).cost_malus >= 0.0)
    }
}

impl NodeEvaluator for FlyNodeEvaluator {
    fn config(&self) -> &EvaluatorConfig {
        &self.walk.config
    }

    fn config_mut(&mut self) -> &mut EvaluatorConfig {
        &mut self.walk.config
    }

    fn prepare<W: PathWorldView + ?Sized>(&mut self, mob: &mut MobPathingState, world: &W) {
        self.walk.prepare(mob, world);
        self.path_type_by_pos_cache.clear();
    }

    fn done(&mut self, mob: &mut MobPathingState) {
        self.walk.done(mob);
        self.path_type_by_pos_cache.clear();
    }

    fn get_start<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
    ) -> usize {
        let mut y = mth::floor_d(mob.pos.y + 0.5);
        let base_x = mth::floor_d(mob.pos.x);
        let base_z = mth::floor_d(mob.pos.z);
        if self.walk.config.can_float && mob.in_water {
            let mut probe_y = mth::floor_d(mob.pos.y);
            while ctx
                .get_block_info(BlockPos::new(base_x, probe_y, base_z))
                .is_water
            {
                probe_y += 1;
            }
            y = probe_y;
        }
        let start_pos = BlockPos::new(base_x, y, base_z);
        let pt = self.get_cached_path_type(ctx, mob, start_pos.x, start_pos.y, start_pos.z);
        let malus = mob.get_pathfinding_malus(pt);
        let idx = arena.get_or_create(start_pos.x, start_pos.y, start_pos.z);
        let node = arena.get_mut(idx);
        node.path_type = pt;
        node.cost_malus = malus;
        idx
    }

    fn get_neighbors<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        node_idx: usize,
        out: &mut [usize; 32],
    ) -> usize {
        let (x, y, z) = {
            let n = arena.get(node_idx);
            (n.x, n.y, n.z)
        };
        let mut count = 0;

        // Exact Paper 26.2 FlyNodeEvaluator cardinal order: S, W, E, N, U, D
        let south = self.find_accepted_node(ctx, mob, arena, x, y, z + 1);
        if Self::is_open(arena, south) {
            out[count] = south.unwrap();
            count += 1;
        }
        let west = self.find_accepted_node(ctx, mob, arena, x - 1, y, z);
        if Self::is_open(arena, west) {
            out[count] = west.unwrap();
            count += 1;
        }
        let east = self.find_accepted_node(ctx, mob, arena, x + 1, y, z);
        if Self::is_open(arena, east) {
            out[count] = east.unwrap();
            count += 1;
        }
        let north = self.find_accepted_node(ctx, mob, arena, x, y, z - 1);
        if Self::is_open(arena, north) {
            out[count] = north.unwrap();
            count += 1;
        }
        let up = self.find_accepted_node(ctx, mob, arena, x, y + 1, z);
        if Self::is_open(arena, up) {
            out[count] = up.unwrap();
            count += 1;
        }
        let down = self.find_accepted_node(ctx, mob, arena, x, y - 1, z);
        if Self::is_open(arena, down) {
            out[count] = down.unwrap();
            count += 1;
        }

        let open_up = Self::has_malus(arena, up);
        let open_down = Self::has_malus(arena, down);
        let open_north = Self::has_malus(arena, north);
        let open_south = Self::has_malus(arena, south);
        let open_east = Self::has_malus(arena, east);
        let open_west = Self::has_malus(arena, west);

        // Vertical diagonals (Paper order)
        let mut try_push = |arena: &mut NodeArena,
                            cond: bool,
                            dx: i32,
                            dy: i32,
                            dz: i32,
                            count: &mut usize|
         -> Option<usize> {
            if cond {
                let n = self.find_accepted_node(ctx, mob, arena, x + dx, y + dy, z + dz);
                if Self::is_open(arena, n) {
                    out[*count] = n.unwrap();
                    *count += 1;
                }
                n
            } else {
                None
            }
        };

        let north_up = try_push(arena, open_north && open_up, 0, 1, -1, &mut count);
        let south_up = try_push(arena, open_south && open_up, 0, 1, 1, &mut count);
        let east_up = try_push(arena, open_east && open_up, 1, 1, 0, &mut count);
        let west_up = try_push(arena, open_west && open_up, -1, 1, 0, &mut count);
        let north_down = try_push(arena, open_north && open_down, 0, -1, -1, &mut count);
        let south_down = try_push(arena, open_south && open_down, 0, -1, 1, &mut count);
        let east_down = try_push(arena, open_east && open_down, 1, -1, 0, &mut count);
        let west_down = try_push(arena, open_west && open_down, -1, -1, 0, &mut count);

        // Horizontal diagonals (Paper order)
        let north_east = try_push(arena, open_north && open_east, 1, 0, -1, &mut count);
        let north_west = try_push(arena, open_north && open_west, -1, 0, -1, &mut count);
        let south_east = try_push(arena, open_south && open_east, 1, 0, 1, &mut count);
        let south_west = try_push(arena, open_south && open_west, -1, 0, 1, &mut count);

        // 3D corners
        let open_ne = Self::has_malus(arena, north_east);
        let open_nw = Self::has_malus(arena, north_west);
        let open_se = Self::has_malus(arena, south_east);
        let open_sw = Self::has_malus(arena, south_west);
        let open_nu = Self::has_malus(arena, north_up);
        let open_su = Self::has_malus(arena, south_up);
        let open_eu = Self::has_malus(arena, east_up);
        let open_wu = Self::has_malus(arena, west_up);
        let open_nd = Self::has_malus(arena, north_down);
        let open_sd = Self::has_malus(arena, south_down);
        let open_ed = Self::has_malus(arena, east_down);
        let open_wd = Self::has_malus(arena, west_down);

        let _ = try_push(
            arena,
            open_up && open_ne && open_nu && open_eu,
            1,
            1,
            -1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_up && open_nw && open_nu && open_wu,
            -1,
            1,
            -1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_up && open_se && open_su && open_eu,
            1,
            1,
            1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_up && open_sw && open_su && open_wu,
            -1,
            1,
            1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_down && open_ne && open_nd && open_ed,
            1,
            -1,
            -1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_down && open_nw && open_nd && open_wd,
            -1,
            -1,
            -1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_down && open_se && open_sd && open_ed,
            1,
            -1,
            1,
            &mut count,
        );
        let _ = try_push(
            arena,
            open_down && open_sw && open_sd && open_wd,
            -1,
            -1,
            1,
            &mut count,
        );

        count
    }

    fn get_path_type_of_mob<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        self.walk
            .evaluate_path_type_with(ctx, mob, x, y, z, Self::fly_path_type)
    }

    fn get_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        Self::fly_path_type(ctx, x, y, z)
    }
}

/// Port of `net.minecraft.world.level.pathfinder.SwimNodeEvaluator` in Paper 26.2.
#[derive(Debug, Default)]
pub struct SwimNodeEvaluator {
    pub config: EvaluatorConfig,
    pub allow_breaching: bool,
    path_types_by_pos_cache: HashMap<i64, PathType>,
}

impl SwimNodeEvaluator {
    #[must_use]
    pub fn new(allow_breaching: bool) -> Self {
        Self {
            config: EvaluatorConfig {
                can_pass_doors: false,
                ..EvaluatorConfig::default()
            },
            allow_breaching,
            path_types_by_pos_cache: HashMap::new(),
        }
    }

    fn get_cached_block_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let key = block_pos_as_long(x, y, z);
        if let Some(&pt) = self.path_types_by_pos_cache.get(&key) {
            return pt;
        }
        let pt = self.get_path_type_of_mob(ctx, mob, x, y, z);
        self.path_types_by_pos_cache.insert(key, pt);
        pt
    }

    fn find_accepted_node<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<usize> {
        let pt = self.get_cached_block_type(ctx, mob, x, y, z);
        if (self.allow_breaching && pt == PathType::Breach) || pt == PathType::Water {
            let malus = mob.get_pathfinding_malus(pt);
            if malus >= 0.0 {
                let idx = arena.get_or_create(x, y, z);
                let node = arena.get_mut(idx);
                node.path_type = pt;
                node.cost_malus = node.cost_malus.max(malus);
                let info = ctx.get_block_info(BlockPos::new(x, y, z));
                if !info.is_water && !info.is_lava {
                    node.cost_malus += 8.0;
                }
                return Some(idx);
            }
        }
        None
    }

    fn is_node_valid(arena: &NodeArena, node: Option<usize>) -> bool {
        node.is_some_and(|idx| !arena.get(idx).closed)
    }

    fn has_malus(arena: &NodeArena, node: Option<usize>) -> bool {
        node.is_some_and(|idx| arena.get(idx).cost_malus >= 0.0)
    }
}

impl NodeEvaluator for SwimNodeEvaluator {
    fn config(&self) -> &EvaluatorConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut EvaluatorConfig {
        &mut self.config
    }

    fn prepare<W: PathWorldView + ?Sized>(&mut self, mob: &mut MobPathingState, _world: &W) {
        self.config.prepare_dimensions(mob);
        self.path_types_by_pos_cache.clear();
    }

    fn done(&mut self, _mob: &mut MobPathingState) {
        self.path_types_by_pos_cache.clear();
    }

    fn get_start<W: PathWorldView + ?Sized>(
        &mut self,
        _ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
    ) -> usize {
        let bb = mob.bounding_box();
        arena.get_or_create(
            mth::floor_d(bb.min_x),
            mth::floor_d(bb.min_y + 0.5),
            mth::floor_d(bb.min_z),
        )
    }

    fn get_neighbors<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        node_idx: usize,
        out: &mut [usize; 32],
    ) -> usize {
        let (x, y, z) = {
            let n = arena.get(node_idx);
            (n.x, n.y, n.z)
        };
        let mut count = 0;
        let mut by_dir = [None; 6];

        for (i, dir) in Direction::ALL.iter().copied().enumerate() {
            let neighbor = self.find_accepted_node(
                ctx,
                mob,
                arena,
                x + dir.step_x(),
                y + dir.step_y(),
                z + dir.step_z(),
            );
            by_dir[i] = neighbor;
            if Self::is_node_valid(arena, neighbor) {
                out[count] = neighbor.unwrap();
                count += 1;
            }
        }

        // Horizontal diagonals using clockwise pairs (Paper exact order)
        for dir in Direction::HORIZONTAL {
            let cw = dir.clock_wise();
            let d1_idx = Direction::ALL.iter().position(|&d| d == dir).unwrap();
            let d2_idx = Direction::ALL.iter().position(|&d| d == cw).unwrap();
            if Self::has_malus(arena, by_dir[d1_idx]) && Self::has_malus(arena, by_dir[d2_idx]) {
                let diag = self.find_accepted_node(
                    ctx,
                    mob,
                    arena,
                    x + dir.step_x() + cw.step_x(),
                    y,
                    z + dir.step_z() + cw.step_z(),
                );
                if Self::is_node_valid(arena, diag) {
                    out[count] = diag.unwrap();
                    count += 1;
                }
            }
        }

        count
    }

    fn get_path_type_of_mob<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        _mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let mut last_water_pathfindable = false;
        for dx in x..(x + self.config.entity_width) {
            for dy in y..(y + self.config.entity_height) {
                for dz in z..(z + self.config.entity_depth) {
                    let info = ctx.get_block_info(BlockPos::new(dx, dy, dz));
                    if !info.is_loaded {
                        return PathType::Blocked;
                    }
                    if !info.is_water && !info.is_lava {
                        if info.is_air {
                            let below = ctx.get_block_info(BlockPos::new(dx, dy - 1, dz));
                            if below.is_pathfindable_water {
                                return PathType::Breach;
                            }
                        }
                        return PathType::Blocked;
                    }
                    if !info.is_water {
                        return PathType::Blocked;
                    }
                    last_water_pathfindable = info.is_pathfindable_water;
                }
            }
        }
        if last_water_pathfindable {
            PathType::Water
        } else {
            PathType::Blocked
        }
    }

    fn get_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        let info = ctx.get_block_info(BlockPos::new(x, y, z));
        if info.is_water && info.is_pathfindable_water {
            PathType::Water
        } else {
            PathType::Blocked
        }
    }
}

/// Port of `net.minecraft.world.level.pathfinder.AmphibiousNodeEvaluator` in Paper 26.2.
#[derive(Debug, Default)]
pub struct AmphibiousNodeEvaluator {
    walk: WalkNodeEvaluator,
    pub prefers_shallow_swimming: bool,
    old_walkable_cost: f32,
    old_water_border_cost: f32,
}

impl AmphibiousNodeEvaluator {
    #[must_use]
    pub fn new(prefers_shallow_swimming: bool) -> Self {
        let mut walk = WalkNodeEvaluator::new();
        walk.amphibious = true;
        Self {
            walk,
            prefers_shallow_swimming,
            old_walkable_cost: 0.0,
            old_water_border_cost: 0.0,
        }
    }

    fn is_vertical_neighbor_valid(
        arena: &NodeArena,
        neighbor: Option<usize>,
        current: usize,
    ) -> bool {
        WalkNodeEvaluator::is_neighbor_valid(arena, neighbor, current)
            && neighbor.is_some_and(|idx| arena.get(idx).path_type == PathType::Water)
    }
}

impl NodeEvaluator for AmphibiousNodeEvaluator {
    fn config(&self) -> &EvaluatorConfig {
        &self.walk.config
    }

    fn config_mut(&mut self) -> &mut EvaluatorConfig {
        &mut self.walk.config
    }

    fn prepare<W: PathWorldView + ?Sized>(&mut self, mob: &mut MobPathingState, world: &W) {
        self.walk.prepare(mob, world);
        mob.set_pathfinding_malus(PathType::Water, 0.0);
        self.old_walkable_cost = mob.get_pathfinding_malus(PathType::Walkable);
        mob.set_pathfinding_malus(PathType::Walkable, 6.0);
        self.old_water_border_cost = mob.get_pathfinding_malus(PathType::WaterBorder);
        mob.set_pathfinding_malus(PathType::WaterBorder, 4.0);
    }

    fn done(&mut self, mob: &mut MobPathingState) {
        mob.set_pathfinding_malus(PathType::Walkable, self.old_walkable_cost);
        mob.set_pathfinding_malus(PathType::WaterBorder, self.old_water_border_cost);
        self.walk.done(mob);
    }

    fn get_start<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
    ) -> usize {
        if !mob.in_water {
            self.walk.get_start(ctx, mob, arena)
        } else {
            let bb = mob.bounding_box();
            let pos = BlockPos::new(
                mth::floor_d(bb.min_x),
                mth::floor_d(bb.min_y + 0.5),
                mth::floor_d(bb.min_z),
            );
            let idx = arena.get_or_create(pos.x, pos.y, pos.z);
            let pt = self.get_path_type_of_mob(ctx, mob, pos.x, pos.y, pos.z);
            let malus = mob.get_pathfinding_malus(pt);
            let node = arena.get_mut(idx);
            node.path_type = pt;
            node.cost_malus = malus;
            idx
        }
    }

    fn get_target(&self, x: f64, y: f64, z: f64) -> Target {
        Target::new(mth::floor_d(x), mth::floor_d(y + 0.5), mth::floor_d(z))
    }

    fn get_neighbors<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        node_idx: usize,
        out: &mut [usize; 32],
    ) -> usize {
        let mut count = self.walk.get_neighbors(ctx, mob, arena, node_idx, out);
        let (x, y, z) = {
            let n = arena.get(node_idx);
            (n.x, n.y, n.z)
        };
        let path_type_above = self.walk.get_cached_path_type(ctx, mob, x, y + 1, z);
        let path_type_current = self.walk.get_cached_path_type(ctx, mob, x, y, z);
        let vertical_delta_limit = if mob.get_pathfinding_malus(path_type_above) >= 0.0
            && path_type_current != PathType::StickyHoney
        {
            mth::floor(mob.max_up_step.max(1.0))
        } else {
            0
        };
        let floor_level = self
            .walk
            .get_floor_level(ctx.level(), mob, BlockPos::new(x, y, z));

        let up = self.walk.find_accepted_node(
            ctx,
            mob,
            arena,
            x,
            y + 1,
            z,
            (vertical_delta_limit - 1).max(0),
            floor_level,
            Direction::Up,
            path_type_current,
        );
        let down = self.walk.find_accepted_node(
            ctx,
            mob,
            arena,
            x,
            y - 1,
            z,
            vertical_delta_limit,
            floor_level,
            Direction::Down,
            path_type_current,
        );

        if Self::is_vertical_neighbor_valid(arena, up, node_idx) && count < 32 {
            out[count] = up.unwrap();
            count += 1;
        }
        if Self::is_vertical_neighbor_valid(arena, down, node_idx)
            && path_type_current != PathType::Trapdoor
            && count < 32
        {
            out[count] = down.unwrap();
            count += 1;
        }

        if self.prefers_shallow_swimming {
            let sea_level = ctx.level().sea_level();
            for &neighbor_idx in &out[..count] {
                let n = arena.get_mut(neighbor_idx);
                if n.path_type == PathType::Water && n.y < sea_level - 10 {
                    n.cost_malus += 1.0;
                }
            }
        }

        count
    }

    fn get_path_type_of_mob<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        self.walk.get_path_type_of_mob(ctx, mob, x, y, z)
    }

    fn get_path_type<W: PathWorldView + ?Sized>(
        &mut self,
        ctx: &mut PathfindingContext<'_, W>,
        x: i32,
        y: i32,
        z: i32,
    ) -> PathType {
        WalkNodeEvaluator::amphibious_path_type(ctx, x, y, z)
    }
}

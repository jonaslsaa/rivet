//! Port of `net.minecraft.world.level.pathfinder.PathFinder` in Paper 26.2.

use std::cmp::Ordering;

use crate::world::entity::ai::goal::context::{BlockPos, GoalRandom};
use crate::world::level::pathfinder::node::{BinaryHeap, Node, NodeArena, Target};
use crate::world::level::pathfinder::node_evaluator::NodeEvaluator;
use crate::world::level::pathfinder::path::Path;
use crate::world::level::pathfinder::path_type::PathTypeCache;
use crate::world::level::pathfinder::world_view::{
    MobPathingState, PathWorldView, PathfindingContext,
};

/// Port of `net.minecraft.world.level.pathfinder.PathFinder` in Paper 26.2.
#[derive(Debug)]
pub struct PathFinder {
    pub max_visited_nodes: i32,
    open_set: BinaryHeap,
    capture_debug: bool,
}

impl PathFinder {
    /// Exact Paper 26.2 A* heuristic multiplier (`PathFinder.FUDGING`).
    pub const FUDGING: f32 = 1.5;

    #[must_use]
    pub fn new(max_visited_nodes: i32) -> Self {
        Self {
            max_visited_nodes,
            open_set: BinaryHeap::new(),
            capture_debug: false,
        }
    }

    #[must_use]
    pub fn with_debug(mut self, capture_debug: bool) -> Self {
        self.capture_debug = capture_debug;
        self
    }

    pub fn set_max_visited_nodes(&mut self, max_visited_nodes: i32) {
        self.max_visited_nodes = max_visited_nodes;
    }

    /// Mirrors `PathFinder.findPath(PathNavigationRegion, Mob, Set<BlockPos>, float, int, float)`.
    ///
    /// Targets are passed as a slice in deterministic order (matching an ordered
    /// target set). Calls `evaluator.prepare`, runs A*, and always calls `evaluator.done`.
    /// `cache` is the level's `ServerLevel.getPathTypeCache()`, which Paper's
    /// `PathfindingContext(level, mob)` always uses on the server. `random` is the
    /// mob's `getRandom()`, used by `NodeEvaluator.getStart()`.
    #[allow(clippy::too_many_arguments)]
    pub fn find_path<E: NodeEvaluator, W: PathWorldView + ?Sized>(
        &mut self,
        evaluator: &mut E,
        world: &W,
        cache: &mut PathTypeCache,
        mob: &mut MobPathingState,
        random: &mut dyn GoalRandom,
        targets: &[BlockPos],
        max_path_distance: f32,
        reach_range: i32,
        max_visited_nodes_multiplier: f32,
    ) -> Option<Path> {
        if targets.is_empty() {
            return None;
        }

        evaluator.prepare(mob, world);
        let mut arena = NodeArena::new();
        let mut ctx = PathfindingContext::new(world, Some(cache), mob.block_position());
        let start_idx = evaluator.get_start(&mut ctx, mob, &mut arena, random);

        // Build ordered map of Target -> BlockPos (deterministic iteration)
        let mut target_entries: Vec<(Target, BlockPos)> = Vec::with_capacity(targets.len());
        for &pos in targets {
            target_entries.push((
                evaluator.get_target(&mut arena, pos.x as f64, pos.y as f64, pos.z as f64),
                pos,
            ));
        }

        let result = self.find_path_internal(
            evaluator,
            &mut ctx,
            mob,
            &mut arena,
            start_idx,
            &mut target_entries,
            max_path_distance,
            reach_range,
            max_visited_nodes_multiplier,
        );

        evaluator.done(mob);
        result
    }

    #[allow(clippy::too_many_arguments, clippy::neg_cmp_op_on_partial_ord)]
    fn find_path_internal<E: NodeEvaluator, W: PathWorldView + ?Sized>(
        &mut self,
        evaluator: &mut E,
        ctx: &mut PathfindingContext<'_, W>,
        mob: &MobPathingState,
        arena: &mut NodeArena,
        start_idx: usize,
        targets: &mut [(Target, BlockPos)],
        max_path_distance: f32,
        reach_range: i32,
        max_visited_nodes_multiplier: f32,
    ) -> Option<Path> {
        let mut reached_targets: Vec<usize> = Vec::new();
        let mut closed_indices: Vec<usize> = Vec::new();

        let best_h = Self::get_best_h(arena, start_idx, targets);
        {
            let start = arena.get_mut(start_idx);
            start.g = 0.0;
            start.h = best_h;
            start.f = start.h;
        }

        self.open_set.clear();
        self.open_set.insert(arena, start_idx);

        let mut count: i32 = 0;
        let max_nodes = (self.max_visited_nodes as f32 * max_visited_nodes_multiplier) as i32;
        let mut neighbors = [0_usize; 32];

        while !self.open_set.is_empty() {
            count += 1;
            if count >= max_nodes {
                break;
            }

            let current_idx = self.open_set.pop(arena);
            arena.get_mut(current_idx).closed = true;

            for (t_idx, (target, _)) in targets.iter_mut().enumerate() {
                if arena
                    .get(current_idx)
                    .distance_manhattan_pos(target.as_block_pos())
                    <= reach_range as f32
                {
                    target.set_reached();
                    reached_targets.push(t_idx);
                }
            }

            if !reached_targets.is_empty() {
                break;
            }

            if self.capture_debug {
                closed_indices.push(current_idx);
            }

            if !(arena.get(current_idx).distance_to(arena.get(start_idx)) >= max_path_distance) {
                let neighbor_count =
                    evaluator.get_neighbors(ctx, mob, arena, current_idx, &mut neighbors);

                for &neighbor_idx in &neighbors[..neighbor_count] {
                    let dist = arena.get(current_idx).distance_to(arena.get(neighbor_idx));
                    let current_walked = arena.get(current_idx).walked_distance;
                    let current_g = arena.get(current_idx).g;
                    let neighbor_malus = arena.get(neighbor_idx).cost_malus;

                    // Paper writes `walkedDistance` before the acceptance test, so a
                    // rejected open-set neighbour keeps the overwritten value.
                    let new_walked = current_walked + dist;
                    arena.get_mut(neighbor_idx).walked_distance = new_walked;
                    let new_g = current_g + dist + neighbor_malus;

                    let neighbor_in_open = arena.get(neighbor_idx).in_open_set();
                    let neighbor_g = arena.get(neighbor_idx).g;
                    if new_walked < max_path_distance && (!neighbor_in_open || new_g < neighbor_g) {
                        let h = Self::get_best_h(arena, neighbor_idx, targets) * Self::FUDGING;
                        let new_f = new_g + h;
                        {
                            let n = arena.get_mut(neighbor_idx);
                            n.came_from = Some(current_idx);
                            n.g = new_g;
                            n.h = h;
                        }
                        if neighbor_in_open {
                            self.open_set.change_cost(arena, neighbor_idx, new_f);
                        } else {
                            arena.get_mut(neighbor_idx).f = new_f;
                            self.open_set.insert(arena, neighbor_idx);
                        }
                    }
                }
            }
        }

        // Paper feature patch "Optimize-Pathfinder": when no target was reached,
        // all targets compete on node count alone; otherwise the reached targets
        // compete on `getDistToTarget` (`Double.compare`), then node count.
        let reached = !reached_targets.is_empty();
        let candidates: Vec<usize> = if reached {
            reached_targets
        } else {
            (0..targets.len()).collect()
        };
        let mut best_path: Option<Path> = None;
        for t_idx in candidates {
            let (target, pos) = &targets[t_idx];
            let Some(best_node_idx) = target.best_node() else {
                continue;
            };
            let candidate = Self::reconstruct_path(arena, best_node_idx, *pos, reached);
            let replace = match &best_path {
                None => true,
                Some(existing) => {
                    let by_count = candidate.get_node_count().cmp(&existing.get_node_count());
                    let ordering = if reached {
                        f64::from(candidate.get_dist_to_target())
                            .total_cmp(&f64::from(existing.get_dist_to_target()))
                            .then(by_count)
                    } else {
                        by_count
                    };
                    ordering == Ordering::Less
                }
            };
            if replace {
                best_path = Some(candidate);
            }
        }

        if let Some(ref mut path) = best_path
            && self.capture_debug
        {
            let open_nodes: Vec<Node> = self
                .open_set
                .as_slice()
                .iter()
                .map(|&idx| arena.get(idx).clone())
                .collect();
            let closed_nodes: Vec<Node> = closed_indices
                .iter()
                .map(|&idx| arena.get(idx).clone())
                .collect();
            let target_nodes: Vec<Target> = targets.iter().map(|(t, _)| t.clone()).collect();
            path.set_debug(open_nodes, closed_nodes, target_nodes);
        }

        best_path
    }

    fn get_best_h(arena: &NodeArena, node_idx: usize, targets: &mut [(Target, BlockPos)]) -> f32 {
        let node = arena.get(node_idx);
        let mut best_h = f32::MAX;
        for (target, _) in targets.iter_mut() {
            let h = target.distance_to_node(node);
            target.update_best(h, node_idx);
            best_h = rivet_util::mth::min_f32(best_h, h);
        }
        best_h
    }

    fn reconstruct_path(
        arena: &NodeArena,
        end_idx: usize,
        target: BlockPos,
        reached: bool,
    ) -> Path {
        let mut nodes = Vec::new();
        let mut cur = Some(end_idx);
        while let Some(idx) = cur {
            let node = arena.get(idx);
            nodes.push(node.clone());
            cur = node.came_from;
        }
        nodes.reverse();
        Path::new(nodes, target, reached)
    }
}

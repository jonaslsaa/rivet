//! Port of `net.minecraft.world.level.lighting.DynamicGraphMinFixedPoint`
//! (MC 26.2, Paper) — the fixed-point light propagation graph.
//!
//! Java: `DynamicGraphMinFixedPoint.java` in `working/Paper`. The abstract
//! base of the vanilla light engines' propagation: it owns a
//! [`LeveledPriorityQueue`], a node → computed-level map, and a `hasWork`
//! flag, and re-derives every node's light level through a min-fixed-point
//! loop. Paper 26.2 keeps it as the base of the vanilla
//! `SkyLightEngine`/`BlockLightEngine` (dead jar-surface under Starlight), and
//! — per issue #184 — the same graph primitives are live under Starlight's
//! `SectionTracker`/`ChunkTracker`, so the class is ported as its own unit.
//!
//! ## The abstract-method seam
//!
//! Java's base class and its subclass are one `this`: the base owns the queue/
//! `computedLevels`/`hasWork` fields and the subclass owns the node levels and
//! neighborhood. Rust cannot express inheritance, so the split is explicit:
//! [`DynamicGraphMinFixedPoint`] is a plain struct owning the base fields, and
//! [`DynamicGraphNode`] is the subclass seam. Every base method takes the
//! graph alongside `&mut self` — the caller owns both, mirroring Java's single
//! `this`. `check_neighbors_after_update` receives the base back so the
//! subclass can call [`check_neighbor`](Self::check_neighbor), exactly as the
//! Java subclass calls `this.checkNeighbor`.
//!
//! `computedLevels` is fastutil's `Long2ByteOpenHashMap` with `minMapSize`
//! no-shrink and `defaultReturnValue((byte)-1)`; the Rust `HashMap` never
//! shrinks on remove and the port treats absence as the byte `255`
//! (`NO_COMPUTED_LEVEL`), so both constructors' capacity hints are accepted
//! for signature fidelity and documented as no-ops. `hasWork` is `volatile`
//! in Java (the light threads); the port is single-owner tick-thread state
//! (OWNERSHIP.md), so it is a plain `bool` refreshed after every mutator that
//! touches the queue — including the exact Java subtlety that `check_neighbor`
//! (the protected neighbor hook) does NOT refresh it.
//!
//! RivetTodo(#184): this is the `mc.world.level.lighting.core` graph unit.
//! The concrete engines that extend it (vanilla `LightEngine` subclasses and
//! Starlight's `SectionTracker`/`ChunkTracker`) defer with their owning units.

use std::collections::HashMap;

use rivet_util::mth;

use crate::lighting::leveled_priority_queue::LeveledPriorityQueue;

/// `DynamicGraphMinFixedPoint.SOURCE` — `Long.MAX_VALUE`, the sentinel for
/// "no source".
pub const SOURCE: i64 = i64::MAX;

/// `NO_COMPUTED_LEVEL` — the byte-level `255` fastutil returns for a node
/// with no computed level (`defaultReturnValue((byte)-1) & 255`).
const NO_COMPUTED_LEVEL: i32 = 255;

/// The subclass seam of `DynamicGraphMinFixedPoint` — the node levels and
/// neighborhood logic the base class's propagation drives.
pub trait DynamicGraphNode {
    /// `getComputedLevel(long node, long knownParent, int knownLevelFromParent)`.
    fn get_computed_level(&self, node: i64, known_parent: i64, known_level_from_parent: i32)
    -> i32;

    /// `checkNeighborsAfterUpdate(long node, int level, boolean onlyDecrease)`
    /// — called after `node`'s level changes; the impl re-checks each neighbor
    /// through [`DynamicGraphMinFixedPoint::check_neighbor`].
    fn check_neighbors_after_update(
        &mut self,
        base: &mut DynamicGraphMinFixedPoint,
        node: i64,
        level: i32,
        only_decrease: bool,
    );

    /// `getLevel(long node)`.
    fn get_level(&self, node: i64) -> i32;

    /// `setLevel(long node, int level)`.
    fn set_level(&mut self, node: i64, level: i32);

    /// `computeLevelFromNeighbor(long from, long to, int fromLevel)`.
    fn compute_level_from_neighbor(&self, from: i64, to: i64, from_level: i32) -> i32;
}

/// `net.minecraft.world.level.lighting.DynamicGraphMinFixedPoint` — the base
/// state (queue, computed levels, work flag) plus the fixed-point loop.
pub struct DynamicGraphMinFixedPoint {
    /// `levelCount` — the priority bucket count (Java enforces `< 254`).
    level_count: i32,
    /// `priorityQueue`.
    priority_queue: LeveledPriorityQueue,
    /// `computedLevels` — node → pending computed level (absent = 255).
    computed_levels: HashMap<i64, u8>,
    /// `hasWork` — whether the queue holds pending nodes.
    has_work: bool,
}

impl DynamicGraphMinFixedPoint {
    /// `DynamicGraphMinFixedPoint(int levelCount, int minQueueSize, int
    /// minMapSize)` — Java throws `IllegalArgumentException` for
    /// `levelCount >= 254`; the port panics with the same message.
    pub fn new(level_count: i32, min_queue_size: usize, _min_map_size: usize) -> Self {
        assert!(level_count < 254, "Level count must be < 254.");
        DynamicGraphMinFixedPoint {
            level_count,
            priority_queue: LeveledPriorityQueue::new(level_count as usize, min_queue_size),
            computed_levels: HashMap::new(),
            has_work: false,
        }
    }

    /// `isSource(long node)` — `node == Long.MAX_VALUE`.
    pub fn is_source(node: i64) -> bool {
        node == SOURCE
    }

    /// `calculatePriority(int level, int computedLevel)` — `min(level,
    /// computedLevel, levelCount - 1)`.
    fn calculate_priority(level: i32, computed_level: i32, level_count: i32) -> i32 {
        level.min(computed_level).min(level_count - 1)
    }

    /// `computedLevels.get(node) & 255` — `255` when the node has no pending
    /// computed level.
    fn get_computed(&self, node: i64) -> i32 {
        self.computed_levels
            .get(&node)
            .map_or(NO_COMPUTED_LEVEL, |&v| v as i32)
    }

    /// `removeFromQueue(long node)` — drop `node` from the queue and its
    /// computed level; refresh `hasWork`.
    pub fn remove_from_queue<G: DynamicGraphNode>(&mut self, graph: &mut G, node: i64) {
        let computed_level = self
            .computed_levels
            .remove(&node)
            .map_or(NO_COMPUTED_LEVEL, |v| v as i32);
        if computed_level != NO_COMPUTED_LEVEL {
            let level = graph.get_level(node);
            let priority = Self::calculate_priority(level, computed_level, self.level_count);
            self.priority_queue
                .dequeue(node, priority as usize, self.level_count as usize);
            self.has_work = !self.priority_queue.is_empty();
        }
    }

    /// `removeIf(LongPredicate)` — remove every queued node matching `pred`.
    /// Java collects into a `LongArrayList` first (iteration over the map's
    /// key set, hash order — order does not affect the outcome); the port
    /// collects a `Vec`.
    pub fn remove_if<G: DynamicGraphNode>(&mut self, graph: &mut G, pred: impl Fn(i64) -> bool) {
        let nodes_to_remove: Vec<i64> = self
            .computed_levels
            .keys()
            .copied()
            .filter(|&node| pred(node))
            .collect();
        for node in nodes_to_remove {
            self.remove_from_queue(graph, node);
        }
    }

    /// `checkNode(long node)` — `checkEdge(node, node, levelCount - 1, false)`.
    pub fn check_node<G: DynamicGraphNode>(&mut self, graph: &mut G, node: i64) {
        self.check_edge(graph, node, node, self.level_count - 1, false);
    }

    /// `checkEdge(long from, long to, int newLevelFrom, boolean
    /// onlyDecreased)` — the public entry; refreshes `hasWork` afterwards.
    pub fn check_edge<G: DynamicGraphNode>(
        &mut self,
        graph: &mut G,
        from: i64,
        to: i64,
        new_level_from: i32,
        only_decreased: bool,
    ) {
        self.check_edge_inner(
            graph,
            from,
            to,
            new_level_from,
            graph.get_level(to),
            self.get_computed(to),
            only_decreased,
        );
        self.has_work = !self.priority_queue.is_empty();
    }

    /// The private 6-argument `checkEdge` — the shared propagation step,
    /// without the `hasWork` refresh (Java's `checkNeighbor` calls this form
    /// and does NOT touch `hasWork`).
    #[allow(clippy::too_many_arguments)] // mirrors the 6-arg Java `checkEdge` + graph/base receiver
    fn check_edge_inner<G: DynamicGraphNode>(
        &mut self,
        graph: &mut G,
        from: i64,
        to: i64,
        mut new_level_from: i32,
        mut level_to: i32,
        mut old_computed_level: i32,
        only_decreased: bool,
    ) {
        if !Self::is_source(to) {
            new_level_from = mth::clamp(new_level_from, 0, self.level_count - 1);
            level_to = mth::clamp(level_to, 0, self.level_count - 1);
            let was_consistent = old_computed_level == NO_COMPUTED_LEVEL;
            if was_consistent {
                old_computed_level = level_to;
            }
            let new_computed_level = if only_decreased {
                old_computed_level.min(new_level_from)
            } else {
                mth::clamp(
                    graph.get_computed_level(to, from, new_level_from),
                    0,
                    self.level_count - 1,
                )
            };
            let old_priority =
                Self::calculate_priority(level_to, old_computed_level, self.level_count);
            if level_to != new_computed_level {
                let new_priority =
                    Self::calculate_priority(level_to, new_computed_level, self.level_count);
                if old_priority != new_priority && !was_consistent {
                    self.priority_queue
                        .dequeue(to, old_priority as usize, new_priority as usize);
                }
                self.priority_queue.enqueue(to, new_priority as usize);
                self.computed_levels.insert(to, new_computed_level as u8);
            } else if !was_consistent {
                self.priority_queue
                    .dequeue(to, old_priority as usize, self.level_count as usize);
                self.computed_levels.remove(&to);
            }
        }
    }

    /// `checkNeighbor(long from, long to, int level, boolean onlyDecreased)` —
    /// the hook the subclass's `check_neighbors_after_update` drives. Java does
    /// NOT refresh `hasWork` here; the port preserves that.
    pub fn check_neighbor<G: DynamicGraphNode>(
        &mut self,
        graph: &mut G,
        from: i64,
        to: i64,
        level: i32,
        only_decreased: bool,
    ) {
        let stored_old_computed_level = self.get_computed(to);
        let level_from = mth::clamp(
            graph.compute_level_from_neighbor(from, to, level),
            0,
            self.level_count - 1,
        );
        if only_decreased {
            self.check_edge_inner(
                graph,
                from,
                to,
                level_from,
                graph.get_level(to),
                stored_old_computed_level,
                only_decreased,
            );
        } else {
            let was_consistent = stored_old_computed_level == NO_COMPUTED_LEVEL;
            let old_computed_level = if was_consistent {
                mth::clamp(graph.get_level(to), 0, self.level_count - 1)
            } else {
                stored_old_computed_level
            };
            if level_from == old_computed_level {
                let level_to = if was_consistent {
                    old_computed_level
                } else {
                    graph.get_level(to)
                };
                self.check_edge_inner(
                    graph,
                    from,
                    to,
                    self.level_count - 1,
                    level_to,
                    stored_old_computed_level,
                    only_decreased,
                );
            }
        }
    }

    /// `hasWork()`.
    pub fn has_work(&self) -> bool {
        self.has_work
    }

    /// `runUpdates(int count)` — drain the queue until `count` updates are run
    /// or the queue empties; returns the leftover budget.
    pub fn run_updates<G: DynamicGraphNode>(&mut self, graph: &mut G, mut count: i32) -> i32 {
        if self.priority_queue.is_empty() {
            return count;
        }
        while !self.priority_queue.is_empty() && count > 0 {
            count -= 1;
            let node = self.priority_queue.remove_first_long();
            let level = mth::clamp(graph.get_level(node), 0, self.level_count - 1);
            let computed_level = self
                .computed_levels
                .remove(&node)
                .map_or(NO_COMPUTED_LEVEL, |v| v as i32);
            if computed_level < level {
                graph.set_level(node, computed_level);
                graph.check_neighbors_after_update(self, node, computed_level, true);
            } else if computed_level > level {
                graph.set_level(node, self.level_count - 1);
                if computed_level != self.level_count - 1 {
                    let priority = Self::calculate_priority(
                        self.level_count - 1,
                        computed_level,
                        self.level_count,
                    );
                    self.priority_queue.enqueue(node, priority as usize);
                    self.computed_levels.insert(node, computed_level as u8);
                }
                graph.check_neighbors_after_update(self, node, level, false);
            }
        }
        self.has_work = !self.priority_queue.is_empty();
        count
    }

    /// `getQueueSize()` — the number of nodes with a pending computed level
    /// (Java returns `computedLevels.size()`, not the queue's element count).
    pub fn get_queue_size(&self) -> usize {
        self.computed_levels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line graph of `size` nodes: each node's neighbors are `i-1`/`i+1`
    /// within bounds, and a node adopts its parent's level unchanged (the
    /// propagation shape of a light flood on a path).
    struct LineGraph {
        levels: Vec<i32>,
    }

    impl LineGraph {
        fn new(size: usize) -> Self {
            LineGraph {
                levels: vec![0; size],
            }
        }
    }

    impl DynamicGraphNode for LineGraph {
        fn get_computed_level(
            &self,
            _node: i64,
            _known_parent: i64,
            known_level_from_parent: i32,
        ) -> i32 {
            known_level_from_parent
        }

        fn check_neighbors_after_update(
            &mut self,
            base: &mut DynamicGraphMinFixedPoint,
            node: i64,
            level: i32,
            only_decrease: bool,
        ) {
            let n = node;
            if n > 0 {
                base.check_neighbor(self, n, n - 1, level, only_decrease);
            }
            if n + 1 < self.levels.len() as i64 {
                base.check_neighbor(self, n, n + 1, level, only_decrease);
            }
        }

        fn get_level(&self, node: i64) -> i32 {
            self.levels[node as usize]
        }

        fn set_level(&mut self, node: i64, level: i32) {
            self.levels[node as usize] = level;
        }

        fn compute_level_from_neighbor(&self, _from: i64, _to: i64, from_level: i32) -> i32 {
            from_level
        }
    }

    /// The increase flood golden from the Paper oracle: raising node 0 to the
    /// top bucket floods the whole line, one node per update, and the returned
    /// count is the leftover budget.
    #[test]
    fn increase_floods_the_line_from_the_source() {
        let mut graph = LineGraph::new(9);
        let mut base = DynamicGraphMinFixedPoint::new(16, 4, 64);
        assert!(!base.has_work());
        assert_eq!(base.get_queue_size(), 0);
        graph.set_level(0, 15);
        base.check_node(&mut graph, 0);
        // Node 0 is already at the top level: checkNode queues nothing.
        assert!(!base.has_work());
        assert_eq!(base.get_queue_size(), 0);
        base.check_edge(&mut graph, 0, 1, 15, false);
        assert_eq!(base.get_queue_size(), 1);
        // 8 queued nodes (1..=8) process, one update each.
        assert_eq!(base.run_updates(&mut graph, 100), 92);
        assert!(!base.has_work());
        for i in 0..9 {
            assert_eq!(graph.levels[i], 15, "node {i} level");
        }
    }

    /// The decrease golden from the Paper oracle: lowering node 0 re-floods
    /// the line downward through `onlyDecreased`, and the fixpoint recomputes
    /// every level.
    #[test]
    fn decrease_floods_the_line_back_down() {
        let mut graph = LineGraph::new(9);
        let mut base = DynamicGraphMinFixedPoint::new(16, 4, 64);
        graph.set_level(0, 15);
        for i in 1..9 {
            graph.set_level(i, 15);
        }
        // Lower node 0 and propagate the decrease.
        base.check_edge(&mut graph, 0, 0, 3, true);
        assert!(base.has_work());
        // Nodes 0..=8 all requeue (9 updates).
        assert_eq!(base.run_updates(&mut graph, 100), 91);
        assert!(!base.has_work());
        for i in 0..9 {
            assert_eq!(graph.levels[i], 3, "node {i} level");
        }
    }

    /// `removeIf` and `removeFromQueue` drop pending nodes without processing
    /// them, and `hasWork` tracks the queue after each mutation.
    #[test]
    fn remove_if_drops_pending_work() {
        let mut graph = LineGraph::new(4);
        let mut base = DynamicGraphMinFixedPoint::new(16, 4, 64);
        graph.set_level(0, 15);
        base.check_edge(&mut graph, 0, 1, 15, false);
        base.check_edge(&mut graph, 0, 2, 15, false);
        assert_eq!(base.get_queue_size(), 2);
        // Removing node 1's pending work leaves node 2 queued.
        base.remove_from_queue(&mut graph, 1);
        assert_eq!(base.get_queue_size(), 1);
        assert!(base.has_work());
        base.remove_if(&mut graph, |node| node == 2);
        assert_eq!(base.get_queue_size(), 0);
        assert!(!base.has_work());
        // Nothing was processed: levels are unchanged.
        assert_eq!(graph.levels, vec![15, 0, 0, 0]);
    }

    #[test]
    fn constructor_rejects_level_count_at_least_254() {
        assert!(std::panic::catch_unwind(|| DynamicGraphMinFixedPoint::new(254, 4, 64)).is_err());
        assert!(std::panic::catch_unwind(|| DynamicGraphMinFixedPoint::new(255, 4, 64)).is_err());
        assert!(std::panic::catch_unwind(|| DynamicGraphMinFixedPoint::new(253, 4, 64)).is_ok());
    }
}

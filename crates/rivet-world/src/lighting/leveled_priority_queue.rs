//! Port of `net.minecraft.world.level.lighting.LeveledPriorityQueue`
//! (MC 26.2, Paper) — the bucket-queue at the heart of the light graph's
//! min-fixed-point propagation.
//!
//! Java: `LeveledPriorityQueue.java` in `working/Paper`. One `long`-keyed
//! FIFO per level; `removeFirstLong` pops the lowest non-empty level first.
//! Each per-level queue is fastutil's `LongLinkedOpenHashSet` (insertion-
//! ordered, dedup), constructed with a minimum size so it never shrinks below
//! the constructor's `minSize` (the anonymous-subclass `rehash` override
//! suppresses shrinks).
//!
//! The port preserves Java's exact ordering semantics: within a level the
//! queue is FIFO (dedup on re-insert, earliest-inserted first out), and
//! `firstQueuedLevel` tracks the lowest occupied level. `dequeue` takes an
//! `upperBound` so `checkFirstQueuedLevel` re-scans upward only as far as
//! Java's callers bound it (Java passes `newPriority` on re-prioritize and
//! `levelCount` on remove).
//!
//! RivetTodo(#184): this is the `mc.world.level.lighting.core` queue unit.
//! No `hasWork` is stored here — `DynamicGraphMinFixedPoint` derives it from
//! `isEmpty()` after every mutator.
//!
//! Known non-blocking note: `enqueue`/`dequeue` are O(level size) per op
//! (`VecDeque::contains`/`retain`), where Java's `LongLinkedOpenHashSet` is
//! O(1). This is acceptable for now: the queue has no consumer yet (the
//! `SectionTracker`/`ChunkTracker` engines that drive it defer with the
//! `starlight.light` unit), and an O(1) order-preserving dedup would need an
//! indexed structure beyond the current scope. Revisit alongside that engine
//! port, not before.

use std::collections::VecDeque;

/// `net.minecraft.world.level.lighting.LeveledPriorityQueue`.
///
/// One `VecDeque` (Rust's stand-in for fastutil's insertion-ordered linked
/// hash set — insertion order is all the graph's FIFO semantics require, and
/// `enqueue` dedups exactly like the set's `add`, which keeps a duplicate's
/// original FIFO position: the graph's re-prioritize path can re-enqueue a
/// node already on the same priority level). Every entry is exactly one long.
pub struct LeveledPriorityQueue {
    /// `levelCount` — the number of priority buckets.
    level_count: usize,
    /// `queues` — one FIFO per level, indexed by priority.
    queues: Vec<VecDeque<i64>>,
    /// `firstQueuedLevel` — the lowest occupied level, or `levelCount` when
    /// empty (Java initializes it to `levelCount`).
    first_queued_level: usize,
}

impl LeveledPriorityQueue {
    /// `LeveledPriorityQueue(int levelCount, int minSize)` — Java's anonymous
    /// `rehash` override makes each bucket a no-shrink set. Rust `VecDeque`
    /// never shrinks on its own, so `min_size` needs no storage: it is
    /// documented but not retained (the deque never drops below it).
    pub fn new(level_count: usize, _min_size: usize) -> Self {
        LeveledPriorityQueue {
            level_count,
            queues: (0..level_count).map(|_| VecDeque::new()).collect(),
            first_queued_level: level_count,
        }
    }

    /// `removeFirstLong()` — pop from the lowest non-empty level. Java throws
    /// `NoSuchElementException` on an empty queue; the port panics with the
    /// same message (indexing would otherwise hit a generic out-of-bounds
    /// panic first).
    pub fn remove_first_long(&mut self) -> i64 {
        assert!(
            self.first_queued_level < self.level_count,
            "removeFirstLong called on empty LeveledPriorityQueue"
        );
        let result = self.queues[self.first_queued_level]
            .pop_front()
            .expect("first_queued_level marks an occupied level after the guard");
        if self.queues[self.first_queued_level].is_empty() {
            self.check_first_queued_level(self.level_count);
        }
        result
    }

    /// `isEmpty()` — `firstQueuedLevel >= levelCount`.
    pub fn is_empty(&self) -> bool {
        self.first_queued_level >= self.level_count
    }

    /// `dequeue(long node, int key, int upperBound)` — remove `node` from
    /// level `key`; if that level empties and it was the lowest, rescan upward
    /// to `upperBound` (Java bounds the scan so a re-prioritized node never
    /// makes an already-removed lower level win).
    pub fn dequeue(&mut self, node: i64, key: usize, upper_bound: usize) {
        let queue = &mut self.queues[key];
        queue.retain(|&n| n != node);
        if queue.is_empty() && self.first_queued_level == key {
            self.check_first_queued_level(upper_bound);
        }
    }

    /// `enqueue(long node, int key)` — push `node` onto level `key`; lower
    /// `firstQueuedLevel` if this is now the lowest occupied level.
    ///
    /// Java's `LongLinkedOpenHashSet.add` is a *set* add: a node already on the
    /// level is a no-op that keeps its original FIFO position. The graph's
    /// re-prioritize path can enqueue a node that is already queued at the same
    /// priority (`checkEdge` when `oldPriority == newPriority` with a changed
    /// computed level), so the port must dedup like the set, not append a
    /// duplicate (a duplicate would process the node twice).
    pub fn enqueue(&mut self, node: i64, key: usize) {
        let queue = &mut self.queues[key];
        if !queue.contains(&node) {
            queue.push_back(node);
        }
        if self.first_queued_level > key {
            self.first_queued_level = key;
        }
    }

    /// `checkFirstQueuedLevel(int upperBound)` — scan upward from the old
    /// lowest level (exclusive) for the next non-empty level below
    /// `upperBound`; `firstQueuedLevel` becomes `upperBound` when none exists.
    fn check_first_queued_level(&mut self, upper_bound: usize) {
        let old_level = self.first_queued_level;
        self.first_queued_level = upper_bound;
        for i in (old_level + 1)..upper_bound {
            if !self.queues[i].is_empty() {
                self.first_queued_level = i;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_lowest_nonempty_level_first_fifo_within_level() {
        // The `probeLeveledPriorityQueue` sequence from the Paper oracle trace:
        // `queue.empty.initial=true`, pops 12 (level 0), 15 (level 1), 11
        // (level 2 FIFO), a re-prioritize dequeue with upperBound=0, then 14, 13,
        // and `queue.empty.final=true`.
        let mut q = LeveledPriorityQueue::new(6, 4);
        assert!(q.is_empty());
        q.enqueue(11, 2);
        q.enqueue(12, 0);
        q.enqueue(13, 2);
        q.enqueue(14, 3);
        q.enqueue(15, 1);
        assert!(!q.is_empty());
        assert_eq!(q.remove_first_long(), 12);
        assert_eq!(q.remove_first_long(), 15);
        assert_eq!(q.remove_first_long(), 11);
        // Re-prioritize 14 from level 3 to level 0: dequeue with upperBound =
        // newPriority 0 — an emptied level below the bound must not re-claim the
        // lead. Level 2 still holds 13, so the queue is not empty.
        q.dequeue(14, 3, 0);
        assert!(!q.is_empty());
        q.enqueue(14, 0);
        assert_eq!(q.remove_first_long(), 14);
        assert_eq!(q.remove_first_long(), 13);
        assert!(q.is_empty());
    }

    #[test]
    fn duplicate_enqueue_keeps_original_fifo_position() {
        // The `queue.dedup.*` goldens from the Paper oracle: `LongLinkedOpenHashSet`
        // is a set, so a duplicate add is a no-op that keeps the original
        // position (1 pops before 2).
        let mut q = LeveledPriorityQueue::new(4, 4);
        q.enqueue(1, 1);
        q.enqueue(2, 1);
        q.enqueue(1, 1);
        assert_eq!(q.remove_first_long(), 1);
        assert_eq!(q.remove_first_long(), 2);
        assert!(q.is_empty());
        // A duplicate add to the lowest level does not re-claim the lead either:
        // `queue.lowest.dupe.empty=true` (the sole node is dequeued and the
        // queue empties).
        let mut q = LeveledPriorityQueue::new(4, 4);
        q.enqueue(5, 2);
        q.enqueue(5, 2);
        q.dequeue(5, 2, 4);
        assert!(q.is_empty());
    }

    #[test]
    fn dequeue_empties_and_rescans_upward() {
        let mut q = LeveledPriorityQueue::new(4, 8);
        q.enqueue(31, 1);
        q.enqueue(32, 1);
        // Removing the only node on the lowest level rescans to level 2.
        q.dequeue(31, 1, 3);
        assert_eq!(q.remove_first_long(), 32);
        assert!(q.is_empty());
        // Remove the sole node on level 2 with upperBound = levelCount.
        q.enqueue(41, 2);
        q.dequeue(41, 2, 4);
        assert!(q.is_empty());
    }

    #[test]
    fn dequeue_never_rescans_past_upper_bound() {
        // `checkEdge`'s re-prioritize calls dequeue with upperBound =
        // newPriority: an emptied level below it must not re-claim the lead.
        let mut q = LeveledPriorityQueue::new(6, 8);
        q.enqueue(10, 0);
        q.enqueue(20, 2);
        q.enqueue(30, 4);
        // Drain level 0; upper bound 2 means only levels 1 is scanned, leaving
        // firstQueuedLevel = 2 even though level 4 also holds a node.
        q.dequeue(10, 0, 2);
        assert_eq!(q.remove_first_long(), 20);
        assert_eq!(q.remove_first_long(), 30);
        assert!(q.is_empty());
    }
}

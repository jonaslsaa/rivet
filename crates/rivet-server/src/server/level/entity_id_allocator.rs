//! `ServerLevel.ENTITY_COUNTER` + `getNextEntityId()` — Paper's entity-id
//! allocation (GitHub #222 residual).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! server/level/ServerLevel.java`:
//!
//! ```java
//! private static final AtomicInteger ENTITY_COUNTER = new AtomicInteger();
//!
//! @Override
//! public int getNextEntityId() {
//!     int id = 0;
//!     while (id == 0 || this.chunkSource.hasEntityWithId(id)) {
//!         id = ENTITY_COUNTER.incrementAndGet();
//!     }
//!     return id;
//! }
//! ```
//!
//! Ported semantics:
//! - The counter starts at 0 and is incremented *before* reading
//!   (`incrementAndGet`), so the first allocation is 1: `0` is never a valid
//!   entity id (the `id == 0` guard). The increment wraps like Java's int
//!   (`AtomicInteger.incrementAndGet` on `i32::MAX` yields `i32::MIN`, never a
//!   Rust overflow panic).
//! - A candidate id already held by a live entity is skipped
//!   (`chunkSource.hasEntityWithId`), so allocation stays distinct even if the
//!   counter wraps or pre-existing entities hold ids at/above it (e.g. entities
//!   loaded from disk). The M1 model has no entity store yet (the entity unit is
//!   deferred, RivetTodo(#222)); the in-use set is the ids of entities currently
//!   present in the level — the players, in this slice — registered via
//!   [`mark_in_use`](EntityIdAllocator::mark_in_use) and released on removal.
//! - The counter is not reset between allocations (Java's `static` field), so a
//!   join/leave/rejoin cycle yields fresh ids rather than reusing freed ones.
//!
//! Ownership: `ENTITY_COUNTER` is `static` — ONE counter shared by every
//! `ServerLevel` in the JVM, so entity ids stay unique across dimensions and
//! world instances. The per-level `hasEntityWithId` guard only re-checks the
//! level's own entities as a loaded-entity / wrap-around safety net; the
//! counter itself is what guarantees cross-level uniqueness. Rivet therefore
//! keeps the allocator at the tick-thread-confined server play scope — the
//! [`PlayerSessionManager`](crate::server::player::session::PlayerSessionManager),
//! which owns the world — NOT on the `ServerLevel`. A per-level allocator that
//! restarted at 1 for each level would hand different levels the same ids. The
//! session spawn allocates before the join burst and marks the id in use only
//! after the burst succeeds: a burst failure consumes the id (the `Entity`
//! constructor already ran `getNextEntityId()`) without registering it, exactly
//! as Paper never rolls the counter back.

use std::collections::HashSet;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};

/// One server-wide counter, shared by player and generated-entity constructors.
/// Occupancy remains a query of the destination level, as in Paper.
#[derive(Clone, Debug, Default)]
pub struct EntityIdSequence(Arc<AtomicI32>);

/// Live IDs in the destination level. Clones observe admission/removal from
/// the same store; saved ProtoChunk entities are not live tracked entities.
#[derive(Clone, Debug, Default)]
pub struct EntityIdOccupancy(Arc<RwLock<HashSet<i32>>>);

impl EntityIdOccupancy {
    pub fn contains(&self, id: i32) -> bool {
        self.0
            .read()
            .expect("entity ID occupancy poisoned")
            .contains(&id)
    }
}

impl EntityIdSequence {
    pub fn next_id(&self, mut is_in_use: impl FnMut(i32) -> bool) -> i32 {
        loop {
            let id = self.0.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
            if id != 0 && !is_in_use(id) {
                return id;
            }
        }
    }
}

/// The server's entity-id allocator — `ENTITY_COUNTER` plus the
/// `chunkSource.hasEntityWithId` guard, owned at the tick-thread server play
/// scope (the session manager), NOT the level: Java's `ENTITY_COUNTER` is
/// `static`, shared across every `ServerLevel`.
#[derive(Debug)]
pub struct EntityIdAllocator {
    /// `ENTITY_COUNTER` — the last incremented value (the high-water mark;
    /// never decremented, so freed ids are never reused).
    next: EntityIdSequence,
    /// The ids of entities currently present in the world (the M1 players) —
    /// `chunkSource.hasEntityWithId` per level, tracked here because the M1
    /// world is a single level.
    in_use: EntityIdOccupancy,
}

impl EntityIdAllocator {
    /// `ENTITY_COUNTER = new AtomicInteger()` — the default (0), so the first
    /// allocation returns 1.
    pub fn new() -> Self {
        EntityIdAllocator {
            next: EntityIdSequence::default(),
            in_use: EntityIdOccupancy::default(),
        }
    }

    /// `ServerLevel.getNextEntityId()` — the skip-zero / skip-in-use loop.
    /// Returns a non-zero id not held by any live entity, advancing the counter
    /// (`AtomicInteger.incrementAndGet`, wrapping).
    pub fn next_id(&mut self) -> i32 {
        self.next.next_id(|id| self.in_use.contains(id))
    }

    /// Share the existing server counter; do not create a per-world counter.
    pub fn sequence(&self) -> EntityIdSequence {
        self.next.clone()
    }

    pub fn occupancy(&self) -> EntityIdOccupancy {
        self.in_use.clone()
    }

    /// Register `id` as held by a live entity (`hasEntityWithId` becomes true).
    /// Called when an entity spawns into the world.
    pub fn mark_in_use(&mut self, id: i32) {
        self.in_use
            .0
            .write()
            .expect("entity ID occupancy poisoned")
            .insert(id);
    }

    /// Release `id` (the entity left the world; `hasEntityWithId` becomes
    /// false). The counter is untouched — freed ids are not reused.
    pub fn release(&mut self, id: i32) {
        self.in_use
            .0
            .write()
            .expect("entity ID occupancy poisoned")
            .remove(&id);
    }
}

impl Default for EntityIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An allocator seeded at a specific counter value (the wrap tests).
    fn at(next: i32) -> EntityIdAllocator {
        EntityIdAllocator {
            next: EntityIdSequence(Arc::new(AtomicI32::new(next))),
            in_use: EntityIdOccupancy::default(),
        }
    }

    #[test]
    fn first_allocation_is_one_not_zero() {
        // `id = ENTITY_COUNTER.incrementAndGet()` with the counter at 0 → 1;
        // the `id == 0` guard means 0 is never returned.
        let mut a = EntityIdAllocator::new();
        assert_eq!(a.next_id(), 1);
    }

    #[test]
    fn generated_and_player_constructors_share_counter_but_query_own_level() {
        let mut players = EntityIdAllocator::new();
        let generated = players.sequence();
        assert_eq!(players.next_id(), 1);
        assert_eq!(generated.next_id(|id| id == 2), 3);
        assert_eq!(players.next_id(), 4);
        let wrapped = at(-1);
        let mut queried = Vec::new();
        assert_eq!(
            wrapped.sequence().next_id(|id| {
                queried.push(id);
                false
            }),
            1
        );
        assert_eq!(queried, vec![1], "zero is rejected before querying storage");
    }

    #[test]
    fn allocations_are_distinct_and_monotonic() {
        let mut a = EntityIdAllocator::new();
        assert_eq!(a.next_id(), 1);
        assert_eq!(a.next_id(), 2);
        assert_eq!(a.next_id(), 3);
    }

    #[test]
    fn in_use_ids_are_skipped() {
        // `hasEntityWithId`: an id held by a live entity is skipped. Pre-seed
        // id 3 as in use → 1, 2, then 3 is skipped → 4.
        let mut a = EntityIdAllocator::new();
        let destination = a.occupancy();
        a.mark_in_use(3);
        assert!(destination.contains(3));
        assert_eq!(a.next_id(), 1);
        assert_eq!(a.next_id(), 2);
        assert_eq!(a.next_id(), 4, "in-use id 3 is skipped");
        a.release(3);
        assert!(!destination.contains(3));
    }

    #[test]
    fn released_ids_are_not_reused() {
        // Releasing frees the id for `hasEntityWithId`, but the counter never
        // decrements, so a join/leave/join cycle yields a fresh id, not the
        // freed one — exactly Paper.
        let mut a = EntityIdAllocator::new();
        let one = a.next_id();
        a.mark_in_use(one);
        a.release(one);
        assert_eq!(a.next_id(), 2, "freed id 1 is not reused");
    }

    #[test]
    fn counter_wraps_like_java_not_panics() {
        // `AtomicInteger.incrementAndGet` wraps: i32::MAX + 1 == i32::MIN, a
        // valid nonzero entity id. The loop must not overflow-panic.
        let mut a = at(i32::MAX);
        assert_eq!(a.next_id(), i32::MIN);
    }

    #[test]
    fn zero_after_wrap_is_skipped() {
        // If the increment lands exactly on 0, the `id == 0` guard keeps going.
        let mut a = at(-1); // -1 + 1 == 0 → skipped → next is 1
        assert_eq!(a.next_id(), 1);
    }

    #[test]
    fn wrapped_counter_skips_still_live_ids() {
        // After a wrap the counter cycles back over ids held by live entities;
        // `hasEntityWithId` skips them. Here an entity allocated near the top
        // of the range is still live when the counter wraps past i32::MAX and
        // re-encounters it.
        let mut a = at(i32::MAX - 1);
        a.mark_in_use(i32::MAX);
        assert_eq!(
            a.next_id(),
            i32::MIN,
            "the wrapped counter skips the still-live i32::MAX"
        );
    }
}

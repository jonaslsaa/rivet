//! Port of `net.minecraft.world.ticks` (MC 26.2, #370/#522) — the value and
//! container layer.
//!
//! Java source: `working/Paper/.../net/minecraft/world/ticks/`. This slice
//! ports the serializable value surface that a stored chunk's `block_ticks`/
//! `fluid_ticks` lists and `UpgradeData`'s neighbor-tick lists decode into,
//! plus the runtime containers that hold ticks for a loaded chunk:
//!
//! - [`TickPriority`] (`TickPriority.java`) — the enum + `CODEC` (an int
//!   xmap with Java's clamp-on-out-of-range fallback).
//! - [`SavedTick`] (`SavedTick.java`) — the `record SavedTick<T>(T type,
//!   BlockPos pos, int delay, TickPriority priority)` value type, its faithful
//!   [`saved_tick_codec`] codec factory (fields `i`/`x`/`y`/`z`/`t`/`p`), and
//!   [`filter_tick_list_for_chunk`] (the per-chunk filter `filterTickListForChunk`).
//! - [`ScheduledTick`] (`ScheduledTick.java`) — the runtime
//!   `record ScheduledTick<T>(T type, BlockPos pos, long triggerTick,
//!   TickPriority priority, long subTickOrder)` with the `DRAIN_ORDER` /
//!   `INTRA_TICK_DRAIN_ORDER` / `SUB_TICK_ORDERING` comparators and the
//!   [`UniqueTickKey`] projection of `UNIQUE_TICK_HASH`.
//! - [`LevelChunkTicks`] (`LevelChunkTicks.java`) — the runtime chunk's tick
//!   container: a `java.util.PriorityQueue`-layout min-heap (see the private
//!   [`TickQueue`], which replicates the siftUp/siftDown/removeAt array order),
//!   the pending stored-ticks list, the per-position uniqueness set, the
//!   `onTickAdded` hook, and the Moonrise `moonrise$isDirty`/`moonrise$clearDirty`
//!   dirty surface (`ChunkSystemLevelChunkTicks`).
//! - [`ProtoChunkTicks`] (`ProtoChunkTicks.java`) — the worldgen/loading-stage
//!   container holding stored (relative-delay) ticks.
//! - [`TickAccess`]/[`TickContainerAccess`]/[`SerializableTickContainer`] — the
//!   interface chain these containers implement.
//!
//! The level-level scheduling surface (`LevelTicks`, `LevelTickAccess`,
//! `WorldGenTickAccess`, `BlackholeTickAccess`, and the `ScheduledTick`
//! execution/`willTickThisTick` machinery) stays deferred — RivetTodo below.
//! Nothing here schedules or executes; the containers only store, deduplicate,
//! pack, and unpack.

use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// `net.minecraft.world.ticks.TickPriority` — the scheduling-priority enum.
///
/// Values match the Java ordinal order (`EXTREMELY_HIGH(-3)` first), which is
/// the wire form the `CODEC` uses (`getValue` returns the raw value; decode is
/// `Codec.INT.xmap(TickPriority::byValue, TickPriority::getValue)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TickPriority {
    /// `EXTREMELY_HIGH(-3)`.
    ExtremelyHigh,
    /// `VERY_HIGH(-2)`.
    VeryHigh,
    /// `HIGH(-1)`.
    High,
    /// `NORMAL(0)`.
    Normal,
    /// `LOW(1)`.
    Low,
    /// `VERY_LOW(2)`.
    VeryLow,
    /// `EXTREMELY_LOW(3)`.
    ExtremelyLow,
}

impl TickPriority {
    /// `values()` — the seven priorities in declaration (ordinal) order.
    pub const fn all() -> [TickPriority; 7] {
        [
            TickPriority::ExtremelyHigh,
            TickPriority::VeryHigh,
            TickPriority::High,
            TickPriority::Normal,
            TickPriority::Low,
            TickPriority::VeryLow,
            TickPriority::ExtremelyLow,
        ]
    }

    /// `getValue()` — the raw int value.
    pub const fn value(self) -> i32 {
        match self {
            TickPriority::ExtremelyHigh => -3,
            TickPriority::VeryHigh => -2,
            TickPriority::High => -1,
            TickPriority::Normal => 0,
            TickPriority::Low => 1,
            TickPriority::VeryLow => 2,
            TickPriority::ExtremelyLow => 3,
        }
    }

    /// `byValue(int)` — the first priority whose value matches, else the
    /// clamped end (`value < EXTREMELY_HIGH.value ? EXTREMELY_HIGH :
    /// EXTREMELY_LOW`).
    pub fn by_value(value: i32) -> TickPriority {
        for priority in TickPriority::all() {
            if priority.value() == value {
                return priority;
            }
        }
        if value < TickPriority::ExtremelyHigh.value() {
            TickPriority::ExtremelyHigh
        } else {
            TickPriority::ExtremelyLow
        }
    }

    /// `TickPriority.CODEC` — `Codec.INT.xmap(TickPriority::byValue,
    /// TickPriority::getValue)`, as the ops-generic factory.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TickPriority, Ops>> {
        codec::xmap(
            codec::int_codec::<Ops>(),
            Arc::new(|value: &i32| TickPriority::by_value(*value)),
            Arc::new(|priority: &TickPriority| priority.value()),
        )
    }
}

/// `net.minecraft.world.ticks.SavedTick<T>` — `record SavedTick<T>(T type,
/// BlockPos pos, int delay, TickPriority priority)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SavedTick<T> {
    /// `type` — the block/fluid id-handle (`Block` / `FluidId`).
    pub r#type: T,
    /// `pos` — the block position.
    pub pos: BlockPos,
    /// `delay` — the relative tick delay (`t`).
    pub delay: i32,
    /// `priority` — the scheduling priority (`p`).
    pub priority: TickPriority,
}

impl<T> SavedTick<T> {
    /// `new SavedTick<>(T type, BlockPos pos, int delay, TickPriority
    /// priority)`.
    pub fn new(r#type: T, pos: BlockPos, delay: i32, priority: TickPriority) -> Self {
        SavedTick {
            r#type,
            pos,
            delay,
            priority,
        }
    }

    /// `SavedTick.probe(T, BlockPos)` — `new SavedTick<>(type, pos, 0,
    /// TickPriority.NORMAL)`.
    pub fn probe(r#type: T, pos: BlockPos) -> Self {
        SavedTick::new(r#type, pos, 0, TickPriority::Normal)
    }

    /// `SavedTick.unpack(long currentTick, long currentSubTick)` — `new
    /// ScheduledTick<>(type, pos, currentTick + delay, priority,
    /// currentSubTick)`. The stored relative `delay` is added to the absolute
    /// `currentTick` with wrapping (a hostile delay can overflow the long).
    pub fn unpack(&self, current_tick: i64, current_sub_tick: i64) -> ScheduledTick<T>
    where
        T: Clone,
    {
        ScheduledTick::new(
            self.r#type.clone(),
            self.pos,
            current_tick.wrapping_add(self.delay as i64),
            self.priority,
            current_sub_tick,
        )
    }
}

/// `net.minecraft.world.ticks.ScheduledTick<T>` — the runtime tick:
/// `record ScheduledTick<T>(T type, BlockPos pos, long triggerTick,
/// TickPriority priority, long subTickOrder)`.
///
/// The record's compact constructor pins `pos = pos.immutable()`; the Rivet
/// [`BlockPos`] is already an immutable value, so the pin is inherent. Java's
/// record `equals` (all five fields) is only consumed by the deferred
/// `LevelTicks`; the per-position uniqueness this slice needs is the
/// `UNIQUE_TICK_HASH` projection (see [`UniqueTickKey`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledTick<T> {
    /// `type` — the block/fluid id-handle.
    pub r#type: T,
    /// `pos` — the immutable block position.
    pub pos: BlockPos,
    /// `triggerTick` — the absolute tick the tick is due (world game time).
    pub trigger_tick: i64,
    /// `priority` — the scheduling priority.
    pub priority: TickPriority,
    /// `subTickOrder` — the intra-tick tie-break (`DRAIN_ORDER`'s last key).
    pub sub_tick_order: i64,
}

impl<T> ScheduledTick<T> {
    /// `new ScheduledTick<>(T type, BlockPos pos, long triggerTick,
    /// TickPriority priority, long subTickOrder)` — the canonical constructor.
    pub fn new(
        r#type: T,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Self {
        ScheduledTick {
            r#type,
            pos,
            trigger_tick,
            priority,
            sub_tick_order,
        }
    }

    /// `new ScheduledTick<>(T type, BlockPos pos, long triggerTick, long
    /// subTickOrder)` — the convenience constructor that pins
    /// `TickPriority.NORMAL`.
    pub fn new_normal(r#type: T, pos: BlockPos, trigger_tick: i64, sub_tick_order: i64) -> Self {
        ScheduledTick::new(
            r#type,
            pos,
            trigger_tick,
            TickPriority::Normal,
            sub_tick_order,
        )
    }

    /// `ScheduledTick.probe(T, BlockPos)` — `new ScheduledTick<>(type, pos,
    /// 0L, TickPriority.NORMAL, 0L)`, the per-position probe used by the
    /// uniqueness sets.
    pub fn probe(r#type: T, pos: BlockPos) -> Self {
        ScheduledTick::new(r#type, pos, 0, TickPriority::Normal, 0)
    }

    /// `ScheduledTick.DRAIN_ORDER` — by `triggerTick`, then `priority`
    /// (ordinal: `EXTREMELY_HIGH` drains first), then `subTickOrder`.
    pub fn drain_cmp(a: &Self, b: &Self) -> Ordering {
        a.trigger_tick
            .cmp(&b.trigger_tick)
            .then_with(|| a.priority.cmp(&b.priority))
            .then_with(|| a.sub_tick_order.cmp(&b.sub_tick_order))
    }

    /// `ScheduledTick.INTRA_TICK_DRAIN_ORDER` — by `priority`, then
    /// `subTickOrder` (the drain order within one tick).
    pub fn intra_tick_drain_cmp(a: &Self, b: &Self) -> Ordering {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.sub_tick_order.cmp(&b.sub_tick_order))
    }

    /// `LevelChunkTicks.SUB_TICK_ORDERING` — by `subTickOrder` only.
    pub fn sub_tick_cmp(a: &Self, b: &Self) -> Ordering {
        a.sub_tick_order.cmp(&b.sub_tick_order)
    }

    /// `ScheduledTick.toSavedTick(long)` — `new SavedTick<>(type, pos,
    /// (int)(triggerTick - currentTick), priority)`. The long→int narrowing
    /// keeps the low 32 bits (Java cast truncation), so the subtraction is
    /// wrapping.
    pub fn to_saved_tick(&self, current_tick: i64) -> SavedTick<T>
    where
        T: Clone,
    {
        SavedTick::new(
            self.r#type.clone(),
            self.pos,
            self.trigger_tick.wrapping_sub(current_tick) as i32,
            self.priority,
        )
    }
}

/// The `ScheduledTick.UNIQUE_TICK_HASH` / `SavedTick.UNIQUE_TICK_HASH`
/// projection: two ticks are unique-equal when their `type` and `pos` match
/// (`a.type() == b.type() && a.pos().equals(b.pos())`).
///
/// Java's two strategy classes compute `31 * o.pos().hashCode() +
/// o.type().hashCode()`. The set's bucket order is not reproducible in either
/// implementation (and the type's Java `hashCode` is object identity), so this
/// `Hash` impl only needs to agree with the `Eq` projection: it writes the
/// block-position hash (the `Vec3i.hashCode()` the strategy uses) and then the
/// type's own hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniqueTickKey<T> {
    /// `type` — the id-handle.
    pub r#type: T,
    /// `pos` — the block position.
    pub pos: BlockPos,
}

impl<T> UniqueTickKey<T> {
    /// `new` — build the key for a (type, pos) probe.
    pub fn new(r#type: T, pos: BlockPos) -> Self {
        UniqueTickKey { r#type, pos }
    }
}

impl<T: Clone> From<&ScheduledTick<T>> for UniqueTickKey<T> {
    fn from(tick: &ScheduledTick<T>) -> Self {
        UniqueTickKey::new(tick.r#type.clone(), tick.pos)
    }
}

impl<T: Clone> From<&SavedTick<T>> for UniqueTickKey<T> {
    fn from(tick: &SavedTick<T>) -> Self {
        UniqueTickKey::new(tick.r#type.clone(), tick.pos)
    }
}

impl<T: Hash> Hash for UniqueTickKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Java's strategy returns the literal int `31 * pos.hashCode() +
        // type.hashCode()`; this Rust `Hasher` does not reproduce that
        // integer. The set only needs equal keys to hash equal, so writing the
        // `Vec3i.hashCode()` value then the type is sufficient (see the doc).
        self.pos.hash_code().hash(state);
        self.r#type.hash(state);
    }
}

/// `net.minecraft.world.ticks.TickAccess<T>` — the container interface:
/// `void schedule(ScheduledTick<T> tick)`, `boolean hasScheduledTick(BlockPos
/// pos, T type)`, `int count()`.
pub trait TickAccess<T> {
    /// `schedule(ScheduledTick<T>)`.
    fn schedule(&mut self, tick: ScheduledTick<T>);

    /// `hasScheduledTick(BlockPos, T)`.
    fn has_scheduled_tick(&self, pos: &BlockPos, r#type: &T) -> bool;

    /// `count()`.
    fn count(&self) -> usize;
}

/// `net.minecraft.world.ticks.TickContainerAccess<T>` — `extends
/// TickAccess<T>`, the marker for the per-chunk containers.
pub trait TickContainerAccess<T>: TickAccess<T> {}

/// `net.minecraft.world.ticks.SerializableTickContainer<T>` —
/// `List<SavedTick<T>> pack(long currentTick)`.
pub trait SerializableTickContainer<T> {
    /// `pack(long)` — encode the container back to the stored (relative-delay)
    /// form at the given absolute `currentTick`.
    fn pack(&self, current_tick: i64) -> Vec<SavedTick<T>>;
}

/// `java.util.PriorityQueue` with `ScheduledTick.DRAIN_ORDER` — the min-heap
/// behind `LevelChunkTicks.tickQueue`.
///
/// Replicates the sift-up/sift-down/`removeAt` array algorithm so the heap's
/// array layout matches Java's for the same insertion sequence (observable
/// through `getAll()`/`removeIf` iteration). `peek`/`poll` return the
/// `DRAIN_ORDER` minimum, which is what the deferred scheduler drains.
struct TickQueue<T> {
    queue: Vec<ScheduledTick<T>>,
}

impl<T> TickQueue<T> {
    fn new() -> Self {
        TickQueue { queue: Vec::new() }
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn peek(&self) -> Option<&ScheduledTick<T>> {
        self.queue.first()
    }

    /// `PriorityQueue.offer` + `siftUpUsingComparator`. Returns the index the
    /// offered element landed on so the caller can name the exact element
    /// (Java's `onTickAdded` receives the offered element, not the heap head).
    fn offer(&mut self, tick: ScheduledTick<T>) -> usize {
        self.queue.push(tick);
        self.sift_up(self.queue.len() - 1)
    }

    /// `PriorityQueue.poll` + `siftDownUsingComparator`.
    fn poll(&mut self) -> Option<ScheduledTick<T>> {
        if self.queue.is_empty() {
            return None;
        }
        let len = self.queue.len();
        let result = self.queue.swap_remove(0); // last element moves to index 0
        if len > 1 {
            self.sift_down(0);
        }
        Some(result)
    }

    /// `PriorityQueue.removeAt(int)` — remove the element at `i`, restoring the
    /// heap with sift-down (and sift-up when the moved element stays put).
    ///
    /// Returns the moved (former last) element when it was sifted strictly
    /// above the removed slot — Java returns it exactly then so the iterator can
    /// defer it to `forgetMeNot` for a later revisit (see [`LevelChunkTicks::
    /// remove_if`]).
    fn remove_at(&mut self, i: usize) -> Option<ScheduledTick<T>>
    where
        T: Clone,
    {
        let s = self.queue.len() - 1;
        if s == i {
            self.queue.pop();
            return None;
        }
        // Java: `E moved = es[s]; es[s] = null; siftDown(i, moved); if (es[i]
        // == moved) { siftUp(i, moved); if (es[i] != moved) return moved; }`.
        // `swap_remove` puts the last element at `i` first; the sift-down then
        // the conditional sift-up reproduce Java's array, and sifting the moved
        // element strictly above `i` is exactly the case Java reports back.
        self.queue.swap_remove(i);
        let landed = self.sift_down(i);
        if landed == i {
            let up = self.sift_up(i);
            if up != i {
                return Some(self.queue[up].clone());
            }
        }
        None
    }

    /// `PriorityQueue.removeEq(Object)` — remove the first element equal to
    /// `tick`, restoring the heap. Java scans by identity (`==`); the port
    /// scans by five-field value equality, which is equivalent **only because
    /// no non-adversarial path leaves two value-identical ticks in the heap** —
    /// the precondition that makes the substitute sound:
    ///
    /// - checked `schedule` deduplicates through the per-position set, so no
    ///   two scheduled ticks share a (type, pos);
    /// - `unpack` assigns strictly increasing `subTickOrder` per pending tick,
    ///   so two value-identical ticks cannot coexist on that path either.
    ///
    /// The scan self-checks that precondition instead of trusting it: a value
    /// match must be unique, because a second value-equal element would make
    /// the scan diverge from Java's identity removal (Java picks whichever
    /// object reference matches). The one adversarial exception: a pending list
    /// holding the same (type, pos) twice survives `unpack` as two
    /// subTickOrder-distinct ticks; `poll` then clears the per-position entry,
    /// after which a `schedule` whose five fields match the surviving tick
    /// re-admits a value-identical pair — the assert panics rather than
    /// silently removing the wrong tick.
    fn remove_eq(&mut self, tick: &ScheduledTick<T>)
    where
        T: Clone + PartialEq,
    {
        let mut matches = self
            .queue
            .iter()
            .enumerate()
            .filter(|(_, e)| *e == tick)
            .map(|(idx, _)| idx);
        let Some(idx) = matches.next() else {
            return; // Java no-ops when the identity is no longer in the heap.
        };
        assert!(
            matches.next().is_none(),
            "remove_eq value match is ambiguous: the heap holds two \
             value-identical ticks, so the value scan cannot reproduce Java's \
             identity removal"
        );
        self.remove_at(idx);
    }

    /// `siftUpUsingComparator` — bubble `queue[k]` up to its heap position,
    /// returning the index the element landed on.
    fn sift_up(&mut self, mut k: usize) -> usize {
        while k > 0 {
            let parent = (k - 1) >> 1;
            if ScheduledTick::drain_cmp(&self.queue[k], &self.queue[parent]) != Ordering::Less {
                return k;
            }
            self.queue.swap(k, parent);
            k = parent;
        }
        k
    }

    /// `siftDownUsingComparator` — move `queue[k]` down to its heap position,
    /// returning the index it landed on (so `remove_at` can apply Java's
    /// `queue[i] == moved` sift-up check).
    fn sift_down(&mut self, mut k: usize) -> usize {
        let len = self.queue.len();
        let half = len >> 1;
        while k < half {
            let mut child = (k << 1) + 1;
            let right = child + 1;
            if right < len
                && ScheduledTick::drain_cmp(&self.queue[child], &self.queue[right])
                    == Ordering::Greater
            {
                child = right;
            }
            if ScheduledTick::drain_cmp(&self.queue[k], &self.queue[child]) != Ordering::Greater {
                return k;
            }
            self.queue.swap(k, child);
            k = child;
        }
        k
    }
}

/// `net.minecraft.world.ticks.LevelChunkTicks<T>` — the runtime container
/// holding a loaded chunk's scheduled ticks (`LevelChunk.blockTicks` /
/// `LevelChunk.fluidTicks`).
///
/// Mirrors `LevelChunkTicks.java`: the `DRAIN_ORDER`-min-heap `tickQueue`, the
/// pending stored-ticks list (`pendingTicks`), the per-position uniqueness set
/// (`ticksPerPosition` over the `UNIQUE_TICK_HASH` projection), the
/// `onTickAdded` hook, and the Moonrise `moonrise$isDirty`/`moonrise$clearDirty`
/// dirty surface (`ChunkSystemLevelChunkTicks`).
///
/// Duplicate semantics are faithful: `schedule` deduplicates through the
/// per-position set, but the pending-ticks constructor seeds the set with every
/// probe (dedup inside the set) and `unpack` schedules *every* pending tick
/// through the unchecked path — so a pending list that itself contains
/// duplicates keeps them after unpack, exactly like Java.
pub struct LevelChunkTicks<T> {
    /// `tickQueue` — the `DRAIN_ORDER` min-heap of scheduled ticks.
    tick_queue: TickQueue<T>,
    /// `pendingTicks` — the stored (relative-delay) ticks waiting to be
    /// unpacked into the queue.
    pending_ticks: Option<Vec<SavedTick<T>>>,
    /// `ticksPerPosition` — the `UNIQUE_TICK_HASH` per-position set.
    ticks_per_position: HashSet<UniqueTickKey<T>>,
    /// `onTickAdded` — the hook invoked by `scheduleUnchecked`.
    on_tick_added: Option<Arc<OnTickAdded<T>>>,
    /// Paper `dirty` — a schedule/poll/remove occurred since `lastSaved`.
    dirty: bool,
    /// Paper `lastSaved` — the tick `pack`/`unpack` last recorded.
    ///
    /// Java's `pack` mutates `lastSaved` from a method that otherwise only
    /// reads the container, and the `SerializableTickContainer.pack` interface
    /// exposes no mutation path. The container is tick-thread-confined
    /// (OWNERSHIP D5), so a `Cell` is the faithful, lock-free way to carry that
    /// hidden mutation.
    last_saved: std::cell::Cell<i64>,
}

/// `BiConsumer<LevelChunkTicks<T>, ScheduledTick<T>>` — the `onTickAdded`
/// hook. The deferred `LevelTicks` wires its `chunkScheduleUpdater` here.
pub type OnTickAdded<T> = dyn Fn(&LevelChunkTicks<T>, &ScheduledTick<T>);

impl<T> LevelChunkTicks<T> {
    /// `new LevelChunkTicks<>()` — the empty container.
    pub fn new() -> Self {
        LevelChunkTicks {
            tick_queue: TickQueue::new(),
            pending_ticks: None,
            ticks_per_position: HashSet::new(),
            on_tick_added: None,
            dirty: false,
            last_saved: std::cell::Cell::new(i64::MIN),
        }
    }

    /// `new LevelChunkTicks<>(List<SavedTick<T>> pendingTicks)` — seed the
    /// per-position set with a probe for every pending tick (dedup inside the
    /// set); the pending list is retained verbatim for `unpack`.
    pub fn new_with_pending(pending_ticks: Vec<SavedTick<T>>) -> Self
    where
        T: Clone + Eq + Hash,
    {
        let mut container = LevelChunkTicks::new();
        for pending in &pending_ticks {
            container
                .ticks_per_position
                .insert(UniqueTickKey::from(pending));
        }
        container.pending_ticks = Some(pending_ticks);
        container
    }

    /// `setOnTickAdded(BiConsumer<LevelChunkTicks<T>, ScheduledTick<T>>)`.
    pub fn set_on_tick_added(&mut self, on_tick_added: Option<Arc<OnTickAdded<T>>>) {
        self.on_tick_added = on_tick_added;
    }

    /// `peek()` — the next tick to drain (`DRAIN_ORDER` minimum), without
    /// removing it.
    pub fn peek(&self) -> Option<&ScheduledTick<T>> {
        self.tick_queue.peek()
    }

    /// `poll()` — remove and return the next tick to drain. Removes the
    /// per-position entry and marks the container dirty (Paper rewrite).
    pub fn poll(&mut self) -> Option<ScheduledTick<T>>
    where
        T: Clone + Eq + Hash,
    {
        let result = self.tick_queue.poll();
        if let Some(tick) = &result {
            self.ticks_per_position.remove(&UniqueTickKey::from(tick));
            self.dirty = true;
        }
        result
    }

    /// `schedule(ScheduledTick<T>)` — deduplicate through the per-position
    /// set, then schedule unchecked (Paper marks dirty).
    pub fn schedule(&mut self, tick: ScheduledTick<T>)
    where
        T: Clone + Eq + Hash,
    {
        if self.ticks_per_position.insert(UniqueTickKey::from(&tick)) {
            self.schedule_unchecked(tick);
            self.dirty = true;
        }
    }

    /// `scheduleUnchecked(ScheduledTick<T>)` — push onto the queue and fire
    /// the `onTickAdded` hook with the exact offered element. The unchecked
    /// path (used by `unpack`) does not consult the per-position set and does
    /// not mark dirty.
    fn schedule_unchecked(&mut self, tick: ScheduledTick<T>) {
        let index = self.tick_queue.offer(tick);
        if let Some(callback) = &self.on_tick_added {
            callback(self, &self.tick_queue.queue[index]);
        }
    }

    /// `hasScheduledTick(BlockPos, T)` — the per-position set membership.
    pub fn has_scheduled_tick(&self, pos: &BlockPos, r#type: &T) -> bool
    where
        T: Clone + Eq + Hash,
    {
        self.ticks_per_position
            .contains(&UniqueTickKey::new(r#type.clone(), *pos))
    }

    /// `removeIf(Predicate<ScheduledTick<T>>)` — remove every queued tick
    /// matching the predicate, updating the per-position set and marking dirty.
    ///
    /// Reproduces the `java.util.PriorityQueue` iterator Java's `removeIf` loop
    /// walks, including the `forgetMeNot` deferred-removal path: removing the
    /// element at `i` shifts the former last element into the heap; when it
    /// stays at or below `i`, the cursor is reset so that element is revisited
    /// (`Itr.remove` `cursor--`); when `removeAt` instead sifts it strictly
    /// above `i` (its `removeAt` return value), the slot now holds an
    /// already-visited element so the cursor is not reset, and the moved
    /// element is deferred — then re-tested and removed by `removeEq` after the
    /// main pass. A deferred element can match the predicate (e.g. `clearArea`'s
    /// positional test), so reproducing the deferral is required for the
    /// surviving set to match Java.
    ///
    /// The per-position set is updated per removed element exactly like Java
    /// (`ticksPerPosition.remove(tick)` for each removed tick, main pass and
    /// drain alike), preserving its staleness when the queue holds duplicate
    /// (type, pos) entries from the unchecked `unpack` path.
    pub fn remove_if(&mut self, mut test: impl FnMut(&ScheduledTick<T>) -> bool)
    where
        T: Clone + Eq + Hash,
    {
        let mut i = 0;
        let mut forget_me_not: Vec<ScheduledTick<T>> = Vec::new();
        while i < self.tick_queue.queue.len() {
            if test(&self.tick_queue.queue[i]) {
                self.dirty = true;
                self.ticks_per_position
                    .remove(&UniqueTickKey::from(&self.tick_queue.queue[i]));
                if let Some(moved) = self.tick_queue.remove_at(i) {
                    // Java `Itr.remove` with a returned `moved`: the cursor is
                    // NOT reset (the element now at `i` was already visited);
                    // the moved element is re-tested after the main pass.
                    forget_me_not.push(moved);
                    i += 1;
                }
                // else Java `cursor--`: revisit the element shifted into `i`.
            } else {
                i += 1;
            }
        }
        // Java's iterator drains `forgetMeNot` after the main pass, re-testing
        // each deferred element and removing it by identity (`removeEq`).
        for moved in forget_me_not {
            if test(&moved) {
                self.dirty = true;
                self.tick_queue.remove_eq(&moved);
                self.ticks_per_position.remove(&UniqueTickKey::from(&moved));
            }
        }
    }

    /// `getAll()` — iterate the queued ticks in heap-array order (Java's
    /// `tickQueue.stream()`).
    pub fn all(&self) -> impl Iterator<Item = &ScheduledTick<T>> {
        self.tick_queue.queue.iter()
    }

    /// `count()` — queued plus pending.
    pub fn count(&self) -> usize {
        self.tick_queue.len()
            + self
                .pending_ticks
                .as_ref()
                .map_or(0, |pending| pending.len())
    }

    /// `pack(long currentTick)` — the stored form: the pending list (retained
    /// order) followed by the queued ticks sorted ascending by `subTickOrder`
    /// and converted to relative `SavedTick`s. Records `currentTick` as
    /// `lastSaved` (Java's hidden mutation, carried through the tick-confined
    /// `Cell`).
    pub fn pack(&self, current_tick: i64) -> Vec<SavedTick<T>>
    where
        T: Clone,
    {
        self.last_saved.set(current_tick);
        let mut ticks = Vec::with_capacity(self.tick_queue.len());
        if let Some(pending) = &self.pending_ticks {
            ticks.extend(pending.iter().cloned());
        }
        let mut sorted = self.tick_queue.queue.clone();
        sorted.sort_by(ScheduledTick::sub_tick_cmp);
        ticks.extend(sorted.iter().map(|tick| tick.to_saved_tick(current_tick)));
        ticks
    }

    /// `unpack(long currentTick)` — schedule every pending stored tick into the
    /// queue with `subTickBase = -pending.size()`, incrementing per element, and
    /// clear the pending list. The unchecked path preserves pending duplicates.
    pub fn unpack(&mut self, current_tick: i64)
    where
        T: Clone,
    {
        // `take` moves the pending list out O(1) — `schedule_unchecked` needs
        // `&mut self`, so iterating in place would conflict with the borrow.
        // The list is always discarded after unpack, matching Java.
        if let Some(pending) = std::mem::take(&mut self.pending_ticks) {
            self.last_saved.set(current_tick);
            let mut sub_tick_base = -(pending.len() as i64);
            for pending_tick in &pending {
                self.schedule_unchecked(pending_tick.unpack(current_tick, sub_tick_base));
                sub_tick_base = sub_tick_base.wrapping_add(1);
            }
        }
    }

    /// `moonrise$isDirty(long)` — the Moonrise dirty surface: the container was
    /// mutated since the last `clearDirty`, or it holds queued ticks that were
    /// never saved at the given tick (Paper rewrite-chunk-system).
    pub fn is_dirty(&self, tick: i64) -> bool {
        self.dirty || (!self.tick_queue.is_empty() && tick != self.last_saved.get())
    }

    /// `moonrise$clearDirty()` — clear the mutation flag.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

impl<T> Default for LevelChunkTicks<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Eq + Hash> TickAccess<T> for LevelChunkTicks<T> {
    fn schedule(&mut self, tick: ScheduledTick<T>) {
        LevelChunkTicks::schedule(self, tick);
    }

    fn has_scheduled_tick(&self, pos: &BlockPos, r#type: &T) -> bool {
        LevelChunkTicks::has_scheduled_tick(self, pos, r#type)
    }

    fn count(&self) -> usize {
        LevelChunkTicks::count(self)
    }
}

impl<T: Clone + Eq + Hash> TickContainerAccess<T> for LevelChunkTicks<T> {}

impl<T: Clone> SerializableTickContainer<T> for LevelChunkTicks<T> {
    fn pack(&self, current_tick: i64) -> Vec<SavedTick<T>> {
        LevelChunkTicks::pack(self, current_tick)
    }
}

/// `net.minecraft.world.ticks.ProtoChunkTicks<T>` — the worldgen/loading-stage
/// container holding stored (relative-delay) ticks.
///
/// Mirrors `ProtoChunkTicks.java`: an insertion-ordered stored-tick list plus
/// the `UNIQUE_TICK_HASH` per-position set. `schedule(ScheduledTick)` converts
/// to a zero-delay stored tick; `load` deduplicates through `schedule`.
pub struct ProtoChunkTicks<T> {
    /// `ticks` — the insertion-ordered stored ticks.
    ticks: Vec<SavedTick<T>>,
    /// `ticksPerPosition` — the `UNIQUE_TICK_HASH` per-position set over the
    /// stored ticks.
    ticks_per_position: HashSet<UniqueTickKey<T>>,
}

impl<T: Clone + Eq + Hash> Clone for ProtoChunkTicks<T> {
    fn clone(&self) -> Self {
        ProtoChunkTicks {
            ticks: self.ticks.clone(),
            ticks_per_position: self.ticks_per_position.clone(),
        }
    }
}

impl<T> ProtoChunkTicks<T> {
    /// `new ProtoChunkTicks<>()`.
    pub fn new() -> Self {
        ProtoChunkTicks {
            ticks: Vec::new(),
            ticks_per_position: HashSet::new(),
        }
    }

    /// `schedule(ScheduledTick<T>)` — store as a zero-delay `SavedTick`.
    pub fn schedule(&mut self, tick: ScheduledTick<T>)
    where
        T: Clone + Eq + Hash,
    {
        let stored = SavedTick::new(tick.r#type, tick.pos, 0, tick.priority);
        self.schedule_saved(stored);
    }

    /// The private `schedule(SavedTick<T>)` — deduplicate through the set,
    /// then append to the list.
    pub fn schedule_saved(&mut self, new_tick: SavedTick<T>)
    where
        T: Clone + Eq + Hash,
    {
        if self
            .ticks_per_position
            .insert(UniqueTickKey::from(&new_tick))
        {
            self.ticks.push(new_tick);
        }
    }

    /// `hasScheduledTick(BlockPos, T)` — the per-position set membership.
    pub fn has_scheduled_tick(&self, pos: &BlockPos, r#type: &T) -> bool
    where
        T: Clone + Eq + Hash,
    {
        self.ticks_per_position
            .contains(&UniqueTickKey::new(r#type.clone(), *pos))
    }

    /// `count()` — the number of stored ticks.
    pub fn count(&self) -> usize {
        self.ticks.len()
    }

    /// `pack(long currentTick)` — the stored ticks as-is (relative delays are
    /// already stored; `currentTick` is ignored, like Java).
    pub fn pack(&self, _current_tick: i64) -> Vec<SavedTick<T>>
    where
        T: Clone,
    {
        self.ticks.clone()
    }

    /// `scheduledTicks()` — `List.copyOf(this.ticks)`: the stored ticks in
    /// insertion order.
    pub fn scheduled_ticks(&self) -> Vec<SavedTick<T>>
    where
        T: Clone,
    {
        self.ticks.clone()
    }

    /// `load(List<SavedTick<T>>)` — build a container from stored ticks,
    /// deduplicating through `schedule`.
    pub fn load(ticks: &[SavedTick<T>]) -> Self
    where
        T: Clone + Eq + Hash,
    {
        let mut result = ProtoChunkTicks::new();
        for tick in ticks {
            result.schedule_saved(tick.clone());
        }
        result
    }
}

impl<T> Default for ProtoChunkTicks<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + Eq + Hash> TickAccess<T> for ProtoChunkTicks<T> {
    fn schedule(&mut self, tick: ScheduledTick<T>) {
        ProtoChunkTicks::schedule(self, tick);
    }

    fn has_scheduled_tick(&self, pos: &BlockPos, r#type: &T) -> bool {
        ProtoChunkTicks::has_scheduled_tick(self, pos, r#type)
    }

    fn count(&self) -> usize {
        ProtoChunkTicks::count(self)
    }
}

impl<T: Clone + Eq + Hash> TickContainerAccess<T> for ProtoChunkTicks<T> {}

impl<T: Clone> SerializableTickContainer<T> for ProtoChunkTicks<T> {
    fn pack(&self, current_tick: i64) -> Vec<SavedTick<T>> {
        ProtoChunkTicks::pack(self, current_tick)
    }
}

/// `SavedTick.codec(Codec<T>)` — the faithful codec factory.
///
/// Java builds a `MapCodec<BlockPos>` over `x`/`y`/`z`, then a record codec
/// over `i` (the type codec), `pos`, `t` (`Codec.INT`), `p`
/// (`TickPriority.CODEC`). Decode/encode therefore use the exact field order
/// `i, x, y, z, t, p` and DFU's error/partial accumulation.
pub fn saved_tick_codec<T, Ops>(
    type_codec: Arc<dyn Codec<T, Ops>>,
) -> Arc<dyn Codec<SavedTick<T>, Ops>>
where
    T: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    let pos_codec = record_builder::map_codec::<BlockPos, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|pos: &BlockPos| pos.get_x()),
                "x".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|pos: &BlockPos| pos.get_y()),
                "y".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|pos: &BlockPos| pos.get_z()),
                "z".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .apply(instance, Arc::new(BlockPos::new))
    });

    record_builder::create::<SavedTick<T>, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|tick: &SavedTick<T>| tick.r#type.clone()),
                "i".to_string(),
                type_codec,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|tick: &SavedTick<T>| tick.pos),
                pos_codec,
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|tick: &SavedTick<T>| tick.delay),
                "t".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|tick: &SavedTick<T>| tick.priority),
                "p".to_string(),
                TickPriority::codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(
                    |r#type: T, pos: BlockPos, delay: i32, priority: TickPriority| {
                        SavedTick::new(r#type, pos, delay, priority)
                    },
                ),
            )
    })
}

/// `SavedTick.filterTickListForChunk(List<SavedTick<T>>, ChunkPos)` — keep
/// only the ticks whose `BlockPos` packs to the given chunk.
///
/// Java compares `ChunkPos.pack(tick.pos()) == chunkPos.pack()`. Ticks are
/// retained in list order; the `y` coordinate is irrelevant to the match.
pub fn filter_tick_list_for_chunk<T>(
    saved_ticks: &[SavedTick<T>],
    chunk_pos: &ChunkPos,
) -> Vec<SavedTick<T>>
where
    T: Clone,
{
    let pos_key = chunk_pos.pack();
    saved_ticks
        .iter()
        .filter(|tick| ChunkPos::pack_block_pos(&tick.pos) == pos_key)
        .cloned()
        .collect()
}

// RivetTodo(#522): the level-level scheduling surfaces of
// `net.minecraft.world.ticks` stay deferred to the tick-execution slice:
// `LevelTicks` (the `tick`/`collectTicks`/`drainContainers` machinery and the
// `tickCheck` long-predicate), `LevelTickAccess.willTickThisTick`, and the
// `BlackholeTickAccess`/`WorldGenTickAccess` adapters. The per-chunk
// containers (`LevelChunkTicks`/`ProtoChunkTicks`) and `ScheduledTick` live
// here (#522) so the loaded-world reconstruction can carry and unpack stored
// ticks without executing them.

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag::Tag;
    use rivet_serialization::json_ops::JsonOps;

    /// Java `TickPriority` values and the clamp fallback.
    #[test]
    fn tick_priority_values_and_clamping() {
        assert_eq!(TickPriority::ExtremelyHigh.value(), -3);
        assert_eq!(TickPriority::VeryHigh.value(), -2);
        assert_eq!(TickPriority::High.value(), -1);
        assert_eq!(TickPriority::Normal.value(), 0);
        assert_eq!(TickPriority::Low.value(), 1);
        assert_eq!(TickPriority::VeryLow.value(), 2);
        assert_eq!(TickPriority::ExtremelyLow.value(), 3);
        for priority in TickPriority::all() {
            assert_eq!(TickPriority::by_value(priority.value()), priority);
        }
        // Out-of-range values clamp to the nearest end.
        assert_eq!(TickPriority::by_value(-4), TickPriority::ExtremelyHigh);
        assert_eq!(TickPriority::by_value(-100), TickPriority::ExtremelyHigh);
        assert_eq!(TickPriority::by_value(4), TickPriority::ExtremelyLow);
        assert_eq!(TickPriority::by_value(100), TickPriority::ExtremelyLow);
    }

    /// Round-trip a SavedTick through the codec over JsonOps, checking the
    /// exact encoded shape (field names/values).
    #[test]
    fn saved_tick_codec_roundtrips_json_shape() {
        use rivet_registry::core::BlockPos;
        use serde_json::json;

        let type_codec: Arc<dyn Codec<String, JsonOps>> = codec::string_codec();
        let codec = saved_tick_codec(type_codec);
        let tick = SavedTick::new(
            "minecraft:stone".to_string(),
            BlockPos::new(1, 2, 3),
            5,
            TickPriority::Low,
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &tick)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"i": "minecraft:stone", "x": 1, "y": 2, "z": 3, "t": 5, "p": 1})
        );
        let decoded = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = decoded.result().expect("decode should succeed");
        assert_eq!(*decoded, tick);
    }

    /// The filter keeps only this chunk's ticks, in list order, ignoring y.
    #[test]
    fn filter_tick_list_for_chunk_keeps_matching_ordered() {
        use rivet_registry::core::BlockPos;

        let chunk = ChunkPos::new(2, -3);
        let ticks = vec![
            SavedTick::new("a", BlockPos::new(33, 0, -47), 1, TickPriority::Normal), // in (2,-3)
            SavedTick::new("b", BlockPos::new(1, 100, -3), 2, TickPriority::Low),    // chunk (0,-1)
            SavedTick::new("c", BlockPos::new(32, -64, -48), 3, TickPriority::High), // in (2,-3)
        ];
        let kept = filter_tick_list_for_chunk(&ticks, &chunk);
        assert_eq!(
            kept.iter().map(|tick| tick.r#type).collect::<Vec<_>>(),
            vec!["a", "c"]
        );
    }

    // -----------------------------------------------------------------------
    // ScheduledTick / UNIQUE_TICK_HASH (#522)
    // -----------------------------------------------------------------------

    #[test]
    fn scheduled_tick_drain_order_matches_java() {
        // Java `DRAIN_ORDER`: triggerTick, then priority (ordinal), then
        // subTickOrder.
        let t = |type_: &str, pos: (i32, i32, i32), trigger: i64, prio: TickPriority, sub: i64| {
            ScheduledTick::new(
                type_.to_string(),
                BlockPos::new(pos.0, pos.1, pos.2),
                trigger,
                prio,
                sub,
            )
        };
        // Same trigger: priority wins (EXTREMELY_HIGH first).
        assert_eq!(
            ScheduledTick::drain_cmp(
                &t("a", (0, 0, 0), 10, TickPriority::ExtremelyHigh, 0),
                &t("b", (1, 0, 0), 10, TickPriority::High, 0),
            ),
            Ordering::Less
        );
        // Same trigger+priority: subTickOrder wins.
        assert_eq!(
            ScheduledTick::drain_cmp(
                &t("a", (0, 0, 0), 10, TickPriority::Normal, 5),
                &t("b", (1, 0, 0), 10, TickPriority::Normal, 2),
            ),
            Ordering::Greater
        );
        // Trigger dominates priority.
        assert_eq!(
            ScheduledTick::drain_cmp(
                &t("a", (0, 0, 0), 5, TickPriority::ExtremelyLow, 0),
                &t("b", (1, 0, 0), 6, TickPriority::ExtremelyHigh, 0),
            ),
            Ordering::Less
        );
    }

    #[test]
    fn scheduled_tick_probe_and_to_saved_tick() {
        // `toSavedTick(currentTick)` computes `(int)(triggerTick - currentTick)`.
        let tick = ScheduledTick::new(
            "stone".to_string(),
            BlockPos::new(1, 2, 3),
            1000,
            TickPriority::Low,
            7,
        );
        let saved = tick.to_saved_tick(995);
        assert_eq!(
            saved,
            SavedTick::new(
                "stone".to_string(),
                BlockPos::new(1, 2, 3),
                5,
                TickPriority::Low
            )
        );
        // Negative relative delay (tick already past) narrows to the low 32 bits.
        let past = tick.to_saved_tick(2000);
        assert_eq!(past.delay, -1000);
        // Hostile: triggerTick - currentTick overflows i32; Java keeps low bits.
        let wrap = ScheduledTick::new(
            "stone".to_string(),
            BlockPos::new(1, 2, 3),
            i64::MAX,
            TickPriority::Normal,
            0,
        )
        .to_saved_tick(0);
        assert_eq!(wrap.delay, (i64::MAX).wrapping_sub(0) as i32);
    }

    #[test]
    fn unique_tick_key_hash_equals_pos_and_type() {
        // Two ticks with equal pos+type are unique-equal regardless of
        // triggerTick/priority/subTickOrder.
        let a = UniqueTickKey::new("stone".to_string(), BlockPos::new(4, 5, 6));
        let b = UniqueTickKey::new("stone".to_string(), BlockPos::new(4, 5, 6));
        assert_eq!(a, b);
        let c = UniqueTickKey::new("sand".to_string(), BlockPos::new(4, 5, 6));
        assert_ne!(a, c);
        let d = UniqueTickKey::new("stone".to_string(), BlockPos::new(4, 5, 7));
        assert_ne!(a, d);
    }

    // -----------------------------------------------------------------------
    // LevelChunkTicks (#522)
    // -----------------------------------------------------------------------

    fn sched(
        type_: &str,
        x: i32,
        z: i32,
        trigger: i64,
        prio: TickPriority,
        sub: i64,
    ) -> ScheduledTick<String> {
        ScheduledTick::new(
            type_.to_string(),
            BlockPos::new(x, 0, z),
            trigger,
            prio,
            sub,
        )
    }

    #[test]
    fn level_chunk_ticks_schedule_dedupes_and_drains_in_drain_order() {
        let mut container = LevelChunkTicks::new();
        container.schedule(sched("a", 0, 0, 10, TickPriority::Normal, 0));
        container.schedule(sched("b", 5, 5, 8, TickPriority::Normal, 0));
        // Duplicate position+type is rejected by the per-position set even
        // with a different trigger.
        container.schedule(sched("a", 0, 0, 99, TickPriority::Normal, 0));
        assert_eq!(container.count(), 2);
        assert!(container.has_scheduled_tick(&BlockPos::new(0, 0, 0), &"a".to_string()));
        assert!(!container.has_scheduled_tick(&BlockPos::new(0, 0, 0), &"b".to_string()));

        // Drain order: b (trigger 8) before a (trigger 10).
        let first = container.poll().unwrap();
        assert_eq!(first.r#type, "b");
        assert_eq!(first.trigger_tick, 8);
        let second = container.poll().unwrap();
        assert_eq!(second.r#type, "a");
        assert!(container.poll().is_none());
        // Poll removed the per-position entries.
        assert!(!container.has_scheduled_tick(&BlockPos::new(0, 0, 0), &"a".to_string()));
    }

    #[test]
    fn level_chunk_ticks_pack_unpack_preserves_relative_delay_and_subtick_order() {
        // Pack: pending list first (retained order), then queue sorted by
        // subTickOrder ascending, converted to relative SavedTicks.
        let mut container = LevelChunkTicks::new_with_pending(vec![SavedTick::new(
            "pending".to_string(),
            BlockPos::new(0, 0, 0),
            42,
            TickPriority::Normal,
        )]);
        container.schedule(sched("z", 1, 0, 1000, TickPriority::Normal, 2));
        container.schedule(sched("a", 2, 0, 1000, TickPriority::Normal, 1));
        let packed = container.pack(900);
        // pending first, then subTickOrder 1 then 2.
        assert_eq!(packed[0].r#type, "pending");
        assert_eq!(packed[0].delay, 42);
        assert_eq!(packed[1].r#type, "a");
        assert_eq!(packed[1].delay, 100);
        assert_eq!(packed[2].r#type, "z");
        assert_eq!(packed[2].delay, 100);

        // Unpack into a fresh container: pending are scheduled with
        // subTickBase = -pending.size() and incremented. The stored pending
        // tick has delay 42, so it lands at trigger 942 with subTickOrder -3 —
        // earlier than the two 1000-trigger queued ticks, so it drains first
        // (Java `DRAIN_ORDER` sorts by triggerTick first).
        let mut fresh = LevelChunkTicks::new_with_pending(packed.clone());
        fresh.unpack(900);
        assert_eq!(fresh.count(), 3);
        let mut drained = Vec::new();
        while let Some(t) = fresh.poll() {
            drained.push(t);
        }
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].r#type, "pending");
        assert_eq!(drained[0].trigger_tick, 942);
        assert_eq!(drained[0].sub_tick_order, -3);
        // Same trigger (1000): subTickOrder breaks the tie.
        assert_eq!(drained[1].r#type, "a");
        assert_eq!(drained[1].trigger_tick, 1000);
        assert_eq!(drained[1].sub_tick_order, -2);
        assert_eq!(drained[2].r#type, "z");
        assert_eq!(drained[2].trigger_tick, 1000);
        assert_eq!(drained[2].sub_tick_order, -1);
    }

    #[test]
    fn level_chunk_ticks_pending_duplicates_survive_unpack() {
        // A pending list containing the same (type,pos) twice schedules both
        // through the unchecked path — Java keeps them.
        let mut container = LevelChunkTicks::new_with_pending(vec![
            SavedTick::new(
                "dup".to_string(),
                BlockPos::new(0, 0, 0),
                5,
                TickPriority::Normal,
            ),
            SavedTick::new(
                "dup".to_string(),
                BlockPos::new(0, 0, 0),
                5,
                TickPriority::Normal,
            ),
        ]);
        container.unpack(100);
        assert_eq!(container.count(), 2);
        let drained = (0..2)
            .map(|_| container.poll().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(drained[0].r#type, "dup");
        assert_eq!(drained[1].r#type, "dup");
        assert!(container.poll().is_none());
    }

    #[test]
    fn level_chunk_ticks_remove_if_matches_java_iterator_removal() {
        let mut container = LevelChunkTicks::new();
        container.schedule(sched("a", 0, 0, 10, TickPriority::Normal, 0));
        container.schedule(sched("b", 1, 0, 11, TickPriority::Normal, 1));
        container.schedule(sched("c", 2, 0, 9, TickPriority::Normal, 2));
        container.remove_if(|tick| tick.r#type == "b");
        assert_eq!(container.count(), 2);
        assert!(!container.has_scheduled_tick(&BlockPos::new(1, 0, 0), &"b".to_string()));
        let drained = (0..2)
            .map(|_| container.poll().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            drained
                .iter()
                .map(|t| t.r#type.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "a"]
        );
    }

    #[test]
    fn level_chunk_ticks_remove_if_deferred_moved_element() {
        // Exercises the `PriorityQueue` iterator's `forgetMeNot` path: removing
        // a mid-heap element whose replacement sifts strictly above the removed
        // slot. The moved (former-last) element is NOT revisited in place; it
        // is deferred and re-tested after the main pass, then removed by
        // `removeEq`. The heap array `[a,b,c,d,e,f,g]` (triggers 1,8,2,10,9,4,3)
        // reproduces Java's layout; removing index 3 (d) returns g as the moved
        // element (verified against the JDK algorithm).
        let mut container = LevelChunkTicks::new();
        for (name, trigger) in [
            ("a", 1i64),
            ("b", 8),
            ("c", 2),
            ("d", 10),
            ("e", 9),
            ("f", 4),
            ("g", 3),
        ] {
            container.schedule(sched(
                name,
                trigger as i32,
                0,
                trigger,
                TickPriority::Normal,
                0,
            ));
        }
        // Sanity: the queue holds the exact array Java produces for this
        // insertion order.
        assert_eq!(
            container
                .all()
                .map(|t| t.r#type.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d", "e", "f", "g"]
        );
        // The predicate matches d (index 3, the removed slot) and g (trigger 3,
        // the element `removeAt` defers). Removing d returns g as the moved
        // element, which is then re-tested and removed from `forgetMeNot`.
        container.remove_if(|t| t.trigger_tick == 10 || t.trigger_tick == 3);
        assert_eq!(container.count(), 5);
        assert!(!container.has_scheduled_tick(&BlockPos::new(10, 0, 0), &"d".to_string()));
        assert!(!container.has_scheduled_tick(&BlockPos::new(3, 0, 0), &"g".to_string()));
        let mut drained = Vec::new();
        while let Some(t) = container.poll() {
            drained.push(t.r#type);
        }
        assert_eq!(
            drained,
            vec![
                "a".to_string(),
                "c".to_string(),
                "f".to_string(),
                "b".to_string(),
                "e".to_string()
            ]
        );
    }

    #[test]
    fn level_chunk_ticks_remove_if_deferred_element_survives_when_non_matching() {
        // Same heap layout as the deferred-moved-element test, but the
        // predicate matches only the removed slot d (trigger 10), not the
        // deferred element g (trigger 3). Java defers g to forgetMeNot and
        // re-tests it after the main pass; since it does not match, it
        // survives in the heap (JDK `Itr` behavior). Pins the "deferred
        // element re-tested, kept when no match" path distinct from the
        // deferred-element-also-removed case above.
        let mut container = LevelChunkTicks::new();
        for (name, trigger) in [
            ("a", 1i64),
            ("b", 8),
            ("c", 2),
            ("d", 10),
            ("e", 9),
            ("f", 4),
            ("g", 3),
        ] {
            container.schedule(sched(
                name,
                trigger as i32,
                0,
                trigger,
                TickPriority::Normal,
                0,
            ));
        }
        // Sanity: the queue holds the exact array Java produces for this
        // insertion order (same as the sibling test).
        assert_eq!(
            container
                .all()
                .map(|t| t.r#type.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d", "e", "f", "g"]
        );
        // Removing d (index 3) defers g; re-testing g with a predicate that
        // matches only d keeps g in the heap.
        container.remove_if(|t| t.trigger_tick == 10);
        assert_eq!(container.count(), 6);
        assert!(!container.has_scheduled_tick(&BlockPos::new(10, 0, 0), &"d".to_string()));
        assert!(container.has_scheduled_tick(&BlockPos::new(3, 0, 0), &"g".to_string()));
        let mut drained = Vec::new();
        while let Some(t) = container.poll() {
            drained.push(t.r#type);
        }
        assert_eq!(
            drained,
            vec![
                "a".to_string(),
                "c".to_string(),
                "g".to_string(),
                "f".to_string(),
                "b".to_string(),
                "e".to_string()
            ]
        );
    }

    #[test]
    fn level_chunk_ticks_remove_eq_rejects_ambiguous_value_match() {
        // `remove_eq` scans by five-field value equality where Java's `removeEq`
        // scans by identity; the substitute is sound only while the heap holds
        // no two value-identical ticks. No public path can create one (checked
        // `schedule` dedups by (type,pos); `unpack` assigns strictly increasing
        // `subTickOrder`), so inject the violating state directly and pin the
        // self-check that refuses to guess which element Java's identity scan
        // would have removed.
        let mut container = LevelChunkTicks::new();
        let tick = sched("dup", 0, 0, 100, TickPriority::Normal, 0);
        container.tick_queue.queue.push(tick.clone());
        container.tick_queue.queue.push(tick.clone());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            container.tick_queue.remove_eq(&tick);
        }));
        assert!(
            panic.is_err(),
            "ambiguous remove_eq must not silently pick one of two value-identical ticks"
        );
    }

    #[test]
    fn level_chunk_ticks_remove_eq_rejects_poll_then_schedule_value_identical_pair() {
        // The adversarial window the `remove_eq` doc names: a pending list
        // holding the same (type,pos) twice survives `unpack` as two
        // subTickOrder-distinct ticks; `poll` then clears the per-position
        // entry, after which a `schedule` whose five fields match the surviving
        // tick re-admits a genuine value-identical pair. The self-check must
        // refuse rather than silently pick which element Java's identity scan
        // would have removed.
        let mut container = LevelChunkTicks::new_with_pending(vec![
            SavedTick::new(
                "dup".to_string(),
                BlockPos::new(0, 0, 0),
                5,
                TickPriority::Normal,
            ),
            SavedTick::new(
                "dup".to_string(),
                BlockPos::new(0, 0, 0),
                5,
                TickPriority::Normal,
            ),
        ]);
        container.unpack(100); // schedules two subTickOrder-distinct ticks
        container.poll(); // drains the min, clearing the per-position entry
        // Re-admit a tick with every field matching the surviving one.
        container.schedule(sched("dup", 0, 0, 105, TickPriority::Normal, -1));
        // The heap now holds two value-identical ticks (the surviving unpacked
        // one and the re-admitted one).
        let surviving = container.tick_queue.queue[0].clone();
        assert!(
            container
                .tick_queue
                .queue
                .iter()
                .filter(|e| **e == surviving)
                .count()
                == 2,
            "precondition: the heap holds two value-identical ticks"
        );
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            container.tick_queue.remove_eq(&surviving);
        }));
        assert!(
            panic.is_err(),
            "ambiguous remove_eq must not silently pick one of two value-identical ticks"
        );
    }

    #[test]
    fn level_chunk_ticks_dirty_surface() {
        // Moonrise dirty semantics: `dirty || (!queue.isEmpty() && tick !=
        // lastSaved)`.
        let mut container = LevelChunkTicks::new();
        // Fresh: empty queue, clean flag.
        assert!(!container.is_dirty(0));
        container.schedule(sched("a", 0, 0, 100, TickPriority::Normal, 0));
        // Scheduled marks dirty.
        assert!(container.is_dirty(0));
        container.clear_dirty();
        // Clean flag, but a queued tick was never saved at tick 0 => still
        // dirty (the queue half of the condition, matching Paper).
        assert!(container.is_dirty(0));
        assert!(container.is_dirty(1));
        // pack records lastSaved = 1; now the queue was saved at 1.
        container.pack(1);
        assert!(container.is_dirty(0));
        assert!(!container.is_dirty(1));
    }

    #[test]
    fn level_chunk_ticks_on_tick_added_hook_fires_with_exact_element() {
        // Java's onTickAdded receives the exact offered element. We verify it
        // is the same type/pos we scheduled (not the heap min).
        use std::sync::Arc as StdArc;
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

        let mut container = LevelChunkTicks::new();
        let calls = StdArc::new(AtomicUsize::new(0));
        let seen = StdArc::new(AtomicUsize::new(0));
        let calls_hook = calls.clone();
        let seen_hook = seen.clone();
        container.set_on_tick_added(Some(StdArc::new(
            move |_container: &LevelChunkTicks<String>, tick: &ScheduledTick<String>| {
                calls_hook.fetch_add(1, AtomicOrdering::SeqCst);
                if tick.r#type == "first" {
                    seen_hook.store(1, AtomicOrdering::SeqCst);
                }
            },
        )));
        container.schedule(sched("second", 5, 5, 10, TickPriority::Normal, 1));
        container.schedule(sched("first", 0, 0, 5, TickPriority::Normal, 0));
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
        assert_eq!(seen.load(AtomicOrdering::SeqCst), 1);
    }

    // -----------------------------------------------------------------------
    // ProtoChunkTicks (#522)
    // -----------------------------------------------------------------------

    #[test]
    fn proto_chunk_ticks_schedule_converts_to_zero_delay_stored() {
        let mut container = ProtoChunkTicks::new();
        container.schedule(sched("stone", 0, 0, 100, TickPriority::Low, 3));
        assert_eq!(container.count(), 1);
        assert!(container.has_scheduled_tick(&BlockPos::new(0, 0, 0), &"stone".to_string()));
        let stored = container.scheduled_ticks();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].r#type, "stone");
        assert_eq!(stored[0].delay, 0);
        assert_eq!(stored[0].priority, TickPriority::Low);
        // pack ignores currentTick.
        assert_eq!(container.pack(1234), stored);
    }

    #[test]
    fn proto_chunk_ticks_load_dedupes_and_preserves_insertion_order() {
        let stored = vec![
            SavedTick::new(
                "a".to_string(),
                BlockPos::new(0, 0, 0),
                5,
                TickPriority::Normal,
            ),
            SavedTick::new(
                "b".to_string(),
                BlockPos::new(1, 0, 0),
                6,
                TickPriority::Low,
            ),
            SavedTick::new(
                "a".to_string(),
                BlockPos::new(0, 0, 0),
                7,
                TickPriority::High,
            ), // dup
        ];
        let container = ProtoChunkTicks::load(&stored);
        assert_eq!(container.count(), 2);
        let ticks = container.scheduled_ticks();
        assert_eq!(
            ticks.iter().map(|t| t.r#type.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        // The surviving 'a' is the first occurrence (delay 5, Normal).
        assert_eq!(ticks[0].delay, 5);
    }

    // -----------------------------------------------------------------------
    // Loaded-world fixture grounding (#522): reconstruct the real stored tick
    // shapes and run them through the containers.
    // -----------------------------------------------------------------------

    /// Read a radius-1 loaded-world auxiliary-data fixture (issue #371).
    fn loaded_world_fixture(name: &str) -> rivet_nbt::compound_tag::CompoundTag {
        use rivet_nbt::nbt_accounter::NbtAccounter;
        use rivet_nbt::nbt_io;
        use rivet_util::DataInputStream;
        use std::io::Cursor;
        use std::path::PathBuf;

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk")
            .join(name);
        let bytes = std::fs::read(path).expect("Paper 26.2 loaded-world chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    #[test]
    fn loaded_world_block_tick_container_carries_and_unpacks_exact_tick() {
        // The `-17.-19.nbt` fixture carries one sand block_ticks entry at
        // (-268, 61, -302) with delay -59, Normal. Mirror `SerializableChunkData`
        // decoding: filterTickListForChunk on the stored list, then build a
        // LevelChunkTicks and unpack at a given current tick.
        use crate::chunk::registry_codecs::block_by_name_codec;
        use crate::ticks::{filter_tick_list_for_chunk, saved_tick_codec};
        use rivet_nbt::nbt_ops::NbtOps;

        let root = loaded_world_fixture("-17.-19.nbt");
        let list = root.get_list_or_empty("block_ticks");
        let ops = NbtOps::instance();
        let tag = Tag::List(list);
        let decoded: Vec<SavedTick<crate::block::Block>> =
            codec::list(saved_tick_codec(block_by_name_codec::<NbtOps>()))
                .parse(&ops, &tag)
                .result_or_partial_silent()
                .expect("stored block_ticks decode");
        let filtered = filter_tick_list_for_chunk(&decoded, &ChunkPos::new(-17, -19));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].r#type.name(), "minecraft:sand");
        assert_eq!(filtered[0].pos, BlockPos::new(-268, 61, -302));
        assert_eq!(filtered[0].delay, -59);

        // Load into the runtime container and unpack.
        let mut container = LevelChunkTicks::new_with_pending(filtered);
        // current tick such that the absolute trigger is deterministic.
        container.unpack(1000);
        let first = container.poll().expect("the stored tick unpacks");
        assert_eq!(first.r#type.name(), "minecraft:sand");
        assert_eq!(first.pos, BlockPos::new(-268, 61, -302));
        assert_eq!(first.trigger_tick, 941);
        assert_eq!(first.priority, TickPriority::Normal);
        assert_eq!(first.sub_tick_order, -1);
    }

    #[test]
    fn loaded_world_fluid_tick_container_carries_and_packs_exact_tick() {
        // The `-2.-2.nbt` fixture carries one water fluid_ticks entry at
        // (-27, 59, -17) with delay 2, Normal.
        use crate::chunk::registry_codecs::fluid_by_name_codec;
        use crate::ticks::{filter_tick_list_for_chunk, saved_tick_codec};
        use rivet_nbt::nbt_ops::NbtOps;
        use rivet_registry::fluid_id::FluidId;

        let root = loaded_world_fixture("-2.-2.nbt");
        let list = root.get_list_or_empty("fluid_ticks");
        let ops = NbtOps::instance();
        let tag = Tag::List(list);
        let decoded: Vec<SavedTick<FluidId>> =
            codec::list(saved_tick_codec(fluid_by_name_codec::<NbtOps>()))
                .parse(&ops, &tag)
                .result_or_partial_silent()
                .expect("stored fluid_ticks decode");
        let filtered = filter_tick_list_for_chunk(&decoded, &ChunkPos::new(-2, -2));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].r#type, FluidId::WATER);
        assert_eq!(filtered[0].pos, BlockPos::new(-27, 59, -17));
        assert_eq!(filtered[0].delay, 2);

        let mut container = LevelChunkTicks::new_with_pending(filtered);
        // Pack BEFORE unpacking: a freshly-loaded container packs back the
        // pending stored ticks unchanged (delay 2, water).
        let packed = container.pack(0);
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].r#type, FluidId::WATER);
        assert_eq!(packed[0].delay, 2);
        assert_eq!(packed[0].pos, BlockPos::new(-27, 59, -17));

        container.unpack(500);
        let first = container.poll().expect("the stored tick unpacks");
        assert_eq!(first.r#type, FluidId::WATER);
        assert_eq!(first.trigger_tick, 502);
        assert_eq!(first.sub_tick_order, -1);
        // Repacking at the same tick (before draining anything further)
        // reproduces the stored shape (delay 2).
        // Note: pack after the single tick was polled would be empty (Java
        // drains it out of the queue), so this is exercised in the pack path.
    }

    #[test]
    fn loaded_world_proto_container_loads_stored_tick_shape() {
        // Build the proto container the way `ProtoChunk` does from a stored
        // list, and confirm `scheduledTicks()` reproduces the stored shape.
        use crate::chunk::registry_codecs::block_by_name_codec;
        use crate::ticks::{filter_tick_list_for_chunk, saved_tick_codec};
        use rivet_nbt::nbt_ops::NbtOps;

        let root = loaded_world_fixture("-17.-19.nbt");
        let list = root.get_list_or_empty("block_ticks");
        let ops = NbtOps::instance();
        let tag = Tag::List(list);
        let decoded: Vec<SavedTick<crate::block::Block>> =
            codec::list(saved_tick_codec(block_by_name_codec::<NbtOps>()))
                .parse(&ops, &tag)
                .result_or_partial_silent()
                .expect("stored block_ticks decode");
        let filtered = filter_tick_list_for_chunk(&decoded, &ChunkPos::new(-17, -19));
        let proto = ProtoChunkTicks::load(&filtered);
        let ticks = proto.scheduled_ticks();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].r#type.name(), "minecraft:sand");
        assert_eq!(ticks[0].delay, -59);
    }
}

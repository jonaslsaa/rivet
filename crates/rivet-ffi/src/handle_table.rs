//! Generational handle-table design for `rivet-ffi`.
//!
//! # Handle-Table Architecture (`OWNERSHIP.md` §JVM-adapter)
//!
//! Foreign callers (Java FFM/Panama shims or C hosts) never receive raw arena
//! pointers or Rust references. Every object exposed across `rivet-ffi` is
//! addressed by a fixed-width `(slot: u32, generation: u32)` pair:
//!
//! - `generation == 0` is reserved as invalid so zero-initialized structs never
//!   alias a live entry.
//! - Each slot stores a monotonically incrementing non-zero `generation` and an
//!   `Option<T>`.
//! - Removing an entry increments the slot's generation (wrapping `u32::MAX -> 1`)
//!   and pushes the slot index onto a LIFO free list.
//! - Re-inserting into a recycled slot issues the incremented generation, so
//!   any caller retaining the previous `(slot, old_generation)` deterministically
//!   observes [`HandleLookupError::Stale`] rather than ABA aliasing.
//! - Capacity is explicitly bounded (`max_slots`) so untrusted callers cannot
//!   exhaust memory without hitting [`HandleInsertError::CapacityExceeded`].

/// Failure returned when resolving a `(slot, generation)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleLookupError {
    /// `generation == 0` or `slot` was never allocated.
    Invalid,
    /// Slot was freed or recycled with a newer generation.
    Stale,
}

/// Failure returned when inserting into a bounded [`GenerationalTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleInsertError {
    CapacityExceeded,
}

struct SlotEntry<T> {
    generation: u32,
    value: Option<T>,
}

/// Bounded generational slot table with O(1) insert, lookup, and removal.
pub struct GenerationalTable<T> {
    slots: Vec<SlotEntry<T>>,
    free_list: Vec<u32>,
    max_slots: u32,
    live_count: u32,
}

impl<T> GenerationalTable<T> {
    pub fn new(max_slots: u32) -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
            max_slots,
            live_count: 0,
        }
    }

    /// Inserts `value` and returns its `(slot, generation)` handle.
    pub fn insert(&mut self, value: T) -> Result<(u32, u32), HandleInsertError> {
        if let Some(slot_idx) = self.free_list.pop() {
            let entry = &mut self.slots[slot_idx as usize];
            debug_assert!(entry.value.is_none());
            entry.value = Some(value);
            self.live_count += 1;
            return Ok((slot_idx, entry.generation));
        }

        let next_slot =
            u32::try_from(self.slots.len()).map_err(|_| HandleInsertError::CapacityExceeded)?;
        if next_slot >= self.max_slots {
            return Err(HandleInsertError::CapacityExceeded);
        }

        self.slots.push(SlotEntry {
            generation: 1,
            value: Some(value),
        });
        self.live_count += 1;
        Ok((next_slot, 1))
    }

    /// Resolves `(slot, generation)` to an immutable reference.
    pub fn get(&self, slot: u32, generation: u32) -> Result<&T, HandleLookupError> {
        if generation == 0 {
            return Err(HandleLookupError::Invalid);
        }
        let entry = self
            .slots
            .get(slot as usize)
            .ok_or(HandleLookupError::Invalid)?;
        if entry.generation != generation {
            return Err(HandleLookupError::Stale);
        }
        entry.value.as_ref().ok_or(HandleLookupError::Stale)
    }

    /// Resolves `(slot, generation)` to a mutable reference.
    pub fn get_mut(&mut self, slot: u32, generation: u32) -> Result<&mut T, HandleLookupError> {
        if generation == 0 {
            return Err(HandleLookupError::Invalid);
        }
        let entry = self
            .slots
            .get_mut(slot as usize)
            .ok_or(HandleLookupError::Invalid)?;
        if entry.generation != generation {
            return Err(HandleLookupError::Stale);
        }
        entry.value.as_mut().ok_or(HandleLookupError::Stale)
    }

    /// Removes the entry at `(slot, generation)`, advancing the slot's
    /// generation and recycling the slot index.
    pub fn remove(&mut self, slot: u32, generation: u32) -> Result<T, HandleLookupError> {
        if generation == 0 {
            return Err(HandleLookupError::Invalid);
        }
        let entry = self
            .slots
            .get_mut(slot as usize)
            .ok_or(HandleLookupError::Invalid)?;
        if entry.generation != generation {
            return Err(HandleLookupError::Stale);
        }
        let value = entry.value.take().ok_or(HandleLookupError::Stale)?;
        entry.generation = next_generation(entry.generation);
        self.free_list.push(slot);
        self.live_count -= 1;
        Ok(value)
    }

    pub fn live_count(&self) -> u32 {
        self.live_count
    }
}

#[inline]
fn next_generation(current: u32) -> u32 {
    let next = current.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

//! Moonrise Delayed26WayDistancePropagator3D, including its 2D LevelMap
//! conditional-write semantics, delayed linked source updates and priority
//! FIFO increase/decrease queues. Internal byte maps are lookup-only: their
//! hash-table iteration order is never consumed by the source algorithm.
use indexmap::IndexSet;
use rivet_registry::core::SectionPos;
use std::collections::{HashMap, VecDeque};

#[cfg(test)]
#[path = "distance_fixture.rs"]
mod tests;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DistancePropagationError {
    #[error("Level must be in (0, 63], not {0}")]
    InvalidLevel(i32),
}
type ChangeCallback = Box<dyn FnMut(i64, i8, i8) + Send>;
pub struct Delayed26WayDistancePropagator3D {
    levels: HashMap<i64, i8>,
    sources: HashMap<i64, i8>,
    updated_sources: IndexSet<i64>,
    increase: [VecDeque<(i64, i8)>; 64],
    remove: [VecDeque<(i64, i8)>; 64],
    increase_bits: u64,
    remove_bits: u64,
    callback: Option<ChangeCallback>,
}
impl Default for Delayed26WayDistancePropagator3D {
    fn default() -> Self {
        Self {
            levels: HashMap::new(),
            sources: HashMap::new(),
            updated_sources: IndexSet::new(),
            increase: std::array::from_fn(|_| VecDeque::new()),
            remove: std::array::from_fn(|_| VecDeque::new()),
            increase_bits: 0,
            remove_bits: 0,
            callback: None,
        }
    }
}
impl Delayed26WayDistancePropagator3D {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_callback(callback: impl FnMut(i64, i8, i8) + Send + 'static) -> Self {
        Self {
            callback: Some(Box::new(callback)),
            ..Self::default()
        }
    }
    pub fn get_level(&self, coordinate: i64) -> i32 {
        i32::from(*self.levels.get(&coordinate).unwrap_or(&0))
    }
    pub fn get_level_at(&self, x: i32, y: i32, z: i32) -> i32 {
        self.get_level(SectionPos::as_long(x, y, z))
    }
    pub fn set_source(
        &mut self,
        coordinate: i64,
        level: i32,
    ) -> Result<(), DistancePropagationError> {
        if level == 0 || level & 63 != level {
            return Err(DistancePropagationError::InvalidLevel(level));
        }
        let previous = self.sources.insert(coordinate, level as i8).unwrap_or(0);
        if previous != level as i8 {
            self.updated_sources.insert(coordinate);
        }
        Ok(())
    }
    pub fn set_source_at(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        level: i32,
    ) -> Result<(), DistancePropagationError> {
        self.set_source(SectionPos::as_long(x, y, z), level)
    }
    pub fn remove_source(&mut self, coordinate: i64) {
        if self.sources.remove(&coordinate).unwrap_or(0) != 0 {
            self.updated_sources.insert(coordinate);
        }
    }
    pub fn remove_source_at(&mut self, x: i32, y: i32, z: i32) {
        self.remove_source(SectionPos::as_long(x, y, z));
    }
    fn enqueue_increase(&mut self, coordinate: i64, index: i8, level: i8) {
        self.increase[index as usize].push_back((coordinate, level));
        self.increase_bits |= 1_u64 << (index as u32);
    }
    fn enqueue_remove(&mut self, coordinate: i64, level: i8) {
        self.remove[level as usize].push_back((coordinate, level));
        self.remove_bits |= 1_u64 << (level as u32);
    }
    fn notify(&mut self, coordinate: i64, old: i8, new: i8) {
        if let Some(callback) = &mut self.callback {
            callback(coordinate, old, new);
        }
    }
    /// Source only reports whether a queued source's current propagated level
    /// differs from its final source level, not whether a setter was called.
    pub fn propagate_updates(&mut self) -> bool {
        if self.updated_sources.is_empty() {
            return false;
        }
        let mut changed = false;
        // Taking a snapshot preserves linked-set order while queueing mutates
        // disjoint fields. Source clears the linked set only AFTER this loop.
        let updates: Vec<_> = self.updated_sources.iter().copied().collect();
        for coordinate in updates {
            let current = *self.levels.get(&coordinate).unwrap_or(&0);
            let updated = *self.sources.get(&coordinate).unwrap_or(&0);
            if current == updated {
                continue;
            }
            changed = true;
            if updated > current {
                self.enqueue_increase(coordinate, updated, updated);
            } else {
                self.enqueue_remove(coordinate, current);
            }
        }
        self.updated_sources.clear();
        self.propagate_increases();
        self.propagate_decreases();
        changed
    }
    fn neighbours(coordinate: i64, mut emit: impl FnMut(i64)) {
        let x = SectionPos::x_of(coordinate);
        let y = SectionPos::y_of(coordinate);
        let z = SectionPos::z_of(coordinate);
        for dy in -1_i32..=1 {
            for dz in -1_i32..=1 {
                for dx in -1_i32..=1 {
                    if dy | dz | dx == 0 {
                        continue;
                    }
                    emit(SectionPos::as_long(
                        x.wrapping_add(dx),
                        y.wrapping_add(dy),
                        z.wrapping_add(dz),
                    ));
                }
            }
        }
    }
    fn propagate_increases(&mut self) {
        while self.increase_bits != 0 {
            let index = 63 - self.increase_bits.leading_zeros();
            while let Some((coordinate, queued)) = self.increase[index as usize].pop_front() {
                let neighbour_check = queued < 0;
                let level = if neighbour_check { -queued } else { queued };
                let current = *self.levels.get(&coordinate).unwrap_or(&0);
                // LevelMap.putIfGreater returns the OLD value regardless of
                // whether it writes; neighbour-check mode performs no write.
                if !neighbour_check && level > current {
                    self.levels.insert(coordinate, level);
                }
                if if neighbour_check {
                    current != level
                } else {
                    current >= level
                } {
                    continue;
                }
                self.notify(coordinate, current, level);
                if level == 1 {
                    continue;
                }
                Self::neighbours(coordinate, |neighbour| {
                    self.enqueue_increase(neighbour, level - 1, level - 1)
                });
            }
            self.increase_bits ^= 1_u64 << index;
        }
    }
    fn propagate_decreases(&mut self) {
        while self.remove_bits != 0 {
            let index = 63 - self.remove_bits.leading_zeros();
            while let Some((coordinate, level)) = self.remove[index as usize].pop_front() {
                let current = *self.levels.get(&coordinate).unwrap_or(&0);
                // LevelMap.removeIfGreaterOrEqual compares the supplied removal
                // level against current, but always returns current unchanged.
                if level >= current {
                    self.levels.remove(&coordinate);
                }
                if current == 0 {
                    continue;
                }
                if current > level {
                    self.enqueue_increase(coordinate, current, -current);
                    continue;
                }
                self.notify(coordinate, current, 0);
                let source = *self.sources.get(&coordinate).unwrap_or(&0);
                if source != 0 {
                    self.enqueue_increase(coordinate, source, source);
                }
                if level == 0 {
                    continue;
                }
                Self::neighbours(coordinate, |neighbour| {
                    self.enqueue_remove(neighbour, level - 1)
                });
            }
            self.remove_bits ^= 1_u64 << index;
        }
        self.propagate_increases();
    }
}

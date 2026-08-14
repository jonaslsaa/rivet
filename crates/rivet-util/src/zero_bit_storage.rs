//! Port of `net.minecraft.util.ZeroBitStorage` (MC 26.2).
//!
//! PROVENANCE: `net/minecraft/util/ZeroBitStorage.java`. Zero-width entries,
//! no backing storage. Java's `copy()` returns `this`; the Rust port returns a
//! fresh value (same observable contents). The Paper `moonrise$countEntries`
//! fast path is ported as [`BitStorage::count_entries`] (issue #216).

use crate::bit_storage::BitStorage;

/// `net.minecraft.util.ZeroBitStorage`.
#[derive(Debug, Clone, Copy)]
pub struct ZeroBitStorage {
    size: usize,
}

impl ZeroBitStorage {
    /// `ZeroBitStorage(int size)`.
    pub fn new(size: usize) -> Self {
        ZeroBitStorage { size }
    }
}

impl BitStorage for ZeroBitStorage {
    fn get_and_set(&mut self, _index: usize, _value: i32) -> i32 {
        0
    }

    fn set(&mut self, _index: usize, _value: i32) {}

    fn get(&self, _index: usize) -> i32 {
        0
    }

    fn get_raw(&self) -> &[i64] {
        &[]
    }

    fn get_raw_mut(&mut self) -> &mut [i64] {
        &mut []
    }

    fn get_size(&self) -> usize {
        self.size
    }

    fn get_bits(&self) -> i32 {
        0
    }

    fn get_all(&self, output: &mut dyn FnMut(i32)) {
        for _ in 0..self.size {
            output(0);
        }
    }

    fn unpack(&self, output: &mut [i32]) {
        for slot in output.iter_mut().take(self.size) {
            *slot = 0;
        }
    }

    fn copy_box(&self) -> Box<dyn BitStorage + Send + Sync> {
        Box::new(*self)
    }

    /// `moonrise$countEntries()` — every entry is palette id 0, so the single
    /// list holds `0..size`. Java materializes the list directly (a wrapped
    /// array); the Rust port builds the same ascending `i16` indices.
    fn count_entries(&self) -> Vec<(i32, Vec<i16>)> {
        vec![(0, (0..self.size).map(|i| i as i16).collect())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_zero() {
        let mut z = ZeroBitStorage::new(4096);
        assert_eq!(z.get_bits(), 0);
        assert_eq!(z.get_size(), 4096);
        assert!(z.get_raw().is_empty());
        for i in [0usize, 1, 4095] {
            assert_eq!(z.get(i), 0);
            assert_eq!(z.get_and_set(i, 7), 0);
            z.set(i, 7);
            assert_eq!(z.get(i), 0);
        }
        let mut unpacked = vec![-1i32; 4096];
        z.unpack(&mut unpacked);
        assert!(unpacked.iter().all(|&v| v == 0));
    }

    #[test]
    fn get_all_emits_size_zeros() {
        let z = ZeroBitStorage::new(5);
        let mut seen = 0;
        z.get_all(&mut |v| {
            assert_eq!(v, 0);
            seen += 1;
        });
        assert_eq!(seen, 5);
    }

    #[test]
    fn copy_preserves_size() {
        let z = ZeroBitStorage::new(4096);
        let c = z.copy_box();
        assert_eq!(c.get_size(), 4096);
        assert_eq!(c.get_bits(), 0);
    }

    /// `moonrise$countEntries` (issue #216): zero-width entries are all palette
    /// id 0, so the single group holds every index, ascending.
    #[test]
    fn count_entries_single_group_of_all_indices() {
        let z = ZeroBitStorage::new(16);
        assert_eq!(
            z.count_entries(),
            vec![(0, (0..16).map(|i| i as i16).collect())]
        );
    }

    #[test]
    fn count_entries_empty_storage_still_has_id_zero_group() {
        // Java's `ZeroBitStorage.moonrise$countEntries` wraps an empty array
        // and puts it under id 0, so size 0 yields one empty group — the map
        // always has the `{0: []}` entry.
        assert_eq!(ZeroBitStorage::new(0).count_entries(), vec![(0, vec![])]);
    }
}

//! Port of `net.minecraft.util.BitStorage` (MC 26.2).
//!
//! PROVENANCE: `net/minecraft/util/BitStorage.java` in `working/Paper`
//! (vanilla 26.2 + Paper patches). The Paper `BlockCountingBitStorage`
//! (Moonrise) extension (`moonrise$countEntries`) is ported as
//! [`BitStorage::count_entries`] (issue #216).
//!
//! The wire format depends on this interface's exact packed-long layout:
//! `PalettedContainer`/`Data.write` emits `getBits()` (one byte) followed by
//! `getRaw()` (the packed `long[]`, big-endian on the wire).

/// `net.minecraft.util.BitStorage` — a packed fixed-width entry array.
///
/// Entries are `getBits()` bits wide, addressed by linear index. The concrete
/// layouts mirror Java exactly:
///
/// - [`SimpleBitStorage`]: entries packed into `ceil(size * bits / 64)`
///   `u64`s. Entry `i` lives in cell `i / valuesPerLong` at bit offset
///   `(i % valuesPerLong) * bits`, where `valuesPerLong = 64 / bits`. This is
///   the layout produced by Java's constructor (`value[cell*vpl + k]` in the
///   `k`-th `bits`-wide slot, slot 0 in the low bits).
/// - [`ZeroBitStorage`]: zero-width entries, no backing storage.
use std::collections::HashMap;

pub trait BitStorage: Send {
    /// `getAndSet(int index, int value)` — writes `value`, returns the prior
    /// entry. Java arithmetic on `int`/`long` is wrapping; the bit masks here
    /// are applied on `u64` with logical shifts.
    fn get_and_set(&mut self, index: usize, value: i32) -> i32;

    /// `set(int index, int value)`.
    fn set(&mut self, index: usize, value: i32);

    /// `get(int index)`.
    fn get(&self, index: usize) -> i32;

    /// `getRaw()` — the packed backing array.
    fn get_raw(&self) -> &[i64];

    /// `getRaw()` on the write path mutates the backing array in place
    /// (`FriendlyByteBuf.readFixedSizeLongArray(storage.getRaw())`), so the
    /// Rust port exposes a `&mut` variant as well.
    fn get_raw_mut(&mut self) -> &mut [i64];

    /// `getSize()`.
    fn get_size(&self) -> usize;

    /// `getBits()` — the entry width in bits (also the wire palette-bits byte
    /// for the owning container).
    fn get_bits(&self) -> i32;

    /// `getAll(IntConsumer)` — visits every entry, index order.
    fn get_all(&self, output: &mut dyn FnMut(i32));

    /// `unpack(int[] output)` — writes every entry into `output[0..size]`.
    fn unpack(&self, output: &mut [i32]);

    /// `copy()` — a fresh storage with identical contents. Java returns `this`
    /// for [`ZeroBitStorage`] and a clone for [`SimpleBitStorage`]; the Rust
    /// port always returns an owned fresh value (Java's shared `this` is an
    /// aliasing optimization that is unobservable on the wire).
    fn copy_box(&self) -> Box<dyn BitStorage>;

    /// `moonrise$countEntries()` — the Paper/Moonrise block-counting
    /// extension. Maps every distinct palette-local id to the ascending list
    /// of storage indices holding it, in first-appearance order of the ids
    /// (the map's insertion order in Java).
    ///
    /// This is the default implementation from `BitStorage.java`
    /// (`computeIfAbsent` over `get(index)`, kept as a compatibility default
    /// for third-party `BitStorage` impls). [`SimpleBitStorage`] and
    /// [`ZeroBitStorage`] override it with Moonrise's fast paths.
    fn count_entries(&self) -> Vec<(i32, Vec<i16>)> {
        let size = self.get_size();
        let mut order: Vec<i32> = Vec::new();
        let mut lists: HashMap<i32, Vec<i16>> = HashMap::new();
        for index in 0..size {
            let palette_idx = self.get(index);
            lists
                .entry(palette_idx)
                .or_insert_with(|| {
                    order.push(palette_idx);
                    Vec::new()
                })
                .push(index as i16);
        }
        order
            .into_iter()
            .map(|id| (id, lists.remove(&id).unwrap()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal third-party-style storage (no `count_entries` override) so the
    /// trait default — Java's `BitStorage.moonrise$countEntries` fallback for
    /// mods that implement the interface — is exercised directly.
    struct FakeStorage {
        entries: Vec<i32>,
    }

    impl BitStorage for FakeStorage {
        fn get_and_set(&mut self, index: usize, value: i32) -> i32 {
            let prev = self.entries[index];
            self.entries[index] = value;
            prev
        }
        fn set(&mut self, index: usize, value: i32) {
            self.entries[index] = value;
        }
        fn get(&self, index: usize) -> i32 {
            self.entries[index]
        }
        fn get_raw(&self) -> &[i64] {
            &[]
        }
        fn get_raw_mut(&mut self) -> &mut [i64] {
            &mut []
        }
        fn get_size(&self) -> usize {
            self.entries.len()
        }
        fn get_bits(&self) -> i32 {
            4
        }
        fn get_all(&self, output: &mut dyn FnMut(i32)) {
            for &e in &self.entries {
                output(e);
            }
        }
        fn unpack(&self, output: &mut [i32]) {
            output.copy_from_slice(&self.entries);
        }
        fn copy_box(&self) -> Box<dyn BitStorage> {
            Box::new(FakeStorage {
                entries: self.entries.clone(),
            })
        }
    }

    #[test]
    fn default_count_entries_groups_by_get_value_in_first_appearance_order() {
        let s = FakeStorage {
            entries: vec![3, 1, 3, 3, 1, 5],
        };
        assert_eq!(
            s.count_entries(),
            vec![(3, vec![0, 2, 3]), (1, vec![1, 4]), (5, vec![5]),]
        );
    }
}

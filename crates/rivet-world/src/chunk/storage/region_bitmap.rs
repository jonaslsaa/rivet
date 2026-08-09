//! Port of `net.minecraft.world.level.chunk.storage.RegionBitmap` (MC 26.2).
//!
//! The per-`RegionFile` sector-allocation tracker — an exact
//! `java.util.BitSet`-equivalent (§3 of `docs/region-file-format-spec.md`).
//! Sector allocation is derived per-file state (never global): it is replayed
//! from the header at open time and mutated as chunks are written/cleared.
//!
//! Faithfulness notes (all observable in `RegionFile`'s allocation order,
//! which drives on-disk sector placement):
//!
//! - `try_allocate` keeps Paper's `firstSet > 0` guard verbatim: a used bit at
//!   index exactly `0` is ignored (unreachable in practice — every call site
//!   uses `start >= 2`).
//! - `allocate` is first-fit with `current` as a per-call local, restarting
//!   from sector 0 each call.
//! - `copy_from` iterates to `max(self.size(), other.size())` where `size` is
//!   the BitSet's *capacity* in bits (`words.len() * 64`), not its logical
//!   length — so copying a smaller bitmap into a larger one clears the high
//!   bits, and copying a larger one into a smaller grows the target.
//!
//! The backing store keeps the invariant that words past the highest set bit
//! are zero, so `get` beyond the store reads `false` exactly like Java's
//! `BitSet.get` past `wordsInUse`, and `next_clear_bit` past the store returns
//! the caller's index (Java's `if (u >= wordsInUse) return fromIndex`). Java's
//! separate `wordsInUse` count is therefore not tracked: its only effect is
//! bounding the `nextSetBit`/`nextClearBit` scans, which is unobservable given
//! that invariant.

const BITS_PER_WORD: usize = u64::BITS as usize;

/// `RegionBitmap` — the per-region-file sector-allocation BitSet (a
/// `java.util.BitSet` equivalent, not a wrapper around one).
pub struct RegionBitmap {
    used: Vec<u64>,
}

impl Default for RegionBitmap {
    /// `new RegionBitmap()` — an empty BitSet.
    fn default() -> Self {
        Self::new()
    }
}

impl RegionBitmap {
    /// `new RegionBitmap()`.
    pub fn new() -> Self {
        Self { used: Vec::new() }
    }

    /// `force(position, size)` — mark `[position, position + size)` used
    /// (unchecked).
    pub fn force(&mut self, position: i32, size: i32) {
        self.set_range(position, position + size);
    }

    /// `free(position, size)` — clear `[position, position + size)`.
    pub fn free(&mut self, position: i32, size: i32) {
        self.clear_range(position, position + size);
    }

    /// `tryAllocate(from, length)` — allocate exactly `[from, from + length)`
    /// only if that run contains no used bit. Keeps Paper's `firstSet > 0`
    /// guard: a used bit at index exactly `0` is ignored.
    pub fn try_allocate(&mut self, from: i32, length: i32) -> bool {
        if let Some(first_set) = self.next_set_bit(from)
            && first_set > 0
            && first_set < from + length
        {
            return false;
        }
        self.set_range(from, from + length);
        true
    }

    /// `allocate(size)` — first-fit from sector 0 (per-call local cursor),
    /// claiming the first `size` bits of the first free run at least that long.
    pub fn allocate(&mut self, size: i32) -> i32 {
        let mut current = 0;
        loop {
            let free_start = self.next_clear_bit(current);
            match self.next_set_bit(free_start) {
                Some(free_end) if free_end - free_start < size => current = free_end,
                _ => {
                    self.force(free_start, size);
                    return free_start;
                }
            }
        }
    }

    /// `copyFrom(other)` — bitwise copy over `max(self.size(), other.size())`.
    pub fn copy_from(&mut self, other: &Self) {
        let bound = self.size().max(other.size());
        for i in 0..bound {
            let bit = i as i32;
            if other.get(bit) {
                self.set(bit);
            } else {
                self.clear(bit);
            }
        }
    }

    /// BitSet `size()` — capacity in bits (`words.len() * 64`), the
    /// `copy_from` scan bound.
    fn size(&self) -> usize {
        self.used.len() * BITS_PER_WORD
    }

    /// BitSet `get(i)` — true iff bit `i` is set (false past the store).
    fn get(&self, i: i32) -> bool {
        let i = usize::try_from(i).expect("bit index must be non-negative");
        let (word, mask) = (i / BITS_PER_WORD, 1u64 << (i % BITS_PER_WORD));
        self.used.get(word).is_some_and(|w| w & mask != 0)
    }

    /// BitSet `set(i)` — set bit `i`, growing the store.
    fn set(&mut self, i: i32) {
        let i = usize::try_from(i).expect("bit index must be non-negative");
        let (word, mask) = (i / BITS_PER_WORD, 1u64 << (i % BITS_PER_WORD));
        if word >= self.used.len() {
            self.used.resize(word + 1, 0);
        }
        self.used[word] |= mask;
    }

    /// BitSet `clear(i)` — clear bit `i` (a no-op past the store, like Java).
    fn clear(&mut self, i: i32) {
        let i = usize::try_from(i).expect("bit index must be non-negative");
        let (word, mask) = (i / BITS_PER_WORD, 1u64 << (i % BITS_PER_WORD));
        if let Some(w) = self.used.get_mut(word) {
            *w &= !mask;
        }
    }

    /// BitSet `nextSetBit(from)` — first set bit at or after `from`, or `None`
    /// (Java returns `-1`).
    fn next_set_bit(&self, from: i32) -> Option<i32> {
        let from = usize::try_from(from).expect("bit index must be non-negative");
        let first_word = from / BITS_PER_WORD;
        for (offset, word) in self.used.iter().enumerate().skip(first_word) {
            let mask = if offset == first_word {
                u64::MAX << (from % BITS_PER_WORD)
            } else {
                u64::MAX
            };
            let candidate = word & mask;
            if candidate != 0 {
                let bit = offset * BITS_PER_WORD + candidate.trailing_zeros() as usize;
                return Some(i32::try_from(bit).expect("bit index fits i32"));
            }
        }
        None
    }

    /// BitSet `nextClearBit(from)` — first clear bit at or after `from`. Java
    /// never returns `-1` here: past the logical length the bit is clear, so a
    /// run that fills the store wraps to the capacity boundary (`wordsInUse *
    /// 64`, equal to `size()` when every word is set).
    fn next_clear_bit(&self, from: i32) -> i32 {
        let from = usize::try_from(from).expect("bit index must be non-negative");
        if from / BITS_PER_WORD >= self.used.len() {
            return from as i32;
        }
        let first_word = from / BITS_PER_WORD;
        for (offset, word) in self.used.iter().enumerate().skip(first_word) {
            let mask = if offset == first_word {
                u64::MAX << (from % BITS_PER_WORD)
            } else {
                u64::MAX
            };
            let candidate = !word & mask;
            if candidate != 0 {
                let bit = offset * BITS_PER_WORD + candidate.trailing_zeros() as usize;
                return i32::try_from(bit).expect("bit index fits i32");
            }
        }
        self.size() as i32
    }

    /// BitSet `set(from, to)` — set every bit in `[from, to)` (empty when
    /// `to <= from`).
    fn set_range(&mut self, from: i32, to: i32) {
        for i in from..to {
            self.set(i);
        }
    }

    /// BitSet `clear(from, to)` — clear every bit in `[from, to)` (empty when
    /// `to <= from`).
    fn clear_range(&mut self, from: i32, to: i32) {
        for i in from..to {
            self.clear(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The set bit indices within the current capacity — Java's
    /// `getUsed()` `@VisibleForTesting` stand-in.
    fn used_bits(bitmap: &RegionBitmap) -> Vec<i32> {
        (0..bitmap.size() as i32)
            .filter(|&i| bitmap.get(i))
            .collect()
    }

    #[test]
    fn force_marks_range_used() {
        let mut b = RegionBitmap::new();
        b.force(0, 2);
        assert_eq!(used_bits(&b), vec![0, 1]);
    }

    #[test]
    fn force_grows_backing_store() {
        let mut b = RegionBitmap::new();
        b.force(10, 3);
        assert_eq!(used_bits(&b), vec![10, 11, 12]);
        assert!(!b.get(9));
        assert!(!b.get(13));
    }

    #[test]
    fn free_clears_only_the_range() {
        let mut b = RegionBitmap::new();
        b.force(0, 8);
        b.free(2, 3);
        assert_eq!(used_bits(&b), vec![0, 1, 5, 6, 7]);
    }

    #[test]
    fn free_out_of_bounds_is_noop() {
        let mut b = RegionBitmap::new();
        b.free(100, 5);
        assert_eq!(used_bits(&b), Vec::<i32>::new());
    }

    #[test]
    fn allocate_is_first_fit_from_zero() {
        let mut b = RegionBitmap::new();
        assert_eq!(b.allocate(1), 0);
        assert_eq!(b.allocate(1), 1);
        assert_eq!(b.allocate(1), 2);
        assert_eq!(used_bits(&b), vec![0, 1, 2]);
    }

    #[test]
    fn allocate_skips_reserved_header_sectors() {
        let mut b = RegionBitmap::new();
        b.force(0, 2); // the header reserves sectors 0 and 1
        assert_eq!(b.allocate(3), 2);
        assert_eq!(used_bits(&b), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn allocate_jumps_past_a_run_that_is_too_small() {
        let mut b = RegionBitmap::new();
        b.force(0, 1);
        b.force(3, 1);
        // Free run [1, 3) has length 2 < 3, so the scan skips it (`current`
        // jumps to freeEnd = 3); the next free run [4, ∞) has no end, so it is
        // allocated at sector 4.
        assert_eq!(b.allocate(3), 4);
        assert_eq!(used_bits(&b), vec![0, 3, 4, 5, 6]);
    }

    #[test]
    fn allocate_wraps_to_capacity_when_a_full_word_is_contiguous() {
        let mut b = RegionBitmap::new();
        b.force(0, 64);
        // Word 0 is fully set, so `nextClearBit` finds nothing in it and
        // returns the capacity boundary (Java's `wordsInUse * 64`).
        assert_eq!(b.allocate(1), 64);
        assert_eq!(used_bits(&b), (0..65).collect::<Vec<_>>());
    }

    #[test]
    fn allocate_prefers_the_earliest_sufficient_run() {
        let mut b = RegionBitmap::new();
        b.force(0, 1);
        b.force(5, 1);
        assert_eq!(b.allocate(2), 1);
        assert_eq!(used_bits(&b), vec![0, 1, 2, 5]);
    }

    #[test]
    fn allocate_reuses_freed_sectors() {
        let mut b = RegionBitmap::new();
        b.force(0, 4);
        b.free(1, 2);
        assert_eq!(b.allocate(1), 1);
        assert_eq!(used_bits(&b), vec![0, 1, 3]);
    }

    #[test]
    fn allocate_returns_strictly_increasing_sectors() {
        let mut b = RegionBitmap::new();
        let runs: Vec<i32> = (0..4).map(|_| b.allocate(2)).collect();
        assert_eq!(runs, vec![0, 2, 4, 6]);
    }

    #[test]
    fn try_allocate_succeeds_on_a_free_run() {
        let mut b = RegionBitmap::new();
        b.force(0, 2);
        assert!(b.try_allocate(2, 3));
        assert_eq!(used_bits(&b), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn try_allocate_fails_when_a_used_bit_is_inside_the_run() {
        let mut b = RegionBitmap::new();
        b.force(0, 2);
        b.force(5, 1);
        assert!(!b.try_allocate(2, 5));
        assert_eq!(used_bits(&b), vec![0, 1, 5]);
    }

    #[test]
    fn try_allocate_succeeds_when_used_bit_sits_at_the_exclusive_end() {
        let mut b = RegionBitmap::new();
        b.force(0, 2);
        b.force(7, 1);
        // `firstSet == 7` is not `< from + length == 7`, so the run is free.
        assert!(b.try_allocate(2, 5));
        assert_eq!(used_bits(&b), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn try_allocate_fails_on_exact_overlap_from_nonzero() {
        let mut b = RegionBitmap::new();
        b.force(2, 2);
        assert!(!b.try_allocate(2, 2));
        assert_eq!(used_bits(&b), vec![2, 3]);
    }

    #[test]
    fn try_allocate_ignores_a_used_bit_at_index_zero() {
        // Paper's `firstSet > 0` guard: a used bit at exactly index 0 does not
        // block an allocation from 0 (unreachable in RegionFile, which always
        // allocates from `start >= 2`).
        let mut b = RegionBitmap::new();
        b.force(0, 1);
        assert!(b.try_allocate(0, 2));
        assert_eq!(used_bits(&b), vec![0, 1]);
    }

    #[test]
    fn copy_from_copies_set_bits() {
        let mut a = RegionBitmap::new();
        a.force(0, 2);
        a.force(10, 3);
        let mut b = RegionBitmap::new();
        b.copy_from(&a);
        assert_eq!(used_bits(&b), vec![0, 1, 10, 11, 12]);
    }

    #[test]
    fn copy_from_clears_high_bits_when_target_is_larger() {
        let mut a = RegionBitmap::new();
        a.force(0, 2);
        let mut b = RegionBitmap::new();
        b.force(0, 8);
        // `other.get` past `a`'s capacity is false, so those bits are cleared.
        b.copy_from(&a);
        assert_eq!(used_bits(&b), vec![0, 1]);
    }

    #[test]
    fn copy_from_grows_when_source_is_larger() {
        let mut a = RegionBitmap::new();
        a.force(100, 1);
        let mut b = RegionBitmap::new();
        b.copy_from(&a);
        assert_eq!(used_bits(&b), vec![100]);
    }

    #[test]
    fn copy_from_of_equal_bitmaps_is_identity() {
        let mut a = RegionBitmap::new();
        a.force(3, 4);
        let mut b = RegionBitmap::new();
        b.force(3, 4);
        b.copy_from(&a);
        assert_eq!(used_bits(&b), used_bits(&a));
    }
}

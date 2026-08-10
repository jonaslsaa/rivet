//! Port of `net.minecraft.world.level.lighting.SpatialLongSet` (MC 26.2,
//! Paper) — a long-key set that packs coordinates 64 at a time.
//!
//! Java: `SpatialLongSet.java` in `working/Paper`. It extends fastutil's
//! `LongLinkedOpenHashSet` (insertion-ordered long set) but overrides
//! `add`/`rem`/`removeFirstLong`/`isEmpty` to delegate to an inner
//! `InternalMap` (a `Long2LongLinkedOpenHashMap`) that groups long keys by
//! their high bits: a coordinate's low 2 bits of each axis are the "inner"
//! key (0..63, one bitmask position), and everything above them is the
//! "outer" key (one map entry per group).
//!
//! ## Key packing (`InternalMap` constants)
//!
//! `X_BITS = Z_BITS = Mth.log2(60_000_000) = 25`, so `Y_BITS = 64 - 25 - 25 =
//! 14`; `Y_OFFSET = 0`, `Z_OFFSET = 14`, `X_OFFSET = 39`. `OUTER_MASK = 3L <<
//! 39 | 3L | 3L << 14` clears the low 2 bits of each axis field:
//! `getOuterKey(key) = key & ~OUTER_MASK`, and `getInnerKey(key)` is the 6-bit
//! `(innerX << 4 | innerZ << 2 | innerY)` index; `getFullKey` reassembles a
//! coordinate from an outer key + inner index. The port keeps this packing
//! exactly (verified against the Paper oracle).
//!
//! ## Internal-map simplification
//!
//! Java's `InternalMap` is a full fastutil open-addressing map (probe slots,
//! `HashCommon.mix`, `link[]` doubly-linked list, `rehash` with a
//! no-shrink `minSize` override). The only *observable* behavior that map
//! contributes is the linked insertion order of outer keys: `removeFirstBit`
//! pops the oldest outer key's lowest set bit, and rehash preserves that
//! order (fastutil's documented linked-set guarantee), so the hash-probe
//! layout is behaviorally invisible. The port therefore replaces the open
//! addressing with an insertion-ordered `Vec` of `(outer_key, bitmask)` pairs
//! plus an index — same observable results for every operation, no probe
//! constants to reproduce. `SpatialLongSet.size()` throws
//! `UnsupportedOperationException` in Java, so the port omits it (there is no
//! way to ask Java for the count; `isEmpty` is all that exists).
//!
//! Java also throws `NoSuchElementException` from `removeFirstBit` on an empty
//! set; the port's `remove_first_bit` panics with the same contract.
//!
//! RivetTodo(#184): this is the `mc.world.level.lighting.core` spatial-set
//! unit. Its Java consumers (`SectionTracker`/`ChunkTracker` and the vanilla
//! `LightEngine` storages) are dead jar-surface under Starlight; the set is
//! ported as the standalone structure its own unit owns.

/// `X_BITS` — `Mth.log2(60000000)`: `ceillog2(60000000) = 26`, minus 1 (not a
/// power of two) = 25. `mth::log2` is not `const`, so the value is written out
/// with the derivation documented.
const X_BITS: i32 = 25;
/// `Z_BITS` — `Mth.log2(60000000)` = 25, as for `X_BITS`.
const Z_BITS: i32 = 25;
/// `Y_BITS` — `64 - X_BITS - Z_BITS`.
const Y_BITS: i32 = 64 - X_BITS - Z_BITS;
/// `Y_OFFSET`.
const Y_OFFSET: i32 = 0;
/// `Z_OFFSET`.
const Z_OFFSET: i32 = Y_BITS;
/// `X_OFFSET`.
const X_OFFSET: i32 = Y_BITS + Z_BITS;
/// `OUTER_MASK` — the low 2 bits of each axis field.
const OUTER_MASK: u64 = 3u64 << X_OFFSET | 3u64 | 3u64 << Z_OFFSET;

/// `SpatialLongSet` — a long-key set of coordinates with Java's exact
/// `getOuterKey`/`getInnerKey`/`getFullKey` packing and insertion-ordered
/// `remove_first_bit` semantics.
pub struct SpatialLongSet {
    /// One `(outer_key, bitmask)` entry per group, in insertion order (an
    /// outer key re-added after removal appends at the end, like Java's
    /// linked set).
    entries: Vec<(u64, u64)>,
    /// `outer_key -> entries` index.
    index: std::collections::HashMap<u64, usize>,
}

impl SpatialLongSet {
    /// `SpatialLongSet(int expected, float f)` — the capacity hints configure
    /// Java's internal map sizing (`expected / 64` entries, load factor `f`);
    /// the port's plain `Vec`/index has no capacity behavior to set, so the
    /// arguments are accepted for signature fidelity only.
    pub fn new(_expected: usize, _f: f32) -> Self {
        SpatialLongSet {
            entries: Vec::new(),
            index: std::collections::HashMap::new(),
        }
    }

    /// `InternalMap.getOuterKey(long)` — the key with the low 2 bits of each
    /// axis cleared.
    pub fn get_outer_key(key: u64) -> u64 {
        key & !OUTER_MASK
    }

    /// `InternalMap.getInnerKey(long)` — the 6-bit `(innerX << 4 | innerZ <<
    /// 2 | innerY)` index into the group's bitmask.
    pub fn get_inner_key(key: u64) -> u8 {
        let inner_x = (key >> X_OFFSET) & 3;
        let inner_y = (key >> Y_OFFSET) & 3;
        let inner_z = (key >> Z_OFFSET) & 3;
        ((inner_x << 4 | inner_z << 2 | inner_y) & 0x3F) as u8
    }

    /// `InternalMap.getFullKey(long, int)` — reassemble a coordinate from an
    /// outer key and an inner index.
    pub fn get_full_key(outer_key: u64, inner_key: u8) -> u64 {
        let inner_key = inner_key as u64;
        outer_key
            | (inner_key >> 4 & 3) << X_OFFSET
            | (inner_key >> 2 & 3) << Z_OFFSET
            | (inner_key & 3) << Y_OFFSET
    }

    /// `add(long)` — insert `key`; returns whether it was already present.
    /// Java's linked set keeps an existing element's FIFO position on a
    /// duplicate add; the port's entries are untouched when the bit is set.
    pub fn add(&mut self, key: u64) -> bool {
        let outer_key = Self::get_outer_key(key);
        let inner_key = Self::get_inner_key(key);
        let bit_mask = 1u64 << inner_key;
        if let Some(&pos) = self.index.get(&outer_key) {
            let entry = &mut self.entries[pos];
            let old_value = (entry.1 & bit_mask) != 0;
            entry.1 |= bit_mask;
            old_value
        } else {
            let pos = self.entries.len();
            self.entries.push((outer_key, bit_mask));
            self.index.insert(outer_key, pos);
            false
        }
    }

    /// `rem(long)` — remove `key`; returns whether it was present. Removing
    /// the last set bit of a group drops the group (Java's `removeFromEntry`
    /// fixPointers+shiftKeys); the remaining groups keep their order.
    pub fn rem(&mut self, key: u64) -> bool {
        let outer_key = Self::get_outer_key(key);
        let inner_key = Self::get_inner_key(key);
        let bit_mask = 1u64 << inner_key;
        let Some(&pos) = self.index.get(&outer_key) else {
            return false;
        };
        let entry = &mut self.entries[pos];
        if (entry.1 & bit_mask) == 0 {
            return false;
        }
        entry.1 &= !bit_mask;
        if entry.1 != 0 {
            return true;
        }
        // Group emptied: remove it, preserving the order of the rest.
        self.entries.remove(pos);
        self.index.remove(&outer_key);
        // Rebuild indices after the positional shift.
        for (i, (k, _)) in self.entries.iter().enumerate() {
            self.index.insert(*k, i);
        }
        true
    }

    /// `removeFirstLong()` — pop the oldest group's lowest set bit; returns
    /// the reassembled coordinate and removes it. Java throws
    /// `NoSuchElementException` on an empty set; the port panics.
    pub fn remove_first_bit(&mut self) -> u64 {
        assert!(!self.entries.is_empty(), "SpatialLongSet is empty");
        let (outer_key, mask) = self.entries[0];
        let inner_key = mask.trailing_zeros() as u8;
        let result = Self::get_full_key(outer_key, inner_key);
        let bit_mask = 1u64 << inner_key;
        let entry = &mut self.entries[0];
        entry.1 &= !bit_mask;
        if entry.1 == 0 {
            self.entries.remove(0);
            self.index.remove(&outer_key);
            for (i, (k, _)) in self.entries.iter().enumerate() {
                self.index.insert(*k, i);
            }
        }
        result
    }

    /// `isEmpty()` — `map.isEmpty()` (no groups at all, Java's contract).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packing constants the Paper oracle reports for `InternalMap`.
    #[test]
    fn packing_constants_match_paper() {
        assert_eq!(X_BITS, 25);
        assert_eq!(Z_BITS, 25);
        assert_eq!(Y_BITS, 14);
        assert_eq!(Y_OFFSET, 0);
        assert_eq!(Z_OFFSET, 14);
        assert_eq!(X_OFFSET, 39);
        // OUTER_MASK = 3L << 39 | 3L | 3L << 14.
        assert_eq!(OUTER_MASK, (3u64 << 39) | 3 | (3 << 14));
    }

    #[test]
    fn packing_round_trips() {
        // A coordinate with axis values in every field: x bits at 39, z at 14,
        // y at 0.
        let key = (7u64 << 39) | (11u64 << 14) | 5u64;
        let outer = SpatialLongSet::get_outer_key(key);
        let inner = SpatialLongSet::get_inner_key(key);
        assert_eq!(outer, (4u64 << 39) | (8u64 << 14) | 4u64);
        // innerX = 7 & 3 = 3, innerZ = 11 & 3 = 3, innerY = 5 & 3 = 1.
        assert_eq!(inner, 3 << 4 | 3 << 2 | 1);
        assert_eq!(SpatialLongSet::get_full_key(outer, inner), key);
    }

    #[test]
    fn add_rem_and_first_bit_order_match_paper() {
        let mut set = SpatialLongSet::new(256, 0.5);
        // Golden from the Paper oracle: three coordinates insert in order;
        // removeFirstBit pops the oldest group's lowest set bit first. a and b
        // do NOT share an outer group (outer(a)=0, outer(b)=0x10004, outer(c)
        // = 1L<<42), so each has its own group and the pop order is a, b, c.
        let a = (1u64 << 39) | (2u64 << 14) | 3u64;
        let b = (1u64 << 39) | (5u64 << 14) | 6u64;
        let c = (9u64 << 39) | (2u64 << 14) | 3u64;
        assert!(set.is_empty());
        assert!(!set.add(a));
        assert!(!set.add(b));
        assert!(!set.add(c));
        assert!(set.add(b)); // duplicate: bit already set
        assert!(!set.is_empty());
        assert_eq!(set.remove_first_bit(), a);
        assert_eq!(set.remove_first_bit(), b);
        assert_eq!(set.remove_first_bit(), c);
        assert!(set.is_empty());
    }

    #[test]
    fn rem_drops_emptied_groups_and_preserves_order() {
        // The `spatial.rem.*` goldens from the Paper oracle. These coordinates
        // all share ONE outer group (their differing axis bits sit inside
        // OUTER_MASK: bits 39/40 are the low 2 bits of the X field, bit 0 the
        // low 2 bits of Y), so a, b, c are three bits of the same group entry.
        // `rem(b)` clears b's bit without dropping the group; the group's
        // remaining bits pop in order a, c.
        let mut set = SpatialLongSet::new(256, 0.5);
        let a = (1u64 << 39) | 1u64;
        let b = (2u64 << 39) | 1u64;
        let c = (3u64 << 39) | 1u64;
        set.add(a);
        set.add(b);
        set.add(c);
        assert!(set.rem(b));
        assert!(!set.rem(b));
        assert_eq!(set.remove_first_bit(), a);
        assert_eq!(set.remove_first_bit(), c);
        assert!(set.is_empty());
        // Re-adding the same coordinate after its group was dropped appends a
        // fresh group at the end (Java's linked-set re-add behavior).
        set.add(a);
        set.add(c);
        assert!(set.rem(a));
        assert_eq!(set.remove_first_bit(), c);
        assert!(set.is_empty());
    }
}

//! `ca.spottedleaf.moonrise.common.util.FlatBitsetUtil` — range operations over
//! a flat `long[]` bitset (`from` inclusive, `to` exclusive).
//!
//! Java source:
//! `working/Paper/paper-server/src/main/java/ca/spottedleaf/moonrise/common/util/FlatBitsetUtil.java`.
//!
//! Java `long` shifts mask the distance with `& 63` (JLS 15.19), so every
//! shift below is `wrapping_shl`/`wrapping_shr`. The bitset words are `u64`
//! (Java `long` bit patterns); `>>>` is `u64 >>`.

const LOG2_LONG: u32 = 6;
const ALL_SET: u64 = u64::MAX;
const BITS_PER_LONG: i32 = 64;

/// `IndexOutOfBoundsException` — the `(from | to | (to - from)) < 0` guard.
fn check_range(from: i32, to: i32) {
    if (from | to | to.wrapping_sub(from)) < 0 {
        panic!("IndexOutOfBoundsException");
    }
}

/// `FlatBitsetUtil.firstSet(long[], int, int)` — `-1` when no bit is set in
/// `[from, to)`.
pub fn first_set(bitset: &[u64], from: i32, to: i32) -> i32 {
    check_range(from, to);

    let mut bitset_idx = (from as u32 >> LOG2_LONG) as usize;
    let mut bit_idx = from & !(BITS_PER_LONG - 1);

    let mut tmp = bitset[bitset_idx] & ALL_SET.wrapping_shl(from as u32);
    loop {
        if tmp != 0 {
            let ret = bit_idx | tmp.trailing_zeros() as i32;
            return if ret >= to { -1 } else { ret };
        }

        bit_idx += BITS_PER_LONG;

        if bit_idx >= to {
            return -1;
        }

        bitset_idx += 1;
        tmp = bitset[bitset_idx];
    }
}

/// `FlatBitsetUtil.firstClear(long[], int, int)` — `-1` when no bit is clear
/// in `[from, to)`.
pub fn first_clear(bitset: &[u64], from: i32, to: i32) -> i32 {
    check_range(from, to);
    // like firstSet, but invert the bitset

    let mut bitset_idx = (from as u32 >> LOG2_LONG) as usize;
    let mut bit_idx = from & !(BITS_PER_LONG - 1);

    let mut tmp = !bitset[bitset_idx] & ALL_SET.wrapping_shl(from as u32);
    loop {
        if tmp != 0 {
            let ret = bit_idx | tmp.trailing_zeros() as i32;
            return if ret >= to { -1 } else { ret };
        }

        bit_idx += BITS_PER_LONG;

        if bit_idx >= to {
            return -1;
        }

        bitset_idx += 1;
        tmp = !bitset[bitset_idx];
    }
}

/// `FlatBitsetUtil.clearRange(long[], int, int)`.
pub fn clear_range(bitset: &mut [u64], from: i32, to: i32) {
    check_range(from, to);

    if from == to {
        return;
    }

    let to = to - 1;

    let from_bitset_idx = (from as u32 >> LOG2_LONG) as usize;
    let to_bitset_idx = (to as u32 >> LOG2_LONG) as usize;

    let keep_first = !ALL_SET.wrapping_shl(from as u32);
    let keep_last = !ALL_SET.wrapping_shr(((BITS_PER_LONG - 1) ^ to) as u32);

    // Objects.checkFromToIndex(fromBitsetIdx, toBitsetIdx, bitset.length)
    assert!(
        from_bitset_idx <= to_bitset_idx && to_bitset_idx <= bitset.len(),
        "IndexOutOfBoundsException"
    );

    if from_bitset_idx == to_bitset_idx {
        // special case: need to keep both first and last
        bitset[from_bitset_idx] &= keep_first | keep_last;
    } else {
        bitset[from_bitset_idx] &= keep_first;

        for word in &mut bitset[from_bitset_idx + 1..to_bitset_idx] {
            *word = 0;
        }

        bitset[to_bitset_idx] &= keep_last;
    }
}

/// `FlatBitsetUtil.isRangeSet(long[], int, int)`.
pub fn is_range_set(bitset: &[u64], from: i32, to: i32) -> bool {
    first_clear(bitset, from, to) == -1
}

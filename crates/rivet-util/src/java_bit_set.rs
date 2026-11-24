//! `java.util.BitSet` — the subset `BitSetDiscreteVoxelShape` and the Moonrise
//! collision code use: `get`/`set`/`clear(from, to)`/`isEmpty`/`nextClearBit`/
//! `toLongArray`/`clone`, with Java's grow-on-write behaviour.
//!
//! Bit `i` lives in word `i >> 6`, bit `i & 63`. `words` may carry trailing
//! zero words after a `clear` (Java's `wordsInUse` trims them lazily);
//! `to_long_array` trims them as `BitSet.toLongArray()` does.

/// `java.util.BitSet`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    /// `new BitSet(int nbits)` — capacity for `nbits` bits.
    pub fn new(nbits: usize) -> BitSet {
        BitSet {
            words: vec![0; nbits.div_ceil(64)],
        }
    }

    fn ensure_capacity(&mut self, words_required: usize) {
        if self.words.len() < words_required {
            self.words.resize(words_required, 0);
        }
    }

    /// `BitSet.get(int)`.
    pub fn get(&self, bit_index: usize) -> bool {
        let word = bit_index >> 6;
        word < self.words.len() && (self.words[word] & (1u64 << (bit_index & 63))) != 0
    }

    /// `BitSet.set(int)`.
    pub fn set(&mut self, bit_index: usize) {
        let word = bit_index >> 6;
        self.ensure_capacity(word + 1);
        self.words[word] |= 1u64 << (bit_index & 63);
    }

    /// `BitSet.clear(int fromIndex, int toIndex)` (`toIndex` exclusive).
    pub fn clear_range(&mut self, from_index: usize, to_index: usize) {
        assert!(
            from_index <= to_index,
            "fromIndex: {from_index} > toIndex: {to_index}"
        );
        for bit in from_index..to_index {
            let word = bit >> 6;
            if word >= self.words.len() {
                break;
            }
            self.words[word] &= !(1u64 << (bit & 63));
        }
    }

    /// `BitSet.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|w| *w == 0)
    }

    /// `BitSet.nextClearBit(int)` — the first clear bit at or after
    /// `from_index` (a bit past the stored words is clear).
    pub fn next_clear_bit(&self, from_index: usize) -> usize {
        let mut bit = from_index;
        while self.get(bit) {
            bit += 1;
        }
        bit
    }

    /// `BitSet.toLongArray()` — the words up to the highest set bit.
    pub fn to_long_array(&self) -> Vec<u64> {
        let mut len = self.words.len();
        while len > 0 && self.words[len - 1] == 0 {
            len -= 1;
        }
        self.words[..len].to_vec()
    }
}

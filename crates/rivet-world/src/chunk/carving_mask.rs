//! Port of `net.minecraft.world.level.chunk.CarvingMask` (MC 26.2) — the
//! worldgen carving bitmap.
//!
//! Java: `CarvingMask.java` in `working/Paper`. One bit per `16×height×16`
//! block column (`getIndex(x, y, z) = x & 15 | (z & 15) << 4 | (y - minY) <<
//! 8`), backed by a `java.util.BitSet` whose `toLongArray`/`valueOf` give the
//! wire form (little-endian 64-bit words). `stream(ChunkPos)` maps each set
//! index back to a `BlockPos` via `pos.getBlockAt`.
//!
//! The Rust port backs the mask with a `Vec<u64>` word set — the same
//! little-endian layout `BitSet` uses, so `to_array`/`from_array` are
//! direct, and `get`/`set` index `(bit >> 6)` inside the word. [`new`]
//! sizes the array to the fixed `256 * height` bit length, keeping `set` in
//! bounds without growth. `from_array` mirrors `BitSet.valueOf`: it trims
//! trailing zero words, so `get` is false beyond them (Java's `wordsInUse`
//! bound) and `set` grows the array on demand.
//!
//! RivetTodo(#399): the carver unit's `CarveChunk` block-surface trait is
//! implemented for the worldgen `ProtoChunk` (the production CARVERS-status
//! driver `NoiseBasedChunkGenerator::apply_carvers` binds it); the ported
//! `CarvingContext` and concrete carvers consume this mask directly. Java's
//! default `additionalMask` is the lambda
//! `(x, y, z) -> false` (`CarvingMask.java` field initializer) — never `null`;
//! the port models it as `Option`, treating `None` as always-false, which
//! preserves the default semantics.

use rivet_registry::core::{BlockPos, ChunkPos};

/// `CarvingMask.Mask` — the additional-mask predicate (`setAdditionalMask`).
pub trait Mask {
    fn test(&self, x: i32, y: i32, z: i32) -> bool;
}

/// `net.minecraft.world.level.chunk.CarvingMask`.
pub struct CarvingMask {
    /// `minY` — the absolute block y of index 0.
    min_y: i32,
    /// `mask` — one bit per column, laid out as `BitSet`'s little-endian
    /// 64-bit words.
    mask: Vec<u64>,
    /// `additionalMask` — `None` is Java's default `(x, y, z) -> false`.
    additional_mask: Option<Box<dyn Mask + Send + Sync>>,
}

impl CarvingMask {
    /// `CarvingMask(int height, int minY)` — `new BitSet(256 * height)`.
    pub fn new(height: i32, min_y: i32) -> Self {
        // Java sizes the backing array `256 * height / 64`; the division is
        // exact (256 is a multiple of 64), so no rounding is needed.
        let word_count = (256 * height / 64) as usize;
        CarvingMask {
            min_y,
            mask: vec![0; word_count],
            additional_mask: None,
        }
    }

    /// `CarvingMask(long[] array, int minY)` — `BitSet.valueOf(array)`.
    ///
    /// Java's `valueOf` trims trailing zero words before copying; the port
    /// does the same, so a `from_array` mask has `wordsInUse`-style bounds
    /// exactly like Java's.
    pub fn from_array(array: &[i64], min_y: i32) -> Self {
        let trimmed = array
            .iter()
            .rposition(|w| *w != 0)
            .map_or(0, |last| last + 1);
        let words: Vec<u64> = array[..trimmed].iter().map(|w| *w as u64).collect();
        CarvingMask {
            min_y,
            mask: words,
            additional_mask: None,
        }
    }

    /// `setAdditionalMask(Mask)`.
    pub fn set_additional_mask(&mut self, additional_mask: Box<dyn Mask + Send + Sync>) {
        self.additional_mask = Some(additional_mask);
    }

    /// Borrow-preserving clone for value-boundary transactions. The optional
    /// predicate is intentionally not guessed or fabricated; callers receive
    /// `None` and can refuse rather than silently changing carving behavior.
    pub fn try_clone(&self) -> Option<Self> {
        if self.additional_mask.is_some() {
            return None;
        }
        Some(Self {
            min_y: self.min_y,
            mask: self.mask.clone(),
            additional_mask: None,
        })
    }

    /// `set(int, int, int)`.
    pub fn set(&mut self, x: i32, y: i32, z: i32) {
        let index = get_index(x, y, z, self.min_y);
        let word = index >> 6;
        if word >= self.mask.len() {
            // `BitSet.set` grows the backing array to hold the bit. This fires
            // for masks built via `from_array` with fewer words than [`new`],
            // and for a [`new`] mask when `y` is above the column (the fixed
            // `256 * height` capacity is exceeded); `BitSet.set` grows there
            // too, so the port mirrors it.
            self.mask.resize(word + 1, 0);
        }
        self.mask[word] |= 1u64 << (index & 63);
    }

    /// `get(int, int, int)` — `additionalMask.test(x, y, z) || mask.get(index)`.
    pub fn get(&self, x: i32, y: i32, z: i32) -> bool {
        if self
            .additional_mask
            .as_ref()
            .is_some_and(|m| m.test(x, y, z))
        {
            return true;
        }
        let index = get_index(x, y, z, self.min_y);
        // `BitSet.get` reads at most `wordsInUse`: a bit beyond the stored
        // words is false (it never grows the array).
        self.mask
            .get(index >> 6)
            .is_some_and(|word| word & (1u64 << (index & 63)) != 0)
    }

    /// `stream(ChunkPos)` — the set indices as `BlockPos`es at absolute y.
    ///
    /// Iterates only the words up to the last non-zero one, mirroring Java's
    /// `BitSet.stream()` bound on `wordsInUse`: a freshly-zeroed mask (for
    /// example) streams nothing rather than scanning its full word array.
    pub fn stream<'a>(&'a self, pos: &'a ChunkPos) -> impl Iterator<Item = BlockPos> + 'a {
        // `wordsInUse` equivalent — the last non-zero word + 1. `new()` keeps
        // the full `256 * height / 64` words at zero, so this trims the scan
        // to the set bits exactly like `BitSet.stream()`.
        let words_in_use = self
            .mask
            .iter()
            .rposition(|w| *w != 0)
            .map_or(0, |last| last + 1);
        self.mask[..words_in_use]
            .iter()
            .enumerate()
            .flat_map(move |(word, bits)| {
                let base = (word as i32) * 64;
                (0..64).filter_map(move |bit| {
                    if bits & (1u64 << bit) == 0 {
                        None
                    } else {
                        let index = base + bit;
                        Some(pos.get_block_at(
                            index & 15,
                            (index >> 8) + self.min_y,
                            (index >> 4) & 15,
                        ))
                    }
                })
            })
    }

    /// `toArray()` — `mask.toLongArray()`.
    pub fn to_array(&self) -> Vec<i64> {
        // `BitSet.toLongArray()` trims trailing zero words.
        let trimmed = self
            .mask
            .iter()
            .rposition(|w| *w != 0)
            .map_or(0, |last| last + 1);
        self.mask[..trimmed].iter().map(|w| *w as i64).collect()
    }
}

/// `getIndex(x, y, z)` — `x & 15 | (z & 15) << 4 | (y - minY) << 8`. The
/// subtraction and shift wrap like Java's int arithmetic (PORTING.md); a
/// `y - minY` outside `i32` wraps instead of panicking. Java then hands the
/// result to `BitSet`, which throws `IndexOutOfBoundsException` on a negative
/// bit index (a `y` below the column); the port panics with the same semantics
/// rather than sign-extending the negative int into a huge `usize` that would
/// abort on an unbounded allocation.
fn get_index(x: i32, y: i32, z: i32, min_y: i32) -> usize {
    let index = x & 15 | (z & 15) << 4 | y.wrapping_sub(min_y) << 8;
    assert!(
        index >= 0,
        "CarvingMask bit index {index} for y {y} below minY {min_y}: Java's BitSet throws IndexOutOfBoundsException"
    );
    index as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn set_below_column_panics_like_java_ioobe() {
        // Java's `BitSet.set` throws `IndexOutOfBoundsException` for a
        // negative bit index — a `y` below the column (`y < minY`). The
        // port must not sign-extend that negative index into a huge `usize`
        // and abort on an unbounded `resize`; it panics instead.
        let mut mask = CarvingMask::new(8, -64);
        mask.set(0, -65, 0);
    }

    #[test]
    fn get_index_is_java_exact() {
        // x&15 | (z&15)<<4 | (y-minY)<<8: bit (0,0,minY) is index 0; bit
        // (1,0,minY) is 1; (0,1,minY) is 16; (0,0,minY+1) is 256 (word 4,
        // bit 0).
        let mut mask = CarvingMask::new(8, -64);
        mask.set(0, -64, 0);
        mask.set(1, -64, 0);
        mask.set(0, -64, 1);
        mask.set(0, -63, 0);
        assert_eq!(
            mask.to_array(),
            vec![1i64 | (1 << 1) | (1 << 16), 0, 0, 0, 1]
        );
        assert!(mask.get(0, -64, 0));
        assert!(mask.get(1, -64, 0));
        assert!(mask.get(0, -64, 1));
        assert!(mask.get(0, -63, 0));
        assert!(!mask.get(2, -64, 0));
    }

    #[test]
    fn stream_yields_world_positions() {
        let mut mask = CarvingMask::new(4, 0);
        mask.set(3, 2, 5);
        let pos = ChunkPos::new(1, -2);
        // ChunkPos.getBlockX(3) = sectionToBlockCoordOffset(1, 3) = 1*16+3 = 19;
        // getBlockZ(5) = -2*16+5 = -27. y = 2 + minY(0) = 2.
        let blocks: Vec<BlockPos> = mask.stream(&pos).collect();
        assert_eq!(blocks.len(), 1);
        use rivet_registry::core::Vec3iLike;
        assert_eq!(blocks[0].coords(), (19, 2, -27));
    }

    #[test]
    fn additional_mask_or_behavior() {
        struct Always;
        impl Mask for Always {
            fn test(&self, _x: i32, _y: i32, _z: i32) -> bool {
                true
            }
        }
        let mut mask = CarvingMask::new(2, 0);
        assert!(!mask.get(7, 0, 7)); // no bits set, no additional mask.
        mask.set_additional_mask(Box::new(Always));
        assert!(mask.get(7, 0, 7));
        assert!(mask.get(0, 1, 0));
        // toArray unaffected by the additional mask (Java's too).
        assert_eq!(mask.to_array(), Vec::<i64>::new());
    }

    #[test]
    fn from_array_round_trips_to_array() {
        let words = vec![0x0013_0000_8000_0005i64, 0x0, 0x7];
        let mask = CarvingMask::from_array(&words, -32);
        // `BitSet.valueOf` trims trailing zero words; the middle zero here is
        // not trailing (word 2 is non-zero), so it survives the round trip.
        assert_eq!(mask.to_array(), words);
        // Word 0's set bits: 0 -> (0,0,minY), 2 -> (2,0,minY). Word 2's set
        // bits (0x7): bit 0 -> index 128 -> x=0, z=8, y=minY.
        assert!(mask.get(0, -32, 0));
        assert!(mask.get(2, -32, 0));
        assert!(mask.get(0, -32, 8));
        assert!(!mask.get(1, -32, 0)); // bit 1 is clear.
    }

    #[test]
    fn from_array_set_grows_and_get_is_false_out_of_range() {
        // `BitSet.valueOf` keeps `wordsInUse` at the last non-zero word:
        // `get` beyond it is false, and `set` grows the backing array. The
        // single-word mask from `from_array(&[0x5], ...)` has index 130 in
        // word 2 — out of range -> false — until `set` writes index 256
        // (word 4), which grows the array.
        let mut mask = CarvingMask::from_array(&[0x5], -64);
        assert!(mask.get(0, -64, 0)); // word 0, bit 0.
        assert!(!mask.get(2, -64, 8)); // index 130, word 2 — out of range.
        mask.set(0, -63, 0); // index 256, word 4 — grows the array.
        assert!(mask.get(0, -63, 0));
        assert!(!mask.get(1, -63, 0)); // index 257 — clear.
    }

    #[test]
    fn from_array_trims_trailing_zero_words_like_value_of() {
        // Java's `BitSet.valueOf` trims trailing zero words (`wordsInUse`
        // shrinks). A bit beyond the trimmed length is out of range (`get`
        // false, never a panic) until `set` grows the array.
        let mut mask = CarvingMask::from_array(&[0x5, 0, 0], -64);
        // `toLongArray` on the trimmed mask is just the non-zero words.
        assert_eq!(mask.to_array(), vec![0x5]);
        assert!(mask.get(0, -64, 0));
        assert!(mask.get(2, -64, 0));
        // Index 64 (word 1) was a trailing zero word — trimmed, so false.
        assert!(!mask.get(0, -64, 4));
        // `set` beyond the trimmed length grows the array (word 4, index 256).
        mask.set(0, -63, 0);
        assert!(mask.get(0, -63, 0));
        assert_eq!(mask.to_array(), vec![0x5, 0, 0, 0, 1]);
    }
}

//! Port of `net.minecraft.util.SimpleBitStorage` (MC 26.2, Paper-patched).
//!
//! PROVENANCE: `net/minecraft/util/SimpleBitStorage.java`. Paper's
//! `BETTER_MAGIC` multiply-shift division (`IntegerUtil.getUnsignedDivisorMagic`,
//! a `spottedleaf-common` dependency not vendored in `working/Paper`) is a
//! performance optimization that is **semantically equivalent** to the direct
//! `index / valuesPerLong` / `index % valuesPerLong` used here — the resulting
//! packed layout and wire bytes are identical (verified against the Java
//! constructor packing below, which is the ground truth for the layout).
//! Paper's `moonrise$countEntries` (block counting) is deferred to M2.

use std::fmt;

use crate::bit_storage::BitStorage;

/// `SimpleBitStorage.InitializationException` — a malformed backing array.
#[derive(Debug)]
pub struct InitializationException {
    message: String,
}

impl InitializationException {
    fn new(got: usize, expected: usize) -> Self {
        InitializationException {
            message: format!(
                "Invalid length given for storage, got: {} but expected: {}",
                got, expected
            ),
        }
    }
}

impl fmt::Display for InitializationException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InitializationException {}

/// `net.minecraft.util.SimpleBitStorage`.
#[derive(Debug)]
pub struct SimpleBitStorage {
    data: Vec<i64>,
    bits: i32,
    mask: i64,
    size: usize,
    values_per_long: usize,
}

impl SimpleBitStorage {
    /// `SimpleBitStorage(int bits, int size, int[] values)` — packs `values`
    /// in Java constructor order: cell `c` holds entries
    /// `values[c*vpl .. c*vpl+vpl)`, entry `c*vpl+k` in the `k`-th `bits`-wide
    /// slot counting from the low bit.
    pub fn from_values(bits: i32, size: usize, values: &[i32]) -> Self {
        Self::validate_bits(bits);
        let mut storage = Self::new(bits, size);
        let mask = storage.mask;
        let bits = bits as u32;

        let mut output_index = 0;
        let mut input_offset = 0;
        while input_offset + storage.values_per_long <= size {
            let mut packed: u64 = 0;
            for index_in_long in (0..storage.values_per_long).rev() {
                packed <<= bits;
                packed |= (values[input_offset + index_in_long] as i64 & mask) as u64;
            }
            storage.data[output_index] = packed as i64;
            output_index += 1;
            input_offset += storage.values_per_long;
        }

        let remainder_count = size - input_offset;
        if remainder_count > 0 {
            let mut last: u64 = 0;
            for index_in_long in (0..remainder_count).rev() {
                last <<= bits;
                last |= (values[input_offset + index_in_long] as i64 & mask) as u64;
            }
            storage.data[output_index] = last as i64;
        }

        storage
    }

    /// Java's `SimpleBitStorage` constructor validates `inclusiveBetween(1, 32, bits)`.
    fn validate_bits(bits: i32) {
        assert!(
            (1..=32).contains(&bits),
            "bits must be in [1, 32], got {bits}"
        );
    }

    /// `SimpleBitStorage(int bits, int size)` — zeroed storage.
    pub fn new(bits: i32, size: usize) -> Self {
        Self::validate_bits(bits);
        let mask = (1i64 << bits) - 1;
        let values_per_long = 64 / bits as usize;
        let required_length = size.div_ceil(values_per_long);
        if size > 4096 {
            // Paper addition: caps the index space the magic multiply-shift is
            // valid for. Kept as a faithful guard.
            panic!("Size > 4096 not supported");
        }
        SimpleBitStorage {
            data: vec![0; required_length],
            bits,
            mask,
            size,
            values_per_long,
        }
    }

    /// `SimpleBitStorage(int bits, int size, long[] data)` — adopts a packed
    /// backing array. Panics (`InitializationException`) when the length
    /// doesn't match `ceil(size / valuesPerLong)`. Bits are validated like the
    /// Java constructor (`inclusiveBetween(1, 32, bits)`).
    pub fn from_raw(bits: i32, size: usize, data: &[i64]) -> Result<Self, InitializationException> {
        Self::validate_bits(bits);
        let values_per_long = 64 / bits as usize;
        let required_length = size.div_ceil(values_per_long);
        if data.len() != required_length {
            return Err(InitializationException::new(data.len(), required_length));
        }
        if size > 4096 {
            // Paper addition: caps the index space the magic multiply-shift is
            // valid for. Java throws IllegalStateException from the
            // data-adoption constructor (`SimpleBitStorage(bits, size, long[])`)
            // after the length check too, so the error surface matches exactly.
            panic!("Size > 4096 not supported");
        }
        Ok(SimpleBitStorage {
            data: data.to_vec(),
            bits,
            mask: (1i64 << bits) - 1,
            size,
            values_per_long,
        })
    }

    fn cell_and_offset(&self, index: usize) -> (usize, usize) {
        // Paper computes these with a 20-bit-precision multiply-shift over the
        // 12-bit index space; the direct quotient/remainder is identical.
        (
            index / self.values_per_long,
            (index % self.values_per_long) * self.bits as usize,
        )
    }
}

impl BitStorage for SimpleBitStorage {
    fn get_and_set(&mut self, index: usize, value: i32) -> i32 {
        let (div_q, div_r) = self.cell_and_offset(index);
        let data = self.data[div_q];
        let mask = self.mask;
        let write = data & !(mask << div_r) | (((value as i64) & mask) << div_r);
        self.data[div_q] = write;
        (data >> div_r & mask) as i32
    }

    fn set(&mut self, index: usize, value: i32) {
        let (div_q, div_r) = self.cell_and_offset(index);
        let mask = self.mask;
        let write = self.data[div_q] & !(mask << div_r) | (((value as i64) & mask) << div_r);
        self.data[div_q] = write;
    }

    fn get(&self, index: usize) -> i32 {
        let (div_q, div_r) = self.cell_and_offset(index);
        (self.data[div_q] >> div_r & self.mask) as i32
    }

    fn get_raw(&self) -> &[i64] {
        &self.data
    }

    fn get_raw_mut(&mut self) -> &mut [i64] {
        &mut self.data
    }

    fn get_size(&self) -> usize {
        self.size
    }

    fn get_bits(&self) -> i32 {
        self.bits
    }

    fn get_all(&self, output: &mut dyn FnMut(i32)) {
        let mut count = 0;
        for &cell in &self.data {
            let mut cell_value = cell;
            for _ in 0..self.values_per_long {
                output((cell_value & self.mask) as i32);
                cell_value >>= self.bits as u32;
                count += 1;
                if count >= self.size {
                    return;
                }
            }
        }
    }

    fn unpack(&self, output: &mut [i32]) {
        let data_length = self.data.len();
        let mut output_offset = 0;

        for i in 0..data_length.saturating_sub(1) {
            let mut cell_value = self.data[i];
            for index_in_long in 0..self.values_per_long {
                output[output_offset + index_in_long] = (cell_value & self.mask) as i32;
                cell_value >>= self.bits as u32;
            }
            output_offset += self.values_per_long;
        }

        let remainder = self.size - output_offset;
        if remainder > 0 {
            let mut cell_value = self.data[data_length - 1];
            for index_in_long in 0..remainder {
                output[output_offset + index_in_long] = (cell_value & self.mask) as i32;
                cell_value >>= self.bits as u32;
            }
        }
    }

    fn copy_box(&self) -> Box<dyn BitStorage> {
        Box::new(SimpleBitStorage {
            data: self.data.clone(),
            bits: self.bits,
            mask: self.mask,
            size: self.size,
            values_per_long: self.values_per_long,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java `SimpleBitStorage(bits, size)` layout: `valuesPerLong = 64/bits`,
    /// cell `c` packs entries `[c*vpl, c*vpl+vpl)` with entry `c*vpl+k` in bits
    /// `[k*bits, (k+1)*bits)`.
    fn expected_raw(bits: i32, size: usize, values: &[i32]) -> Vec<i64> {
        let vpl = 64 / bits as usize;
        let mask = (1i64 << bits) - 1;
        let mut out = Vec::new();
        let mut offset = 0;
        while offset + vpl <= size {
            let mut packed = 0u64;
            for k in (0..vpl).rev() {
                packed <<= bits;
                packed |= (values[offset + k] as i64 & mask) as u64;
            }
            out.push(packed as i64);
            offset += vpl;
        }
        if offset < size {
            let mut packed = 0u64;
            for k in (0..size - offset).rev() {
                packed <<= bits;
                packed |= (values[offset + k] as i64 & mask) as u64;
            }
            out.push(packed as i64);
        }
        out
    }

    #[test]
    fn packing_matches_java_constructor_layout() {
        // bits=4, size=16 -> one long, value[0] in the low 4 bits.
        let mut values = [0i32; 16];
        values[0] = 1;
        let s = SimpleBitStorage::from_values(4, 16, &values);
        assert_eq!(s.get_raw(), &[0x1]);
        assert_eq!(s.get(0), 1);
        assert_eq!(s.get(1), 0);
        assert_eq!(s.get(15), 0);
        assert_eq!(s.get_bits(), 4);

        // value[1] = 1 -> second slot -> bit offset 4.
        let mut values = [0i32; 16];
        values[1] = 1;
        let s = SimpleBitStorage::from_values(4, 16, &values);
        assert_eq!(s.get_raw(), &[0x10]);
        assert_eq!(s.get(1), 1);

        // value[15] = 1 -> highest slot -> bit offset 60 -> 1L << 60.
        let mut values = [0i32; 16];
        values[15] = 1;
        let s = SimpleBitStorage::from_values(4, 16, &values);
        assert_eq!(s.get_raw(), &[0x1000_0000_0000_0000]);
        assert_eq!(s.get(15), 1);
    }

    #[test]
    fn packing_handles_remainder_cell() {
        // bits=5, size=20: vpl=12, so 12 in the first cell, 8 in the second.
        let mut values = vec![0i32; 20];
        values[12] = 3; // first entry of the remainder cell.
        let s = SimpleBitStorage::from_values(5, 20, &values);
        assert_eq!(s.get_raw().len(), 2);
        assert_eq!(s.get(12), 3);
        // Slot for entry 12 is the low 5 bits of cell 1.
        assert_eq!(s.get_raw()[1] & 0x1F, 3);
    }

    #[test]
    fn get_and_set_returns_previous() {
        let mut s = SimpleBitStorage::new(4, 64);
        assert_eq!(s.get_and_set(3, 9), 0);
        assert_eq!(s.get(3), 9);
        assert_eq!(s.get_and_set(3, 2), 9);
        assert_eq!(s.get(3), 2);
        assert_eq!(s.get_and_set(3, 15), 2);
        assert_eq!(s.get(3), 15);
    }

    #[test]
    fn set_clears_prior_value() {
        let mut s = SimpleBitStorage::new(4, 64);
        s.set(5, 15);
        s.set(5, 0);
        assert_eq!(s.get(5), 0);
        s.set(5, 15);
        s.set(5, 1);
        assert_eq!(s.get(5), 1);
    }

    #[test]
    fn values_per_long_and_raw_length() {
        // bits=1 -> vpl=64, 4096 entries -> 64 longs.
        assert_eq!(SimpleBitStorage::new(1, 4096).get_raw().len(), 64);
        // bits=4 -> vpl=16 -> 256 longs.
        assert_eq!(SimpleBitStorage::new(4, 4096).get_raw().len(), 256);
        // bits=15 -> vpl=4 -> 1024 longs.
        assert_eq!(SimpleBitStorage::new(15, 4096).get_raw().len(), 1024);
        // bits=32 -> vpl=2 -> 2048 longs.
        assert_eq!(SimpleBitStorage::new(32, 4096).get_raw().len(), 2048);
    }

    #[test]
    fn unpack_and_get_all_match_get() {
        for (bits, size) in [(4usize, 4096usize), (5, 4096), (15, 4096), (3, 21), (6, 10)] {
            let mut s = SimpleBitStorage::new(bits as i32, size);
            let max = (1u64 << bits) - 1;
            for i in 0..size {
                s.set(i, ((i as u64 * 7 + 3) & max) as i32);
            }
            let mut unpacked = vec![0i32; size];
            s.unpack(&mut unpacked);
            for (i, &got) in unpacked.iter().enumerate() {
                assert_eq!(got, s.get(i), "unpack[{i}] bits={bits}");
            }
            let mut all = Vec::new();
            s.get_all(&mut |v| all.push(v));
            assert_eq!(all, unpacked, "get_all bits={bits}");
        }
    }

    #[test]
    fn round_trip_from_raw() {
        let mut s = SimpleBitStorage::new(4, 4096);
        for i in 0..4096 {
            s.set(i, (i % 16) as i32);
        }
        let raw = s.get_raw().to_vec();
        let back = SimpleBitStorage::from_raw(4, 4096, &raw).expect("valid length");
        assert_eq!(back.get_raw(), s.get_raw());
        for i in 0..4096 {
            assert_eq!(back.get(i), s.get(i));
        }
    }

    #[test]
    fn from_raw_rejects_bad_length() {
        let err = SimpleBitStorage::from_raw(4, 4096, &[0i64; 255]).unwrap_err();
        assert_eq!(
            err.message,
            "Invalid length given for storage, got: 255 but expected: 256"
        );
        assert!(SimpleBitStorage::from_raw(4, 4096, &[0i64; 256]).is_ok());
    }

    #[test]
    #[should_panic(expected = "Size > 4096 not supported")]
    fn from_raw_rejects_size_over_4096() {
        // 4097 entries at 4 bits -> ceil(4097/16) = 257 longs, so the length
        // check passes first (Java order) and the size guard fires.
        let _ = SimpleBitStorage::from_raw(4, 4097, &[0i64; 257]);
    }

    #[test]
    fn copy_is_independent() {
        let mut s = SimpleBitStorage::new(4, 64);
        s.set(0, 7);
        let mut c = s.copy_box();
        let c = c.as_mut();
        c.set(0, 9);
        assert_eq!(s.get(0), 7);
        assert_eq!(c.get(0), 9);
    }

    #[test]
    #[should_panic(expected = "Size > 4096 not supported")]
    fn rejects_size_over_4096() {
        let _ = SimpleBitStorage::new(4, 4097);
    }

    #[test]
    #[should_panic(expected = "bits must be in [1, 32]")]
    fn rejects_zero_bits() {
        let _ = SimpleBitStorage::new(0, 16);
    }

    #[test]
    fn golden_raw_examples() {
        // bits=4, size=16: entries [1, 2, 3, ...] pack value[0] in low nibble.
        let values: Vec<i32> = (1..=16).collect();
        let raw = expected_raw(4, 16, &values);
        let s = SimpleBitStorage::from_values(4, 16, &values);
        assert_eq!(s.get_raw(), raw.as_slice());
    }
}

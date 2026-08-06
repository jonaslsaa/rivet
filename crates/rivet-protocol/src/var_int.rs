//! Port of `net.minecraft.network.VarInt` — Protocol VarInt encoding.
//!
//! Java: `public class VarInt`. Reads/writes operate on the `bytes` crate's
//! `Buf`/`BufMut` traits, the Rust analog of netty `ByteBuf`. The Paper
//! optimizations are preserved exactly: `write` peels the common 1- and 2-byte
//! cases before the reference loop, and `get_byte_size` uses the
//! `VARINT_EXACT_BYTE_LENGTHS` lookup table indexed by leading-zero count.
//! Observable byte output is identical to the vanilla reference loops.
//!
//! `read` throws Java's unchecked `RuntimeException("VarInt too big")` when a
//! value spans more than `MAX_VARINT_SIZE` bytes; per PORTING.md line 33 the
//! unchecked exception maps to `panic!`.

use bytes::{Buf, BufMut};

/// `VarInt.MAX_VARINT_SIZE`.
pub const MAX_VARINT_SIZE: i32 = 5;
const DATA_BITS_MASK: u8 = 0x7F;
const CONTINUATION_BIT_MASK: u8 = 0x80;
const DATA_BITS_PER_BYTE: u32 = 7;

/// `VarInt.VARINT_EXACT_BYTE_LENGTHS[Integer.numberOfLeadingZeros(value)]`.
///
/// `VARINT_EXACT_BYTE_LENGTHS[i] = ceil((31 - (i - 1)) / 7)` for `i` in
/// `0..=32`, with index 32 special-cased to 1 for value 0. Computed in closed
/// form: `ceil(n / 7) == (n + 6) / 7` for the integer `n = 32 - i`.
const VARINT_EXACT_BYTE_LENGTHS: [u8; 33] = {
    let mut table = [0u8; 33];
    let mut i = 0;
    while i <= 32 {
        let n = 32i32 - i;
        table[i as usize] = ((n + 6) / 7) as u8;
        i += 1;
    }
    table[32] = 1; // Special case for the number 0.
    table
};

/// `VarInt.getByteSize(int)`.
pub fn get_byte_size(value: i32) -> i32 {
    VARINT_EXACT_BYTE_LENGTHS[(value as u32).leading_zeros() as usize] as i32
}

/// `VarInt.getByteSizeSlow(int)` — the reference loop.
pub fn get_byte_size_slow(value: i32) -> i32 {
    for i in 1..MAX_VARINT_SIZE {
        if value & (-1i32 << (i * DATA_BITS_PER_BYTE as i32)) == 0 {
            return i;
        }
    }

    MAX_VARINT_SIZE
}

/// `VarInt.hasContinuationBit(byte)`.
pub fn has_continuation_bit(in_byte: u8) -> bool {
    in_byte & CONTINUATION_BIT_MASK == CONTINUATION_BIT_MASK
}

/// `VarInt.read(ByteBuf)`.
///
/// Accumulates in `u32`; Java accumulates in `i32` where shifts mask the
/// shift distance mod 32. `wrapping_shl` (shift distance mod 32) reproduces
/// the same bit pattern, cast back to `i32` at the end.
pub fn read(input: &mut impl Buf) -> i32 {
    let mut out: u32 = 0;
    let mut bytes: u32 = 0;

    loop {
        let in_byte = input.get_u8();
        out |= ((in_byte & DATA_BITS_MASK) as u32).wrapping_shl(bytes * DATA_BITS_PER_BYTE);
        bytes += 1;
        if bytes > MAX_VARINT_SIZE as u32 {
            panic!("VarInt too big");
        }
        if !has_continuation_bit(in_byte) {
            break;
        }
    }

    out as i32
}

/// `VarInt.write(ByteBuf, int)` — Paper-optimized: peels the 1- and 2-byte
/// cases (the most common VarInt sizes) before falling back to the reference
/// loop.
pub fn write(output: &mut impl BufMut, value: i32) {
    let v = value as u32;
    if v & 0xFFFF_FF80 == 0 {
        output.put_u8(v as u8);
    } else if v & 0xFFFF_C000 == 0 {
        // int s = (value & 0x7F | 0x80) << 8 | (value >>> 7);
        let s = ((v & 0x7F) | 0x80) << 8 | (v >> 7);
        output.put_u16(s as u16);
    } else {
        write_slow(output, value);
    }
}

/// `VarInt.writeSlow(ByteBuf, int)` — the reference loop.
pub fn write_slow(output: &mut impl BufMut, value: i32) {
    let mut v = value as u32;
    while v & 0xFFFF_FF80 != 0 {
        output.put_u8((v & DATA_BITS_MASK as u32) as u8 | CONTINUATION_BIT_MASK);
        v >>= DATA_BITS_PER_BYTE;
    }

    output.put_u8(v as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    fn encoded(value: i32) -> Vec<u8> {
        let mut buf = BytesMut::new();
        write(&mut buf, value);
        buf.to_vec()
    }

    fn decoded(bytes: &[u8]) -> i32 {
        read(&mut BytesMut::from(bytes))
    }

    #[test]
    fn byte_size_table() {
        assert_eq!(get_byte_size(0), 1);
        assert_eq!(get_byte_size(1), 1);
        assert_eq!(get_byte_size(127), 1);
        assert_eq!(get_byte_size(128), 2);
        assert_eq!(get_byte_size(16_383), 2);
        assert_eq!(get_byte_size(16_384), 3);
        assert_eq!(get_byte_size(2_097_151), 3);
        assert_eq!(get_byte_size(2_097_152), 4);
        assert_eq!(get_byte_size(268_435_455), 4);
        assert_eq!(get_byte_size(268_435_456), 5);
        assert_eq!(get_byte_size(i32::MAX), 5);
        assert_eq!(get_byte_size(-1), 5);
        assert_eq!(get_byte_size(i32::MIN), 5);
    }

    #[test]
    fn byte_size_slow_matches_fast() {
        for v in [
            0,
            1,
            127,
            128,
            16_383,
            16_384,
            2_097_151,
            2_097_152,
            268_435_455,
            268_435_456,
            i32::MAX,
            -1,
            i32::MIN,
        ] {
            assert_eq!(get_byte_size_slow(v), get_byte_size(v), "value {v}");
        }
    }

    #[test]
    fn has_continuation_bit_basic() {
        assert!(!has_continuation_bit(0x7F));
        assert!(has_continuation_bit(0x80));
        assert!(has_continuation_bit(0xFF));
        assert!(!has_continuation_bit(0x00));
    }

    #[test]
    fn exact_encodings() {
        assert_eq!(encoded(0), [0x00]);
        assert_eq!(encoded(1), [0x01]);
        assert_eq!(encoded(127), [0x7F]);
        assert_eq!(encoded(128), [0x80, 0x01]);
        assert_eq!(encoded(300), [0xAC, 0x02]);
        assert_eq!(encoded(16_383), [0xFF, 0x7F]);
        assert_eq!(encoded(16_384), [0x80, 0x80, 0x01]);
        assert_eq!(encoded(i32::MAX), [0xFF, 0xFF, 0xFF, 0xFF, 0x07]);
        assert_eq!(encoded(i32::MIN), [0x80, 0x80, 0x80, 0x80, 0x08]);
        assert_eq!(encoded(-1), [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    }

    #[test]
    fn round_trip_boundaries() {
        for v in [
            0,
            1,
            127,
            128,
            16_383,
            16_384,
            2_097_151,
            2_097_152,
            268_435_455,
            268_435_456,
            i32::MAX,
            -1,
            -128,
            -16_384,
            i32::MIN,
        ] {
            assert_eq!(decoded(&encoded(v)), v, "value {v}");
        }
    }

    #[test]
    fn read_accepts_multiple_of_seven_bits() {
        // 0x7F | 0x7F << 7 = 16_383, encoded as [0xFF, 0x7F].
        assert_eq!(decoded(&[0xFF, 0x7F]), 16_383);
    }

    #[test]
    #[should_panic(expected = "VarInt too big")]
    fn read_too_big() {
        // Six bytes each with the continuation bit set forces a 6-byte read.
        let mut buf = BytesMut::from(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80][..]);
        read(&mut buf);
    }
}

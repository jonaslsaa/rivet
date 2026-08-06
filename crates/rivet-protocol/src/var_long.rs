//! Port of `net.minecraft.network.VarLong` — Protocol VarLong encoding.
//!
//! Java: `public class VarLong`. Same conventions as `VarInt` (see `var_int.rs`):
//! reads/writes operate on the `bytes` crate's `Buf`/`BufMut` traits.
//!
//! `read` throws Java's unchecked `RuntimeException("VarLong too big")` when a
//! value spans more than `MAX_VARLONG_SIZE` bytes; per PORTING.md line 33 the
//! unchecked exception maps to `panic!`. The accumulation uses `wrapping_shl`
//! so the 10th byte's 63-bit shift and any over-length shift cannot overflow:
//! Java masks the shift distance mod 64 and throws on the same byte, so the
//! observable outcome is a panic either way.

use bytes::{Buf, BufMut};

/// `VarLong.MAX_VARLONG_SIZE`.
pub const MAX_VARLONG_SIZE: i32 = 10;
const DATA_BITS_MASK: u8 = 0x7F;
const CONTINUATION_BIT_MASK: u8 = 0x80;
const DATA_BITS_PER_BYTE: u32 = 7;

/// `VarLong.getByteSize(long)`.
pub fn get_byte_size(value: i64) -> i32 {
    for i in 1..MAX_VARLONG_SIZE {
        if value & (-1i64 << (i * DATA_BITS_PER_BYTE as i32)) == 0 {
            return i;
        }
    }

    MAX_VARLONG_SIZE
}

/// `VarLong.hasContinuationBit(byte)`.
pub fn has_continuation_bit(in_byte: u8) -> bool {
    in_byte & CONTINUATION_BIT_MASK == CONTINUATION_BIT_MASK
}

/// `VarLong.read(ByteBuf)`.
pub fn read(input: &mut impl Buf) -> i64 {
    let mut out: u64 = 0;
    let mut bytes: u32 = 0;

    loop {
        let in_byte = input.get_u8();
        out |= ((in_byte & DATA_BITS_MASK) as u64).wrapping_shl(bytes * DATA_BITS_PER_BYTE);
        bytes += 1;
        if bytes > MAX_VARLONG_SIZE as u32 {
            panic!("VarLong too big");
        }
        if !has_continuation_bit(in_byte) {
            break;
        }
    }

    out as i64
}

/// `VarLong.write(ByteBuf, long)`.
pub fn write(output: &mut impl BufMut, value: i64) {
    let mut v = value as u64;
    while v & 0xFFFF_FFFF_FFFF_FF80 != 0 {
        output.put_u8((v & DATA_BITS_MASK as u64) as u8 | CONTINUATION_BIT_MASK);
        v >>= DATA_BITS_PER_BYTE;
    }

    output.put_u8(v as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    fn encoded(value: i64) -> Vec<u8> {
        let mut buf = BytesMut::new();
        write(&mut buf, value);
        buf.to_vec()
    }

    fn decoded(bytes: &[u8]) -> i64 {
        read(&mut BytesMut::from(bytes))
    }

    #[test]
    fn byte_size_boundaries() {
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
        assert_eq!(get_byte_size(34_359_738_367), 5);
        assert_eq!(get_byte_size(34_359_738_368), 6);
        assert_eq!(get_byte_size(i64::MAX), 9);
        assert_eq!(get_byte_size(-1), 10);
        assert_eq!(get_byte_size(i64::MIN), 10);
    }

    #[test]
    fn exact_encodings() {
        assert_eq!(encoded(0), [0x00]);
        assert_eq!(encoded(1), [0x01]);
        assert_eq!(encoded(127), [0x7F]);
        assert_eq!(encoded(128), [0x80, 0x01]);
        assert_eq!(encoded(300), [0xAC, 0x02]);
        assert_eq!(
            encoded(i64::MAX),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F]
        );
        assert_eq!(
            encoded(i64::MIN),
            [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
        );
        assert_eq!(
            encoded(-1),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01]
        );
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
            34_359_738_367,
            34_359_738_368,
            i64::MAX,
            -1,
            -128,
            -16_384,
            i64::MIN,
        ] {
            assert_eq!(decoded(&encoded(v)), v, "value {v}");
        }
    }

    #[test]
    fn read_accepts_full_63_bit_payload() {
        // Nine bytes: 8 continuation bytes of 0xFF (7 data bits each) then a
        // terminating 0x7F (7 data bits) -> 63 bits set (i64::MAX).
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F];
        assert_eq!(decoded(&bytes), i64::MAX);
    }

    #[test]
    #[should_panic(expected = "VarLong too big")]
    fn read_too_big() {
        // Eleven bytes each with the continuation bit set forces an 11-byte read.
        let mut buf = BytesMut::from(&[0x80; 11][..]);
        read(&mut buf);
    }
}

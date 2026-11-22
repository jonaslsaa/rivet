//! Port of `ca.spottedleaf.dataconverter.util.IntegerUtil`.
//!
//! All arithmetic uses explicit wrapping/unsigned-shift semantics matching Java's
//! 32-bit `int` and 64-bit `long` operators.

/// `IntegerUtil` — bit-twiddling, log2, Hacker's Delight division magic, and
/// integer hash helpers.
pub struct IntegerUtil;

impl IntegerUtil {
    /// `IntegerUtil.HIGH_BIT_U32 = Integer.MIN_VALUE`.
    pub const HIGH_BIT_U32: i32 = i32::MIN;
    /// `IntegerUtil.HIGH_BIT_U64 = Long.MIN_VALUE`.
    pub const HIGH_BIT_U64: i64 = i64::MIN;

    /// `IntegerUtil.ceilLog2(int)`.
    #[inline]
    pub fn ceil_log2_i32(value: i32) -> i32 {
        32 - value.wrapping_sub(1).leading_zeros() as i32
    }

    /// `IntegerUtil.ceilLog2(long)`.
    #[inline]
    pub fn ceil_log2_i64(value: i64) -> i64 {
        64 - value.wrapping_sub(1).leading_zeros() as i64
    }

    /// `IntegerUtil.floorLog2(int)`.
    #[inline]
    pub fn floor_log2_i32(value: i32) -> i32 {
        31 ^ (value.leading_zeros() as i32)
    }

    /// `IntegerUtil.floorLog2(long)`.
    #[inline]
    pub fn floor_log2_i64(value: i64) -> i32 {
        63 ^ (value.leading_zeros() as i32)
    }

    /// `IntegerUtil.roundCeilLog2(int)`.
    #[inline]
    pub fn round_ceil_log2_i32(value: i32) -> i32 {
        let shift = value.wrapping_sub(1).leading_zeros().wrapping_sub(1);
        (Self::HIGH_BIT_U32 as u32).wrapping_shr(shift) as i32
    }

    /// `IntegerUtil.roundCeilLog2(long)`.
    #[inline]
    pub fn round_ceil_log2_i64(value: i64) -> i64 {
        let shift = value.wrapping_sub(1).leading_zeros().wrapping_sub(1);
        (Self::HIGH_BIT_U64 as u64).wrapping_shr(shift) as i64
    }

    /// `IntegerUtil.roundFloorLog2(int)`.
    #[inline]
    pub fn round_floor_log2_i32(value: i32) -> i32 {
        (Self::HIGH_BIT_U32 as u32).wrapping_shr(value.leading_zeros()) as i32
    }

    /// `IntegerUtil.roundFloorLog2(long)`.
    #[inline]
    pub fn round_floor_log2_i64(value: i64) -> i64 {
        (Self::HIGH_BIT_U64 as u64).wrapping_shr(value.leading_zeros()) as i64
    }

    /// `IntegerUtil.isPowerOfTwo(int)` — note: returns `true` for `0` as in Java.
    #[inline]
    pub fn is_power_of_two_i32(n: i32) -> bool {
        Self::get_trailing_bit_i32(n) == n
    }

    /// `IntegerUtil.isPowerOfTwo(long)` — note: returns `true` for `0` as in Java.
    #[inline]
    pub fn is_power_of_two_i64(n: i64) -> bool {
        Self::get_trailing_bit_i64(n) == n
    }

    /// `IntegerUtil.getTrailingBit(int)`.
    #[inline]
    pub fn get_trailing_bit_i32(n: i32) -> i32 {
        n.wrapping_neg() & n
    }

    /// `IntegerUtil.getTrailingBit(long)`.
    #[inline]
    pub fn get_trailing_bit_i64(n: i64) -> i64 {
        n.wrapping_neg() & n
    }

    /// `IntegerUtil.trailingZeros(int)`.
    #[inline]
    pub fn trailing_zeros_i32(n: i32) -> i32 {
        n.trailing_zeros() as i32
    }

    /// `IntegerUtil.trailingZeros(long)`.
    #[inline]
    pub fn trailing_zeros_i64(n: i64) -> i32 {
        n.trailing_zeros() as i32
    }

    /// `IntegerUtil.getDivisorMultiple(long)` — Hacker's Delight signed division magic.
    #[inline]
    pub fn get_divisor_multiple(numbers: i64) -> i32 {
        ((numbers as u64) >> 32) as i32
    }

    /// `IntegerUtil.getDivisorShift(long)` — Hacker's Delight signed division magic.
    #[inline]
    pub fn get_divisor_shift(numbers: i64) -> i32 {
        numbers as i32
    }

    /// `IntegerUtil.getDivisorNumbers(int)` — Hacker's Delight signed division magic.
    pub fn get_divisor_numbers(d: i32) -> i64 {
        let ad = Self::branchless_abs_i32(d);
        if ad < 2 {
            panic!("|number| must be in [2, 2^31 -1], not: {d}");
        }

        const TWO31: i32 = i32::MIN;
        const MASK: i64 = 0xFFFF_FFFF;

        let mut p: i32 = 31;

        let t: i32 = TWO31.wrapping_add(((d as u32) >> 31) as i32);
        let anc: i32 = t
            .wrapping_sub(1)
            .wrapping_sub((((t as i64) & MASK) % (ad as i64)) as i32);
        let mut q1: i32 = (((TWO31 as i64) & MASK) / ((anc as i64) & MASK)) as i32;
        let mut r1: i32 = TWO31.wrapping_sub(q1.wrapping_mul(anc));
        let mut q2: i32 = (((TWO31 as i64) & MASK) / ((ad as i64) & MASK)) as i32;
        let mut r2: i32 = TWO31.wrapping_sub(q2.wrapping_mul(ad));
        let mut delta: i32;

        loop {
            p = p.wrapping_add(1);
            q1 = q1.wrapping_mul(2);
            r1 = r1.wrapping_mul(2);
            if ((r1 as i64) & MASK) >= ((anc as i64) & MASK) {
                q1 = q1.wrapping_add(1);
                r1 = r1.wrapping_sub(anc);
            }
            q2 = q2.wrapping_mul(2);
            r2 = r2.wrapping_mul(2);
            if ((r2 as i64) & MASK) >= ((ad as i64) & MASK) {
                q2 = q2.wrapping_add(1);
                r2 = r2.wrapping_sub(ad);
            }
            delta = ad.wrapping_sub(r2);
            if !(((q1 as i64) & MASK) < ((delta as i64) & MASK) || (q1 == delta && r1 == 0)) {
                break;
            }
        }

        let mut magic_num: i32 = q2.wrapping_add(1);
        if d < 0 {
            magic_num = magic_num.wrapping_neg();
        }
        let shift: i32 = p;
        ((magic_num as i64) << 32) | (shift as i64)
    }

    /// `IntegerUtil.branchlessAbs(int)`.
    #[inline]
    pub fn branchless_abs_i32(val: i32) -> i32 {
        let mask = val >> 31;
        (mask ^ val).wrapping_sub(mask)
    }

    /// `IntegerUtil.branchlessAbs(long)`.
    #[inline]
    pub fn branchless_abs_i64(val: i64) -> i64 {
        let mask = val >> 63;
        (mask ^ val).wrapping_sub(mask)
    }

    /// `IntegerUtil.hash0(int)`.
    #[inline]
    pub fn hash0_i32(mut x: i32) -> i32 {
        x = x.wrapping_mul(0x3693_5555_u32 as i32);
        x ^= ((x as u32) >> 16) as i32;
        x
    }

    /// `IntegerUtil.hash1(int)`.
    #[inline]
    pub fn hash1_i32(mut x: i32) -> i32 {
        x ^= ((x as u32) >> 15) as i32;
        x = x.wrapping_mul(0x356a_aaad_u32 as i32);
        x ^= ((x as u32) >> 17) as i32;
        x
    }

    /// `IntegerUtil.hash2(int)`.
    #[inline]
    pub fn hash2_i32(mut x: i32) -> i32 {
        x ^= ((x as u32) >> 16) as i32;
        x = x.wrapping_mul(0x7feb_352d_u32 as i32);
        x ^= ((x as u32) >> 15) as i32;
        x = x.wrapping_mul(0x846c_a68b_u32 as i32);
        x ^= ((x as u32) >> 16) as i32;
        x
    }

    /// `IntegerUtil.hash3(int)`.
    #[inline]
    pub fn hash3_i32(mut x: i32) -> i32 {
        x ^= ((x as u32) >> 17) as i32;
        x = x.wrapping_mul(0xed5a_d4bb_u32 as i32);
        x ^= ((x as u32) >> 11) as i32;
        x = x.wrapping_mul(0xac4c_1b51_u32 as i32);
        x ^= ((x as u32) >> 15) as i32;
        x = x.wrapping_mul(0x3184_8bab_u32 as i32);
        x ^= ((x as u32) >> 14) as i32;
        x
    }

    /// `IntegerUtil.hash1(long)`.
    #[inline]
    pub fn hash1_i64(mut x: i64) -> i64 {
        x ^= ((x as u64) >> 27) as i64;
        x = x.wrapping_mul(0xb249_24b7_1d2d_354b_u64 as i64);
        x ^= ((x as u64) >> 28) as i64;
        x
    }

    /// `IntegerUtil.hash2(long)`.
    #[inline]
    pub fn hash2_i64(mut x: i64) -> i64 {
        x ^= ((x as u64) >> 32) as i64;
        x = x.wrapping_mul(0xd6e8_feb8_6659_fd93_u64 as i64);
        x ^= ((x as u64) >> 32) as i64;
        x = x.wrapping_mul(0xd6e8_feb8_6659_fd93_u64 as i64);
        x ^= ((x as u64) >> 32) as i64;
        x
    }

    /// `IntegerUtil.hash3(long)`.
    #[inline]
    pub fn hash3_i64(mut x: i64) -> i64 {
        x ^= ((x as u64) >> 45) as i64;
        x = x.wrapping_mul(0xc161_abe5_704b_6c79_u64 as i64);
        x ^= ((x as u64) >> 41) as i64;
        x = x.wrapping_mul(0xe3e5_389a_edbc_90f7_u64 as i64);
        x ^= ((x as u64) >> 56) as i64;
        x = x.wrapping_mul(0x1f9a_ba75_a52d_b073_u64 as i64);
        x ^= ((x as u64) >> 53) as i64;
        x
    }
}

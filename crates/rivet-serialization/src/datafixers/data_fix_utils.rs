//! Port of `com.mojang.datafixers.DataFixUtils`.
//!
//! Only the pieces the builder foundation needs are ported: the version-key
//! math (`makeKey`/`getVersion`/`getSubVersion`) and the bit-twiddling helpers.
//! The `Optional`/`ByteBuffer` helpers are dropped (no matching Rust surfaces).

/// `DataFixUtils.makeKey(version)` — `makeKey(version, 0)`.
pub fn make_key(version: i32) -> i32 {
    make_key_sub(version, 0)
}

/// `DataFixUtils.makeKey(version, subVersion)` — `version * 10 + subVersion`
/// with Java `int` wrapping.
pub fn make_key_sub(version: i32, sub_version: i32) -> i32 {
    version.wrapping_mul(10).wrapping_add(sub_version)
}

/// `DataFixUtils.getVersion(key)` — `key / 10`.
pub fn get_version(key: i32) -> i32 {
    key / 10
}

/// `DataFixUtils.getSubVersion(key)` — `key % 10`.
pub fn get_sub_version(key: i32) -> i32 {
    key % 10
}

/// `DataFixUtils.smallestEncompassingPowerOfTwo(input)` — the bit-hack based
/// on `http://graphics.stanford.edu/~seander/bithacks.html#RoundUpPowerOf2`.
/// Java `int` semantics: result shifts are sign-preserving, the final `+ 1`
/// wraps.
pub fn smallest_encompassing_power_of_two(input: i32) -> i32 {
    let mut result = input.wrapping_sub(1);
    result |= result >> 1;
    result |= result >> 2;
    result |= result >> 4;
    result |= result >> 8;
    result |= result >> 16;
    result.wrapping_add(1)
}

/// `DataFixUtils.isPowerOfTwo(input)` (private in Java).
fn is_power_of_two(input: i32) -> bool {
    input != 0 && (input & (input - 1)) == 0
}

/// The `MULTIPLY_DE_BRUIJN_BIT_POSITION` table used by `ceillog2`.
const MULTIPLY_DE_BRUIJN_BIT_POSITION: [i32; 32] = [
    0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16, 7, 26,
    12, 18, 6, 11, 5, 10, 9,
];

/// `DataFixUtils.ceillog2(input)` — Java `int` shift semantics.
pub fn ceillog2(input: i32) -> i32 {
    let input = if is_power_of_two(input) {
        input
    } else {
        smallest_encompassing_power_of_two(input)
    };
    MULTIPLY_DE_BRUIJN_BIT_POSITION[((input as i64 * 0x077C_B531i64) >> 27) as usize & 0x1F]
}

/// `DataFixUtils.make(factory)` — evaluate a closure once.
pub fn make<T>(factory: impl FnOnce() -> T) -> T {
    factory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_key_round_trips() {
        assert_eq!(make_key(99), 990);
        assert_eq!(make_key_sub(99, 4), 994);
        assert_eq!(get_version(994), 99);
        assert_eq!(get_sub_version(994), 4);
        assert_eq!(get_version(990), 99);
        assert_eq!(get_sub_version(990), 0);
    }

    #[test]
    fn make_key_wraps_like_java_int() {
        // Java int overflow: makeKey(i32::MAX) = i32::MAX * 10 which wraps.
        // i32::MAX * 10 = 21_474_836_470 fits in u32/i64, then `as i32` wraps.
        assert_eq!(make_key(i32::MAX), (i32::MAX as i64 * 10) as i32);
        assert_eq!(make_key_sub(i32::MAX, 5), (i32::MAX as i64 * 10 + 5) as i32);
        assert_eq!(get_version(i32::MAX), 214_748_364);
        assert_eq!(get_sub_version(i32::MAX), 7);
    }

    #[test]
    fn power_of_two_helpers() {
        assert_eq!(smallest_encompassing_power_of_two(1), 1);
        assert_eq!(smallest_encompassing_power_of_two(3), 4);
        assert_eq!(smallest_encompassing_power_of_two(5), 8);
        assert_eq!(ceillog2(4), 2);
        assert_eq!(ceillog2(5), 3);
        assert_eq!(ceillog2(1), 0);
    }
}

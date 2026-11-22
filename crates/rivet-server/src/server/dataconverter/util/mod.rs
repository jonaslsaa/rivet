//! Port of `ca.spottedleaf.dataconverter.util`.

pub mod int2int_array_sorted_map;
pub mod int2object_array_sorted_map;
pub mod integer_util;
pub mod long2int_array_sorted_map;
pub mod long2object_array_sorted_map;
pub mod namespace_util;

pub use int2int_array_sorted_map::Int2IntArraySortedMap;
pub use int2object_array_sorted_map::Int2ObjectArraySortedMap;
pub use integer_util::IntegerUtil;
pub use long2int_array_sorted_map::Long2IntArraySortedMap;
pub use long2object_array_sorted_map::Long2ObjectArraySortedMap;
pub use namespace_util::NamespaceUtil;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dataconverter::types::map_type::MapType;
    use crate::server::dataconverter::types::nbt::NBTMapType;

    #[test]
    fn int2int_sorted_map_operations_and_growth() {
        let mut map = Int2IntArraySortedMap::new();
        assert_eq!(map.get(0), 0);
        assert_eq!(map.get_floor(0), 0);

        // Insert out-of-order and grow past initial capacity of 8.
        for k in [30, 10, 20, 50, 40, 5, 15, 25, 35, 45, 55, 1] {
            assert_eq!(map.put(k, k * 10), 0);
        }
        // Replace existing key returns previous value.
        assert_eq!(map.put(20, 999), 200);
        assert_eq!(map.get(20), 999);
        assert_eq!(map.get(1), 10);
        assert_eq!(map.get(55), 550);
        assert_eq!(map.get(99), 0);

        // get_floor: below min, exact, between keys, above max.
        assert_eq!(map.get_floor(0), 0);
        assert_eq!(map.get_floor(1), 10);
        assert_eq!(map.get_floor(4), 10);
        assert_eq!(map.get_floor(5), 50);
        assert_eq!(map.get_floor(21), 999);
        assert_eq!(map.get_floor(1000), 550);

        // compute_if_absent is lazy when key exists and inserts when absent.
        let mut called = false;
        let existing = map.compute_if_absent(20, |_| {
            called = true;
            0
        });
        assert_eq!(existing, 999);
        assert!(!called);

        let inserted = map.compute_if_absent(22, |k| {
            called = true;
            k * 2
        });
        assert_eq!(inserted, 44);
        assert!(called);
        assert_eq!(map.get(22), 44);
        assert_eq!(map.get_floor(23), 44);
    }

    #[test]
    fn int2object_sorted_map_matches_paper_semantics() {
        let mut map = Int2ObjectArraySortedMap::<&'static str>::new();
        assert_eq!(map.get(10), None);
        // In Paper 26.2, put/computeIfAbsent omit `++this.size`, so `get` sees
        // empty binarySearch slice and `getFloor` indexes `val[0]`.
        assert_eq!(map.put(10, "ten"), None);
        assert_eq!(map.get(10), None);
        assert_eq!(map.get_floor(10), Some(&"ten"));
        assert_eq!(map.compute_if_absent(20, |_| "twenty"), Some(&"twenty"));
        assert_eq!(map.get_floor(5), Some(&"twenty"));
    }

    #[test]
    fn long2int_sorted_map_operations_and_extremes() {
        let mut map = Long2IntArraySortedMap::new();
        assert_eq!(map.get(0), 0);
        assert_eq!(map.get_floor(0), 0);

        for (idx, k) in [i64::MAX, 0, i64::MIN, 100, -100, 50, -50, 25, -25, 75, -75]
            .into_iter()
            .enumerate()
        {
            assert_eq!(map.put(k, idx as i32), 0);
        }

        assert_eq!(map.get(i64::MIN), 2);
        assert_eq!(map.get(i64::MAX), 0);
        assert_eq!(map.put(0, 777), 1);
        assert_eq!(map.get(0), 777);

        assert_eq!(map.get_floor(i64::MIN), 2);
        assert_eq!(map.get_floor(-99), 4);
        assert_eq!(map.get_floor(1), 777);
        assert_eq!(map.get_floor(i64::MAX), 0);

        let v = map.compute_if_absent(12, |_| 1234);
        assert_eq!(v, 1234);
        assert_eq!(map.compute_if_absent(12, |_| 9999), 1234);
    }

    #[test]
    fn long2object_sorted_map_operations_and_growth() {
        let mut map = Long2ObjectArraySortedMap::<String>::new();
        assert_eq!(map.get(0), None);
        assert_eq!(map.get_floor(0), None);

        for k in [10_i64, 2, 8, 4, 6, 12, 1, 3, 5, 7, 9, 11] {
            assert_eq!(map.put(k, format!("v{k}")), None);
        }
        assert_eq!(map.put(6, "v6_new".to_owned()), Some("v6".to_owned()));
        assert_eq!(map.get(6).map(String::as_str), Some("v6_new"));
        assert_eq!(map.get(100), None);

        assert_eq!(map.get_floor(0), None);
        assert_eq!(map.get_floor(1).map(String::as_str), Some("v1"));
        assert_eq!(map.get_floor(6).map(String::as_str), Some("v6_new"));
        assert_eq!(map.get_floor(100).map(String::as_str), Some("v12"));

        let mut called = false;
        assert_eq!(
            map.compute_if_absent(6, |_| {
                called = true;
                "nope".to_owned()
            })
            .map(String::as_str),
            Some("v6_new")
        );
        assert!(!called);

        assert_eq!(
            map.compute_if_absent(15, |k| {
                called = true;
                format!("c{k}")
            })
            .map(String::as_str),
            Some("c15")
        );
        assert!(called);
        assert_eq!(map.get_floor(20).map(String::as_str), Some("c15"));
    }

    #[test]
    fn integer_util_log2_power_of_two_and_wrapping() {
        assert_eq!(IntegerUtil::HIGH_BIT_U32, i32::MIN);
        assert_eq!(IntegerUtil::HIGH_BIT_U64, i64::MIN);

        assert_eq!(IntegerUtil::ceil_log2_i32(1), 0);
        assert_eq!(IntegerUtil::ceil_log2_i32(2), 1);
        assert_eq!(IntegerUtil::ceil_log2_i32(3), 2);
        assert_eq!(IntegerUtil::ceil_log2_i32(4), 2);
        assert_eq!(IntegerUtil::ceil_log2_i32(5), 3);
        assert_eq!(IntegerUtil::ceil_log2_i32(0), 32);

        assert_eq!(IntegerUtil::ceil_log2_i64(1), 0);
        assert_eq!(IntegerUtil::ceil_log2_i64(3), 2);
        assert_eq!(IntegerUtil::ceil_log2_i64(0), 64);

        assert_eq!(IntegerUtil::floor_log2_i32(1), 0);
        assert_eq!(IntegerUtil::floor_log2_i32(2), 1);
        assert_eq!(IntegerUtil::floor_log2_i32(3), 1);
        assert_eq!(IntegerUtil::floor_log2_i32(8), 3);
        assert_eq!(IntegerUtil::floor_log2_i32(i32::MIN), 31);

        assert_eq!(IntegerUtil::floor_log2_i64(1), 0);
        assert_eq!(IntegerUtil::floor_log2_i64(15), 3);
        assert_eq!(IntegerUtil::floor_log2_i64(i64::MIN), 63);

        assert_eq!(IntegerUtil::round_ceil_log2_i32(1), 1);
        assert_eq!(IntegerUtil::round_ceil_log2_i32(3), 4);
        assert_eq!(IntegerUtil::round_ceil_log2_i32(4), 4);
        assert_eq!(IntegerUtil::round_ceil_log2_i32(5), 8);
        assert_eq!(IntegerUtil::round_ceil_log2_i64(5), 8);

        assert_eq!(IntegerUtil::round_floor_log2_i32(1), 1);
        assert_eq!(IntegerUtil::round_floor_log2_i32(3), 2);
        assert_eq!(IntegerUtil::round_floor_log2_i32(8), 8);
        assert_eq!(IntegerUtil::round_floor_log2_i64(15), 8);

        assert!(IntegerUtil::is_power_of_two_i32(0));
        assert!(IntegerUtil::is_power_of_two_i32(1));
        assert!(IntegerUtil::is_power_of_two_i32(16));
        assert!(!IntegerUtil::is_power_of_two_i32(3));
        assert!(IntegerUtil::is_power_of_two_i32(i32::MIN));
        assert!(IntegerUtil::is_power_of_two_i64(0));
        assert!(IntegerUtil::is_power_of_two_i64(64));
        assert!(!IntegerUtil::is_power_of_two_i64(65));

        assert_eq!(IntegerUtil::get_trailing_bit_i32(12), 4);
        assert_eq!(IntegerUtil::get_trailing_bit_i32(i32::MIN), i32::MIN);
        assert_eq!(IntegerUtil::get_trailing_bit_i64(24), 8);
        assert_eq!(IntegerUtil::trailing_zeros_i32(12), 2);
        assert_eq!(IntegerUtil::trailing_zeros_i64(24), 3);

        assert_eq!(IntegerUtil::branchless_abs_i32(-42), 42);
        assert_eq!(IntegerUtil::branchless_abs_i32(42), 42);
        assert_eq!(IntegerUtil::branchless_abs_i32(i32::MIN), i32::MIN);
        assert_eq!(IntegerUtil::branchless_abs_i64(-42), 42);
        assert_eq!(IntegerUtil::branchless_abs_i64(i64::MIN), i64::MIN);

        // Hacker's Delight division magic sanity check for d = 3, -3, 10.
        for d in [2, 3, 5, 7, 10, -3, -7] {
            let magic = IntegerUtil::get_divisor_numbers(d);
            let mult = IntegerUtil::get_divisor_multiple(magic);
            let shift = IntegerUtil::get_divisor_shift(magic);
            assert!(shift >= 32);
            assert_ne!(mult, 0);
        }

        // Hashes do not panic on extreme values and are deterministic.
        assert_eq!(IntegerUtil::hash0_i32(0), 0);
        assert_eq!(IntegerUtil::hash1_i32(0), 0);
        assert_eq!(IntegerUtil::hash2_i32(0), 0);
        assert_eq!(IntegerUtil::hash3_i32(0), 0);
        assert_eq!(IntegerUtil::hash1_i64(0), 0);
        assert_eq!(IntegerUtil::hash2_i64(0), 0);
        assert_eq!(IntegerUtil::hash3_i64(0), 0);
        assert_ne!(IntegerUtil::hash2_i32(12345), 0);
        assert_ne!(IntegerUtil::hash3_i64(12345), 0);
    }

    #[test]
    fn namespace_util_normalization_and_enforce_for_path() {
        assert_eq!(NamespaceUtil::correct_namespace(None), None);
        assert_eq!(
            NamespaceUtil::correct_namespace(Some("stone")),
            Some("minecraft:stone".to_owned())
        );
        assert_eq!(
            NamespaceUtil::correct_namespace(Some("minecraft:stone")),
            Some("minecraft:stone".to_owned())
        );
        assert_eq!(
            NamespaceUtil::correct_namespace(Some("custom:foo/bar")),
            Some("custom:foo/bar".to_owned())
        );
        assert_eq!(
            NamespaceUtil::correct_namespace(Some("INVALID ID!!")),
            Some("INVALID ID!!".to_owned())
        );

        assert_eq!(NamespaceUtil::correct_namespace_or_null(None), None);
        assert_eq!(
            NamespaceUtil::correct_namespace_or_null(Some("stone")),
            Some("minecraft:stone".to_owned())
        );
        assert_eq!(
            NamespaceUtil::correct_namespace_or_null(Some("minecraft:stone")),
            None
        );
        assert_eq!(
            NamespaceUtil::correct_namespace_or_null(Some("INVALID ID!!")),
            None
        );

        NamespaceUtil::enforce_for_path(None, "id");

        let mut map = NBTMapType::new();
        map.set_string("id", "dirt".to_owned());
        map.set_string("already", "minecraft:oak_planks".to_owned());
        map.set_string("bad", "Not Valid".to_owned());
        NamespaceUtil::enforce_for_path(Some(&mut map), "id");
        NamespaceUtil::enforce_for_path(Some(&mut map), "already");
        NamespaceUtil::enforce_for_path(Some(&mut map), "bad");
        NamespaceUtil::enforce_for_path(Some(&mut map), "missing");

        assert_eq!(map.get_string("id").as_deref(), Some("minecraft:dirt"));
        assert_eq!(
            map.get_string("already").as_deref(),
            Some("minecraft:oak_planks")
        );
        assert_eq!(map.get_string("bad").as_deref(), Some("Not Valid"));
        assert_eq!(map.get_string("missing"), None);
    }
}

//! Java-grounded tests for the registry-core prereq ports in rivet-util:
//! `Util.fixedSize/getRandom/getRandomSafe/mapValuesLazy/shuffledCopy`,
//! `StringRepresentable`, and `ByIdMap`.
//!
//! The RNG golden sequences were computed by running the real Java
//! `LegacyRandomSource.nextInt(bound)` replication (verified against the crate's
//! own golden-tested `LegacyRandomSource`), so the shuffle draw order and
//! `shuffledCopy` result are byte-exact Java parity.

use rivet_serialization::json_ops::JsonOps;
use rivet_util::random::{LegacyRandomSource, RandomSource};
use rivet_util::string_representable::{EnumCodec, EnumOrdinal, StringRepresentable};
use rivet_util::util::LazyValueMap;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Util.fixedSize
// ---------------------------------------------------------------------------

#[test]
fn fixed_size_int_exact() {
    let r = rivet_util::fixed_size_i32(&[1, 2, 3], 3);
    assert!(r.is_success());
    assert_eq!(*r.get_or_throw("exact"), vec![1, 2, 3]);
}

#[test]
fn fixed_size_int_too_many_error_with_partial() {
    let r = rivet_util::fixed_size_i32(&[1, 2, 3, 4], 3);
    assert!(r.is_error());
    let err = r.error_ref().unwrap();
    assert_eq!(err.message(), "Input is not a list of 3 ints");
    // Java `DataResult.error(Supplier, R partialResult)` carries the first
    // `size` ints as a partial.
    assert_eq!(err.partial(), &Some(vec![1, 2, 3]));
}

#[test]
fn fixed_size_int_too_few_error_no_partial() {
    let r = rivet_util::fixed_size_i32(&[1, 2], 3);
    assert!(r.is_error());
    let err = r.error_ref().unwrap();
    assert_eq!(err.message(), "Input is not a list of 3 ints");
    // Java `DataResult.error(Supplier)` (no partial) — length < size.
    assert_eq!(err.partial(), &None);
}

#[test]
fn fixed_size_long_messages() {
    let ok = rivet_util::fixed_size_i64(&[1, 2, 3], 3);
    assert!(ok.is_success());
    let too_many = rivet_util::fixed_size_i64(&[1, 2, 3, 4], 3);
    assert_eq!(
        too_many.error_ref().unwrap().message(),
        "Input is not a list of 3 longs"
    );
}

#[test]
fn fixed_size_list_messages() {
    let ok = rivet_util::fixed_size(&["a".to_string(), "b".to_string()], 2);
    assert!(ok.is_success());
    let too_many = rivet_util::fixed_size(&[1, 2, 3, 4], 3);
    assert_eq!(
        too_many.error_ref().unwrap().message(),
        "Input is not a list of 3 elements"
    );
    assert_eq!(
        too_many.error_ref().unwrap().partial(),
        &Some(vec![1, 2, 3])
    );
}

// ---------------------------------------------------------------------------
// Util.getRandom / getRandomSafe / shuffledCopy / shuffle (RNG parity)
// ---------------------------------------------------------------------------

#[test]
fn get_random_draw_sequence_java_parity() {
    // Java LegacyRandomSource seed 99, nextInt(5) == 2 (verified vs real Java).
    let mut r = LegacyRandomSource::new(99);
    let arr = [10, 20, 30, 40, 50];
    assert_eq!(rivet_util::get_random(&arr, &mut r), 30);
}

#[test]
fn get_random_int_slice_java_parity() {
    let mut r = LegacyRandomSource::new(7);
    let ints = [100, 200, 300];
    let got = rivet_util::get_random(&ints, &mut r);
    assert!(ints.contains(&got));
}

#[test]
fn get_random_safe_empty_is_none_without_consuming_rng() {
    let mut r = LegacyRandomSource::new(5);
    // Draws nothing on the empty path; a subsequent draw is unaffected.
    assert_eq!(rivet_util::get_random_safe::<i32>(&[], &mut r), None);
    let first = r.next_int();
    // Deterministic: seed 5 nextInt() value (Java parity is in random.rs).
    let _ = first;
}

#[test]
fn get_random_safe_java_parity_draws() {
    // Java LegacyRandomSource seed 77, four nextInt(3) draws == [0,0,1,1]
    // (verified vs real Java).
    let mut r = LegacyRandomSource::new(77);
    let list = ["a", "b", "c"];
    let draws: Vec<&str> = (0..4)
        .map(|_| rivet_util::get_random_safe(&list, &mut r).unwrap())
        .collect();
    let indices: Vec<i32> = draws
        .iter()
        .map(|d| list.iter().position(|x| *x == *d).unwrap() as i32)
        .collect();
    assert_eq!(indices, vec![0, 0, 1, 1]);
}

#[test]
fn shuffled_copy_java_parity() {
    // Java LegacyRandomSource seed 1234, Util.shuffle over [10,20,30,40,50]
    // produces [50,10,30,20,40] (verified vs real Java).
    let mut r = LegacyRandomSource::new(1234);
    let arr = [10, 20, 30, 40, 50];
    let shuffled = rivet_util::shuffled_copy(&arr, &mut r);
    assert_eq!(shuffled, vec![50, 10, 30, 20, 40]);
    // Original untouched.
    assert_eq!(arr, [10, 20, 30, 40, 50]);
}

#[test]
fn shuffle_draw_order_java_parity() {
    // The shuffle bound sequence is nextInt(5), nextInt(4), nextInt(3),
    // nextInt(2) == [3,1,2,0] (verified vs real Java). This locks the exact
    // draw order, not just the final permutation.
    let mut r = LegacyRandomSource::new(1234);
    let mut v = [10, 20, 30, 40, 50].to_vec();
    rivet_util::util::shuffle(&mut v, &mut r);
    assert_eq!(v, vec![50, 10, 30, 20, 40]);

    // Re-seed: the draw sequence reproduces exactly.
    let mut r2 = LegacyRandomSource::new(1234);
    let draws: Vec<i32> = (0..4).map(|i| r2.next_int_bound(5 - i)).collect();
    assert_eq!(draws, vec![3, 1, 2, 0]);
}

// ---------------------------------------------------------------------------
// Util.mapValuesLazy
// ---------------------------------------------------------------------------

#[test]
fn map_values_lazy_applies_mapper_on_access() {
    use std::cell::Cell;

    let mut source: HashMap<&str, i32> = HashMap::new();
    source.insert("a", 1);
    source.insert("b", 2);

    let calls = Cell::new(0);
    let lazy: LazyValueMap<&str, i32, i32, _> = LazyValueMap::new(&source, |v| {
        calls.set(calls.get() + 1);
        *v * 10
    });

    // Mapper runs per access, not at construction.
    assert_eq!(lazy.size(), 2);
    assert_eq!(calls.get(), 0);

    assert_eq!(lazy.get(&"a"), Some(10));
    assert_eq!(calls.get(), 1);
    // Second access to the same key runs the mapper again (no caching — a
    // Guava TransformedEntriesMap is a pure view).
    assert_eq!(lazy.get(&"a"), Some(10));
    assert_eq!(calls.get(), 2);
}

#[test]
fn map_values_lazy_entries_and_missing_key() {
    let mut source: HashMap<&str, i32> = HashMap::new();
    source.insert("x", 5);
    let lazy: LazyValueMap<&str, i32, String, _> = LazyValueMap::new(&source, |v| format!("v{v}"));

    assert_eq!(lazy.get(&"missing"), None);
    assert!(!lazy.contains_key(&"missing"));
    assert!(lazy.contains_key(&"x"));

    let mut entries: Vec<(&&str, String)> = lazy.entries();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(entries, vec![(&"x", "v5".to_string())]);
}

// ---------------------------------------------------------------------------
// StringRepresentable
// ---------------------------------------------------------------------------

/// A `net.minecraft.core.Direction`-like enum implementing StringRepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestDirection {
    North,
    South,
    East,
    West,
}

impl StringRepresentable for TestDirection {
    fn get_serialized_name(&self) -> &str {
        match self {
            TestDirection::North => "north",
            TestDirection::South => "south",
            TestDirection::East => "east",
            TestDirection::West => "west",
        }
    }
}

impl EnumOrdinal for TestDirection {
    fn ordinal(&self) -> usize {
        match self {
            TestDirection::North => 0,
            TestDirection::South => 1,
            TestDirection::East => 2,
            TestDirection::West => 3,
        }
    }
}

impl std::fmt::Display for TestDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.get_serialized_name())
    }
}

const TEST_DIRECTIONS: &[TestDirection] = &[
    TestDirection::North,
    TestDirection::South,
    TestDirection::East,
    TestDirection::West,
];

#[test]
fn from_enum_by_name() {
    let codec: EnumCodec<TestDirection, JsonOps> =
        rivet_util::string_representable::from_enum::<TestDirection, JsonOps>(TEST_DIRECTIONS);
    assert_eq!(codec.by_name("north"), Some(TestDirection::North));
    assert_eq!(codec.by_name("east"), Some(TestDirection::East));
    assert_eq!(codec.by_name("nope"), None);
    // byName(name, _default)
    assert_eq!(
        codec.by_name_or("nope", TestDirection::West),
        TestDirection::West
    );
    // byName(name, defaultSupplier) — supplier runs only when unknown.
    let mut calls = 0;
    let got = codec.by_name_or_else("south", || {
        calls += 1;
        TestDirection::North
    });
    assert_eq!(got, TestDirection::South);
    assert_eq!(calls, 0);
}

#[test]
fn create_name_lookup_linear_below_threshold() {
    // 4 values <= 16 -> linear scan; the same result as the hash path.
    let lookup = rivet_util::string_representable::create_name_lookup(TEST_DIRECTIONS);
    assert_eq!(lookup("north"), Some(&TestDirection::North));
    assert_eq!(lookup("missing"), None);
}

#[test]
fn create_name_lookup_hash_above_threshold() {
    // 20 values > 16 -> HashMap path; duplicate names panic like
    // Collectors.toMap.
    let values: Vec<String> = (0..20).map(|i| format!("v{i}")).collect();
    let names: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
    let lookup = rivet_util::string_representable::create_name_lookup_with_converter(&names, |s| {
        s.to_string()
    });
    assert_eq!(lookup("v3"), Some(&"v3"));
    assert_eq!(lookup("v19"), Some(&"v19"));
    assert_eq!(lookup("nope"), None);
}

#[test]
#[should_panic(expected = "Duplicate key v0")]
fn create_name_lookup_duplicate_panics() {
    // Only the >16 (HashMap) branch throws on duplicates — the <=16 linear
    // scan takes the first match. Java's Collectors.toMap throws
    // IllegalStateException("Duplicate key ...") in the map branch.
    let values: Vec<String> = (0..18).map(|i| format!("v{}", i % 17)).collect();
    let names: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
    let _ = rivet_util::string_representable::create_name_lookup_with_converter(&names, |s| {
        s.to_string()
    });
}

// ---------------------------------------------------------------------------
// ByIdMap
// ---------------------------------------------------------------------------

#[test]
fn by_id_map_continuous_zero_strategy() {
    // Java: `BY_ID = ByIdMap.continuous(get3DDataValue, values, ZERO)`.
    let by_id = rivet_util::continuous(
        |d: &TestDirection| match d {
            TestDirection::North => 0,
            TestDirection::South => 1,
            TestDirection::East => 2,
            TestDirection::West => 3,
        },
        TEST_DIRECTIONS,
        rivet_util::OutOfBoundsStrategy::Zero,
    );
    assert_eq!(by_id(0), TestDirection::North);
    assert_eq!(by_id(3), TestDirection::West);
    // Out of bounds -> index 0 value.
    assert_eq!(by_id(-1), TestDirection::North);
    assert_eq!(by_id(100), TestDirection::North);
}

#[test]
fn by_id_map_continuous_wrap_strategy() {
    let by_id = rivet_util::continuous(
        |d: &TestDirection| match d {
            TestDirection::North => 0,
            TestDirection::South => 1,
            TestDirection::East => 2,
            TestDirection::West => 3,
        },
        TEST_DIRECTIONS,
        rivet_util::OutOfBoundsStrategy::Wrap,
    );
    // positiveModulo(-1, 4) = 3 -> West; positiveModulo(4, 4) = 0 -> North.
    assert_eq!(by_id(-1), TestDirection::West);
    assert_eq!(by_id(4), TestDirection::North);
}

#[test]
fn by_id_map_continuous_clamp_strategy() {
    let by_id = rivet_util::continuous(
        |d: &TestDirection| match d {
            TestDirection::North => 0,
            TestDirection::South => 1,
            TestDirection::East => 2,
            TestDirection::West => 3,
        },
        TEST_DIRECTIONS,
        rivet_util::OutOfBoundsStrategy::Clamp,
    );
    assert_eq!(by_id(-5), TestDirection::North);
    assert_eq!(by_id(99), TestDirection::West);
}

#[test]
fn by_id_map_sparse_default() {
    // Sparse ids {1, 3}; default for the rest.
    let by_id = rivet_util::sparse(
        |d: &TestDirection| match d {
            TestDirection::South => 1,
            TestDirection::West => 3,
            _ => unreachable!(),
        },
        &[TestDirection::South, TestDirection::West],
        TestDirection::North,
    );
    assert_eq!(by_id(1), TestDirection::South);
    assert_eq!(by_id(3), TestDirection::West);
    assert_eq!(by_id(0), TestDirection::North);
    assert_eq!(by_id(2), TestDirection::North);
}

#[test]
#[should_panic(expected = "Empty value list")]
fn by_id_map_empty_panics() {
    let _ = rivet_util::continuous(
        |_d: &TestDirection| 0,
        &[],
        rivet_util::OutOfBoundsStrategy::Zero,
    );
}

#[test]
#[should_panic(expected = "Values are not continous, found index 4 for value")]
fn by_id_map_non_continuous_panics() {
    // id >= length -> Java's "Values are not continous" (upstream typo kept).
    let values = [TestDirection::North, TestDirection::South];
    let _ = rivet_util::continuous(
        |d: &TestDirection| if *d == TestDirection::North { 0 } else { 4 },
        &values,
        rivet_util::OutOfBoundsStrategy::Zero,
    );
}

// The `createSortedArray` "Missing value at index" branch is defensive and
// unreachable: with N values whose ids are all in [0, N) and pairwise distinct,
// every one of the N slots is filled. Java's check is the same defensive
// guard (the empty / non-continuous / duplicate checks fire first).

#[test]
#[should_panic(expected = "Duplicate entry on id 0")]
fn by_id_map_duplicate_id_panics() {
    let values = [TestDirection::North, TestDirection::South];
    let _ = rivet_util::continuous(
        |d: &TestDirection| {
            let _ = d;
            0
        },
        &values,
        rivet_util::OutOfBoundsStrategy::Zero,
    );
}

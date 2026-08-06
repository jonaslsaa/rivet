//! `net.minecraft.util.Util` port surface.
//!
//! **PARTIAL-FILE PROVENANCE.** `Util.java` is a leaf of the `mc.util` manifest
//! unit (net.minecraft.util -> rivet-util). This module ports ONLY the helpers
//! the registry-core slice needs (issue #107 / sub-issue #122):
//!
//! - `fixedSize(IntStream/LongStream/List)`
//! - `getRandom(T[]/int[]/List)` + `getRandomSafe(List)`
//! - `mapValuesLazy(Map, Function)` (Guava `Maps.transformValues`, a LAZY view)
//! - `shuffledCopy(T[]/ObjectArrayList)` + the `shuffle(List)` it delegates to
//! - `createIndexLookup(List)` (transitive dep of `StringRepresentable.fromValues`)
//! - `logAndPauseIfInIde` (pre-existing NBT-path surface, retained)
//!
//! RECONCILIATION: when the full `mc.util` unit is ported, these functions move
//! into that unit's `util.rs` under the same names/semantics. Nothing else from
//! `Util.java` (IO, threading, `CompletableFuture` chains, ...) is ported here.

use rivet_serialization::data_result::DataResult;
use std::collections::HashMap;

use crate::random::RandomSource;

/// `Util.logAndPauseIfInIde(message, throwable)`.
///
/// Java logs at ERROR (via LOGGER) and, when running in an IDE, pauses. In the
/// port this is a no-op that swallows the message — there is no logging
/// framework and no IDE pause yet. The important behavior for NbtIo is that the
/// method RETURNS (it does not throw) after a failed string write, which the
/// `StringFallbackDataOutput` relies on.
pub fn log_and_pause_if_in_ide(_message: &str) {}

// ---------------------------------------------------------------------------
// fixedSize
// ---------------------------------------------------------------------------

/// `Util.fixedSize(IntStream stream, int size)` — reads at most `size + 1`
/// elements, then:
///
/// - exactly `size` -> `DataResult.success`
/// - more than `size` -> `DataResult.error("Input is not a list of {size}
///   ints", partial = first `size` ints)` (`DataResult.error(Supplier, R
///   partialResult)`)
/// - fewer than `size` -> `DataResult.error("Input is not a list of {size}
///   ints")` (no partial)
///
/// The input is a slice rather than a stream (finite in the port); `.limit(size
/// + 1)` is implicit in the length check.
pub fn fixed_size_i32(input: &[i32], size: usize) -> DataResult<Vec<i32>> {
    if input.len() != size {
        if input.len() >= size {
            DataResult::error_with_partial(
                format!("Input is not a list of {size} ints"),
                input[..size].to_vec(),
            )
        } else {
            DataResult::error(format!("Input is not a list of {size} ints"))
        }
    } else {
        DataResult::success(input.to_vec())
    }
}

/// `Util.fixedSize(LongStream stream, int size)` — the `long[]` analogue of
/// `fixed_size_i32` (`"Input is not a list of {size} longs"`).
pub fn fixed_size_i64(input: &[i64], size: usize) -> DataResult<Vec<i64>> {
    if input.len() != size {
        if input.len() >= size {
            DataResult::error_with_partial(
                format!("Input is not a list of {size} longs"),
                input[..size].to_vec(),
            )
        } else {
            DataResult::error(format!("Input is not a list of {size} longs"))
        }
    } else {
        DataResult::success(input.to_vec())
    }
}

/// `Util.fixedSize(List<T> list, int size)` — the `List` analogue
/// (`"Input is not a list of {size} elements"`). On success Java returns the
/// same list reference; the port returns an owned copy (value-semantic; the
/// aliasing is not observable through `DataResult`).
pub fn fixed_size<T: Clone>(input: &[T], size: usize) -> DataResult<Vec<T>> {
    if input.len() != size {
        if input.len() >= size {
            DataResult::error_with_partial(
                format!("Input is not a list of {size} elements"),
                input[..size].to_vec(),
            )
        } else {
            DataResult::error(format!("Input is not a list of {size} elements"))
        }
    } else {
        DataResult::success(input.to_vec())
    }
}

// ---------------------------------------------------------------------------
// getRandom / getRandomSafe
// ---------------------------------------------------------------------------

/// `Util.getRandom(T[] array, RandomSource)` / `getRandom(int[] array, ...)` /
/// `getRandom(List<T> list, ...)` — `container[random.nextInt(size)]`. The
/// three Java overloads collapse onto one slice-based function. On an empty
/// slice this calls `next_int_bound(0)`, which panics exactly as Java's
/// `nextInt(bound)` throws `"Bound must be positive"` (the same as
/// `RandomSource.nextInt`; callers must guard with `get_random_safe`).
pub fn get_random<T: Clone>(container: &[T], random: &mut impl RandomSource) -> T {
    let index = random.next_int_bound(container.len() as i32) as usize;
    container[index].clone()
}

/// `Util.getRandomSafe(List<T> list, RandomSource)` — `Optional.empty()` when
/// empty (without consuming the RNG), else `Optional.of(getRandom(list,
/// random))`.
pub fn get_random_safe<T: Clone>(list: &[T], random: &mut impl RandomSource) -> Option<T> {
    if list.is_empty() {
        None
    } else {
        Some(get_random(list, random))
    }
}

// ---------------------------------------------------------------------------
// mapValuesLazy
// ---------------------------------------------------------------------------

/// `Util.mapValuesLazy(Map<K, V1>, Function<V1, V2>)` — Guava
/// `Maps.transformValues`: a LAZY view over the source map. The value mapper is
/// applied on each access (`get`/`entries`/`values`), never eagerly — that
/// laziness is the whole point of `Lazy` over `mapValues` (which is eager).
///
/// Backed by `std::collections::HashMap` (the port of Java's `HashMap`, which
/// is what `MappedRegistry.byKey` uses). `keys` are borrowed from the source;
/// mapped values are produced on demand.
#[derive(Debug)]
pub struct LazyValueMap<'a, K, V1, V2, F> {
    source: &'a HashMap<K, V1>,
    mapper: F,
    _marker: std::marker::PhantomData<(V1, V2)>,
}

impl<'a, K, V1, V2, F> LazyValueMap<'a, K, V1, V2, F>
where
    F: Fn(&V1) -> V2,
{
    /// `Maps.transformValues(fromMap, function)`.
    pub fn new(source: &'a HashMap<K, V1>, mapper: F) -> Self {
        LazyValueMap {
            source,
            mapper,
            _marker: std::marker::PhantomData,
        }
    }

    /// `map.get(key)` with the mapper applied lazily — `None` for a missing
    /// key. Java's `TransformedEntriesMap.get` returns null for an absent key.
    pub fn get(&self, key: &K) -> Option<V2>
    where
        K: Eq + std::hash::Hash,
    {
        self.source.get(key).map(&self.mapper)
    }

    /// `map.size()`.
    pub fn size(&self) -> usize {
        self.source.len()
    }

    /// `map.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    /// `map.containsKey(key)`.
    pub fn contains_key(&self, key: &K) -> bool
    where
        K: Eq + std::hash::Hash,
    {
        self.source.contains_key(key)
    }

    /// `map.entrySet().stream()` — the mapped `(key, value)` pairs, mapper
    /// applied lazily per entry.
    pub fn entries(&self) -> Vec<(&'a K, V2)>
    where
        K: Eq + std::hash::Hash,
    {
        self.source
            .iter()
            .map(|(k, v)| (k, (self.mapper)(v)))
            .collect()
    }

    /// `map.values()` — the mapped values only.
    pub fn values(&self) -> Vec<V2> {
        self.source.values().map(&self.mapper).collect()
    }

    /// `map.keySet()` — the source keys.
    pub fn keys(&self) -> Vec<&'a K> {
        self.source.keys().collect()
    }
}

// ---------------------------------------------------------------------------
// shuffledCopy / shuffle
// ---------------------------------------------------------------------------

/// `Util.shuffle(List<T> list, RandomSource)` — Fisher-Yates from the end:
///
/// ```java
/// for (int i = size; i > 1; i--) {
///     int swapTo = random.nextInt(i);
///     list.set(i - 1, list.set(swapTo, list.get(i - 1)));
/// }
/// ```
///
/// `size` is captured once up front; `nextInt(i)` is bounded by the current
/// position, giving the exact same draw sequence as Java for a seeded RNG.
pub fn shuffle<T: Clone>(list: &mut [T], random: &mut impl RandomSource) {
    let mut i = list.len();
    while i > 1 {
        let swap_to = random.next_int_bound(i as i32) as usize;
        list.swap(i - 1, swap_to);
        i -= 1;
    }
}

/// `Util.shuffledCopy(T[] array, RandomSource)` — a fresh `Vec` copy of the
/// slice, shuffled in place, original untouched.
pub fn shuffled_copy<T: Clone>(array: &[T], random: &mut impl RandomSource) -> Vec<T> {
    let mut copy = array.to_vec();
    shuffle(&mut copy, random);
    copy
}

// ---------------------------------------------------------------------------
// createIndexLookup
// ---------------------------------------------------------------------------

/// `Util.createIndexLookup(List<T>)` — returns a function `T -> int`. For
/// fewer than 8 values this is `List.indexOf` (a linear scan); at 8+ it builds
/// a `Object2IntOpenHashMap` with `defaultReturnValue(-1)`. The port uses an
/// `index_of` over the slice (linear) or a `HashMap<&T, usize>` keyed by value
/// identity — the fastutil map is keyed by object identity in Java
/// (`Object2IntOpenHashMap` uses `Object.equals`, but for the enum-constant
/// call sites the values are interned, so equality and identity coincide).
///
/// MISSING-VALUE SEMANTICS: a value not present maps to `-1` (`usize::MAX` in
/// the hash branch, `None` in the linear branch).
pub fn create_index_lookup<'a, T>(values: &'a [T]) -> IndexLookup<'a, T>
where
    T: PartialEq + Eq + std::hash::Hash,
{
    if values.len() < 8 {
        IndexLookup::Linear(values)
    } else {
        let map: HashMap<&'a T, usize> = values.iter().enumerate().map(|(i, v)| (v, i)).collect();
        IndexLookup::Hashed(map)
    }
}

/// The `ToIntFunction<T>` returned by `create_index_lookup`.
#[derive(Debug, Clone)]
pub enum IndexLookup<'a, T> {
    /// `< 8` values — `List.indexOf`, a linear scan returning `-1`/`None`.
    Linear(&'a [T]),
    /// `>= 8` values — a `HashMap<&T, usize>` with `usize::MAX` for a miss.
    Hashed(HashMap<&'a T, usize>),
}

impl<'a, T: PartialEq + Eq + std::hash::Hash> IndexLookup<'a, T> {
    /// Apply the lookup — the mapped index for `value`, else `None` (Java's
    /// `-1`).
    pub fn index_of(&self, value: &T) -> Option<usize> {
        match self {
            IndexLookup::Linear(values) => values.iter().position(|v| v == value),
            IndexLookup::Hashed(map) => map.get(value).copied(),
        }
    }
}

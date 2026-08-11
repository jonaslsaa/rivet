//! `net.minecraft.util.random` weighted-selection value layer — the full port
//! of `Weighted`, `WeightedList`, and `WeightedRandom` (Paper 26.2, issue
//! #353), the value-layer prerequisite for provider/features work.
//!
//! Fidelity notes (PORTING.md):
//! - `Weighted.weight` is validated `>= 0` in the constructor; a negative
//!   weight throws `IllegalArgumentException("Weight should be >= 0")`. The
//!   IDE-only "0 weight" warning (via `Util.logAndPauseIfInIde`) is dropped —
//!   it depends on `SharedConstants.IS_RUNNING_IN_IDE` and only logs; the
//!   port's `log_and_pause_if_in_ide` is a no-op. The 0-weight *value* is
//!   legal and preserved.
//! - `WeightedRandom.getTotalWeight` accumulates in `long` (wrapping i32
//!   weights) and throws `IllegalArgumentException("Sum of weights must be <=
//!   2147483647")` if the total exceeds `i32::MAX`. Negative individual
//!   weights are only reachable through a caller-supplied `ToIntFunction`
//!   (the `Weighted` constructor forbids them); the port mirrors that with a
//!   generic `weight_fn`.
//! - `WeightedRandom.getRandomItem(random, items, totalWeight, weightGetter)`
//!   throws on `totalWeight < 0`, returns `None` when `totalWeight == 0`
//!   (WITHOUT consuming the RNG), else consumes exactly one
//!   `random.nextInt(totalWeight)` draw and walks the list subtracting
//!   weights until the running index goes negative (first strictly-greater
//!   prefix wins).
//! - `WeightedRandom.getWeightedItem(items, index, weightGetter)` walks with
//!   a wrapping-i32 subtraction (the port keeps `i32` exactly; `index` is
//!   `i32` in Java, and the loop is `index -= weight; if (index < 0)`). A
//!   negative initial `index` returns the first item (the first subtraction
//!   pushes it further below zero); an `index` that never goes below zero
//!   (e.g. `index == total`) yields `None` — faithfully.
//! - `WeightedList` picks its selector by `totalWeight`: `0` -> no selector
//!   (`isEmpty()` true, `getRandom` -> `None`, `getRandomOrThrow` throws
//!   `IllegalStateException("Weighted list has no elements")`); `1..63` ->
//!   `Flat` (an array of length `totalWeight`, each entry the item value
//!   repeated `weight` times, `get(selection)` is a direct index); `>= 64`
//!   -> `Compact` (walks entries subtracting weights, the same loop as
//!   `WeightedRandom.getWeightedItem`, but `get` throws
//!   `IllegalStateException(selection + " exceeded total weight")` on
//!   fall-through — reachable only with a caller-supplied out-of-range
//!   selection). `getRandom`/`getRandomOrThrow` draw `nextInt(totalWeight)`
//!   and feed `get(selection)`.
//! - `Flat` uses `Object[]` in Java; the port uses `Vec<E>` of cloned values
//!   (the flat array never exposes aliasing through the public surface —
//!   `getRandom` returns by clone). Selection order is exactly Java's: the
//!   item occupies slots `[prefix, prefix + weight)` and a `selection` hits
//!   the first slot whose `prefix > selection`.
//! - Codecs: `Weighted.codec` is a record codec over `"data"` (the element
//!   codec, either a `Codec<E>` via `.fieldOf("data")` or an element
//!   `MapCodec`) and `"weight"` via `ExtraCodecs.NON_NEGATIVE_INT` — a
//!   negative `weight` fails decode with exactly
//!   `"Value must be non-negative: N"`. `WeightedList.codec`/
//!   `nonEmptyCodec` map over a list; the `nonEmpty` variant validates
//!   `isEmpty()` with `"Weighted list must contain at least one entry with
//!   non-zero weight"`. `Weighted.map`/`WeightedList.map` preserve weights
//!   and iteration order.
//! - Equality: `WeightedList` compares `totalWeight` and `items`
//!   (order-sensitive), matching Java's `equals`. No `Hash` impl is provided —
//!   Java's `hashCode` (`31 * totalWeight + items.hashCode()`) is a formula
//!   that Rust's derived `Hash` would NOT reproduce, and no consumer in this
//!   value layer needs hashing.
//!
//! RivetTodo(#126): the network `StreamCodec` overloads (`Weighted.streamCodec`,
//! `WeightedList.streamCodec`) are omitted — the protocol crate owns that
//! surface and no consumer in this value layer needs it.

use crate::random::RandomSource;
use crate::util::log_and_pause_if_in_ide;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::extra_codecs;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::fmt;
use std::sync::Arc;

/// `WeightedList.FLAT_THRESHOLD` — a total weight below this uses the flat
/// (array-index) selector; at/above it the compact (walk) selector.
const FLAT_THRESHOLD: i32 = 64;

/// `net.minecraft.util.random.Weighted<T>` — a `(value, weight)` record whose
/// constructor rejects a negative weight. Like the Java record, the fields are
/// private and only reachable through the validated constructor (a struct
/// literal cannot produce a negative-weight entry, which the selector and
/// total-weight code rely on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Weighted<T> {
    value: T,
    weight: i32,
}

impl<T> Weighted<T> {
    /// The record constructor. Throws `IllegalArgumentException("Weight should
    /// be >= 0")` when `weight < 0`, exactly like Java. A `0` weight is legal;
    /// Java's IDE-only log warning is dropped (`log_and_pause_if_in_ide` is a
    /// no-op in the port).
    pub fn new(value: T, weight: i32) -> Self {
        if weight < 0 {
            log_and_pause_if_in_ide("Weight should be >= 0");
            panic!("Weight should be >= 0");
        }
        Weighted { value, weight }
    }

    /// `Weighted.value()`.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// `Weighted.weight()`.
    pub fn weight(&self) -> i32 {
        self.weight
    }

    /// `Weighted.map(Function<T, U>)` — maps the value, keeps the weight.
    pub fn map<U>(self, function: impl FnOnce(T) -> U) -> Weighted<U> {
        Weighted::new(function(self.value), self.weight)
    }
}

/// `Weighted.codec(Codec<E>)` — a record codec over `"data"` (via
/// `elementCodec.fieldOf("data")`) and `"weight"` (`NON_NEGATIVE_INT`).
pub fn weighted_codec<E, Ops>(
    element_codec: Arc<dyn Codec<E, Ops>>,
) -> Arc<dyn Codec<Weighted<E>, Ops>>
where
    E: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    weighted_codec_map(codec::field_of(element_codec, "data".to_string()))
}

/// `Weighted.codec(MapCodec<E>)` — the variant taking an element `MapCodec`
/// directly (no `.fieldOf("data")` wrap).
pub fn weighted_codec_map<E, Ops>(
    element_codec: Arc<dyn MapCodec<E, Ops>>,
) -> Arc<dyn Codec<Weighted<E>, Ops>>
where
    E: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &Weighted<E>| w.value.clone()),
                element_codec,
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|w: &Weighted<E>| w.weight),
                "weight".to_string(),
                extra_codecs::non_negative_int_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|value: E, weight: i32| Weighted::new(value, weight)),
            )
    })
}

impl<T: fmt::Display> fmt::Display for Weighted<T> {
    /// `Weighted.toString()` — `"Weighted[value=..., weight=...]"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Weighted[value={}, weight={}]", self.value, self.weight)
    }
}

/// `net.minecraft.util.random.WeightedRandom` — stateless weighted-selection
/// helpers.
pub struct WeightedRandom;

impl WeightedRandom {
    /// `WeightedRandom.getTotalWeight(List<T>, ToIntFunction<T>)` — the sum of
    /// the item weights as a `long`, throwing
    /// `IllegalArgumentException("Sum of weights must be <= 2147483647")` when
    /// it exceeds `i32::MAX`. The port takes the generic `weight_fn`
    /// (`Fn(&T) -> i32`) in place of Java's `ToIntFunction<T>`.
    pub fn get_total_weight<T>(items: &[T], weight_fn: impl Fn(&T) -> i32) -> i32 {
        let total_weight: i64 = items.iter().map(|item| weight_fn(item) as i64).sum();
        if total_weight > 2147483647 {
            panic!("Sum of weights must be <= 2147483647");
        } else {
            total_weight as i32
        }
    }

    /// `WeightedRandom.getRandomItem(RandomSource, List<T>, int totalWeight,
    /// ToIntFunction<T>)` — throws on a negative `totalWeight`, returns `None`
    /// when it is `0` (without consuming the RNG), else draws one
    /// `random.nextInt(totalWeight)` and selects via `get_weighted_item`.
    pub fn get_random_item<T>(
        random: &mut impl RandomSource,
        items: &[T],
        total_weight: i32,
        weight_fn: impl Fn(&T) -> i32,
    ) -> Option<T>
    where
        T: Clone,
    {
        if total_weight < 0 {
            log_and_pause_if_in_ide("Negative total weight in getRandomItem");
            panic!("Negative total weight in getRandomItem");
        }
        if total_weight == 0 {
            return None;
        }
        let selection = random.next_int_bound(total_weight);
        Self::get_weighted_item(items, selection, weight_fn)
    }

    /// `WeightedRandom.getWeightedItem(List<T>, int index,
    /// ToIntFunction<T>)` — walks the items subtracting weights (i32
    /// wrapping, exactly Java's `index -= weight; if (index < 0)`) and returns
    /// the first item whose prefix strictly exceeds the selection. An index
    /// that never goes below zero (e.g. `>= total`) returns `None`; a negative
    /// start returns the first item (the first subtraction pushes it further
    /// below zero).
    pub fn get_weighted_item<T>(
        items: &[T],
        mut index: i32,
        weight_fn: impl Fn(&T) -> i32,
    ) -> Option<T>
    where
        T: Clone,
    {
        for item in items {
            index = index.wrapping_sub(weight_fn(item));
            if index < 0 {
                return Some(item.clone());
            }
        }
        None
    }

    /// `WeightedRandom.getRandomItem(RandomSource, List<T>, ToIntFunction<T>)`
    /// — the total-computing convenience overload.
    pub fn get_random_item_from_total<T>(
        random: &mut impl RandomSource,
        items: &[T],
        weight_fn: impl Fn(&T) -> i32,
    ) -> Option<T>
    where
        T: Clone,
    {
        let total_weight = Self::get_total_weight(items, &weight_fn);
        Self::get_random_item(random, items, total_weight, weight_fn)
    }
}

/// `net.minecraft.util.random.WeightedList<E>` — a fixed weighted distribution
/// with an internal selector (flat array under 64 total weight, compact walk
/// at/above it).
#[derive(Clone)]
pub struct WeightedList<E> {
    total_weight: i32,
    items: Vec<Weighted<E>>,
    selector: Option<Selector<E>>,
}

impl<E: Clone> WeightedList<E> {
    /// The constructor — `List.copyOf(items)`, then `getTotalWeight`, then the
    /// selector split on `totalWeight`. Panics on overflow
    /// (`"Sum of weights must be <= 2147483647"`) exactly like Java.
    pub fn new(items: &[Weighted<E>]) -> Self {
        let items_vec: Vec<Weighted<E>> = items.to_vec();
        let total_weight = WeightedRandom::get_total_weight(items, |w| w.weight);
        let selector = if total_weight == 0 {
            None
        } else if total_weight < FLAT_THRESHOLD {
            Some(Selector::Flat(FlatSelector::new(&items_vec, total_weight)))
        } else {
            Some(Selector::Compact(CompactSelector::new(&items_vec)))
        };
        WeightedList {
            total_weight,
            items: items_vec,
            selector,
        }
    }

    /// `WeightedList.of()` — the empty list.
    pub fn of() -> Self {
        WeightedList::new(&[])
    }

    /// `WeightedList.of(E value)` — a single element with weight 1.
    pub fn of_value(value: E) -> Self {
        WeightedList::new(&[Weighted::new(value, 1)])
    }

    /// `WeightedList.of(E... items)` — each element with weight 1.
    pub fn of_values(items: &[E]) -> Self {
        let weighted: Vec<Weighted<E>> = items
            .iter()
            .map(|item| Weighted::new(item.clone(), 1))
            .collect();
        WeightedList::new(&weighted)
    }

    /// `WeightedList.of(Weighted<E>... items)` / `of(List<Weighted<E>> items)`
    /// — from pre-weighted entries. Java's varargs and `List` overloads both
    /// take a `List<Weighted<E>>`; in Rust both collapse to a slice.
    pub fn of_weighted_list(items: &[Weighted<E>]) -> Self {
        WeightedList::new(items)
    }

    /// `WeightedList.isEmpty()` — true when there is no selector, i.e. the
    /// total weight is `0` (empty or all-zero weights).
    pub fn is_empty(&self) -> bool {
        self.selector.is_none()
    }

    /// `WeightedList.map(Function<E, T>)` — maps the values (a lazy view in
    /// Java via `Lists.transform`; the port materializes eagerly — the weights
    /// and order are identical and the laziness is not observable through the
    /// public surface). Recomputes the selector from the mapped weights.
    pub fn map<T: Clone>(&self, mapper: impl Fn(&E) -> T) -> WeightedList<T> {
        let mapped: Vec<Weighted<T>> = self
            .items
            .iter()
            .map(|w| Weighted::new(mapper(&w.value), w.weight))
            .collect();
        WeightedList::new(&mapped)
    }

    /// `WeightedList.getRandom(RandomSource)` — `None` when empty (without
    /// consuming the RNG), else one `nextInt(totalWeight)` draw and the
    /// selector's `get`.
    pub fn get_random(&self, random: &mut impl RandomSource) -> Option<E> {
        let selector = self.selector.as_ref()?;
        let selection = random.next_int_bound(self.total_weight);
        Some(selector.get(selection))
    }

    /// `WeightedList.getRandomOrThrow(RandomSource)` — `getRandom` but throws
    /// `IllegalStateException("Weighted list has no elements")` when empty
    /// (the RNG is consumed only when non-empty, mirroring `getRandom`).
    pub fn get_random_or_throw(&self, random: &mut impl RandomSource) -> E {
        match self.get_random(random) {
            Some(value) => value,
            None => {
                log_and_pause_if_in_ide("Weighted list has no elements");
                panic!("Weighted list has no elements");
            }
        }
    }

    /// `WeightedList.unwrap()` — the underlying entries (a fresh copy of the
    /// stored list).
    pub fn unwrap(&self) -> Vec<Weighted<E>> {
        self.items.clone()
    }

    /// `WeightedList.contains(E value)` — whether any entry's value equals
    /// `value` (Java's `item.value().equals(value)`).
    pub fn contains(&self, value: &E) -> bool
    where
        E: PartialEq,
    {
        self.items.iter().any(|item| item.value == *value)
    }
}

/// `WeightedList.codec(Codec<E>)` — a list of `Weighted.codec(element)`.
pub fn weighted_list_codec<E2, Ops>(
    element_codec: Arc<dyn Codec<E2, Ops>>,
) -> Arc<dyn Codec<WeightedList<E2>, Ops>>
where
    E2: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    entry_to_list_codec(weighted_codec(element_codec))
}

/// `WeightedList.codec(MapCodec<E>)` — the element-`MapCodec` overload.
pub fn weighted_list_codec_map<E2, Ops>(
    element_codec: Arc<dyn MapCodec<E2, Ops>>,
) -> Arc<dyn Codec<WeightedList<E2>, Ops>>
where
    E2: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    entry_to_list_codec(weighted_codec_map(element_codec))
}

/// `WeightedList.nonEmptyCodec(Codec<E>)` — `codec` plus a validation that the
/// decoded list is non-empty, else `DataResult.error("Weighted list must
/// contain at least one entry with non-zero weight")`.
pub fn weighted_list_non_empty_codec<E2, Ops>(
    element_codec: Arc<dyn Codec<E2, Ops>>,
) -> Arc<dyn Codec<WeightedList<E2>, Ops>>
where
    E2: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    entry_to_non_empty_list_codec(weighted_codec(element_codec))
}

/// `WeightedList.nonEmptyCodec(MapCodec<E>)` — the element-`MapCodec`
/// overload.
pub fn weighted_list_non_empty_codec_map<E2, Ops>(
    element_codec: Arc<dyn MapCodec<E2, Ops>>,
) -> Arc<dyn Codec<WeightedList<E2>, Ops>>
where
    E2: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    entry_to_non_empty_list_codec(weighted_codec_map(element_codec))
}

/// `entryToListCodec` — `weightedElementCodec.listOf().xmap(WeightedList::of,
/// WeightedList::unwrap)`.
fn entry_to_list_codec<E2, Ops>(
    weighted_element_codec: Arc<dyn Codec<Weighted<E2>, Ops>>,
) -> Arc<dyn Codec<WeightedList<E2>, Ops>>
where
    E2: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    codec::xmap(
        codec::list(weighted_element_codec),
        Arc::new(|entries: &Vec<Weighted<E2>>| WeightedList::new(entries)),
        Arc::new(|list: &WeightedList<E2>| list.unwrap()),
    )
}

/// `entryToNonEmptyListCodec` — `entryToListCodec` with a non-empty
/// `validate`.
fn entry_to_non_empty_list_codec<E2, Ops>(
    weighted_element_codec: Arc<dyn Codec<Weighted<E2>, Ops>>,
) -> Arc<dyn Codec<WeightedList<E2>, Ops>>
where
    E2: 'static + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    let base = entry_to_list_codec(weighted_element_codec);
    codec::validate(
        base,
        Arc::new(|list: &WeightedList<E2>| {
            if list.is_empty() {
                DataResult::error(
                    "Weighted list must contain at least one entry with non-zero weight",
                )
            } else {
                DataResult::success(list.clone())
            }
        }),
    )
}

impl<E: PartialEq> PartialEq for WeightedList<E> {
    /// `WeightedList.equals` — `this == obj || (obj instanceof WeightedList &&
    /// totalWeight == obj.totalWeight && Objects.equals(items, obj.items))`.
    fn eq(&self, other: &Self) -> bool {
        self.total_weight == other.total_weight && self.items == other.items
    }
}

impl<E: PartialEq> Eq for WeightedList<E> {}

impl<E: fmt::Debug> fmt::Debug for WeightedList<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // WeightedList has no Java toString (identity-based Object.toString);
        // derive Debug over the entries for Rust ergonomics.
        f.debug_struct("WeightedList")
            .field("total_weight", &self.total_weight)
            .field("items", &self.items)
            .finish()
    }
}

/// The selector abstraction (Java's private `WeightedList.Selector<E>`).
#[derive(Clone)]
enum Selector<E> {
    /// `WeightedList.Flat<E>` — an array of length `totalWeight`; `get` is a
    /// direct index.
    Flat(FlatSelector<E>),
    /// `WeightedList.Compact<E>` — a walk over the entries; `get` throws on
    /// fall-through.
    Compact(CompactSelector<E>),
}

impl<E: Clone> Selector<E> {
    fn get(&self, selection: i32) -> E {
        match self {
            Selector::Flat(flat) => flat.get(selection),
            Selector::Compact(compact) => compact.get(selection),
        }
    }
}

/// `WeightedList.Flat<E>`.
#[derive(Clone)]
struct FlatSelector<E> {
    /// The `Object[] entries` — each item's value repeated `weight` times.
    entries: Vec<E>,
}

impl<E: Clone> FlatSelector<E> {
    fn new(entries: &[Weighted<E>], total_weight: i32) -> Self {
        let mut flat = Vec::with_capacity(total_weight as usize);
        for entry in entries {
            let weight = entry.weight;
            for _ in 0..weight {
                flat.push(entry.value.clone());
            }
        }
        FlatSelector { entries: flat }
    }

    /// `Flat.get(int selection)` — `(E) entries[selection]`.
    fn get(&self, selection: i32) -> E {
        // Java indexes `entries[selection]` with selection in `[0, totalWeight)`
        // (guaranteed by `nextInt(totalWeight)`); out-of-range here would be a
        // caller bug and panics like any out-of-bounds index.
        self.entries[selection as usize].clone()
    }
}

/// `WeightedList.Compact<E>`.
#[derive(Clone)]
struct CompactSelector<E> {
    /// The `Weighted<?>[] entries`.
    entries: Vec<Weighted<E>>,
}

impl<E: Clone> CompactSelector<E> {
    fn new(entries: &[Weighted<E>]) -> Self {
        CompactSelector {
            entries: entries.to_vec(),
        }
    }

    /// `Compact.get(int selection)` — walk subtracting weights; on fall-through
    /// throws `IllegalStateException(selection + " exceeded total weight")`.
    fn get(&self, mut selection: i32) -> E {
        for entry in &self.entries {
            selection = selection.wrapping_sub(entry.weight);
            if selection < 0 {
                return entry.value.clone();
            }
        }
        log_and_pause_if_in_ide(&format!("{} exceeded total weight", selection));
        panic!("{} exceeded total weight", selection);
    }
}

/// `WeightedList.Builder<E>`.
#[derive(Debug, Clone, Default)]
pub struct WeightedListBuilder<E> {
    /// The `ImmutableList.Builder<Weighted<E>>` accumulation.
    result: Vec<Weighted<E>>,
}

impl<E> WeightedListBuilder<E> {
    /// `Builder.add(E item)` — weight 1.
    pub fn add(&mut self, item: E) -> &mut Self {
        self.add_weighted(item, 1)
    }

    /// `Builder.add(E item, int weight)`.
    pub fn add_weighted(&mut self, item: E, weight: i32) -> &mut Self {
        self.result.push(Weighted::new(item, weight));
        self
    }

    /// `Builder.build()`.
    pub fn build(&self) -> WeightedList<E>
    where
        E: Clone,
    {
        WeightedList::new(&self.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::{LegacyRandomSource, RandomSource, XoroshiroRandomSource};
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    // --- WeightedRandom selection goldens (Paper 26.2, see Java probe) ---

    /// The `[("a",1), ("b",3), ("c",2)]` item set used across the goldens.
    fn abc() -> Vec<Weighted<&'static str>> {
        vec![
            Weighted::new("a", 1),
            Weighted::new("b", 3),
            Weighted::new("c", 2),
        ]
    }

    #[test]
    fn weighted_random_get_random_item_seeded() {
        let mut r = LegacyRandomSource::new(12345);
        let items = abc();
        let got: Vec<&str> = (0..12)
            .map(|_| {
                WeightedRandom::get_random_item_from_total(&mut r, &items, |w: &Weighted<&str>| {
                    w.weight
                })
                .unwrap()
                .value
            })
            .collect();
        assert_eq!(
            got,
            vec!["b", "c", "b", "a", "b", "c", "b", "a", "b", "b", "c", "a"]
        );
    }

    #[test]
    fn weighted_random_get_random_item_explicit_total() {
        let mut r = LegacyRandomSource::new(999);
        let items = abc();
        let got: Vec<&str> = (0..6)
            .map(|_| {
                WeightedRandom::get_random_item(&mut r, &items, 6, |w: &Weighted<&str>| w.weight)
                    .unwrap()
                    .value
            })
            .collect();
        assert_eq!(got, vec!["b", "b", "c", "b", "b", "b"]);
    }

    #[test]
    fn weighted_random_get_weighted_item_indexes() {
        let items = abc();
        let got: Vec<Option<&str>> = (0i32..7)
            .map(|idx| {
                WeightedRandom::get_weighted_item(&items, idx, |w: &Weighted<&str>| w.weight)
                    .map(|w| w.value)
            })
            .collect();
        assert_eq!(
            got,
            vec![
                Some("a"),
                Some("b"),
                Some("b"),
                Some("b"),
                Some("c"),
                Some("c"),
                None // index == total (6) falls through
            ]
        );
    }

    #[test]
    fn weighted_random_get_total_weight_custom() {
        // Negative via a custom getter (Weighted ctor forbids negatives).
        let items = vec![Weighted::new("a", 5), Weighted::new("b", 7)];
        assert_eq!(
            WeightedRandom::get_total_weight(&items, |w: &Weighted<&str>| w.weight - 5),
            2
        );
        assert_eq!(
            WeightedRandom::get_total_weight::<Weighted<&str>>(&[], |_| 1),
            0
        );
    }

    #[test]
    #[should_panic(expected = "Sum of weights must be <= 2147483647")]
    fn weighted_random_get_total_weight_overflows() {
        let items = vec![Weighted::new("a", 1), Weighted::new("b", 1)];
        let _ = WeightedRandom::get_total_weight(&items, |_| i32::MAX);
    }

    #[test]
    #[should_panic(expected = "Negative total weight in getRandomItem")]
    fn weighted_random_negative_total_panics() {
        let mut r = LegacyRandomSource::new(1);
        let _ = WeightedRandom::get_random_item::<Weighted<&str>>(&mut r, &[], -5, |_| 1);
    }

    #[test]
    fn weighted_random_zero_total_returns_none_without_consuming_rng() {
        let items = vec![Weighted::new("a", 0)];
        let mut r = LegacyRandomSource::new(1);
        assert_eq!(
            WeightedRandom::get_random_item(&mut r, &items, 0, |w: &Weighted<&str>| w.weight),
            None
        );
        // The RNG must not be consumed by a zero-total call.
        assert_eq!(r.next_int(), -1155869325); // seed 1 first draw (Java golden)
    }

    #[test]
    fn weighted_random_zero_weight_items() {
        // [a:0, b:2, c:0] — total 2, selection always 0 or 1 -> always "b".
        let items = vec![
            Weighted::new("a", 0),
            Weighted::new("b", 2),
            Weighted::new("c", 0),
        ];
        let mut r = LegacyRandomSource::new(4242);
        let got: Vec<&str> = (0..8)
            .map(|_| {
                WeightedRandom::get_random_item_from_total(&mut r, &items, |w: &Weighted<&str>| {
                    w.weight
                })
                .unwrap()
                .value
            })
            .collect();
        assert_eq!(got, vec!["b"; 8]);
    }

    #[test]
    fn weighted_random_get_weighted_item_negative_index() {
        let items = abc();
        // index -1 < 0 after subtracting weight 1 -> first item.
        assert_eq!(
            WeightedRandom::get_weighted_item(&items, -1, |w: &Weighted<&str>| w.weight)
                .map(|w| w.value),
            Some("a")
        );
    }

    // --- WeightedList selector goldens ---

    #[test]
    fn flat_selector_seeded_sequence() {
        let wl = WeightedList::new(&abc());
        let mut r = LegacyRandomSource::new(12345);
        let got: Vec<&str> = (0..12).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(
            got,
            vec!["b", "c", "b", "a", "b", "c", "b", "a", "b", "b", "c", "a"]
        );
    }

    #[test]
    fn compact_selector_seeded_sequence() {
        // total 90 >= 64 -> Compact.
        let wl = WeightedList::new(&[
            Weighted::new("x", 20),
            Weighted::new("y", 30),
            Weighted::new("z", 40),
        ]);
        let mut r = LegacyRandomSource::new(12345);
        let got: Vec<&str> = (0..12).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(
            got,
            vec!["y", "y", "z", "x", "z", "x", "y", "z", "z", "z", "z", "y"]
        );
    }

    #[test]
    fn compact_selector_with_zero_weight_entry() {
        let wl = WeightedList::new(&[
            Weighted::new("zero", 0),
            Weighted::new("x", 20),
            Weighted::new("y", 30),
            Weighted::new("z", 40),
        ]);
        let mut r = LegacyRandomSource::new(12345);
        let got: Vec<&str> = (0..8).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["y", "y", "z", "x", "z", "x", "y", "z"]);
    }

    #[test]
    fn flat_selector_boundary() {
        // total 5 < 64 -> Flat.
        let wl = WeightedList::new(&[Weighted::new("a", 2), Weighted::new("b", 3)]);
        let mut r = LegacyRandomSource::new(55);
        let got: Vec<&str> = (0..10).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["a", "b", "b", "b", "b", "a", "b", "a", "b", "b"]);
    }

    #[test]
    fn flat_selector_ten_total_boundary() {
        let wl = WeightedList::new(&[Weighted::new("a", 4), Weighted::new("b", 6)]);
        let mut r = LegacyRandomSource::new(11);
        let got: Vec<&str> = (0..10).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["b", "b", "a", "b", "a", "b", "a", "a", "a", "b"]);
    }

    #[test]
    fn threshold_64_uses_compact() {
        // total exactly 64 -> Compact (>= threshold); every draw is "big".
        let wl = WeightedList::new(&[Weighted::new("big", 64)]);
        let mut r = LegacyRandomSource::new(12345);
        let got: Vec<&str> = (0..8).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["big"; 8]);
    }

    #[test]
    fn builder_order_and_selection() {
        let mut builder = WeightedListBuilder::default();
        builder.add_weighted("p", 2).add_weighted("q", 1);
        let wl = builder.build();
        let mut r = LegacyRandomSource::new(777);
        let got: Vec<&str> = (0..9).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["p", "p", "q", "p", "q", "p", "p", "q", "p"]);
        // Builder preserves insertion order in unwrap.
        assert_eq!(
            wl.unwrap(),
            vec![Weighted::new("p", 2), Weighted::new("q", 1)]
        );
    }

    #[test]
    fn varargs_of_values_seeded_sequence() {
        let wl = WeightedList::of_values(&["a", "b", "c"]);
        assert_eq!(
            WeightedRandom::get_total_weight(&wl.unwrap(), |w: &Weighted<&str>| w.weight),
            3
        );
        let mut r = LegacyRandomSource::new(5);
        let got: Vec<&str> = (0..8).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["c", "b", "c", "c", "a", "c", "b", "c"]);
    }

    #[test]
    fn map_preserves_weights_and_order() {
        let wl = WeightedList::new(&[Weighted::new("a", 1), Weighted::new("b", 2)]);
        let mapped = wl.map(|s| s.to_uppercase());
        assert_eq!(
            mapped.unwrap(),
            vec![
                Weighted::new("A".to_string(), 1),
                Weighted::new("B".to_string(), 2)
            ]
        );
        let mut r = LegacyRandomSource::new(3);
        let got: Vec<String> = (0..6).map(|_| mapped.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["B", "B", "A", "B", "A", "A"]);
    }

    // --- empty / zero-weight semantics ---

    #[test]
    fn empty_list_is_empty_and_does_not_consume_rng() {
        let wl = WeightedList::<&str>::of();
        assert!(wl.is_empty());
        let mut r = LegacyRandomSource::new(12345);
        assert_eq!(wl.get_random(&mut r), None);
        assert_eq!(r.next_int(), 1553932502); // RNG untouched (seed 12345 first draw)
    }

    #[test]
    #[should_panic(expected = "Weighted list has no elements")]
    fn empty_get_random_or_throw_panics() {
        let wl = WeightedList::<&str>::of();
        let mut r = LegacyRandomSource::new(1);
        let _ = wl.get_random_or_throw(&mut r);
    }

    #[test]
    fn all_zero_weights_is_empty() {
        let mut builder = WeightedListBuilder::default();
        builder.add_weighted("a", 0);
        let wl = builder.build();
        assert!(wl.is_empty());
        let mut r = LegacyRandomSource::new(1);
        assert_eq!(wl.get_random(&mut r), None);
    }

    // --- validation ---

    #[test]
    #[should_panic(expected = "Weight should be >= 0")]
    fn weighted_rejects_negative_weight() {
        let _ = Weighted::new("x", -1);
    }

    #[test]
    #[should_panic(expected = "Sum of weights must be <= 2147483647")]
    fn weighted_list_weight_sum_overflow() {
        let _ = WeightedList::new(&[Weighted::new("a", i32::MAX), Weighted::new("b", 1)]);
    }

    #[test]
    fn weighted_zero_weight_is_legal() {
        let w = Weighted::new("x", 0);
        assert_eq!(w.weight, 0);
    }

    // --- equals / contains / unwrap ---

    #[test]
    fn weighted_list_equals_and_contains() {
        let a1 = WeightedList::new(&[Weighted::new("x", 1), Weighted::new("y", 2)]);
        let a2 = WeightedList::new(&[Weighted::new("x", 1), Weighted::new("y", 2)]);
        let a3 = WeightedList::new(&[Weighted::new("x", 1), Weighted::new("y", 3)]);
        assert_eq!(a1, a2);
        assert_ne!(a1, a3);
        assert!(a1.contains(&"x"));
        assert!(!a1.contains(&"z"));
    }

    // --- codecs (Paper 26.2 JsonOps goldens) ---

    #[test]
    fn weighted_codec_round_trip() {
        let codec = weighted_codec::<String, JsonOps>(rivet_serialization::codec::string_codec::<
            JsonOps,
        >());
        let input = json!({"data": "q", "weight": 7});
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, Weighted::new("q".to_string(), 7));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(encoded, json!({"data": "q", "weight": 7}));
    }

    #[test]
    fn weighted_codec_rejects_negative_weight_with_exact_message() {
        let codec = weighted_codec::<String, JsonOps>(rivet_serialization::codec::string_codec::<
            JsonOps,
        >());
        let input = json!({"data": "q", "weight": -1});
        let error = codec
            .parse(&JsonOps::INSTANCE, &input)
            .error_ref()
            .map(|e| e.message().to_string());
        assert_eq!(error.as_deref(), Some("Value must be non-negative: -1"));
    }

    #[test]
    fn weighted_list_codec_round_trip() {
        let codec =
            weighted_list_codec::<String, JsonOps>(rivet_serialization::codec::string_codec::<
                JsonOps,
            >());
        let wl = WeightedList::new(&[
            Weighted::new("a".to_string(), 1),
            Weighted::new("b".to_string(), 2),
        ]);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &wl)
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(
            encoded,
            json!([{"data": "a", "weight": 1}, {"data": "b", "weight": 2}])
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, wl);
    }

    #[test]
    fn weighted_list_codec_rejects_negative_weight_in_array() {
        let codec =
            weighted_list_codec::<String, JsonOps>(rivet_serialization::codec::string_codec::<
                JsonOps,
            >());
        let input = json!([{"data": "a", "weight": -1}]);
        let error = codec
            .parse(&JsonOps::INSTANCE, &input)
            .error_ref()
            .map(|e| e.message().to_string());
        assert_eq!(error.as_deref(), Some("Value must be non-negative: -1"));
    }

    #[test]
    fn weighted_list_non_empty_codec_rejects_empty() {
        let codec = weighted_list_non_empty_codec::<String, JsonOps>(
            rivet_serialization::codec::string_codec::<JsonOps>(),
        );
        let error = codec
            .parse(&JsonOps::INSTANCE, &json!([]))
            .error_ref()
            .map(|e| e.message().to_string());
        assert_eq!(
            error.as_deref(),
            Some("Weighted list must contain at least one entry with non-zero weight")
        );
        // Encode of an empty list fails the same way (validate runs both ways).
        let wl = WeightedList::<String>::of();
        let error = codec
            .encode_start(&JsonOps::INSTANCE, &wl)
            .error_ref()
            .map(|e| e.message().to_string());
        assert_eq!(
            error.as_deref(),
            Some("Weighted list must contain at least one entry with non-zero weight")
        );
    }

    #[test]
    fn weighted_list_non_empty_codec_accepts_non_empty() {
        let codec = weighted_list_non_empty_codec::<String, JsonOps>(
            rivet_serialization::codec::string_codec::<JsonOps>(),
        );
        let input = json!([{"data": "ok", "weight": 1}]);
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(decoded.unwrap(), vec![Weighted::new("ok".to_string(), 1)]);
    }

    #[test]
    fn weighted_map_codec_variant_uses_element_map_codec() {
        // Weighted.codec(MapCodec<E>) — the element codec IS a map codec, no
        // extra fieldOf wrap; the "data" key is the element's own field.
        let element_map = rivet_serialization::codec::field_of::<String, JsonOps>(
            rivet_serialization::codec::string_codec::<JsonOps>(),
            "data".to_string(),
        );
        let codec = weighted_codec_map::<String, JsonOps>(element_map);
        let input = json!({"data": "q", "weight": 7});
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(decoded, Weighted::new("q".to_string(), 7));
    }

    #[test]
    fn weighted_list_map_codec_variant() {
        let element_map = rivet_serialization::codec::field_of::<String, JsonOps>(
            rivet_serialization::codec::string_codec::<JsonOps>(),
            "data".to_string(),
        );
        let codec = weighted_list_codec_map::<String, JsonOps>(element_map);
        let input = json!([
            {"data": "m1", "weight": 3},
            {"data": "m2", "weight": 5}
        ]);
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(
            decoded.unwrap(),
            vec![
                Weighted::new("m1".to_string(), 3),
                Weighted::new("m2".to_string(), 5)
            ]
        );
    }

    #[test]
    fn xoroshiro_weighted_selection_matches_java() {
        // Compact list under XoroshiroRandomSource (paper Java golden).
        let wl = WeightedList::new(&[
            Weighted::new("x", 20),
            Weighted::new("y", 30),
            Weighted::new("z", 40),
        ]);
        let mut r = XoroshiroRandomSource::new(12345);
        let got: Vec<&str> = (0..8).map(|_| wl.get_random(&mut r).unwrap()).collect();
        assert_eq!(got, vec!["x", "z", "z", "x", "y", "z", "x", "y"]);
        // Flat list under XoroshiroRandomSource.
        let wl2 = WeightedList::new(&abc());
        let mut r2 = XoroshiroRandomSource::new(12345);
        let got2: Vec<&str> = (0..8).map(|_| wl2.get_random(&mut r2).unwrap()).collect();
        assert_eq!(got2, vec!["a", "c", "c", "a", "b", "c", "b", "b"]);
    }
}

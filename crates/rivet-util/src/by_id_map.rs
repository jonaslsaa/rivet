//! **Full** port of `net.minecraft.util.ByIdMap` (85-line Java class, ported
//! wholesale — `Direction.BY_ID` in the registry-core slice needs
//! `ByIdMap.continuous`, and `net.minecraft.core.IdMap` uses `sparse`).
//!
//! PROVENANCE: leaf of the `mc.util` manifest unit (net.minecraft.util ->
//! rivet-util). RECONCILIATION: stays in this module when the full unit lands.
//!
//! Java/Rust mapping notes:
//! - Java's `IntFunction<T>` (an `int -> T` boxed function) maps to
//!   `Arc<dyn Fn(i32) -> T>`; the id argument is always an `i32` (Java `int`).
//! - `T` is required `Copy`: the returned closures hand out owned values, the
//!   analogue of Java returning object references to (interned) constants.
//! - `Mth.positiveModulo` / `Mth.clamp` come from `rivet_util::mth`, matching
//!   the Java calls into `net.minecraft.util.Mth`.
//! - Every failure path panics with Java's exact `IllegalArgumentException`
//!   message (including the upstream "continous" typo) — these are programmer
//!   errors thrown at construction, never caught at runtime.

use std::collections::HashMap;
use std::sync::Arc;

/// `ByIdMap.OutOfBoundsStrategy` — how a `continuous` id lookup handles an
/// id outside `[0, values.length)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutOfBoundsStrategy {
    /// `ZERO` — fall back to the value at index 0.
    Zero,
    /// `WRAP` — `sortedValues[Mth.positiveModulo(id, length)]`.
    Wrap,
    /// `CLAMP` — `sortedValues[Mth.clamp(id, 0, length - 1)]`.
    Clamp,
}

/// `ByIdMap.createMap(ToIntFunction, T[])` — the `Int2ObjectOpenHashMap`
/// build shared by `sparse` and `continuous`. `Int2ObjectOpenHashMap.put`
/// returns the previous value, so a duplicate id is a construction error.
fn create_map<T: Copy + std::fmt::Debug>(
    id_getter: impl Fn(&T) -> i32,
    values: &[T],
) -> HashMap<i32, T> {
    if values.is_empty() {
        panic!("Empty value list");
    }

    let mut result = HashMap::new();
    for value in values {
        let id = id_getter(value);
        if let Some(previous) = result.insert(id, *value) {
            panic!("Duplicate entry on id {id}: current={value:?}, previous={previous:?}");
        }
    }
    result
}

/// `ByIdMap.sparse(ToIntFunction<T>, T[], T _default)` — an id lookup that
/// returns `_default` for any id with no entry (Java
/// `Objects.requireNonNullElse(map.get(id), _default)`).
pub fn sparse<T: Copy + std::fmt::Debug + 'static>(
    id_getter: impl Fn(&T) -> i32,
    values: &[T],
    default: T,
) -> Arc<dyn Fn(i32) -> T> {
    let map = create_map(id_getter, values);
    Arc::new(move |id| map.get(&id).copied().unwrap_or(default))
}

/// `ByIdMap.createSortedArray(ToIntFunction<T>, T[])` — the dense
/// `id -> value` array. Validates empty, non-continuous (`id < 0 || id >=
/// length`), duplicate, and missing ids — all `IllegalArgumentException`s.
fn create_sorted_array<T: Copy + std::fmt::Debug>(
    id_getter: impl Fn(&T) -> i32,
    values: &[T],
) -> Vec<T> {
    let length = values.len();
    if length == 0 {
        panic!("Empty value list");
    }

    let mut result: Vec<Option<T>> = vec![None; length];
    for value in values {
        let id = id_getter(value);
        if id < 0 || id as usize >= length {
            panic!("Values are not continous, found index {id} for value {value:?}");
        }
        let slot = &mut result[id as usize];
        if slot.is_some() {
            panic!("Duplicate entry on id {id}: current={value:?}, previous={slot:?}");
        }
        *slot = Some(*value);
    }

    for (i, slot) in result.iter().enumerate() {
        if slot.is_none() {
            panic!("Missing value at index: {i}");
        }
    }

    result.into_iter().map(|slot| slot.unwrap()).collect()
}

/// `ByIdMap.continuous(ToIntFunction<T>, T[], OutOfBoundsStrategy)` — the
/// dense `id -> value` array with the strategy's out-of-bounds handling.
pub fn continuous<T: Copy + std::fmt::Debug + 'static>(
    id_getter: impl Fn(&T) -> i32,
    values: &[T],
    strategy: OutOfBoundsStrategy,
) -> Arc<dyn Fn(i32) -> T> {
    let sorted = create_sorted_array(id_getter, values);
    let length = sorted.len() as i32;

    match strategy {
        OutOfBoundsStrategy::Zero => {
            let zero_value = sorted[0];
            Arc::new(move |id| {
                if id >= 0 && id < length {
                    sorted[id as usize]
                } else {
                    zero_value
                }
            })
        }
        OutOfBoundsStrategy::Wrap => {
            Arc::new(move |id| sorted[crate::mth::positive_modulo(id, length) as usize])
        }
        OutOfBoundsStrategy::Clamp => {
            Arc::new(move |id| sorted[crate::mth::clamp(id, 0, length - 1) as usize])
        }
    }
}

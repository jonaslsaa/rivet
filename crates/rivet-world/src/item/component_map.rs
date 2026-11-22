//! Port of `net.minecraft.core.component.DataComponentGetter`,
//! `net.minecraft.core.component.DataComponentHolder`, and
//! `net.minecraft.core.component.DataComponentMap`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use rivet_registry::Identifier;
use rivet_serialization::{Codec, DataResult, DynamicOps};

use crate::item::component_type::{
    DataComponentTypeId, DataComponentValue, TypedDataComponent, decode_component_value,
    encode_component_value,
};

/// Port of `net.minecraft.core.component.DataComponentGetter`.
pub trait DataComponentGetter {
    fn get(&self, component_type: DataComponentTypeId) -> Option<&DataComponentValue>;

    fn get_or_default<'a>(
        &'a self,
        component_type: DataComponentTypeId,
        default_value: &'a DataComponentValue,
    ) -> &'a DataComponentValue {
        self.get(component_type).unwrap_or(default_value)
    }

    fn has(&self, component_type: DataComponentTypeId) -> bool {
        self.get(component_type).is_some()
    }

    fn get_i32(&self, component_type: DataComponentTypeId) -> Option<i32> {
        self.get(component_type)
            .and_then(DataComponentValue::as_i32)
    }

    fn get_i32_or(&self, component_type: DataComponentTypeId, default: i32) -> i32 {
        self.get_i32(component_type).unwrap_or(default)
    }

    fn get_bool(&self, component_type: DataComponentTypeId) -> Option<bool> {
        self.get(component_type)
            .and_then(DataComponentValue::as_bool)
    }
}

/// Port of `net.minecraft.core.component.DataComponentHolder`.
pub trait DataComponentHolder {
    fn components(&self) -> &DataComponentMap;

    fn get_component(&self, component_type: DataComponentTypeId) -> Option<&DataComponentValue> {
        self.components().get(component_type)
    }

    fn has_component(&self, component_type: DataComponentTypeId) -> bool {
        self.components().has(component_type)
    }
}

/// Port of `net.minecraft.core.component.DataComponentMap`.
///
/// Immutable, identity-keyed map from `DataComponentTypeId` to
/// `DataComponentValue` with deterministic insertion-order iteration via
/// `IndexMap`.
#[derive(Clone, Debug, Default)]
pub struct DataComponentMap {
    map: IndexMap<DataComponentTypeId, DataComponentValue>,
}

impl DataComponentMap {
    /// Returns an empty `DataComponentMap` (`DataComponentMap.EMPTY`).
    pub fn empty() -> Self {
        Self {
            map: IndexMap::new(),
        }
    }

    /// Creates a `DataComponentMap` from an ordered `IndexMap`.
    pub(crate) fn from_index_map(map: IndexMap<DataComponentTypeId, DataComponentValue>) -> Self {
        Self { map }
    }

    /// Starts a new `DataComponentMapBuilder` (`DataComponentMap.builder()`).
    pub fn builder() -> DataComponentMapBuilder {
        DataComponentMapBuilder::new()
    }

    /// Port of `DataComponentMap.composite(prototype, overrides)`.
    ///
    /// Lookup checks `overrides` first, then `prototype`. Iteration yields
    /// effective entries with `overrides` values taking precedence.
    pub fn composite(prototype: &DataComponentMap, overrides: &DataComponentMap) -> Self {
        let mut combined = prototype.map.clone();
        for (&k, v) in &overrides.map {
            combined.insert(k, v.clone());
        }
        Self { map: combined }
    }

    /// Port of `DataComponentMap.filter(Predicate<DataComponentType<?>>)`.
    pub fn filter<F>(&self, mut predicate: F) -> Self
    where
        F: FnMut(DataComponentTypeId) -> bool,
    {
        let mut filtered = IndexMap::new();
        for (&k, v) in &self.map {
            if predicate(k) {
                filtered.insert(k, v.clone());
            }
        }
        Self { map: filtered }
    }

    /// Returns the number of entries in this map (`DataComponentMap.size()`).
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if this map has no entries (`DataComponentMap.isEmpty()`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns the ordered set of keys (`DataComponentMap.keySet()`).
    pub fn key_set(&self) -> IndexSet<DataComponentTypeId> {
        self.map.keys().copied().collect()
    }

    /// Iterates over entries as `(DataComponentTypeId, &DataComponentValue)`.
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (DataComponentTypeId, &DataComponentValue)> {
        self.map.iter().map(|(&k, v)| (k, v))
    }

    /// Iterates over entries as `TypedDataComponent` values, matching Paper's
    /// `Iterable<TypedDataComponent<?>>`.
    pub fn typed_iter(&self) -> impl ExactSizeIterator<Item = TypedDataComponent> + '_ {
        self.map
            .iter()
            .map(|(&k, v)| TypedDataComponent::new(k, v.clone()))
    }

    /// Java `AbstractMap.hashCode()` — order-independent sum of
    /// `key.hashCode() ^ value.hashCode()`, wrapping in `i32`.
    pub fn java_hash_code(&self) -> i32 {
        let mut sum: i32 = 0;
        for (&k, v) in &self.map {
            let entry_hash = k.java_hash_code() ^ v.java_hash_code();
            sum = sum.wrapping_add(entry_hash);
        }
        sum
    }
}

impl DataComponentGetter for DataComponentMap {
    #[inline]
    fn get(&self, component_type: DataComponentTypeId) -> Option<&DataComponentValue> {
        self.map.get(&component_type)
    }

    #[inline]
    fn has(&self, component_type: DataComponentTypeId) -> bool {
        self.map.contains_key(&component_type)
    }
}

impl PartialEq for DataComponentMap {
    fn eq(&self, other: &Self) -> bool {
        if self.map.len() != other.map.len() {
            return false;
        }
        self.map
            .iter()
            .all(|(k, v)| other.map.get(k).is_some_and(|ov| ov == v))
    }
}

impl Eq for DataComponentMap {}

impl Hash for DataComponentMap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.java_hash_code().hash(state);
    }
}

impl fmt::Display for DataComponentMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (idx, (k, v)) in self.map.iter().enumerate() {
            if idx > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{k}=>{v}")?;
        }
        f.write_str("}")
    }
}

type MapValidator = Arc<dyn Fn(&DataComponentMap) -> Result<(), String> + Send + Sync>;

/// Port of `DataComponentMap.Builder`.
#[derive(Clone, Default)]
pub struct DataComponentMapBuilder {
    map: IndexMap<DataComponentTypeId, DataComponentValue>,
    validators: Vec<MapValidator>,
}

impl fmt::Debug for DataComponentMapBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataComponentMapBuilder")
            .field("map", &self.map)
            .field("validator_count", &self.validators.len())
            .finish()
    }
}

impl DataComponentMapBuilder {
    pub fn new() -> Self {
        Self {
            map: IndexMap::new(),
            validators: Vec::new(),
        }
    }

    /// `Builder.set(type, value)` — inserts a non-null value.
    pub fn set(
        &mut self,
        component_type: DataComponentTypeId,
        value: DataComponentValue,
    ) -> &mut Self {
        self.map.insert(component_type, value);
        self
    }

    /// `Builder.set(type, @Nullable value)` — inserts `Some(v)` or removes on `None`.
    pub fn set_opt(
        &mut self,
        component_type: DataComponentTypeId,
        value: Option<DataComponentValue>,
    ) -> &mut Self {
        match value {
            Some(v) => {
                self.map.insert(component_type, v);
            }
            None => {
                self.map.shift_remove(&component_type);
            }
        }
        self
    }

    /// `Builder.addAll(DataComponentMap)` — preserves iteration order.
    pub fn add_all(&mut self, other: &DataComponentMap) -> &mut Self {
        for (k, v) in other.iter() {
            self.map.insert(k, v.clone());
        }
        self
    }

    /// `Builder.addValidator(Validator)` — composes validators executed during `build()`.
    pub fn add_validator<F>(&mut self, validator: F) -> &mut Self
    where
        F: Fn(&DataComponentMap) -> Result<(), String> + Send + Sync + 'static,
    {
        self.validators.push(Arc::new(validator));
        self
    }

    /// `Builder.build()` — runs all composed validators and returns the immutable map.
    pub fn build(self) -> Result<DataComponentMap, String> {
        let map = if self.map.is_empty() {
            DataComponentMap::empty()
        } else {
            DataComponentMap::from_index_map(self.map)
        };
        for v in &self.validators {
            v(&map)?;
        }
        Ok(map)
    }
}

/// Port of `DataComponentMap.CODEC` (`makeCodec(DataComponentType.CODEC)`).
///
/// - Encoding skips transient components (`!type.is_transient()`), matching
///   Paper's `filter(c -> !c.isTransient())`.
/// - Decoding rejects unknown component IDs and transient component types.
pub fn data_component_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DataComponentMap, Ops>>
{
    crate::item::codec_from_fns(
        "DataComponentMap",
        |value: &DataComponentMap, ops: &Ops| {
            let mut entries = Vec::new();
            for (k, v) in value.iter() {
                if k.is_transient() {
                    continue;
                }
                let Some(name) = k.name() else {
                    return DataResult::error(format!("Unregistered component type: {k}"));
                };
                let encoded = match crate::item::data_result_into(encode_component_value(k, v, ops))
                {
                    Ok(val) => val,
                    Err(err) => return DataResult::error(err),
                };
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string(name.to_string()),
                    encoded,
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let pairs = match crate::item::data_result_into(ops.get_map_values(input)) {
                Ok(p) => p,
                Err(err) => return DataResult::error(err),
            };
            let mut builder = DataComponentMapBuilder::new();
            for rivet_serialization::Pair {
                first: k_raw,
                second: v_raw,
            } in pairs
            {
                let key_str = match crate::item::data_result_into(ops.get_string_value(&k_raw)) {
                    Ok(s) => s,
                    Err(err) => return DataResult::error(err),
                };
                let parsed_id = match crate::item::data_result_into(Identifier::read(&key_str)) {
                    Ok(id) => id,
                    Err(err) => return DataResult::error(err),
                };
                let Some(comp_type) = DataComponentTypeId::from_identifier(&parsed_id) else {
                    return DataResult::error(format!("Unknown component type: {parsed_id}"));
                };
                if comp_type.is_transient() {
                    return DataResult::error(format!(
                        "Component type {comp_type} is transient and cannot be persisted"
                    ));
                }
                let val = match crate::item::data_result_into(decode_component_value(
                    comp_type, ops, &v_raw,
                )) {
                    Ok(v) => v,
                    Err(err) => return DataResult::error(err),
                };
                builder.set(comp_type, val);
            }
            match builder.build() {
                Ok(map) => DataResult::success(map),
                Err(err) => DataResult::error(err),
            }
        },
    )
}

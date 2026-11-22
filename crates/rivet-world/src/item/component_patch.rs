//! Port of `net.minecraft.core.component.DataComponentPatch` and
//! `net.minecraft.core.component.PatchedDataComponentMap`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};
use rivet_registry::Identifier;
use rivet_serialization::{Codec, DataResult, DynamicOps, Unit, codec};

use crate::item::component_map::{DataComponentGetter, DataComponentMap, DataComponentMapBuilder};
use crate::item::component_type::{
    DataComponentTypeId, DataComponentValue, TypedDataComponent, decode_component_value,
    encode_component_value,
};

/// Prefix denoting component removal in `DataComponentPatch.CODEC` (`"!minecraft:damage"`).
pub const REMOVED_PREFIX: char = '!';

/// Result of `DataComponentPatch::split()`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SplitResult {
    pub added: DataComponentMap,
    pub removed: IndexSet<DataComponentTypeId>,
}

/// Port of `net.minecraft.core.component.DataComponentPatch`.
///
/// Maps each modified `DataComponentTypeId` to:
/// - `Some(value)` — component is set to `value`
/// - `None` — component is explicitly removed (`!type`)
#[derive(Clone, Debug, Default)]
pub struct DataComponentPatch {
    map: IndexMap<DataComponentTypeId, Option<DataComponentValue>>,
}

impl DataComponentPatch {
    /// Returns an empty `DataComponentPatch` (`DataComponentPatch.EMPTY`).
    pub fn empty() -> Self {
        Self {
            map: IndexMap::new(),
        }
    }

    pub(crate) fn from_index_map(
        map: IndexMap<DataComponentTypeId, Option<DataComponentValue>>,
    ) -> Self {
        Self { map }
    }

    /// Starts a new `DataComponentPatchBuilder` (`DataComponentPatch.builder()`).
    pub fn builder() -> DataComponentPatchBuilder {
        DataComponentPatchBuilder::new()
    }

    /// Port of `DataComponentPatch.get(DataComponentType<T>)`.
    ///
    /// Returns:
    /// - `None` if this type is not mentioned in the patch
    /// - `Some(None)` if this type is explicitly removed
    /// - `Some(Some(val))` if this type is set to `val`
    #[inline]
    pub fn get(&self, component_type: DataComponentTypeId) -> Option<Option<&DataComponentValue>> {
        self.map.get(&component_type).map(|opt| opt.as_ref())
    }

    /// Returns `true` if this patch has no entries (`DataComponentPatch.isEmpty()`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns the number of entries in this patch (`DataComponentPatch.size()`).
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Iterates over patch entries in deterministic order (`DataComponentPatch.entrySet()`).
    pub fn iter(
        &self,
    ) -> impl ExactSizeIterator<Item = (DataComponentTypeId, Option<&DataComponentValue>)> {
        self.map.iter().map(|(&k, v)| (k, v.as_ref()))
    }

    /// Port of `DataComponentPatch.forget(Predicate<DataComponentType<?>>)`.
    ///
    /// Returns a copy with all matching component types removed from the patch.
    pub fn forget<F>(&self, mut predicate: F) -> Self
    where
        F: FnMut(DataComponentTypeId) -> bool,
    {
        if self.is_empty() {
            return Self::empty();
        }
        let mut copy = self.map.clone();
        copy.retain(|&k, _| !predicate(k));
        if copy.is_empty() {
            Self::empty()
        } else {
            Self { map: copy }
        }
    }

    /// Port of `DataComponentPatch.split()`.
    pub fn split(&self) -> SplitResult {
        if self.is_empty() {
            return SplitResult {
                added: DataComponentMap::empty(),
                removed: IndexSet::new(),
            };
        }
        let mut added = DataComponentMapBuilder::new();
        let mut removed = IndexSet::new();
        for (&k, v) in &self.map {
            match v {
                Some(val) => {
                    added.set(k, val.clone());
                }
                None => {
                    removed.insert(k);
                }
            }
        }
        SplitResult {
            added: added.build().expect("split added map is valid"),
            removed,
        }
    }

    /// Order-independent Java `Map.hashCode()` for this patch.
    pub fn java_hash_code(&self) -> i32 {
        patch_map_java_hash_code(&self.map)
    }
}

fn patch_map_java_hash_code(
    map: &IndexMap<DataComponentTypeId, Option<DataComponentValue>>,
) -> i32 {
    let mut sum: i32 = 0;
    for (&k, v) in map {
        let val_hash = match v {
            Some(val) => val.java_hash_code(),
            None => 0,
        };
        sum = sum.wrapping_add(k.java_hash_code() ^ val_hash);
    }
    sum
}

impl PartialEq for DataComponentPatch {
    fn eq(&self, other: &Self) -> bool {
        if self.map.len() != other.map.len() {
            return false;
        }
        self.map
            .iter()
            .all(|(k, v)| other.map.get(k).is_some_and(|ov| ov == v))
    }
}

impl Eq for DataComponentPatch {}

impl Hash for DataComponentPatch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.java_hash_code().hash(state);
    }
}

impl fmt::Display for DataComponentPatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (idx, (k, v)) in self.map.iter().enumerate() {
            if idx > 0 {
                f.write_str(", ")?;
            }
            match v {
                Some(val) => write!(f, "{k}=>{val}")?,
                None => write!(f, "!{k}")?,
            }
        }
        f.write_str("}")
    }
}

/// Port of `DataComponentPatch.Builder`.
#[derive(Clone, Debug, Default)]
pub struct DataComponentPatchBuilder {
    map: IndexMap<DataComponentTypeId, Option<DataComponentValue>>,
}

impl DataComponentPatchBuilder {
    pub fn new() -> Self {
        Self {
            map: IndexMap::new(),
        }
    }

    /// `Builder.set(type, value)`.
    pub fn set(
        &mut self,
        component_type: DataComponentTypeId,
        value: DataComponentValue,
    ) -> &mut Self {
        self.map.insert(component_type, Some(value));
        self
    }

    /// `Builder.remove(type)`.
    pub fn remove(&mut self, component_type: DataComponentTypeId) -> &mut Self {
        self.map.insert(component_type, None);
        self
    }

    /// `Builder.set(TypedDataComponent)`.
    pub fn set_typed(&mut self, component: TypedDataComponent) -> &mut Self {
        self.set(component.component_type, component.value)
    }

    /// `Builder.set(Iterable<TypedDataComponent<?>>)`.
    pub fn set_all<I>(&mut self, components: I) -> &mut Self
    where
        I: IntoIterator<Item = TypedDataComponent>,
    {
        for c in components {
            self.set_typed(c);
        }
        self
    }

    /// `Builder.copy(DataComponentPatch)`.
    pub fn copy(&mut self, orig: &DataComponentPatch) -> &mut Self {
        for (k, v) in orig.iter() {
            self.map.insert(k, v.cloned());
        }
        self
    }

    /// `Builder.clear(type)` — removes any patch entry (set or remove) for `type`.
    pub fn clear(&mut self, component_type: DataComponentTypeId) -> &mut Self {
        self.map.shift_remove(&component_type);
        self
    }

    /// `Builder.isSet(type)`.
    pub fn is_set(&self, component_type: DataComponentTypeId) -> bool {
        matches!(self.map.get(&component_type), Some(Some(_)))
    }

    /// `Builder.isRemoved(type)`.
    pub fn is_removed(&self, component_type: DataComponentTypeId) -> bool {
        matches!(self.map.get(&component_type), Some(None))
    }

    /// `Builder.build()`.
    pub fn build(self) -> DataComponentPatch {
        if self.map.is_empty() {
            DataComponentPatch::empty()
        } else {
            DataComponentPatch::from_index_map(self.map)
        }
    }
}

/// Port of `DataComponentPatch.PatchKey`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PatchKey {
    pub component_type: DataComponentTypeId,
    pub removed: bool,
}

impl PatchKey {
    pub fn parse(raw: &str) -> DataResult<Self> {
        let (removed, id_str) = match raw.strip_prefix(REMOVED_PREFIX) {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        let parsed_id = match crate::item::data_result_into(Identifier::read(id_str)) {
            Ok(id) => id,
            Err(err) => return DataResult::error(err),
        };
        let Some(component_type) = DataComponentTypeId::from_identifier(&parsed_id) else {
            return DataResult::error(format!("Unknown component type: {parsed_id}"));
        };
        if component_type.is_transient() {
            return DataResult::error(format!(
                "Component type {component_type} is transient and cannot be patched persistently"
            ));
        }
        DataResult::success(Self {
            component_type,
            removed,
        })
    }

    pub fn encode_string(self) -> DataResult<String> {
        if self.component_type.is_transient() {
            return DataResult::error(format!(
                "Component type {} is transient and cannot be encoded",
                self.component_type
            ));
        }
        let Some(name) = self.component_type.name() else {
            return DataResult::error(format!(
                "Unregistered component type: {}",
                self.component_type
            ));
        };
        if self.removed {
            DataResult::success(format!("{REMOVED_PREFIX}{name}"))
        } else {
            DataResult::success(name.to_string())
        }
    }
}

/// Port of `DataComponentPatch.CODEC`.
///
/// - Encodes set entries as `"minecraft:<id>": <value>` and removed entries as
///   `"!minecraft:<id>": {}`.
/// - Rejects transient component types and unknown component IDs.
pub fn data_component_patch_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<DataComponentPatch, Ops>> {
    crate::item::codec_from_fns(
        "DataComponentPatch",
        |patch: &DataComponentPatch, ops: &Ops| {
            let mut entries = Vec::with_capacity(patch.len());
            for (k, v) in patch.iter() {
                let key = PatchKey {
                    component_type: k,
                    removed: v.is_none(),
                };
                let key_str = match crate::item::data_result_into(key.encode_string()) {
                    Ok(s) => s,
                    Err(err) => return DataResult::error(err),
                };
                let encoded_val = match v {
                    Some(val) => {
                        match crate::item::data_result_into(encode_component_value(k, val, ops)) {
                            Ok(encoded) => encoded,
                            Err(err) => return DataResult::error(err),
                        }
                    }
                    None => match crate::item::data_result_into(
                        codec::empty::<Ops>().encode_start(ops, &Unit),
                    ) {
                        Ok(empty_map) => empty_map,
                        Err(err) => return DataResult::error(err),
                    },
                };
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string(key_str),
                    encoded_val,
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let pairs = match crate::item::data_result_into(ops.get_map_values(input)) {
                Ok(p) => p,
                Err(err) => return DataResult::error(err),
            };
            let mut builder = DataComponentPatchBuilder::new();
            for rivet_serialization::Pair {
                first: k_raw,
                second: v_raw,
            } in pairs
            {
                let key_str = match crate::item::data_result_into(ops.get_string_value(&k_raw)) {
                    Ok(s) => s,
                    Err(err) => return DataResult::error(err),
                };
                let patch_key = match crate::item::data_result_into(PatchKey::parse(&key_str)) {
                    Ok(pk) => pk,
                    Err(err) => return DataResult::error(err),
                };
                if patch_key.removed {
                    if let Err(err) =
                        crate::item::data_result_into(codec::empty::<Ops>().parse(ops, &v_raw))
                    {
                        return DataResult::error(err);
                    }
                    builder.remove(patch_key.component_type);
                } else {
                    let val = match crate::item::data_result_into(decode_component_value(
                        patch_key.component_type,
                        ops,
                        &v_raw,
                    )) {
                        Ok(v) => v,
                        Err(err) => return DataResult::error(err),
                    };
                    builder.set(patch_key.component_type, val);
                }
            }
            DataResult::success(builder.build())
        },
    )
}

/// Port of `net.minecraft.core.component.PatchedDataComponentMap`.
///
/// Combines an immutable prototype `DataComponentMap` with a mutable
/// copy-on-write patch (`Arc<IndexMap<...>>`).
#[derive(Clone, Debug)]
pub struct PatchedDataComponentMap {
    prototype: DataComponentMap,
    patch: Arc<IndexMap<DataComponentTypeId, Option<DataComponentValue>>>,
}

impl PatchedDataComponentMap {
    /// `new PatchedDataComponentMap(DataComponentMap prototype)`.
    pub fn new(prototype: DataComponentMap) -> Self {
        Self {
            prototype,
            patch: Arc::new(IndexMap::new()),
        }
    }

    /// `PatchedDataComponentMap.fromPatch(prototype, patch)`.
    ///
    /// Sanitizes the patch against `prototype`:
    /// - Setting a component to a value equal to the prototype default is dropped.
    /// - Removing a component not present in the prototype is dropped.
    pub fn from_patch(prototype: DataComponentMap, patch: &DataComponentPatch) -> Self {
        if patch.is_empty() || is_patch_sanitized(&prototype, patch) {
            Self {
                prototype,
                patch: Arc::new(patch.map.clone()),
            }
        } else {
            let mut sanitized = IndexMap::new();
            for (&k, v) in &patch.map {
                let default_val = prototype.get(k);
                match (v, default_val) {
                    (Some(val), Some(def)) if val == def => {}
                    (None, None) => {}
                    (val_opt, _) => {
                        sanitized.insert(k, val_opt.clone());
                    }
                }
            }
            Self {
                prototype,
                patch: Arc::new(sanitized),
            }
        }
    }

    /// Returns the prototype `DataComponentMap`.
    #[inline]
    pub fn prototype(&self) -> &DataComponentMap {
        &self.prototype
    }

    /// Port of `PatchedDataComponentMap.hasNonDefault(DataComponentType<?>)`.
    #[inline]
    pub fn has_non_default(&self, component_type: DataComponentTypeId) -> bool {
        self.patch.contains_key(&component_type)
    }

    /// Port of `PatchedDataComponentMap.set(type, value)`.
    ///
    /// If `value` equals the prototype default, removes the patch entry rather
    /// than storing a redundant override. Returns the previous effective value.
    pub fn set(
        &mut self,
        component_type: DataComponentTypeId,
        value: DataComponentValue,
    ) -> Option<DataComponentValue> {
        let default_val = self.prototype.get(component_type);
        let patch_map = Arc::make_mut(&mut self.patch);
        let prev = if default_val.is_some_and(|def| def == &value) {
            patch_map.shift_remove(&component_type)
        } else {
            patch_map.insert(component_type, Some(value))
        };
        match prev {
            Some(opt) => opt,
            None => default_val.cloned(),
        }
    }

    /// Port of `PatchedDataComponentMap.set(type, @Nullable value)`.
    pub fn set_opt(
        &mut self,
        component_type: DataComponentTypeId,
        value: Option<DataComponentValue>,
    ) -> Option<DataComponentValue> {
        match value {
            Some(v) => self.set(component_type, v),
            None => self.remove(component_type),
        }
    }

    /// Port of `PatchedDataComponentMap.remove(type)`.
    ///
    /// Records an explicit removal (`None`) only if the prototype contains this
    /// component; otherwise deletes any patch entry. Returns the previous
    /// effective value.
    pub fn remove(&mut self, component_type: DataComponentTypeId) -> Option<DataComponentValue> {
        let default_val = self.prototype.get(component_type);
        let patch_map = Arc::make_mut(&mut self.patch);
        let prev = if default_val.is_some() {
            patch_map.insert(component_type, None)
        } else {
            patch_map.shift_remove(&component_type)
        };
        match prev {
            Some(opt) => opt,
            None => default_val.cloned(),
        }
    }

    /// Port of `PatchedDataComponentMap.applyPatch(DataComponentPatch)`.
    ///
    /// Normalizes each entry against the prototype.
    pub fn apply_patch(&mut self, patch: &DataComponentPatch) {
        for (k, v) in patch.iter() {
            match v {
                Some(val) => {
                    self.set(k, val.clone());
                }
                None => {
                    self.remove(k);
                }
            }
        }
    }

    /// Port of `PatchedDataComponentMap.restorePatch(DataComponentPatch)`.
    ///
    /// Replaces the patch wholesale with `patch` (without normalizing against
    /// prototype, matching Paper's `restorePatch`).
    pub fn restore_patch(&mut self, patch: &DataComponentPatch) {
        let patch_map = Arc::make_mut(&mut self.patch);
        patch_map.clear();
        for (k, v) in patch.iter() {
            patch_map.insert(k, v.cloned());
        }
    }

    /// Port of `PatchedDataComponentMap.clearPatch()`.
    pub fn clear_patch(&mut self) {
        if !self.patch.is_empty() {
            Arc::make_mut(&mut self.patch).clear();
        }
    }

    /// Port of `PatchedDataComponentMap.setAll(DataComponentMap)`.
    pub fn set_all(&mut self, components: &DataComponentMap) {
        for (k, v) in components.iter() {
            self.set(k, v.clone());
        }
    }

    /// Port of `PatchedDataComponentMap.keySet()`.
    ///
    /// Starts with prototype keys and applies patch additions and removals in
    /// deterministic order.
    pub fn key_set(&self) -> IndexSet<DataComponentTypeId> {
        if self.patch.is_empty() {
            return self.prototype.key_set();
        }
        let mut keys = self.prototype.key_set();
        for (&k, v) in self.patch.iter() {
            if v.is_some() {
                keys.insert(k);
            } else {
                keys.shift_remove(&k);
            }
        }
        keys
    }

    /// Effective number of present components (`PatchedDataComponentMap.size()`).
    pub fn len(&self) -> usize {
        if self.patch.is_empty() {
            return self.prototype.len();
        }
        let mut count = self.prototype.len();
        for (&k, v) in self.patch.iter() {
            let in_patch = v.is_some();
            let in_proto = self.prototype.has(k);
            if in_patch != in_proto {
                count = if in_patch { count + 1 } else { count - 1 };
            }
        }
        count
    }

    /// Returns `true` if no effective components are present.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Port of `PatchedDataComponentMap.iterator()`.
    ///
    /// Emits patched (present) values first in patch order, followed by
    /// unchanged prototype entries in prototype order.
    pub fn typed_iter(&self) -> Vec<TypedDataComponent> {
        if self.patch.is_empty() {
            return self.prototype.typed_iter().collect();
        }
        let mut out = Vec::with_capacity(self.len());
        for (&k, v) in self.patch.iter() {
            if let Some(val) = v {
                out.push(TypedDataComponent::new(k, val.clone()));
            }
        }
        for (k, v) in self.prototype.iter() {
            if !self.patch.contains_key(&k) {
                out.push(TypedDataComponent::new(k, v.clone()));
            }
        }
        out
    }

    /// Port of `PatchedDataComponentMap.asPatch()`.
    pub fn as_patch(&self) -> DataComponentPatch {
        if self.patch.is_empty() {
            DataComponentPatch::empty()
        } else {
            DataComponentPatch::from_index_map((*self.patch).clone())
        }
    }

    /// Port of `PatchedDataComponentMap.copy()` — shares the patch Arc until
    /// either copy mutates (copy-on-write).
    pub fn copy(&self) -> Self {
        Self {
            prototype: self.prototype.clone(),
            patch: Arc::clone(&self.patch),
        }
    }

    /// Port of `PatchedDataComponentMap.toImmutableMap()`.
    pub fn to_immutable_map(&self) -> DataComponentMap {
        if self.patch.is_empty() {
            return self.prototype.clone();
        }
        let mut map = IndexMap::new();
        for (k, v) in self.prototype.iter() {
            if !self.patch.contains_key(&k) {
                map.insert(k, v.clone());
            }
        }
        for (&k, v) in self.patch.iter() {
            if let Some(val) = v {
                map.insert(k, val.clone());
            }
        }
        DataComponentMap::from_index_map(map)
    }

    /// Port of `PatchedDataComponentMap.hashCode()`:
    /// `this.prototype.hashCode() + this.patch.hashCode() * 31` (wrapping `i32`).
    pub fn java_hash_code(&self) -> i32 {
        let proto_hash = self.prototype.java_hash_code();
        let patch_hash = patch_map_java_hash_code(&self.patch);
        proto_hash.wrapping_add(patch_hash.wrapping_mul(31))
    }
}

fn is_patch_sanitized(prototype: &DataComponentMap, patch: &DataComponentPatch) -> bool {
    for (&k, v) in &patch.map {
        let default_val = prototype.get(k);
        match (v, default_val) {
            (Some(val), Some(def)) if val == def => return false,
            (None, None) => return false,
            _ => {}
        }
    }
    true
}

impl DataComponentGetter for PatchedDataComponentMap {
    fn get(&self, component_type: DataComponentTypeId) -> Option<&DataComponentValue> {
        if let Some(patched) = self.patch.get(&component_type) {
            return patched.as_ref();
        }
        self.prototype.get(component_type)
    }
}

impl PartialEq for PatchedDataComponentMap {
    fn eq(&self, other: &Self) -> bool {
        if self.prototype != other.prototype {
            return false;
        }
        if self.patch.len() != other.patch.len() {
            return false;
        }
        self.patch
            .iter()
            .all(|(k, v)| other.patch.get(k).is_some_and(|ov| ov == v))
    }
}

impl Eq for PatchedDataComponentMap {}

impl Hash for PatchedDataComponentMap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.java_hash_code().hash(state);
    }
}

impl fmt::Display for PatchedDataComponentMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (idx, item) in self.typed_iter().iter().enumerate() {
            if idx > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{item}")?;
        }
        f.write_str("}")
    }
}

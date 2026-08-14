//! Port of `net.minecraft.world.level.saveddata.SavedDataType` (MC 26.2).
//!
//! The registry-entry record that binds a saved-data blob's `Identifier` id,
//! fresh-construction supplier, `CODEC`, and `DataFixTypes` fixer. Equality is
//! by id only (two types with the same id are the same type), and the record
//! `toString` prints only the id.

use rivet_registry::Identifier;
use std::sync::Arc;

/// `DataFixTypes` (owned by the pending `mc.util.datafix` unit) — the
/// value-identity handle the saved-data storage uses to look up the right DFU
/// schema for a payload.
pub use crate::level::saveddata::stub_data_fix_types::DataFixTypes;

/// `net.minecraft.world.level.saveddata.SavedDataType<T>`.
///
/// Java is a record `SavedDataType<T extends SavedData>(Identifier id,
/// Supplier<T> constructor, Codec<T> codec, DataFixTypes dataFixType)` with
/// final (immutable) components. The components are private with read
/// accessors, so the `id` that `PartialEq`/`Eq`/`Hash` are derived over cannot
/// be mutated after the value is placed in a map/set. Java's `T extends
/// SavedData` bound is not enforceable while `SavedData` is a plain struct, so
/// any `T` is accepted. The `Supplier<T>`/`Codec<T>` are carried as `Arc`s so
/// the value can be shared cheaply. `type_()` builds a fresh equivalent value
/// per call rather than reproducing Java's `static final` `TYPE` singletons —
/// equality is by `id` only, so the fresh values are identical.
pub struct SavedDataType<T> {
    /// Java `Identifier id` — the filename/type identity (e.g.
    /// `minecraft:wandering_trader`).
    id: Identifier,
    /// Java `Supplier<T> constructor` — builds a fresh instance with default
    /// state (used when no saved payload exists).
    constructor: Arc<dyn Fn() -> T + Send + Sync>,
    /// Java `Codec<T> codec` — the payload codec.
    codec: Arc<dyn rivet_serialization::Codec<T, rivet_nbt::nbt_ops::NbtOps>>,
    /// Java `DataFixTypes dataFixType` — the datafixer type reference for this
    /// saved-data population.
    data_fix_type: DataFixTypes,
}

impl<T> SavedDataType<T> {
    /// `new(Identifier, Supplier<T>, Codec<T>, DataFixTypes)`.
    pub fn new(
        id: Identifier,
        constructor: Arc<dyn Fn() -> T + Send + Sync>,
        codec: Arc<dyn rivet_serialization::Codec<T, rivet_nbt::nbt_ops::NbtOps>>,
        data_fix_type: DataFixTypes,
    ) -> Self {
        Self {
            id,
            constructor,
            codec,
            data_fix_type,
        }
    }

    /// `id()` — Java record accessor.
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// `constructor()` — Java record accessor.
    pub fn constructor(&self) -> &Arc<dyn Fn() -> T + Send + Sync> {
        &self.constructor
    }

    /// `codec()` — Java record accessor.
    pub fn codec(&self) -> &Arc<dyn rivet_serialization::Codec<T, rivet_nbt::nbt_ops::NbtOps>> {
        &self.codec
    }

    /// `dataFixType()` — Java record accessor.
    pub fn data_fix_type(&self) -> DataFixTypes {
        self.data_fix_type
    }
}

impl<T> PartialEq for SavedDataType<T> {
    /// `SavedDataType.equals(Object)` — equal when the other is a
    /// `SavedDataType<?>` with the same `id`.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for SavedDataType<T> {}

impl<T> std::hash::Hash for SavedDataType<T> {
    /// `SavedDataType.hashCode()` — delegates to `id.hashCode()`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> std::fmt::Display for SavedDataType<T> {
    /// `SavedDataType.toString()` — `"SavedDataType[" + id + "]"`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SavedDataType[{}]", self.id)
    }
}

impl<T> std::fmt::Debug for SavedDataType<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::saveddata::weather_data::WeatherData;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of<T>(value: &SavedDataType<T>) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn equality_is_id_bound_not_component_identity() {
        // Java `SavedDataType.equals` compares only the id; two fresh `TYPE`
        // values share the id but hold distinct constructor/codec Arcs.
        let a = WeatherData::type_();
        let b = WeatherData::type_();
        assert_eq!(a, b, "same id must compare equal despite distinct codecs");
        assert_eq!(hash_of(&a), hash_of(&b), "hashCode delegates to id");

        // A same-shape type with a different id is not equal.
        let different_id = SavedDataType::new(
            Identifier::with_default_namespace("other_weather"),
            Arc::new(WeatherData::new),
            WeatherData::codec::<rivet_nbt::nbt_ops::NbtOps>(),
            DataFixTypes::SavedDataWeather,
        );
        assert_ne!(a, different_id, "different ids must not compare equal");
    }

    #[test]
    fn display_prints_only_the_id() {
        let t = WeatherData::type_();
        assert_eq!(t.to_string(), "SavedDataType[minecraft:weather]");
        assert_eq!(format!("{t:?}"), "SavedDataType[minecraft:weather]");
    }
}

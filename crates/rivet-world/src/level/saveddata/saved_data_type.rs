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
/// Supplier<T> constructor, Codec<T> codec, DataFixTypes dataFixType)`. The
/// `Supplier<T>`/`Codec<T>` are carried as owned closures so the type can be
/// built once (`WanderingTraderData::TYPE`, `WeatherData::TYPE`) and shared;
/// codecs are ops-generic in Rivet, so the `codec` slot is the NbtOps-pinned
/// codec the disk runtime will use.
pub struct SavedDataType<T> {
    /// Java `Identifier id` — the filename/type identity (e.g.
    /// `minecraft:wandering_trader`).
    pub id: Identifier,
    /// Java `Supplier<T> constructor` — builds a fresh instance with default
    /// state (used when no saved payload exists).
    pub constructor: Arc<dyn Fn() -> T + Send + Sync>,
    /// Java `Codec<T> codec` — the payload codec.
    pub codec: Arc<dyn rivet_serialization::Codec<T, rivet_nbt::nbt_ops::NbtOps>>,
    /// Java `DataFixTypes dataFixType` — the datafixer type reference for this
    /// saved-data population.
    pub data_fix_type: DataFixTypes,
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

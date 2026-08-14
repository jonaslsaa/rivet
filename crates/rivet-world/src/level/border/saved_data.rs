//! STUB(mc.world.level.saveddata) — minimal seams for the pending
//! `net.minecraft.world.level.saveddata` unit (and its `DataFixTypes`
//! dependency), created only so `WorldBorder`'s `extends SavedData` supertype
//! and `TYPE` handle compile. Replaced by the real ports when those units land.
//!
//! - `SavedData` — the abstract base with the private dirty flag
//!   (`setDirty()`/`setDirty(boolean)`/`isDirty()`).
//! - `SavedDataType<T>` — the `(Identifier id, Supplier<T> constructor,
//!   Codec<T> codec, DataFixTypes dataFixType)` record with identity equality
//!   on `id`.
//! - `DataFixTypes` — the data-fixer reference enum; only the variant this
//!   unit references is declared.

use rivet_registry::Identifier;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// STUB(mc.world.level.saveddata) — `SavedData`, the abstract base class with
/// the private `dirty` flag.
#[derive(Debug, Clone, Copy, Default)]
pub struct SavedData {
    dirty: bool,
}

impl SavedData {
    /// `new SavedData()` — the default constructor (`dirty = false`).
    pub fn new() -> Self {
        SavedData { dirty: false }
    }

    /// `SavedData.setDirty()`.
    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }

    /// `SavedData.setDirty(boolean)`.
    pub fn set_dirty_bool(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// `SavedData.isDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

/// STUB(mc.util.datafix) — `DataFixTypes`, the data-fixer schema reference
/// enum. Only `SAVED_DATA_WORLD_BORDER` (the variant this unit references) is
/// declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFixTypes {
    /// `DataFixTypes.SAVED_DATA_WORLD_BORDER`.
    SavedDataWorldBorder,
}

/// STUB(mc.world.level.saveddata) — `SavedDataType<T>`, the
/// `(Identifier id, Supplier<T> constructor, Codec<T> codec, DataFixTypes
/// dataFixType)` record with `equals`/`hashCode`/`toString` by `id`
/// (`PartialEq`/`Eq`/`Hash` mirror `equals`/`hashCode`; `Debug` mirrors
/// `toString`).
///
/// Java's `SavedDataType<T>` holds an ops-free `Codec<T>`; the port's codecs
/// are ops-parameterized, so the record is generic over `Ops`.
pub struct SavedDataType<T, Ops: DynamicOps + 'static> {
    /// `SavedDataType.id`.
    pub id: Identifier,
    /// `SavedDataType.constructor`.
    pub constructor: Arc<dyn Fn() -> T + Send + Sync>,
    /// `SavedDataType.codec`.
    pub codec: Arc<dyn Codec<T, Ops>>,
    /// `SavedDataType.dataFixType`.
    pub data_fix_type: DataFixTypes,
}

impl<T, Ops: DynamicOps + 'static> PartialEq for SavedDataType<T, Ops> {
    /// `SavedDataType.equals` — identity on `id`.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T, Ops: DynamicOps + 'static> Eq for SavedDataType<T, Ops> {}

impl<T, Ops: DynamicOps + 'static> std::hash::Hash for SavedDataType<T, Ops> {
    /// `SavedDataType.hashCode` — `this.id.hashCode()`.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T, Ops: DynamicOps + 'static> std::fmt::Debug for SavedDataType<T, Ops> {
    /// `SavedDataType.toString` — `"SavedDataType[" + this.id + "]"` where the
    /// id renders as `Identifier.toString` (`namespace:path`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SavedDataType[{}]", self.id)
    }
}

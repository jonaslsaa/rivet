//! Port of `net.minecraft.network.syncher.SynchedEntityData` (MC 26.2).
//!
//! The per-entity store of synced data items, indexed by accessor id. OWNERSHIP
//! (arenas + IDs) makes this a value owned by the `Entity` struct, so the
//! Java `entity` back-reference (`SyncedDataHolder`) is **removed**: `set` and
//! `assign_values` return/apply and the owning entity's wrapper fires
//! `on_synced_data_updated` itself (struct embedding means no vtable). Java
//! fires the callback only when `set` applies (`forceDirty || notEqual`); the
//! wrapper preserves that from `set`'s return value.
//!
//! Fidelity notes:
//! - `DataValue.read` maps an out-of-range serializer id to Java's
//!   `DecoderException("Unknown serializer type {n}")` (panic) and a blocked
//!   serializer to a loud blocked note.
//! - `DataItem.value()`/`DataValue::create` clone the value (`ForValueType`
//!   identity copy); the `ITEM_STACK` deep copy is deferred with that
//!   serializer.
//! - The `assignValue` serializer check is by *identity* (`SerializerId`
//!   equality — Java compares the serializer *objects* by reference, not id).

use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;

use super::entity_data_accessor::EntityDataAccessor;
use super::entity_data_serializer::SyncedValue;
use super::serialized_value::SerializedValue;
use super::serializer_id::SerializerId;

/// `SynchedEntityData.MAX_ID_VALUE` — defined once on
/// [`super::entity_data_accessor`], re-exported here.
pub use super::entity_data_accessor::MAX_ID_VALUE;

/// `SynchedEntityData` — the dense, id-indexed store of `DataItem`s.
#[derive(Debug, Clone)]
pub struct SynchedEntityData {
    items: Box<[DataItem]>,
    is_dirty: bool,
}

impl SynchedEntityData {
    /// `SynchedEntityData.getItem(accessor)`.
    pub fn get_item(&self, id: u8) -> &DataItem {
        &self.items[id as usize]
    }

    /// `SynchedEntityData.get(accessor)` — the typed value, downcast from the
    /// erased store (Java's unchecked `(T) dataItem.getValue()`).
    pub fn get<T: SyncedValue>(&self, accessor: EntityDataAccessor<T>) -> &T {
        let item = &self.items[accessor.id() as usize];
        debug_assert_eq!(
            item.serializer,
            T::SERIALIZER,
            "accessor {} uses serializer {:?} but slot holds {:?}",
            accessor.id(),
            T::SERIALIZER,
            item.serializer
        );
        T::downcast(&item.value).unwrap_or_else(|| {
            panic!(
                "value type does not match serializer for accessor {}",
                accessor.id()
            )
        })
    }

    /// `SynchedEntityData.set(accessor, value)` — `set(accessor, value, false)`.
    ///
    /// Returns whether the value applied (`forceDirty || notEqual`); the owning
    /// entity's wrapper fires `on_synced_data_updated` from that return value
    /// (the Java back-reference removed per OWNERSHIP).
    pub fn set<T: SyncedValue>(&mut self, accessor: EntityDataAccessor<T>, value: T) -> bool {
        self.set_with(accessor, value, false)
    }

    /// `SynchedEntityData.set(accessor, value, forceDirty)`.
    pub fn set_with<T: SyncedValue>(
        &mut self,
        accessor: EntityDataAccessor<T>,
        value: T,
        force_dirty: bool,
    ) -> bool {
        let item = &mut self.items[accessor.id() as usize];
        let value = value.into_value();
        if force_dirty || item.value != value {
            item.value = value;
            item.set_dirty(true);
            self.is_dirty = true;
            true
        } else {
            false
        }
    }

    /// `SynchedEntityData.isDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    /// `SynchedEntityData.packDirty()` — `None` when not dirty, else clears the
    /// dirty flag and returns the dirty items as `DataValue`s.
    pub fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
        if !self.is_dirty {
            return None;
        }
        self.is_dirty = false;
        let mut result = Vec::new();
        for item in self.items.iter_mut() {
            if item.is_dirty() {
                item.set_dirty(false);
                result.push(item.value());
            }
        }
        Some(result)
    }

    /// `SynchedEntityData.getNonDefaultValues()` — the items whose value
    /// differs from their initial value, as `DataValue`s.
    pub fn get_non_default_values(&self) -> Option<Vec<DataValue>> {
        let mut result = None;
        for item in self.items.iter() {
            if !item.is_set_to_default() {
                result.get_or_insert_with(Vec::new).push(item.value());
            }
        }
        result
    }

    /// `SynchedEntityData.packAll()` (Paper) — every slot as a `DataValue`.
    pub fn pack_all(&self) -> Vec<DataValue> {
        self.items.iter().map(DataItem::value).collect()
    }

    /// `SynchedEntityData.assignValues(List<DataValue>)` — applies wire-received
    /// values, validating each item's serializer *by identity* against the slot
    /// (Java: `Objects.equals(item.serializer(), dataItem.accessor.serializer())`).
    ///
    /// The Java `entity.onSyncedDataUpdated(...)` callbacks are the wrapper's
    /// job (back-reference removed); `assign_values` returns the ids that were
    /// assigned so the wrapper can fire per-id and the whole-list callback in
    /// Java's order.
    pub fn assign_values(&mut self, items: &[DataValue]) -> Vec<u8> {
        let mut assigned = Vec::with_capacity(items.len());
        for item in items {
            self.assign_value(item);
            assigned.push(item.id);
        }
        assigned
    }

    /// `SynchedEntityData.assignValue(DataItem, DataValue)` — the serializer
    /// identity check and the value write.
    fn assign_value(&mut self, item: &DataValue) {
        let data_item = &mut self.items[item.id as usize];
        if item.serializer != data_item.serializer {
            panic!(
                "Invalid entity data item type for field {} on entity: old={:?}, new={:?}",
                data_item.id, data_item.value, item.value
            );
        }
        data_item.set_value(item.value.clone());
    }
}

/// `SynchedEntityData.DataItem<T>` — one slot: accessor id + serializer, the
/// current and initial values, and the dirty flag.
#[derive(Debug, Clone)]
pub struct DataItem {
    id: u8,
    serializer: SerializerId,
    value: SerializedValue,
    initial_value: SerializedValue,
    dirty: bool,
}

impl DataItem {
    /// The `DataItem` constructor — `initialValue = value` (Java's `new
    /// DataItem<>(accessor, initialValue)` sets `value = initialValue`).
    pub fn new(id: u8, serializer: SerializerId, value: SerializedValue) -> Self {
        DataItem {
            id,
            serializer,
            initial_value: value.clone(),
            value,
            dirty: false,
        }
    }

    /// `DataItem.getAccessor().id()`.
    pub fn id(&self) -> u8 {
        self.id
    }

    /// `DataItem.getAccessor().serializer()` (id identity).
    pub fn serializer(&self) -> SerializerId {
        self.serializer
    }

    /// `DataItem.getValue()` — a clone of the erased value.
    pub fn get_value(&self) -> SerializedValue {
        self.value.clone()
    }

    /// `DataItem.setValue(T)`.
    pub fn set_value(&mut self, value: SerializedValue) {
        self.value = value;
    }

    /// `DataItem.isDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// `DataItem.setDirty(boolean)`.
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// `DataItem.isSetToDefault()` — `initialValue.equals(value)`.
    pub fn is_set_to_default(&self) -> bool {
        self.initial_value == self.value
    }

    /// `DataItem.value()` — `DataValue.create(accessor, value)`, the identity
    /// copy (`ForValueType`).
    pub fn value(&self) -> DataValue {
        DataValue {
            id: self.id,
            serializer: self.serializer,
            value: self.value.clone(),
        }
    }
}

/// `SynchedEntityData.DataValue<T>` — one wire item `(id, serializer, value)`.
///
/// The `id` is the accessor id; `serializer` is the value's serializer identity.
/// The two are independent id spaces (the accessor id selects the slot, the
/// serializer id selects the value codec) — `read` takes the accessor id as a
/// parameter, exactly like Java.
#[derive(Debug, Clone, PartialEq)]
pub struct DataValue {
    /// The accessor id (`0..=MAX_ID_VALUE`).
    pub id: u8,
    /// The value's serializer identity (wire VarInt).
    pub serializer: SerializerId,
    /// The erased value.
    pub value: SerializedValue,
}

impl DataValue {
    /// `DataValue.create(accessor, value)` — `serializer.copy(value)`. Identity
    /// copy for every `ForValueType` serializer; the `ITEM_STACK` deep copy is
    /// deferred with that serializer.
    pub fn create<T: SyncedValue>(accessor: EntityDataAccessor<T>, value: T) -> Self {
        DataValue {
            id: accessor.id(),
            serializer: T::SERIALIZER,
            value: value.into_value(),
        }
    }

    /// `DataValue.write` — `writeByte(id) + writeVarInt(serializerId)` then the
    /// value codec.
    pub fn write(&self, output: &mut RegistryFriendlyByteBuf) {
        let serializer_id = self.serializer.serialized_id();
        // Java: `getSerializedId(serializer) < 0` -> EncoderException. The enum
        // is the closed registered set, so the id is always >= 0; the check is
        // structural (see serializer_id.rs).
        if serializer_id < 0 {
            panic!("Unknown serializer type {:?}", self.serializer);
        }
        output.inner_mut().write_byte(self.id as i8);
        output.inner_mut().write_var_int(serializer_id);
        self.value.write(output);
    }

    /// `DataValue.read(input, id)` — the accessor id is supplied by the caller
    /// (the packet unpack loop read it first); the serializer id is the next
    /// VarInt. An unregistered serializer id is Java's
    /// `DecoderException("Unknown serializer type {n}")` (panic); a blocked one
    /// panics with the blocked note.
    pub fn read(input: &mut RegistryFriendlyByteBuf, id: u8) -> Self {
        let type_id = input.inner_mut().read_var_int();
        let serializer = SerializerId::try_from(type_id)
            .unwrap_or_else(|| panic!("Unknown serializer type {type_id}"));
        let value = SerializedValue::read(input, serializer);
        DataValue {
            id,
            serializer,
            value,
        }
    }
}

/// `SynchedEntityData.Builder` — pre-sized to the leaf's accessor count,
/// enforcing Java's bounds/duplicate checks at `define` and the every-slot
/// check at `build`.
#[derive(Debug)]
pub struct Builder {
    items: Vec<Option<DataItem>>,
}

impl Builder {
    /// `new Builder(entity)` — the Java ctor sizes the array from
    /// `ClassTreeIdRegistry.getCount(entity.getClass())`; OWNERSHIP makes that
    /// the leaf's compile-time `ACCESSOR_COUNT`, passed in.
    pub fn new(count: usize) -> Self {
        Builder {
            items: vec![None; count],
        }
    }

    /// `Builder.define(accessor, value)`.
    ///
    /// Java's `id > itemsById.length` check panics `"Data value id is too big
    /// with {id}! (Max is {length})"`; an id exactly `== length` passes that
    /// check and then throws `ArrayIndexOutOfBounds` from the array write — the
    /// Rust port reproduces the index-out-of-bounds as the slice index panic.
    /// The `"Unregistered serializer"` check is structurally always-satisfied
    /// (the serializer enum is the closed registered set; the custom-serializer
    /// runtime table is deferred), so it is not emitted.
    pub fn define<T: SyncedValue>(
        &mut self,
        accessor: EntityDataAccessor<T>,
        value: T,
    ) -> &mut Self {
        let id = accessor.id() as usize;
        if id > self.items.len() {
            panic!(
                "Data value id is too big with {}! (Max is {})",
                accessor.id(),
                self.items.len()
            );
        }
        if self.items[id].is_some() {
            panic!("Duplicate id value for {}!", accessor.id());
        }
        self.items[id] = Some(DataItem::new(
            accessor.id(),
            T::SERIALIZER,
            value.into_value(),
        ));
        self
    }

    /// `Builder.build()` — every slot `0..count` must be defined (Java's
    /// `IllegalStateException("Entity {clazz} has not defined synched data
    /// value {i}")`, minus the class name).
    pub fn build(self) -> SynchedEntityData {
        for (i, item) in self.items.iter().enumerate() {
            if item.is_none() {
                panic!("Entity has not defined synched data value {i}");
            }
        }
        SynchedEntityData {
            items: self
                .items
                .into_iter()
                .map(|o| o.expect("checked above"))
                .collect(),
            is_dirty: false,
        }
    }
}

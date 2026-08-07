//! Port of `net.minecraft.network.syncher.EntityDataSerializer` (MC 26.2).
//!
//! Java's serializer is an interface with a `codec()` and a `copy(value)`. Its
//! identity is the `CrudeIncrementalIntIdentityHashBiMap` key: the 43 wire ids
//! are fixed at static-init. In the Rust port that identity collapses to the
//! [`SerializerId`] enum, and `EntityDataSerializer<T>` is a typed ZST tag over
//! the value type `T` (the `codec()` is [`SerializedValue`]'s dispatch).
//!
//! Java's two copy kinds are preserved by the value model rather than the
//! serializer: `ForValueType` serializers copy by identity, which maps to
//! `SerializedValue: Clone`; the one deep-copying serializer (`ITEM_STACK`,
//! `ItemStack.copy`) is blocked with the rest of the entity layer, so a future
//! `Clone` for its variant must be the deep copy (documented on
//! [`crate::syncher::serialized_value`]).
//!
//! `registerSerializer` (Paper plugins) is the one place a runtime table is
//! unavoidable; per "no speculative abstractions" it is deferred — the enum is
//! closed today and the dispatch `match` is structured so an `id >= 43`
//! fallback is cheap later.

use std::marker::PhantomData;

use super::entity_data_accessor::EntityDataAccessor;
use super::serialized_value::SerializedValue;
use super::serializer_id::SerializerId;

/// The value-type ↔ serializer binding: every concrete value type that can be
/// stored as synced data knows its [`SerializerId`] and how to enter/leave the
/// erased [`SerializedValue`] union.
///
/// This replaces 43 manual `EntityDataSerializer<T>` const constructors: the
/// serializer↔value-type map is bijective (43 distinct value types), so each
/// value type owns its id, and `get::<T>`/`set` stay typed.
pub trait SyncedValue: Clone + PartialEq + Send + Sync + 'static {
    /// The value type's serializer id (its identity).
    const SERIALIZER: SerializerId;

    /// Move the value into the erased wire union (`DataValue::create`'s copy,
    /// identity for every `ForValueType` serializer).
    fn into_value(self) -> SerializedValue;

    /// Extract `&Self` from an erased value — the Rust analogue of Java's
    /// unchecked `(T) item.value()` downcast, total because the variant and
    /// the id are the same information.
    fn downcast(value: &SerializedValue) -> Option<&Self>;
}

/// `EntityDataSerializer<T>` — the typed serializer tag. A ZST: Java's
/// serializers are stateless singletons (`forValueType` only stores the codec),
/// and object identity maps to the enum-variant identity.
///
/// `Clone`/`Copy` are implemented by hand (not derived): the derived forms would
/// bound them on `T: Copy`, but the tag is a ZST and must stay copyable for any
/// value type `T`.
#[derive(Debug)]
pub struct EntityDataSerializer<T: SyncedValue> {
    _marker: PhantomData<fn() -> T>,
}

impl<T: SyncedValue> Clone for EntityDataSerializer<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: SyncedValue> Copy for EntityDataSerializer<T> {}

impl<T: SyncedValue> EntityDataSerializer<T> {
    /// The singleton for `T` — Java's `EntityDataSerializer.forValueType(codec)`
    /// instance for the concrete value type.
    pub const fn new() -> Self {
        EntityDataSerializer {
            _marker: PhantomData,
        }
    }

    /// `EntityDataSerializer.createAccessor(int)`.
    pub fn create_accessor(self, id: u8) -> EntityDataAccessor<T> {
        EntityDataAccessor::new(id, self)
    }
}

impl<T: SyncedValue> Default for EntityDataSerializer<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SyncedValue> EntityDataSerializer<T> {
    /// `EntityDataSerializers.getSerializedId(serializer)` — the wire id.
    pub fn serialized_id(self) -> i32 {
        T::SERIALIZER.serialized_id()
    }
}

impl<T: SyncedValue> PartialEq for EntityDataSerializer<T> {
    /// Java serializer equality is object identity; the Rust singleton for a
    /// given `T` is unique, so two tags of the same value type are always equal.
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl<T: SyncedValue> Eq for EntityDataSerializer<T> {}

/// The `EntityDataSerializers` static fields. Only the two serializers proven
/// by the #153 join fixture (`set_entity_data` carries `FLOAT` and `BYTE`) have
/// concrete value types today; every other serializer's identity is the
/// [`SerializerId`] enum (declared, id-pinned) with its [`SerializedValue`]
/// variant blocked for the M3 entity wave.
pub mod serializers {
    use super::*;

    /// `EntityDataSerializers.BYTE`.
    pub const BYTE: EntityDataSerializer<i8> = EntityDataSerializer::new();
    /// `EntityDataSerializers.FLOAT`.
    pub const FLOAT: EntityDataSerializer<f32> = EntityDataSerializer::new();
}

impl SyncedValue for i8 {
    const SERIALIZER: SerializerId = SerializerId::Byte;

    fn into_value(self) -> SerializedValue {
        SerializedValue::Byte(self)
    }

    fn downcast(value: &SerializedValue) -> Option<&i8> {
        match value {
            SerializedValue::Byte(v) => Some(v),
            _ => None,
        }
    }
}

impl SyncedValue for f32 {
    const SERIALIZER: SerializerId = SerializerId::Float;

    fn into_value(self) -> SerializedValue {
        SerializedValue::Float(self)
    }

    fn downcast(value: &SerializedValue) -> Option<&f32> {
        match value {
            SerializedValue::Float(v) => Some(v),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializer_id_identity() {
        assert_eq!(EntityDataSerializer::<i8>::new().serialized_id(), 0);
        assert_eq!(EntityDataSerializer::<f32>::new().serialized_id(), 3);
        // Same value type -> same singleton (equality is trivially true).
        assert_eq!(
            EntityDataSerializer::<f32>::new(),
            EntityDataSerializer::<f32>::new()
        );
    }

    #[test]
    fn create_accessor_propagates_id() {
        let accessor = EntityDataSerializer::<f32>::new().create_accessor(9);
        assert_eq!(accessor.id(), 9);
        assert_eq!(accessor.serializer().serialized_id(), 3);
    }
}

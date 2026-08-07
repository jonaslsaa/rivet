//! Port of `net.minecraft.network.syncher.EntityDataAccessor` (MC 26.2).
//!
//! The accessor is a value type — `(int id, EntityDataSerializer<T> serializer)`
//! — with `equals`/`hashCode` **by id only** (the serializer is ignored, so
//! cross-class accessors legitimately share ids). Java ids are dense per
//! concrete class, assigned by the global `ClassTreeIdRegistry` (base `Entity`
//! gets `0..N`, a subclass continues `N..`), capped at `MAX_ID_VALUE = 254`;
//! OWNERSHIP collapses that runtime registry to compile-time leaf consts, so a
//! Rust accessor is built with an explicit id.

use std::fmt;
use std::hash::{Hash, Hasher};

use super::entity_data_serializer::{EntityDataSerializer, SyncedValue};

/// `SynchedEntityData.MAX_ID_VALUE` — the accessor-id ceiling (`u8` id space,
/// id 255 is the packet EOF sentinel). Defined once and re-exported from
/// [`crate::syncher::synched_entity_data`].
pub const MAX_ID_VALUE: u8 = 254;

/// `EntityDataAccessor<T>` — `(id, serializer)`, equality by id only.
///
/// `Clone`/`Copy` are by hand (not derived), like the serializer tag: the
/// accessor is a value type and must stay copyable for any value type `T`.
#[derive(Debug)]
pub struct EntityDataAccessor<T: SyncedValue> {
    id: u8,
    serializer: EntityDataSerializer<T>,
}

impl<T: SyncedValue> Clone for EntityDataAccessor<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: SyncedValue> Copy for EntityDataAccessor<T> {}

impl<T: SyncedValue> EntityDataAccessor<T> {
    /// `new EntityDataAccessor<>(int, EntityDataSerializer)` — Java's record
    /// constructor; leaf code builds accessors via
    /// `EntityDataSerializer::create_accessor` with a compile-time id.
    pub fn new(id: u8, serializer: EntityDataSerializer<T>) -> Self {
        EntityDataAccessor { id, serializer }
    }

    /// `EntityDataAccessor.id()`.
    pub fn id(&self) -> u8 {
        self.id
    }

    /// `EntityDataAccessor.serializer()`.
    pub fn serializer(&self) -> EntityDataSerializer<T> {
        self.serializer
    }
}

impl<T: SyncedValue> PartialEq for EntityDataAccessor<T> {
    /// `EntityDataAccessor.equals` — compares `id` only, exactly like Java.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<T: SyncedValue> Eq for EntityDataAccessor<T> {}

impl<T: SyncedValue> Hash for EntityDataAccessor<T> {
    /// `EntityDataAccessor.hashCode` — the id.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T: SyncedValue> fmt::Display for EntityDataAccessor<T> {
    /// `EntityDataAccessor.toString` — `"<entity data: " + id + ">"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<entity data: {}>", self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::entity_data_serializer::serializers;
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    fn hash_of<T: Hash>(value: T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn equality_and_hash_are_by_id_only() {
        let a = serializers::FLOAT.create_accessor(9);
        let b = serializers::FLOAT.create_accessor(9);
        let c = serializers::FLOAT.create_accessor(10);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Equal accessors hash equal; different ids hash different.
        assert_eq!(hash_of(a), hash_of(b));
        assert_ne!(hash_of(a), hash_of(c));
    }

    #[test]
    fn accessor_does_not_compare_serializer() {
        // Java ignores the serializer in `equals` — the id is the whole
        // comparison key, and the id spaces are per-class (a byte at id 9 and a
        // float at id 9 on different leaves are the same key). Rust's
        // `PartialEq` impl is monomorphic in `T`, so accessors of different
        // value types cannot be compared; the cross-serializer claim is pinned
        // through the id, which is exactly the field Java compares.
        let byte = serializers::BYTE.create_accessor(9);
        let float = serializers::FLOAT.create_accessor(9);
        assert_eq!(byte.id(), float.id());
        // Within one value type, equality is by id only.
        assert_eq!(
            serializers::BYTE.create_accessor(9),
            serializers::BYTE.create_accessor(9)
        );
        assert_ne!(byte, serializers::BYTE.create_accessor(8));
    }

    #[test]
    fn display_is_java_format() {
        assert_eq!(
            format!("{}", serializers::FLOAT.create_accessor(9)),
            "<entity data: 9>"
        );
    }
}

//! Port of `net.minecraft.network.syncher` (MC 26.2) — the synced entity data
//! (issue #90, MANIFEST line 269).
//!
//! Two halves:
//! - the **packet-side wire value model** (`SerializedValue`, `SerializerId`,
//!   `DataValue`) that `ClientboundSetEntityDataPacket` carries and the #153
//!   fixture proves;
//! - the **entity-side store** (`SynchedEntityData`/`DataItem`/`Builder`,
//!   `EntityDataAccessor`, `EntityDataSerializer`, `SyncedDataHolder`), the
//!   mutable per-entity data model ported ahead of its consumer.
//!
//! RivetTodo(#222): the entity-side store is not yet referenced outside this
//! module — its interface embeds entity assumptions (`assign_values` returning
//! raw ids instead of accessors) that will be re-derived against the owning
//! `Entity` in the M3 entity wave.
//!
//! Java classes, one module each:
//! - [`entity_data_serializer`] — `EntityDataSerializer` (identity: the
//!   [`SerializerId`] enum).
//! - [`serializer_id`] — `EntityDataSerializers` static-block registration ids
//!   (the `CrudeIncrementalIntIdentityHashBiMap` collapsed to a compile-time
//!   enum per OWNERSHIP).
//! - [`entity_data_accessor`] — `EntityDataAccessor` (a value type, equality by
//!   id only).
//! - [`serialized_value`] — the erased wire value union (43 variants, ids
//!   pinned; `BYTE`/`FLOAT` codecs live, the rest blocked for the M3 entity
//!   wave).
//! - [`synched_entity_data`] — `SynchedEntityData` + `DataItem` + `DataValue` +
//!   `Builder`.
//! - [`synced_data_holder`] — `SyncedDataHolder` (the entity-side callback
//!   surface).
//!
//! The `ClassTreeIdRegistry` (per-class dense accessor ids) collapses to
//! compile-time leaf consts (OWNERSHIP); no runtime registry is ported.
//! `EntityDataSerializers.registerSerializer` (Paper plugins via `rivet-ffi`)
//! is the one place a runtime table is unavoidable and is deferred — the enum
//! is closed today.

pub mod entity_data_accessor;
pub mod entity_data_serializer;
pub mod serialized_value;
pub mod serializer_id;
pub mod synced_data_holder;
pub mod synched_entity_data;

pub use entity_data_accessor::{EntityDataAccessor, MAX_ID_VALUE};
pub use entity_data_serializer::{EntityDataSerializer, SyncedValue, serializers};
pub use serialized_value::SerializedValue;
pub use serializer_id::SerializerId;
pub use synced_data_holder::SyncedDataHolder;
pub use synched_entity_data::{
    Builder as SynchedEntityDataBuilder, DataItem, DataValue, SynchedEntityData,
};

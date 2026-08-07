//! Port of `net.minecraft.network.syncher.SyncedDataHolder` (MC 26.2).
//!
//! Java: the interface an `Entity` implements to receive
//! `onSyncedDataUpdated(EntityDataAccessor<?>)` callbacks when its
//! `SynchedEntityData` changes. OWNERSHIP removes the `SynchedEntityData`
//! back-reference: `set`/`assign_values` return what applied and the owning
//! entity's wrapper fires the callback itself, so this trait is the shape of
//! that wrapper, not a field the store holds.

use super::entity_data_accessor::EntityDataAccessor;
use super::entity_data_serializer::SyncedValue;

/// `SyncedDataHolder` — the entity-side callback surface for synced-data
/// updates. Object-safe over the erased id, matching Java's erased
/// `onSyncedDataUpdated(EntityDataAccessor<?>)`.
pub trait SyncedDataHolder {
    /// `SyncedDataHolder.onSyncedDataUpdated(EntityDataAccessor<?>)` — fired
    /// after a value is applied. The concrete entity (struct embedding) calls
    /// its own override, so no vtable dispatch is needed.
    fn on_synced_data_updated<T: SyncedValue>(&mut self, accessor: EntityDataAccessor<T>);
}

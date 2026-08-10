//! Port of `net.minecraft.world.level.storage.ValueInputContextHelper` (issue
//! #382).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/ValueInputContextHelper.java`. Holds the `HolderLookup.Provider` and
//! the `DynamicOps<Tag>` (the `createSerializationContext(NbtOps)` result — a
//! `RegistryOps<Tag, NbtOps>` in this crate), plus the shared empty `ValueInput`
//! and empty list singletons.

use crate::level::storage::value_input::{
    EmptyValueInput, TypedInputList, ValueInput, ValueInputList,
};
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::access::RegistryAccess;
use rivet_registry::registry_ops::RegistryOps;
use std::sync::Arc;

/// The serialization-context ops over `NbtOps` — Java's
/// `lookup.createSerializationContext(NbtOps.INSTANCE)`.
pub type TagContextOps = RegistryOps<Tag, NbtOps>;

/// `net.minecraft.world.level.storage.ValueInputContextHelper`.
pub struct ValueInputContextHelper {
    lookup: RegistryAccess,
    ops: Arc<TagContextOps>,
}

impl ValueInputContextHelper {
    /// `ValueInputContextHelper(HolderLookup.Provider, DynamicOps<Tag>)`.
    ///
    /// Java's constructor ignores the passed ops and rebuilds it through
    /// `lookup.createSerializationContext(ops)`.
    pub fn new(lookup: RegistryAccess, ops: NbtOps) -> Self {
        let ops = Arc::new(RegistryOps::create_from_access(&ops, lookup.clone()));
        ValueInputContextHelper { lookup, ops }
    }

    /// `ValueInputContextHelper.ops()`.
    pub fn ops(&self) -> &Arc<TagContextOps> {
        &self.ops
    }

    /// `ValueInputContextHelper.lookup()`.
    pub fn lookup(&self) -> &RegistryAccess {
        &self.lookup
    }

    /// `ValueInputContextHelper.empty()`.
    pub fn empty(&self) -> ValueInput {
        ValueInput::Empty(EmptyValueInput::new(self.lookup.clone()))
    }

    /// `ValueInputContextHelper.emptyList()`.
    pub fn empty_list(&self) -> ValueInputList {
        ValueInputList::Empty
    }

    /// `ValueInputContextHelper.emptyTypedList()`.
    pub fn empty_typed_list<A>(&self) -> TypedInputList<A> {
        TypedInputList::Empty
    }
}

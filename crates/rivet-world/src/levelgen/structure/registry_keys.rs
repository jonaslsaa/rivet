//! Single typed registry-key definitions for native structure authority.
use super::structure_registry::{Structure, StructureSet};
use rivet_registry::{Identifier, ResourceKey, registry::RegistryKey};
use std::sync::LazyLock;

pub static STRUCTURE: LazyLock<RegistryKey<Structure>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/structure"))
});
pub static STRUCTURE_SET: LazyLock<RegistryKey<StructureSet>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/structure_set"))
});

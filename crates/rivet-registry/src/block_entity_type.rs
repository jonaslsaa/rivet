//! Registry identity surface for
//! `net.minecraft.world.level.block.entity.BlockEntityType` (MC 26.2, #341).
//!
//! Paper's real value also owns a factory and its valid-block set. Those fields
//! require the excluded block-entity subclasses and are not needed to identify
//! a stored chunk payload. This port therefore carries only the generated
//! built-in id. It deliberately exposes no generic constructor or fake entity
//! factory: resolving a known type does not claim its payload is supported.

use std::sync::{Arc, LazyLock};

use crate::generated::block_entity_types::{BLOCK_ENTITY_TYPE_BY_ID, block_entity_type_id};
use crate::registries::BLOCK_ENTITY_TYPE;
use crate::{Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey};

/// The one built-in block-entity registry instance used by packet codec
/// contexts and by every public value lookup. `Registry` encodes values by
/// allocation identity, so rebuilding this table per caller would detach the
/// returned `Arc`s from the registry used to encode them.
static BUILT_IN_REGISTRY_ACCESS: LazyLock<RegistryAccess> = LazyLock::new(|| {
    let mut builder = RegistryBuilder::new(&BLOCK_ENTITY_TYPE);
    for (id, name) in BLOCK_ENTITY_TYPE_BY_ID.iter().enumerate() {
        let key = ResourceKey::create(&BLOCK_ENTITY_TYPE, Identifier::parse(name));
        builder.register(
            &key,
            Arc::new(BlockEntityType { id: id as u16 }),
            RegistrationInfo::BUILT_IN,
        );
    }
    RegistryAccess::from_single_registry((*BLOCK_ENTITY_TYPE).clone(), builder.freeze())
});

/// A known Minecraft 26.2 built-in block-entity type.
///
/// Values can only be obtained from the report-generated id/name space. An
/// unknown numeric id or resource identifier remains `None`, matching the
/// non-defaulted vanilla registry rather than folding to a fabricated type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlockEntityType {
    id: u16,
}

impl BlockEntityType {
    /// Resolve a built-in registry id. Unknown ids are not representable.
    pub fn from_id(id: u16) -> Option<Arc<Self>> {
        BUILT_IN_REGISTRY_ACCESS
            .lookup(&BLOCK_ENTITY_TYPE)
            .expect("built-in block-entity registry is present")
            .by_id_arc(id.into())
            .cloned()
    }

    /// Resolve the NBT/registry resource identifier used by Paper's
    /// `BuiltInRegistries.BLOCK_ENTITY_TYPE.byNameCodec()`.
    pub fn from_identifier(identifier: &Identifier) -> Option<Arc<Self>> {
        block_entity_type_id(&identifier.to_string()).and_then(Self::from_id)
    }

    /// Resolve a serialized namespaced identifier.
    pub fn from_name(name: &str) -> Option<Arc<Self>> {
        block_entity_type_id(name).and_then(Self::from_id)
    }

    /// Numeric built-in registry/network id.
    pub const fn id(&self) -> u16 {
        self.id
    }

    /// Canonical namespaced identifier text.
    pub fn name(&self) -> &'static str {
        BLOCK_ENTITY_TYPE_BY_ID[self.id as usize]
    }

    /// Canonical resource identifier.
    pub fn identifier(&self) -> Identifier {
        Identifier::parse(self.name())
    }

    /// Access Paper's non-defaulted built-in registry in registration order.
    ///
    /// The returned access shares the canonical registry and element `Arc`
    /// identities. It is the codec context for production chunk packets.
    pub fn built_in_registry_access() -> RegistryAccess {
        BUILT_IN_REGISTRY_ACCESS.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representative_paper_26_2_ids_are_generated() {
        assert_eq!(
            BlockEntityType::from_name("minecraft:furnace")
                .unwrap()
                .id(),
            0
        );
        assert_eq!(
            BlockEntityType::from_name("minecraft:chest").unwrap().id(),
            1
        );
        assert_eq!(
            BlockEntityType::from_name("minecraft:mob_spawner")
                .unwrap()
                .id(),
            9
        );
        assert_eq!(
            BlockEntityType::from_name("minecraft:crafter")
                .unwrap()
                .id(),
            42
        );
        assert_eq!(
            BlockEntityType::from_name("minecraft:potent_sulfur")
                .unwrap()
                .id(),
            48
        );
    }

    #[test]
    fn unknown_ids_and_identifiers_remain_unknown() {
        assert_eq!(BlockEntityType::from_id(49), None);
        assert_eq!(BlockEntityType::from_id(u16::MAX), None);
        assert_eq!(
            BlockEntityType::from_name("minecraft:not_a_block_entity"),
            None
        );
        assert_eq!(
            BlockEntityType::from_identifier(&Identifier::parse("example:chest")),
            None
        );
    }

    #[test]
    fn every_generated_identity_round_trips_through_the_real_registry() {
        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        assert_eq!(registry.size() as usize, BLOCK_ENTITY_TYPE_BY_ID.len());

        for (id, name) in BLOCK_ENTITY_TYPE_BY_ID.iter().enumerate() {
            let identifier = Identifier::parse(name);
            let value = registry
                .get_optional(&identifier)
                .expect("generated identifier is registered");
            assert_eq!(value.id(), id as u16);
            assert_eq!(value.name(), *name);
            assert_eq!(value.identifier(), identifier);
            assert_eq!(registry.get_id(value), id as i32);
            assert_eq!(registry.by_id(id as i32), Some(value));
            assert_eq!(registry.get_key(value), Some(identifier));
        }
    }

    #[test]
    fn registry_lookup_and_encode_identity_preserve_missing_behavior() {
        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        assert!(
            registry
                .get_optional(&Identifier::parse("minecraft:unknown"))
                .is_none()
        );
        assert!(registry.by_id(49).is_none());

        // Equal content in a fresh allocation is not the registered Java-style
        // object identity, so the existing packet encoder must reject it.
        let registered = BlockEntityType::from_name("minecraft:chest").unwrap();
        assert_eq!(registry.get_id(&registered), 1);
        let detached = Arc::new((*registered).clone());
        assert_eq!(registry.get_id(&detached), -1);
    }
}

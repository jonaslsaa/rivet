//! Port of `net.minecraft.world.level.levelgen.feature.rootplacers.
//! RootPlacerType` (class, 26.2).
//!
//! Java is the per-placer wrapper holding each `MapCodec<P>`; its single
//! constant is a `register(...)` call into `BuiltInRegistries.ROOT_PLACER_TYPE`,
//! carrying the placer's `MapCodec`. The Rust port mirrors
//! `BlockStateProviderType`'s identity split: the placer's type identity is the
//! opaque [`RootPlacerTypeId`] handle (the registry element identity — element
//! id == insertion index), and the per-type `MapCodec` is resolved by the
//! dispatch table in `root_placer`, not stored on the id.
//!
//! The single Paper constant is declared with its exact registry identity
//! (id 0), reproducing `BuiltInRegistries.ROOT_PLACER_TYPE`'s insertion order.

/// The `RootPlacerType<P>` registry element identity — the per-type `u32` id
/// (element id == holder id == insertion index) plus its registry-key location,
/// mirroring `BlockStateProviderTypeId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RootPlacerTypeId {
    /// The per-type `u32` identity (insertion index in the root-placer-type
    /// registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "mangrove_root_placer", …)` → `minecraft:mangrove_root_placer`).
    pub location: &'static str,
}

impl RootPlacerTypeId {
    /// `new RootPlacerTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> RootPlacerTypeId {
        RootPlacerTypeId { id, location }
    }

    /// `RegistryKey.getValue()` — the location as a string, the value the
    /// by-name codec encodes/decodes.
    pub fn location(&self) -> &'static str {
        self.location
    }
}

/// The single `RootPlacerTypes` constant — Paper's exact declaration order in
/// `RootPlacerType.java` (the `BuiltInRegistries.ROOT_PLACER_TYPE` insertion
/// order, so element id 0).
pub struct RootPlacerTypes;
impl RootPlacerTypes {
    /// `register("mangrove_root_placer", MangroveRootPlacer.CODEC)`.
    pub const MANGROVE_ROOT_PLACER: RootPlacerTypeId =
        RootPlacerTypeId::new(0, "minecraft:mangrove_root_placer");
}

/// `BuiltInRegistries.ROOT_PLACER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. The single Paper entry is registered,
/// so the known location resolves.
pub fn root_placer_type_by_name(name: &str) -> Option<RootPlacerTypeId> {
    match name {
        "minecraft:mangrove_root_placer" => Some(RootPlacerTypes::MANGROVE_ROOT_PLACER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.ROOT_PLACER_TYPE` element id equals the
        // insertion index in `RootPlacerType.java`'s declaration order.
        assert_eq!(RootPlacerTypes::MANGROVE_ROOT_PLACER.id, 0);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            RootPlacerTypes::MANGROVE_ROOT_PLACER.location,
            "minecraft:mangrove_root_placer"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        assert_eq!(
            root_placer_type_by_name(RootPlacerTypes::MANGROVE_ROOT_PLACER.location),
            Some(RootPlacerTypes::MANGROVE_ROOT_PLACER)
        );
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(root_placer_type_by_name("minecraft:nope"), None);
        assert_eq!(root_placer_type_by_name("mangrove_root_placer"), None);
    }
}

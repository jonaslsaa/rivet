//! Port of `net.minecraft.world.level.levelgen.heightproviders.HeightProviderType`
//! (interface, 26.2).
//!
//! Java is the interface every height provider's `type()` returns; its six
//! constants are `register(...)` calls into
//! `BuiltInRegistries.HEIGHT_PROVIDER_TYPE`, each holding the provider's
//! `MapCodec`, in this exact declaration order. The Rust port mirrors
//! `BlockPredicateType`'s identity split: the provider's type identity is the
//! opaque [`HeightProviderTypeId`] handle (the registry element identity —
//! element id == insertion index), and the per-type `MapCodec`s are resolved by
//! the dispatch table in `height_provider`, not stored on the id.
//!
//! Unlike `BlockPredicateType` there is no behavior-carrying marker trait: the
//! height-provider dispatch is a closed enum over its six concrete types (the
//! same shape `VerticalAnchor` uses), so the type id alone drives both the codec
//! dispatch and `type()`.
//!
//! All six Paper constants are declared with their exact registry identity and
//! declaration order (ids 0..=5), reproducing
//! `BuiltInRegistries.HEIGHT_PROVIDER_TYPE`'s insertion order.

/// The `HeightProviderType<P>` registry element identity — the per-type `u32`
/// id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `BlockPredicateTypeId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeightProviderTypeId {
    /// The per-type `u32` identity (insertion index in the height-provider-type
    /// registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "constant", …)` → `minecraft:constant`).
    pub location: &'static str,
}

impl HeightProviderTypeId {
    /// `new HeightProviderTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> HeightProviderTypeId {
        HeightProviderTypeId { id, location }
    }
}

/// The six `HeightProviderTypes` constants — Paper's exact declaration order in
/// `HeightProviderType.java` (the `BuiltInRegistries.HEIGHT_PROVIDER_TYPE`
/// insertion order, so element ids 0..=5).
pub struct HeightProviderTypes;
impl HeightProviderTypes {
    /// `register("constant", ConstantHeight.CODEC)`.
    pub const CONSTANT: HeightProviderTypeId = HeightProviderTypeId::new(0, "minecraft:constant");
    /// `register("uniform", UniformHeight.CODEC)`.
    pub const UNIFORM: HeightProviderTypeId = HeightProviderTypeId::new(1, "minecraft:uniform");
    /// `register("biased_to_bottom", BiasedToBottomHeight.CODEC)`.
    pub const BIASED_TO_BOTTOM: HeightProviderTypeId =
        HeightProviderTypeId::new(2, "minecraft:biased_to_bottom");
    /// `register("very_biased_to_bottom", VeryBiasedToBottomHeight.CODEC)`.
    pub const VERY_BIASED_TO_BOTTOM: HeightProviderTypeId =
        HeightProviderTypeId::new(3, "minecraft:very_biased_to_bottom");
    /// `register("trapezoid", TrapezoidHeight.CODEC)`.
    pub const TRAPEZOID: HeightProviderTypeId = HeightProviderTypeId::new(4, "minecraft:trapezoid");
    /// `register("weighted_list", WeightedListHeight.CODEC)`.
    pub const WEIGHTED_LIST: HeightProviderTypeId =
        HeightProviderTypeId::new(5, "minecraft:weighted_list");
}

/// `BuiltInRegistries.HEIGHT_PROVIDER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All six Paper entries are registered,
/// so every known location resolves.
pub fn height_provider_type_by_name(name: &str) -> Option<HeightProviderTypeId> {
    match name {
        "minecraft:constant" => Some(HeightProviderTypes::CONSTANT),
        "minecraft:uniform" => Some(HeightProviderTypes::UNIFORM),
        "minecraft:biased_to_bottom" => Some(HeightProviderTypes::BIASED_TO_BOTTOM),
        "minecraft:very_biased_to_bottom" => Some(HeightProviderTypes::VERY_BIASED_TO_BOTTOM),
        "minecraft:trapezoid" => Some(HeightProviderTypes::TRAPEZOID),
        "minecraft:weighted_list" => Some(HeightProviderTypes::WEIGHTED_LIST),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.HEIGHT_PROVIDER_TYPE` element ids equal the
        // insertion index in `HeightProviderType.java`'s declaration order.
        assert_eq!(HeightProviderTypes::CONSTANT.id, 0);
        assert_eq!(HeightProviderTypes::UNIFORM.id, 1);
        assert_eq!(HeightProviderTypes::BIASED_TO_BOTTOM.id, 2);
        assert_eq!(HeightProviderTypes::VERY_BIASED_TO_BOTTOM.id, 3);
        assert_eq!(HeightProviderTypes::TRAPEZOID.id, 4);
        assert_eq!(HeightProviderTypes::WEIGHTED_LIST.id, 5);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(HeightProviderTypes::CONSTANT.location, "minecraft:constant");
        assert_eq!(HeightProviderTypes::UNIFORM.location, "minecraft:uniform");
        assert_eq!(
            HeightProviderTypes::BIASED_TO_BOTTOM.location,
            "minecraft:biased_to_bottom"
        );
        assert_eq!(
            HeightProviderTypes::VERY_BIASED_TO_BOTTOM.location,
            "minecraft:very_biased_to_bottom"
        );
        assert_eq!(
            HeightProviderTypes::TRAPEZOID.location,
            "minecraft:trapezoid"
        );
        assert_eq!(
            HeightProviderTypes::WEIGHTED_LIST.location,
            "minecraft:weighted_list"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            HeightProviderTypes::CONSTANT,
            HeightProviderTypes::UNIFORM,
            HeightProviderTypes::BIASED_TO_BOTTOM,
            HeightProviderTypes::VERY_BIASED_TO_BOTTOM,
            HeightProviderTypes::TRAPEZOID,
            HeightProviderTypes::WEIGHTED_LIST,
        ] {
            assert_eq!(height_provider_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(height_provider_type_by_name("minecraft:nope"), None);
        assert_eq!(height_provider_type_by_name("constant"), None);
    }
}

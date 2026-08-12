//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! BlockStateProviderType` (class, 26.2).
//!
//! Java is the per-provider wrapper holding each `MapCodec<P>`; its eight
//! constants are `register(...)` calls into `BuiltInRegistries.
//! BLOCKSTATE_PROVIDER_TYPE`, each carrying the provider's `MapCodec`, in this
//! exact declaration order. The Rust port mirrors `BlockPredicateType`'s
//! identity split: the provider's type identity is the opaque
//! [`BlockStateProviderTypeId`] handle (the registry element identity — element
//! id == insertion index), and the per-type `MapCodec`s are resolved by the
//! dispatch table in `block_state_provider`, not stored on the id.
//!
//! All eight Paper constants are declared with their exact registry identity
//! and declaration order (ids 0..=7), reproducing
//! `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE`'s insertion order.

/// The `BlockStateProviderType<P>` registry element identity — the per-type
/// `u32` id (element id == holder id == insertion index) plus its registry-key
/// location, mirroring `BlockPredicateTypeId`. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockStateProviderTypeId {
    /// The per-type `u32` identity (insertion index in the block-state-provider-
    /// type registry).
    pub id: u32,
    /// The registry-key location of the type's registration (`register(
    /// "simple_state_provider", …)` → `minecraft:simple_state_provider`).
    pub location: &'static str,
}

impl BlockStateProviderTypeId {
    /// `new BlockStateProviderTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> BlockStateProviderTypeId {
        BlockStateProviderTypeId { id, location }
    }
}

/// The eight `BlockStateProviderTypes` constants — Paper's exact declaration
/// order in `BlockStateProviderType.java` (the
/// `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE` insertion order, so element ids
/// 0..=7).
pub struct BlockStateProviderTypes;
impl BlockStateProviderTypes {
    /// `register("simple_state_provider", SimpleStateProvider.CODEC)`.
    pub const SIMPLE_STATE_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(0, "minecraft:simple_state_provider");
    /// `register("weighted_state_provider", WeightedStateProvider.CODEC)`.
    pub const WEIGHTED_STATE_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(1, "minecraft:weighted_state_provider");
    /// `register("noise_threshold_provider", NoiseThresholdProvider.CODEC)`.
    pub const NOISE_THRESHOLD_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(2, "minecraft:noise_threshold_provider");
    /// `register("noise_provider", NoiseProvider.CODEC)`.
    pub const NOISE_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(3, "minecraft:noise_provider");
    /// `register("dual_noise_provider", DualNoiseProvider.CODEC)`.
    pub const DUAL_NOISE_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(4, "minecraft:dual_noise_provider");
    /// `register("rotated_block_provider", RotatedBlockProvider.CODEC)`.
    pub const ROTATED_BLOCK_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(5, "minecraft:rotated_block_provider");
    /// `register("randomized_int_state_provider", RandomizedIntStateProvider.CODEC)`.
    pub const RANDOMIZED_INT_STATE_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(6, "minecraft:randomized_int_state_provider");
    /// `register("rule_based_state_provider", RuleBasedStateProvider.CODEC)`.
    pub const RULE_BASED_STATE_PROVIDER: BlockStateProviderTypeId =
        BlockStateProviderTypeId::new(7, "minecraft:rule_based_state_provider");
}

/// `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.get(Identifier)` — resolve a
/// registry-key location to its type id. All eight Paper entries are
/// registered, so every known location resolves.
pub fn block_state_provider_type_by_name(name: &str) -> Option<BlockStateProviderTypeId> {
    match name {
        "minecraft:simple_state_provider" => Some(BlockStateProviderTypes::SIMPLE_STATE_PROVIDER),
        "minecraft:weighted_state_provider" => {
            Some(BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER)
        }
        "minecraft:noise_threshold_provider" => {
            Some(BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER)
        }
        "minecraft:noise_provider" => Some(BlockStateProviderTypes::NOISE_PROVIDER),
        "minecraft:dual_noise_provider" => Some(BlockStateProviderTypes::DUAL_NOISE_PROVIDER),
        "minecraft:rotated_block_provider" => Some(BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER),
        "minecraft:randomized_int_state_provider" => {
            Some(BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER)
        }
        "minecraft:rule_based_state_provider" => {
            Some(BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_declaration_order_and_ids() {
        // The `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE` element ids equal the
        // insertion index in `BlockStateProviderType.java`'s declaration order.
        assert_eq!(BlockStateProviderTypes::SIMPLE_STATE_PROVIDER.id, 0);
        assert_eq!(BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER.id, 1);
        assert_eq!(BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER.id, 2);
        assert_eq!(BlockStateProviderTypes::NOISE_PROVIDER.id, 3);
        assert_eq!(BlockStateProviderTypes::DUAL_NOISE_PROVIDER.id, 4);
        assert_eq!(BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER.id, 5);
        assert_eq!(BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER.id, 6);
        assert_eq!(BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER.id, 7);
    }

    #[test]
    fn paper_registry_key_locations() {
        assert_eq!(
            BlockStateProviderTypes::SIMPLE_STATE_PROVIDER.location,
            "minecraft:simple_state_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER.location,
            "minecraft:weighted_state_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER.location,
            "minecraft:noise_threshold_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::NOISE_PROVIDER.location,
            "minecraft:noise_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::DUAL_NOISE_PROVIDER.location,
            "minecraft:dual_noise_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER.location,
            "minecraft:rotated_block_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER.location,
            "minecraft:randomized_int_state_provider"
        );
        assert_eq!(
            BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER.location,
            "minecraft:rule_based_state_provider"
        );
    }

    #[test]
    fn by_name_resolves_every_registered_type() {
        for id in [
            BlockStateProviderTypes::SIMPLE_STATE_PROVIDER,
            BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER,
            BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER,
            BlockStateProviderTypes::NOISE_PROVIDER,
            BlockStateProviderTypes::DUAL_NOISE_PROVIDER,
            BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER,
            BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER,
            BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER,
        ] {
            assert_eq!(block_state_provider_type_by_name(id.location), Some(id));
        }
    }

    #[test]
    fn by_name_unknown_location_is_none() {
        assert_eq!(block_state_provider_type_by_name("minecraft:nope"), None);
        assert_eq!(
            block_state_provider_type_by_name("simple_state_provider"),
            None
        );
    }
}

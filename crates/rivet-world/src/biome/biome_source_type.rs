//! Port of `net.minecraft.world.level.biome.BiomeSourceType` — the
//! `BuiltInRegistries.BIOME_SOURCE` element identity (`mc.world.level.biome.source`
//! unit).
//!
//! Java has no `BiomeSourceType` class; the registry `BuiltInRegistries.
//! BIOME_SOURCE` holds a `MapCodec<? extends BiomeSource>` per source, and
//! `BiomeSources.bootstrap` registers the four types by name (`fixed`,
//! `multi_noise`, `checkerboard`, `the_end` in that exact declaration order).
//! The Rust port mirrors `Feature`/`BlockPredicate`/`FeatureSizeType`'s
//! identity split: the source's type identity is the opaque [`BiomeSourceTypeId`]
//! handle, and the per-type `MapCodec`s are resolved by the dispatch table in
//! `biome_source`, not stored on the id.

use std::fmt::Debug;

/// The `BiomeSourceType` registry element identity — the per-type `u32` id
/// (element id == insertion index in `BiomeSources.bootstrap`'s registration
/// order) plus its registry-key location. Identity-semantic (not `Copy`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BiomeSourceTypeId {
    /// The per-type `u32` identity (insertion index in `worldgen/biome_source`).
    pub id: u32,
    /// The registry-key location of the type's registration (`register("fixed",
    /// …)` → `minecraft:fixed`).
    pub location: &'static str,
}

impl BiomeSourceTypeId {
    /// `new BiomeSourceTypeId(u32, location)`.
    pub const fn new(id: u32, location: &'static str) -> BiomeSourceTypeId {
        BiomeSourceTypeId { id, location }
    }
}

/// The four `BiomeSources.bootstrap` constants — Paper's exact registration
/// order in `BiomeSources.java` (the `BuiltInRegistries.BIOME_SOURCE` insertion
/// order, so element ids 0..=3).
pub struct BiomeSourceTypes;
impl BiomeSourceTypes {
    /// `Registry.register(registry, "fixed", FixedBiomeSource.CODEC)`.
    pub const FIXED: BiomeSourceTypeId = BiomeSourceTypeId::new(0, "minecraft:fixed");
    /// `Registry.register(registry, "multi_noise", MultiNoiseBiomeSource.CODEC)`.
    pub const MULTI_NOISE: BiomeSourceTypeId = BiomeSourceTypeId::new(1, "minecraft:multi_noise");
    /// `Registry.register(registry, "checkerboard", CheckerboardColumnBiomeSource.CODEC)`.
    pub const CHECKERBOARD: BiomeSourceTypeId = BiomeSourceTypeId::new(2, "minecraft:checkerboard");
    /// `Registry.register(registry, "the_end", TheEndBiomeSource.CODEC)`.
    pub const THE_END: BiomeSourceTypeId = BiomeSourceTypeId::new(3, "minecraft:the_end");
}

/// `BiomeSourceType.byName` — resolve a registry location to its type id (the
/// decode half of the `"type"` by-name codec).
pub fn biome_source_type_by_name(name: &str) -> Option<BiomeSourceTypeId> {
    match name {
        "minecraft:fixed" => Some(BiomeSourceTypes::FIXED),
        "minecraft:multi_noise" => Some(BiomeSourceTypes::MULTI_NOISE),
        "minecraft:checkerboard" => Some(BiomeSourceTypes::CHECKERBOARD),
        "minecraft:the_end" => Some(BiomeSourceTypes::THE_END),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_registration_order_and_ids() {
        // The `BuiltInRegistries.BIOME_SOURCE` element ids equal the insertion
        // index in `BiomeSources.bootstrap`'s declaration order.
        assert_eq!(BiomeSourceTypes::FIXED.id, 0);
        assert_eq!(BiomeSourceTypes::MULTI_NOISE.id, 1);
        assert_eq!(BiomeSourceTypes::CHECKERBOARD.id, 2);
        assert_eq!(BiomeSourceTypes::THE_END.id, 3);
        assert_eq!(BiomeSourceTypes::FIXED.location, "minecraft:fixed");
        assert_eq!(
            BiomeSourceTypes::MULTI_NOISE.location,
            "minecraft:multi_noise"
        );
        assert_eq!(
            BiomeSourceTypes::CHECKERBOARD.location,
            "minecraft:checkerboard"
        );
        assert_eq!(BiomeSourceTypes::THE_END.location, "minecraft:the_end");
    }

    #[test]
    fn by_name_resolves_only_the_four_registered_types() {
        assert_eq!(
            biome_source_type_by_name("minecraft:fixed"),
            Some(BiomeSourceTypes::FIXED)
        );
        assert_eq!(
            biome_source_type_by_name("minecraft:multi_noise"),
            Some(BiomeSourceTypes::MULTI_NOISE)
        );
        assert_eq!(
            biome_source_type_by_name("minecraft:checkerboard"),
            Some(BiomeSourceTypes::CHECKERBOARD)
        );
        assert_eq!(
            biome_source_type_by_name("minecraft:the_end"),
            Some(BiomeSourceTypes::THE_END)
        );
        assert_eq!(biome_source_type_by_name("minecraft:custom"), None);
    }
}

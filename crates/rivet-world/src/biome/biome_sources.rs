//! Port of `net.minecraft.world.level.biome.BiomeSources` (26.2) — the
//! `mc.world.level.biome.source` unit.
//!
//! Registers the four biome-source types into `BuiltInRegistries.BIOME_SOURCE`
//! in Paper's exact declaration order:
//!
//! ```text
//! bootstrap(Registry<MapCodec<? extends BiomeSource>> registry):
//!     Registry.register(registry, "fixed",        FixedBiomeSource.CODEC);
//!     Registry.register(registry, "multi_noise",  MultiNoiseBiomeSource.CODEC);
//!     Registry.register(registry, "checkerboard", CheckerboardColumnBiomeSource.CODEC);
//!     return Registry.register(registry, "the_end", TheEndBiomeSource.CODEC);
//! ```
//!
//! Java's registry holds each source's `MapCodec`; the Rust port keeps the
//! identity split (see [`crate::biome::biome_source_type`]): the registry holds
//! the [`BiomeSourceTypeId`] handles — element id == insertion index — and the
//! dispatch table in [`crate::biome::biome_source`] resolves the per-type
//! `MapCodec`s by name. `bootstrap` reproduces the registration order (and the
//! Java return of the `the_end` registration).

use crate::biome::biome_source_type::{BiomeSourceTypeId, BiomeSourceTypes};

/// `BiomeSources.bootstrap(Registry<MapCodec<? extends BiomeSource>>)` — the
/// four registrations in declaration order, returning the `the_end` element
/// (Java returns the last `Registry.register`).
///
/// The port does not construct the `Registry` object: the four type identities
/// are the [`BiomeSourceTypes`] constants (element id == insertion order), the
/// `"type"` by-name codec resolves them through the hardcoded match in
/// [`crate::biome::biome_source_type`], and the dispatch table in
/// [`crate::biome::biome_source::codec_for_type`] resolves the per-type
/// `MapCodec`s. So `bootstrap` is the registration *order* made explicit — it
/// returns the last-registered element, the `the_end` id, without materializing
/// the registry. The `element_key` helper below is test-only: it builds a local
/// frozen registry so the insertion-order ids can be verified against the
/// constants.
pub fn bootstrap() -> BiomeSourceTypeId {
    // Java returns `Registry.register(registry, "the_end", ...)` — the
    // last-registered element id (element id == insertion index).
    BiomeSourceTypes::THE_END
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_source_type::biome_source_type_by_name;
    use rivet_registry::RegistryAccess;
    use rivet_registry::ResourceKey;
    use rivet_registry::holder_lookup::HolderLookup;
    use rivet_registry::{RegistrationInfo, RegistryBuilder};
    use std::sync::Arc;

    /// `ResourceKey.create(registryKey, Identifier.withDefaultNamespace(name))`
    /// — the per-type element key (Java's `Registry.register(registry, name,
    /// codec)` location).
    fn element_key(
        registry_key: &rivet_registry::ResourceKey<rivet_registry::Registry<BiomeSourceTypeId>>,
        name: &str,
    ) -> ResourceKey<BiomeSourceTypeId> {
        ResourceKey::create(
            registry_key,
            rivet_registry::Identifier::with_default_namespace(name),
        )
    }

    #[test]
    fn bootstrap_returns_the_last_registered_type_and_all_types_resolve_by_name() {
        // `bootstrap` returns the last-registered element id (Java's `the_end`
        // `Registry.register`); the registration *contents* are the four
        // `BiomeSourceTypes` constants, which the `"type"` by-name codec
        // resolves through `biome_source_type_by_name`.
        let the_end = bootstrap();
        assert_eq!(the_end, BiomeSourceTypes::THE_END);
        for name in [
            "minecraft:fixed",
            "minecraft:multi_noise",
            "minecraft:checkerboard",
            "minecraft:the_end",
        ] {
            assert!(
                biome_source_type_by_name(name).is_some(),
                "{name} must be registered"
            );
        }
        assert_eq!(biome_source_type_by_name("minecraft:custom"), None);
    }

    #[test]
    fn registry_element_ids_match_insertion_order() {
        // The `BIOME_SOURCE` registry builder assigns ids in registration
        // order — verify the frozen registry's element ids line up with the
        // `BiomeSourceTypes` constants (element id == insertion index).
        let key = rivet_registry::ResourceKey::create_registry_key(
            rivet_registry::Identifier::with_default_namespace("worldgen/biome_source"),
        );
        let mut builder = RegistryBuilder::<BiomeSourceTypeId>::new(&key);
        builder.register(
            &element_key(&key, "fixed"),
            Arc::new(BiomeSourceTypes::FIXED),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &element_key(&key, "multi_noise"),
            Arc::new(BiomeSourceTypes::MULTI_NOISE),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &element_key(&key, "checkerboard"),
            Arc::new(BiomeSourceTypes::CHECKERBOARD),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &element_key(&key, "the_end"),
            Arc::new(BiomeSourceTypes::THE_END),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let ids: Vec<u32> = registry
            .list_elements()
            .iter()
            .filter_map(|h| match h {
                rivet_registry::Holder::Reference { id, .. } => Some(*id),
                rivet_registry::Holder::Direct(_) => None,
            })
            .collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
        let _ = RegistryAccess::from_single_registry(key, registry);
    }
}

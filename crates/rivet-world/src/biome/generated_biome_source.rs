//! The generated biome-registry bootstrap for current-version generated chunks
//! (issue #178, `mc.world.level.biome.core` unit).
//!
//! `ChunkAccess.fillBiomesFromNoise` needs a `BiomeResolver` producing the
//! dense biome ids a worldgen chunk's sections store. Java reaches that through
//! the runtime `RegistryAccess` (the `BuiltInRegistries.BIOME` getter); the port
//! resolves the same biomes straight from the generated
//! `minecraft:worldgen/biome` tables (`rivet-registry::generated::biomes`), so
//! a generated chunk can be filled without booting a registry.
//!
//! [`overworld_biome_source`] builds the resolved overworld
//! `MultiNoiseBiomeSource` over that getter (Java
//! `MultiNoiseBiomeSourceParameterLists.OVERWORLD`), and [`dense_biome_id`] is
//! the `Holder<BiomeId>` → dense `u16` conversion the section fill's
//! `map_biome` seam needs for the `section_reconstruction::BiomeId` container.

use crate::biome::multi_noise_biome_source::MultiNoiseBiomeSource;
use crate::biome::multi_noise_biome_source_parameter_list::{
    BY_NAME, MultiNoiseBiomeSourceParameterList,
};
use rivet_registry::ResourceKey;
use rivet_registry::TagKey;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::generated::tags::WORLDGEN_BIOME_TAG_BY_NAME;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::holder_set::HolderSet;

/// `BuiltInRegistries.BIOME` getter resolved over the generated biome tables.
///
/// Java's `RegistryGetter` resolves a biome `ResourceKey` to its registered
/// holder; the port resolves the generated name to a `Direct` id holder. Every
/// name in the generated table resolves (`BIOME_BY_NAME` is total over the
/// registry), so `get_or_throw` never trips for a valid key.
pub struct GeneratedBiomeGetter;

impl HolderGetter<BiomeId> for GeneratedBiomeGetter {
    fn get(&self, key: &ResourceKey<BiomeId>) -> Option<Holder<BiomeId>> {
        BiomeId::from_name(&key.identifier().to_string()).map(Holder::direct)
    }

    /// The generated `minecraft:worldgen/biome` tag tables
    /// (`WORLDGEN_BIOME_TAG_BY_NAME`, element names resolved through
    /// `BIOME_BY_NAME`). Both tables are generated from the same MC 26.2
    /// registry load, so every tag element resolves; an unknown tag key reports
    /// `None` like Java's `Optional.empty()`. The set is `Direct` (a registry
    /// `Named` set needs a `RegistryId` owner the registry-less getter does not
    /// have); the generated paths consume the member holders, not the named
    /// identity.
    fn get_tag(&self, tag: &TagKey<BiomeId>) -> Option<HolderSet<BiomeId>> {
        let names = WORLDGEN_BIOME_TAG_BY_NAME.get(&tag.location().to_string())?;
        Some(HolderSet::direct(
            names
                .iter()
                .map(|name| {
                    Holder::direct(BiomeId::from_name(name).unwrap_or_else(|| {
                        panic!("generated biome tag element {name} does not resolve")
                    }))
                })
                .collect(),
        ))
    }
}

/// The resolved overworld `MultiNoiseBiomeSource` — Java's
/// `MultiNoiseBiomeSourceParameterLists.OVERWORLD` (the `overworld` preset
/// applied through the generated biome getter, with the 7594-point table built
/// by the `.data`-owned `OverworldBiomeBuilder`). No registry bootstrapping is
/// required.
pub fn overworld_biome_source() -> MultiNoiseBiomeSource {
    let preset = BY_NAME
        .get("minecraft:overworld")
        .expect("overworld preset is generated");
    let parameter_list =
        MultiNoiseBiomeSourceParameterList::new(preset.clone(), &GeneratedBiomeGetter);
    MultiNoiseBiomeSource::create_from_preset(parameter_list)
}

/// `Holder<BiomeId>` → dense `u16` — the shared holder→dense-id conversion for
/// the biome-id element model: the `map_biome` seam of the worldgen chunk's
/// `section_reconstruction::BiomeId` container, and the `#177` surface-build
/// runtime. A `Direct` holder reads its id; a `Reference` holder (the
/// codec-produced form) carries the registry id.
pub fn dense_biome_id(holder: &Holder<BiomeId>) -> u16 {
    match holder {
        Holder::Direct(biome) => biome.id(),
        Holder::Reference { id, .. } => *id as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_source::BiomeSource;

    #[test]
    fn overworld_biome_source_resolves_from_the_generated_tables() {
        let source = overworld_biome_source();
        // The overworld preset's table is non-empty (7594 points) and every
        // biome name resolves through the generated getter.
        assert!(!source.collect_possible_biomes().is_empty());
        // `minecraft:plains` (id 40) is among the possible overworld biomes.
        assert!(
            source
                .collect_possible_biomes()
                .contains(&Holder::direct(BiomeId::from_id(40)))
        );
    }

    #[test]
    fn generated_biome_getter_resolves_names_and_rejects_unknowns() {
        let plains_key = ResourceKey::create(
            &rivet_registry::registries::BIOME,
            rivet_registry::Identifier::with_default_namespace("plains"),
        );
        assert_eq!(
            GeneratedBiomeGetter.get(&plains_key),
            Some(Holder::direct(BiomeId::from_id(40)))
        );
        let unknown = ResourceKey::create(
            &rivet_registry::registries::BIOME,
            rivet_registry::Identifier::with_default_namespace("not_a_biome"),
        );
        assert_eq!(GeneratedBiomeGetter.get(&unknown), None);
    }

    #[test]
    fn dense_biome_id_extracts_the_direct_id_and_reference_id() {
        assert_eq!(dense_biome_id(&Holder::direct(BiomeId::from_id(40))), 40);
        assert_eq!(
            dense_biome_id(&Holder::reference(rivet_registry::holder::RegistryId(0), 7)),
            7
        );
    }

    #[test]
    fn generated_biome_getter_resolves_generated_tags_and_rejects_unknowns() {
        let tag_key = |name: &str| {
            rivet_registry::TagKey::create(
                &rivet_registry::registries::BIOME,
                rivet_registry::Identifier::with_default_namespace(name),
            )
        };
        // `minecraft:allows_surface_slime_spawns` → [swamp, mangrove_swamp].
        let set = GeneratedBiomeGetter
            .get_tag(&tag_key("allows_surface_slime_spawns"))
            .expect("generated tag resolves");
        let swamp = Holder::direct(BiomeId::from_id(
            BiomeId::from_name("minecraft:swamp").unwrap().id(),
        ));
        let mangrove_swamp = Holder::direct(BiomeId::from_id(
            BiomeId::from_name("minecraft:mangrove_swamp").unwrap().id(),
        ));
        assert_eq!(set, HolderSet::direct(vec![swamp, mangrove_swamp]));
        // An unknown tag key reports `None` (Java `Optional.empty()`).
        assert_eq!(GeneratedBiomeGetter.get_tag(&tag_key("not_a_tag")), None);
    }
}

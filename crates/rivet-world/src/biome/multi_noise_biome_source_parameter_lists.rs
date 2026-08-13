//! Port of `net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterLists`
//! (26.2) — the `mc.world.level.biome.source` unit.
//!
//! The two registered preset keys (`nether`/`overworld`) and their bootstrap,
//! which resolves the biome `HolderGetter` through the `BootstrapContext` and
//! registers a `MultiNoiseBiomeSourceParameterList` per preset:
//!
//! ```text
//! bootstrap(BootstrapContext<MultiNoiseBiomeSourceParameterList>):
//!     HolderGetter<Biome> biomes = context.lookup(Registries.BIOME);
//!     context.register(NETHER,   new MultiNoiseBiomeSourceParameterList(Preset.NETHER,   biomes));
//!     context.register(OVERWORLD, new MultiNoiseBiomeSourceParameterList(Preset.OVERWORLD, biomes));
//! ```
//!
//! The `BootstrapContext::lookup` `Option` is a documented seam deviation
//! (Java always returns the `UniversalLookup` empty fallback; see the
//! `bootstrap_context` module docs), so an absent biome registry reports `None`
//! and is treated as a bootstrap error.
//!
//! Both preset keys are registered: `bootstrap` registers `NETHER` then
//! `OVERWORLD` (Paper's declaration order).

use crate::biome::biome_source::keys;
use crate::biome::multi_noise_biome_source_parameter_list::{
    MultiNoiseBiomeSourceParameterList, Preset,
};
use crate::data::worldgen::bootstrap_context::BootstrapContext;
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use std::sync::LazyLock;

/// `MultiNoiseBiomeSourceParameterLists.NETHER` — `register("nether")`.
pub static NETHER: LazyLock<ResourceKey<MultiNoiseBiomeSourceParameterList>> =
    LazyLock::new(|| register("nether"));
/// `MultiNoiseBiomeSourceParameterLists.OVERWORLD` — `register("overworld")`.
pub static OVERWORLD: LazyLock<ResourceKey<MultiNoiseBiomeSourceParameterList>> =
    LazyLock::new(|| register("overworld"));

/// `MultiNoiseBiomeSourceParameterLists.bootstrap(BootstrapContext<
/// MultiNoiseBiomeSourceParameterList>)` — resolves the biome getter and
/// registers the supported preset lists (Java `context.register`, the stable
/// lifecycle default).
pub fn bootstrap(context: &mut impl BootstrapContext<MultiNoiseBiomeSourceParameterList>) {
    // Java's `context.lookup(Registries.BIOME)` — each list is built inside a
    // block that releases the `&mut context` borrow before the register call
    // (the `NoiseRouterData.bootstrap` idiom).
    let nether = {
        let biomes = context
            .lookup(&rivet_registry::registries::BIOME)
            .expect("biome registry present in bootstrap");
        MultiNoiseBiomeSourceParameterList::new(Preset::nether(), biomes)
    };
    context.register_default(&NETHER, nether);
    let overworld = {
        let biomes = context
            .lookup(&rivet_registry::registries::BIOME)
            .expect("biome registry present in bootstrap");
        MultiNoiseBiomeSourceParameterList::new(Preset::overworld(), biomes)
    };
    context.register_default(&OVERWORLD, overworld);
}

/// `MultiNoiseBiomeSourceParameterLists.register(String)` —
/// `ResourceKey.create(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST,
/// Identifier.withDefaultNamespace(name))`.
fn register(name: &str) -> ResourceKey<MultiNoiseBiomeSourceParameterList> {
    ResourceKey::create(
        &keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST,
        Identifier::with_default_namespace(name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_use_the_preset_registry_with_exact_locations() {
        assert_eq!(NETHER.identifier().to_string(), "minecraft:nether");
        assert_eq!(OVERWORLD.identifier().to_string(), "minecraft:overworld");
        assert_eq!(
            NETHER.registry().to_string(),
            "minecraft:worldgen/multi_noise_biome_source_parameter_list"
        );
    }

    #[test]
    fn bootstrap_registers_both_presets_in_declaration_order() {
        use crate::data::worldgen::bootstrap_context::{RecordedRegistration, RecordingContext};
        use rivet_registry::biome_id::BiomeId;
        use rivet_registry::generated::biomes::BIOME_BY_ID;
        use rivet_registry::holder::RegistryId;
        use rivet_registry::{RegistrationInfo, RegistryAccess, RegistryBuilder};
        use rivet_serialization::lifecycle::Lifecycle;
        use std::sync::Arc;

        // Java's `context.lookup(Registries.BIOME)` resolves the real biome
        // registry, so the access carries one with every generated biome key
        // (both presets `getOrThrow` exactly these — the nether's five and the
        // overworld's 55).
        let mut builder = RegistryBuilder::<BiomeId>::new(&rivet_registry::registries::BIOME);
        for (i, name) in BIOME_BY_ID.iter().enumerate() {
            builder.register(
                &rivet_registry::ResourceKey::create(
                    &rivet_registry::registries::BIOME,
                    rivet_registry::Identifier::parse(name),
                ),
                Arc::new(BiomeId::from_id(i as u16)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let biome_registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(
            rivet_registry::registries::BIOME.clone(),
            biome_registry,
        );

        let mut context = RecordingContext::<MultiNoiseBiomeSourceParameterList>::new(
            RegistryId(7),
            keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST.clone(),
            access,
        );
        bootstrap(&mut context);

        let regs: Vec<RecordedRegistration<MultiNoiseBiomeSourceParameterList>> =
            context.registrations().iter().cloned().collect();
        // Both presets register, nether first (Paper's declaration order).
        assert_eq!(regs.len(), 2);
        assert_eq!(regs[0].key.identifier().to_string(), "minecraft:nether");
        assert_eq!(regs[0].lifecycle, Lifecycle::stable());
        assert_eq!(regs[1].key.identifier().to_string(), "minecraft:overworld");
        assert_eq!(regs[1].lifecycle, Lifecycle::stable());
        // The nether preset's five biome keys are present in declaration order.
        let nether_used: Vec<String> = regs[0]
            .value
            .preset
            .used_biomes()
            .iter()
            .map(|k| k.identifier().to_string())
            .collect();
        assert_eq!(
            nether_used,
            vec![
                "minecraft:nether_wastes",
                "minecraft:soul_sand_valley",
                "minecraft:crimson_forest",
                "minecraft:warped_forest",
                "minecraft:basalt_deltas",
            ]
        );
        // The overworld preset carries the full 7594-point list (55 distinct
        // biomes) — the `.data` table is applied, not deferred.
        assert_eq!(regs[1].value.parameters().values().len(), 7594);
        assert_eq!(regs[1].value.preset.used_biomes().len(), 55);
    }
}

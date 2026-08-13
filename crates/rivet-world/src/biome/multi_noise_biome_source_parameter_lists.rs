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
//! The `OVERWORLD` registration is deferred: `Preset::overworld` application is
//! fallible (`OverworldDeferred`) while the `.data`-owned
//! `OverworldBiomeBuilder::add_biomes` STUB emits nothing, so `bootstrap`
//! registers only the `NETHER` preset. See the RivetTodo on [`bootstrap`].

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
///
/// Only the `NETHER` preset is registered today. The `OVERWORLD` registration
/// is deferred — see the RivetTodo — and is not applied, so the fallible
/// `Preset::overworld` provider is never invoked here.
pub fn bootstrap(context: &mut impl BootstrapContext<MultiNoiseBiomeSourceParameterList>) {
    // Java's `context.lookup(Registries.BIOME)` — the list is built inside a
    // block that releases the `&mut context` borrow before the register call
    // (the `NoiseRouterData.bootstrap` idiom).
    let nether = {
        let biomes = context
            .lookup(&rivet_registry::registries::BIOME)
            .expect("biome registry present in bootstrap");
        // The nether provider builds its five entries directly and never
        // returns the overworld deferral.
        MultiNoiseBiomeSourceParameterList::new(Preset::nether(), biomes)
            .expect("the nether preset is never deferred")
    };
    context.register_default(&NETHER, nether);
    // RivetTodo(mc.world.level.biome.data): the OVERWORLD registration is
    // deferred until the `.data` unit's `OverworldBiomeBuilder::add_biomes`
    // table lands (applying `Preset::overworld` currently returns
    // `Err(OverworldDeferred)`). Removal condition: the table emits the
    // overworld parameter list, then add the OVERWORLD registration after the
    // NETHER line (Paper's declaration order) with no other call-site changes.
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
    fn bootstrap_registers_the_nether_preset_while_overworld_is_deferred() {
        use crate::biome::biomes;
        use crate::data::worldgen::bootstrap_context::{RecordedRegistration, RecordingContext};
        use rivet_registry::biome_id::BiomeId;
        use rivet_registry::holder::RegistryId;
        use rivet_registry::{RegistrationInfo, RegistryAccess, RegistryBuilder};
        use rivet_serialization::lifecycle::Lifecycle;
        use std::sync::Arc;

        // Java's `context.lookup(Registries.BIOME)` resolves the real biome
        // registry, so the access carries one with the five nether keys (the
        // `Preset::nether` provider `getOrThrow`s exactly these).
        let nether_keys = [
            &biomes::NETHER_WASTES,
            &biomes::SOUL_SAND_VALLEY,
            &biomes::CRIMSON_FOREST,
            &biomes::WARPED_FOREST,
            &biomes::BASALT_DELTAS,
        ];
        let mut builder = RegistryBuilder::<BiomeId>::new(&rivet_registry::registries::BIOME);
        for (i, key) in nether_keys.iter().enumerate() {
            builder.register(
                key,
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

        // Only the nether preset is supported today: the overworld preset's
        // `add_biomes` is the `STUB(mc.world.level.biome.data)` table (see
        // `overworld_biome_builder`), so applying it is the typed deferral and
        // `bootstrap` never registers it. A full overworld entry lands with the
        // `.data` unit.
        let regs: Vec<RecordedRegistration<MultiNoiseBiomeSourceParameterList>> =
            context.registrations().iter().cloned().collect();
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].key.identifier().to_string(), "minecraft:nether");
        assert_eq!(regs[0].lifecycle, Lifecycle::stable());
        // The nether preset's five biome keys are present in declaration order.
        let used: Vec<String> = regs[0]
            .value
            .preset
            .used_biomes()
            .expect("the nether preset is never deferred")
            .iter()
            .map(|k| k.identifier().to_string())
            .collect();
        assert_eq!(
            used,
            vec![
                "minecraft:nether_wastes",
                "minecraft:soul_sand_valley",
                "minecraft:crimson_forest",
                "minecraft:warped_forest",
                "minecraft:basalt_deltas",
            ]
        );
    }
}

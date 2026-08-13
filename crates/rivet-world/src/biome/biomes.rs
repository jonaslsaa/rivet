//! `net.minecraft.world.level.biome.Biomes` — the full `ResourceKey<BiomeId>`
//! hub (owned by the `mc.world.level.biome.data` manifest unit).
//!
//! `Biomes.java` is the generated registry hub of biome `ResourceKey`s. All 66
//! keys are declared here as id-model constants (`ResourceKey<BiomeId>` over
//! the pure [`rivet_registry::biome_id::BiomeId`] handle), in `Biomes.java`
//! declaration order. The nether/overworld preset builders, the `the_end`
//! source, and the `OverworldBiomeBuilder` tables all bridge by these keys.
//!
//! `register_from_full_name` resolves the full `minecraft:<path>` identifiers
//! carried by the generated preset tables (`worldgen.rs` `ParameterPoint.biome`)
//! to the same value-equal `ResourceKey`s.

use rivet_registry::biome_id::BiomeId;
use rivet_registry::registries;
use rivet_registry::{Identifier, ResourceKey};
use std::sync::LazyLock;

/// `Biomes.register(String name)` — `ResourceKey.create(Registries.BIOME,
/// Identifier.withDefaultNamespace(name))`.
fn register(name: &str) -> ResourceKey<BiomeId> {
    ResourceKey::create(
        &*registries::BIOME,
        Identifier::with_default_namespace(name),
    )
}

/// `ResourceKey.create(Registries.BIOME, Identifier.parse(fullName))` — the
/// resolution for the full `minecraft:<path>` biome names in the generated
/// preset parameter-point tables (`mc.world.level.biome.source`'s
/// `OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS`). The result is value-equal to the
/// [`register`] hub key for the same biome.
pub(crate) fn register_from_full_name(full_name: &str) -> ResourceKey<BiomeId> {
    ResourceKey::create(&*registries::BIOME, Identifier::parse(full_name))
}

/// `Biomes.THE_VOID` — `register("the_void")`.
pub static THE_VOID: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("the_void"));
/// `Biomes.PLAINS` — `register("plains")`.
pub static PLAINS: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("plains"));
/// `Biomes.SUNFLOWER_PLAINS` — `register("sunflower_plains")`.
pub static SUNFLOWER_PLAINS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("sunflower_plains"));
/// `Biomes.SNOWY_PLAINS` — `register("snowy_plains")`.
pub static SNOWY_PLAINS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("snowy_plains"));
/// `Biomes.ICE_SPIKES` — `register("ice_spikes")`.
pub static ICE_SPIKES: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("ice_spikes"));
/// `Biomes.DESERT` — `register("desert")`.
pub static DESERT: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("desert"));
/// `Biomes.SWAMP` — `register("swamp")`.
pub static SWAMP: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("swamp"));
/// `Biomes.MANGROVE_SWAMP` — `register("mangrove_swamp")`.
pub static MANGROVE_SWAMP: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("mangrove_swamp"));
/// `Biomes.FOREST` — `register("forest")`.
pub static FOREST: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("forest"));
/// `Biomes.FLOWER_FOREST` — `register("flower_forest")`.
pub static FLOWER_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("flower_forest"));
/// `Biomes.BIRCH_FOREST` — `register("birch_forest")`.
pub static BIRCH_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("birch_forest"));
/// `Biomes.DARK_FOREST` — `register("dark_forest")`.
pub static DARK_FOREST: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("dark_forest"));
/// `Biomes.PALE_GARDEN` — `register("pale_garden")`.
pub static PALE_GARDEN: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("pale_garden"));
/// `Biomes.OLD_GROWTH_BIRCH_FOREST` — `register("old_growth_birch_forest")`.
pub static OLD_GROWTH_BIRCH_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("old_growth_birch_forest"));
/// `Biomes.OLD_GROWTH_PINE_TAIGA` — `register("old_growth_pine_taiga")`.
pub static OLD_GROWTH_PINE_TAIGA: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("old_growth_pine_taiga"));
/// `Biomes.OLD_GROWTH_SPRUCE_TAIGA` — `register("old_growth_spruce_taiga")`.
pub static OLD_GROWTH_SPRUCE_TAIGA: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("old_growth_spruce_taiga"));
/// `Biomes.TAIGA` — `register("taiga")`.
pub static TAIGA: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("taiga"));
/// `Biomes.SNOWY_TAIGA` — `register("snowy_taiga")`.
pub static SNOWY_TAIGA: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("snowy_taiga"));
/// `Biomes.SAVANNA` — `register("savanna")`.
pub static SAVANNA: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("savanna"));
/// `Biomes.SAVANNA_PLATEAU` — `register("savanna_plateau")`.
pub static SAVANNA_PLATEAU: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("savanna_plateau"));
/// `Biomes.WINDSWEPT_HILLS` — `register("windswept_hills")`.
pub static WINDSWEPT_HILLS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("windswept_hills"));
/// `Biomes.WINDSWEPT_GRAVELLY_HILLS` — `register("windswept_gravelly_hills")`.
pub static WINDSWEPT_GRAVELLY_HILLS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("windswept_gravelly_hills"));
/// `Biomes.WINDSWEPT_FOREST` — `register("windswept_forest")`.
pub static WINDSWEPT_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("windswept_forest"));
/// `Biomes.WINDSWEPT_SAVANNA` — `register("windswept_savanna")`.
pub static WINDSWEPT_SAVANNA: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("windswept_savanna"));
/// `Biomes.JUNGLE` — `register("jungle")`.
pub static JUNGLE: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("jungle"));
/// `Biomes.SPARSE_JUNGLE` — `register("sparse_jungle")`.
pub static SPARSE_JUNGLE: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("sparse_jungle"));
/// `Biomes.BAMBOO_JUNGLE` — `register("bamboo_jungle")`.
pub static BAMBOO_JUNGLE: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("bamboo_jungle"));
/// `Biomes.BADLANDS` — `register("badlands")`.
pub static BADLANDS: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("badlands"));
/// `Biomes.ERODED_BADLANDS` — `register("eroded_badlands")`.
pub static ERODED_BADLANDS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("eroded_badlands"));
/// `Biomes.WOODED_BADLANDS` — `register("wooded_badlands")`.
pub static WOODED_BADLANDS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("wooded_badlands"));
/// `Biomes.MEADOW` — `register("meadow")`.
pub static MEADOW: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("meadow"));
/// `Biomes.CHERRY_GROVE` — `register("cherry_grove")`.
pub static CHERRY_GROVE: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("cherry_grove"));
/// `Biomes.GROVE` — `register("grove")`.
pub static GROVE: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("grove"));
/// `Biomes.SNOWY_SLOPES` — `register("snowy_slopes")`.
pub static SNOWY_SLOPES: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("snowy_slopes"));
/// `Biomes.FROZEN_PEAKS` — `register("frozen_peaks")`.
pub static FROZEN_PEAKS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("frozen_peaks"));
/// `Biomes.JAGGED_PEAKS` — `register("jagged_peaks")`.
pub static JAGGED_PEAKS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("jagged_peaks"));
/// `Biomes.STONY_PEAKS` — `register("stony_peaks")`.
pub static STONY_PEAKS: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("stony_peaks"));
/// `Biomes.RIVER` — `register("river")`.
pub static RIVER: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("river"));
/// `Biomes.FROZEN_RIVER` — `register("frozen_river")`.
pub static FROZEN_RIVER: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("frozen_river"));
/// `Biomes.BEACH` — `register("beach")`.
pub static BEACH: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("beach"));
/// `Biomes.SNOWY_BEACH` — `register("snowy_beach")`.
pub static SNOWY_BEACH: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("snowy_beach"));
/// `Biomes.STONY_SHORE` — `register("stony_shore")`.
pub static STONY_SHORE: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("stony_shore"));
/// `Biomes.WARM_OCEAN` — `register("warm_ocean")`.
pub static WARM_OCEAN: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("warm_ocean"));
/// `Biomes.LUKEWARM_OCEAN` — `register("lukewarm_ocean")`.
pub static LUKEWARM_OCEAN: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("lukewarm_ocean"));
/// `Biomes.DEEP_LUKEWARM_OCEAN` — `register("deep_lukewarm_ocean")`.
pub static DEEP_LUKEWARM_OCEAN: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("deep_lukewarm_ocean"));
/// `Biomes.OCEAN` — `register("ocean")`.
pub static OCEAN: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("ocean"));
/// `Biomes.DEEP_OCEAN` — `register("deep_ocean")`.
pub static DEEP_OCEAN: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("deep_ocean"));
/// `Biomes.COLD_OCEAN` — `register("cold_ocean")`.
pub static COLD_OCEAN: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("cold_ocean"));
/// `Biomes.DEEP_COLD_OCEAN` — `register("deep_cold_ocean")`.
pub static DEEP_COLD_OCEAN: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("deep_cold_ocean"));
/// `Biomes.FROZEN_OCEAN` — `register("frozen_ocean")`.
pub static FROZEN_OCEAN: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("frozen_ocean"));
/// `Biomes.DEEP_FROZEN_OCEAN` — `register("deep_frozen_ocean")`.
pub static DEEP_FROZEN_OCEAN: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("deep_frozen_ocean"));
/// `Biomes.MUSHROOM_FIELDS` — `register("mushroom_fields")`.
pub static MUSHROOM_FIELDS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("mushroom_fields"));
/// `Biomes.DRIPSTONE_CAVES` — `register("dripstone_caves")`.
pub static DRIPSTONE_CAVES: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("dripstone_caves"));
/// `Biomes.LUSH_CAVES` — `register("lush_caves")`.
pub static LUSH_CAVES: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("lush_caves"));
/// `Biomes.DEEP_DARK` — `register("deep_dark")`.
pub static DEEP_DARK: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("deep_dark"));
/// `Biomes.SULFUR_CAVES` — `register("sulfur_caves")`.
pub static SULFUR_CAVES: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("sulfur_caves"));
/// `Biomes.NETHER_WASTES` — `register("nether_wastes")`.
pub static NETHER_WASTES: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("nether_wastes"));
/// `Biomes.WARPED_FOREST` — `register("warped_forest")`.
pub static WARPED_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("warped_forest"));
/// `Biomes.CRIMSON_FOREST` — `register("crimson_forest")`.
pub static CRIMSON_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("crimson_forest"));
/// `Biomes.SOUL_SAND_VALLEY` — `register("soul_sand_valley")`.
pub static SOUL_SAND_VALLEY: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("soul_sand_valley"));
/// `Biomes.BASALT_DELTAS` — `register("basalt_deltas")`.
pub static BASALT_DELTAS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("basalt_deltas"));
/// `Biomes.THE_END` — `register("the_end")`.
pub static THE_END: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("the_end"));
/// `Biomes.END_HIGHLANDS` — `register("end_highlands")`.
pub static END_HIGHLANDS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("end_highlands"));
/// `Biomes.END_MIDLANDS` — `register("end_midlands")`.
pub static END_MIDLANDS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("end_midlands"));
/// `Biomes.SMALL_END_ISLANDS` — `register("small_end_islands")`.
pub static SMALL_END_ISLANDS: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("small_end_islands"));
/// `Biomes.END_BARRENS` — `register("end_barrens")`.
pub static END_BARRENS: LazyLock<ResourceKey<BiomeId>> = LazyLock::new(|| register("end_barrens"));

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::biomes::BIOME_BY_ID;

    /// `Biomes.java`'s 66 keys in declaration order.
    const ALL_KEYS: [(&LazyLock<ResourceKey<BiomeId>>, &str); 66] = [
        (&THE_VOID, "the_void"),
        (&PLAINS, "plains"),
        (&SUNFLOWER_PLAINS, "sunflower_plains"),
        (&SNOWY_PLAINS, "snowy_plains"),
        (&ICE_SPIKES, "ice_spikes"),
        (&DESERT, "desert"),
        (&SWAMP, "swamp"),
        (&MANGROVE_SWAMP, "mangrove_swamp"),
        (&FOREST, "forest"),
        (&FLOWER_FOREST, "flower_forest"),
        (&BIRCH_FOREST, "birch_forest"),
        (&DARK_FOREST, "dark_forest"),
        (&PALE_GARDEN, "pale_garden"),
        (&OLD_GROWTH_BIRCH_FOREST, "old_growth_birch_forest"),
        (&OLD_GROWTH_PINE_TAIGA, "old_growth_pine_taiga"),
        (&OLD_GROWTH_SPRUCE_TAIGA, "old_growth_spruce_taiga"),
        (&TAIGA, "taiga"),
        (&SNOWY_TAIGA, "snowy_taiga"),
        (&SAVANNA, "savanna"),
        (&SAVANNA_PLATEAU, "savanna_plateau"),
        (&WINDSWEPT_HILLS, "windswept_hills"),
        (&WINDSWEPT_GRAVELLY_HILLS, "windswept_gravelly_hills"),
        (&WINDSWEPT_FOREST, "windswept_forest"),
        (&WINDSWEPT_SAVANNA, "windswept_savanna"),
        (&JUNGLE, "jungle"),
        (&SPARSE_JUNGLE, "sparse_jungle"),
        (&BAMBOO_JUNGLE, "bamboo_jungle"),
        (&BADLANDS, "badlands"),
        (&ERODED_BADLANDS, "eroded_badlands"),
        (&WOODED_BADLANDS, "wooded_badlands"),
        (&MEADOW, "meadow"),
        (&CHERRY_GROVE, "cherry_grove"),
        (&GROVE, "grove"),
        (&SNOWY_SLOPES, "snowy_slopes"),
        (&FROZEN_PEAKS, "frozen_peaks"),
        (&JAGGED_PEAKS, "jagged_peaks"),
        (&STONY_PEAKS, "stony_peaks"),
        (&RIVER, "river"),
        (&FROZEN_RIVER, "frozen_river"),
        (&BEACH, "beach"),
        (&SNOWY_BEACH, "snowy_beach"),
        (&STONY_SHORE, "stony_shore"),
        (&WARM_OCEAN, "warm_ocean"),
        (&LUKEWARM_OCEAN, "lukewarm_ocean"),
        (&DEEP_LUKEWARM_OCEAN, "deep_lukewarm_ocean"),
        (&OCEAN, "ocean"),
        (&DEEP_OCEAN, "deep_ocean"),
        (&COLD_OCEAN, "cold_ocean"),
        (&DEEP_COLD_OCEAN, "deep_cold_ocean"),
        (&FROZEN_OCEAN, "frozen_ocean"),
        (&DEEP_FROZEN_OCEAN, "deep_frozen_ocean"),
        (&MUSHROOM_FIELDS, "mushroom_fields"),
        (&DRIPSTONE_CAVES, "dripstone_caves"),
        (&LUSH_CAVES, "lush_caves"),
        (&DEEP_DARK, "deep_dark"),
        (&SULFUR_CAVES, "sulfur_caves"),
        (&NETHER_WASTES, "nether_wastes"),
        (&WARPED_FOREST, "warped_forest"),
        (&CRIMSON_FOREST, "crimson_forest"),
        (&SOUL_SAND_VALLEY, "soul_sand_valley"),
        (&BASALT_DELTAS, "basalt_deltas"),
        (&THE_END, "the_end"),
        (&END_HIGHLANDS, "end_highlands"),
        (&END_MIDLANDS, "end_midlands"),
        (&SMALL_END_ISLANDS, "small_end_islands"),
        (&END_BARRENS, "end_barrens"),
    ];

    #[test]
    fn keys_use_the_worldgen_biome_registry_with_exact_locations() {
        for (key, name) in ALL_KEYS {
            let key = &**key;
            assert_eq!(key.registry().to_string(), "minecraft:worldgen/biome");
            assert_eq!(key.identifier().to_string(), format!("minecraft:{name}"));
        }
    }

    #[test]
    fn the_hub_covers_exactly_the_generated_biome_table() {
        // The 66 keys == the generated `BIOME_BY_ID` 66 names — the hub is the
        // complete `Biomes.java` set, with no extra or missing entries.
        let mut hub: Vec<String> = ALL_KEYS
            .iter()
            .map(|(k, _)| k.identifier().to_string())
            .collect();
        hub.sort();
        let mut generated: Vec<String> = BIOME_BY_ID.iter().map(|n| n.to_string()).collect();
        generated.sort();
        assert_eq!(hub, generated);
        assert_eq!(hub.len(), 66);
    }

    #[test]
    fn register_from_full_name_matches_the_hub_key() {
        // The generated preset tables carry full `minecraft:<path>` identifiers;
        // resolving them yields the value-equal hub key.
        assert_eq!(
            register_from_full_name("minecraft:mushroom_fields"),
            *MUSHROOM_FIELDS
        );
        assert_eq!(register_from_full_name("minecraft:deep_dark"), *DEEP_DARK);
        assert_eq!(register_from_full_name("minecraft:plains"), *PLAINS);
    }
}

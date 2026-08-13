//! STUB(mc.world.level.biome.data) — `net.minecraft.world.level.biome.Biomes`.
//!
//! `Biomes.java` is the generated registry hub of biome `ResourceKey`s (owned
//! by the `mc.world.level.biome.data` manifest unit). This unit's `the_end`
//! source and the `multi_noise` NETHER preset bridge by these keys, so the
//! ten keys they touch are declared here as `ResourceKey<BiomeId>` id-model
//! constants (the merged `biome.core` carries the biome as the pure
//! [`rivet_registry::biome_id::BiomeId`] handle). When the `.data` unit lands,
//! the full generated hub replaces this stub and the source unit's references
//! move over unchanged.

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

/// `Biomes.NETHER_WASTES` — `register("nether_wastes")`.
pub static NETHER_WASTES: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("nether_wastes"));
/// `Biomes.SOUL_SAND_VALLEY` — `register("soul_sand_valley")`.
pub static SOUL_SAND_VALLEY: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("soul_sand_valley"));
/// `Biomes.CRIMSON_FOREST` — `register("crimson_forest")`.
pub static CRIMSON_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("crimson_forest"));
/// `Biomes.WARPED_FOREST` — `register("warped_forest")`.
pub static WARPED_FOREST: LazyLock<ResourceKey<BiomeId>> =
    LazyLock::new(|| register("warped_forest"));
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

    #[test]
    fn keys_use_the_worldgen_biome_registry_with_exact_locations() {
        let cases: [(&LazyLock<ResourceKey<BiomeId>>, &str); 10] = [
            (&NETHER_WASTES, "nether_wastes"),
            (&SOUL_SAND_VALLEY, "soul_sand_valley"),
            (&CRIMSON_FOREST, "crimson_forest"),
            (&WARPED_FOREST, "warped_forest"),
            (&BASALT_DELTAS, "basalt_deltas"),
            (&THE_END, "the_end"),
            (&END_HIGHLANDS, "end_highlands"),
            (&END_MIDLANDS, "end_midlands"),
            (&SMALL_END_ISLANDS, "small_end_islands"),
            (&END_BARRENS, "end_barrens"),
        ];
        for (key, name) in cases {
            let key = &**key;
            assert_eq!(key.registry().to_string(), "minecraft:worldgen/biome");
            assert_eq!(key.identifier().to_string(), format!("minecraft:{name}"));
        }
    }
}

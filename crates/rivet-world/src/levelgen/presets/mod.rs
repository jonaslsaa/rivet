//! Port of `net.minecraft.world.level.levelgen.presets` (package, 26.2).
//!
//! The world presets: `WorldPreset` (the dimension-stem map value, its
//! `"dimensions"` codec with the missing-overworld validation, and the
//! `worldgen/world_preset` registry file codec) and `WorldPresets` (the seven
//! built-in preset keys, the `fromSettings`/`createNormalWorldDimensions`/
//! `getNormalOverworld`/`createTestWorldDimensions` helpers, and the bootstrap
//! that registers the presets from the dimension types, noise settings,
//! biomes, placed features, structure sets, and multi-noise parameter lists).
//! The `bootstrap` and `fromSettings` bodies defer with RivetTodo(#185) (the
//! noise-based stems need `NoiseBasedChunkGenerator` to implement
//! `ChunkGenerator`, and `fromSettings` needs a generator type-downcast); the
//! seven keys and the three `HolderLookup.Provider` helpers are functional —
//! see `world_presets`.
//!
//! The `LevelStem`/`WorldDimensions` values are the
//! `mc.world.level.levelgen.settings` unit's shells, the biome reference rides
//! the `biome.core` id-model (`Registry<BiomeId>`), and the flat dependency
//! (`FlatLevelGeneratorSettings`) is the merged `mc.world.level.levelgen.flat`
//! unit. The cross-unit seam below declares the `WORLD_PRESET` registry key
//! (the value's own registry) and the `BuiltinDimensionTypes` keys (the
//! pending `mc.world.level.dimension` unit, `STUB(mc.world.level.dimension)`).

pub mod world_preset;
pub mod world_presets;

use rivet_registry::Identifier;
use rivet_registry::Registry;
use rivet_registry::ResourceKey;
use rivet_registry::registries;
use std::sync::LazyLock;

use crate::levelgen::presets::world_preset::WorldPreset;

/// `Registries.WORLD_PRESET`
/// (`ResourceKey.createRegistryKey("minecraft:worldgen/world_preset")`) — the
/// registry the preset codec and the seven built-in preset registrations live
/// in (the `flat` unit's `FLAT_LEVEL_GENERATOR_PRESET` precedent).
pub static WORLD_PRESET: LazyLock<ResourceKey<Registry<WorldPreset>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/world_preset"))
});

/// STUB(mc.world.level.dimension): `BuiltinDimensionTypes` keys — the built-in
/// `DIMENSION_TYPE` registry entries the presets bootstrap names. Pending
/// `mc.world.level.dimension` owns the real keys; only the three the bootstrap
/// uses (`BuiltinDimensionTypes.OVERWORLD`/`NETHER`/`END`) are declared here —
/// `OVERWORLD_CAVES` is deferred until a consumer lands, since the bootstrap
/// never references it.
pub mod builtin_dimension_types {
    use super::*;

    /// `BuiltinDimensionTypes.OVERWORLD` — `register("overworld")`.
    pub static OVERWORLD: LazyLock<ResourceKey<registries::DimensionType>> = LazyLock::new(|| {
        ResourceKey::create(
            &registries::DIMENSION_TYPE,
            Identifier::with_default_namespace("overworld"),
        )
    });

    /// `BuiltinDimensionTypes.NETHER` — `register("the_nether")`.
    pub static NETHER: LazyLock<ResourceKey<registries::DimensionType>> = LazyLock::new(|| {
        ResourceKey::create(
            &registries::DIMENSION_TYPE,
            Identifier::with_default_namespace("the_nether"),
        )
    });

    /// `BuiltinDimensionTypes.END` — `register("the_end")`.
    pub static END: LazyLock<ResourceKey<registries::DimensionType>> = LazyLock::new(|| {
        ResourceKey::create(
            &registries::DIMENSION_TYPE,
            Identifier::with_default_namespace("the_end"),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_preset_registry_key_matches_java() {
        assert_eq!(
            WORLD_PRESET.identifier().to_string(),
            "minecraft:worldgen/world_preset"
        );
    }

    #[test]
    fn builtin_dimension_types_match_java() {
        assert_eq!(
            builtin_dimension_types::OVERWORLD.identifier().to_string(),
            "minecraft:overworld"
        );
        assert_eq!(
            builtin_dimension_types::NETHER.identifier().to_string(),
            "minecraft:the_nether"
        );
        assert_eq!(
            builtin_dimension_types::END.identifier().to_string(),
            "minecraft:the_end"
        );
        for key in [
            &*builtin_dimension_types::OVERWORLD,
            &*builtin_dimension_types::NETHER,
            &*builtin_dimension_types::END,
        ] {
            assert!(key.is_for(&registries::DIMENSION_TYPE));
        }
    }
}

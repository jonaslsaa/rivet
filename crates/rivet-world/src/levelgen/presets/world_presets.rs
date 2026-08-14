//! Port of `net.minecraft.world.level.levelgen.presets.WorldPresets` (class,
//! 26.2) — the `mc.world.level.levelgen.presets` unit.
//!
//! The seven built-in `worldgen/world_preset` keys and the `bootstrap` that
//! registers them, plus the `fromSettings`/`createNormalWorldDimensions`/
//! `getNormalOverworld`/`createTestWorldDimensions` helpers. The preset VALUE
//! (the dimension-stem map) is [`world_preset`](crate::levelgen::presets::world_preset).
//!
//! ### The deferred bootstrap seams
//!
//! Java's private `WorldPresets.Bootstrap` builds the stems from
//! `new NoiseBasedChunkGenerator(biomeSource, noiseSettings)`. The noisegen
//! value shell's `NoiseBasedChunkGenerator` does NOT implement the
//! `ChunkGenerator` trait — the owning `mc.world.level.chunk.generator`
//! realization defers that (RivetTodo(#185); the shell's module doc is
//! explicit: "the shell does not implement this trait") — while `LevelStem`
//! needs an `Arc<dyn ChunkGenerator>`. So the nether/end stems and the
//! NORMAL/LARGE_BIOMES/AMPLIFIED/SINGLE_BIOME_SURFACE overworld stems (every
//! noise-based stem the Bootstrap builds) cannot be constructed without
//! `impl ChunkGenerator for NoiseBasedChunkGenerator`, an API invention the
//! #185 owning unit owns. The multi-noise preset resolution compounds it:
//! Java reads `multiNoiseBiomeSourceParameterLists.getOrThrow(NETHER).value()`
//! — a holder-to-value resolution that needs `&dyn HolderLookup`, while
//! `BootstrapContext::lookup` returns `&dyn HolderGetter` (no `value()`); the
//! flat unit's caller-supplies-resolved-values pattern is the alternative.
//! `bootstrap` is therefore an explicit seam that names both blockers.
//!
//! `fromSettings` needs `levelStem.generator() instanceof FlatLevelSource /
//! DebugLevelSource / NoiseBasedChunkGenerator` — a concrete-type switch on
//! the `&dyn ChunkGenerator` the trait returns. The `ChunkGenerator` trait has
//! no `&dyn Any` bridge (the #185 owning realization provides it, the
//! `BiomeSource::as_any` precedent), so the switch fails explicitly — the same
//! seam as `WorldDimensions::isDebug`.
//!
//! ### What is ported in full
//!
//! The seven keys and the three `HolderLookup.Provider` helpers
//! ([`create_normal_world_dimensions`], [`get_normal_overworld`],
//! [`create_test_world_dimensions`]) are functional: they resolve the frozen
//! `WORLD_PRESET` registry through the `RegistryAccess` (Rust's
//! `HolderLookup.Provider`), `getOrThrow` the builtin preset, and read its
//! value — no unported surface.

use crate::data::worldgen::bootstrap_context::BootstrapContext;
use crate::levelgen::presets::WORLD_PRESET;
use crate::levelgen::presets::world_preset::WorldPreset;
use crate::levelgen::settings::level_stem::LevelStem;
use crate::levelgen::settings::world_dimensions::WorldDimensions;
use rivet_registry::Identifier;
use rivet_registry::RegistryAccess;
use rivet_registry::ResourceKey;
use rivet_registry::holder_lookup::HolderGetter;
use std::sync::LazyLock;

/// `WorldPresets.NORMAL` — `register("normal")`.
pub static NORMAL: LazyLock<ResourceKey<WorldPreset>> = LazyLock::new(|| register("normal"));
/// `WorldPresets.FLAT` — `register("flat")`.
pub static FLAT: LazyLock<ResourceKey<WorldPreset>> = LazyLock::new(|| register("flat"));
/// `WorldPresets.FLAT_ALL_DIMENSIONS` — `register("flat_all_dimensions")`.
pub static FLAT_ALL_DIMENSIONS: LazyLock<ResourceKey<WorldPreset>> =
    LazyLock::new(|| register("flat_all_dimensions"));
/// `WorldPresets.LARGE_BIOMES` — `register("large_biomes")`.
pub static LARGE_BIOMES: LazyLock<ResourceKey<WorldPreset>> =
    LazyLock::new(|| register("large_biomes"));
/// `WorldPresets.AMPLIFIED` — `register("amplified")`.
pub static AMPLIFIED: LazyLock<ResourceKey<WorldPreset>> = LazyLock::new(|| register("amplified"));
/// `WorldPresets.SINGLE_BIOME_SURFACE` — `register("single_biome_surface")`.
pub static SINGLE_BIOME_SURFACE: LazyLock<ResourceKey<WorldPreset>> =
    LazyLock::new(|| register("single_biome_surface"));
/// `WorldPresets.DEBUG` — `register("debug_all_block_states")`.
pub static DEBUG: LazyLock<ResourceKey<WorldPreset>> =
    LazyLock::new(|| register("debug_all_block_states"));

/// `register(String)` — `ResourceKey.create(Registries.WORLD_PRESET,
/// Identifier.withDefaultNamespace(name))`.
fn register(name: &str) -> ResourceKey<WorldPreset> {
    ResourceKey::create(&*WORLD_PRESET, Identifier::with_default_namespace(name))
}

/// `WorldPresets.bootstrap(BootstrapContext<WorldPreset>)` — the seven preset
/// registrations.
///
/// The `new WorldPresets.Bootstrap(context).bootstrap()` body is a seam (see
/// the module docs): every noise-based stem (NORMAL/LARGE_BIOMES/AMPLIFIED/
/// SINGLE_BIOME_SURFACE overworlds and the shared nether/end stems) needs
/// `NoiseBasedChunkGenerator` to be an `Arc<dyn ChunkGenerator>`, which the
/// #185 owning realization has not provided, and the multi-noise preset values
/// need a `&dyn HolderLookup` resolution `BootstrapContext::lookup` cannot
/// offer. The FLAT/FLAT_ALL_DIMENSIONS/DEBUG registrations are representable
/// alone but the FLAT/DEBUG presets reuse the noise-based nether/end stems via
/// `createPresetWithCustomOverworld`, so no partial registration is faithful.
pub fn bootstrap(_context: &mut impl BootstrapContext<WorldPreset>) {
    panic!(
        "WorldPresets.bootstrap is not implemented (RivetTodo(#185)): the noise-based stems need NoiseBasedChunkGenerator to implement ChunkGenerator, and the multi-noise presets need HolderLookup value resolution (BootstrapContext::lookup returns HolderGetter)"
    )
}

/// `fromSettings(WorldDimensions)` — `Optional.of(FLAT)` for a flat overworld,
/// `DEBUG` for a debug overworld, `NORMAL` for a noise-based overworld, else
/// empty.
///
/// The concrete-type switch on `levelStem.generator()` needs a `&dyn Any`
/// bridge on the `ChunkGenerator` trait; the #185 owning realization provides
/// it (the `BiomeSource::as_any` precedent). Fails explicitly rather than
/// fabricate a result — the same seam as `WorldDimensions::isDebug`.
pub fn from_settings(_dimensions: &WorldDimensions) -> Option<ResourceKey<WorldPreset>> {
    panic!(
        "WorldPresets.fromSettings is not implemented (RivetTodo(#185)): needs a ChunkGenerator type-downcast (as_any) to dispatch FlatLevelSource/DebugLevelSource/NoiseBasedChunkGenerator"
    )
}

/// `createNormalWorldDimensions(HolderLookup.Provider)` — resolve the NORMAL
/// preset from the access's `WORLD_PRESET` registry and build its dimensions.
pub fn create_normal_world_dimensions(access: &RegistryAccess) -> WorldDimensions {
    let lookup = access
        .lookup(&*WORLD_PRESET)
        .expect("WORLD_PRESET registry present in the access");
    lookup
        .get_or_throw(&*NORMAL)
        .value(lookup)
        .create_world_dimensions()
}

/// `getNormalOverworld(HolderLookup.Provider)` — the NORMAL preset's overworld
/// stem (`overworld().orElseThrow()`). Returns the cloned stem value (the
/// value-shell ownership model; Java hands out the referenced `LevelStem`).
pub fn get_normal_overworld(access: &RegistryAccess) -> LevelStem {
    let lookup = access
        .lookup(&*WORLD_PRESET)
        .expect("WORLD_PRESET registry present in the access");
    lookup
        .get_or_throw(&*NORMAL)
        .value(lookup)
        .overworld()
        .expect("the normal preset must contain an overworld stem")
        .clone()
}

/// `createTestWorldDimensions(HolderLookup.Provider)` — resolve the
/// FLAT_ALL_DIMENSIONS preset from the access's `WORLD_PRESET` registry and
/// build its dimensions.
pub fn create_test_world_dimensions(access: &RegistryAccess) -> WorldDimensions {
    let lookup = access
        .lookup(&*WORLD_PRESET)
        .expect("WORLD_PRESET registry present in the access");
    lookup
        .get_or_throw(&*FLAT_ALL_DIMENSIONS)
        .value(lookup)
        .create_world_dimensions()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::settings::debug_level_source::DebugLevelSource;
    use crate::levelgen::settings::level_stem;
    use rivet_registry::RegistrationInfo;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::holder::Holder;
    use rivet_registry::registries;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn debug_stem() -> LevelStem {
        let source = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        LevelStem::new(Holder::direct(registries::DimensionType), Arc::new(source))
    }

    fn stem_map() -> HashMap<ResourceKey<registries::LevelStem>, LevelStem> {
        HashMap::from([
            ((*level_stem::OVERWORLD).clone(), debug_stem()),
            ((*level_stem::NETHER).clone(), debug_stem()),
            ((*level_stem::END).clone(), debug_stem()),
        ])
    }

    /// A `RegistryAccess` with a frozen `WORLD_PRESET` registry holding the
    /// NORMAL and FLAT_ALL_DIMENSIONS presets (each the three debug stems) —
    /// the `HolderLookup.Provider` the `create*` helpers read.
    fn access_with_presets() -> RegistryAccess {
        let mut builder: RegistryBuilder<WorldPreset> = RegistryBuilder::new(&*WORLD_PRESET);
        builder.register(
            &*NORMAL,
            Arc::new(WorldPreset::new(stem_map())),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &*FLAT_ALL_DIMENSIONS,
            Arc::new(WorldPreset::new(stem_map())),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_single_registry((*WORLD_PRESET).clone(), registry)
    }

    #[test]
    fn the_seven_preset_keys_match_java() {
        let cases: &[(&ResourceKey<WorldPreset>, &str)] = &[
            (&*NORMAL, "minecraft:normal"),
            (&*FLAT, "minecraft:flat"),
            (&*FLAT_ALL_DIMENSIONS, "minecraft:flat_all_dimensions"),
            (&*LARGE_BIOMES, "minecraft:large_biomes"),
            (&*AMPLIFIED, "minecraft:amplified"),
            (&*SINGLE_BIOME_SURFACE, "minecraft:single_biome_surface"),
            (&*DEBUG, "minecraft:debug_all_block_states"),
        ];
        for (key, expected) in cases {
            assert_eq!(key.identifier().to_string(), *expected, "{key}");
            assert!(key.is_for(&*WORLD_PRESET), "{key}");
        }
    }

    #[test]
    fn create_normal_world_dimensions_resolves_the_normal_preset_stems() {
        let access = access_with_presets();
        let dimensions = create_normal_world_dimensions(&access);
        assert!(dimensions.get(&level_stem::OVERWORLD).is_some());
        assert!(dimensions.get(&level_stem::NETHER).is_some());
        assert!(dimensions.get(&level_stem::END).is_some());
    }

    #[test]
    fn get_normal_overworld_is_the_normal_presets_overworld() {
        let access = access_with_presets();
        let lookup = access.lookup(&*WORLD_PRESET).expect("preset registry");
        let overworld = get_normal_overworld(&access);
        // The returned stem equals the NORMAL preset's own overworld value.
        let normal = lookup.get_or_throw(&*NORMAL);
        let expected = normal.value(lookup).overworld().expect("overworld stem");
        assert_eq!(overworld.ty, expected.ty);
    }

    #[test]
    fn create_test_world_dimensions_resolves_the_flat_all_dimensions_preset() {
        let access = access_with_presets();
        let dimensions = create_test_world_dimensions(&access);
        assert!(dimensions.get(&level_stem::OVERWORLD).is_some());
        assert!(dimensions.get(&level_stem::NETHER).is_some());
        assert!(dimensions.get(&level_stem::END).is_some());
    }
}

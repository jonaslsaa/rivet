//! Port of `net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorPresets`
//! (class, 26.2).
//!
//! The nine built-in `worldgen/flat_level_generator_preset` registrations
//! bootstrapped by `FlatLevelGeneratorPresets.bootstrap` — each a display item,
//! a biome, a structure-set holder set, decoration/lakes flags, and a layer
//! column (reversed: the Java registration appends layers in reverse so the
//! first entry ends up at the bottom of the column).
//!
//! `TEST_WORLD` is declared but never registered (Java keeps the key for the
//! `test_world` preset that the debug generator references). The cross-unit
//! structure-set and lake-placement keys are the STUBs in the parent module
//! (`structure.framework`, `data.worldgen.placement`), and the two blocks not
//! in the curated [`Blocks`](crate::block::blocks::Blocks) table
//! (`snow`, `short_grass`) resolve by name through the generated id table.

use crate::biome::biomes;
use crate::block::Block;
use crate::data::worldgen::bootstrap_context::BootstrapContext;
use crate::levelgen::flat::flat_layer_info::FlatLayerInfo;
use crate::levelgen::flat::flat_level_generator_preset::FlatLevelGeneratorPreset;
use crate::levelgen::flat::flat_level_generator_settings::FlatLevelGeneratorSettings;
use crate::levelgen::flat::{
    FLAT_LEVEL_GENERATOR_PRESET, ItemLike, LAKE_LAVA_SURFACE, LAKE_LAVA_UNDERGROUND,
    STRUCTURE_DESERT_PYRAMIDS, STRUCTURE_IGLOOS, STRUCTURE_MINESHAFTS, STRUCTURE_OCEAN_MONUMENTS,
    STRUCTURE_OCEAN_RUINS, STRUCTURE_PILLAGER_OUTPOSTS, STRUCTURE_RUINED_PORTALS,
    STRUCTURE_SHIPWRECKS, STRUCTURE_STRONGHOLDS, STRUCTURE_VILLAGES, StructureSet,
};
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::holder_set::HolderSet;
use std::sync::LazyLock;

/// `FlatLevelGeneratorPresets.CLASSIC_FLAT`.
pub static CLASSIC_FLAT: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("classic_flat"));
/// `FlatLevelGeneratorPresets.TUNNELERS_DREAM`.
pub static TUNNELERS_DREAM: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("tunnelers_dream"));
/// `FlatLevelGeneratorPresets.WATER_WORLD`.
pub static WATER_WORLD: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("water_world"));
/// `FlatLevelGeneratorPresets.OVERWORLD`.
pub static OVERWORLD: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("overworld"));
/// `FlatLevelGeneratorPresets.SNOWY_KINGDOM`.
pub static SNOWY_KINGDOM: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("snowy_kingdom"));
/// `FlatLevelGeneratorPresets.BOTTOMLESS_PIT`.
pub static BOTTOMLESS_PIT: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("bottomless_pit"));
/// `FlatLevelGeneratorPresets.DESERT`.
pub static DESERT: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("desert"));
/// `FlatLevelGeneratorPresets.REDSTONE_READY`.
pub static REDSTONE_READY: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("redstone_ready"));
/// `FlatLevelGeneratorPresets.TEST_WORLD` — declared, never registered (Java
/// keeps the key for the debug generator's `test_world` preset).
pub static TEST_WORLD: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("test_world"));
/// `FlatLevelGeneratorPresets.THE_VOID`.
pub static THE_VOID: LazyLock<ResourceKey<FlatLevelGeneratorPreset>> =
    LazyLock::new(|| register("the_void"));

/// `register(String)` — `ResourceKey.create(Registries.FLAT_LEVEL_GENERATOR_PRESET,
/// Identifier.withDefaultNamespace(name))`.
fn register(name: &str) -> ResourceKey<FlatLevelGeneratorPreset> {
    ResourceKey::create(
        &*FLAT_LEVEL_GENERATOR_PRESET,
        Identifier::with_default_namespace(name),
    )
}

/// `bootstrap(BootstrapContext<FlatLevelGeneratorPreset>)` — the nine
/// registrations in Java order. The caller supplies the block registry's real
/// `RegistryId` (the block-registry binding Java's `Block.builtInRegistryHolder()`
/// bakes into each layer holder; the id-model `Block` handle carries none).
pub fn bootstrap(
    context: &mut impl BootstrapContext<FlatLevelGeneratorPreset>,
    block_registry: rivet_registry::holder::RegistryId,
) {
    bootstrap_one(
        context,
        &CLASSIC_FLAT,
        ItemLike::Block(crate::block::blocks::Blocks::GRASS_BLOCK),
        &biomes::PLAINS,
        &[&*STRUCTURE_VILLAGES],
        false,
        false,
        &[
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::GRASS_BLOCK, block_registry),
            FlatLayerInfo::from_block(2, crate::block::blocks::Blocks::DIRT, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &TUNNELERS_DREAM,
        ItemLike::Block(crate::block::blocks::Blocks::STONE),
        &biomes::WINDSWEPT_HILLS,
        &[&*STRUCTURE_MINESHAFTS, &*STRUCTURE_STRONGHOLDS],
        true,
        false,
        &[
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::GRASS_BLOCK, block_registry),
            FlatLayerInfo::from_block(5, crate::block::blocks::Blocks::DIRT, block_registry),
            FlatLayerInfo::from_block(230, crate::block::blocks::Blocks::STONE, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &WATER_WORLD,
        ItemLike::Item("minecraft:water_bucket"),
        &biomes::DEEP_OCEAN,
        &[
            &*STRUCTURE_OCEAN_RUINS,
            &*STRUCTURE_SHIPWRECKS,
            &*STRUCTURE_OCEAN_MONUMENTS,
        ],
        false,
        false,
        &[
            FlatLayerInfo::from_block(90, crate::block::blocks::Blocks::WATER, block_registry),
            FlatLayerInfo::from_block(5, crate::block::blocks::Blocks::GRAVEL, block_registry),
            FlatLayerInfo::from_block(5, crate::block::blocks::Blocks::DIRT, block_registry),
            FlatLayerInfo::from_block(5, crate::block::blocks::Blocks::STONE, block_registry),
            FlatLayerInfo::from_block(64, crate::block::blocks::Blocks::DEEPSLATE, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &OVERWORLD,
        ItemLike::Block(Block::from_name("minecraft:short_grass").expect("short_grass")),
        &biomes::PLAINS,
        &[
            &*STRUCTURE_VILLAGES,
            &*STRUCTURE_MINESHAFTS,
            &*STRUCTURE_PILLAGER_OUTPOSTS,
            &*STRUCTURE_RUINED_PORTALS,
            &*STRUCTURE_STRONGHOLDS,
        ],
        true,
        true,
        &[
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::GRASS_BLOCK, block_registry),
            FlatLayerInfo::from_block(3, crate::block::blocks::Blocks::DIRT, block_registry),
            FlatLayerInfo::from_block(59, crate::block::blocks::Blocks::STONE, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &SNOWY_KINGDOM,
        ItemLike::Block(Block::from_name("minecraft:snow").expect("snow")),
        &biomes::SNOWY_PLAINS,
        &[&*STRUCTURE_VILLAGES, &*STRUCTURE_IGLOOS],
        false,
        false,
        &[
            FlatLayerInfo::from_block(
                1,
                Block::from_name("minecraft:snow").expect("snow"),
                block_registry,
            ),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::GRASS_BLOCK, block_registry),
            FlatLayerInfo::from_block(3, crate::block::blocks::Blocks::DIRT, block_registry),
            FlatLayerInfo::from_block(59, crate::block::blocks::Blocks::STONE, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &BOTTOMLESS_PIT,
        ItemLike::Item("minecraft:feather"),
        &biomes::PLAINS,
        &[&*STRUCTURE_VILLAGES],
        false,
        false,
        &[
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::GRASS_BLOCK, block_registry),
            FlatLayerInfo::from_block(3, crate::block::blocks::Blocks::DIRT, block_registry),
            FlatLayerInfo::from_block(2, crate::block::blocks::Blocks::COBBLESTONE, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &DESERT,
        ItemLike::Block(crate::block::blocks::Blocks::SAND),
        &biomes::DESERT,
        &[
            &*STRUCTURE_VILLAGES,
            &*STRUCTURE_DESERT_PYRAMIDS,
            &*STRUCTURE_MINESHAFTS,
            &*STRUCTURE_STRONGHOLDS,
        ],
        true,
        false,
        &[
            FlatLayerInfo::from_block(8, crate::block::blocks::Blocks::SAND, block_registry),
            FlatLayerInfo::from_block(52, crate::block::blocks::Blocks::SANDSTONE, block_registry),
            FlatLayerInfo::from_block(3, crate::block::blocks::Blocks::STONE, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &REDSTONE_READY,
        ItemLike::Item("minecraft:redstone"),
        &biomes::DESERT,
        &[],
        false,
        false,
        &[
            FlatLayerInfo::from_block(116, crate::block::blocks::Blocks::SANDSTONE, block_registry),
            FlatLayerInfo::from_block(3, crate::block::blocks::Blocks::STONE, block_registry),
            FlatLayerInfo::from_block(1, crate::block::blocks::Blocks::BEDROCK, block_registry),
        ],
    );
    bootstrap_one(
        context,
        &THE_VOID,
        ItemLike::Block(crate::block::blocks::Blocks::BARRIER),
        &biomes::THE_VOID,
        &[],
        true,
        false,
        &[FlatLayerInfo::from_block(
            1,
            crate::block::blocks::Blocks::AIR,
            block_registry,
        )],
    );
}

/// `Bootstrap.register(...)` — resolves the structure/biome/lake getters,
/// builds the direct structure holder set, constructs the settings (decoration
/// and lakes flags first, then the reversed layers), and registers the preset
/// with its display item.
///
/// The Java `register` takes the layers in declaration order and appends them
/// to `layersInfo` in reverse (so the last entry is the top of the column); the
/// port's `bootstrap_one` receives the same declaration order and reverses it
/// internally.
#[allow(clippy::too_many_arguments)] // Java's private `Bootstrap.register` 7 params + the context.
fn bootstrap_one(
    context: &mut impl BootstrapContext<FlatLevelGeneratorPreset>,
    key: &ResourceKey<FlatLevelGeneratorPreset>,
    icon: ItemLike,
    biome: &ResourceKey<BiomeId>,
    structures: &[&ResourceKey<StructureSet>],
    decoration: bool,
    add_lakes: bool,
    layers: &[FlatLayerInfo],
) {
    // Java: `context.lookup(Registries.STRUCTURE_SET)` /
    // `context.lookup(Registries.PLACED_FEATURE)` / `context.lookup(Registries.BIOME)`.
    let structure_sets = context
        .lookup::<StructureSet>(&*super::STRUCTURE_SET)
        .expect("STRUCTURE_SET registry");
    let placed_features = context
        .lookup::<PlacedFeature>(&*crate::biome::biome_generation_settings::PLACED_FEATURE)
        .expect("PLACED_FEATURE registry");
    let biomes_getter = context
        .lookup::<BiomeId>(&*rivet_registry::registries::BIOME)
        .expect("BIOME registry");

    let structures_holder = HolderSet::direct(
        structures
            .iter()
            .map(|s| structure_sets.get_or_throw(s))
            .collect(),
    );
    let mut generator = FlatLevelGeneratorSettings::new(
        Some(structures_holder),
        biomes_getter.get_or_throw(biome),
        FlatLevelGeneratorSettings::create_lakes_list(
            placed_features.get_or_throw(&*LAKE_LAVA_UNDERGROUND),
            placed_features.get_or_throw(&*LAKE_LAVA_SURFACE),
        ),
    );
    if decoration {
        generator.set_decoration();
    }
    if add_lakes {
        generator.set_add_lakes();
    }
    for layer in layers.iter().rev() {
        generator.get_layers_info_mut().push(layer.clone());
    }

    context.register(
        key,
        FlatLevelGeneratorPreset {
            display_item: icon.as_item().built_in_registry_holder(),
            settings: generator,
        },
        rivet_serialization::Lifecycle::stable(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_generation_settings::PLACED_FEATURE;
    use crate::block::blocks::Blocks;
    use crate::data::worldgen::bootstrap_context::{RecordedRegistration, RecordingContext};
    use crate::levelgen::flat::STRUCTURE_SET;
    use rivet_registry::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder::{Holder, RegistryId};
    use rivet_registry::holder_lookup::HolderLookup;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registries;
    use rivet_registry::root::AnyBox;
    use std::sync::Arc;

    type PresetKey = ResourceKey<FlatLevelGeneratorPreset>;

    /// One Java `register` row: (preset key, biome key, structure-set keys,
    /// display-item name, decoration, addLakes).
    type PresetCase<'a> = (
        &'a PresetKey,
        &'a ResourceKey<BiomeId>,
        &'a [&'a ResourceKey<StructureSet>],
        &'a str,
        bool,
        bool,
    );

    /// The registries `FlatLevelGeneratorPresets.bootstrap` resolves through:
    /// biome (the six preset biomes), block (empty — only its `RegistryId` is
    /// consumed; layer references resolve through the generated block table),
    /// structure set (the ten builtin keys), and placed feature (the two lava
    /// lakes).
    fn access() -> RegistryAccess {
        let mut biomes_reg = RegistryBuilder::new(&*registries::BIOME);
        for key in [
            &*biomes::PLAINS,
            &*biomes::WINDSWEPT_HILLS,
            &*biomes::DEEP_OCEAN,
            &*biomes::SNOWY_PLAINS,
            &*biomes::DESERT,
            &*biomes::THE_VOID,
        ] {
            biomes_reg.register(
                &ResourceKey::create(&*registries::BIOME, key.identifier().clone()),
                Arc::new(BiomeId::from_id(0)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let biome_registry = biomes_reg.freeze();

        let block_registry =
            RegistryBuilder::<registries::BlockType>::new(&*registries::BLOCK).freeze();

        let mut structure_reg = RegistryBuilder::new(&*STRUCTURE_SET);
        for key in [
            &*STRUCTURE_STRONGHOLDS,
            &*STRUCTURE_VILLAGES,
            &*STRUCTURE_MINESHAFTS,
            &*STRUCTURE_PILLAGER_OUTPOSTS,
            &*STRUCTURE_RUINED_PORTALS,
            &*STRUCTURE_IGLOOS,
            &*STRUCTURE_DESERT_PYRAMIDS,
            &*STRUCTURE_OCEAN_RUINS,
            &*STRUCTURE_SHIPWRECKS,
            &*STRUCTURE_OCEAN_MONUMENTS,
        ] {
            structure_reg.register(
                &ResourceKey::create(&*STRUCTURE_SET, key.identifier().clone()),
                Arc::new(StructureSet),
                RegistrationInfo::BUILT_IN,
            );
        }
        let structure_registry = structure_reg.freeze();

        let mut placed_reg = RegistryBuilder::new(&*PLACED_FEATURE);
        for key in [&*LAKE_LAVA_UNDERGROUND, &*LAKE_LAVA_SURFACE] {
            placed_reg.register(
                &ResourceKey::create(&*PLACED_FEATURE, key.identifier().clone()),
                Arc::new(PlacedFeature::new(
                    Holder::reference(RegistryId(0), 0),
                    Vec::new(),
                )),
                RegistrationInfo::BUILT_IN,
            );
        }
        let placed_registry = placed_reg.freeze();

        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/biome",
                )),
                Box::new(biome_registry) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
                Box::new(block_registry) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/structure_set",
                )),
                Box::new(structure_registry) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/placed_feature",
                )),
                Box::new(placed_registry) as AnyBox,
            ),
        ])
    }

    /// Run `bootstrap` over a recording context and return the in-order
    /// registrations plus the access (for the key-resolution assertions).
    fn bootstrap_presets() -> (
        Vec<RecordedRegistration<FlatLevelGeneratorPreset>>,
        RegistryAccess,
    ) {
        let access = access();
        let block_registry_id = access
            .lookup::<registries::BlockType>(&*registries::BLOCK)
            .expect("block registry")
            .registry_id();
        let mut context: RecordingContext<FlatLevelGeneratorPreset> = RecordingContext::new(
            RegistryId(0),
            (*FLAT_LEVEL_GENERATOR_PRESET).clone(),
            access.clone(),
        );
        bootstrap(&mut context, block_registry_id);
        (context.registrations().iter().cloned().collect(), access)
    }

    fn preset<'a>(
        registrations: &'a [RecordedRegistration<FlatLevelGeneratorPreset>],
        key: &PresetKey,
    ) -> &'a FlatLevelGeneratorPreset {
        registrations
            .iter()
            .find(|r| &r.key == key)
            .map(|r| &r.value)
            .expect("preset registered")
    }

    /// The preset's display item name (bootstrap produces a `Direct` holder).
    fn display_name(preset: &FlatLevelGeneratorPreset) -> &str {
        match &preset.display_item {
            Holder::Direct(item) => item.name(),
            Holder::Reference { .. } => "reference",
        }
    }

    /// The preset's structure-override keys, in registration order.
    fn structure_keys(
        settings: &FlatLevelGeneratorSettings,
        lookup: &dyn HolderLookup<StructureSet>,
    ) -> Vec<ResourceKey<StructureSet>> {
        settings
            .structure_overrides()
            .map(|overrides| overrides.iter().map(|h| h.key(lookup)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn registers_all_nine_presets_in_java_order_and_never_test_world() {
        let (registrations, _access) = bootstrap_presets();
        let keys: Vec<&PresetKey> = registrations.iter().map(|r| &r.key).collect();
        let expected: Vec<&PresetKey> = vec![
            &*CLASSIC_FLAT,
            &*TUNNELERS_DREAM,
            &*WATER_WORLD,
            &*OVERWORLD,
            &*SNOWY_KINGDOM,
            &*BOTTOMLESS_PIT,
            &*DESERT,
            &*REDSTONE_READY,
            &*THE_VOID,
        ];
        assert_eq!(
            keys, expected,
            "registrations must be the nine builtins in Java bootstrap order"
        );
        for r in &registrations {
            assert_eq!(r.lifecycle, rivet_serialization::Lifecycle::stable());
        }
        assert!(
            !keys.iter().any(|k| **k == *TEST_WORLD),
            "TEST_WORLD is declared but never registered (Java keeps the key for the debug generator)"
        );
    }

    #[test]
    fn classic_flat_layers_are_reversed_bottom_first_and_not_yet_expanded() {
        let (registrations, _access) = bootstrap_presets();
        let settings = &preset(&registrations, &CLASSIC_FLAT).settings;

        // Java's `Bootstrap.register` appends layers in REVERSE declaration
        // order, so the declaration `grass(1), dirt(2), bedrock(1)` becomes the
        // bottom-first column `bedrock(1), dirt(2), grass(1)`.
        let layers = settings.get_layers_info();
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].get_height(), 1);
        assert_eq!(layers[0].get_block_state().block(), Blocks::BEDROCK.id());
        assert_eq!(layers[1].get_height(), 2);
        assert_eq!(layers[1].get_block_state().block(), Blocks::DIRT.id());
        assert_eq!(layers[2].get_height(), 1);
        assert_eq!(
            layers[2].get_block_state().block(),
            Blocks::GRASS_BLOCK.id()
        );

        // Bootstrap never runs `updateLayers`, so the expanded column stays
        // empty and `voidGen` keeps its constructor default `false` — matching
        // Java's `register` (the column is only expanded later, by
        // `withBiomeAndLayers` / the settings codec).
        assert!(settings.get_layers().is_empty());
        assert!(!settings.void_gen());
        assert!(!settings.decoration());
        assert!(!settings.add_lakes());
    }

    #[test]
    fn the_void_is_a_single_air_layer_with_decoration_but_void_gen_untouched() {
        let (registrations, _access) = bootstrap_presets();
        let settings = &preset(&registrations, &THE_VOID).settings;

        let layers = settings.get_layers_info();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].get_height(), 1);
        assert_eq!(layers[0].get_block_state().block(), Blocks::AIR.id());
        assert!(settings.get_layers().is_empty());
        // `updateLayers` has not run during bootstrap, so `voidGen` is still
        // the constructor default `false` even for an all-air preset (Java's
        // `register` never expands the column either).
        assert!(
            !settings.void_gen(),
            "voidGen is computed by updateLayers, not bootstrap"
        );
        assert!(
            settings.decoration(),
            "THE_VOID registers with decoration=true"
        );
        assert!(!settings.add_lakes());
        assert!(
            settings
                .structure_overrides()
                .is_some_and(|s| s.iter().next().is_none()),
            "THE_VOID registers with an empty structure override set"
        );
    }

    #[test]
    fn overworld_layer_order_is_reversed() {
        let (registrations, _access) = bootstrap_presets();
        let settings = &preset(&registrations, &OVERWORLD).settings;

        // A second, longer sequence: the reversed declaration
        // `grass(1), dirt(3), stone(59), bedrock(1)`.
        let layers = settings.get_layers_info();
        assert_eq!(layers.len(), 4);
        assert_eq!(layers[0].get_height(), 1);
        assert_eq!(layers[0].get_block_state().block(), Blocks::BEDROCK.id());
        assert_eq!(layers[1].get_height(), 59);
        assert_eq!(layers[1].get_block_state().block(), Blocks::STONE.id());
        assert_eq!(layers[2].get_height(), 3);
        assert_eq!(layers[2].get_block_state().block(), Blocks::DIRT.id());
        assert_eq!(layers[3].get_height(), 1);
        assert_eq!(
            layers[3].get_block_state().block(),
            Blocks::GRASS_BLOCK.id()
        );
    }

    #[test]
    fn preset_biomes_structures_and_display_items_match_java() {
        let (registrations, access) = bootstrap_presets();
        let biome_lookup: &dyn HolderLookup<BiomeId> = access
            .lookup::<BiomeId>(&*registries::BIOME)
            .expect("biome registry");
        let structure_lookup: &dyn HolderLookup<StructureSet> = access
            .lookup::<StructureSet>(&*STRUCTURE_SET)
            .expect("structure registry");

        // The nine preset registrations: (key, biome, structure keys, display
        // item, decoration, addLakes) — Java's `Bootstrap.run` data.
        let cases: &[PresetCase<'_>; 9] = &[
            (
                &*CLASSIC_FLAT,
                &*biomes::PLAINS,
                &[&*STRUCTURE_VILLAGES],
                "minecraft:grass_block",
                false,
                false,
            ),
            (
                &*TUNNELERS_DREAM,
                &*biomes::WINDSWEPT_HILLS,
                &[&*STRUCTURE_MINESHAFTS, &*STRUCTURE_STRONGHOLDS],
                "minecraft:stone",
                true,
                false,
            ),
            (
                &*WATER_WORLD,
                &*biomes::DEEP_OCEAN,
                &[
                    &*STRUCTURE_OCEAN_RUINS,
                    &*STRUCTURE_SHIPWRECKS,
                    &*STRUCTURE_OCEAN_MONUMENTS,
                ],
                "minecraft:water_bucket",
                false,
                false,
            ),
            (
                &*OVERWORLD,
                &*biomes::PLAINS,
                &[
                    &*STRUCTURE_VILLAGES,
                    &*STRUCTURE_MINESHAFTS,
                    &*STRUCTURE_PILLAGER_OUTPOSTS,
                    &*STRUCTURE_RUINED_PORTALS,
                    &*STRUCTURE_STRONGHOLDS,
                ],
                "minecraft:short_grass",
                true,
                true,
            ),
            (
                &*SNOWY_KINGDOM,
                &*biomes::SNOWY_PLAINS,
                &[&*STRUCTURE_VILLAGES, &*STRUCTURE_IGLOOS],
                "minecraft:snow",
                false,
                false,
            ),
            (
                &*BOTTOMLESS_PIT,
                &*biomes::PLAINS,
                &[&*STRUCTURE_VILLAGES],
                "minecraft:feather",
                false,
                false,
            ),
            (
                &*DESERT,
                &*biomes::DESERT,
                &[
                    &*STRUCTURE_VILLAGES,
                    &*STRUCTURE_DESERT_PYRAMIDS,
                    &*STRUCTURE_MINESHAFTS,
                    &*STRUCTURE_STRONGHOLDS,
                ],
                "minecraft:sand",
                true,
                false,
            ),
            (
                &*REDSTONE_READY,
                &*biomes::DESERT,
                &[],
                "minecraft:redstone",
                false,
                false,
            ),
            (
                &*THE_VOID,
                &*biomes::THE_VOID,
                &[],
                "minecraft:barrier",
                true,
                false,
            ),
        ];

        for (key, biome_key, expected_structures, display, decoration, lakes) in *cases {
            let preset = preset(&registrations, key);
            let settings = &preset.settings;
            assert!(
                settings.get_biome().is_key(biome_lookup, biome_key),
                "{key}: biome mismatch"
            );
            let expected_structures: Vec<ResourceKey<StructureSet>> =
                expected_structures.iter().map(|k| (*k).clone()).collect();
            assert_eq!(
                structure_keys(settings, structure_lookup),
                expected_structures,
                "{key}: structure-set mismatch"
            );
            assert_eq!(
                display_name(preset),
                display,
                "{key}: display-item mismatch"
            );
            assert_eq!(
                settings.decoration(),
                decoration,
                "{key}: decoration mismatch"
            );
            assert_eq!(settings.add_lakes(), lakes, "{key}: addLakes mismatch");
        }
    }
}

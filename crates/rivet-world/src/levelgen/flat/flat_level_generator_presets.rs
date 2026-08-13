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

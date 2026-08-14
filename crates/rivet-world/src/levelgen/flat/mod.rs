//! Port of `net.minecraft.world.level.levelgen.flat` (package, 26.2).
//!
//! The flat world type: `FlatLevelGeneratorSettings` (the per-world settings
//! codec, its layer list, and `adjustGenerationSettings` — the derived biome
//! generation settings), `FlatLayerInfo` (a single flat layer), the
//! `FlatLevelGeneratorPreset` value + its `worldgen/flat_level_generator_preset`
//! registry file codec, and `FlatLevelGeneratorPresets` (the nine built-in
//! preset registrations bootstrapped into that registry).
//!
//! The biome reference is carried as `Holder<BiomeId>` (the `biome.core`
//! id-model, `Registry<BiomeId>`); the `"biome"` field and the
//! `RegistryOps.retrieveElement(Biomes.PLAINS)` fallback use the
//! [`biome_id_codec`](crate::biome::biome_id_codec) family. The cross-unit
//! dependencies that are still pending (structure set registry, item registry,
//! misc overworld placements, `Feature.FILL_LAYER`/`PlacementUtils.inlinePlaced`,
//! and the biome-value registry) are exposed as the smallest honest typed
//! `STUB(unit-id)` seams below — nothing outside this unit is pulled in.

pub mod flat_layer_info;
pub mod flat_level_generator_preset;
pub mod flat_level_generator_presets;
pub mod flat_level_generator_settings;

use crate::levelgen::feature::FeatureId;
use crate::levelgen::flat::flat_level_generator_preset::FlatLevelGeneratorPreset;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Identifier;
use rivet_registry::Registry;
use rivet_registry::ResourceKey;
use rivet_registry::holder::Holder;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Cross-unit STUB seams (out-of-unit pending types; `// STUB(unit-id)`).
// ---------------------------------------------------------------------------

/// STUB(mc.world.level.levelgen.structure.framework): the `StructureSet`
/// registry element. Pending `mc.world.level.levelgen.structure` owns the real
/// value (a placement/rotation/refactor list over structure templates); this
/// unit needs only its registry identity to carry the `structure_overrides`
/// holder set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureSet;

/// STUB(mc.world.level.levelgen.structure.framework): `Registries.STRUCTURE_SET`
/// (`ResourceKey.createRegistryKey("minecraft:worldgen/structure_set")`). The
/// registry itself is bound by the structure unit; here it names the
/// `structure_overrides` holder-set codec.
pub static STRUCTURE_SET: LazyLock<ResourceKey<Registry<StructureSet>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/structure_set"))
});

/// STUB(mc.world.item): the `Item` registry element — a namespaced item name
/// handle. Pending `mc.world.item` owns the real item value; the flat presets
/// need only a display-item holder for the `"display"` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStub(String);

impl ItemStub {
    /// The minimal STUB carrier — the item's registered name.
    ///
    /// STUB(mc.world.item): `new ItemStub(String)` is a placeholder for the
    /// real item; consumers in this unit construct it from a known name.
    pub fn new(name: &str) -> Self {
        ItemStub(name.to_string())
    }

    /// The registered name (`minecraft:stone`).
    pub fn name(&self) -> &str {
        &self.0
    }

    /// STUB(mc.world.item): `Item.builtInRegistryHolder()` — Java yields a
    /// `Holder.Reference<Item>` in the builtin item registry. With no ITEM
    /// registry bound, the display holder is carried inline as a `Direct`.
    pub fn built_in_registry_holder(self) -> Holder<ItemStub> {
        Holder::direct(self)
    }
}

/// STUB(mc.world.item): the ITEM registry key
/// (`ResourceKey.createRegistryKey("minecraft:item")`).
pub static ITEM: LazyLock<ResourceKey<Registry<ItemStub>>> =
    LazyLock::new(|| ResourceKey::create_registry_key(Identifier::with_default_namespace("item")));

/// `Registries.FLAT_LEVEL_GENERATOR_PRESET`
/// (`ResourceKey.createRegistryKey("minecraft:worldgen/flat_level_generator_preset")`)
/// — the registry the preset codec and the nine built-in presets live in.
pub static FLAT_LEVEL_GENERATOR_PRESET: LazyLock<ResourceKey<Registry<FlatLevelGeneratorPreset>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/flat_level_generator_preset",
        ))
    });

/// STUB(mc.world.item): `net.minecraft.world.level.ItemLike` — a block or item
/// used as a preset icon. `asItem()` returns the item handle (a block's item
/// is its drop item; the flat presets only need the display name, so both
/// variants resolve to a named item).
pub enum ItemLike {
    /// A `Block` icon (`asItem()` resolves the block's registered name).
    Block(crate::block::Block),
    /// An `Item` icon by registered name.
    Item(&'static str),
}

impl ItemLike {
    /// `ItemLike.asItem()` — the display item handle.
    pub fn as_item(&self) -> ItemStub {
        match self {
            ItemLike::Block(block) => ItemStub::new(block.name()),
            ItemLike::Item(name) => ItemStub::new(name),
        }
    }
}

/// STUB(mc.world.level.levelgen.structure): `BuiltinStructureSets` keys
/// referenced by the flat presets. Pending `mc.world.level.levelgen.structure`
/// owns the real keys; these name the same registry entries.
macro_rules! structure_set_key {
    ($name:ident, $id:literal) => {
        pub static $name: LazyLock<ResourceKey<StructureSet>> = LazyLock::new(|| {
            ResourceKey::create(&STRUCTURE_SET, Identifier::with_default_namespace($id))
        });
    };
}

structure_set_key!(STRUCTURE_STRONGHOLDS, "strongholds");
structure_set_key!(STRUCTURE_VILLAGES, "villages");
structure_set_key!(STRUCTURE_MINESHAFTS, "mineshafts");
structure_set_key!(STRUCTURE_PILLAGER_OUTPOSTS, "pillager_outposts");
structure_set_key!(STRUCTURE_RUINED_PORTALS, "ruined_portals");
structure_set_key!(STRUCTURE_IGLOOS, "igloos");
structure_set_key!(STRUCTURE_DESERT_PYRAMIDS, "desert_pyramids");
structure_set_key!(STRUCTURE_OCEAN_RUINS, "ocean_ruins");
structure_set_key!(STRUCTURE_SHIPWRECKS, "shipwrecks");
structure_set_key!(STRUCTURE_OCEAN_MONUMENTS, "ocean_monuments");

/// STUB(mc.data.worldgen.placement): `MiscOverworldPlacements` lake keys
/// (`createKey("lake_lava_underground")` / `createKey("lake_lava_surface")`),
/// `ResourceKey`s in the `worldgen/placed_feature` registry. Pending
/// `mc.data.worldgen.placement` owns the real keys.
macro_rules! placed_feature_key {
    ($name:ident, $id:literal) => {
        pub static $name: LazyLock<ResourceKey<PlacedFeature>> = LazyLock::new(|| {
            ResourceKey::create(
                &crate::biome::biome_generation_settings::PLACED_FEATURE,
                Identifier::with_default_namespace($id),
            )
        });
    };
}

placed_feature_key!(LAKE_LAVA_UNDERGROUND, "lake_lava_underground");
placed_feature_key!(LAKE_LAVA_SURFACE, "lake_lava_surface");

/// STUB(mc.world.level.levelgen.feature.core): `Feature.FILL_LAYER` — the feature
/// registry id of `fill_layer`. The value is the real protocol id from the
/// generated table (`registries.json` `minecraft:worldgen/feature`
/// `minecraft:fill_layer` = 48 — `Feature.java`'s 49th `register("fill_layer", ...)`,
/// 0-indexed; `no_op` is 0). Pending `feature.core` owns the id; `feature_place`
/// will panic for id 48 until the #181 codegen emits `FillLayerFeature`.
pub const FILL_LAYER: FeatureId = FeatureId::new(48);

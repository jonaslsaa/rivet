//! Port of `net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings`
//! (class, 26.2).
//!
//! The per-world flat generation settings: the 8-field `CODEC`
//! (`structureOverrides`, `layers`, `addLakes`, `decoration`, `biome`, and the
//! three `RegistryOps.retrieveElement` context fields — plains biome,
//! `lake_lava_underground`, `lake_lava_surface`), the
//! `comapFlatMap(validateHeight, identity).stable()` wrapper, the
//! layer-expansion `updateLayers()` / `voidGen` state, and
//! `adjustGenerationSettings` (the derived `BiomeGenerationSettings` honoring
//! lakes/decoration/structure-filtered feature steps and the `FILL_LAYER`
//! non-opaque-layer features).
//!
//! The biome is carried as `Holder<BiomeId>` (the `biome.core` id-model). The
//! `"biome"` field decodes a lenient optional of [`biome_id_codec`] (absent or
//! malformed → `None`), and the `RegistryOps.retrieveElement(Biomes.PLAINS)`
//! field supplies the constructor's `fallbackBiome` (`getBiome` logs and falls
//! back on `None`). The `record_builder` applicative supports at most six
//! fields per group, so the three context-retrieval fields nest as an inner
//! triple field (a `Group6` composition) — wire-identical to Java's flat
//! 8-field `RecordCodecBuilder` (context-retrieval fields encode as no-ops).
//!
//! Cross-unit seams: `StructureSet`/`STRUCTURE_SET` (`structure.framework`),
//! the lake `PlacedFeature` keys (`data.worldgen.placement`), and the biome
//! value registry behind `adjustGenerationSettings` (`biome.core`; the
//! id-model carries only `Holder<BiomeId>`). `adjustGenerationSettings` emits
//! the `FILL_LAYER` top-layer-modification features inline (feature id 48,
//! `minecraft:fill_layer` from the #181 generated table) with no placement
//! modifiers — `PlacementUtils.inlinePlaced(Feature.FILL_LAYER, config)`.

use crate::biome::biome_generation_settings::{BiomeGenerationSettings, PlainBuilder};
use crate::biome::biome_id_codec::biome_id_codec;
use crate::biome::biomes;
use crate::block::blocks::Blocks;
use crate::level::dimension::Y_SIZE;
use crate::levelgen::feature::ConfiguredFeature;
use crate::levelgen::feature::configurations::LayerConfiguration;
use crate::levelgen::flat::FILL_LAYER;
use crate::levelgen::flat::StructureSet;
use crate::levelgen::flat::flat_layer_info::{FlatLayerInfo, flat_layer_info_codec};
use crate::levelgen::generation_step::Decoration;
use crate::levelgen::heightmap::{self, StateFlags};
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderLookup;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::{RegistryOpsLookup, retrieve_element};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::string_representable::EnumOrdinal;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.flat.FlatLevelGeneratorSettings`.
#[derive(Debug, Clone)]
pub struct FlatLevelGeneratorSettings {
    /// `structureOverrides` — the optional structure-set holder set.
    structure_overrides: Option<HolderSet<StructureSet>>,
    /// `layersInfo` — the per-layer infos in registration order.
    layers_info: Vec<FlatLayerInfo>,
    /// `biome` — the resolved biome (`getBiome`, fallback plains).
    biome: Holder<BiomeId>,
    /// `layers` — the block-state column expanded from `layersInfo`. Nullable
    /// slots: `adjustGenerationSettings` nulls non-opaque layer slots on the
    /// LIVE list (Java `layers.set(i, null)`), which the future
    /// `FlatLevelSource` reads with `state != null` / `state == null ? AIR :
    /// state` checks — the `None` slot is the Java null.
    layers: Vec<Option<BlockState>>,
    /// `voidGen` — all layers are air.
    void_gen: bool,
    /// `decoration`.
    decoration: bool,
    /// `addLakes`.
    add_lakes: bool,
    /// `lakes` — `List.of(lavaUnderground, lavaSurface)`.
    lakes: Vec<Holder<PlacedFeature>>,
}

impl FlatLevelGeneratorSettings {
    /// The private full constructor: resolves the biome (`getBiome`, with the
    /// `"Unknown biome, defaulting to plains"` log on an empty optional),
    /// assembles the lakes list, applies `setAddLakes`/`setDecoration`, then
    /// expands the layers.
    #[allow(clippy::too_many_arguments)] // Java's private 8-param constructor.
    fn new_full(
        structure_overrides: Option<HolderSet<StructureSet>>,
        layers: Vec<FlatLayerInfo>,
        lakes: bool,
        features: bool,
        biome: Option<Holder<BiomeId>>,
        fallback_biome: Holder<BiomeId>,
        lava_underground: Holder<PlacedFeature>,
        lava_surface: Holder<PlacedFeature>,
    ) -> Self {
        let mut settings = FlatLevelGeneratorSettings::new(
            structure_overrides,
            get_biome(biome, fallback_biome),
            vec![lava_underground, lava_surface],
        );
        if lakes {
            settings.set_add_lakes();
        }
        if features {
            settings.set_decoration();
        }
        settings.layers_info.extend(layers);
        settings.update_layers();
        settings
    }

    /// The public constructor — `FlatLevelGeneratorSettings(Optional<HolderSet>,
    /// Holder<Biome>, List<Holder<PlacedFeature>>)`.
    pub fn new(
        structure_overrides: Option<HolderSet<StructureSet>>,
        biome: Holder<BiomeId>,
        lakes: Vec<Holder<PlacedFeature>>,
    ) -> Self {
        FlatLevelGeneratorSettings {
            structure_overrides,
            layers_info: Vec::new(),
            biome,
            layers: Vec::new(),
            void_gen: false,
            decoration: false,
            add_lakes: false,
            lakes,
        }
    }

    /// `withBiomeAndLayers(List<FlatLayerInfo>, Optional<HolderSet<StructureSet>>,
    /// Holder<Biome>)` — a copy with the given layers (re-expanding after each
    /// add, as Java does) and the flags carried over.
    pub fn with_biome_and_layers(
        &self,
        layers: &[FlatLayerInfo],
        structure_overrides: Option<HolderSet<StructureSet>>,
        biome: Holder<BiomeId>,
    ) -> FlatLevelGeneratorSettings {
        let mut settings =
            FlatLevelGeneratorSettings::new(structure_overrides, biome, self.lakes.clone());
        for layer_info in layers {
            settings.layers_info.push(layer_info.clone());
            settings.update_layers();
        }
        if self.decoration {
            settings.set_decoration();
        }
        if self.add_lakes {
            settings.set_add_lakes();
        }
        settings
    }

    /// `setDecoration()`.
    pub fn set_decoration(&mut self) {
        self.decoration = true;
    }

    /// `setAddLakes()`.
    pub fn set_add_lakes(&mut self) {
        self.add_lakes = true;
    }

    /// `adjustGenerationSettings(Holder<Biome>)` — the derived
    /// `BiomeGenerationSettings`: the source biome's own generation settings
    /// when it differs from this settings' biome; otherwise a `PlainBuilder`
    /// seeded with this biome's generation settings, the lake features, the
    /// filtered decoration steps, and a `FILL_LAYER` feature per non-opaque
    /// layer.
    ///
    /// `biomes` is the biome registry lookup the biome-value seam resolves
    /// through. Java's `Holder.Reference` stores its own key/value inline, so
    /// `sourceBiome.is(Biomes.THE_VOID)` needs no parameter; the id-model's
    /// back-reference rule (holder.rs) makes that key resolution require the
    /// owning lookup here.
    ///
    /// STUB(mc.world.level.biome.core): the id-model carries biomes
    /// as `Holder<BiomeId>` — there is no biome-value registry to resolve
    /// `sourceBiome.value().getGenerationSettings()`. The method body below is
    /// the faithful 26.2 port; it panics at the first biome-value resolution
    /// until the biome value registry lands (nothing in this unit calls it).
    ///
    /// Takes `&mut self`: Java's non-opaque-layer loop nulls the LIVE
    /// `getLayers()` list (`layers.set(i, null)`), so the mutation is on
    /// `self.layers`.
    pub fn adjust_generation_settings(
        &mut self,
        source_biome: Holder<BiomeId>,
        biomes: &dyn HolderLookup<BiomeId>,
    ) -> BiomeGenerationSettings {
        // Java `!sourceBiome.equals(this.biome)` is OBJECT identity
        // (`Holder.Reference` does not override `equals`, so two distinct
        // instances for the same biome compare unequal). The pure-ID model
        // collapses that per-object identity into the documented `(RegistryId,
        // id)` identity contract (holder.rs: repeated `lookup.get(key)`
        // constructs the same value and `RegistryId + id` is the identity), so
        // the derived structural `PartialEq` is the id-model's identity
        // comparison: the same registry entry IS the same holder.
        if source_biome != self.biome {
            return self.biome_generation_settings_of(&source_biome).clone();
        }

        let biome_generation_settings = self.biome_generation_settings_of(&self.biome);
        let mut new_generation_settings = PlainBuilder::default();
        if self.add_lakes {
            for lake in &self.lakes {
                new_generation_settings =
                    new_generation_settings.add_feature(Decoration::Lakes, lake.clone());
            }
        }

        // `(!this.voidGen || sourceBiome.is(Biomes.THE_VOID)) && this.decoration`.
        let biome_decoration =
            (!self.void_gen || is_the_void(&source_biome, biomes)) && self.decoration;
        if biome_decoration {
            let features = biome_generation_settings.features();
            for (step_index, set) in features.iter().enumerate() {
                if step_index != Decoration::UndergroundStructures.ordinal()
                    && step_index != Decoration::SurfaceStructures.ordinal()
                    && (!self.add_lakes || step_index != Decoration::Lakes.ordinal())
                {
                    for feature in set.iter() {
                        new_generation_settings = new_generation_settings
                            .add_feature_index(step_index as i32, feature.clone());
                    }
                }
            }
        }

        // Java iterates the LIVE `this.getLayers()` list and nulls non-opaque
        // slots as it goes (`layers.set(i, null)`), so slots later in the list
        // are read via `layers.get(i)` on the same mutating list. `self.layers`
        // is a `Vec<Option<BlockState>>`; the port reads `self.layers[i]`
        // through a shared index and sets the slot to `None` — the faithful
        // mapping of the mutating alias. No slot is skipped on the read side:
        // every `layers.get(i)` runs before/independent of the `set`, so a
        // single indexed loop over `self.layers` reproduces Java's reads.
        for i in 0..self.layers.len() {
            let Some(layer) = self.layers[i] else {
                continue;
            };
            // `Heightmap.Types.MOTION_BLOCKING.isOpaque().test(layer)`.
            if !heightmap::Heightmap::is_opaque(
                heightmap::Types::MotionBlocking,
                StateFlags {
                    is_air: layer.is_air(),
                    blocks_motion: layer.blocks_motion(),
                    has_fluid: !layer.fluid_empty(),
                    // `MOTION_BLOCKING` is `blocksMotion() || !getFluidState().isEmpty()`
                    // (Java `Heightmap`) — it has no leaves term, so `is_leaves`
                    // is not computed here (no per-layer tag scan).
                    is_leaves: false,
                },
            ) {
                // Java `layers.set(i, null)` on the LIVE `this.layers` — the
                // `None` slot is the Java null that the future `FlatLevelSource`
                // reads (`state != null` skip in `fillFromNoise`/
                // `getBaseHeight`, `state == null ? AIR : state` in
                // `getBaseColumn`), so the nullable slot carries the observable
                // side effect.
                self.layers[i] = None;
                // `PlacementUtils.inlinePlaced(Feature.FILL_LAYER, new
                // LayerConfiguration(i, layer))` — `layer` is the ORIGINAL
                // non-opaque state (captured before the null set), as Java.
                // `FILL_LAYER` is the feature registry id (48, the #181
                // generated-table protocol id of `minecraft:fill_layer`); the
                // `inlinePlaced` shape is a `Direct` holder of a `PlacedFeature`
                // with no placement modifiers. The `ConfiguredFeature` feature
                // slot is `Holder::direct` so no configured-feature registry is
                // needed to build the holder.
                new_generation_settings =
                    new_generation_settings.add_feature(Decoration::TopLayerModification, {
                        let configured = ConfiguredFeature::new(
                            FILL_LAYER.clone(),
                            LayerConfiguration::new(i as i32, layer),
                        );
                        Holder::direct(PlacedFeature::new(
                            Holder::direct(configured.into_erased()),
                            Vec::new(),
                        ))
                    });
            }
        }

        new_generation_settings.build()
    }

    /// The biome's `BiomeGenerationSettings` — the pending biome-value seam.
    ///
    /// STUB(mc.world.level.biome.core): resolving `Holder<BiomeId>`
    /// to the `Biome` value (and its generation settings) needs the biome-value
    /// registry; until then the settings cannot be derived.
    ///
    /// RivetTodo(#178): when the biome-value registry lands, re-verify the
    /// `adjust_generation_settings` branch divergence — Java's
    /// `!sourceBiome.equals(this.biome)` is OBJECT identity (`Holder.Reference`
    /// inherits `Object.equals`), so two distinct instances of the SAME biome
    /// compare unequal and take the `sourceBiome.value().getGenerationSettings()`
    /// path; the id-model's structural `PartialEq` (same registry entry == same
    /// holder) treats them as equal and takes the derived-`PlainBuilder` path.
    fn biome_generation_settings_of(&self, _biome: &Holder<BiomeId>) -> &BiomeGenerationSettings {
        panic!(
            "STUB(mc.world.level.biome.core): no biome value registry; \
             adjustGenerationSettings cannot resolve a Holder<BiomeId> to generation settings"
        )
    }

    /// `structureOverrides()`.
    pub fn structure_overrides(&self) -> Option<&HolderSet<StructureSet>> {
        self.structure_overrides.as_ref()
    }

    /// `getBiome()`.
    pub fn get_biome(&self) -> &Holder<BiomeId> {
        &self.biome
    }

    /// `getLayersInfo()`.
    pub fn get_layers_info(&self) -> &[FlatLayerInfo] {
        &self.layers_info
    }

    /// `getLayersInfo()` as the live mutable list — the flat presets append
    /// layers in reverse (Java `getLayersInfo().add(...)`).
    pub fn get_layers_info_mut(&mut self) -> &mut Vec<FlatLayerInfo> {
        &mut self.layers_info
    }

    /// `getLayers()`.
    pub fn get_layers(&self) -> &[Option<BlockState>] {
        &self.layers
    }

    /// `decoration`.
    pub fn decoration(&self) -> bool {
        self.decoration
    }

    /// `addLakes`.
    pub fn add_lakes(&self) -> bool {
        self.add_lakes
    }

    /// `voidGen`.
    pub fn void_gen(&self) -> bool {
        self.void_gen
    }

    /// `updateLayers()` — clears the expanded column and rebuilds it from
    /// `layersInfo`, then recomputes `voidGen` (`allMatch(s ->
    /// s.is(Blocks.AIR))` — Java's `is(Block)` is block identity, not state
    /// equality; a `None` slot never qualifies: the null slots from
    /// `adjustGenerationSettings` are never air).
    pub fn update_layers(&mut self) {
        self.layers.clear();
        for layer in &self.layers_info {
            let state = layer.get_block_state();
            for _ in 0..layer.get_height() {
                self.layers.push(Some(state));
            }
        }
        self.void_gen = self
            .layers
            .iter()
            .all(|s| s.is_some_and(|st| st.block() == Blocks::AIR.id()));
    }

    /// `getDefault(HolderGetter<Biome>, HolderGetter<StructureSet>,
    /// HolderGetter<PlacedFeature>)` — strongholds + villages, bedrock(1) /
    /// dirt(2) / grass_block(1).
    ///
    /// STUB(mc.world.level.levelgen.structure.framework): the two structure
    /// keys are resolved through the getters; with the structure registry
    /// pending, the caller passes the already-resolved holders.
    pub fn get_default(
        biomes: &dyn HolderLookup<BiomeId>,
        strongholds: Holder<StructureSet>,
        villages: Holder<StructureSet>,
        placed_features: &[Holder<PlacedFeature>],
        block_registry: rivet_registry::holder::RegistryId,
    ) -> FlatLevelGeneratorSettings {
        let structure_settings = HolderSet::direct(vec![strongholds, villages]);
        let mut result = FlatLevelGeneratorSettings::new(
            Some(structure_settings),
            Self::get_default_biome(biomes),
            placed_features.to_vec(),
        );
        result.layers_info.push(FlatLayerInfo::from_block(
            1,
            crate::block::blocks::Blocks::BEDROCK,
            block_registry,
        ));
        result.layers_info.push(FlatLayerInfo::from_block(
            2,
            crate::block::blocks::Blocks::DIRT,
            block_registry,
        ));
        result.layers_info.push(FlatLayerInfo::from_block(
            1,
            crate::block::blocks::Blocks::GRASS_BLOCK,
            block_registry,
        ));
        result.update_layers();
        result
    }

    /// `getDefaultBiome(HolderGetter<Biome>)` — `getOrThrow(Biomes.PLAINS)`.
    pub fn get_default_biome(biomes: &dyn HolderLookup<BiomeId>) -> Holder<BiomeId> {
        biomes.get_or_throw(&biomes::PLAINS)
    }

    /// `createLakesList(HolderGetter<PlacedFeature>)` — the two lava-lake
    /// holders. The caller passes the already-resolved holders (the
    /// `data.worldgen.placement` registry is pending).
    pub fn create_lakes_list(
        lava_underground: Holder<PlacedFeature>,
        lava_surface: Holder<PlacedFeature>,
    ) -> Vec<Holder<PlacedFeature>> {
        vec![lava_underground, lava_surface]
    }
}

/// `sourceBiome.is(Biomes.THE_VOID)` in the id model — Java's
/// `Holder.Reference.is(ResourceKey)` is `key() == key`: the reference's key
/// resolved through its OWNING registry. The id-model back-reference rule makes
/// that resolution require the owning biome lookup, so the faithful check is
/// the lookup-based `Holder::is_key` — NOT a bare element-id compare (a
/// reference whose element id coincides with THE_VOID's generated id but lives
/// in a different registry would be misclassified as the void). A `Direct`
/// holder is never the void (Java `Direct.is(ResourceKey)` is always false).
fn is_the_void(biome: &Holder<BiomeId>, biomes: &dyn HolderLookup<BiomeId>) -> bool {
    biome.is_key(biomes, &biomes::THE_VOID)
}

/// `getBiome(Optional<? extends Holder<Biome>>, Holder<Biome>)` — the
/// empty-optional log + plains fallback.
fn get_biome(biome: Option<Holder<BiomeId>>, fallback_biome: Holder<BiomeId>) -> Holder<BiomeId> {
    match biome {
        Some(b) => b,
        None => {
            // Java: `LOGGER.error("Unknown biome, defaulting to plains")` — the
            // port has no logger; the message is emitted to stderr, the crate's
            // established error/warn-level logging seam (`tracing` is not a
            // dependency of `rivet-world`; see
            // `FeatureFlagRegistry::from_names`).
            eprintln!("Unknown biome, defaulting to plains");
            fallback_biome
        }
    }
}

/// `validateHeight(FlatLevelGeneratorSettings)` — the `comapFlatMap` decoder;
/// `sum(layersInfo heights) > Y_SIZE` errors with the exact Java message,
/// carrying the partially-decoded `settings` (`DataResult.error(msg, settings)`).
fn validate_height(
    settings: &FlatLevelGeneratorSettings,
) -> DataResult<FlatLevelGeneratorSettings> {
    // Java `mapToInt(FlatLayerInfo::getHeight).sum()` wraps silently on int
    // overflow (PORTING.md wrapping arithmetic); `Iterator::sum::<i32>` would
    // panic in debug builds instead.
    let total_height: i32 = settings
        .layers_info
        .iter()
        .map(|l| l.get_height())
        .fold(0, i32::wrapping_add);
    if total_height > Y_SIZE {
        DataResult::error_with_partial(
            format!("Sum of layer heights is > {}", Y_SIZE),
            settings.clone(),
        )
    } else {
        DataResult::success(settings.clone())
    }
}

/// `RegistryCodecs.homogeneousList(Registries.STRUCTURE_SET).lenientOptionalFieldOf("structure_overrides")`
/// — the optional holder-set field over the structure registry.
fn structure_overrides_field<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<Option<HolderSet<StructureSet>>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<StructureSet>, Ops>> =
        Arc::new(RegistryFixedCodec::create(&super::STRUCTURE_SET));
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<StructureSet>, Ops>> = Arc::new(
        HolderSetCodec::create(&super::STRUCTURE_SET, element, false),
    );
    codec::optional_field("structure_overrides".to_string(), holder_set, true)
}

/// `Biome.CODEC.lenientOptionalFieldOf("biome").orElseGet(Optional::empty)` —
/// the lenient optional biome field (absent or malformed → `None`).
fn biome_field<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<Option<Holder<BiomeId>>, Ops>> {
    let inner = codec::optional_field("biome".to_string(), biome_id_codec::<Ops>(), true);
    // `.orElseGet(Optional::empty)` — the lenient optional never errors (absent
    // and malformed both already yield `None`), so this is a wire no-op kept
    // for structure.
    map_codec::or_else_get_value(inner, Arc::new(|| None))
}

/// `ExtraCodecs.optionalAlwaysPresentFieldOf(Codec.BOOL, "lakes"/"features",
/// false)` — a NON-lenient optional bool that ALWAYS encodes (absent → false on
/// decode, and the key is written even when false). The port has no
/// `optionalAlwaysPresentFieldOf` helper; the wire-identical construction is
/// `optional_field(name, bool, false).xmap(o -> o.orElse(false), a -> Some(a))`.
fn always_present_bool_field<Ops: DynamicOps + 'static>(
    name: &str,
) -> Arc<dyn rivet_serialization::map_codec::MapCodec<bool, Ops>> {
    map_codec::xmap(
        codec::optional_field(name.to_string(), codec::bool_codec::<Ops>(), false),
        Arc::new(|o: &Option<bool>| o.unwrap_or(false)),
        Arc::new(|b: &bool| Some(*b)),
    )
}

/// `FlatLevelGeneratorSettings.CODEC` — the 8-field record codec
/// (`comapFlatMap(validateHeight, identity).stable()`), as the ops-generic
/// `flat_level_generator_settings_codec::<Ops>()` factory.
///
/// The `record_builder` applicative supports at most six fields per group, so
/// the three `retrieveElement` context fields (plains biome fallback,
/// `lake_lava_underground`, `lake_lava_surface`) nest as an inner triple field
/// — the outer `Group6` is `(structureOverrides, layers, lakes, features,
/// biome)` plus the triple. Context-retrieval fields encode as no-ops, so the
/// wire form is identical to Java's flat 8-field builder.
pub fn flat_level_generator_settings_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<FlatLevelGeneratorSettings, Ops>> {
    // Inner record: the three retrieveElement context fields as a
    // `(Holder<BiomeId>, Holder<PlacedFeature>, Holder<PlacedFeature>)` triple.
    #[allow(clippy::type_complexity)] // the 3-holder context-field triple.
    let inner: Arc<
        dyn rivet_serialization::map_codec::MapCodec<
                (
                    Holder<BiomeId>,
                    Holder<PlacedFeature>,
                    Holder<PlacedFeature>,
                ),
                Ops,
            >,
    > = record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &(Holder<BiomeId>, _, _)| t.0.clone()),
                retrieve_element(&biomes::PLAINS),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &(_, Holder<PlacedFeature>, _)| t.1.clone()),
                retrieve_element(&super::LAKE_LAVA_UNDERGROUND),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &(_, _, Holder<PlacedFeature>)| t.2.clone()),
                retrieve_element(&super::LAKE_LAVA_SURFACE),
            ))
            .apply(
                instance,
                Arc::new(
                    |fallback: Holder<BiomeId>,
                     lava_underground: Holder<PlacedFeature>,
                     lava_surface: Holder<PlacedFeature>| {
                        (fallback, lava_underground, lava_surface)
                    },
                ),
            )
    });

    // Java's three `RegistryOps.retrieveElement` fields have no `forGetter` —
    // their encoders are no-ops that never consult the value. The `record_builder`
    // encoder calls the getter unconditionally, so it must return *something*; a
    // fixed placeholder (never read) mirrors Java's absent getter and avoids
    // touching `self.lakes` (empty for a hand-built settings).
    let dummy_context = (
        Holder::reference(rivet_registry::holder::RegistryId(0), 0),
        Holder::reference(rivet_registry::holder::RegistryId(0), 0),
        Holder::reference(rivet_registry::holder::RegistryId(0), 0),
    );

    let codec = record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|s: &FlatLevelGeneratorSettings| s.structure_overrides.clone()),
                structure_overrides_field::<Ops>(),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|s: &FlatLevelGeneratorSettings| s.layers_info.clone()),
                "layers".to_string(),
                codec::list(flat_layer_info_codec::<Ops>()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &FlatLevelGeneratorSettings| s.add_lakes),
                always_present_bool_field::<Ops>("lakes"),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &FlatLevelGeneratorSettings| s.decoration),
                always_present_bool_field::<Ops>("features"),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|s: &FlatLevelGeneratorSettings| Some(s.biome.clone())),
                biome_field::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                // The inner context fields encode as no-ops; the getter value is
                // discarded (the placeholder above), so the encode is faithful to
                // Java's no-`forGetter` fields.
                Arc::new(move |_: &FlatLevelGeneratorSettings| dummy_context.clone()),
                inner,
            ))
            .apply(
                instance,
                Arc::new(
                    |structure_overrides: Option<HolderSet<StructureSet>>,
                     layers: Vec<FlatLayerInfo>,
                     lakes: bool,
                     features: bool,
                     biome: Option<Holder<BiomeId>>,
                     (fallback_biome, lava_underground, lava_surface): (
                        Holder<BiomeId>,
                        Holder<PlacedFeature>,
                        Holder<PlacedFeature>,
                    )| {
                        FlatLevelGeneratorSettings::new_full(
                            structure_overrides,
                            layers,
                            lakes,
                            features,
                            biome,
                            fallback_biome,
                            lava_underground,
                            lava_surface,
                        )
                    },
                ),
            )
    });

    // `.comapFlatMap(validateHeight, identity).stable()`.
    codec::stable(codec::comap_flat_map(
        codec,
        Arc::new(validate_height),
        Arc::new(|s: &FlatLevelGeneratorSettings| s.clone()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_generation_settings;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder_lookup::HolderGetter;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registries;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A `RegistryAccess` registering `minecraft:plains` (as the id 40) in the
    /// biome registry, `stone`/`dirt` in the block registry, and the two lava
    /// lakes in the placed-feature registry — the three registries the settings
    /// codec resolves through its ops.
    fn access() -> RegistryAccess {
        let mut biomes_reg = RegistryBuilder::new(&*registries::BIOME);
        biomes_reg.register(
            &ResourceKey::create(&*registries::BIOME, Identifier::parse("minecraft:plains")),
            Arc::new(BiomeId::from_id(40)),
            RegistrationInfo::BUILT_IN,
        );
        let biome_registry = biomes_reg.freeze();

        // Air-first registration order (air=0, stone=1, dirt=2) makes each
        // element id coincide with its generated `BlockId`, so decoded layer
        // references expand coherently through `get_block_state` (which resolves
        // element ids via the generated table).
        let mut blocks = RegistryBuilder::new(&*registries::BLOCK);
        for name in ["minecraft:air", "minecraft:stone", "minecraft:dirt"] {
            blocks.register(
                &ResourceKey::create(&*registries::BLOCK, Identifier::parse(name)),
                Arc::new(registries::BlockType),
                RegistrationInfo::BUILT_IN,
            );
        }
        let block_registry = blocks.freeze();

        let mut placed = RegistryBuilder::new(&*biome_generation_settings::PLACED_FEATURE);
        for name in [
            "minecraft:lake_lava_underground",
            "minecraft:lake_lava_surface",
        ] {
            placed.register(
                &ResourceKey::create(
                    &*biome_generation_settings::PLACED_FEATURE,
                    Identifier::parse(name),
                ),
                Arc::new(PlacedFeature::new(
                    rivet_registry::Holder::reference(rivet_registry::RegistryId(0), 0),
                    Vec::new(),
                )),
                RegistrationInfo::BUILT_IN,
            );
        }
        let placed_registry = placed.freeze();

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
                    "worldgen/placed_feature",
                )),
                Box::new(placed_registry) as AnyBox,
            ),
        ])
    }

    #[test]
    fn codec_round_trips_the_full_settings_record() {
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        let codec = flat_level_generator_settings_codec::<TestOps>();
        let settings = json!({
            "layers": [{ "height": 1, "block": "minecraft:stone" }],
            "biome": "minecraft:plains",
            "lakes": true,
            "features": false,
        });
        let parsed = codec.parse(&ops, &settings);
        let decoded = parsed.result().expect("decode should succeed");
        assert!(decoded.add_lakes);
        assert!(!decoded.decoration);
        assert_eq!(decoded.layers_info.len(), 1);
        assert_eq!(decoded.layers_info[0].get_height(), 1);
        assert_eq!(decoded.layers.len(), 1);
        // The decoded `minecraft:stone` reference (element id 1, air being 0)
        // expanded through `update_layers` to the stone default state — pinning
        // the id-model coherence between the codec and `get_block_state`. The
        // slot is `Some` (only `adjustGenerationSettings` nulls slots).
        assert_eq!(
            decoded.layers[0]
                .expect("no adjustGenerationSettings ran")
                .block()
                .id() as u32,
            1
        );
        // The `"biome"` field decoded to the plains reference.
        let biome_lookup = access
            .lookup::<BiomeId>(&*registries::BIOME)
            .expect("biome registry");
        assert!(decoded.biome.is_key(biome_lookup, &biomes::PLAINS));
        // The `retrieveElement` context fields resolved the two lakes.
        assert_eq!(decoded.lakes.len(), 2);
        assert!(decoded.lakes[0].registry_id().is_some());
        assert!(decoded.lakes[1].registry_id().is_some());

        let encoded = codec
            .encode_start(&ops, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded["layers"], settings["layers"]);
        assert_eq!(encoded["biome"], json!("minecraft:plains"));
        assert_eq!(encoded["lakes"], json!(true));
        assert_eq!(encoded["features"], json!(false));
    }

    #[test]
    fn validate_height_rejects_a_column_taller_than_y_size() {
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = flat_level_generator_settings_codec::<TestOps>();
        // Two full-height layers sum to `2 * Y_SIZE` — over the limit the
        // `comapFlatMap` validator rejects (a single layer is capped at `Y_SIZE`
        // by the layer codec's int range, so the sum is what trips it).
        let too_tall = json!({
            "layers": [
                { "height": Y_SIZE, "block": "minecraft:stone" },
                { "height": Y_SIZE, "block": "minecraft:stone" },
            ],
            "biome": "minecraft:plains",
        });
        let result = codec.parse(&ops, &too_tall);
        let message = result
            .error_ref()
            .map(|e| e.message().to_string())
            .expect("a > Y_SIZE column must error");
        assert_eq!(message, format!("Sum of layer heights is > {}", Y_SIZE));
    }

    #[test]
    fn absent_optional_fields_default_to_false_and_empty() {
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = flat_level_generator_settings_codec::<TestOps>();
        let minimal = json!({
            "layers": [{ "height": 1, "block": "minecraft:stone" }],
            "biome": "minecraft:plains",
        });
        let parsed = codec.parse(&ops, &minimal);
        let decoded = parsed.result().expect("decode should succeed");
        assert!(!decoded.add_lakes);
        assert!(!decoded.decoration);
        assert!(decoded.structure_overrides.is_none());
    }

    /// `sourceBiome.is(Biomes.THE_VOID)` resolves the reference's key through
    /// its OWNING registry — NOT a bare element-id compare against THE_VOID's
    /// generated id (58). A biome registry that places `minecraft:the_void` at
    /// element id 0 (a different registration order) still identifies it as the
    /// void through the key, and a `Direct` holder is never the void.
    #[test]
    fn is_the_void_resolves_the_key_through_the_owning_registry() {
        // A biome registry with a non-generated insertion order: the_void at
        // element id 0, plains at id 1.
        let mut biomes_reg = RegistryBuilder::new(&*registries::BIOME);
        for (key, value) in [(&*biomes::THE_VOID, 0u16), (&*biomes::PLAINS, 1u16)] {
            biomes_reg.register(
                &ResourceKey::create(&*registries::BIOME, key.identifier().clone()),
                Arc::new(BiomeId::from_id(value)),
                RegistrationInfo::BUILT_IN,
            );
        }
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome")),
            Box::new(biomes_reg.freeze()) as AnyBox,
        )]);
        let biomes = access
            .lookup::<BiomeId>(&*registries::BIOME)
            .expect("biome registry");

        // the_void at element id 0: a bare compare against the generated id 58
        // would report false (the old buggy behavior); the key resolution
        // reports true.
        let the_void = biomes.get_or_throw(&biomes::THE_VOID);
        assert_eq!(the_void.registry_id(), Some(biomes.registry_id()));
        assert!(is_the_void(&the_void, biomes));

        // plains at element id 1 is not the void.
        let plains = biomes.get_or_throw(&biomes::PLAINS);
        assert!(!is_the_void(&plains, biomes));

        // A `Direct` holder is never the void (Java `Direct.is(ResourceKey)` is
        // always false).
        let direct = Holder::direct(BiomeId::from_id(58));
        assert!(!is_the_void(&direct, biomes));
    }

    /// `updateLayers` recomputes `voidGen` by BLOCK identity
    /// (`allMatch(s -> s.is(Blocks.AIR))` — `state.getBlock() == Blocks.AIR`),
    /// not state equality. A column of air layers is void; any non-air layer
    /// is not.
    #[test]
    fn update_layers_sets_void_gen_by_block_identity() {
        let registry_id = block_registry_id(&access());
        let mut air = FlatLevelGeneratorSettings::new(None, plains_holder(&access()), Vec::new());
        air.layers_info.push(FlatLayerInfo::from_block(
            1,
            crate::block::blocks::Blocks::AIR,
            registry_id,
        ));
        air.update_layers();
        assert!(air.void_gen, "an all-air column must be void");

        let mut stone = FlatLevelGeneratorSettings::new(None, plains_holder(&access()), Vec::new());
        stone.layers_info.push(FlatLayerInfo::from_block(
            1,
            crate::block::blocks::Blocks::STONE,
            registry_id,
        ));
        stone.update_layers();
        assert!(!stone.void_gen, "a stone column must not be void");
    }

    /// The block registry's real `RegistryId` (assigned by the global counter)
    /// — the reference-id the hand-built layer holders carry.
    fn block_registry_id(access: &RegistryAccess) -> rivet_registry::holder::RegistryId {
        access
            .lookup::<registries::BlockType>(&*registries::BLOCK)
            .expect("block registry")
            .registry_id()
    }

    /// A `plains` biome holder resolved through the test access (the settings
    /// constructors need a biome holder; only the id is carried).
    fn plains_holder(access: &RegistryAccess) -> Holder<BiomeId> {
        access
            .lookup::<BiomeId>(&*registries::BIOME)
            .expect("biome registry")
            .get_or_throw(&biomes::PLAINS)
    }
}

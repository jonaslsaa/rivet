//! Port of `net.minecraft.world.level.levelgen.FlatLevelSource` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The superflat `ChunkGenerator`: a `FlatLevelGeneratorSettings` value (the
//! `mc.world.level.levelgen.flat` unit's record, #178) plus the fixed flat
//! biome source. The portable surface is ported faithfully — `getSpawnHeight`,
//! `getBaseHeight`, `getBaseColumn`, the `fillFromNoise` layer writes, and the
//! `getMinY`/`getGenDepth`/`getSeaLevel` constants — and the `CODEC` is the
//! real `FlatLevelGeneratorSettings.CODEC` `fieldOf("settings")`.
//!
//! ### The `FlatLevelGeneratorSettings` dependency
//!
//! `FlatLevelSource.CODEC` reads `FlatLevelGeneratorSettings.CODEC` — the
//! `mc.world.level.levelgen.flat` unit (issue #178, merged as the `levelgen::flat`
//! module). This unit consumes the record directly: the nullable `getLayers()`
//! list (`Vec<Option<BlockState>>`, where the `None` slot is Java's `null`
//! introduced by `adjustGenerationSettings`), `getBiome()`, and
//! `flat_level_generator_settings_codec::<Ops>()`.
//!
//! ### The `ChunkGenerator` trait
//!
//! `FlatLevelSource` implements the trait surface
//! (`rivet-world::chunk::chunk_generator`) for every method whose Java body is
//! nameable here — the constants (`getMinY`/`getGenDepth`/`getSeaLevel`), the
//! world-surface reads (`getSpawnHeight`/`getBaseHeight`/`getBaseColumn`), the
//! no-op `addDebugScreenInfo`, and `getBiomeSource`. The worldgen lifecycle
//! steps (`buildSurface`/`applyCarvers`/`spawnOriginalMobs` — all empty in
//! Java — plus `applyBiomeDecoration`, left on the Java default) stay on the
//! trait's default panic seams (RivetTodo #185); `fillFromNoise` ports its real
//! body as an inherent method over the generic `ProtoChunk` (the noisegen
//! value-shell shape), and `createState` (the structure-state override) defers
//! with the `mc.world.level.chunk.generator` structure units (RivetTodo #185).

use crate::biome::biome_source::BiomeSource;
use crate::biome::fixed_biome_source::FixedBiomeSource;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::chunk::proto_chunk::ProtoChunk;
use crate::chunk::storage::chunk_reconstruction::{block_state_predicates, resolve_state_flags};
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::blending::blender::Blender;
use crate::levelgen::flat::flat_level_generator_settings::{
    FlatLevelGeneratorSettings, flat_level_generator_settings_codec,
};
use crate::levelgen::heightmap::{Heightmap, Types};
use crate::levelgen::noisegen::random_state::RandomState;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

use crate::block::blocks::Blocks;

/// `net.minecraft.world.level.levelgen.FlatLevelSource`.
pub struct FlatLevelSource {
    /// `settings`.
    settings: FlatLevelGeneratorSettings,
    /// The `biomeSource` base field — the fixed flat biome source.
    biome_source: FixedBiomeSource,
}

impl FlatLevelSource {
    /// `FlatLevelSource(FlatLevelGeneratorSettings)` — `this(generatorSettings,
    /// new FixedBiomeSource(generatorSettings.getBiome()))`.
    pub fn new(generator_settings: FlatLevelGeneratorSettings) -> Self {
        let biome = generator_settings.get_biome().clone();
        FlatLevelSource::new_with_biome_source(generator_settings, FixedBiomeSource::new(biome))
    }

    /// `FlatLevelSource(FlatLevelGeneratorSettings, BiomeSource)` — the
    /// CraftBukkit second constructor. The base's `Util.memoize(
    /// generatorSettings::adjustGenerationSettings)` memo defers with the flat
    /// unit's biome-value seam (RivetTodo #178); the shell does not run it.
    pub fn new_with_biome_source(
        generator_settings: FlatLevelGeneratorSettings,
        biome_source: FixedBiomeSource,
    ) -> Self {
        FlatLevelSource {
            settings: generator_settings,
            biome_source,
        }
    }

    /// `settings()`.
    pub fn settings(&self) -> &FlatLevelGeneratorSettings {
        &self.settings
    }

    /// `fillFromNoise(Blender, RandomState, StructureManager, ChunkAccess)` —
    /// the superflat block fill: write each non-null layer (up to the chunk
    /// height) as a full 16×16 slab, then `CompletableFuture.completedFuture(
    /// centerChunk)`.
    ///
    /// The `Blender`/`StructureManager` params are unused in the body (dropped,
    /// as in the ported `ChunkGenerator` seams). The two worldgen heightmaps are
    /// created once up front (Java's `getOrCreateHeightmapUnprimed` pair, before
    /// the loop), then each write routes through
    /// [`ProtoChunk::write_worldgen_block`] (the real worldgen block write the
    /// noisegen unit uses), which updates both heightmaps per block — see
    /// [`write_layer_block`]. Java's `if (blockState != null)` guard skips the
    /// `None` slots introduced by `adjustGenerationSettings` (RivetTodo #178's
    /// biome-value seam); no write or heightmap update happens for them.
    pub fn fill_from_noise<B, S>(
        &self,
        _blender: Blender,
        _random_state: &RandomState,
        center_chunk: &mut ProtoChunk<BlockState, B, S>,
    ) where
        B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        S: Eq + std::hash::Hash,
    {
        let layers = self.settings.get_layers();

        // Java creates the two worldgen heightmaps before the layer loop (the
        // `oceanFloor`/`worldSurface` locals); the per-write update below needs
        // them to exist.
        center_chunk.get_or_create_heightmap_unprimed(Types::OceanFloorWg);
        center_chunk.get_or_create_heightmap_unprimed(Types::WorldSurfaceWg);

        let min_y = center_chunk.get_min_y();
        let layer_count = i32::min(center_chunk.get_height(), layers.len() as i32);
        for layer_index in 0..layer_count {
            // Java `if (blockState != null)` — skip the nullable slots.
            let Some(state) = layers[layer_index as usize] else {
                continue;
            };
            let y = min_y.wrapping_add(layer_index);
            for x in 0..16 {
                for z in 0..16 {
                    write_layer_block(center_chunk, x, y, z, state);
                }
            }
        }
    }
}

/// `FlatLevelSource.fillFromNoise`'s per-block write — Java's
/// `centerChunk.setBlockState(blockPos.set(x, y, z), blockState)` followed by
/// the two worldgen heightmap `update`s. Routes through the worldgen block
/// write (see `below_zero_retrogen`'s `write_block` for the same seam). The
/// heightmaps are created once by `fillFromNoise` before the loop.
fn write_layer_block<B, S>(
    chunk: &mut ProtoChunk<BlockState, B, S>,
    x: i32,
    y: i32,
    z: i32,
    state: BlockState,
) where
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    let section_index = chunk.get_section_index(y);
    let predicates = block_state_predicates();
    chunk.write_worldgen_block(
        section_index,
        x & 15,
        y & 15,
        z & 15,
        y,
        state,
        &predicates.is_air,
        &predicates.is_randomly_ticking,
        &predicates.fluid_is_empty,
        &predicates.fluid_is_randomly_ticking,
        &predicates.is_special_colliding,
    );
}

/// `FlatLevelSource.CODEC` — the ops-generic
/// `flat_level_source_map_codec::<Ops>()` factory: the single
/// `FlatLevelGeneratorSettings.CODEC.fieldOf("settings")` context field,
/// `.stable()`.
///
/// The settings codec resolves its `RegistryOps.retrieveElement` context fields
/// (the plains biome fallback and the two lava lakes), so the factory requires
/// `RegistryOpsLookup` — the same bound as the flat settings codec.
pub fn flat_level_source_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<FlatLevelSource, Ops>> {
    let settings_field = codec::field_of(
        flat_level_generator_settings_codec::<Ops>(),
        "settings".to_string(),
    );
    map_codec::stable(record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|s: &FlatLevelSource| s.settings.clone()),
                settings_field,
            ))
            .apply(instance, Arc::new(FlatLevelSource::new))
    }))
}

impl ChunkGenerator for FlatLevelSource {
    fn get_min_y(&self) -> i32 {
        0
    }

    fn get_gen_depth(&self) -> i32 {
        384
    }

    /// `getSeaLevel()` — -63 (overridden from the base's panicking seam).
    fn get_sea_level(&self) -> i32 {
        -63
    }

    /// `getSpawnHeight(LevelHeightAccessor)` — `heightAccessor.getMinY() +
    /// Math.min(heightAccessor.getHeight(), settings.getLayers().size())`.
    fn get_spawn_height(&self, height_accessor: &dyn LevelHeightAccessor) -> i32 {
        height_accessor.get_min_y().wrapping_add(i32::min(
            height_accessor.get_height(),
            self.settings.get_layers().len() as i32,
        ))
    }

    /// `getBaseHeight(...)` — the topmost opaque, non-null layer's surface, or
    /// `minY`.
    ///
    /// Java's `state != null && type.isOpaque().test(state)` skips the `None`
    /// slots introduced by `adjustGenerationSettings` (RivetTodo #178's
    /// biome-value seam).
    fn get_base_height(
        &self,
        _x: i32,
        _z: i32,
        ty: Types,
        height_accessor: &dyn LevelHeightAccessor,
        _random_state: &RandomState,
    ) -> i32 {
        let layers = self.settings.get_layers();
        let mut layer_index = i32::min(layers.len() as i32 - 1, height_accessor.get_max_y());
        while layer_index >= 0 {
            // Java `state != null && type.isOpaque().test(state)`.
            if let Some(state) = layers[layer_index as usize]
                && Heightmap::is_opaque(ty, resolve_state_flags(&state))
            {
                return height_accessor
                    .get_min_y()
                    .wrapping_add(layer_index)
                    .wrapping_add(1);
            }
            layer_index -= 1;
        }
        height_accessor.get_min_y()
    }

    /// `getBaseColumn(...)` — `new NoiseColumn(minY, layers.limit(height)
    /// .map(state -> state == null ? AIR : state))`.
    fn get_base_column(
        &self,
        _x: i32,
        _z: i32,
        height_accessor: &dyn LevelHeightAccessor,
        _random_state: &RandomState,
    ) -> Option<(i32, Vec<BlockState>)> {
        let air = Blocks::AIR.default_block_state();
        let column: Vec<BlockState> = self
            .settings
            .get_layers()
            .iter()
            .take(height_accessor.get_height() as usize)
            .map(|state| state.unwrap_or(air))
            .collect();
        Some((height_accessor.get_min_y(), column))
    }

    /// `addDebugScreenInfo(...)` — the no-op override (Java's is empty).
    fn add_debug_screen_info(
        &self,
        _result: &mut Vec<String>,
        _random_state: &RandomState,
        _feet_pos: &BlockPos,
    ) {
    }

    fn get_biome_source(&self) -> &dyn BiomeSource {
        &self.biome_source
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::level_chunk_section::LevelChunkSection;
    use crate::chunk::storage::chunk_reconstruction::resolve_state_flags;
    use crate::chunk::storage::section_reconstruction::{
        BiomeId as SectionBiomeId, current_version_container_factory,
    };
    use crate::chunk::upgrade_data::UpgradeData;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::flat::flat_layer_info::FlatLayerInfo;
    use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
    use crate::levelgen::synth::normal_noise::NoiseParameters;
    use rivet_registry::Identifier;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::core::ChunkPos;
    use rivet_registry::holder::{Holder, RegistryId};
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry::Registry;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    use crate::block::blocks::Blocks;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A `RegistryAccess` registering `minecraft:plains` (as the id 40) in the
    /// biome registry, `air`/`stone`/`dirt` in the block registry, and the two
    /// lava lakes in the placed-feature registry — the registries the settings
    /// codec resolves through its ops (the `flat_level_generator_settings::tests`
    /// access pattern).
    fn access() -> RegistryAccess {
        let mut biomes_reg = RegistryBuilder::new(&*rivet_registry::registries::BIOME);
        biomes_reg.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BIOME,
                Identifier::parse("minecraft:plains"),
            ),
            Arc::new(BiomeId::from_id(40)),
            RegistrationInfo::BUILT_IN,
        );
        let biome_registry = biomes_reg.freeze();

        // Air-first registration order (air=0, stone=1, dirt=2) makes each
        // element id coincide with its generated `BlockId`, so decoded layer
        // references expand coherently through `get_block_state`.
        let mut blocks = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        for name in ["minecraft:air", "minecraft:stone", "minecraft:dirt"] {
            blocks.register(
                &ResourceKey::create(&*rivet_registry::registries::BLOCK, Identifier::parse(name)),
                Arc::new(rivet_registry::registries::BlockType),
                RegistrationInfo::BUILT_IN,
            );
        }
        let block_registry = blocks.freeze();

        let mut placed =
            RegistryBuilder::new(&*crate::biome::biome_generation_settings::PLACED_FEATURE);
        for name in [
            "minecraft:lake_lava_underground",
            "minecraft:lake_lava_surface",
        ] {
            placed.register(
                &ResourceKey::create(
                    &*crate::biome::biome_generation_settings::PLACED_FEATURE,
                    Identifier::parse(name),
                ),
                Arc::new(crate::levelgen::placement::PlacedFeature::new(
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

    /// The `plains` biome holder resolved through the test access (the flat
    /// settings constructors need a biome holder).
    fn plains_holder(access: &RegistryAccess) -> Holder<BiomeId> {
        use rivet_registry::HolderGetter;
        access
            .lookup::<BiomeId>(&*rivet_registry::registries::BIOME)
            .expect("biome registry")
            .get_or_throw(&crate::biome::biomes::PLAINS)
    }

    /// The block registry's real `RegistryId` (assigned by the global counter) —
    /// the reference-id the hand-built `FlatLayerInfo` holders carry.
    fn block_registry_id(access: &RegistryAccess) -> RegistryId {
        access
            .lookup::<rivet_registry::registries::BlockType>(&*rivet_registry::registries::BLOCK)
            .expect("block registry")
            .registry_id()
    }

    /// A `FlatLevelSource` whose settings expand the given per-layer infos (the
    /// flat presets' build pattern: `new` then `get_layers_info_mut` + `update_layers`).
    fn source(access: &RegistryAccess, layers: Vec<FlatLayerInfo>) -> FlatLevelSource {
        let mut settings = FlatLevelGeneratorSettings::new(None, plains_holder(access), Vec::new());
        for layer in layers {
            settings.get_layers_info_mut().push(layer);
        }
        settings.update_layers();
        FlatLevelSource::new(settings)
    }

    fn accessor() -> crate::level::height_accessor::SimpleLevelHeightAccessor {
        create_accessor(-64, 384)
    }

    /// The `noisegen` test pattern — a noise registry populated via
    /// `NoiseData.bootstrap` so `RandomState::create` can resolve its nine
    /// `Noises.*` keys (see `debug_level_source::tests`).
    fn populated_registries() -> (Registry<NoiseParameters>, Registry<DensityFunctionValue>) {
        use crate::data::worldgen::bootstrap_context::RecordingContext;
        use crate::data::worldgen::noise_data;
        use crate::levelgen::noise::registry_keys;
        use rivet_registry::RegistrationInfo;
        use rivet_registry::RegistryAccess;
        use rivet_registry::holder::RegistryId as Id;

        let noise_key = &registry_keys::NOISE;
        let mut noise_builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(noise_key);
        let mut noise_ctx = RecordingContext::<NoiseParameters>::new(
            Id(0),
            (*noise_key).clone(),
            RegistryAccess::empty(),
        );
        noise_data::bootstrap(&mut noise_ctx);
        for reg in noise_ctx.registrations() {
            noise_builder.register(
                &reg.key,
                Arc::new(reg.value.clone()),
                RegistrationInfo::BUILT_IN,
            );
        }
        let noise_registry = noise_builder.freeze();
        let df_key = &registry_keys::DENSITY_FUNCTION;
        let df_registry: Registry<DensityFunctionValue> = RegistryBuilder::new(df_key).freeze();
        (noise_registry, df_registry)
    }

    fn random_state<'a>(
        noise_registry: &'a Registry<NoiseParameters>,
        df_registry: &'a Registry<DensityFunctionValue>,
    ) -> RandomState<'a> {
        RandomState::create(
            &crate::levelgen::noisegen::noise_generator_settings::dummy(),
            noise_registry,
            df_registry,
            1234,
        )
    }

    /// A one-block-thick layer of `block` (the flat preset shape for the
    /// 1-layer tests).
    fn layer(access: &RegistryAccess, block: crate::block::Block) -> FlatLayerInfo {
        FlatLayerInfo::from_block(1, block, block_registry_id(access))
    }

    #[test]
    fn constants_and_surface_reads_match_java() {
        // Three one-block layers: bedrock at 0, dirt at 1, grass at 2.
        let access = access();
        let flat = source(
            &access,
            vec![
                layer(&access, Blocks::BEDROCK),
                layer(&access, Blocks::DIRT),
                layer(&access, Blocks::GRASS_BLOCK),
            ],
        );
        assert_eq!(flat.get_min_y(), 0);
        assert_eq!(flat.get_gen_depth(), 384);
        assert_eq!(flat.get_sea_level(), -63);

        let accessor = accessor();
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);

        // getSpawnHeight = minY + min(height, layers.size()) = -64 + 3.
        assert_eq!(flat.get_spawn_height(&accessor), -61);
        // getBaseHeight: the topmost opaque layer (index 2) -> minY + 3.
        assert_eq!(
            ChunkGenerator::get_base_height(
                &flat,
                0,
                0,
                Types::WorldSurfaceWg,
                &accessor,
                &random_state
            ),
            -61
        );
        // getBaseColumn: the three layers over the accessor's height.
        assert_eq!(
            ChunkGenerator::get_base_column(&flat, 0, 0, &accessor, &random_state),
            Some((
                -64,
                vec![
                    Blocks::BEDROCK.default_block_state(),
                    Blocks::DIRT.default_block_state(),
                    Blocks::GRASS_BLOCK.default_block_state(),
                ]
            ))
        );
    }

    #[test]
    fn get_base_height_skips_non_opaque_top_layers() {
        // Air on top of stone: the loop starts at the top (index 1 = air, not
        // opaque for WORLD_SURFACE_WG), so it falls to the stone at index 0 ->
        // minY + 1.
        let access = access();
        let flat = source(
            &access,
            vec![layer(&access, Blocks::STONE), layer(&access, Blocks::AIR)],
        );
        let accessor = accessor();
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        assert_eq!(
            ChunkGenerator::get_base_height(
                &flat,
                0,
                0,
                Types::WorldSurfaceWg,
                &accessor,
                &random_state
            ),
            -63
        );
    }

    #[test]
    fn get_spawn_height_is_min_y_for_empty_layers() {
        let access = access();
        let flat = source(&access, vec![]);
        assert_eq!(flat.get_spawn_height(&accessor()), -64);
    }

    #[test]
    fn fill_from_noise_writes_layers_and_primes_heightmaps() {
        // The overworld chunk shape (the noisegen `worldgen_proto` pattern).
        let factory = current_version_container_factory();
        let air = Blocks::AIR.default_block_state();
        let sections: Vec<LevelChunkSection<BlockState, SectionBiomeId>> = (0..24)
            .map(|_| {
                LevelChunkSection::new_all_air(
                    factory.create_for_block_states(),
                    factory.create_for_biomes(),
                )
            })
            .collect();
        let mut proto: ProtoChunk<BlockState, SectionBiomeId, &'static str> = ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create_accessor(-64, 384),
            &factory,
            Some(sections),
            air,
            air,
            &resolve_state_flags,
        );

        let access = access();
        let flat = source(
            &access,
            vec![
                layer(&access, Blocks::BEDROCK),
                layer(&access, Blocks::DIRT),
            ],
        );
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        flat.fill_from_noise(Blender::empty(), &random_state, &mut proto);

        // Layer 0 (bedrock) at y=-64, layer 1 (dirt) at y=-63; above stays air.
        let bedrock = Blocks::BEDROCK.default_block_state();
        let dirt = Blocks::DIRT.default_block_state();
        assert_eq!(proto.get_block_state(0, -64, 0), bedrock);
        assert_eq!(proto.get_block_state(15, -63, 15), dirt);
        assert_eq!(proto.get_block_state(0, -62, 0), air);

        // The worldgen heightmaps were primed and updated to the top of the
        // dirt layer: height (firstAvailable - 1) = -63.
        let min_y = -64;
        assert_eq!(
            proto
                .get_or_create_heightmap_unprimed(Types::WorldSurfaceWg)
                .get_height_at(0, 0, min_y),
            -63
        );
        assert_eq!(
            proto
                .get_or_create_heightmap_unprimed(Types::OceanFloorWg)
                .get_height_at(0, 0, min_y),
            -63
        );
    }

    #[test]
    fn codec_round_trips_through_the_real_settings_codec() {
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        let codec = map_codec::codec_of(flat_level_source_map_codec::<TestOps>());
        let encoded_in = json!({
            "settings": {
                "layers": [{ "height": 1, "block": "minecraft:stone" }],
                "biome": "minecraft:plains",
                "lakes": true,
                "features": false,
            }
        });
        let parsed = codec.parse(&ops, &encoded_in);
        let source = parsed.result().expect("decode should succeed");

        // The single `settings` field resolved through the flat settings codec.
        assert_eq!(source.settings().get_layers().len(), 1);
        assert_eq!(
            source.settings().get_layers()[0]
                .expect("no adjustGenerationSettings ran")
                .block()
                .id() as u32,
            1
        );
        assert!(source.settings().add_lakes());
        assert!(!source.settings().decoration());
        assert_eq!(source.get_min_y(), 0);
        assert_eq!(source.get_sea_level(), -63);

        // Encode round-trips the settings record back to its wire form.
        let encoded = codec
            .encode_start(&ops, source)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded["settings"]["layers"],
            encoded_in["settings"]["layers"]
        );
        assert_eq!(encoded["settings"]["biome"], json!("minecraft:plains"));
        assert_eq!(encoded["settings"]["lakes"], json!(true));
        assert_eq!(encoded["settings"]["features"], json!(false));
    }
}

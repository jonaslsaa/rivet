//! Port of `net.minecraft.world.level.levelgen.DebugLevelSource` (26.2) — the
//! `mc.world.level.levelgen.settings` unit.
//!
//! The debug-mode `ChunkGenerator`: a fixed `Biomes.PLAINS` biome source and a
//! static grid of *every possible block state* laid out on a `BARRIER` floor
//! (the debug world's 16×16 state grid). The `CODEC` is the single
//! `RegistryOps.retrieveElement(Biomes.PLAINS)` context field — a debug world
//! always resolves to the plains biome, and the codec writes no settings.
//!
//! `ALL_BLOCKS` is the full block-state enumeration Java builds from
//! `BuiltInRegistries.BLOCK` × `getStateDefinition().getPossibleStates()`. The
//! port reconstructs the same ordered list from the generated
//! `BLOCK_STATE_BASES` table (block registry order, then each block's
//! contiguous `[base, base+count)` state range — the mixed-radix
//! `buildStateList` order), so the debug grid layout matches Java.
//!
//! ### The `ChunkGenerator` trait
//!
//! `DebugLevelSource` extends the abstract `ChunkGenerator`; the port
//! implements the trait surface (`rivet-world::chunk::chunk_generator`) for
//! every method whose Java body is nameable here — the constants
//! (`getMinY`/`getGenDepth`/`getSeaLevel`), the two world-surface reads
//! (`getBaseHeight` → 0, `getBaseColumn` → the empty `NoiseColumn`), the
//! no-op `addDebugScreenInfo`, and `getBiomeSource`. The worldgen lifecycle
//! steps (`buildSurface`/`applyCarvers`/`spawnOriginalMobs`/`fillFromNoise`/
//! `createBiomes` — all empty in Java except `fillFromNoise`, which is the
//! `completedFuture(centerChunk)` pass-through) stay on the trait's default
//! panic seams: their real signatures take `WorldGenRegion`/
//! `StructureManager`/the generic `ChunkAccess`, which the owning
//! `mc.world.level.chunk.generator` realization provides (RivetTodo #185).
//!
//! `applyBiomeDecoration`'s two `WorldGenLevel.setBlock` writes defer with the
//! `#228` block-write surface; the loop structure is ported and the write is
//! an explicit seam (see [`set_block`]).

use crate::biome::biome_source::BiomeSource;
use crate::biome::biomes;
use crate::biome::fixed_biome_source::FixedBiomeSource;
use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::heightmap::Types;
use crate::levelgen::noisegen::random_state::RandomState;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, SectionPos};
use rivet_registry::generated::block_states::{BLOCK_STATE_BASES, BLOCK_STATE_COUNT, StateId};
use rivet_registry::holder::Holder;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth;
use std::sync::{Arc, LazyLock};

use crate::block::blocks::Blocks;

/// `DebugLevelSource.BLOCK_MARGIN` — the 2-block margin around the debug grid.
pub const BLOCK_MARGIN: i32 = 2;

/// `DebugLevelSource.ALL_BLOCKS` — every possible block state, in
/// `BuiltInRegistries.BLOCK` order × `getPossibleStates()` order (see the
/// module doc).
pub static ALL_BLOCKS: LazyLock<Vec<BlockState>> = LazyLock::new(|| {
    let mut all = Vec::with_capacity(BLOCK_STATE_COUNT as usize);
    for block in BLOCK_STATE_BASES.iter() {
        for local in 0..block.count {
            all.push(BlockState::new(StateId(block.base + local)));
        }
    }
    all
});

/// `DebugLevelSource.GRID_WIDTH` — `Mth.ceil(Mth.sqrt(ALL_BLOCKS.size()))`.
pub static GRID_WIDTH: LazyLock<i32> =
    LazyLock::new(|| mth::ceil(mth::sqrt(ALL_BLOCKS.len() as f32)));

/// `DebugLevelSource.GRID_HEIGHT` — `Mth.ceil((float)ALL_BLOCKS.size() /
/// GRID_WIDTH)`.
pub static GRID_HEIGHT: LazyLock<i32> =
    LazyLock::new(|| mth::ceil(ALL_BLOCKS.len() as f32 / *GRID_WIDTH as f32));

/// `DebugLevelSource.AIR` — `Blocks.AIR.defaultBlockState()`.
pub static AIR: LazyLock<BlockState> = LazyLock::new(|| Blocks::AIR.default_block_state());
/// `DebugLevelSource.BARRIER` — `Blocks.BARRIER.defaultBlockState()`.
pub static BARRIER: LazyLock<BlockState> = LazyLock::new(|| Blocks::BARRIER.default_block_state());

/// `DebugLevelSource.HEIGHT` — the debug grid's top surface.
pub const HEIGHT: i32 = 70;
/// `DebugLevelSource.BARRIER_HEIGHT` — the barrier floor.
pub const BARRIER_HEIGHT: i32 = 60;

/// `net.minecraft.world.level.levelgen.DebugLevelSource`.
pub struct DebugLevelSource {
    /// The `biomeSource` base field — the fixed plains biome source.
    biome_source: FixedBiomeSource,
}

impl DebugLevelSource {
    /// `new DebugLevelSource(Holder.Reference<Biome> plains)` — `super(new
    /// FixedBiomeSource(plains))`.
    pub fn new(plains: Holder<BiomeId>) -> Self {
        DebugLevelSource {
            biome_source: FixedBiomeSource::new(plains),
        }
    }

    /// `DebugLevelSource.getBlockStateFor(int worldX, int worldZ)` — the
    /// debug grid's block state at the world column.
    pub fn get_block_state_for(mut world_x: i32, mut world_z: i32) -> BlockState {
        let mut state = *AIR;
        if world_x > 0 && world_z > 0 && world_x % 2 != 0 && world_z % 2 != 0 {
            world_x /= 2;
            world_z /= 2;
            if world_x <= *GRID_WIDTH && world_z <= *GRID_HEIGHT {
                let index = mth::abs_i32(world_x.wrapping_mul(*GRID_WIDTH).wrapping_add(world_z));
                if (index as usize) < ALL_BLOCKS.len() {
                    state = ALL_BLOCKS[index as usize];
                }
            }
        }
        state
    }

    /// `applyBiomeDecoration(WorldGenLevel, ChunkAccess, StructureManager)` —
    /// lays the barrier floor at `y=60` and the debug grid surface at `y=70`.
    ///
    /// The `StructureManager` parameter is unused in the body (dropped, as in
    /// the ported `ChunkGenerator` seams). The two `WorldGenLevel.setBlock`
    /// writes defer with the `#228` block-write surface (see [`set_block`]);
    /// the grid layout is ported faithfully.
    pub fn apply_biome_decoration<B, S>(
        &self,
        level: &mut dyn WorldGenLevel,
        chunk: &ChunkAccess<BlockState, B, S>,
    ) where
        B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        S: Eq + std::hash::Hash,
    {
        let center_pos = chunk.get_pos();
        let chunk_x = center_pos.x();
        let chunk_z = center_pos.z();

        for x in 0..16 {
            for z in 0..16 {
                let world_x = SectionPos::section_to_block_coord_offset(chunk_x, x);
                let world_z = SectionPos::section_to_block_coord_offset(chunk_z, z);
                set_block(
                    level,
                    &BlockPos::new(world_x, BARRIER_HEIGHT, world_z),
                    *BARRIER,
                );
                let state = DebugLevelSource::get_block_state_for(world_x, world_z);
                set_block(level, &BlockPos::new(world_x, HEIGHT, world_z), state);
            }
        }
    }
}

/// The `WorldGenLevel.setBlock(BlockPos, BlockState, int flags)` seam —
/// Java's `DebugLevelSource.applyBiomeDecoration` writes the barrier floor and
/// the grid surface through it. The `#228` block-write surface is not ported,
/// so the write fails explicitly rather than fabricate a block state.
///
/// `Block.UPDATE_CLIENTS` (the `flags` argument) is a `BlockBehaviour` bit
/// flag that also defers with the block-behavior surface; the seam drops it.
fn set_block(_level: &mut dyn WorldGenLevel, _pos: &BlockPos, _state: BlockState) {
    panic!("WorldGenLevel.setBlock is not implemented (RivetTodo #228)")
}

/// `DebugLevelSource.CODEC` — the ops-generic
/// `debug_level_source_map_codec::<Ops>()` factory: the single
/// `RegistryOps.retrieveElement(Biomes.PLAINS)` context field, `.stable()`.
pub fn debug_level_source_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<DebugLevelSource, Ops>> {
    map_codec::stable(record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|s: &DebugLevelSource| s.biome_source.biome.clone()),
                rivet_registry::registry_ops::retrieve_element(&biomes::PLAINS),
            ))
            .apply(instance, Arc::new(DebugLevelSource::new))
    }))
}

impl ChunkGenerator for DebugLevelSource {
    fn get_min_y(&self) -> i32 {
        0
    }

    fn get_gen_depth(&self) -> i32 {
        384
    }

    /// `getSeaLevel()` — 63 (overridden from the base's panicking seam).
    fn get_sea_level(&self) -> i32 {
        63
    }

    /// `getBaseHeight(...)` — 0.
    fn get_base_height(
        &self,
        _x: i32,
        _z: i32,
        _ty: Types,
        _height_accessor: &dyn LevelHeightAccessor,
        _random_state: &RandomState,
    ) -> i32 {
        0
    }

    /// `getBaseColumn(...)` — `new NoiseColumn(0, new BlockState[0])` (the
    /// `(clamped_min_y, Vec)` seam shape).
    fn get_base_column(
        &self,
        _x: i32,
        _z: i32,
        _height_accessor: &dyn LevelHeightAccessor,
        _random_state: &RandomState,
    ) -> Option<(i32, Vec<BlockState>)> {
        Some((0, Vec::new()))
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
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
    use crate::levelgen::synth::normal_noise::NoiseParameters;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::core::{BlockPos, ChunkPos};

    /// The overworld debug chunk shape (`applyBiomeDecoration` receives): 24
    /// all-air sections over the `-64..=319` accessor (the noisegen
    /// `worldgen_proto` pattern, built as the Java `ChunkAccess` parameter).
    fn debug_chunk() -> ChunkAccess<BlockState, SectionBiomeId, &'static str> {
        let factory = current_version_container_factory();
        let sections: Vec<LevelChunkSection<BlockState, SectionBiomeId>> = (0..24)
            .map(|_| {
                LevelChunkSection::new_all_air(
                    factory.create_for_block_states(),
                    factory.create_for_biomes(),
                )
            })
            .collect();
        ChunkAccess::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            create_accessor(-64, 384),
            &factory,
            0,
            Some(sections),
            &resolve_state_flags,
        )
    }

    /// The minimal `WorldGenLevel` the decoration writes against (the
    /// `setBlock` seam never reaches it — the write panics first).
    struct MockLevel {
        min_y: i32,
        height: i32,
    }

    impl LevelHeightAccessor for MockLevel {
        fn get_height(&self) -> i32 {
            self.height
        }
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
    }

    impl WorldGenLevel for MockLevel {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            *AIR
        }
    }

    #[test]
    fn all_blocks_matches_the_state_table_and_grid_dims() {
        // `ALL_BLOCKS` is the full dense state table (Java's registry ×
        // possible-states enumeration).
        assert_eq!(ALL_BLOCKS.len(), BLOCK_STATE_COUNT as usize);
        // `GRID_WIDTH = ceil(sqrt(n))`, `GRID_HEIGHT = ceil(n / GRID_WIDTH)`.
        assert_eq!(*GRID_WIDTH, mth::ceil(mth::sqrt(BLOCK_STATE_COUNT as f32)));
        assert_eq!(
            *GRID_HEIGHT,
            mth::ceil(BLOCK_STATE_COUNT as f32 / *GRID_WIDTH as f32)
        );
        assert!(*GRID_WIDTH * *GRID_HEIGHT >= BLOCK_STATE_COUNT as i32);
    }

    #[test]
    fn all_blocks_are_the_dense_state_table_in_registry_order() {
        // Java's `ALL_BLOCKS` is `BuiltInRegistries.BLOCK` (registry order) ×
        // `getPossibleStates()` (the mixed-radix state order) — exactly the
        // dense global state table. `ALL_BLOCKS[i].id() == i` is the
        // construction; the independent checks are the mid-list block
        // identities (against the `Blocks` constants, a separate table) and
        // the monotonic, contiguous block ownership (a transposed base table
        // would break it).
        for (i, state) in ALL_BLOCKS.iter().enumerate() {
            assert_eq!(state.id().0, i as u16, "ALL_BLOCKS[{i}] out of sequence");
        }
        // Mid-list identities: air/stone at 0/1, grass_block's two states at
        // 8/9 (base 8, count 2), dirt at 10 (the next base).
        assert_eq!(ALL_BLOCKS[0].block(), Blocks::AIR.id());
        assert_eq!(ALL_BLOCKS[1].block(), Blocks::STONE.id());
        assert_eq!(ALL_BLOCKS[8].block(), Blocks::GRASS_BLOCK.id());
        assert_eq!(ALL_BLOCKS[9].block(), Blocks::GRASS_BLOCK.id());
        assert_eq!(ALL_BLOCKS[10].block(), Blocks::DIRT.id());
        // Block ownership is monotonic with contiguous per-block runs.
        for pair in ALL_BLOCKS.windows(2) {
            let (a, b) = (pair[0].block().0, pair[1].block().0);
            assert!(b >= a, "block ownership regressed: {b} < {a}");
        }
    }

    #[test]
    fn get_block_state_for_is_air_outside_the_grid() {
        // Not both positive.
        assert_eq!(DebugLevelSource::get_block_state_for(0, 5), *AIR);
        assert_eq!(DebugLevelSource::get_block_state_for(5, 0), *AIR);
        // Even coordinate -> outside the state grid.
        assert_eq!(DebugLevelSource::get_block_state_for(2, 5), *AIR);
        assert_eq!(DebugLevelSource::get_block_state_for(5, 2), *AIR);
        // Far beyond the grid -> the index test fails -> AIR.
        assert_eq!(DebugLevelSource::get_block_state_for(1001, 1001), *AIR);
    }

    #[test]
    fn get_block_state_for_reads_the_grid() {
        // `(3, 3)` halves to grid cell `(1, 1)` -> `abs(1 * GRID_WIDTH + 1)`.
        let index = mth::abs_i32(*GRID_WIDTH + 1);
        assert!(index < ALL_BLOCKS.len() as i32);
        assert_eq!(
            DebugLevelSource::get_block_state_for(3, 3),
            ALL_BLOCKS[index as usize]
        );
        // The first cell `(1, 1)` is the air default state (block 0, state 0).
        assert_eq!(DebugLevelSource::get_block_state_for(1, 1), ALL_BLOCKS[0]);
    }

    #[test]
    fn constants_and_surface_reads_match_java() {
        assert_eq!(HEIGHT, 70);
        assert_eq!(BARRIER_HEIGHT, 60);
        assert_eq!(BLOCK_MARGIN, 2);
        let source = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        assert_eq!(source.get_min_y(), 0);
        assert_eq!(source.get_gen_depth(), 384);
        assert_eq!(source.get_sea_level(), 63);
        let accessor = crate::level::height_accessor::create(-64, 384);
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        assert_eq!(
            ChunkGenerator::get_base_height(
                &source,
                0,
                0,
                Types::WorldSurfaceWg,
                &accessor,
                &random_state
            ),
            0
        );
        assert_eq!(
            ChunkGenerator::get_base_column(&source, 0, 0, &accessor, &random_state),
            Some((0, Vec::new()))
        );
    }

    #[test]
    fn apply_biome_decoration_write_is_the_228_seam() {
        // The grid layout is ported; the `setBlock` write fails explicitly with
        // the #228 seam rather than fabricate a state.
        let source = DebugLevelSource::new(Holder::direct(BiomeId::from_id(40)));
        let chunk = debug_chunk();
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut level = MockLevel {
                min_y: -64,
                height: 384,
            };
            source.apply_biome_decoration(&mut level, &chunk);
        }));
        let message = panic_message(panic_result);
        assert!(
            message.contains("WorldGenLevel.setBlock") && message.contains("RivetTodo #228"),
            "set_block seam must name the #228 deferral, got: {message}"
        );
    }

    fn panic_message<T>(result: std::thread::Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-str panic payload>")
                .to_string(),
        }
    }

    /// The `noisegen` test pattern — a noise registry populated via
    /// `NoiseData.bootstrap` so `RandomState::create` can resolve its nine
    /// `Noises.*` keys.
    fn populated_registries() -> (
        rivet_registry::Registry<NoiseParameters>,
        rivet_registry::Registry<DensityFunctionValue>,
    ) {
        use crate::data::worldgen::bootstrap_context::RecordingContext;
        use crate::data::worldgen::noise_data;
        use crate::levelgen::noise::registry_keys;
        use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
        use crate::levelgen::synth::normal_noise::NoiseParameters;
        use rivet_registry::RegistrationInfo;
        use rivet_registry::RegistryAccess;
        use rivet_registry::RegistryBuilder;
        use rivet_registry::holder::RegistryId;

        let noise_key = &registry_keys::NOISE;
        let mut noise_builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(noise_key);
        let mut noise_ctx = RecordingContext::<NoiseParameters>::new(
            RegistryId(0),
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
        let df_registry: rivet_registry::Registry<DensityFunctionValue> =
            RegistryBuilder::new(df_key).freeze();
        (noise_registry, df_registry)
    }

    fn random_state<'a>(
        noise_registry: &'a rivet_registry::Registry<NoiseParameters>,
        df_registry: &'a rivet_registry::Registry<DensityFunctionValue>,
    ) -> RandomState<'a> {
        RandomState::create(
            &crate::levelgen::noisegen::noise_generator_settings::dummy(),
            noise_registry,
            df_registry,
            1234,
        )
    }

    /// `DebugLevelSource.CODEC` — the single `RegistryOps.retrieveElement(
    /// Biomes.PLAINS)` context field, `.stable()`. The wire form is an empty
    /// map (the element is fetched from the registry context, not written); a
    /// decode resolves the plains biome through the ops access and reconstructs
    /// the fixed source, and the encode round-trips to the same empty map.
    #[test]
    fn codec_round_trips_the_plains_element_through_the_registry() {
        use rivet_registry::HolderGetter;
        use rivet_registry::RegistrationInfo;
        use rivet_registry::RegistryBuilder;
        use rivet_registry::ResourceKey;
        use rivet_registry::access::RegistryAccess;
        use rivet_registry::identifier::Identifier;
        use rivet_registry::registry_ops::RegistryOps;
        use rivet_serialization::json_ops::JsonOps;
        use serde_json::json;

        let mut biomes_reg = RegistryBuilder::new(&*rivet_registry::registries::BIOME);
        biomes_reg.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BIOME,
                Identifier::parse("minecraft:plains"),
            ),
            Arc::new(BiomeId::from_id(40)),
            RegistrationInfo::BUILT_IN,
        );
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome")),
            Box::new(biomes_reg.freeze()) as rivet_registry::root::AnyBox,
        )]);
        // The plains holder the decode must reconstruct (resolved from the
        // access before the ops consumes it).
        let plains = access
            .lookup::<BiomeId>(&*rivet_registry::registries::BIOME)
            .expect("biome registry")
            .get_or_throw(&biomes::PLAINS);

        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = map_codec::codec_of(debug_level_source_map_codec::<
            RegistryOps<serde_json::Value, JsonOps>,
        >());

        let encoded_in = json!({});
        let parsed = codec.parse(&ops, &encoded_in);
        let source = parsed.result().expect("decode should succeed");

        // The fixed plains biome source resolves through the registry context.
        assert_eq!(
            source.get_biome_source().collect_possible_biomes(),
            vec![plains]
        );

        // Encode round-trips to the empty wire form.
        let encoded = codec
            .encode_start(&ops, source)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, encoded_in);
    }
}

//! Actual private Paper global-spawn body compared against canonical loaded
//! native chunks, not a mock terrain/height/collision implementation.
use super::*;
use crate::server::level::{
    MissingChunkPolicy, OverworldGenerator, ServerLevel, ServerLevelConfig,
    level_chunk::{LevelChunk, StructureKey},
};
use rivet_registry::{
    Identifier,
    block_state_properties::{BlockStateProperties, SlabType},
    generated::blocks::BlockId,
};
use rivet_world::{
    block::blocks::Blocks,
    chunk::{
        proto_chunk::ProtoChunk,
        status::ChunkStatus,
        storage::{
            chunk_reconstruction::resolve_state_flags,
            section_reconstruction::{BiomeId, current_version_container_factory},
        },
        upgrade_data::UpgradeData,
    },
    level::{height_accessor, storage::level_data::default_respawn_data},
};

fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).unwrap())
}
fn chunk(pos: ChunkPos, climate: ChunkPos, terrain: &str) -> LevelChunk {
    let accessor = height_accessor::create(-64, 384);
    let mut proto: ProtoChunk<BlockState, BiomeId, StructureKey> = ProtoChunk::new(
        pos,
        UpgradeData::empty(24),
        accessor,
        &current_version_container_factory(),
        None,
        Blocks::AIR.default_block_state(),
        block("minecraft:void_air"),
        &resolve_state_flags,
    );
    let skipped = terrain == "void"
        || terrain == "skip_two"
            && (pos == climate || pos == ChunkPos::new(climate.x() + 1, climate.z()));
    if !skipped {
        for x in 0..16 {
            for z in 0..16 {
                if terrain == "late_column" && !((x == 2 && z == 3) || (x == 3 && z == 0)) {
                    continue;
                }
                let state = match terrain {
                    "water" => Blocks::WATER.default_block_state(),
                    "bottom_slab" => block("minecraft:stone_slab"),
                    "top_slab" => block("minecraft:stone_slab")
                        .set_value(BlockStateProperties::SLAB_TYPE, SlabType::Top)
                        .unwrap(),
                    "shulker" => block("minecraft:shulker_box"),
                    _ => Blocks::STONE.default_block_state(),
                };
                proto.set_block_state(
                    pos.get_min_block_x() + x,
                    62,
                    pos.get_min_block_z() + z,
                    Blocks::STONE.default_block_state(),
                );
                proto.set_block_state(
                    pos.get_min_block_x() + x,
                    63,
                    pos.get_min_block_z() + z,
                    state,
                );
            }
        }
    }
    proto.base_mut().prime_heightmaps(&[
        Types::MotionBlocking,
        Types::WorldSurface,
        Types::OceanFloor,
        Types::MotionBlockingNoLeaves,
    ]);
    proto.set_persisted_status(ChunkStatus::Spawn);
    LevelChunk::from_generated_spawn_proto(proto).unwrap()
}
fn world(seed: i64) -> ServerLevel {
    let respawn_data = default_respawn_data();
    let config = ServerLevelConfig {
        seed,
        is_flat: false,
        sea_level: 63,
        missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
        spawn_chunk: ChunkPos::containing(&respawn_data.pos()),
        respawn_data,
        ..ServerLevelConfig::default()
    };
    let mut world = ServerLevel::new_region_backed(config);
    let root = tempfile::tempdir().unwrap();
    let environment = GenerationEnvironment::load(
        root.path(),
        &Identifier::parse("minecraft:overworld"),
        &rivet_world::flag::default_flags(),
        0,
    )
    .unwrap();
    world.install_generation_environment(environment).unwrap();
    world
}

#[test]
fn actual_paper_global_spawn_matches_real_loaded_chunk_search() {
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/initial-spawn.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let seed = row["seed"].as_i64().unwrap();
        let terrain = row["terrain"].as_str().unwrap();
        let generator = OverworldGenerator::new(seed);
        let mut world = world(seed);
        let climate: [i32; 3] = serde_json::from_value(row["climate"].clone()).unwrap();
        let climate_position = BlockPos::new(climate[0], climate[1], climate[2]);
        let center = ChunkPos::containing(&climate_position);
        for x in -5..=5 {
            for z in -5..=5 {
                let pos = ChunkPos::new(center.x() + x, center.z() + z);
                world
                    .chunk_map_mut()
                    .install(pos, chunk(pos, center, terrain));
            }
        }
        let outcome = world
            .initialize_spawn_from_loaded_chunks(
                &generator,
                generator.random_state().sampler(),
                None,
            )
            .unwrap();
        let expected: [i32; 3] = serde_json::from_value(row["spawn"].clone()).unwrap();
        assert_eq!(outcome.climate_position, climate_position, "{line}");
        assert_eq!(
            outcome.position,
            BlockPos::new(expected[0], expected[1], expected[2]),
            "{line}"
        );
        assert_eq!(world.get_respawn_data().pos(), outcome.position);
        assert_eq!(
            outcome.searched_chunks as u64,
            row["queried_chunks"].as_u64().unwrap(),
            "{line}"
        );
        assert_eq!(
            outcome.used_fallback,
            row["writes"].as_array().unwrap().len() == 1
        );
    }
}

#[test]
fn missing_full_and_unknown_dynamic_shape_refuse_without_fake_success() {
    use rivet_world::level::block_shape_context::DetachedShapeContext;
    let generator = OverworldGenerator::new(42);
    let climate = generator.random_state().sampler().find_spawn_position();
    let center = ChunkPos::containing(&climate);
    let mut missing = world(42);
    assert_eq!(
        missing.initialize_spawn_from_loaded_chunks(
            &generator,
            generator.random_state().sampler(),
            None
        ),
        Err(InitialSpawnError::MissingFullChunk(center))
    );
    assert_eq!(
        missing.get_respawn_data().pos(),
        BlockPos::new(
            center.get_min_block_x() + 8,
            64,
            center.get_min_block_z() + 8
        ),
        "source fallback remains written on failure, not returned as success"
    );
    let mut dynamic = world(42);
    dynamic
        .chunk_map_mut()
        .install(center, chunk(center, center, "shulker"));
    assert!(matches!(
        dynamic.initialize_spawn_from_loaded_chunks(
            &generator,
            generator.random_state().sampler(),
            None
        ),
        Err(InitialSpawnError::Shape {
            reason: ShapeQueryError::DynamicShapeContextUnavailable,
            ..
        })
    ));
    let mut context = DetachedShapeContext::default();
    assert!(
        matches!(
            dynamic.initialize_spawn_from_loaded_chunks(
                &generator,
                generator.random_state().sampler(),
                Some(&mut context)
            ),
            Err(InitialSpawnError::Shape {
                reason: ShapeQueryError::DynamicShapeContextMissing { .. },
                ..
            })
        ),
        "missing context must not become a full shulker fallback"
    );
}

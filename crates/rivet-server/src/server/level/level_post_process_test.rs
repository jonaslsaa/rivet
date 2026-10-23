//! Differential traversal tests. Java controls only storage/setBlock and the
//! pending lookup boundary; Rust additionally executes actual chest promotion.
use super::*;
use crate::server::level::{
    chunk_map::ChunkMap,
    generated_world::OverworldGenerator,
    level_block_entity_runtime::{LevelBlockEntityRuntime, NativeTickerKind},
    level_chunk::LevelChunk,
    server_level::ServerLevelConfig,
    world_runtime_settings::WorldRuntimeSettings,
};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{
    block_state_properties::BlockStateProperties,
    core::Direction,
    generated::block_behaviors::{BEHAVIOR_FLAG_RANDOM_TICKING, behavior_of},
};
use rivet_util::random::ThreadUnsafeRandom;
use rivet_world::chunk::{proto_chunk::ProtoChunk, status::ChunkStatus};
use serde_json::{Value, json};

#[derive(Default)]
struct Services {
    writes: Vec<Value>,
    added: Vec<BlockPos>,
    fail_at: usize,
}
impl BlockEntityWorldServices for Services {
    fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<(), BlockEntityTickerError> {
        self.added.push(pos);
        Ok(())
    }
    fn require_ticker_body(&self, _: NativeTickerKind) -> Result<(), BlockEntityTickerError> {
        panic!("chests have no ticker")
    }
    fn tick_body(
        &mut self,
        _: &mut LevelBlockEntityRuntime,
        _: &mut ChunkMap,
        _: &mut ThreadUnsafeRandom,
        _: BlockPos,
        _: NativeTickerKind,
    ) -> Result<(), BlockEntityTickerError> {
        panic!("no ticker execution")
    }
    fn report_tick_exception(
        &mut self,
        _: BlockPos,
        _: &str,
    ) -> Result<(), BlockEntityTickerError> {
        panic!("no ticker execution")
    }
    fn mid_tick_tasks(&mut self) -> Result<(), BlockEntityTickerError> {
        panic!("no ticker execution")
    }
}
impl GenerationPostProcessServices for Services {
    fn fluid_tick(
        &mut self,
        _: &mut ServerLevel,
        _: BlockPos,
        _: BlockState,
    ) -> Result<(), GenerationPostProcessError> {
        Err(GenerationPostProcessError::Unavailable(
            "actual fluid handler absent in dry fixture".into(),
        ))
    }
    fn liquid_block_tick(
        &mut self,
        _: &mut ServerLevel,
        _: BlockPos,
        _: BlockState,
    ) -> Result<(), GenerationPostProcessError> {
        panic!("dry fixture")
    }
    fn set_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Result<bool, GenerationPostProcessError> {
        if self.fail_at > 0 && self.writes.len() + 1 == self.fail_at {
            return Err(GenerationPostProcessError::Unavailable(
                "source write sentinel".into(),
            ));
        }
        self.writes.push(json!([
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            state.id().0,
            flags
        ]));
        // Controlled canonical storage, equivalent to the Java fixture's
        // setBlock override; no claim of lighting/events/full Level.setBlock.
        let chunk = level
            .chunk_map
            .get_chunk_mut(ChunkPos::containing(&pos))
            .unwrap();
        let base = chunk.generation_base_mut();
        let section = ((pos.get_y() - base.get_min_y()) >> 4) as usize;
        base.get_section_mut(section).set_block_state(
            pos.get_x() & 15,
            pos.get_y() & 15,
            pos.get_z() & 15,
            state.id(),
            &|id| BlockState::new(*id).is_air(),
            &|id| behavior_of(*id) & BEHAVIOR_FLAG_RANDOM_TICKING != 0,
            &|id| BlockState::new(*id).fluid_empty(),
            &|_| false,
            &|_| false,
        );
        Ok(true)
    }
}
fn block(name: &str) -> BlockState {
    Block::from_name(&format!("minecraft:{name}"))
        .unwrap()
        .default_block_state()
}
fn fixture(row: &Value) -> (ServerLevel, Services) {
    static GENERATOR: std::sync::LazyLock<std::sync::Arc<OverworldGenerator>> =
        std::sync::LazyLock::new(|| std::sync::Arc::new(OverworldGenerator::new(42)));
    let mut proto = GENERATOR
        .create_holder(ChunkPos::ZERO)
        .into_state()
        .into_loan()
        .0;
    let name = row["name"].as_str().unwrap();
    let count = row["pending_count"].as_u64().unwrap() as i32;
    if count > 0 {
        for index in 0..count {
            let pos = BlockPos::new(
                (index * 7) & 15,
                64 + (index >> 4),
                ((index * 11) >> 4) & 15,
            );
            proto.set_block_state(pos.get_x(), pos.get_y(), pos.get_z(), block("chest"));
            let mut tag = CompoundTag::new();
            tag.put_string("id", "DUMMY");
            tag.put_int("x", pos.get_x());
            tag.put_int("y", pos.get_y());
            tag.put_int("z", pos.get_z());
            proto.base_mut().set_block_entity_nbt(tag);
        }
    } else {
        let a = BlockPos::new(8, 64, 8);
        let state = if name == "fence" {
            block("oak_fence")
        } else {
            block("cocoa")
                .set_value(BlockStateProperties::HORIZONTAL_FACING, Direction::North)
                .unwrap()
        };
        proto.set_block_state(8, 64, 8, state);
        if name == "fence" {
            proto.set_block_state(7, 64, 8, block("stone"));
        }
        let packed = ProtoChunk::<
            BlockState,
            rivet_world::chunk::storage::section_reconstruction::BiomeId,
            rivet_registry::Identifier,
        >::pack_offset_coordinates(&a);
        proto.base_mut().add_packed_post_process(&[packed], 8);
        if name == "duplicate" {
            proto.base_mut().add_packed_post_process(&[packed], 8);
        }
        if name == "write_failure" {
            proto.set_block_state(9, 64, 8, state);
            let b = BlockPos::new(9, 64, 8);
            let packed = ProtoChunk::<
                BlockState,
                rivet_world::chunk::storage::section_reconstruction::BiomeId,
                rivet_registry::Identifier,
            >::pack_offset_coordinates(&b);
            proto.base_mut().add_packed_post_process(&[packed], 8);
        }
    }
    proto.set_persisted_status(ChunkStatus::Spawn);
    let mut chunk = LevelChunk::from_generated_spawn_proto(proto).unwrap();
    chunk.set_loaded(true);
    let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
    level.runtime_settings = Some(WorldRuntimeSettings::from_yaml("", "", "", "fixture").unwrap());
    level.chunk_map.install(ChunkPos::ZERO, chunk);
    (
        level,
        Services {
            fail_at: if name == "write_failure" { 2 } else { 0 },
            ..Default::default()
        },
    )
}
#[test]
fn actual_paper_post_processing_traversal_flags_pending_order_and_failure_retention() {
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/chunk-post-process.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        let (mut level, mut services) = fixture(&row);
        let result = level.post_process_generation(ChunkPos::ZERO, &mut services);
        assert_eq!(result.is_err(), row["failed"].as_bool().unwrap(), "{line}");
        assert_eq!(json!(services.writes), row["writes"], "{line}");
        assert_eq!(
            json!(
                services
                    .added
                    .iter()
                    .map(|p| [p.get_x(), p.get_y(), p.get_z()])
                    .collect::<Vec<_>>()
            ),
            row["lookups"],
            "{line}"
        );
        let chunk = level.chunk_map.get_chunk(ChunkPos::ZERO).unwrap();
        assert_eq!(
            chunk.is_post_processing_done(),
            row["done"].as_bool().unwrap(),
            "{line}"
        );
        assert_eq!(
            json!(
                chunk
                    .post_processing()
                    .iter()
                    .map(Vec::len)
                    .collect::<Vec<_>>()
            ),
            row["sections"],
            "{line}"
        );
        assert_eq!(
            chunk.pending_block_entities().len() as u64,
            row["remaining_pending"].as_u64().unwrap(),
            "{line}"
        );
        assert_eq!(
            chunk.block_entities().len(),
            row["pending_count"].as_u64().unwrap() as usize,
            "pending lookup executed actual native chest promotion"
        );
        count += 1;
    }
    assert_eq!(count, 8);
}
#[test]
fn unavailable_fluid_body_keeps_original_postprocess_entries_and_done_false() {
    let row = json!({"name":"cocoa","pending_count":0});
    let (mut level, mut services) = fixture(&row);
    let pos = BlockPos::new(8, 64, 8);
    // Controlled write only to establish the initial actual fluid state.
    services
        .set_block(&mut level, pos, block("water"), 276)
        .unwrap();
    services.writes.clear();
    assert!(matches!(
        level.post_process_generation(ChunkPos::ZERO, &mut services),
        Err(GenerationPostProcessError::Unavailable(_))
    ));
    let chunk = level.chunk_map.get_chunk(ChunkPos::ZERO).unwrap();
    assert_eq!(chunk.post_processing()[8].len(), 1);
    assert!(!chunk.is_post_processing_done());
    assert_eq!(chunk.get_block_state(8, 64, 8), block("water").id());
    assert!(services.writes.is_empty());
}

#[test]
fn live_level_bounds_return_void_air_without_resolving_missing_chunks() {
    let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
    let mut services = Services::default();
    let min_y = level.get_min_y();
    let max_y = min_y + level.get_height() - 1;
    let view = LivePostProcessLevel {
        level: &mut level,
        services: &mut services,
        ticking_enabled: true,
    };
    for pos in [
        BlockPos::new(0, min_y - 1, 0),
        BlockPos::new(0, max_y + 1, 0),
        BlockPos::new(i32::MAX, 64, 0),
        BlockPos::new(i32::MIN, 64, 0),
        BlockPos::new(0, 64, i32::MAX),
        BlockPos::new(0, 64, i32::MIN),
    ] {
        assert_eq!(view.get_block_state(&pos), block("void_air"), "{pos:?}");
    }
    // Valid positions still require their canonical chunk. Level's validity
    // predicate is deliberately different from isInWorldBoundsHorizontal.
    for pos in [
        BlockPos::new(0, min_y, 0),
        BlockPos::new(0, max_y, 0),
        BlockPos::new(30_000_000, 64, 0),
    ] {
        if ChunkPos::containing(&pos).is_valid() {
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                view.get_block_state(&pos)
            }))
            .unwrap_err();
            assert!(matches!(
                panic.downcast_ref::<GenerationPostProcessError>(),
                Some(GenerationPostProcessError::MissingChunk(_))
            ));
        }
    }
}

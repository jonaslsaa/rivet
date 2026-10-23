use super::*;
use crate::server::{
    level::{
        chunk_map::ChunkMap,
        generated_world::OverworldGenerator,
        level_block_entity_runtime::{LevelBlockEntityRuntime, NativeTickerKind},
        level_chunk::LevelChunk,
        server_level::ServerLevelConfig,
    },
    lighting::LiveLightRuntime,
};
use rivet_registry::generated::block_states::StateId;
use rivet_util::random::ThreadUnsafeRandom;
use rivet_world::{chunk::status::ChunkStatus, level::height_accessor::create};
use serde_json::{Value, json};

struct Services {
    tick_thread: std::thread::ThreadId,
}
impl Default for Services {
    fn default() -> Self {
        Self {
            tick_thread: std::thread::current().id(),
        }
    }
}
impl BlockEntityWorldServices for Services {
    fn on_block_entity_added(&mut self, _: BlockPos) -> Result<(), BlockEntityTickerError> {
        panic!("no entities in these source cases")
    }
    fn require_ticker_body(&self, _: NativeTickerKind) -> Result<(), BlockEntityTickerError> {
        panic!("no entities")
    }
    fn tick_body(
        &mut self,
        _: &mut LevelBlockEntityRuntime,
        _: &mut ChunkMap,
        _: &mut ThreadUnsafeRandom,
        _: BlockPos,
        _: NativeTickerKind,
    ) -> Result<(), BlockEntityTickerError> {
        panic!("not ticking")
    }
    fn report_tick_exception(
        &mut self,
        _: BlockPos,
        _: &str,
    ) -> Result<(), BlockEntityTickerError> {
        panic!("not ticking")
    }
    fn mid_tick_tasks(&mut self) -> Result<(), BlockEntityTickerError> {
        panic!("not ticking")
    }
}
impl LiveBlockMutationServices for Services {
    fn check_block_place_thread(&self) -> Result<(), LiveBlockMutationError> {
        if self.tick_thread == std::thread::current().id() {
            Ok(())
        } else {
            Err(LiveBlockMutationError::Unavailable(
                "not the retained tick owner".into(),
            ))
        }
    }
    fn block_effect(
        &mut self,
        _: &mut ServerLevel,
        _: LiveBlockEffect,
    ) -> Result<(), LiveBlockMutationError> {
        Err(LiveBlockMutationError::Unavailable(
            "fixture does not execute external world effects".into(),
        ))
    }
    fn block_physics_event(
        &mut self,
        _: &mut ServerLevel,
        _: BlockPos,
        _: BlockState,
    ) -> Result<bool, LiveBlockMutationError> {
        panic!("chunk-only source cases")
    }
    fn changed_state_keeps_block_entity(
        &mut self,
        _: BlockState,
        _: BlockState,
    ) -> Result<bool, LiveBlockMutationError> {
        panic!("no entities")
    }
}
const POS: BlockPos = BlockPos::new(8, 64, 8);
fn level(old: BlockState, light: bool) -> ServerLevel {
    static GENERATOR: LazyLock<Arc<OverworldGenerator>> =
        LazyLock::new(|| Arc::new(OverworldGenerator::new(42)));
    let mut proto = GENERATOR
        .create_holder(ChunkPos::ZERO)
        .into_state()
        .into_loan()
        .0;
    // Exercise the actual shared worldgen predicate path, not controlled
    // counter initialization. Promotion must retain those genuine counts.
    proto.set_block_state(8, 64, 8, old);
    proto.set_persisted_status(ChunkStatus::Spawn);
    let mut chunk = LevelChunk::from_generated_spawn_proto(proto).unwrap();
    let base = chunk.generation_base_mut();
    base.prime_heightmaps(&[
        Types::MotionBlocking,
        Types::MotionBlockingNoLeaves,
        Types::OceanFloor,
        Types::WorldSurface,
    ]);
    chunk.try_mark_saved(0);
    let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
    level.chunk_map.install(ChunkPos::ZERO, chunk);
    if light {
        level.install_live_light(LiveLightRuntime::new(create(-64, 384), true, true));
    }
    level
}

#[test]
fn actual_paper_live_section_counts_heightmaps_and_light_boundaries() {
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/live-block-mutation.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        let old = BlockState::new(StateId(row["old"].as_u64().unwrap() as u16));
        let state = BlockState::new(StateId(row["state"].as_u64().unwrap() as u16));
        let mut level = level(old, true);
        let result = level
            .set_chunk_block_live(
                POS,
                state,
                row["flags"].as_u64().unwrap() as u32,
                &mut Services::default(),
            )
            .unwrap();
        assert_eq!(
            json!(result.map_or(-1, |s| i32::from(s.id().0))),
            row["result"],
            "{row}"
        );
        let chunk = level.chunk_map.get_chunk_mut(ChunkPos::ZERO).unwrap();
        let section = chunk.generation_base().get_section(8);
        assert_eq!(
            json!([
                section.non_empty_block_count(),
                section.ticking_block_count(),
                section.ticking_fluid_count(),
                section.fluid_count(),
                i16::from(section.has_special_colliding_blocks())
            ]),
            row["counts"],
            "{row}"
        );
        let heights: Vec<_> = [
            Types::MotionBlocking,
            Types::MotionBlockingNoLeaves,
            Types::OceanFloor,
            Types::WorldSurface,
        ]
        .into_iter()
        .map(|ty| chunk.get_height_at(ty, 8, 8))
        .collect();
        assert_eq!(json!(heights), row["heights"], "{row}");
        assert_eq!(
            chunk.is_unsaved(0),
            row["unsaved"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            level.live_light.as_ref().unwrap().has_pending_updates(),
            !row["effects"].as_array().unwrap().is_empty()
        );
        cases += 1;
    }
    assert_eq!(cases, 11);
}

#[test]
fn unavailable_light_retains_exact_prior_terrain_and_heightmap_effects() {
    let stone = BlockState::of(
        rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
    );
    let mut level = level(BlockState::new(StateId(0)), false);
    let result = level.set_chunk_block_live(POS, stone, 276, &mut Services::default());
    assert!(matches!(
        result,
        Err(LiveBlockMutationError::Light(
            crate::server::lighting::LiveLightError::NotInstalled
        ))
    ));
    let chunk = level.chunk_map.get_chunk_mut(ChunkPos::ZERO).unwrap();
    assert_eq!(chunk.get_block_state(8, 64, 8), stone.id());
    assert_eq!(chunk.get_height_at(Types::MotionBlocking, 8, 8), 64);
    assert!(
        !chunk.is_unsaved(0),
        "markUnsaved occurs only after the light and block callbacks"
    );
}

#[cfg(unix)]
#[test]
fn off_thread_poi_change_refuses_before_touching_storage() {
    let mut level = level(BlockState::new(StateId(0)), false);
    let foreign_thread = std::thread::spawn(|| std::thread::current().id())
        .join()
        .unwrap();
    let mut services = Services {
        tick_thread: foreign_thread,
    };
    let composter = BlockState::of(
        rivet_registry::generated::blocks::BlockId::from_name("minecraft:composter").unwrap(),
    );
    let result = level.mutation_poi(POS, BlockState::new(StateId(0)), composter, &mut services);
    assert!(
        matches!(result,Err(LiveBlockMutationError::Unavailable(message)) if message=="not the retained tick owner")
    );
    assert!(level.poi_storage.is_none());
}

#[test]
fn anti_xray_controller_requires_actual_configuration_and_only_executes_source_disabled_body() {
    use crate::server::level::world_runtime_settings::WorldRuntimeSettings;
    let air = BlockState::new(StateId(0));
    let mut level = level(air, false);
    assert!(matches!(
        level.anti_xray_block_change_live(POS, air, None, 276, 512),
        Err(LiveBlockMutationError::MissingSettings)
    ));
    for enabled in [false, true] {
        let yaml = format!("anticheat: {{anti-xray: {{enabled: {enabled}}}}}");
        level.runtime_settings =
            Some(WorldRuntimeSettings::from_yaml("", &yaml, "", "minecraft:overworld").unwrap());
        let result = level.anti_xray_block_change_live(POS, air, None, 276, 512);
        if enabled {
            assert!(matches!(
                result,
                Err(LiveBlockMutationError::AntiXrayEnabled)
            ));
        } else {
            assert!(result.is_ok());
        }
    }
}

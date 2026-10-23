//! Canonical level-side ticker access. The runtime owns handles, ChunkMap owns
//! payloads, and Level owns the random stream. World effects remain explicit.
use super::ServerLevel;
use crate::server::level::{
    chunk_map::ChunkMap,
    level_block_entity_runtime::{
        BlockEntityTickerBackend, BlockEntityTickerError, LevelBlockEntityRuntime, NativeTickerKind,
    },
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos},
};
use rivet_util::random::ThreadUnsafeRandom;
use rivet_world::block_entity::NativeBlockEntity;

type Result<T> = std::result::Result<T, BlockEntityTickerError>;

/// Actual world operations, supplied by the tick owner. No default implementation
/// declares absent event, entity-spawn or environmental authority available.
pub trait BlockEntityWorldServices {
    fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<()>;
    fn require_ticker_body(&self, kind: NativeTickerKind) -> Result<()>;
    fn should_tick_blocks_at(&self, pos: BlockPos) -> bool;
    /// Includes actual border, BLOCK_TICKING status and loaded entity chunk.
    fn is_ticking(&self, pos: BlockPos) -> bool;
    fn tick_body(
        &mut self,
        runtime: &mut LevelBlockEntityRuntime,
        chunks: &mut ChunkMap,
        random: &mut ThreadUnsafeRandom,
        pos: BlockPos,
        kind: NativeTickerKind,
    ) -> Result<()>;
    fn report_tick_exception(&mut self, pos: BlockPos, message: &str) -> Result<()>;
    fn mid_tick_tasks(&mut self) -> Result<()>;
}

struct CanonicalBackend<'a, S> {
    chunks: &'a mut ChunkMap,
    random: &'a mut ThreadUnsafeRandom,
    services: &'a mut S,
}

impl<S: BlockEntityWorldServices> BlockEntityTickerBackend for CanonicalBackend<'_, S> {
    fn live_entity(&self, pos: BlockPos) -> Option<&NativeBlockEntity> {
        self.chunks
            .get_chunk(ChunkPos::containing(&pos))?
            .generation_base()
            .get_block_entity(&pos)
    }

    fn live_entity_mut(&mut self, pos: BlockPos) -> Option<&mut NativeBlockEntity> {
        self.chunks
            .get_chunk_mut(ChunkPos::containing(&pos))?
            .generation_base_mut()
            .get_block_entity_mut(&pos)
    }

    fn current_state(&self, pos: BlockPos) -> Result<BlockState> {
        let chunk = self
            .chunks
            .get_chunk(ChunkPos::containing(&pos))
            .ok_or_else(|| {
                BlockEntityTickerError::Unavailable(format!("missing runtime chunk at {pos:?}"))
            })?;
        Ok(BlockState::new(chunk.get_block_state(
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
        )))
    }

    fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<()> {
        self.services.on_block_entity_added(pos)
    }
    fn require_ticker_body(&self, kind: NativeTickerKind) -> Result<()> {
        self.services.require_ticker_body(kind)
    }
    fn should_tick_blocks_at(&self, pos: BlockPos) -> bool {
        self.services.should_tick_blocks_at(pos)
    }
    fn is_ticking(&self, pos: BlockPos) -> bool {
        self.services.is_ticking(pos)
    }
    fn tick_body(
        &mut self,
        runtime: &mut LevelBlockEntityRuntime,
        pos: BlockPos,
        kind: NativeTickerKind,
    ) -> Result<()> {
        self.services
            .tick_body(runtime, self.chunks, self.random, pos, kind)
    }
    fn remove_block_entity(&mut self, pos: BlockPos) -> Result<()> {
        let chunk = self
            .chunks
            .get_chunk_mut(ChunkPos::containing(&pos))
            .ok_or_else(|| {
                BlockEntityTickerError::Unavailable(format!("missing runtime chunk at {pos:?}"))
            })?;
        chunk.generation_base_mut().remove_block_entity(&pos);
        Ok(())
    }
    fn report_tick_exception(&mut self, pos: BlockPos, message: &str) -> Result<()> {
        self.services.report_tick_exception(pos, message)
    }
    fn mid_tick_tasks(&mut self) -> Result<()> {
        self.services.mid_tick_tasks()
    }
}

impl ServerLevel {
    /// FULL registration visits only live objects. Pending NBT remains lazy.
    /// This does not itself complete FULL or activate block ticking.
    pub fn register_chunk_block_entities(
        &mut self,
        pos: ChunkPos,
        ticking_enabled: bool,
        chunk_in_level: bool,
        services: &mut impl BlockEntityWorldServices,
    ) -> Result<()> {
        let positions: Vec<_> = self
            .chunk_map
            .get_chunk(pos)
            .ok_or_else(|| {
                BlockEntityTickerError::Unavailable(format!("missing runtime chunk {pos}"))
            })?
            .generation_base()
            .block_entity_positions_in_source_order();
        let mut backend = CanonicalBackend {
            chunks: &mut self.chunk_map,
            random: &mut self.world_random,
            services,
        };
        self.block_entity_runtime.register_existing_live(
            &mut backend,
            positions,
            ticking_enabled,
            chunk_in_level,
        )
    }

    /// The world's ticker list and RNG are retained between calls. The normal
    /// tick/freeze decision comes from the actual server tick-rate manager.
    pub fn tick_block_entities(
        &mut self,
        runs_normally: bool,
        services: &mut impl BlockEntityWorldServices,
    ) -> Result<()> {
        self.block_entity_runtime.tick(
            &mut CanonicalBackend {
                chunks: &mut self.chunk_map,
                random: &mut self.world_random,
                services,
            },
            runs_normally,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::{level_chunk::LevelChunk, server_level::ServerLevelConfig};
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::RandomSource;
    use rivet_world::block_entity::NativeBlockEntityData;
    use rivet_world::chunk::status::ChunkStatus;

    #[derive(Default)]
    struct Services {
        added: Vec<BlockPos>,
        ticks: usize,
    }
    impl BlockEntityWorldServices for Services {
        fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<()> {
            self.added.push(pos);
            Ok(())
        }
        fn require_ticker_body(&self, kind: NativeTickerKind) -> Result<()> {
            assert_eq!(kind, NativeTickerKind::Beehive);
            Ok(())
        }
        fn should_tick_blocks_at(&self, _: BlockPos) -> bool {
            true
        }
        fn is_ticking(&self, _: BlockPos) -> bool {
            true
        }
        fn tick_body(
            &mut self,
            _: &mut LevelBlockEntityRuntime,
            chunks: &mut ChunkMap,
            random: &mut ThreadUnsafeRandom,
            pos: BlockPos,
            kind: NativeTickerKind,
        ) -> Result<()> {
            assert_eq!(kind, NativeTickerKind::Beehive);
            self.ticks += 1;
            let entity = chunks
                .get_chunk_mut(ChunkPos::containing(&pos))
                .unwrap()
                .generation_base_mut()
                .get_block_entity_mut(&pos)
                .unwrap();
            let NativeBlockEntityData::Beehive(hive) = entity.data_mut() else {
                panic!("expected canonical hive")
            };
            crate::server::level::level_block_entity_runtime::tick_beehive(
                hive,
                random,
                false,
                |_, _, _| panic!("young occupant must not release"),
                || panic!("young occupant must not mark released"),
                || Ok(()),
            )
        }
        fn report_tick_exception(&mut self, _: BlockPos, message: &str) -> Result<()> {
            panic!("unexpected tick error: {message}")
        }
        fn mid_tick_tasks(&mut self) -> Result<()> {
            panic!("one ticker cannot reach mid-tick batch")
        }
    }

    #[test]
    fn canonical_hive_tick_mutates_live_payload_and_retains_world_rng() {
        let generator =
            std::sync::Arc::new(crate::server::level::generated_world::OverworldGenerator::new(42));
        let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
        let pos = BlockPos::new(3, 64, 8);
        let block = BlockState::of(BlockId::from_name("minecraft:bee_nest").unwrap());
        state.chunk_mut().set_block_state(3, 64, 8, block);
        let input=parse_compound_fully("{id:'minecraft:beehive',bees:[{entity_data:{id:'minecraft:bee',HasNectar:1b},ticks_in_hive:0,min_ticks_in_hive:600}]}").unwrap();
        state.chunk_mut().base_mut().set_block_entity(
            NativeBlockEntity::load(pos, block, &input)
                .unwrap()
                .unwrap(),
        );
        let pending = BlockPos::new(4, 64, 8);
        let pending_tag = parse_compound_fully("{id:'DUMMY',x:4,y:64,z:8}").unwrap();
        state
            .chunk_mut()
            .base_mut()
            .set_block_entity_nbt(pending_tag.clone());
        state.chunk_mut().set_persisted_status(ChunkStatus::Spawn);
        let (proto, _) = state.into_loan();
        let runtime = LevelChunk::from_generated_spawn_proto(proto).unwrap();
        let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
        level.chunk_map.install(ChunkPos::ZERO, runtime);
        level.world_random.set_seed(99);
        let mut services = Services::default();
        level
            .register_chunk_block_entities(ChunkPos::ZERO, true, true, &mut services)
            .unwrap();
        assert_eq!(services.added, [pos]);
        assert_eq!(level.block_entity_runtime.registered_count(), 1);
        level.tick_block_entities(false, &mut services).unwrap();
        assert_eq!(services.ticks, 0);
        level.tick_block_entities(true, &mut services).unwrap();
        level.tick_block_entities(true, &mut services).unwrap();
        assert_eq!(services.ticks, 2);
        let chunk = level.chunk_map.get_chunk(ChunkPos::ZERO).unwrap();
        assert!(chunk.get_block_entity(&pos).unwrap().has_level());
        let saved = chunk
            .get_block_entity(&pos)
            .unwrap()
            .save_with_full_metadata();
        assert_eq!(
            saved
                .get_list("bees")
                .unwrap()
                .get_compound(0)
                .unwrap()
                .get_int("ticks_in_hive"),
            Some(2)
        );
        assert_eq!(
            chunk.pending_block_entities().get(&pending),
            Some(&pending_tag)
        );
        let mut expected = ThreadUnsafeRandom::new(99);
        expected.next_double();
        expected.next_double();
        assert_eq!(level.world_random.next_long(), expected.next_long());
    }
}

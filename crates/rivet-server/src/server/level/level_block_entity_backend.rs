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
    /// Optional observer after the canonical Level/ServerLevel debug hook.
    /// This is not a replacement authority for the source onAdded behavior.
    fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<()>;
    /// Awake removal must execute the real source clear immediately. Missing
    /// transport is an unavailable authority, never a successful debug no-op.
    fn hive_debug_transport(
        &mut self,
    ) -> Result<&mut dyn crate::server::level::level_debug_synchronizers::HiveDebugTransport> {
        Err(BlockEntityTickerError::Unavailable(
            "awake block-entity removal requires actual hive debug transport".into(),
        ))
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBlockEntityCreation {
    Check,
    Immediate,
}

struct CanonicalBackend<'a, S> {
    chunks: &'a mut ChunkMap,
    random: &'a mut ThreadUnsafeRandom,
    debug: &'a mut crate::server::level::level_debug_synchronizers::LevelDebugSynchronizers,
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
        if self.live_entity(pos).is_some() {
            // Disjoint borrows: the source getter refers to this live object,
            // while the retained synchronizer owns only identity and cache.
            let entity = self
                .chunks
                .get_chunk(ChunkPos::containing(&pos))
                .and_then(|chunk| chunk.generation_base().get_block_entity(&pos))
                .expect("live entity checked");
            self.debug.register_block_entity(entity)?;
        }
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
        // isInLevel is loaded || clientSide; this is a real server chunk.
        if chunk.is_loaded() {
            let base = chunk.generation_base_mut();
            let removed = base.take_live_block_entity(&pos);
            base.remove_block_entity_nbt(&pos);
            if let Some(mut removed) = removed {
                // The four supported EntityBlocks have no game-event listener.
                // Source removes maps before debug, and setsRemoved only after
                // debug returns. Do not roll back those partial effects on Err.
                if !self.debug.is_sleeping() {
                    self.debug.drop_block_entity_with(pos, |members, update| {
                        crate::server::level::level_debug_synchronizers::send_to_tracking(
                            members,
                            update,
                            self.services.hive_debug_transport()?,
                        )
                    })?;
                }
                removed.set_removed();
            }
        }
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
    /// Only the hive source is polled here. A global debug tick must also
    /// execute wake enumeration and the other enabled source synchronizers.
    pub fn tick_hive_debug<'a>(
        &mut self,
        players: impl IntoIterator<
            Item = (
                rivet_util::mth::Uuid,
                &'a std::collections::HashSet<
                    crate::server::level::level_debug_synchronizers::DebugSubscription,
                >,
                Vec<ChunkPos>,
            ),
        >,
        transport: &mut impl crate::server::level::level_debug_synchronizers::HiveDebugTransport,
    ) -> Result<()> {
        self.debug_synchronizers
            .tick_hives(&self.chunk_map, players, transport)
    }

    /// Real LevelChunk lazy lookup/promotion followed by addAndRegister's
    /// loaded-only hooks. Pending tags are consumed only by actual promotion;
    /// missing native codecs/bodies remain errors with canonical data retained.
    /// Reading an already-live value is not evidence of FULL activation.
    pub fn load_native_block_entity(
        &mut self,
        pos: BlockPos,
        creation: NativeBlockEntityCreation,
        ticking_enabled: bool,
        services: &mut impl BlockEntityWorldServices,
    ) -> Result<Option<&NativeBlockEntity>> {
        let chunk_pos = ChunkPos::containing(&pos);
        let chunk = self.chunk_map.get_chunk_mut(chunk_pos).ok_or_else(|| {
            BlockEntityTickerError::Unavailable(format!("missing runtime chunk {chunk_pos}"))
        })?;
        if let Some(entity) = chunk.generation_base().get_block_entity(&pos) {
            if entity.is_removed() {
                chunk.generation_base_mut().remove_live_block_entity(&pos);
                return Ok(None);
            }
            return Ok(self
                .chunk_map
                .get_chunk(chunk_pos)
                .and_then(|chunk| chunk.get_block_entity(&pos)));
        }
        let native_error = |error: rivet_world::block_entity::NativeBlockEntityError| {
            BlockEntityTickerError::Unavailable(error.to_string())
        };
        let mut created = chunk
            .promote_pending_block_entity(pos)
            .map_err(native_error)?;
        if !created && creation == NativeBlockEntityCreation::Immediate {
            let state =
                BlockState::new(chunk.get_block_state(pos.get_x(), pos.get_y(), pos.get_z()));
            if crate::server::level::structure_snapshot_data::has_block_entity(state) {
                let mut entity = NativeBlockEntity::fresh(pos, state).map_err(native_error)?;
                entity.associate_level();
                chunk.generation_base_mut().set_block_entity(entity);
                created = true;
            }
        }
        if !created {
            return Ok(None);
        }
        let loaded = chunk.is_loaded();
        if loaded {
            let mut backend = CanonicalBackend {
                chunks: &mut self.chunk_map,
                random: &mut self.world_random,
                debug: &mut self.debug_synchronizers,
                services,
            };
            self.block_entity_runtime.register_existing_live(
                &mut backend,
                [pos],
                ticking_enabled,
                true,
            )?;
        }
        Ok(self
            .chunk_map
            .get_chunk(chunk_pos)
            .and_then(|chunk| chunk.get_block_entity(&pos)))
    }

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
            debug: &mut self.debug_synchronizers,
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
                debug: &mut self.debug_synchronizers,
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

    struct LazyServices {
        added: usize,
    }
    impl BlockEntityWorldServices for LazyServices {
        fn on_block_entity_added(&mut self, _: BlockPos) -> Result<()> {
            self.added += 1;
            Ok(())
        }
        fn require_ticker_body(&self, kind: NativeTickerKind) -> Result<()> {
            Err(BlockEntityTickerError::Unavailable(format!(
                "fixture has no {kind:?} world/entity/event executor"
            )))
        }
        fn should_tick_blocks_at(&self, _: BlockPos) -> bool {
            panic!("lookup does not tick")
        }
        fn is_ticking(&self, _: BlockPos) -> bool {
            panic!("lookup does not tick")
        }
        fn tick_body(
            &mut self,
            _: &mut LevelBlockEntityRuntime,
            _: &mut ChunkMap,
            _: &mut ThreadUnsafeRandom,
            _: BlockPos,
            _: NativeTickerKind,
        ) -> Result<()> {
            panic!("lookup must not run a body")
        }
        fn report_tick_exception(&mut self, _: BlockPos, _: &str) -> Result<()> {
            panic!("lookup must not run a body")
        }
        fn mid_tick_tasks(&mut self) -> Result<()> {
            panic!("lookup does not tick")
        }
    }

    #[test]
    fn actual_paper_lazy_creation_and_explicit_unavailable_ticker_boundaries() {
        let generator =
            std::sync::Arc::new(crate::server::level::generated_world::OverworldGenerator::new(42));
        let pos = BlockPos::new(8, 64, 8);
        let mut cases = 0;
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/lazy-block-entity.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let family = row["family"].as_str().unwrap();
            let name = match family {
                "chest" => "minecraft:chest",
                "hive" => "minecraft:bee_nest",
                "spawner" => "minecraft:spawner",
                "heart" => "minecraft:creaking_heart",
                _ => panic!("oracle family"),
            };
            let block = BlockState::of(BlockId::from_name(name).unwrap());
            let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
            state.chunk_mut().set_block_state(8, 64, 8, block);
            if let Some(input) = row["input_snbt"].as_str() {
                state
                    .chunk_mut()
                    .base_mut()
                    .set_block_entity_nbt(parse_compound_fully(input).unwrap());
            }
            state.chunk_mut().set_persisted_status(ChunkStatus::Spawn);
            let mut chunk = LevelChunk::from_generated_spawn_proto(state.into_loan().0).unwrap();
            chunk.set_loaded(row["loaded"].as_bool().unwrap());
            let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
            level.chunk_map.install(ChunkPos::ZERO, chunk);
            let mut services = LazyServices { added: 0 };
            let creation = if row["immediate"].as_bool().unwrap() {
                NativeBlockEntityCreation::Immediate
            } else {
                NativeBlockEntityCreation::Check
            };
            let first = level.load_native_block_entity(pos, creation, true, &mut services);
            if row["tickers"].as_u64().unwrap() > 0 {
                // Paper has a complete executor. This controlled Rust world
                // deliberately does not: retaining the actual promoted value
                // is valid, claiming successful FULL registration is not.
                assert!(
                    matches!(first, Err(BlockEntityTickerError::Unavailable(_))),
                    "{row}"
                );
                assert_eq!(level.block_entity_runtime.registered_count(), 0);
                let entity = level
                    .chunk_map
                    .get_chunk(ChunkPos::ZERO)
                    .unwrap()
                    .get_block_entity(&pos)
                    .unwrap();
                assert_eq!(
                    entity.save_with_full_metadata(),
                    parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
                    "{row}"
                );
                assert!(entity.has_level());
            } else {
                let first = first.unwrap().map(NativeBlockEntity::ticker_identity);
                assert_eq!(first.is_some(), row["present"].as_bool().unwrap(), "{row}");
                let second = level
                    .load_native_block_entity(pos, creation, true, &mut services)
                    .unwrap();
                let same = first.as_ref().zip(second).is_some_and(|(first, second)| {
                    std::sync::Weak::ptr_eq(first, &second.ticker_identity())
                });
                assert_eq!(same, row["same_identity"].as_bool().unwrap(), "{row}");
                if let Some(entity) = second {
                    assert_eq!(
                        entity.has_level(),
                        row["has_level"].as_bool().unwrap(),
                        "{row}"
                    );
                    assert_eq!(
                        entity.save_with_full_metadata(),
                        parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
                        "{row}"
                    );
                }
            }
            assert_eq!(
                services.added as u64,
                row["added"].as_u64().unwrap(),
                "{row}"
            );
            assert_eq!(
                level
                    .chunk_map
                    .get_chunk(ChunkPos::ZERO)
                    .unwrap()
                    .pending_block_entities()
                    .contains_key(&pos),
                row["pending"].as_bool().unwrap(),
                "{row}"
            );
            cases += 1;
        }
        assert_eq!(cases, 80);
    }

    #[derive(Default)]
    struct Services {
        added: Vec<BlockPos>,
        ticks: usize,
        debug: Option<DebugTransport>,
    }
    #[derive(Default)]
    struct DebugTransport(Vec<crate::server::level::level_debug_synchronizers::HiveDebugUpdate>);
    impl crate::server::level::level_debug_synchronizers::HiveDebugTransport for DebugTransport {
        fn subscriber_order(
            &self,
            members: &std::collections::HashSet<rivet_util::mth::Uuid>,
        ) -> Result<Vec<rivet_util::mth::Uuid>> {
            assert!(members.len() <= 1);
            Ok(members.iter().copied().collect())
        }
        fn is_chunk_tracked(&self, _: rivet_util::mth::Uuid, _: ChunkPos) -> bool {
            true
        }
        fn send(
            &mut self,
            _: rivet_util::mth::Uuid,
            update: crate::server::level::level_debug_synchronizers::HiveDebugUpdate,
        ) -> Result<()> {
            self.0.push(update);
            Ok(())
        }
    }
    impl BlockEntityWorldServices for Services {
        fn hive_debug_transport(
            &mut self,
        ) -> Result<&mut dyn crate::server::level::level_debug_synchronizers::HiveDebugTransport>
        {
            self.debug.as_mut().map(|debug| debug as &mut dyn crate::server::level::level_debug_synchronizers::HiveDebugTransport)
                .ok_or_else(|| BlockEntityTickerError::Unavailable("actual test transport absent".into()))
        }
        fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<()> {
            self.added.push(pos);
            Ok(())
        }
        fn require_ticker_body(&self, _: NativeTickerKind) -> Result<()> {
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
    fn canonical_removal_clears_awake_hive_immediately_and_requires_real_transport() {
        use crate::server::level::level_debug_synchronizers::{
            DebugSubscription, ServerDebugSubscribers,
        };
        use rivet_util::mth::Uuid;
        use std::collections::HashSet;
        let pos = BlockPos::new(8, 64, 8);
        let player = Uuid { most: 3, least: 9 };
        for (loaded, awake, transport, name, hive_subscription, register) in [
            (true, true, true, "bee_nest", true, true),
            (true, true, false, "bee_nest", true, true),
            (true, false, false, "bee_nest", true, true),
            (false, true, true, "bee_nest", true, true),
            (true, true, false, "chest", true, true),
            (true, true, false, "spawner", true, true),
            (true, true, false, "creaking_heart", true, true),
            (true, true, false, "bee_nest", false, true),
            (true, true, false, "bee_nest", true, false),
        ] {
            let generator = std::sync::Arc::new(
                crate::server::level::generated_world::OverworldGenerator::new(42),
            );
            let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
            let block = BlockState::of(BlockId::from_name(&format!("minecraft:{name}")).unwrap());
            state.chunk_mut().set_block_state(8, 64, 8, block);
            state
                .chunk_mut()
                .base_mut()
                .set_block_entity(NativeBlockEntity::fresh(pos, block).unwrap());
            state.chunk_mut().set_persisted_status(ChunkStatus::Spawn);
            let mut chunk = LevelChunk::from_generated_spawn_proto(state.into_loan().0).unwrap();
            chunk.set_loaded(loaded);
            let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
            level.chunk_map.install(ChunkPos::ZERO, chunk);
            let mut services = Services {
                debug: transport.then(DebugTransport::default),
                ..Default::default()
            };
            let subscriptions = HashSet::from([if hive_subscription {
                DebugSubscription::BeeHives
            } else {
                DebugSubscription::Brains
            }]);
            if awake {
                let mut subscribers = ServerDebugSubscribers::default();
                subscribers.tick([(player, &subscriptions)]);
                level.begin_debug_tick(&subscribers);
            }
            if register {
                level
                    .register_chunk_block_entities(ChunkPos::ZERO, true, true, &mut services)
                    .unwrap();
            }
            if awake && name == "bee_nest" && hive_subscription && register {
                services.debug.get_or_insert_with(DebugTransport::default);
                level
                    .tick_hive_debug(
                        [(player, &subscriptions, vec![ChunkPos::ZERO])],
                        services.debug.as_mut().unwrap(),
                    )
                    .unwrap();
                assert_eq!(services.debug.as_ref().unwrap().0.len(), 1);
                if !transport {
                    services.debug = None;
                }
            }
            let outcome = CanonicalBackend {
                chunks: &mut level.chunk_map,
                random: &mut level.world_random,
                debug: &mut level.debug_synchronizers,
                services: &mut services,
            }
            .remove_block_entity(pos);
            let broadcasts = loaded && awake && name == "bee_nest" && hive_subscription && register;
            assert_eq!(outcome.is_ok(), !broadcasts || transport);
            let live = level
                .chunk_map
                .get_chunk(ChunkPos::ZERO)
                .unwrap()
                .generation_base()
                .get_block_entity(&pos);
            assert_eq!(
                live.is_some(),
                !loaded,
                "native map mutation precedes any debug authority error"
            );
            if loaded {
                assert_eq!(
                    level.debug_synchronizers.registered_hive_count(),
                    0,
                    "local source is removed even without subscribers or when sending fails"
                );
            }
            if loaded && awake && transport {
                let packets = &services.debug.as_ref().unwrap().0;
                assert_eq!(
                    packets.len(),
                    2,
                    "clear is immediate, not deferred to a later tick"
                );
                assert_eq!(packets[1].value, None);
                assert_eq!(level.debug_synchronizers.registered_hive_count(), 0);
            }
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

//! ServerLevel registration over canonical chunk-owned live containers.
//! This does not implement FULL post-load readiness or a default tick handler.
use super::ServerLevel;
use crate::server::level::{chunk_map::ChunkMap, level_chunk::LevelChunk};
use rivet_registry::{
    core::{BlockPos, ChunkPos},
    fluid_id::FluidId,
};
use rivet_world::{
    block::Block,
    ticks::{LevelChunkTicks, LevelTicks, ScheduledTick, TickContainers},
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TickRuntimeError {
    #[error("tick container chunk {0} is not loaded")]
    MissingChunk(ChunkPos),
    #[error("chunk {0} has not registered both tick containers")]
    NotRegistered(ChunkPos),
}

/// A bounded borrow of the canonical map, never a second container cache.
pub struct BlockTickContainers<'a>(&'a mut ChunkMap);
pub struct FluidTickContainers<'a>(&'a mut ChunkMap);

impl BlockTickContainers<'_> {
    pub fn get_chunk_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunk> {
        self.0.get_chunk_mut(pos)
    }
}
impl FluidTickContainers<'_> {
    pub fn get_chunk_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunk> {
        self.0.get_chunk_mut(pos)
    }
}
impl TickContainers<Block> for BlockTickContainers<'_> {
    fn get(&self, pos: ChunkPos) -> Option<&LevelChunkTicks<Block>> {
        self.0.get_chunk(pos).map(LevelChunk::block_ticks)
    }
    fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunkTicks<Block>> {
        self.0.get_chunk_mut(pos).map(LevelChunk::block_ticks_mut)
    }
}
impl TickContainers<FluidId> for FluidTickContainers<'_> {
    fn get(&self, pos: ChunkPos) -> Option<&LevelChunkTicks<FluidId>> {
        self.0.get_chunk(pos).map(LevelChunk::fluid_ticks)
    }
    fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunkTicks<FluidId>> {
        self.0.get_chunk_mut(pos).map(LevelChunk::fluid_ticks_mut)
    }
}

impl ServerLevel {
    /// Source LevelChunk.registerTickContainerInLevel. No saved delays are
    /// converted here, and no FULL/block-ticking status is manufactured.
    pub fn register_chunk_ticks(&mut self, pos: ChunkPos) -> Result<(), TickRuntimeError> {
        let chunk = self
            .chunk_map
            .get_chunk_mut(pos)
            .ok_or(TickRuntimeError::MissingChunk(pos))?;
        self.block_ticks.add_container(pos, chunk.block_ticks_mut());
        self.fluid_ticks.add_container(pos, chunk.fluid_ticks_mut());
        Ok(())
    }

    /// Source ServerLevel.startTickingChunk -> chunk.unpackTicks(gameTime).
    /// Block-ticking ticket authority calls this separately after registration.
    pub fn activate_chunk_ticks(&mut self, pos: ChunkPos) -> Result<(), TickRuntimeError> {
        if !self.block_ticks.has_container(pos) || !self.fluid_ticks.has_container(pos) {
            return Err(TickRuntimeError::NotRegistered(pos));
        }
        self.chunk_map
            .get_chunk_mut(pos)
            .ok_or(TickRuntimeError::MissingChunk(pos))?
            .unpack_ticks(self.game_time);
        Ok(())
    }

    pub fn unregister_chunk_ticks(&mut self, pos: ChunkPos) -> Result<(), TickRuntimeError> {
        let chunk = self
            .chunk_map
            .get_chunk_mut(pos)
            .ok_or(TickRuntimeError::MissingChunk(pos))?;
        self.block_ticks
            .remove_container(pos, chunk.block_ticks_mut());
        self.fluid_ticks
            .remove_container(pos, chunk.fluid_ticks_mut());
        Ok(())
    }

    /// Detach scheduler callbacks before returning ownership. The caller still
    /// owns saving/eviction acknowledgement; no pending or queued tick is lost.
    pub fn take_loaded_chunk(&mut self, pos: ChunkPos) -> Option<LevelChunk> {
        self.unregister_chunk_ticks(pos).ok()?;
        self.chunk_map.take(pos)
    }

    /// A loaded canonical value is not replaceable behind the scheduler's
    /// index. On collision the caller retains its entire incoming chunk.
    pub fn install_loaded_chunk(&mut self, chunk: LevelChunk) -> Result<(), Box<LevelChunk>> {
        let pos = chunk.pos();
        if self.chunk_map.get_chunk(pos).is_some() {
            return Err(Box::new(chunk));
        }
        self.chunk_map.install(pos, chunk);
        Ok(())
    }

    pub fn schedule_block_tick(&mut self, tick: ScheduledTick<Block>) -> bool {
        self.block_ticks
            .schedule(&mut BlockTickContainers(&mut self.chunk_map), tick)
    }
    pub fn schedule_fluid_tick(&mut self, tick: ScheduledTick<FluidId>) -> bool {
        self.fluid_ticks
            .schedule(&mut FluidTickContainers(&mut self.chunk_map), tick)
    }

    /// Actual tick-check authority and executable handler are mandatory inputs;
    /// there is no successful no-op handler or all-loaded-is-ticking default.
    pub fn run_block_ticks(
        &mut self,
        max: i32,
        check: impl FnMut(ChunkPos) -> bool,
        output: impl FnMut(&mut LevelTicks<Block>, &mut BlockTickContainers<'_>, BlockPos, Block),
    ) {
        self.block_ticks.tick(
            &mut BlockTickContainers(&mut self.chunk_map),
            self.game_time,
            max,
            check,
            output,
        );
    }
    pub fn run_fluid_ticks(
        &mut self,
        max: i32,
        check: impl FnMut(ChunkPos) -> bool,
        output: impl FnMut(&mut LevelTicks<FluidId>, &mut FluidTickContainers<'_>, BlockPos, FluidId),
    ) {
        self.fluid_ticks.tick(
            &mut FluidTickContainers(&mut self.chunk_map),
            self.game_time,
            max,
            check,
            output,
        );
    }

    /// The same source collect/run/cleanup phases, with canonical world access
    /// during output. The scheduler never moves, so reentrant schedule and
    /// willTickThisTick reach its actual active queues/index. Errors/panics
    /// leave the source partial state and do not run normal-return cleanup.
    pub fn run_fluid_ticks_with_world<E>(
        &mut self,
        max: i32,
        check: impl FnMut(ChunkPos) -> bool,
        mut output: impl FnMut(&mut Self, BlockPos, FluidId) -> Result<(), E>,
    ) -> Result<(), E> {
        self.fluid_ticks.collect_due_ticks(
            &mut FluidTickContainers(&mut self.chunk_map),
            self.game_time,
            max,
            check,
        );
        while let Some((pos, kind)) = self.fluid_ticks.next_collected_tick() {
            output(self, pos, kind)?;
        }
        self.fluid_ticks.cleanup_after_tick();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::level::server_level::ServerLevelConfig;
    use rivet_world::ticks::{SavedTick, TickPriority};
    use serde_json::{Value, json};

    fn label(block: Block) -> &'static str {
        match block.name() {
            "minecraft:sand" => "t",
            "minecraft:stone" => "q",
            _ => panic!("fixture block"),
        }
    }

    #[test]
    fn fluid_world_callback_reenters_actual_scheduler_and_preserves_failed_output() {
        let mut world = ServerLevel::new(ServerLevelConfig::default());
        world.register_chunk_ticks(ChunkPos::ZERO).unwrap();
        world.activate_chunk_ticks(ChunkPos::ZERO).unwrap();
        let a = BlockPos::new(0, 64, 0);
        let b = BlockPos::new(1, 64, 0);
        let c = BlockPos::new(2, 64, 0);
        for (order, pos) in [a, b].into_iter().enumerate() {
            assert!(world.schedule_fluid_tick(ScheduledTick::new_normal(
                FluidId::WATER,
                pos,
                0,
                order as i64
            )));
        }
        let mut seen = Vec::new();
        let result = world.run_fluid_ticks_with_world(
            10,
            |_| true,
            |level, pos, fluid| {
                seen.push(pos);
                assert_eq!(fluid, FluidId::WATER);
                assert!(!level.fluid_ticks.will_tick_this_tick(&a, &fluid));
                assert!(level.fluid_ticks.will_tick_this_tick(&b, &fluid));
                assert!(level.schedule_fluid_tick(ScheduledTick::new_normal(fluid, c, 0, 2)));
                assert!(level.fluid_ticks.has_scheduled_tick(
                    &FluidTickContainers(&mut level.chunk_map),
                    &c,
                    &fluid
                ));
                Err::<(), _>("actual effect unavailable")
            },
        );
        assert_eq!(result, Err("actual effect unavailable"));
        // Java LevelTicks has no finally cleanup after a thrown output. B is
        // still collected; C was scheduled on the retained canonical engine.
        assert!(world.fluid_ticks.will_tick_this_tick(&b, &FluidId::WATER));
        world
            .run_fluid_ticks_with_world(
                10,
                |_| true,
                |_, pos, _| {
                    seen.push(pos);
                    Ok::<_, ()>(())
                },
            )
            .unwrap();
        assert_eq!(seen, [a, b, c]);
        assert!(!world.fluid_ticks.will_tick_this_tick(&b, &FluidId::WATER));
        assert_eq!(
            world
                .fluid_ticks
                .count(&FluidTickContainers(&mut world.chunk_map)),
            0
        );
    }

    #[test]
    fn canonical_registration_and_activation_match_actual_java_sequence() {
        fn assert_send<T: Send>() {}
        assert_send::<ServerLevel>();
        let row: Value =
            include_str!("../../../../../tools/rivet-oracle/fixtures/level-ticks.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|row| row["name"] == "pending_duplicate_activation")
                .unwrap();
        let mut world = ServerLevel::new(ServerLevelConfig::default());
        let mut events = Vec::new();
        for (step, command) in row["commands"].as_array().unwrap().iter().enumerate() {
            let n = |i: usize| command[i].as_i64().unwrap();
            match command[1].as_str().unwrap() {
                "add" => {
                    let pos = ChunkPos::new(n(2) as i32, n(3) as i32);
                    if world.chunk_map.get_chunk(pos).is_none() {
                        assert!(world.install_loaded_chunk(LevelChunk::new(pos)).is_ok());
                    }
                    if let Some(pending) = command.get(4).and_then(Value::as_array) {
                        let ticks = pending
                            .iter()
                            .map(|tick| {
                                SavedTick::new(
                                    Block::from_name(if tick[0] == "t" {
                                        "minecraft:sand"
                                    } else {
                                        "minecraft:stone"
                                    })
                                    .unwrap(),
                                    BlockPos::new(
                                        tick[1].as_i64().unwrap() as i32,
                                        tick[2].as_i64().unwrap() as i32,
                                        tick[3].as_i64().unwrap() as i32,
                                    ),
                                    tick[4].as_i64().unwrap() as i32,
                                    TickPriority::by_value(tick[5].as_i64().unwrap() as i32),
                                )
                            })
                            .collect();
                        *world
                            .chunk_map
                            .get_chunk_mut(pos)
                            .unwrap()
                            .block_ticks_mut() = LevelChunkTicks::new_with_pending(ticks);
                    }
                    world.register_chunk_ticks(pos).unwrap();
                }
                "unpack" => {
                    world.set_game_time(n(4));
                    world
                        .activate_chunk_ticks(ChunkPos::new(n(2) as i32, n(3) as i32))
                        .unwrap();
                }
                "tick" => {
                    world.set_game_time(n(2));
                    let blocked: Vec<_> = command[4]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| {
                            ChunkPos::new(
                                p[0].as_i64().unwrap() as i32,
                                p[1].as_i64().unwrap() as i32,
                            )
                        })
                        .collect();
                    world.run_block_ticks(n(3) as i32, |pos| !blocked.contains(&pos), |scheduler, store, pos, kind| {
                        events.push(json!({"tick":[label(kind),pos.get_x(),pos.get_y(),pos.get_z()],"count":scheduler.count(store)}));
                    });
                }
                _ => panic!("unexpected fixture operation"),
            }
            if let Some(snapshot) = row["snapshots"]
                .as_array()
                .unwrap()
                .iter()
                .find(|snapshot| snapshot["step"] == step)
            {
                let expected = snapshot["a"]["containers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["pos"] == json!([2, 0]))
                    .unwrap();
                let ticks = world
                    .chunk_map
                    .get_chunk(ChunkPos::new(2, 0))
                    .unwrap()
                    .block_ticks();
                assert_eq!(ticks.count(), expected["count"].as_u64().unwrap() as usize);
                let actual: Vec<_> = ticks
                    .all()
                    .map(|tick| {
                        json!([
                            label(tick.r#type),
                            tick.pos.get_x(),
                            tick.pos.get_y(),
                            tick.pos.get_z(),
                            tick.trigger_tick,
                            tick.priority.value(),
                            tick.sub_tick_order
                        ])
                    })
                    .collect();
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    expected["ticks"],
                    "step{step}"
                );
            }
        }
        let expected: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| json!({"tick":event["tick"],"count":event["count"]}))
            .collect();
        assert_eq!(events, expected);

        let pos = ChunkPos::new(2, 0);
        let incoming = LevelChunk::new(pos);
        assert!(
            world.install_loaded_chunk(incoming).is_err(),
            "registered canonical chunk is never overwritten"
        );
        let mut detached = world.take_loaded_chunk(pos).unwrap();
        let kind = Block::from_name("minecraft:sand").unwrap();
        detached
            .block_ticks_mut()
            .schedule(ScheduledTick::new_normal(
                kind,
                BlockPos::new(32, 64, 0),
                106,
                0,
            ));
        world.set_game_time(106);
        world.run_block_ticks(
            10,
            |_| true,
            |_, _, _, _| panic!("detached container callback must be disconnected"),
        );
        assert!(world.install_loaded_chunk(detached).is_ok());
        world.register_chunk_ticks(pos).unwrap();
        let mut output = Vec::new();
        world.run_block_ticks(10, |_| true, |_, _, pos, kind| output.push((pos, kind)));
        assert_eq!(output, [(BlockPos::new(32, 64, 0), kind)]);
    }

    #[test]
    fn canonical_tick_pack_dirty_lifecycle_matches_actual_java() {
        let mut world = ServerLevel::new(ServerLevelConfig::default());
        let pos = ChunkPos::ZERO;
        let t = Block::from_name("minecraft:sand").unwrap();
        let q = Block::from_name("minecraft:stone").unwrap();
        // The default fixture already owns the origin chunk.
        *world
            .chunk_map
            .get_chunk_mut(pos)
            .unwrap()
            .block_ticks_mut() = LevelChunkTicks::new_with_pending(vec![
            SavedTick::new(t, BlockPos::new(1, 64, 1), 5, TickPriority::Normal),
            SavedTick::new(q, BlockPos::new(2, 64, 1), 0, TickPriority::High),
        ]);
        let mut detached = None;
        for line in include_str!(
            "../../../../../tools/rivet-oracle/fixtures/tick-container-lifecycle.jsonl"
        )
        .lines()
        {
            let expected: Value = serde_json::from_str(line).unwrap();
            let phase = expected["phase"].as_str().unwrap();
            let time = expected["time"].as_i64().unwrap();
            world.set_game_time(time);
            match phase {
                "pending" | "aged" => {}
                "registered" => world.register_chunk_ticks(pos).unwrap(),
                "activated" => world.activate_chunk_ticks(pos).unwrap(),
                "polled" => {
                    world
                        .chunk_map
                        .get_chunk_mut(pos)
                        .unwrap()
                        .block_ticks_mut()
                        .poll();
                }
                "cleaned" => world
                    .chunk_map
                    .get_chunk_mut(pos)
                    .unwrap()
                    .block_ticks_mut()
                    .clear_dirty(),
                "scheduled" => {
                    assert!(world.schedule_block_tick(ScheduledTick::new(
                        q,
                        BlockPos::new(2, 64, 1),
                        108,
                        TickPriority::Low,
                        33
                    )));
                }
                "detached" => {
                    let mut chunk = world.take_loaded_chunk(pos).unwrap();
                    chunk.block_ticks_mut().schedule(ScheduledTick::new_normal(
                        t,
                        BlockPos::new(3, 64, 1),
                        102,
                        34,
                    ));
                    detached = Some(chunk);
                }
                _ => panic!("unexpected lifecycle phase"),
            }
            let chunk = match detached.as_mut() {
                Some(chunk) => chunk,
                None => world.chunk_map.get_chunk_mut(pos).unwrap(),
            };
            let ticks = chunk.block_ticks_mut();
            let before = ticks.is_dirty(time);
            let count = ticks.count();
            let saved: Vec<_> = ticks
                .pack(time)
                .iter()
                .map(|tick| {
                    json!([
                        label(tick.r#type),
                        tick.pos.get_x(),
                        tick.pos.get_y(),
                        tick.pos.get_z(),
                        tick.delay,
                        tick.priority.value()
                    ])
                })
                .collect();
            assert_eq!(
                json!({"phase":phase,"time":time,"before_pack_dirty":before,"count":count,"saved":saved,"after_pack_dirty":ticks.is_dirty(time)}),
                expected
            );
        }
    }
}

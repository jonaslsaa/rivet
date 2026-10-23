//! Level's ticker lists and LevelChunk's rebindable wrappers. Payloads stay in
//! the canonical chunk; neither pending NBT nor a second block-entity map is
//! loaded here. This is not a claim that missing server tick bodies are ready.
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{block_state::BlockState, core::BlockPos};
use rivet_util::RandomSource;
use rivet_world::block_entity::{
    NativeBlockEntity, NativeBlockEntityData, beehive_data::BeehiveData,
};
use std::{collections::HashMap, sync::Weak};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTickerKind {
    Beehive,
    Spawner,
    CreakingHeart,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlockEntityTickerError {
    #[error("block-entity ticking authority unavailable: {0}")]
    Unavailable(String),
    #[error("block-entity ticker failed: {0}")]
    Body(String),
}

/// The owning level supplies real gates and callbacks. In particular,
/// require_ticker_body must not accept a stub or an unavailable entity/event
/// authority merely because a wrapper can be allocated.
pub trait BlockEntityTickerBackend {
    fn live_entity(&self, pos: BlockPos) -> Option<&NativeBlockEntity>;
    fn live_entity_mut(&mut self, pos: BlockPos) -> Option<&mut NativeBlockEntity>;
    fn current_state(&self, pos: BlockPos) -> Result<BlockState, BlockEntityTickerError>;
    fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<(), BlockEntityTickerError>;
    fn require_ticker_body(&self, kind: NativeTickerKind) -> Result<(), BlockEntityTickerError>;
    fn should_tick_blocks_at(&self, pos: BlockPos) -> bool;
    /// World border, actual BLOCK_TICKING status and areEntitiesLoaded.
    fn is_ticking(&self, pos: BlockPos) -> bool;
    fn tick_body(
        &mut self,
        runtime: &mut LevelBlockEntityRuntime,
        pos: BlockPos,
        kind: NativeTickerKind,
    ) -> Result<(), BlockEntityTickerError>;
    fn remove_block_entity(&mut self, pos: BlockPos) -> Result<(), BlockEntityTickerError>;
    fn report_tick_exception(
        &mut self,
        pos: BlockPos,
        message: &str,
    ) -> Result<(), BlockEntityTickerError>;
    fn mid_tick_tasks(&mut self) -> Result<(), BlockEntityTickerError>;
}

#[derive(Debug)]
struct BoundTicker {
    pos: BlockPos,
    identity: Weak<()>,
    kind: NativeTickerKind,
}

/// An arena entry is a source wrapper object: rebind preserves the list slot,
/// NULL_TICKER detaches it, and the level loop removes it on its next scan.
#[derive(Debug, Default)]
pub struct LevelBlockEntityRuntime {
    wrappers: Vec<Option<BoundTicker>>,
    by_position: HashMap<BlockPos, usize>,
    active: Vec<usize>,
    pending: Vec<usize>,
    ticking: bool,
    free_wrappers: Vec<usize>,
}

impl LevelBlockEntityRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Caller supplies the actual live-map iteration order. Pending positions
    /// are never enumerated or promoted. The four native families have no
    /// EntityBlock game-event listener; the debug registration hook is real.
    pub fn register_existing_live(
        &mut self,
        backend: &mut impl BlockEntityTickerBackend,
        positions: impl IntoIterator<Item = BlockPos>,
        ticking_enabled: bool,
        chunk_in_level: bool,
    ) -> Result<(), BlockEntityTickerError> {
        for pos in positions {
            let Some(entity) = backend.live_entity_mut(pos) else {
                continue;
            };
            entity.associate_level();
            backend.on_block_entity_added(pos)?;
            self.update_ticker(backend, pos, ticking_enabled, chunk_in_level)?;
        }
        Ok(())
    }

    pub fn update_ticker(
        &mut self,
        backend: &impl BlockEntityTickerBackend,
        pos: BlockPos,
        ticking_enabled: bool,
        chunk_in_level: bool,
    ) -> Result<(), BlockEntityTickerError> {
        // Paper returns before getTicker/removal when globally disabled.
        if !ticking_enabled {
            return Ok(());
        }
        let Some(entity) = backend.live_entity(pos) else {
            return Ok(());
        };
        let Some(kind) = server_ticker(entity) else {
            self.remove_ticker(pos);
            return Ok(());
        };
        backend.require_ticker_body(kind)?;
        let binding = BoundTicker {
            pos,
            identity: entity.ticker_identity(),
            kind,
        };
        if let Some(&id) = self.by_position.get(&pos) {
            self.wrappers[id] = Some(binding);
        } else if chunk_in_level {
            let id = if let Some(id) = self.free_wrappers.pop() {
                self.wrappers[id] = Some(binding);
                id
            } else {
                let id = self.wrappers.len();
                self.wrappers.push(Some(binding));
                id
            };
            self.by_position.insert(pos, id);
            if self.ticking {
                self.pending.push(id)
            } else {
                self.active.push(id)
            }
        }
        Ok(())
    }

    pub fn remove_ticker(&mut self, pos: BlockPos) {
        if let Some(id) = self.by_position.remove(&pos) {
            self.wrappers[id] = None;
        }
    }

    pub fn clear_chunk_tickers(&mut self, positions: impl IntoIterator<Item = BlockPos>) {
        for pos in positions {
            self.remove_ticker(pos);
        }
    }

    pub fn registered_count(&self) -> usize {
        self.by_position.len()
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn tick(
        &mut self,
        backend: &mut impl BlockEntityTickerBackend,
        runs_normally: bool,
    ) -> Result<(), BlockEntityTickerError> {
        self.ticking = true;
        self.active.append(&mut self.pending);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.tick_inner(backend, runs_normally)
        }));
        self.ticking = false;
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn tick_inner(
        &mut self,
        backend: &mut impl BlockEntityTickerBackend,
        runs_normally: bool,
    ) -> Result<(), BlockEntityTickerError> {
        let mut remove = Vec::new();
        let mut ticked = 0usize;
        for index in 0..self.active.len() {
            let id = self.active[index];
            let Some(bound) = self.wrappers[id].as_ref() else {
                remove.push(id);
                continue;
            };
            let pos = bound.pos;
            let kind = bound.kind;
            let entity = backend.live_entity(pos);
            let matches = entity.is_some_and(|entity| {
                !entity.is_removed() && Weak::ptr_eq(&bound.identity, &entity.ticker_identity())
            });
            if !matches {
                remove.push(id);
                continue;
            }
            if !runs_normally || !backend.should_tick_blocks_at(pos) {
                continue;
            }
            if entity.is_some_and(NativeBlockEntity::has_level) && backend.is_ticking(pos) {
                let state = backend.current_state(pos)?;
                if !valid_state(kind, state) {
                    backend.remove_block_entity(pos)?;
                    self.remove_ticker(pos);
                } else {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        backend.tick_body(self, pos, kind)
                    }));
                    let failure = match result {
                        Ok(Ok(())) => None,
                        Ok(Err(BlockEntityTickerError::Unavailable(reason))) => {
                            return Err(BlockEntityTickerError::Unavailable(reason));
                        }
                        Ok(Err(BlockEntityTickerError::Body(reason))) => Some(reason),
                        Err(payload) => Some(
                            payload
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).into()))
                                .unwrap_or_else(|| "native block-entity ticker panic".into()),
                        ),
                    };
                    if let Some(message) = failure {
                        backend.report_tick_exception(pos, &message)?;
                        backend.remove_block_entity(pos)?;
                        self.remove_ticker(pos);
                    }
                }
            }
            ticked += 1;
            if ticked & 7 == 0 {
                backend.mid_tick_tasks()?;
            }
        }
        self.active.retain(|id| !remove.contains(id));
        for id in remove {
            if let Some(bound) = self.wrappers[id].take()
                && self.by_position.get(&bound.pos) == Some(&id)
            {
                self.by_position.remove(&bound.pos);
            }
            self.free_wrappers.push(id);
        }
        Ok(())
    }
}

pub fn server_ticker(entity: &NativeBlockEntity) -> Option<NativeTickerKind> {
    use rivet_registry::block_state_properties::{BlockStateProperties, CreakingHeartState};
    match entity.data() {
        NativeBlockEntityData::Chest(_) => None,
        NativeBlockEntityData::Spawner(_) => Some(NativeTickerKind::Spawner),
        NativeBlockEntityData::Beehive(_) => Some(NativeTickerKind::Beehive),
        NativeBlockEntityData::CreakingHeart(_) => (entity
            .state()
            .get_value(BlockStateProperties::CREAKING_HEART_STATE)
            != Some(CreakingHeartState::Uprooted.into()))
        .then_some(NativeTickerKind::CreakingHeart),
    }
}

fn valid_state(kind: NativeTickerKind, state: BlockState) -> bool {
    match kind {
        NativeTickerKind::Beehive => matches!(
            state.block().name(),
            "minecraft:beehive" | "minecraft:bee_nest"
        ),
        NativeTickerKind::Spawner => state.block().name() == "minecraft:spawner",
        NativeTickerKind::CreakingHeart => state.block().name() == "minecraft:creaking_heart",
    }
}

/// The actual hive server tick, including post-release RNG. These mandatory
/// callbacks are executable world operations, not synthetic events or a
/// blanket release rejection. mark_changed is called only when bees leave.
pub fn tick_beehive<R: RandomSource>(
    hive: &mut BeehiveData,
    random: &mut R,
    cooldown_failed_releases: bool,
    release: impl FnMut(&CompoundTag, bool, Option<BlockPos>) -> Result<bool, BlockEntityTickerError>,
    mut mark_changed: impl FnMut() -> Result<(), BlockEntityTickerError>,
    mut play_work_sound: impl FnMut() -> Result<(), BlockEntityTickerError>,
) -> Result<(), BlockEntityTickerError> {
    if hive.tick_occupants(cooldown_failed_releases, release)? {
        mark_changed()?;
    }
    if hive.has_occupants() && random.next_double() < 0.005 {
        play_work_sound()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::tag_parser::parse_compound_fully;
    use rivet_registry::{
        block_state_properties::{BlockStateProperties, CreakingHeartState},
        generated::blocks::BlockId,
    };
    use rivet_util::random_source::LegacyRandomSource;

    fn fixture() -> impl Iterator<Item = serde_json::Value> {
        include_str!("../../../../../tools/rivet-oracle/fixtures/block-entity-ticker.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
    }
    fn state(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).unwrap())
    }

    #[test]
    fn actual_paper_hive_server_ticks_preserve_age_sound_and_world_rng() {
        let pos = BlockPos::new(3, 64, -8);
        let mut cases = 0;
        for row in fixture().filter(|row| row["kind"] == "hive") {
            let mut hive = BeehiveData::load(
                &parse_compound_fully(row["input_snbt"].as_str().unwrap()).unwrap(),
            );
            let mut random = LegacyRandomSource::new(99);
            let mut sounds = 0;
            for _ in 0..row["ticks"].as_u64().unwrap() {
                tick_beehive(
                    &mut hive,
                    &mut random,
                    row["cooldown"].as_bool().unwrap(),
                    |_, _, _| Ok(false),
                    || panic!("the real stay-in-hive gate prevented release"),
                    || {
                        sounds += 1;
                        Ok(())
                    },
                )
                .unwrap();
            }
            assert_eq!(
                hive.save_with_full_metadata(&pos),
                parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap(),
                "{row}"
            );
            assert_eq!(sounds, row["sounds"].as_u64().unwrap());
            assert_eq!(random.next_long(), row["rng_next"].as_i64().unwrap());
            cases += 1;
        }
        assert_eq!(cases, 30);
    }

    #[test]
    fn actual_paper_server_get_ticker_classifies_all_native_families() {
        let heart = state("minecraft:creaking_heart");
        let states = [
            state("minecraft:chest"),
            state("minecraft:spawner"),
            state("minecraft:bee_nest"),
            state("minecraft:beehive"),
            heart,
            heart
                .set_value(
                    BlockStateProperties::CREAKING_HEART_STATE,
                    CreakingHeartState::Dormant,
                )
                .unwrap(),
            heart
                .set_value(
                    BlockStateProperties::CREAKING_HEART_STATE,
                    CreakingHeartState::Awake,
                )
                .unwrap(),
        ];
        let rows: Vec<_> = fixture()
            .filter(|row| row["kind"] == "classification")
            .collect();
        assert_eq!(states.len(), rows.len());
        for (state, row) in states.into_iter().zip(rows) {
            assert!(
                row["state"]
                    .as_str()
                    .unwrap()
                    .contains(state.block().name())
            );
            let entity = NativeBlockEntity::fresh(BlockPos::ZERO, state).unwrap();
            assert_eq!(
                server_ticker(&entity).is_some(),
                row["ticker"].as_bool().unwrap(),
                "{row}"
            );
        }
    }

    #[derive(Default)]
    struct Backend {
        entities: HashMap<BlockPos, NativeBlockEntity>,
        states: HashMap<BlockPos, BlockState>,
        trace: Vec<String>,
        a_rebound: bool,
        inner_gate: bool,
        fail: bool,
        panic_gate: bool,
    }
    impl Backend {
        fn insert_hive(&mut self, pos: BlockPos) {
            let state = state("minecraft:bee_nest");
            let mut entity = NativeBlockEntity::fresh(pos, state).unwrap();
            entity.associate_level();
            self.entities.insert(pos, entity);
            self.states.insert(pos, state);
        }
    }
    impl BlockEntityTickerBackend for Backend {
        fn live_entity(&self, pos: BlockPos) -> Option<&NativeBlockEntity> {
            self.entities.get(&pos)
        }
        fn live_entity_mut(&mut self, pos: BlockPos) -> Option<&mut NativeBlockEntity> {
            self.entities.get_mut(&pos)
        }
        fn current_state(&self, pos: BlockPos) -> Result<BlockState, BlockEntityTickerError> {
            Ok(self.states[&pos])
        }
        fn on_block_entity_added(&mut self, _: BlockPos) -> Result<(), BlockEntityTickerError> {
            Ok(())
        }
        fn require_ticker_body(
            &self,
            kind: NativeTickerKind,
        ) -> Result<(), BlockEntityTickerError> {
            if kind == NativeTickerKind::Beehive {
                Ok(())
            } else {
                Err(BlockEntityTickerError::Unavailable(format!("{kind:?}")))
            }
        }
        fn should_tick_blocks_at(&self, _: BlockPos) -> bool {
            assert!(!self.panic_gate, "fixture gate failure");
            true
        }
        fn is_ticking(&self, _: BlockPos) -> bool {
            self.inner_gate
        }
        fn tick_body(
            &mut self,
            runtime: &mut LevelBlockEntityRuntime,
            pos: BlockPos,
            _: NativeTickerKind,
        ) -> Result<(), BlockEntityTickerError> {
            if self.fail {
                return Err(BlockEntityTickerError::Body(
                    "actual fixture tick failed".into(),
                ));
            }
            if pos == BlockPos::ZERO {
                if self.a_rebound {
                    self.trace.push("A2".into());
                } else {
                    self.trace.push("A1".into());
                    self.a_rebound = true;
                    let b = BlockPos::new(1, 64, 0);
                    self.insert_hive(b);
                    runtime.update_ticker(self, b, true, true)?;
                    runtime.update_ticker(self, pos, true, true)?;
                }
            } else {
                self.trace.push("B".into());
            }
            Ok(())
        }
        fn remove_block_entity(&mut self, pos: BlockPos) -> Result<(), BlockEntityTickerError> {
            self.entities.remove(&pos);
            self.trace.push("removed".into());
            Ok(())
        }
        fn report_tick_exception(
            &mut self,
            _: BlockPos,
            _: &str,
        ) -> Result<(), BlockEntityTickerError> {
            self.trace.push("exception".into());
            Ok(())
        }
        fn mid_tick_tasks(&mut self) -> Result<(), BlockEntityTickerError> {
            self.trace.push("mid_tick".into());
            Ok(())
        }
    }

    #[test]
    fn actual_paper_wrapper_rebind_pending_freeze_and_null_lifecycle() {
        let mut backend = Backend {
            inner_gate: true,
            ..Backend::default()
        };
        backend.insert_hive(BlockPos::ZERO);
        let mut runtime = LevelBlockEntityRuntime::new();
        runtime
            .register_existing_live(&mut backend, [BlockPos::ZERO], true, true)
            .unwrap();
        runtime.tick(&mut backend, true).unwrap();
        backend.trace.push("after1".into());
        assert_eq!(runtime.pending_count(), 1);
        runtime.tick(&mut backend, true).unwrap();
        backend.trace.push("after2".into());
        runtime.tick(&mut backend, false).unwrap();
        backend.trace.push("frozen".into());
        runtime.remove_ticker(BlockPos::ZERO);
        runtime.tick(&mut backend, true).unwrap();
        backend.trace.push("after_remove".into());
        let c = BlockPos::new(2, 64, 0);
        backend.insert_hive(c);
        runtime.update_ticker(&backend, c, false, true).unwrap();
        runtime.tick(&mut backend, true).unwrap();
        let row = fixture().find(|row| row["kind"] == "lifecycle").unwrap();
        assert_eq!(serde_json::to_value(&backend.trace).unwrap(), row["trace"]);
        assert_eq!(runtime.registered_count(), 1);
    }

    #[test]
    fn missing_body_refuses_registration_and_real_failures_remove_after_reporting() {
        let mut backend = Backend {
            inner_gate: true,
            fail: true,
            ..Backend::default()
        };
        backend.insert_hive(BlockPos::ZERO);
        let mut runtime = LevelBlockEntityRuntime::new();
        runtime
            .register_existing_live(&mut backend, [BlockPos::ZERO], true, true)
            .unwrap();
        runtime.tick(&mut backend, true).unwrap();
        assert_eq!(backend.trace, ["exception", "removed"]);
        assert_eq!(runtime.registered_count(), 0);
        let entity = NativeBlockEntity::fresh(BlockPos::ZERO, state("minecraft:spawner")).unwrap();
        backend.entities.insert(BlockPos::ZERO, entity);
        assert!(matches!(
            runtime.register_existing_live(&mut backend, [BlockPos::ZERO], true, true),
            Err(BlockEntityTickerError::Unavailable(_))
        ));
        assert_eq!(runtime.registered_count(), 0);
    }

    #[test]
    fn gate_panic_does_not_leave_registration_in_pending_mode() {
        let mut backend = Backend {
            inner_gate: true,
            panic_gate: true,
            ..Backend::default()
        };
        backend.insert_hive(BlockPos::ZERO);
        let mut runtime = LevelBlockEntityRuntime::new();
        runtime
            .register_existing_live(&mut backend, [BlockPos::ZERO], true, true)
            .unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || runtime.tick(&mut backend, true)
            ))
            .is_err()
        );
        let other = BlockPos::new(1, 64, 0);
        backend.insert_hive(other);
        runtime.update_ticker(&backend, other, true, true).unwrap();
        assert_eq!(runtime.pending_count(), 0);
    }
}

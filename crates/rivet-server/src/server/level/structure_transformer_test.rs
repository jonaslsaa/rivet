use super::*;
use rivet_registry::generated::blocks::BlockId;
use rivet_world::level::height_accessor::LevelHeightAccessor;
use std::{collections::HashMap, sync::Mutex};

#[derive(Debug)]
struct Entity {
    position: [f64; 3],
    alive: bool,
    generation: bool,
    id: i32,
}
impl StructureEntity for Entity {
    fn position(&self) -> [f64; 3] {
        self.position
    }
    fn is_alive(&self) -> bool {
        self.alive
    }
    fn set_generation(&mut self, value: bool) {
        self.generation = value;
    }
}
fn entity(id: i32, position: [f64; 3]) -> Entity {
    Entity {
        position,
        alive: true,
        generation: false,
        id,
    }
}
fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).unwrap())
}
#[derive(Default)]
struct Backend {
    states: HashMap<BlockPos, BlockState>,
    trace: Vec<String>,
    loaded: HashMap<ChunkPos, Vec<Entity>>,
    admitted: Vec<i32>,
    fail_admission: Option<i32>,
}
impl LevelHeightAccessor for Backend {
    fn get_min_y(&self) -> i32 {
        -64
    }
    fn get_height(&self) -> i32 {
        384
    }
}
impl WorldGenLevel for Backend {
    fn get_seed(&self) -> i64 {
        42
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states
            .get(pos)
            .copied()
            .unwrap_or_else(|| block("minecraft:stone"))
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.trace
            .push(format!("write:{}:{flags}", state.block().name()));
        self.states.insert(*pos, state);
        true
    }
    fn schedule_tick(&mut self, _: &BlockPos, fluid: FluidId, delay: i32) {
        self.trace.push(format!("tick:{}:{delay}", fluid.id()));
    }
    fn mark_pos_for_post_processing(&mut self, _: &BlockPos) {
        self.trace.push("mark".into());
    }
}
impl StructureTransformBackend<Entity> for Backend {
    fn fresh_snapshot(
        &mut self,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<CraftBlockSnapshot, StructureError> {
        self.trace.push("fresh".into());
        super::super::structure_snapshot_data::fresh_snapshot(pos, state)
    }
    fn world_snapshot(&mut self, pos: BlockPos) -> Result<CraftBlockSnapshot, StructureError> {
        self.trace.push("world".into());
        super::super::structure_snapshot_data::fresh_snapshot(pos, self.get_block_state(&pos))
    }
    fn load_snapshot(&mut self, _: BlockPos, data: &CompoundTag) -> Result<(), StructureError> {
        self.trace
            .push(format!("load:{}", data.get_string("id").unwrap()));
        Ok(())
    }
    fn add_entity_with_passengers(&mut self, e: Entity) -> Result<bool, StructureError> {
        self.trace.push(format!("admit:{}", e.id));
        if self.fail_admission == Some(e.id) {
            return Err(StructureError::PlacementUnavailable(
                "actual admission failure".into(),
            ));
        }
        self.admitted.push(e.id);
        Ok(true)
    }
    fn load_chunk_entities(&mut self, p: ChunkPos) -> Result<Vec<Entity>, StructureError> {
        self.trace
            .push(format!("load_entities:{},{}", p.x(), p.z()));
        Ok(self.loaded.remove(&p).unwrap_or_default())
    }
    fn clear_chunk_entities(&mut self, p: ChunkPos) -> Result<(), StructureError> {
        self.trace
            .push(format!("clear_entities:{},{}", p.x(), p.z()));
        Ok(())
    }
}

#[test]
fn source_snapshot_copies_reset_between_callbacks_and_write_effects_keep_source_order() {
    let p = BlockPos::new(8, 64, 8);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let first: BlockTransformer<Entity> = Arc::new(|_, pos, mut incoming, changes| {
        assert_eq!(pos, BlockPos::new(8, 64, 8));
        assert_eq!(changes.original().state, block("minecraft:oak_fence"));
        changes.original().state = block("minecraft:air");
        changes.world().state = block("minecraft:air");
        assert_eq!(changes.original().state, block("minecraft:air"));
        incoming.state = block("minecraft:wall_torch");
        Ok(incoming)
    });
    let output = seen.clone();
    let second: BlockTransformer<Entity> = Arc::new(move |_, _, mut incoming, changes| {
        output.lock().unwrap().push(incoming.state);
        assert_eq!(changes.original().state, block("minecraft:oak_fence"));
        assert_eq!(changes.world().state, block("minecraft:stone"));
        incoming.state = block("minecraft:oak_fence")
            .set_value(
                rivet_registry::block_state_properties::BlockStateProperties::WATERLOGGED,
                true,
            )
            .unwrap();
        Ok(incoming)
    });
    let mut transformer =
        StructureTransformer::new(ChunkPos::ZERO, -64, 384, vec![first, second], vec![]);
    let mut backend = Backend::default();
    transformer
        .set_block(&mut backend, p, block("minecraft:oak_fence"), 2)
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), [block("minecraft:wall_torch")]);
    assert_eq!(
        backend.trace,
        [
            "fresh",
            "world",
            "write:minecraft:oak_fence:2",
            "tick:2:0",
            "mark"
        ]
    );
    // Explicit Craft BE placement without block transformers has no accessor
    // fluid/shape bookkeeping, but MUST load its full snapshot after writing.
    let mut direct = StructureTransformer::<Entity>::new(ChunkPos::ZERO, -64, 384, vec![], vec![]);
    backend.trace.clear();
    let snapshot = super::super::structure_snapshot_data::loot_chest_snapshot(
        p,
        block("minecraft:chest"),
        "minecraft:chests/buried_treasure",
        42,
    )
    .unwrap();
    direct.place_snapshot(&mut backend, p, snapshot, 2).unwrap();
    assert_eq!(
        backend.trace,
        ["write:minecraft:chest:2", "load:minecraft:chest"]
    );
}

#[test]
fn source_entity_chain_uses_original_coordinates_and_does_not_short_circuit_false() {
    let first: EntityTransformer<Entity> = Arc::new(|region, pos, entity, allowed| {
        assert!(allowed && entity.generation);
        assert_eq!(pos, BlockPos::new(-1, 64, 2));
        assert!(region.contains([-16.0, -64.0, -16.0]));
        assert!(!region.contains([32.0, 64.0, 2.0]));
        region.stage_entity(super::tests::entity(10, [1.0, 64.0, 1.0]))?;
        entity.position = [20.0, 70.0, 20.0];
        Ok(false)
    });
    let second: EntityTransformer<Entity> = Arc::new(|_, pos, _, allowed| {
        assert!(!allowed);
        assert_eq!(pos, BlockPos::new(-1, 64, 2));
        Ok(true)
    });
    let mut transformer =
        StructureTransformer::new(ChunkPos::ZERO, -64, 384, vec![], vec![first, second]);
    let mut backend = Backend::default();
    let mut direct = entity(1, [-0.25, 64.75, 2.5]);
    assert!(
        transformer
            .transform_entity(&mut backend, &mut direct)
            .unwrap()
    );
    assert!(
        backend.admitted.is_empty(),
        "accessor caller, not transformer, performs direct admission"
    );
    transformer.finish(&mut backend).unwrap();
    assert_eq!(backend.admitted, [10]);
    assert_eq!(
        backend.trace,
        ["admit:10"],
        "unqueried existing chunk entities are not cleared"
    );
}

#[test]
fn source_lazy_entity_clear_order_partial_commit_and_abort_preserve_world_effects() {
    let callback: EntityTransformer<Entity> = Arc::new(|region, _, _, _| {
        let existing = region.entities()?;
        assert_eq!(existing.iter().map(|e| e.id).collect::<Vec<_>>(), [1]);
        assert!(existing[0].generation);
        region.stage_entity(entity(2, [3.0, 64.0, 3.0]))?;
        Ok(true)
    });
    let mut transformer =
        StructureTransformer::new(ChunkPos::ZERO, -64, 384, vec![], vec![callback]);
    let mut backend = Backend::default();
    backend.loaded.insert(
        ChunkPos::new(-1, -1),
        vec![entity(1, [1.0, 64.0, 1.0]), entity(3, [50.0, 64.0, 50.0])],
    );
    backend.fail_admission = Some(2);
    let mut direct = entity(9, [0.0, 64.0, 0.0]);
    transformer
        .transform_entity(&mut backend, &mut direct)
        .unwrap();
    backend.set_block(&BlockPos::ZERO, block("minecraft:oak_planks"), 2);
    assert_eq!(
        transformer.finish(&mut backend),
        Err(StructureError::PlacementUnavailable(
            "actual admission failure".into()
        ))
    );
    transformer.abort();
    transformer.abort();
    assert_eq!(backend.admitted, [1]);
    assert_eq!(
        backend.get_block_state(&BlockPos::ZERO),
        block("minecraft:oak_planks")
    );
    let expected: Vec<String> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| format!("clear_entities:{x},{z}")))
        .collect();
    assert_eq!(
        backend
            .trace
            .iter()
            .filter(|v| v.starts_with("clear_entities"))
            .cloned()
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        backend
            .trace
            .ends_with(&["admit:1".into(), "admit:2".into()])
    );
    assert!(!transformer.can_transform_blocks());
}

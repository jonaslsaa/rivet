//! Direct state.updateShape output, tick ordering and RNG against real Paper.
use super::update_shape;
use crate::server::level::world_gen_region::region_can_survive;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::block_states::StateId;
use rivet_util::RandomSource;
use rivet_util::random_source::XoroshiroRandomSource;
use rivet_util::worldgen_random::AlgorithmRandomSource;
use rivet_world::block::Block;
use rivet_world::block::blocks::Blocks;
use rivet_world::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
use std::collections::HashMap;

#[derive(Default)]
struct Storage {
    states: HashMap<BlockPos, BlockState>,
    ticks: Vec<serde_json::Value>,
}
impl Storage {
    fn tick(&mut self, pos: &BlockPos, name: &str, delay: i32) {
        self.ticks.push(serde_json::json!([
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            name,
            delay
        ]));
    }
}
impl LevelHeightAccessor for Storage {
    fn get_height(&self) -> i32 {
        384
    }
    fn get_min_y(&self) -> i32 {
        -64
    }
}
impl WorldGenLevel for Storage {
    fn get_seed(&self) -> i64 {
        42
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states
            .get(pos)
            .copied()
            .unwrap_or_else(|| Blocks::AIR.default_block_state())
    }
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        region_can_survive(self, state, pos)
    }
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.tick(pos, fluid.name(), delay);
    }
    fn schedule_block_tick(&mut self, pos: &BlockPos, block: Block, delay: i32) {
        self.tick(pos, block.name(), delay);
    }
}

#[test]
fn tree_and_water_shape_updates_match_live_paper() {
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/block-shape-behavior.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let state = |field: &str| {
            BlockState::new(StateId(
                u16::try_from(row[field].as_u64().unwrap()).unwrap(),
            ))
        };
        let pos = BlockPos::new(3, 70, -5);
        let direction = Direction::VALUES
            .into_iter()
            .find(|dir| dir.get_serialized_name() == row["direction"].as_str().unwrap())
            .unwrap();
        let mut level = Storage::default();
        level.states.insert(pos, state("state"));
        level.states.insert(pos.above(), state("above"));
        level.states.insert(pos.below(), state("below"));
        level
            .states
            .insert(pos.relative(&direction), state("neighbor"));
        let mut random = AlgorithmRandomSource::Xoroshiro(XoroshiroRandomSource::new(42));
        let result = update_shape(
            &mut level,
            &mut random,
            state("state"),
            &pos,
            direction,
            state("neighbor"),
        );
        assert_eq!(result, state("result"), "result {}", row["name"]);
        assert_eq!(
            serde_json::Value::Array(level.ticks),
            row["ticks"],
            "ticks {}",
            row["name"]
        );
        assert_eq!(
            random.next_long(),
            row["next_long"].as_i64().unwrap(),
            "RNG {}",
            row["name"]
        );
    }
}

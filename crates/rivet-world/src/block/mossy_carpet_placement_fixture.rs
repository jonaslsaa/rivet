//! Ordered writes and level-RNG continuation from actual Paper placeAt.
use super::place_at;
use crate::block::blocks::Blocks;
use crate::level::{WorldGenLevel, height_accessor::LevelHeightAccessor};
use rivet_registry::{block_state::BlockState, core::BlockPos, generated::block_states::StateId};
use rivet_util::{RandomSource, random_source::XoroshiroRandomSource};
use std::collections::HashMap;

#[derive(Default)]
struct Storage {
    states: HashMap<BlockPos, BlockState>,
    writes: Vec<serde_json::Value>,
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
        0
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states
            .get(pos)
            .copied()
            .unwrap_or_else(|| Blocks::AIR.default_block_state())
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.writes.push(serde_json::json!([
            pos.get_x(),
            pos.get_y(),
            pos.get_z(),
            state.id().0,
            flags
        ]));
        self.states.insert(*pos, state) != Some(state)
    }
}

#[test]
fn moss_carpet_placement_matches_live_paper() {
    for line in
        include_str!("../../../../tools/rivet-oracle/fixtures/moss-carpet-placement.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut level = Storage::default();
        for cell in row["initial"].as_array().unwrap() {
            let coordinate = |i: usize| i32::try_from(cell[i].as_i64().unwrap()).unwrap();
            level.states.insert(
                BlockPos::new(coordinate(0), coordinate(1), coordinate(2)),
                BlockState::new(StateId(u16::try_from(cell[3].as_u64().unwrap()).unwrap())),
            );
        }
        let mut random = XoroshiroRandomSource::new(row["seed"].as_i64().unwrap());
        place_at(&mut level, &BlockPos::new(3, 70, -5), &mut random, 2);
        assert_eq!(
            serde_json::Value::Array(level.writes),
            row["writes"],
            "writes mask={} seed={} condition={}",
            row["mask"],
            row["seed"],
            row["condition"]
        );
        assert_eq!(
            random.next_long(),
            row["next_long"].as_i64().unwrap(),
            "RNG mask={} seed={} condition={}",
            row["mask"],
            row["seed"],
            row["condition"]
        );
    }
}

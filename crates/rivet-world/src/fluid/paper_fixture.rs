//! Storage/event boundary for the genuine Paper FlowingFluid oracle.
use super::{containers, flowing::FluidWorld, *};
use crate::{
    block::{Block, liquid_block::LiquidBlockWorld},
    level::block_shape_context::collision_geometry,
};
use rivet_registry::{
    core::{BlockPos, Direction},
    generated::block_states::StateId,
};
use rivet_util::{random::LegacyRandomSource, random_source::RandomSource};
use std::collections::HashMap;

pub(crate) fn position(pos: BlockPos) -> String {
    format!("{},{},{}", pos.get_x(), pos.get_y(), pos.get_z())
}
pub(crate) fn parse_position(text: &str) -> BlockPos {
    let v: Vec<i32> = text.split(',').map(|s| s.parse().unwrap()).collect();
    BlockPos::new(v[0], v[1], v[2])
}
pub(crate) fn state(id: u64) -> BlockState {
    BlockState::new(StateId(id as u16))
}

pub(crate) struct World {
    pub blocks: HashMap<BlockPos, BlockState>,
    pub trace: Vec<String>,
    pub floor: i32,
    pub fast: bool,
    pub unloaded_east: bool,
    pub policy: String,
    pub random: LegacyRandomSource,
    pub water_over_lava_flow_speed: i32,
}
impl World {
    pub(crate) fn new(row: &serde_json::Value) -> Self {
        Self {
            blocks: row["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        parse_position(r["pos"].as_str().unwrap()),
                        state(r["state"].as_u64().unwrap()),
                    )
                })
                .collect(),
            trace: Vec::new(),
            floor: row["floor"].as_i64().unwrap() as i32,
            fast: row["fast"].as_bool().unwrap(),
            unloaded_east: row["name"] == "unloaded_east",
            policy: row["name"].as_str().unwrap().to_owned(),
            random: LegacyRandomSource::new(99),
            water_over_lava_flow_speed: 5,
        }
    }
    pub(crate) fn raw(&self, pos: BlockPos) -> BlockState {
        self.blocks.get(&pos).copied().unwrap_or_else(|| {
            BlockState::of(
                BlockId::from_name(if pos.get_y() < self.floor {
                    "minecraft:stone"
                } else {
                    "minecraft:air"
                })
                .unwrap(),
            )
        })
    }
}
impl FluidWorld for World {
    fn block(&mut self, pos: BlockPos) -> FluidResult<BlockState> {
        self.trace.push(format!("get:{}", position(pos)));
        Ok(self.raw(pos))
    }
    fn block_if_loaded(&mut self, pos: BlockPos) -> FluidResult<Option<BlockState>> {
        self.trace
            .push(format!("cache:{},{}", pos.get_x() >> 4, pos.get_z() >> 4));
        if self.unloaded_east && pos.get_x() >> 4 > 0 {
            return Ok(None);
        }
        self.trace.push(format!("peek:{}", position(pos)));
        Ok(Some(self.raw(pos)))
    }
    fn set_block(&mut self, pos: BlockPos, state: BlockState, flags: u32) -> FluidResult<bool> {
        self.trace
            .push(format!("set:{}:{}:{flags}", position(pos), state.id().0));
        let previous = self.blocks.insert(pos, state);
        Ok(previous != Some(state))
    }
    fn schedule(&mut self, pos: BlockPos, fluid: FluidId, delay: i32) -> FluidResult<()> {
        self.trace
            .push(format!("schedule:{}:{}:{delay}", position(pos), fluid.0));
        Ok(())
    }
    fn random_bound(&mut self, bound: i32) -> i32 {
        self.random.next_int_bound(bound)
    }
    fn fast_lava(&self) -> bool {
        self.fast
    }
    fn source_conversion(&self, _kind: FluidKind) -> bool {
        true
    }
    fn can_pass_wall(
        &mut self,
        from: BlockPos,
        a: BlockState,
        direction: Direction,
        to: BlockPos,
        b: BlockState,
    ) -> FluidResult<bool> {
        let a = collision_geometry(&a, &from)
            .ok_or_else(|| FluidError::Unavailable("oracle dynamic source collision".into()))?;
        let b = collision_geometry(&b, &to)
            .ok_or_else(|| FluidError::Unavailable("oracle dynamic target collision".into()))?;
        Ok(!a.merged_face_occludes(&b, direction))
    }
    fn is_liquid_container(&self, state: BlockState) -> FluidResult<bool> {
        Ok(containers::behavior(state)?.is_container())
    }
    fn can_place_liquid(
        &mut self,
        _pos: BlockPos,
        state: BlockState,
        fluid: FluidId,
    ) -> FluidResult<bool> {
        containers::can_place(state, fluid)
    }
    fn place_liquid(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        fluid: FluidState,
    ) -> FluidResult<bool> {
        containers::place_standard(self, pos, state, fluid)
    }
    fn excluded_from_holding(&self, state: BlockState) -> FluidResult<bool> {
        Ok(containers::behavior(state)?.excluded)
    }
    fn level_change_event(
        &mut self,
        pos: BlockPos,
        new: BlockState,
    ) -> FluidResult<Option<BlockState>> {
        self.trace
            .push(format!("level:{}:{}", position(pos), new.id().0));
        if self.policy == "cancel_level" {
            return Ok(None);
        }
        Ok(Some(if self.policy == "alter_level" {
            new.set_value(P::LEVEL, 7).unwrap()
        } else {
            new
        }))
    }
    fn dispatch_from_to_event(
        &mut self,
        _source: BlockPos,
        direction: Direction,
    ) -> FluidResult<bool> {
        self.trace.push(format!(
            "flow:{}",
            direction.get_name().to_ascii_uppercase()
        ));
        Ok(
            !(self.policy == "cancel_down" && direction == Direction::Down
                || self.policy == "cancel_north" && direction == Direction::North),
        )
    }
    fn form_block_event(
        &mut self,
        pos: BlockPos,
        new: BlockState,
        flags: u32,
    ) -> FluidResult<bool> {
        self.trace.push(format!("form:{}", new.id().0));
        if self.policy == "cancel_form" {
            return Ok(false);
        }
        let new = if self.policy == "alter_form" {
            BlockState::of(BlockId::from_name("minecraft:glass").unwrap())
        } else {
            new
        };
        self.set_block(pos, new, flags)?;
        Ok(true)
    }
    fn drop_resources(
        &mut self,
        _pos: BlockPos,
        state: BlockState,
        _source: BlockPos,
    ) -> FluidResult<()> {
        if state.is_air() {
            Ok(())
        } else {
            Err(FluidError::Unavailable(format!(
                "fixture drop resources {}",
                state.block().name()
            )))
        }
    }
    fn lava_fizz(&mut self, pos: BlockPos) -> FluidResult<()> {
        self.trace.push(format!("event:{}:1501:0", position(pos)));
        Ok(())
    }
}
impl LiquidBlockWorld for World {
    fn water_over_lava_flow_speed(&self) -> i32 {
        self.water_over_lava_flow_speed
    }
    fn schedule_block(&mut self, pos: BlockPos, block: Block, delay: i32) -> FluidResult<()> {
        self.trace.push(format!(
            "schedule_block:{}:{}:{delay}",
            position(pos),
            block.id().0
        ));
        Ok(())
    }
}

#[test]
fn scheduled_fluid_trace_matches_live_paper() {
    for line in include_str!("../../../../tools/rivet-oracle/fixtures/flowing-fluid.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut world = World::new(&row);
        let pos = parse_position(row["origin"].as_str().unwrap());
        let initial = state(row["initial"].as_u64().unwrap());
        flowing::FlowingFluid::new(if row["lava"].as_bool().unwrap() {
            FluidKind::Lava
        } else {
            FluidKind::Water
        })
        .tick(
            &mut world,
            pos,
            initial,
            FluidState::from_block(initial).unwrap(),
        )
        .unwrap();
        let expected: Vec<String> = serde_json::from_value(row["trace"].clone()).unwrap();
        assert_eq!(
            world.trace, expected,
            "{} lava={} fast={}",
            row["name"], row["lava"], row["fast"]
        );
        assert_eq!(
            world.random.next_long(),
            row["next_long"].as_i64().unwrap(),
            "{} RNG",
            row["name"]
        );
    }
}

#[test]
fn opposing_collision_face_unions_match_live_paper() {
    let pos = BlockPos::new(0, 64, 0);
    let mut count = 0;
    for line in
        include_str!("../../../../tools/rivet-oracle/fixtures/fluid-collision-faces.tsv").lines()
    {
        let fields: Vec<_> = line.split('\t').collect();
        let a = state(fields[0].parse().unwrap());
        let b = state(fields[1].parse().unwrap());
        let direction = Direction::from_3d_data_value(fields[2].parse().unwrap());
        let shape_a = collision_geometry(&a, &pos).unwrap();
        let shape_b = collision_geometry(&b, &pos.relative(&direction)).unwrap();
        assert_eq!(
            shape_a.merged_face_occludes(&shape_b, direction),
            fields[3] == "1",
            "{line} {} {}",
            a.block().name(),
            b.block().name()
        );
        count += 1;
    }
    assert_eq!(count, 13254);
}

#[test]
fn every_registered_block_fluid_state_matches_live_paper_projection() {
    use rivet_registry::generated::block_states::BLOCK_STATE_COUNT;
    use sha2::{Digest, Sha256};
    for line in
        include_str!("../../../../tools/rivet-oracle/fixtures/fluid-block-states.tsv").lines()
    {
        let fields: Vec<_> = line.split('\t').collect();
        let start: u16 = fields[0].parse().unwrap();
        let length: u16 = fields[1].parse().unwrap();
        for id in start..start + length {
            let fluid = FluidState::from_block(BlockState::new(StateId(id))).unwrap();
            assert_eq!(
                format!(
                    "{},{},{}",
                    fluid.id().0,
                    fluid.amount(),
                    u8::from(fluid.falling())
                ),
                fields[2],
                "state {id} {}",
                BlockState::new(StateId(id)).block().name()
            );
        }
    }
    let mut hash = Sha256::new();
    let mut previous = None;
    let mut start = 0;
    let mut length = 0;
    let mut runs = 0;
    for id in 0..BLOCK_STATE_COUNT {
        let value = FluidState::from_block(BlockState::new(StateId(id))).unwrap();
        let value = format!(
            "{},{},{}",
            value.id().0,
            value.amount(),
            u8::from(value.falling())
        );
        if previous.as_ref().is_some_and(|old| old != &value) {
            hash.update(format!(
                "{start}\t{length}\t{}\n",
                previous.as_ref().unwrap()
            ));
            runs += 1;
            start = id;
            length = 0;
        }
        previous = Some(value);
        length += 1;
    }
    hash.update(format!("{start}\t{length}\t{}\n", previous.unwrap()));
    runs += 1;
    assert_eq!(runs, 15752);
    assert_eq!(
        format!("{:x}", hash.finalize()),
        "916e9cd513aa7d2171b701720acdb35d9ece2a343af2b3f9d529b00bf2c3d01e"
    );
}

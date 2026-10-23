//! LiquidBlock placement/neighbour callbacks. Fluid simulation and block
//! callbacks are distinct: mixing happens here, bubble propagation in tick.
use super::{Block, bubble_column_block};
use crate::fluid::{FluidError, FluidKind, FluidResult, FluidState, flowing::FluidWorld};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, Direction},
    generated::blocks::BlockId,
};

pub trait LiquidBlockWorld: FluidWorld {
    /// Resolved Paper per-world environment.waterOverLavaFlowSpeed.
    fn water_over_lava_flow_speed(&self) -> i32;
    fn schedule_block(&mut self, pos: BlockPos, block: Block, delay: i32) -> FluidResult<()>;
}

fn kind(state: BlockState) -> FluidResult<FluidKind> {
    match state.block().name() {
        "minecraft:water" => Ok(FluidKind::Water),
        "minecraft:lava" => Ok(FluidKind::Lava),
        _ => Err(FluidError::InvalidState(
            "LiquidBlock callback on non-liquid block".into(),
        )),
    }
}

pub fn flow_speed(
    world: &mut impl LiquidBlockWorld,
    pos: BlockPos,
    state: BlockState,
) -> FluidResult<i32> {
    let fluid = kind(state)?;
    if fluid == FluidKind::Water {
        for neighbor in [pos.north(), pos.south(), pos.west(), pos.east()] {
            if let Some(block) = world.block_if_loaded(neighbor)?
                && FluidState::from_block(block)?.kind() == Some(FluidKind::Lava)
            {
                return Ok(world.water_over_lava_flow_speed());
            }
        }
    }
    Ok(fluid.tick_delay(world.fast_lava()))
}

fn should_spread(
    world: &mut impl LiquidBlockWorld,
    pos: BlockPos,
    state: BlockState,
) -> FluidResult<bool> {
    if kind(state)? != FluidKind::Lava {
        return Ok(true);
    }
    let soul_soil = world.block(pos.below())?.block().name() == "minecraft:soul_soil";
    // Source iterates possible flow directions, then reads their opposites.
    for direction in [
        Direction::Down,
        Direction::South,
        Direction::North,
        Direction::East,
        Direction::West,
    ] {
        let neighbor = pos.relative(&direction.get_opposite());
        let fluid = FluidState::from_block(world.block(neighbor)?)?;
        let replacement = if fluid.kind() == Some(FluidKind::Water) {
            Some(if FluidState::from_block(world.block(pos)?)?.is_source() {
                "minecraft:obsidian"
            } else {
                "minecraft:cobblestone"
            })
        } else if soul_soil && world.block(neighbor)?.block().name() == "minecraft:blue_ice" {
            Some("minecraft:basalt")
        } else {
            None
        };
        if let Some(name) = replacement {
            if world.form_block_event(pos, BlockState::of(BlockId::from_name(name).unwrap()), 3)? {
                world.lava_fizz(pos)?;
            }
            // Cancellation does not resume scheduling or check other faces.
            return Ok(false);
        }
    }
    Ok(true)
}

/// Both onPlace and neighborChanged execute this identical source body.
pub fn on_place_or_neighbor_changed(
    world: &mut impl LiquidBlockWorld,
    pos: BlockPos,
    state: BlockState,
) -> FluidResult<()> {
    if should_spread(world, pos, state)? {
        let speed = flow_speed(world, pos, state)?;
        world.schedule(pos, FluidState::from_block(state)?.id(), speed)?;
    }
    if bubble_column_block::can_occupy(super::blocks::Blocks::BUBBLE_COLUMN, state) {
        let below = world.block(pos.below())?;
        if below.is_in_tag("minecraft:enables_bubble_column_drag_down")
            || below.is_in_tag("minecraft:enables_bubble_column_push_up")
        {
            world.schedule_block(pos, Block::new(state.block()), 20)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluid::paper_fixture::{World, parse_position, state};
    use rivet_util::random_source::RandomSource;

    #[test]
    fn liquid_callbacks_match_live_paper() {
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/liquid-block.jsonl").lines()
        {
            let mut row: serde_json::Value = serde_json::from_str(line).unwrap();
            // The Java storage shell has a stone floor at y=64 in every case.
            row["floor"] = 64.into();
            let mut world = World::new(&row);
            world.policy = row["policy"].as_str().unwrap().to_owned();
            world.unloaded_east = row["name"] == "water_unloaded";
            world.water_over_lava_flow_speed = 17;
            on_place_or_neighbor_changed(
                &mut world,
                parse_position(row["origin"].as_str().unwrap()),
                state(row["initial"].as_u64().unwrap()),
            )
            .unwrap();
            let effects: Vec<_> = world
                .trace
                .iter()
                .filter(|s| {
                    !s.starts_with("get:") && !s.starts_with("cache:") && !s.starts_with("peek:")
                })
                .cloned()
                .collect();
            let expected: Vec<String> = serde_json::from_value(row["effects"].clone()).unwrap();
            assert_eq!(effects, expected, "{row}");
            assert_eq!(
                world.random.next_long(),
                row["next_long"].as_i64().unwrap(),
                "{row}"
            );
        }
    }
}

//! Actual LiquidBlock.tick and BubbleColumnBlock.updateColumn. Reads and
//! writes use the supplied world; setBlock must include its real live effects.
use super::{Block, blocks::Blocks};
use crate::level::WorldGenLevel;
use rivet_registry::{
    block_state::BlockState,
    block_state_property::Property,
    core::BlockPos,
    fluid_id::FluidId,
    generated::{block_properties::BlockPropertyId, tags::FLUID_TAG_BY_NAME},
};

const DRAG: Property = Property::from_id(BlockPropertyId::Drag);

/// Shared storage boundary for generation facades and fallible live mutation.
/// An unchanged write is distinct from a runtime failure.
pub trait ColumnWorld {
    type Error;
    fn column_block(&mut self, pos: BlockPos) -> Result<BlockState, Self::Error>;
    fn column_set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Result<bool, Self::Error>;
}

impl<W: WorldGenLevel + ?Sized> ColumnWorld for W {
    type Error = std::convert::Infallible;
    fn column_block(&mut self, pos: BlockPos) -> Result<BlockState, Self::Error> {
        Ok(self.get_block_state(&pos))
    }
    fn column_set_block(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Result<bool, Self::Error> {
        Ok(self.set_block(&pos, state, flags))
    }
}

fn full_source_can_occupy(state: BlockState) -> bool {
    let fluid = FluidId::from_id(state.fluid_id());
    // The two registered source fluids have amount eight and are full.
    // Flowing fluid IDs remain distinct even at falling amount eight.
    matches!(fluid, FluidId::WATER | FluidId::LAVA)
        && FLUID_TAG_BY_NAME["minecraft:bubble_column_can_occupy"].contains(&fluid.name())
}

pub fn can_occupy(column: Block, state: BlockState) -> bool {
    state.block() == column.id()
        || (matches!(state.block().name(), "minecraft:water" | "minecraft:lava")
            && full_source_can_occupy(state))
}

fn column_state(column: Block, below: BlockState, current: BlockState) -> BlockState {
    if below.block() == column.id() {
        below
    } else if below.is_in_tag("minecraft:enables_bubble_column_push_up") {
        column
            .default_block_state()
            .set_value(DRAG, false)
            .expect("bubble column drag property")
    } else if below.is_in_tag("minecraft:enables_bubble_column_drag_down") {
        column
            .default_block_state()
            .set_value(DRAG, true)
            .expect("bubble column drag property")
    } else if current.block() == column.id() {
        Blocks::WATER.default_block_state()
    } else {
        current
    }
}

pub fn update_column<W: ColumnWorld + ?Sized>(
    column: Block,
    level: &mut W,
    pos: BlockPos,
    current: BlockState,
    below: BlockState,
) -> Result<(), W::Error> {
    if !can_occupy(column, current) {
        return Ok(());
    }
    let state = column_state(column, below, current);
    // Paper deliberately ignores the initial result, even if the block was
    // already identical. Only a later failed/unchanged write stops the ascent.
    level.column_set_block(pos, state, 2)?;
    let mut next = pos.above();
    while can_occupy(column, level.column_block(next)?) {
        if !level.column_set_block(next, state, 2)? {
            return Ok(());
        }
        next = next.above();
    }
    Ok(())
}

/// LiquidBlock.tick; lava mixing belongs to onPlace/neighborChanged, not here.
pub fn liquid_tick<W: ColumnWorld + ?Sized>(
    level: &mut W,
    state: BlockState,
    pos: BlockPos,
) -> Result<(), W::Error> {
    if full_source_can_occupy(state) {
        let below = level.column_block(pos.below())?;
        // The four-argument Java overload re-reads the current position.
        let current = level.column_block(pos)?;
        update_column(Blocks::BUBBLE_COLUMN, level, pos, current, below)?;
    }
    Ok(())
}

pub fn bubble_tick<W: ColumnWorld + ?Sized>(
    level: &mut W,
    state: BlockState,
    pos: BlockPos,
) -> Result<(), W::Error> {
    let below = level.column_block(pos.below())?;
    update_column(Block::new(state.block()), level, pos, state, below)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_registry::generated::block_states::StateId;
    use std::collections::HashMap;

    #[derive(serde::Deserialize)]
    struct Case {
        current: u16,
        below: u16,
        middle: u16,
        reject: usize,
        writes: Vec<(i32, u16, u32, bool)>,
        end: Vec<u16>,
    }
    struct World {
        cells: HashMap<BlockPos, BlockState>,
        writes: Vec<(i32, u16, u32, bool)>,
        reject: usize,
    }
    impl LevelHeightAccessor for World {
        fn get_min_y(&self) -> i32 {
            -64
        }
        fn get_height(&self) -> i32 {
            384
        }
    }
    impl WorldGenLevel for World {
        fn get_seed(&self) -> i64 {
            42
        }
        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.cells
                .get(pos)
                .copied()
                .unwrap_or(Blocks::AIR.default_block_state())
        }
        fn is_empty_block(&self, pos: &BlockPos) -> bool {
            self.get_block_state(pos).is_air()
        }
        fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
            let accepted =
                self.writes.len() + 1 != self.reject && self.get_block_state(pos) != state;
            self.writes
                .push((pos.get_y(), state.id().0, flags, accepted));
            if accepted {
                self.cells.insert(*pos, state);
            }
            accepted
        }
    }
    #[test]
    fn actual_paper_bubble_column_ticks_preserve_writes_and_stop_conditions() {
        let rows = include_str!("../../../../tools/rivet-oracle/fixtures/bubble-column.jsonl");
        let pos = BlockPos::new(8, 64, 8);
        for (index, line) in rows.lines().enumerate() {
            let case: Case = serde_json::from_str(line).unwrap();
            let state = BlockState::new(StateId(case.current));
            let mut world = World {
                cells: HashMap::from([
                    (pos.below(), BlockState::new(StateId(case.below))),
                    (pos, state),
                    (pos.above(), BlockState::new(StateId(case.middle))),
                    (pos.above_steps(2), Blocks::WATER.default_block_state()),
                    (pos.above_steps(3), Blocks::STONE.default_block_state()),
                ]),
                writes: Vec::new(),
                reject: case.reject,
            };
            if state.block() == Blocks::BUBBLE_COLUMN.id() {
                bubble_tick(&mut world, state, pos).unwrap();
            } else {
                liquid_tick(&mut world, state, pos).unwrap();
            }
            assert_eq!(world.writes, case.writes, "Paper row {index}");
            let end: Vec<_> = (64..68)
                .map(|y| world.get_block_state(&BlockPos::new(8, y, 8)).id().0)
                .collect();
            assert_eq!(end, case.end, "Paper row {index}");
        }
    }
}

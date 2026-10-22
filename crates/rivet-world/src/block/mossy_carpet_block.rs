//! Paper MossyCarpetBlock placement and neighbor-state negotiation.

#[cfg(test)]
#[path = "mossy_carpet_placement_fixture.rs"]
mod placement_fixture;

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::{Property, PropertyValue};
use rivet_registry::core::{BlockPos, Direction, Plane};
use rivet_registry::generated::block_properties::BlockPropertyId;
use rivet_util::random::RandomSource;

fn side_property(direction: Direction) -> Property {
    Property::from_id(match direction {
        Direction::North => BlockPropertyId::North3,
        Direction::East => BlockPropertyId::East3,
        Direction::South => BlockPropertyId::South3,
        Direction::West => BlockPropertyId::West3,
        _ => unreachable!("moss carpet sides are horizontal"),
    })
}

fn is_base(state: BlockState) -> bool {
    state.get_value(BlockStateProperties::BOTTOM) == Some(PropertyValue::Bool(true))
}

pub fn can_survive(level: &dyn WorldGenLevel, state: BlockState, pos: &BlockPos) -> bool {
    let below = level.get_block_state(&pos.below());
    if is_base(state) {
        !below.is_air()
    } else {
        below.block() == Blocks::PALE_MOSS_CARPET.id() && is_base(below)
    }
}

fn has_faces(state: BlockState) -> bool {
    is_base(state)
        || Plane::Horizontal.faces().iter().any(|&direction| {
            state.get_value(side_property(direction)) != Some(PropertyValue::Enum("none"))
        })
}

fn updated_state(
    mut state: BlockState,
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    create_sides: bool,
) -> BlockState {
    let create_sides = create_sides || is_base(state);
    let mut above = None;
    let mut below = None;
    for &direction in Plane::Horizontal.faces() {
        let property = side_property(direction);
        let neighbor_pos = pos.relative(&direction);
        let neighbor = level.get_block_state(&neighbor_pos);
        let mut side = if level.can_attach_to(&neighbor_pos, &neighbor, &direction.get_opposite()) {
            if create_sides {
                PropertyValue::Enum("low")
            } else {
                state.get_value(property).expect("moss side")
            }
        } else {
            PropertyValue::Enum("none")
        };
        if side == PropertyValue::Enum("low") {
            let above = *above.get_or_insert_with(|| level.get_block_state(&pos.above()));
            if above.block() == Blocks::PALE_MOSS_CARPET.id()
                && above.get_value(property) != Some(PropertyValue::Enum("none"))
                && !is_base(above)
            {
                side = PropertyValue::Enum("tall");
            }
            if !is_base(state) {
                let below = *below.get_or_insert_with(|| level.get_block_state(&pos.below()));
                if below.block() == Blocks::PALE_MOSS_CARPET.id()
                    && below.get_value(property) == Some(PropertyValue::Enum("none"))
                {
                    side = PropertyValue::Enum("none");
                }
            }
        }
        state = state.set_value(property, side).expect("moss side value");
    }
    state
}

pub fn update_shape(level: &dyn WorldGenLevel, state: BlockState, pos: &BlockPos) -> BlockState {
    if !can_survive(level, state, pos) {
        return Blocks::AIR.default_block_state();
    }
    let state = updated_state(state, level, pos, false);
    if has_faces(state) {
        state
    } else {
        Blocks::AIR.default_block_state()
    }
}

/// The caller supplies the LEVEL random stream, never the feature stream.
pub fn place_at<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    pos: &BlockPos,
    random: &mut R,
    update_type: u32,
) {
    let base = updated_state(
        Blocks::PALE_MOSS_CARPET.default_block_state(),
        level,
        pos,
        true,
    );
    level.set_block(pos, base, update_type);
    let above_pos = pos.above();
    let previous = level.get_block_state(&above_pos);
    let is_carpet = previous.block() == Blocks::PALE_MOSS_CARPET.id();
    if (is_carpet && is_base(previous)) || (!is_carpet && !previous.can_be_replaced()) {
        return;
    }
    let no_base = Blocks::PALE_MOSS_CARPET
        .default_block_state()
        .set_value(BlockStateProperties::BOTTOM, false)
        .expect("moss base");
    let mut topper = updated_state(no_base, level, &above_pos, true);
    for &direction in Plane::Horizontal.faces() {
        let property = side_property(direction);
        if topper.get_value(property) != Some(PropertyValue::Enum("none")) && !random.next_boolean()
        {
            topper = topper
                .set_value(property, PropertyValue::Enum("none"))
                .expect("moss side");
        }
    }
    if has_faces(topper) && topper != previous {
        level.set_block(&above_pos, topper, update_type);
        let base = updated_state(base, level, pos, true);
        level.set_block(pos, base, update_type);
    }
}

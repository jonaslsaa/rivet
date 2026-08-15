//! STUB(mc.world.level.block) —
//! `net.minecraft.world.level.block.DoublePlantBlock`.
//!
//! The owning `mc.world.level.block` unit (issue #228) has not ported the
//! block class yet; this unit only needs the `placeAt` helper
//! `SimpleBlockFeature.place` consumes for double-plant blocks
//! (`TallGrassBlock`/`TallFlowerBlock`/`PitcherPlantBlock` subclasses). The
//! helper reduces to the existing `WorldGenLevel` write/fluid seams, so it is
//! the minimal faithful surface — the block class itself (and its
//! `getLowerHalf`/`getUpperHalf`/`isDoublePlant` value surface) stays with
//! the owning unit.

use crate::level::WorldGenLevel;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::{BlockStateProperties, DoubleBlockHalf};
use rivet_registry::core::BlockPos;
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::tags::FLUID_TAG_BY_NAME;

/// `DoublePlantBlock.copyWaterloggedFrom(LevelReader, BlockPos, BlockState)`
/// — `state.hasProperty(WATERLOGGED) ? state.setValue(WATERLOGGED,
/// level.isWaterAt(pos)) : state`. In 26.2 `LevelReader.isWaterAt(pos)` is
/// `this.getFluidState(pos).is(FluidTags.WATER)` (LevelReader.java:149): the
/// cell's `FluidState` read directly, with no predicate-indirection layer. The
/// Rust models that read through the `WorldGenLevel::is_fluid_at_position`
/// seam (RivetTodo #232), which resolves the cell's fluid id and applies the
/// predicate.
///
/// `fluid.is(FluidTags.WATER)` is tag membership, not exact-id equality: the
/// `minecraft:water` fluid tag contains both `minecraft:water` and
/// `minecraft:flowing_water` (verified in 26.2 `water.json`), so a cell holding
/// flowing water is waterlogged `true` in Java.
fn copy_waterlogged_from(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    state: BlockState,
) -> BlockState {
    if state.has_property(BlockStateProperties::WATERLOGGED) {
        let water = level.is_fluid_at_position(pos, &|fluid| is_water(*fluid));
        state
            .set_value(BlockStateProperties::WATERLOGGED, water)
            .expect("waterlogged property present, so set_value succeeds")
    } else {
        state
    }
}

/// `fluid.is(FluidTags.WATER)` — the `minecraft:water` fluid-tag membership
/// predicate. Mirrors `BlockState::is_in_tag`: `FLUID_TAG_BY_NAME`'s element
/// names (in tag-file order) are matched against the fluid's canonical name, so
/// both `minecraft:water` and `minecraft:flowing_water` are members. Unknown
/// tags read as `false`, matching `is(TagKey)` on an unbound tag.
fn is_water(fluid: FluidId) -> bool {
    let Some(elements) = FLUID_TAG_BY_NAME.get("minecraft:water") else {
        return false;
    };
    let name = fluid.name();
    // `elements` is a tag-file-ordered slice of fluid names; a linear scan is
    // fine (the water tag has two entries, and the check is not on a hot path).
    elements.contains(&name)
}

/// `DoublePlantBlock.placeAt(WorldGenLevel, BlockState, BlockPos, int)` — the
/// two `LevelWriter.setBlock` writes (lower then upper half) with the
/// waterlogged copy. `pos` is the lower half; the upper half is `pos.above()`.
pub fn place_at(
    level: &mut dyn WorldGenLevel,
    state: BlockState,
    pos: &BlockPos,
    update_type: u32,
) {
    let lower = state
        .set_value(
            BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Lower,
        )
        .expect("a DoublePlantBlock state has the half property");
    level.set_block(pos, copy_waterlogged_from(level, pos, lower), update_type);
    let upper_pos = pos.above();
    let upper = state
        .set_value(
            BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Upper,
        )
        .expect("a DoublePlantBlock state has the half property");
    level.set_block(
        &upper_pos,
        copy_waterlogged_from(level, &upper_pos, upper),
        update_type,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `isWaterAt` -> `fluid.is(FluidTags.WATER)` is tag membership: both
    /// `minecraft:water` (id 2) and `minecraft:flowing_water` (id 1) are in the
    /// `minecraft:water` fluid tag, so a double-plant cell holding either is
    /// waterlogged. The empty fluid and lava are not members.
    #[test]
    fn water_tag_membership_matches_fluid_is_water() {
        assert!(is_water(FluidId::from_name("minecraft:water").unwrap()));
        assert!(is_water(
            FluidId::from_name("minecraft:flowing_water").unwrap()
        ));
        assert!(!is_water(FluidId::EMPTY));
        assert!(!is_water(FluidId::from_name("minecraft:lava").unwrap()));
    }
}

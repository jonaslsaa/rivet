//! Port of the `net.minecraft.world.level.levelgen.feature` coral family —
//! `CoralFeature` (abstract), `CoralClawFeature`, `CoralMushroomFeature`,
//! `CoralTreeFeature` (class, 26.2) — owned by the
//! `mc.world.level.levelgen.feature.coral` manifest unit.
//!
//! Java: the abstract `Feature<NoneFeatureConfiguration>` base draws one block
//! from `#minecraft:coral_blocks` and, when the tag is non-empty, dispatches to
//! the subclass `placeFeature`. The three concrete features share
//! `placeCoralBlock`: a cell is written (with `Block.UPDATE_ALL`, 3) when it is
//! water or a `#minecraft:corals` block with water above; then a
//! `nextFloat < 0.25` draw may top it with a random `#minecraft:corals` coral,
//! else a `nextFloat < 0.05` draw may top it with a sea pickle (its `pickles`
//! count from a `nextInt(4)` draw), and each of the four horizontal faces gets
//! a `nextFloat < 0.2` draw that may attach a random `#minecraft:wall_corals`
//! fan (its `facing` set from the face when it carries the property). All the
//! topping writes use `Block.UPDATE_CLIENTS` (2).
//!
//! The RNG order is load-bearing: the `#coral_blocks` draw, then the subclass's
//! shape draws, then per-cell the `placeCoralBlock` draws exactly as Java's
//! `&&`/`||` short-circuiting evaluates them (the gate itself consumes no RNG).
//! The tag-random-element seam — `BuiltInRegistries.BLOCK.getRandomElementOf
//! (tag, random)` — is composed from `rivet_util::get_random_safe` over
//! `BLOCK_TAG_BY_NAME` (the tag's element names in tag-file order, resolved via
//! `BlockId::from_name`): one `nextInt(size)` draw, `None` (with no draw) only
//! on an empty tag.
//!
//! Java's abstract `placeFeature` is modelled as the `place_feature` argument
//! to [`place_coral`], so the three concrete features share this exact walk
//! without a trait object (the same shape the mushroom family's `MakeCap`
//! uses).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::{BlockPos, Plane};
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::generated::tags::BLOCK_TAG_BY_NAME;
use rivet_util::RandomSource;
use rivet_util::get_random_safe;

/// `Feature.setBlock` — `level.setBlock(pos, state, Block.UPDATE_ALL)`.
const UPDATE_ALL: u32 = 3;

/// `Block.UPDATE_CLIENTS` — the write-flag constant the topping writes use.
const UPDATE_CLIENTS: u32 = 2;

/// `BuiltInRegistries.BLOCK.getRandomElementOf(tag, random)` — the block tag's
/// element names resolved to their default `BlockState` (`None` — without an
/// RNG draw — when the tag is empty).
pub(crate) fn tag_random_block_state<R: RandomSource>(
    tag: &str,
    random: &mut R,
) -> Option<BlockState> {
    let names = BLOCK_TAG_BY_NAME.get(tag)?;
    let name = get_random_safe(names, random)?;
    Some(BlockState::of(BlockId::from_name(name).unwrap()))
}

/// `CoralFeature.placeCoralBlock(LevelAccessor, RandomSource, BlockPos,
/// BlockState)` — write `state` at `pos` when it is water or a
/// `#minecraft:corals` block with water above, then run the topping draws.
pub(crate) fn place_coral_block<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    pos: &BlockPos,
    state: BlockState,
) -> bool {
    let above = pos.above();
    let target = level.get_block_state(pos);
    if (target.block() == Blocks::WATER.id() || target.is_in_tag("minecraft:corals"))
        && level.get_block_state(&above).block() == Blocks::WATER.id()
    {
        level.set_block(pos, state, UPDATE_ALL);
        if random.next_float() < 0.25 {
            if let Some(coral) = tag_random_block_state("minecraft:corals", random) {
                level.set_block(&above, coral, UPDATE_CLIENTS);
            }
        } else if random.next_float() < 0.05 {
            let state = Blocks::SEA_PICKLE
                .default_block_state()
                .set_value(
                    BlockStateProperties::PICKLES,
                    PropertyValue::Int(random.next_int_bound(4).wrapping_add(1)),
                )
                .expect("sea pickle carries the pickles property");
            level.set_block(&above, state, UPDATE_CLIENTS);
        }

        for direction in Plane::Horizontal.faces() {
            // Java's nested `if (nextFloat() < 0.2F)` / water check / wall-coral
            // draw collapse to a let-chain — the short-circuit keeps the
            // `nextFloat` roll, then the water read, then the tag draw, exactly
            // Java's evaluation order.
            let relative = pos.relative(direction);
            if random.next_float() < 0.2
                && level.get_block_state(&relative).block() == Blocks::WATER.id()
                && let Some(coral) = tag_random_block_state("minecraft:wall_corals", random)
            {
                let state = if coral.has_property(BlockStateProperties::FACING) {
                    coral
                        .set_value(BlockStateProperties::FACING, *direction)
                        .expect("wall coral carries the facing property")
                } else {
                    coral
                };
                level.set_block(&relative, state, UPDATE_CLIENTS);
            }
        }

        true
    } else {
        false
    }
}

/// The `CoralFeature.place` walk — draw one `#minecraft:coral_blocks` block and
/// dispatch to the subclass `placeFeature` (the `place_feature` argument).
pub(crate) fn place_coral<
    R: RandomSource,
    F: Fn(&mut dyn WorldGenLevel, &mut R, &BlockPos, BlockState) -> bool,
>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    place_feature: F,
) -> bool {
    let Some(coral) = tag_random_block_state("minecraft:coral_blocks", random) else {
        return false;
    };
    place_feature(level, random, origin, coral)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{RecordingRandom, RngCall, TestLevel, access};
    use rivet_registry::generated::blocks::BlockId;

    fn water() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:water").unwrap())
    }

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    fn coral() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:tube_coral_block").unwrap())
    }

    /// `placeCoralBlock` writes the coral when the target is water with water
    /// above (`Block.UPDATE_ALL`), then starts the topping draws with the
    /// `0.25` roll.
    #[test]
    fn place_coral_block_writes_when_target_is_water() {
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 0, 0), water());
        level.states.insert(BlockPos::new(0, 1, 0), water());
        let mut random = RecordingRandom::new(1);
        let pos = BlockPos::new(0, 0, 0);
        assert!(place_coral_block(&mut level, &mut random, &pos, coral()));
        assert_eq!(level.writes[0], (pos, coral()));
        assert_eq!(random.calls[0], RngCall::Float);
    }

    /// A `#minecraft:corals` target also accepts the write (it is replaced by
    /// the coral block).
    #[test]
    fn place_coral_block_accepts_a_corals_target() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:tube_coral").unwrap()),
        );
        level.states.insert(BlockPos::new(0, 1, 0), water());
        let mut random = RecordingRandom::new(1);
        let pos = BlockPos::new(0, 0, 0);
        assert!(place_coral_block(&mut level, &mut random, &pos, coral()));
        assert_eq!(level.writes[0], (pos, coral()));
    }

    /// A solid target fails the gate: `false`, no write, and no RNG draws (the
    /// gate short-circuits before any draw).
    #[test]
    fn place_coral_block_rejects_a_solid_target_without_draws() {
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 0, 0), stone());
        level.states.insert(BlockPos::new(0, 1, 0), water());
        let mut random = RecordingRandom::new(1);
        assert!(!place_coral_block(
            &mut level,
            &mut random,
            &BlockPos::new(0, 0, 0),
            coral()
        ));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// A water target whose `above` is not water also fails the gate with no
    /// draws.
    #[test]
    fn place_coral_block_rejects_when_above_is_not_water() {
        let mut level = TestLevel::over(access());
        level.states.insert(BlockPos::new(0, 0, 0), water());
        level.states.insert(BlockPos::new(0, 1, 0), stone());
        let mut random = RecordingRandom::new(1);
        assert!(!place_coral_block(
            &mut level,
            &mut random,
            &BlockPos::new(0, 0, 0),
            coral()
        ));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// The tag-random-element seam draws one `nextInt(size)` with the tag's
    /// element count as the bound, resolving to a `#minecraft:coral_blocks`
    /// member.
    #[test]
    fn tag_random_block_state_draws_a_coral_block() {
        let mut random = RecordingRandom::new(1);
        let state = tag_random_block_state("minecraft:coral_blocks", &mut random).unwrap();
        assert_eq!(random.calls, vec![RngCall::IntBound(5)]);
        assert!(matches!(
            state.block().name(),
            "minecraft:tube_coral_block"
                | "minecraft:brain_coral_block"
                | "minecraft:bubble_coral_block"
                | "minecraft:fire_coral_block"
                | "minecraft:horn_coral_block"
        ));
    }

    /// The `#minecraft:corals` and `#minecraft:wall_corals` tags draw with
    /// their own sizes (10 and 5).
    #[test]
    fn tag_draws_use_the_tag_size_as_bound() {
        let mut random = RecordingRandom::new(1);
        assert_eq!(random.calls.len(), 0);
        let _ = tag_random_block_state("minecraft:corals", &mut random).unwrap();
        assert_eq!(random.calls, vec![RngCall::IntBound(10)]);
        let _ = tag_random_block_state("minecraft:wall_corals", &mut random).unwrap();
        assert_eq!(
            random.calls,
            vec![RngCall::IntBound(10), RngCall::IntBound(5)]
        );
    }
}

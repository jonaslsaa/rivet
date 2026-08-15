//! Port of `net.minecraft.world.level.levelgen.feature.MultifaceGrowthFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.multifacegrowth`
//! manifest unit (the first step-9 seed-42 feature: `glow_lichen`, FeatureId 20).
//!
//! Java: `Feature<MultifaceGrowthConfiguration>` that grows a multiface block
//! (`glow_lichen` for seed 42). `place` gates the origin cell (air or water),
//! gates the growth block (`config.placeBlock instanceof
//! MultifaceSpreadeableBlock`), shuffles the config's valid directions, tries
//! `placeGrowthIfPossible` on the origin, then walks each search direction —
//! drawing the per-direction placement-directions list once, then retrying
//! `placeGrowthIfPossible` at the fixed one-step cell `searchRange` times
//! (`pos.setWithOffset(origin, searchDirection)` re-sets the same cell every
//! iteration — the search never advances). `placeGrowthIfPossible` grows at the
//! first placement direction whose neighbour is in `config.canBePlacedOn`
//! (returning `false` if `getStateForPlacement` rejects that neighbour),
//! writing with `Block.UPDATE_ALL`, marking the cell for post-processing, then
//! drawing `random.nextFloat() < config.chanceOfSpreading` and — when it fires —
//! the block's `MultifaceSpreader.spreadFromFaceTowardRandomDirection`
//! (`Direction.allShuffled` then the first spreadable face), whose write uses
//! `Block.UPDATE_CLIENTS` and marks the spread cell for post-processing.
//!
//! The RNG draw order (pinned for the deterministic tests): (1) the valid-
//! directions shuffle, (2) per search direction a placement-directions
//! shuffle drawn BEFORE the retry loop, (3) on a successful growth a
//! `nextFloat()` spread draw, (4) if it fires, the six-direction `allShuffled`
//! spreader shuffle.
//!
//! STUB(mc.world.level.block): the growth behavior lives in the block package
//! (`MultifaceSpreadeableBlock.getSpreader`, `MultifaceBlock.getStateForPlacement`/
//! `isValidStateForPlacement`/`hasFace`/`canAttachTo`, and the
//! `MultifaceSpreader` spread flow). The seed-42 block (`glow_lichen`) uses
//! `MultifaceSpreader.DefaultSpreaderConfig`; that config and the shared
//! `MultifaceBlock` state/attach logic are ported here faithfully (the same
//! cross-unit STUB pattern as `chorus_growth.rs`), routing `canAttachTo` — the
//! face-sturdiness of the support neighbour — through the
//! `WorldGenLevel::is_face_sturdy` seam (RivetTodo #232), exactly as
//! `vines_feature.rs` does. That seam is the blocker for production placement:
//! the only production `WorldGenLevel` impl (the `WorldGenRegion` facade in
//! `rivet-server`, `mc.server.level.pipeline.region`) does not override
//! `is_face_sturdy`, so the trait default at `world_gen_level.rs` panics — a
//! real FEATURES pass would panic on the first placement attempt. The runtime
//! body ported here is therefore exercised only against the test double
//! (`TestLevel`, `face_sturdy = true`); placing seed-42 `glow_lichen` in
//! production still requires the #232 block/shape world-access port.
//! `SculkVeinBlock.getSpreader()` returns a
//! `SculkVeinSpreaderConfig` whose replace/spread rules differ; that config is
//! not ported, so a sculk-vein growth block fails explicitly (RivetTodo #232)
//! rather than growing with the wrong spreader. Resolving the spreader up front
//! in `place` (rather than at each spread site, as Java's lazy
//! `getSpreader()` call does) is behaviour-identical for the stateless default
//! config and makes the sculk-vein deferral fail before any write.

use crate::block::Block;
use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::MultifaceGrowthConfiguration;
use crate::levelgen::feature::configurations::multiface_growth_configuration::is_multiface_spreadeable;
use crate::levelgen::feature::{FeatureBehavior, FeaturePlaceContext, is_block};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::Property;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::fluid_id::FluidId;
use rivet_util::RandomSource;
use rivet_util::util::shuffled_copy;

/// `Block.UPDATE_ALL` — `placeGrowthIfPossible` writes (setBlock + notify).
const UPDATE_ALL: u32 = 3;
/// `Block.UPDATE_CLIENTS` — the spreader's `SpreadConfig.placeBlock` writes.
const UPDATE_CLIENTS: u32 = 2;

/// `MultifaceBlock.PROPERTY_BY_DIRECTION` / `getFaceProperty(Direction)` — the
/// face boolean property for a direction (UP→UP, DOWN→DOWN, NORTH→NORTH,
/// EAST→EAST, SOUTH→SOUTH, WEST→WEST).
fn property_for_face(direction: &Direction) -> Property {
    match direction {
        Direction::Up => BlockStateProperties::UP,
        Direction::Down => BlockStateProperties::DOWN,
        Direction::North => BlockStateProperties::NORTH,
        Direction::South => BlockStateProperties::SOUTH,
        Direction::West => BlockStateProperties::WEST,
        Direction::East => BlockStateProperties::EAST,
    }
}

/// `MultifaceBlock.hasFace(BlockState, Direction)` —
/// `state.getValueOrElse(property, false)`, the per-face presence bit.
fn has_face(state: BlockState, face: &Direction) -> bool {
    matches!(
        state.get_value(property_for_face(face)),
        Some(PropertyValue::Bool(true))
    )
}

/// `MultifaceBlock.isValidStateForPlacement(BlockGetter, BlockState, BlockPos,
/// Direction)` — the face is supported (always true for the multiface-spreadeable
/// blocks, which do not override `isFaceSupported`) and either the cell is not
/// already this block or the face is not yet present; then the neighbour on the
/// face must be attachable (`MultifaceBlock.canAttachTo` = the support/
/// collision shape is full on the opposite face — the `is_face_sturdy` seam).
fn is_valid_state_for_placement(
    block: Block,
    level: &dyn WorldGenLevel,
    old_state: BlockState,
    placement_pos: &BlockPos,
    placement_direction: &Direction,
) -> bool {
    if is_block(old_state, block) && has_face(old_state, placement_direction) {
        return false;
    }
    let neighbour_pos = placement_pos.relative(placement_direction);
    let neighbour_state = level.get_block_state(&neighbour_pos);
    level.is_face_sturdy(
        &neighbour_pos,
        &neighbour_state,
        &placement_direction.get_opposite(),
    )
}

/// `MultifaceBlock.getStateForPlacement(BlockState, BlockGetter, BlockPos,
/// Direction)` — the state to write: when the cell already holds this block it
/// is preserved (accumulating faces and waterlogging); otherwise the default
/// state, waterlogged when the old cell holds a source of water; then the new
/// face property is set. `None` when `isValidStateForPlacement` fails.
fn get_state_for_placement(
    block: Block,
    old_state: BlockState,
    level: &dyn WorldGenLevel,
    placement_pos: &BlockPos,
    placement_direction: &Direction,
) -> Option<BlockState> {
    if !is_valid_state_for_placement(block, level, old_state, placement_pos, placement_direction) {
        return None;
    }
    let new_state = if is_block(old_state, block) {
        old_state
    } else if old_state.fluid_id() == FluidId::WATER.id() {
        // `oldState.getFluidState().isSourceOfType(Fluids.WATER)` — the source-
        // water fluid id (2; flowing water is id 1) is the faithful conjunct.
        block
            .default_block_state()
            .set_value(BlockStateProperties::WATERLOGGED, true)
            .expect("multiface-spreadeable blocks carry the waterlogged property")
    } else {
        block.default_block_state()
    };
    Some(
        new_state
            .set_value(property_for_face(placement_direction), true)
            .expect("multiface-spreadeable blocks carry the face property for every direction"),
    )
}

/// `MultifaceSpreader.SpreadPos` — the `(pos, face, source)` record the
/// spread flow writes through (CraftBukkit's spread-event triple).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpreadPos {
    pos: BlockPos,
    face: Direction,
    source: BlockPos,
}

/// `MultifaceSpreader.SpreadType` — the three spread-position derivations,
/// tried in `DEFAULT_SPREAD_ORDER` (SAME_POSITION, SAME_PLANE, WRAP_AROUND).
enum SpreadType {
    SamePosition,
    SamePlane,
    WrapAround,
}

impl SpreadType {
    /// `SpreadType.getSpreadPos(BlockPos, Direction, Direction)`.
    fn get_spread_pos(
        &self,
        pos: BlockPos,
        spread_direction: Direction,
        from_face: Direction,
    ) -> SpreadPos {
        match self {
            SpreadType::SamePosition => SpreadPos {
                pos,
                face: spread_direction,
                source: pos,
            },
            SpreadType::SamePlane => SpreadPos {
                pos: pos.relative(&spread_direction),
                face: from_face,
                source: pos,
            },
            SpreadType::WrapAround => SpreadPos {
                pos: pos.relative(&spread_direction).relative(&from_face),
                face: spread_direction.get_opposite(),
                source: pos,
            },
        }
    }
}

/// `MultifaceSpreader.SpreadConfig.placeBlock` (the default) — write the
/// spread state with `Block.UPDATE_CLIENTS`; when `postProcess`, first mark the
/// cell for post-processing. Returns the `handleBlockSpreadEvent` verdict (the
/// `setBlock` result).
fn place_spread_block(
    block: Block,
    level: &mut dyn WorldGenLevel,
    spread_pos: &SpreadPos,
    post_process: bool,
) -> bool {
    let old_state = level.get_block_state(&spread_pos.pos);
    let Some(spread_state) =
        get_state_for_placement(block, old_state, level, &spread_pos.pos, &spread_pos.face)
    else {
        return false;
    };
    if post_process {
        level.mark_pos_for_post_processing(&spread_pos.pos);
    }
    level.set_block(&spread_pos.pos, spread_state, UPDATE_CLIENTS)
}

/// `MultifaceSpreader.DefaultSpreaderConfig.stateCanBeReplaced` — the spread
/// cell is air, already this block, or a source-water cell.
fn state_can_be_replaced(
    block: Block,
    _spread_pos: &SpreadPos,
    existing_state: &BlockState,
) -> bool {
    existing_state.is_air()
        || is_block(*existing_state, block)
        || (is_block(*existing_state, Blocks::WATER)
            && existing_state.fluid_id() == FluidId::WATER.id())
}

/// `MultifaceSpreader.DefaultSpreaderConfig.canSpreadInto(BlockGetter, BlockPos,
/// SpreadPos)` — `stateCanBeReplaced` on the target cell and the block's
/// `isValidStateForPlacement` for the spread face.
fn can_spread_into(
    block: Block,
    level: &dyn WorldGenLevel,
    _source_pos: &BlockPos,
    spread_pos: &SpreadPos,
) -> bool {
    let existing_state = level.get_block_state(&spread_pos.pos);
    state_can_be_replaced(block, spread_pos, &existing_state)
        && is_valid_state_for_placement(
            block,
            level,
            existing_state,
            &spread_pos.pos,
            &spread_pos.face,
        )
}

/// `MultifaceSpreader.getSpreadFromFaceTowardDirection(BlockState, BlockGetter,
/// BlockPos, Direction, Direction, SpreadPredicate)` — reject a spread along
/// the starting face's own axis; then, when the source has the starting face
/// and not the spread direction, try the spread types in order and return the
/// first whose target `canSpreadInto` accepts.
fn get_spread_from_face_toward_direction(
    block: Block,
    state: BlockState,
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    starting_face: &Direction,
    spread_direction: &Direction,
) -> Option<SpreadPos> {
    if spread_direction.get_axis() == starting_face.get_axis() {
        return None;
    }
    // `config.isOtherBlockValidAsSource(state)` is false for the default config.
    if has_face(state, starting_face) && !has_face(state, spread_direction) {
        for spread_type in [
            SpreadType::SamePosition,
            SpreadType::SamePlane,
            SpreadType::WrapAround,
        ] {
            let spread_pos = spread_type.get_spread_pos(*pos, *spread_direction, *starting_face);
            if can_spread_into(block, level, pos, &spread_pos) {
                return Some(spread_pos);
            }
        }
        None
    } else {
        None
    }
}

/// `MultifaceSpreader.spreadFromFaceTowardRandomDirection(BlockState,
/// LevelAccessor, BlockPos, Direction, RandomSource, boolean)` — shuffle the
/// six directions (`Direction.allShuffled`, deferred in the `Direction` port and
/// reproduced here via `shuffled_copy` of `Direction.VALUES`), then grow at the
/// first direction whose `getSpreadFromFaceTowardDirection` target places.
/// Returns `true` iff a spread cell was written.
fn spread_from_face_toward_random_direction<R: RandomSource>(
    block: Block,
    state: BlockState,
    level: &mut dyn WorldGenLevel,
    pos: &BlockPos,
    starting_face: &Direction,
    random: &mut R,
    post_process: bool,
) -> bool {
    for spread_direction in shuffled_copy(&Direction::VALUES, random) {
        if let Some(spread_pos) = get_spread_from_face_toward_direction(
            block,
            state,
            level,
            pos,
            starting_face,
            &spread_direction,
        ) && place_spread_block(block, level, &spread_pos, post_process)
        {
            return true;
        }
    }
    false
}

/// `MultifaceSpreadeableBlock.getSpreader()` — the spreader a multiface growth
/// block grows with. `GlowLichenBlock.getSpreader()` returns `new
/// MultifaceSpreader(this)` (the `DefaultSpreaderConfig`, whose
/// replace/spread rules are ported above). `SculkVeinBlock.getSpreader()`
/// returns a `MultifaceSpreader(new SculkVeinSpreaderConfig(...))` whose rules
/// differ — that config defers with the block port (RivetTodo #232), so the
/// sculk-vein growth block fails explicitly rather than growing with the wrong
/// spreader. Java resolves this lazily at each spread site; resolving once in
/// `place` is behaviour-identical for the stateless default config and makes
/// the sculk-vein deferral fail before any write.
fn get_spreader(block: Block) {
    match block.name() {
        "minecraft:glow_lichen" => (),
        other => panic!(
            "multiface_growth growth block '{other}' spreader is not ported (RivetTodo #232: SculkVeinSpreaderConfig defers with the block port)"
        ),
    }
}

/// `MultifaceGrowthFeature.isAirOrWater(BlockState)` — the origin/search-cell
/// gate: air or a `minecraft:water` block.
fn is_air_or_water(state: BlockState) -> bool {
    state.is_air() || is_block(state, Blocks::WATER)
}

/// `MultifaceGrowthFeature.placeGrowthIfPossible(...)` — grow the block at
/// `pos` on the first placement direction whose neighbour is in
/// `config.canBePlacedOn`; `false` when `getStateForPlacement` rejects that
/// neighbour (never advancing past a `canBePlacedOn` neighbour, exactly as
/// Java's `return false`), `true` after the write.
fn place_growth_if_possible<R: RandomSource>(
    block: Block,
    level: &mut dyn WorldGenLevel,
    pos: &BlockPos,
    old_state: BlockState,
    config: &MultifaceGrowthConfiguration,
    random: &mut R,
    placement_directions: &[Direction],
) -> bool {
    for placement_direction in placement_directions {
        let neighbour_pos = pos.relative(placement_direction);
        let neighbour_state = level.get_block_state(&neighbour_pos);
        if config
            .can_be_placed_on()
            .contains_id(neighbour_state.block().id() as u32)
        {
            let Some(new_state) =
                get_state_for_placement(block, old_state, level, pos, placement_direction)
            else {
                return false;
            };
            level.set_block(pos, new_state, UPDATE_ALL);
            level.mark_pos_for_post_processing(pos);
            if random.next_float() < config.chance_of_spreading() {
                spread_from_face_toward_random_direction(
                    block,
                    new_state,
                    level,
                    pos,
                    placement_direction,
                    random,
                    true,
                );
            }
            return true;
        }
    }
    false
}

/// `net.minecraft.world.level.levelgen.feature.MultifaceGrowthFeature`.
#[derive(Debug)]
pub struct MultifaceGrowthFeature;

/// `Feature.MULTIFACE_GROWTH` — the registered `minecraft:multiface_growth`
/// singleton (the feature registry's insertion index 20).
pub const MULTIFACE_GROWTH: MultifaceGrowthFeature = MultifaceGrowthFeature;

impl FeatureBehavior<MultifaceGrowthConfiguration> for MultifaceGrowthFeature {
    /// `MultifaceGrowthFeature.place(FeaturePlaceContext<MultifaceGrowthConfiguration>)`.
    ///
    /// ```java
    /// if (!isAirOrWater(level.getBlockState(origin))) return false;
    /// else if (!(config.placeBlock instanceof MultifaceSpreadeableBlock placerBlock)) return false;
    /// else {
    ///     List<Direction> var14 = config.getShuffledDirections(random);
    ///     if (placeGrowthIfPossible(placerBlock, level, origin, level.getBlockState(origin), config, random, var14)) return true;
    ///     BlockPos.MutableBlockPos pos = origin.mutable();
    ///     for (Direction searchDirection : var14) {
    ///         pos.set(origin);
    ///         List<Direction> placementDirections = config.getShuffledDirectionsExcept(random, searchDirection.getOpposite());
    ///         for (int i = 0; i < config.searchRange; i++) {
    ///             pos.setWithOffset(origin, searchDirection);
    ///             BlockState state = level.getBlockState(pos);
    ///             if (!isAirOrWater(state) && !state.is(config.placeBlock)) break;
    ///             if (placeGrowthIfPossible(placerBlock, level, pos, state, config, random, placementDirections)) return true;
    ///         }
    ///     }
    ///     return false;
    /// }
    /// ```
    ///
    /// The search inner loop re-sets `pos` to `origin.relative(searchDirection)`
    /// every iteration, so it retries `placeGrowthIfPossible` `searchRange`
    /// times at the fixed one-step cell (the search never advances); the port
    /// reproduces that non-accumulating behaviour exactly.
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, MultifaceGrowthConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            origin,
            random,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let config: &MultifaceGrowthConfiguration = config;
        let origin = **origin;

        if !is_air_or_water(level.get_block_state(&origin)) {
            return false;
        }
        let place_block = config.place_block();
        if !is_multiface_spreadeable(place_block) {
            return false;
        }
        // `placerBlock.getSpreader()` — resolved up front (see `get_spreader`).
        get_spreader(place_block);

        let var14 = config.get_shuffled_directions(random);
        // Java evaluates `level.getBlockState(origin)` as a call argument before
        // entering `placeGrowthIfPossible`, so the read must be hoisted out of
        // the mutable-borrow site.
        let origin_state = level.get_block_state(&origin);
        if place_growth_if_possible(
            place_block,
            level,
            &origin,
            origin_state,
            config,
            random,
            &var14,
        ) {
            return true;
        }
        for search_direction in &var14 {
            let placement_directions =
                config.get_shuffled_directions_except(random, search_direction.get_opposite());
            for _ in 0..config.search_range() {
                let pos = origin.relative(search_direction);
                let state = level.get_block_state(&pos);
                if !is_air_or_water(state) && !is_block(state, place_block) {
                    break;
                }
                if place_growth_if_possible(
                    place_block,
                    level,
                    &pos,
                    state,
                    config,
                    random,
                    &placement_directions,
                ) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::BlockPos;
    use rivet_registry::holder::Holder;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::holder_set::HolderSet;
    use rivet_registry::registries::BlockType;

    /// The seed-42 glow-lichen config: ceiling + wall (no floor), so the valid
    /// directions are `[UP, NORTH, EAST, SOUTH, WEST]`; `searchRange` 20 and
    /// `chanceOfSpreading` as chosen per test.
    fn config(
        chance_of_spreading: f32,
        can_be_placed_on: HolderSet<BlockType>,
    ) -> MultifaceGrowthConfiguration {
        MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:glow_lichen").unwrap(),
            20,
            false,
            true,
            true,
            chance_of_spreading,
            can_be_placed_on,
        )
    }

    /// A `canBePlacedOn` set containing stone (id 1) as registry-reference
    /// members — `state.is(set)` maps to `set.contains_id(state.block().id())`,
    /// which matches `Reference` members by element id (the matching-registry
    /// contract in `holder_set.rs`).
    fn on_stone() -> HolderSet<BlockType> {
        HolderSet::direct(vec![Holder::reference(
            RegistryId(0),
            Blocks::STONE.id().0 as u32,
        )])
    }

    /// An empty `canBePlacedOn` set — the fixture for the search-break-on-solid
    /// test, where no cell may ever attach.
    fn on_nothing() -> HolderSet<BlockType> {
        HolderSet::empty()
    }

    fn place_with<R: rivet_util::RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
        config: &MultifaceGrowthConfiguration,
    ) -> bool {
        let generator = TestGenerator;
        MULTIFACE_GROWTH.place(&mut FeaturePlaceContext::new(
            None, level, &generator, random, &origin, config,
        ))
    }

    /// A non-air/water origin fails the first gate with no writes and no draws.
    #[test]
    fn non_air_or_water_origin_returns_false() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, 0), BlockState::of(Blocks::STONE.id()));
        let mut random = RecordingRandom::new(42);
        let result = place_with(
            &mut level,
            BlockPos::new(0, 0, 0),
            &mut random,
            &config(0.5, on_stone()),
        );
        assert!(!result);
        assert!(level.writes.is_empty());
        assert!(level.post_processing.is_empty());
        assert!(random.calls.is_empty());
    }

    /// A growth block that is not multiface-spreadeable fails the instanceof
    /// gate with no writes and no draws (the shuffle happens after the gate).
    #[test]
    fn non_multiface_growth_block_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(42);
        let config = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:stone").unwrap(),
            20,
            false,
            true,
            true,
            0.5,
            on_stone(),
        );
        let result = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
        assert!(!result);
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// A sculk-vein growth block passes the instanceof gate but its
    /// `SculkVeinSpreaderConfig` spreader is not ported — placement fails
    /// explicitly (RivetTodo #232) rather than growing with the wrong spreader.
    #[test]
    #[should_panic(expected = "RivetTodo #232")]
    fn sculk_vein_growth_block_defers_explicitly() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(42);
        let config = MultifaceGrowthConfiguration::new(
            Block::from_name("minecraft:sculk_vein").unwrap(),
            20,
            false,
            true,
            true,
            0.5,
            on_stone(),
        );
        let _ = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
    }

    /// The origin gate passes and the first placement direction (NORTH at seed
    /// 42) finds a stone neighbour: the origin is written with the NORTH face,
    /// `Block.UPDATE_ALL` (3), marked for post-processing, and — with a
    /// `chanceOfSpreading` of 0 — only the one `nextFloat()` spread draw
    /// follows the valid-directions shuffle (the spread never fires).
    #[test]
    fn places_on_origin_with_no_spread_draw_order() {
        let mut level = TestLevel::over(access());
        // origin.relative(NORTH) = (0, 0, -1) hosts stone.
        level
            .states
            .insert(BlockPos::new(0, 0, -1), BlockState::of(Blocks::STONE.id()));
        let mut random = RecordingRandom::new(42);
        let config = config(0.0, on_stone());
        let result = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
        assert!(result);
        // One write at the origin: glow_lichen with the NORTH face, UPDATE_ALL.
        assert_eq!(level.writes.len(), 1);
        let (pos, state) = &level.writes[0];
        assert_eq!(*pos, BlockPos::new(0, 0, 0));
        assert_eq!(
            state.block(),
            Block::from_name("minecraft:glow_lichen").unwrap().id()
        );
        assert_eq!(
            state.get_value(BlockStateProperties::NORTH),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(level.writes_flags, vec![UPDATE_ALL]);
        assert_eq!(level.post_processing, vec![BlockPos::new(0, 0, 0)]);
        // Seed 42 valid-dirs = [UP, NORTH, EAST, SOUTH, WEST]; a 5-shuffle is 4
        // draws (5,4,3,2), then the spread `nextFloat()` (0.0 chance → no fire).
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(5),
                RngCall::IntBound(4),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
                RngCall::Float,
            ]
        );
    }

    /// With a `chanceOfSpreading` of 1, the spread always fires: after the
    /// `nextFloat()` the six-direction `allShuffled` (seed 42 order
    /// [SOUTH, WEST, EAST, NORTH, DOWN, UP]) runs. The starting face is NORTH
    /// (Z axis), so SOUTH (same axis) is axis-rejected; the first candidate
    /// that passes the axis/gate/canSpreadInto checks is WEST at the
    /// SAME_POSITION type — the origin gains the WEST face in a
    /// `Block.UPDATE_CLIENTS` (2) write and is marked for post-processing again.
    #[test]
    fn spread_always_fires_and_accumulates_faces() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, -1), BlockState::of(Blocks::STONE.id()));
        let mut random = RecordingRandom::new(42);
        let config = config(1.0, on_stone());
        let result = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
        assert!(result);
        // Write 1: origin glow_lichen + NORTH, UPDATE_ALL. Write 2: origin
        // glow_lichen + NORTH + WEST (the SAME_POSITION spread), UPDATE_CLIENTS.
        assert_eq!(level.writes.len(), 2);
        let (pos, state) = &level.writes[1];
        assert_eq!(*pos, BlockPos::new(0, 0, 0));
        assert_eq!(
            state.get_value(BlockStateProperties::NORTH),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::WEST),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(level.writes_flags, vec![UPDATE_ALL, UPDATE_CLIENTS]);
        assert_eq!(
            level.post_processing,
            vec![BlockPos::new(0, 0, 0), BlockPos::new(0, 0, 0)]
        );
        // 5-shuffle (5,4,3,2) + Float + 6-shuffle (6,5,4,3,2) — the allShuffled
        // draws happen even though only the first candidate places.
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(5),
                RngCall::IntBound(4),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
                RngCall::Float,
                RngCall::IntBound(6),
                RngCall::IntBound(5),
                RngCall::IntBound(4),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
            ]
        );
    }

    /// A water origin is allowed and the grown cell is waterlogged: the origin
    /// water state (fluid id 2) yields a `WATERLOGGED` glow-lichen, then the
    /// NORTH face is added.
    #[test]
    fn water_origin_waterlogs_the_grown_cell() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, 0), BlockState::of(Blocks::WATER.id()));
        level
            .states
            .insert(BlockPos::new(0, 0, -1), BlockState::of(Blocks::STONE.id()));
        let mut random = RecordingRandom::new(42);
        let config = config(0.0, on_stone());
        let result = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
        assert!(result);
        assert_eq!(level.writes.len(), 1);
        let (pos, state) = &level.writes[0];
        assert_eq!(*pos, BlockPos::new(0, 0, 0));
        assert_eq!(
            state.get_value(BlockStateProperties::WATERLOGGED),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            state.get_value(BlockStateProperties::NORTH),
            Some(PropertyValue::Bool(true))
        );
    }

    /// The origin gate passes but no placement direction has a `canBePlacedOn`
    /// neighbour (the set is empty), and every search cell is a solid block:
    /// each search direction breaks at its first retry (a non-air/water,
    /// non-placeBlock cell), so the feature returns `false` with no writes. Draw
    /// order: the valid-directions 5-shuffle (4 draws), then one except-shuffle
    /// per search direction (drawn before the loop). The first four search
    /// directions exclude a valid face, so their shuffles are 4-element (3
    /// draws); the last search direction is UP, whose excluded opposite DOWN is
    /// not in the valid set, so its except-shuffle is a no-op filter and stays a
    /// 5-element shuffle (4 draws) — 4 + 4*3 + 4 draws, all bounded ints.
    #[test]
    fn search_breaks_on_solid_cells_and_returns_false() {
        let mut level = TestLevel::over(access());
        // A ring of stone one step out in every direction; the origin stays air.
        for dir in Direction::VALUES {
            level.states.insert(
                BlockPos::new(0, 0, 0).relative(&dir),
                BlockState::of(Blocks::STONE.id()),
            );
        }
        let mut random = RecordingRandom::new(42);
        let config = config(0.5, on_nothing());
        let result = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
        assert!(!result);
        assert!(level.writes.is_empty());
        assert!(level.post_processing.is_empty());
        let mut expected = vec![
            RngCall::IntBound(5),
            RngCall::IntBound(4),
            RngCall::IntBound(3),
            RngCall::IntBound(2),
        ];
        // Search NORTH (exclude SOUTH), EAST (exclude WEST), SOUTH (exclude
        // NORTH), WEST (exclude EAST): 4-element except-shuffles, 3 draws each.
        for _ in 0..4 {
            expected.extend([
                RngCall::IntBound(4),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
            ]);
        }
        // Search UP (exclude DOWN): DOWN is not in the valid set, so the filter
        // is a no-op and the shuffle stays 5-element, 4 draws.
        expected.extend([
            RngCall::IntBound(5),
            RngCall::IntBound(4),
            RngCall::IntBound(3),
            RngCall::IntBound(2),
        ]);
        assert_eq!(random.calls, expected);
    }

    /// The origin placement fails (none of its five neighbours is in
    /// `canBePlacedOn`), and the search reaches an air cell whose placement
    /// succeeds with `chanceOfSpreading` 0. Seed 42's first search direction is
    /// NORTH; the origin's NORTH neighbour (0,0,-1) is air, so the search cell
    /// at `origin.relative(NORTH)` places, attaching on the UP face to the stone
    /// its UP neighbour (0,1,-1) provides. This pins the per-direction
    /// placement-directions shuffle (drawn before the retry loop) and the
    /// search-time draw order: 5-shuffle, then a 4-shuffle (excluding the SOUTH
    /// face), then the spread `nextFloat()`.
    #[test]
    fn search_places_on_the_one_step_cell() {
        let mut level = TestLevel::over(access());
        // The search cell (0,0,-1) attaches on its UP face: (0,1,-1) hosts
        // stone. The origin's own neighbours are all air, so only the search
        // cell can place.
        level
            .states
            .insert(BlockPos::new(0, 1, -1), BlockState::of(Blocks::STONE.id()));
        let mut random = RecordingRandom::new(42);
        let config = config(0.0, on_stone());
        let result = place_with(&mut level, BlockPos::new(0, 0, 0), &mut random, &config);
        assert!(result);
        // Seed 42 valid-dirs 5-shuffle = [NORTH, EAST, SOUTH, WEST, UP]; the
        // origin placement finds no stone neighbour (the origin's neighbours are
        // air, none in `canBePlacedOn`), so the search starts at NORTH. Its
        // placement list is the valid dirs minus SOUTH (4-shuffle, 3 draws),
        // then the cell (0,0,-1) grows on its first placement direction.
        assert_eq!(level.writes.len(), 1);
        let (pos, state) = &level.writes[0];
        assert_eq!(*pos, BlockPos::new(0, 0, -1));
        assert_eq!(
            state.block(),
            Block::from_name("minecraft:glow_lichen").unwrap().id()
        );
        assert_eq!(
            state.get_value(BlockStateProperties::UP),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(level.post_processing, vec![BlockPos::new(0, 0, -1)]);
        assert_eq!(
            random.calls,
            vec![
                RngCall::IntBound(5),
                RngCall::IntBound(4),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
                // getShuffledDirectionsExcept(NORTH.getOpposite() = SOUTH).
                RngCall::IntBound(4),
                RngCall::IntBound(3),
                RngCall::IntBound(2),
                // The growth's spread draw (chance 0 → no fire).
                RngCall::Float,
            ]
        );
    }
}

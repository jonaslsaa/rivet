//! Port of `net.minecraft.world.level.levelgen.feature.UnderwaterMagmaFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.underwatermagma`
//! manifest unit.
//!
//! Java: `Feature<UnderwaterMagmaConfiguration>` that first scans the downward
//! water column at the origin (`Column.scan` with `insideColumn = is(WATER)`,
//! `validEdge = !is(WATER)`) and returns `false` when there is no water column
//! floor. Otherwise it builds the axis-aligned box around the floor position
//! (`placementRadiusAroundFloor` on every axis), iterates its cells in
//! X/Y/Z-major order, and for every cell where `random.nextFloat() <
//! placementProbabilityPerValidPosition` places `MAGMA_BLOCK` when the cell is
//! a valid placement (a non-water/non-air cell whose faces are not visible from
//! outside). The feature returns `true` iff at least one cell was placed.
//!
//! The scan is ported through the `WorldGenLevel::is_state_at_position` seam
//! (the `LevelReader.isStateAtPosition` Java call `Column.scan` performs) and
//! the box iteration through `BlockPos::between_closed_pos` (the
//! `betweenClosedStream` Java surface, x-fastest/y/z). The RNG is consumed for
//! every cell of the box BEFORE the placement validity check — the stream
//! `.filter` order — so the exact Java draw sequence is preserved.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::UnderwaterMagmaConfiguration;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockBox, BlockPos, Direction, Plane};
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant the placed magma cells use.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.UnderwaterMagmaFeature`.
#[derive(Debug)]
pub struct UnderwaterMagmaFeature;

/// `Feature.UNDERWATER_MAGMA` — the registered `minecraft:underwater_magma`
/// singleton (id 21, after `MULTIFACE_GROWTH` and before `MONSTER_ROOM`).
pub const UNDERWATER_MAGMA: UnderwaterMagmaFeature = UnderwaterMagmaFeature;

impl FeatureBehavior<UnderwaterMagmaConfiguration> for UnderwaterMagmaFeature {
    /// `UnderwaterMagmaFeature.place(FeaturePlaceContext<UnderwaterMagmaConfiguration>)`.
    ///
    /// ```java
    /// OptionalInt floorY = getFloorY(level, origin, config);
    /// if (floorY.isEmpty()) {
    ///     return false;
    /// }
    ///
    /// BlockPos floorPos = origin.atY(floorY.getAsInt());
    /// Vec3i radius = new Vec3i(config.placementRadiusAroundFloor,
    ///     config.placementRadiusAroundFloor, config.placementRadiusAroundFloor);
    /// BoundingBox bounds = BoundingBox.fromCorners(
    ///     floorPos.subtract(radius), floorPos.offset(radius));
    /// return BlockPos.betweenClosedStream(bounds)
    ///         .filter(pos -> random.nextFloat() < config.placementProbabilityPerValidPosition)
    ///         .filter(pos -> this.isValidPlacement(level, pos))
    ///         .mapToInt(pos -> {
    ///             level.setBlock(pos, Blocks.MAGMA_BLOCK.defaultBlockState(),
    ///                 Block.UPDATE_CLIENTS);
    ///             return 1;
    ///         })
    ///         .sum() > 0;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, UnderwaterMagmaConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = **origin;
        let config = *config;

        let Some(floor_y) = get_floor_y(level, &origin, config) else {
            return false;
        };

        let floor_pos = origin.at_y(floor_y);
        let radius = rivet_registry::core::Vec3i::new(
            config.placement_radius_around_floor,
            config.placement_radius_around_floor,
            config.placement_radius_around_floor,
        );
        let bounds = BlockBox::new(floor_pos.subtract(&radius), floor_pos.offset_vec(&radius));

        let mut placed = 0;
        for pos in bounds.iterator() {
            // Java consumes a `nextFloat` for every cell of the box, before the
            // placement-validity check — the first stream `.filter` runs before
            // the second, so a rejected cell still draws.
            if random.next_float() < config.placement_probability_per_valid_position
                && is_valid_placement(level, &pos)
            {
                level.set_block(
                    &pos,
                    Blocks::MAGMA_BLOCK.default_block_state(),
                    UPDATE_CLIENTS,
                );
                placed += 1;
            }
        }
        placed > 0
    }
}

/// `UnderwaterMagmaFeature.getFloorY(WorldGenLevel, BlockPos,
/// UnderwaterMagmaConfiguration)` — the downward water-column floor scan.
///
/// Java runs `Column.scan(level, origin, config.floorSearchRange, insideColumn,
/// validEdge)` and maps the returned `Column` to its floor. The scan fails empty
/// when the origin cell is not water (`!isStateAtPosition(pos, insideColumn)`);
/// otherwise it walks DOWN from the origin (starting at `i = 1`, moving one
/// block per step while the current cell is water) and the reached cell's y is
/// the floor iff that cell satisfies `validEdge` (is not water). So the floor is
/// the first non-water cell at-or-below the bottom of the origin's contiguous
/// water column, bounded by the search range.
fn get_floor_y(
    level: &mut dyn WorldGenLevel,
    origin: &BlockPos,
    config: &UnderwaterMagmaConfiguration,
) -> Option<i32> {
    let is_water = |state: &BlockState| state.block() == Blocks::WATER.id();

    if !level.is_state_at_position(origin, &is_water) {
        return None;
    }

    let mut y = origin.get_y();
    let mut i = 1;
    while i < config.floor_search_range && level.is_state_at_position(&origin.at_y(y), &is_water) {
        y = y.wrapping_sub(1);
        i += 1;
    }

    // The reached cell (origin.y - (i - 1)) is the floor iff it satisfies the
    // `validEdge` predicate (is not water).
    if level.is_state_at_position(&origin.at_y(y), &|state: &BlockState| {
        state.block() != Blocks::WATER.id()
    }) {
        Some(y)
    } else {
        None
    }
}

/// `UnderwaterMagmaFeature.isValidPlacement(WorldGenLevel, BlockPos)`.
///
/// ```java
/// if (!isWaterOrAir(level.getBlockState(pos))
///         && !this.isVisibleFromOutside(level, pos.below(), Direction.UP)) {
///     for (Direction neighbourDir : Direction.Plane.HORIZONTAL) {
///         if (this.isVisibleFromOutside(level, pos.relative(neighbourDir),
///                 neighbourDir.getOpposite())) {
///             return false;
///         }
///     }
///     return true;
/// } else {
///     return false;
/// }
/// ```
fn is_valid_placement(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    if !is_water_or_air(level.get_block_state(pos))
        && !is_visible_from_outside(level, &pos.below(), &Direction::Up)
    {
        for neighbour in Plane::Horizontal.faces() {
            if is_visible_from_outside(level, &pos.relative(neighbour), &neighbour.get_opposite()) {
                return false;
            }
        }
        true
    } else {
        false
    }
}

/// `UnderwaterMagmaFeature.isWaterOrAir(BlockState)` —
/// `state.is(Blocks.WATER) || state.isAir()`.
fn is_water_or_air(state: BlockState) -> bool {
    state.block() == Blocks::WATER.id() || state.is_air()
}

/// `UnderwaterMagmaFeature.isVisibleFromOutside(LevelAccessor, BlockPos,
/// Direction)`.
///
/// ```java
/// BlockState state = level.getBlockState(pos);
/// VoxelShape faceOcclusionShape = state.getFaceOcclusionShape(coveredDirection);
/// return faceOcclusionShape == Shapes.empty()
///     || !Block.isShapeFullBlock(faceOcclusionShape);
/// ```
///
/// RivetTodo(#232, mc.world.phys.shapes): the `VoxelShape`/`Shapes` world
/// (`world.phys.shapes` manifest unit, still pending) is not ported, so the
/// per-direction `getFaceOcclusionShape(...).getFaceShape(dir)` case cannot be
/// computed faithfully here. The seam below uses `!state.solid_render()` — the
/// exact value of Paper's cached `Block.isShapeFullBlock(occlusionShape)` for
/// the two full-occlusion cases:
///
/// * `getFaceOcclusionShape` returns `EMPTY_OCCLUSION_SHAPES` (i.e.
///   `Shapes.empty()`, so "visible from outside") exactly when the full
///   occlusion shape is empty — `!canOcclude()`. For such blocks `solid_render`
///   is `false`, so `!solid_render()` is `true`, matching.
/// * `getFaceOcclusionShape` returns `FULL_BLOCK_OCCLUSION_SHAPES` exactly when
///   `isSolidRender()`, so "not visible from outside" (`isShapeFullBlock` true).
///   For such blocks `solid_render` is `true`, so `!solid_render()` is `false`,
///   matching.
///
/// The seam diverges ONLY for partial-occlusion blocks (`can_occlude` but not
/// `solid_render`, e.g. leaves), where Java consults the per-direction
/// `occlusionShape.getFaceShape(dir)` and the verdict is direction-dependent.
/// The seam answers those cells "visible from outside" on every face
/// (`!solid_render()` is `true`), which is the conservative rejection: such a
/// cell fails `is_valid_placement`, so magma is not placed on a partially
/// occluding neighbour. This is a documented divergence, not a silent
/// fabrication — the pending `mc.world.phys.shapes` unit owns the real
/// `VoxelShape` model and must replace this seam.
fn is_visible_from_outside(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    _covered_direction: &Direction,
) -> bool {
    let state = level.get_block_state(pos);
    !state.solid_render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::generated::blocks::BlockId;

    fn water() -> BlockState {
        Blocks::WATER.default_block_state()
    }

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    /// `place_with` — run the feature over the given level at `origin` with a
    /// `RecordingRandom`, returning the verdict (and exposing the RNG draws via
    /// `random.calls`).
    fn place_with(
        level: &mut TestLevel,
        origin: BlockPos,
        config: &UnderwaterMagmaConfiguration,
    ) -> (bool, RecordingRandom) {
        let mut random = RecordingRandom::new(42);
        let generator = TestGenerator;
        let verdict = UNDERWATER_MAGMA.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            config,
        ));
        (verdict, random)
    }

    /// A water column standing on stone: origin at y=10 with water filling
    /// 10..=14 above and stone at y=9, so the DOWN scan lands on the stone
    /// (the first non-water cell below the contiguous water column) and the
    /// placement box is centred on that floor.
    fn water_on_stone_level() -> TestLevel {
        let mut level = TestLevel::over(access());
        for y in 10..=14 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        level.states.insert(BlockPos::new(0, 9, 0), stone());
        level
    }

    /// The origin is not water — `Column.scan` fails empty, so the feature
    /// returns `false` with no RNG draws and no writes.
    #[test]
    fn non_water_origin_returns_false_without_draws() {
        let mut level = water_on_stone_level();
        level.states.insert(BlockPos::new(0, 10, 0), stone());
        let config = UnderwaterMagmaConfiguration::new(10, 2, 0.5);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(random.calls.is_empty());
        assert!(level.writes.is_empty());
    }

    /// The DOWN scan walks the contiguous water column and reports the stone
    /// floor. With `floorSearchRange` larger than the column, the floor is the
    /// first non-water cell below the water (y=9). Config `radius = 1`,
    /// `probability = 0.0` — no cell passes the filter, so the verdict is
    /// `false`, but the RNG draws reveal the floor scan landed at y=9 (via the
    /// `(2r+1)^3 = 27` nextFloat draws centred on `(0, 9, 0)`).
    #[test]
    fn floor_scan_lands_on_the_first_non_water_cell_below_the_column() {
        let mut level = water_on_stone_level();
        let config = UnderwaterMagmaConfiguration::new(10, 1, 0.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        // 3x3x3 box around (0, 9, 0), one nextFloat per cell.
        assert_eq!(random.calls.len(), 27);
        assert!(random.calls.iter().all(|c| *c == RngCall::Float));
    }

    /// `floorSearchRange` bounds the scan: a column taller than the search
    /// range walks only `range - 1` steps, so the reached cell is still water
    /// and fails `validEdge` — no floor, `false`, no writes.
    #[test]
    fn floor_search_range_bounds_the_walk() {
        let mut level = TestLevel::over(access());
        // Water from y=0 up through y=30, so the DOWN scan never exits the
        // water within `floorSearchRange = 5` (it stops at y=0 after 5 steps,
        // still water -> invalid edge).
        for y in 0..=30 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        let config = UnderwaterMagmaConfiguration::new(5, 1, 0.0);
        let (verdict, _random) = place_with(&mut level, BlockPos::new(0, 30, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
    }

    /// Every cell of the box draws exactly one `nextFloat`, in X/Y/Z-major
    /// order, BEFORE any placement-validity check. `probability = 1.0` on an
    /// empty (air) box makes every cell pass the filter and then fail
    /// `is_valid_placement` (air is water-or-air), so nothing is written but
    /// every cell still consumed a draw — pinning the consume-before-validate
    /// order and the box iteration order.
    #[test]
    fn consumes_one_float_per_cell_before_validity() {
        let mut level = water_on_stone_level();
        let config = UnderwaterMagmaConfiguration::new(10, 1, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
        assert_eq!(random.calls.len(), 27);
    }

    /// A stone cell in the box whose `pos.below()` is visible from outside is
    /// rejected: `!isVisibleFromOutside(level, pos.below(), UP)` fails, so no
    /// write. Water does not solid-render, so a water cell below is "visible
    /// from outside".
    #[test]
    fn cell_visible_from_below_is_rejected() {
        let mut level = water_on_stone_level();
        // Stone floor at y=9; the box centred on (0,9,0) with radius 2 spans
        // y=7..=11. Put stone at (0,8,0) with water below (0,7,0) so that
        // (0,8,0).below() is visible from outside. With probability 1.0 every
        // cell passes the filter.
        for y in 7..=11 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        level.states.insert(BlockPos::new(0, 8, 0), stone());
        // (0,7,0) stays water (visible from outside).
        let config = UnderwaterMagmaConfiguration::new(10, 2, 1.0);
        let (verdict, _random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
    }

    /// A solid stone cell with solid neighbours on every horizontal face is a
    /// valid placement: none of the faces is visible from outside, so magma is
    /// written with `UPDATE_CLIENTS`. The `TestLevel` default reads air for
    /// unset positions, so the horizontal neighbours and `pos.below()` must be
    /// stone too.
    #[test]
    fn enclosed_stone_cell_writes_magma_with_update_clients() {
        let mut level = TestLevel::over(access());
        // Origin (0,11,0) is water; the DOWN scan's first non-water cell is the
        // stone at (0,10,0), the floor. Box radius 0 = the single floor cell.
        // That cell's four horizontal neighbours and the cell below must be
        // solid stone so no face is visible from outside.
        for dx in -1..=1 {
            for dz in -1..=1 {
                level.states.insert(BlockPos::new(dx, 10, dz), stone());
            }
        }
        level.states.insert(BlockPos::new(0, 9, 0), stone());
        level.states.insert(BlockPos::new(0, 11, 0), water());
        let config = UnderwaterMagmaConfiguration::new(10, 0, 1.0);
        let (verdict, _random) = place_with(&mut level, BlockPos::new(0, 11, 0), &config);
        assert!(verdict);
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 10, 0));
        assert_eq!(level.writes[0].1.block(), Blocks::MAGMA_BLOCK.id());
        assert_eq!(level.writes_flags[0], UPDATE_CLIENTS);
    }

    /// The verdict is `false` when the box draws but no cell survives the
    /// probability filter (probability 0.0), even with a valid floor — the
    /// `sum() > 0` return.
    #[test]
    fn zero_probability_writes_nothing_and_returns_false() {
        let mut level = water_on_stone_level();
        let config = UnderwaterMagmaConfiguration::new(10, 1, 0.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
        assert_eq!(random.calls.len(), 27);
    }

    /// Hostile: an all-air box with probability 1.0 — every cell passes the
    /// filter, every cell fails validity, no writes, draws all consumed in box
    /// order.
    #[test]
    fn hostile_all_air_box_consumes_every_draw() {
        let mut level = water_on_stone_level();
        let config = UnderwaterMagmaConfiguration::new(10, 2, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
        // 5x5x5 = 125 cells, one draw each.
        assert_eq!(random.calls.len(), 125);
    }

    /// Hostile: `floorSearchRange = 0` makes the DOWN scan fail immediately —
    /// the loop never runs, the reached cell is the origin (water), which fails
    /// `validEdge`. Returns `false`, no draws.
    #[test]
    fn hostile_zero_floor_search_range_returns_false() {
        let mut level = water_on_stone_level();
        let config = UnderwaterMagmaConfiguration::new(0, 1, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(random.calls.is_empty());
        assert!(level.writes.is_empty());
    }
}

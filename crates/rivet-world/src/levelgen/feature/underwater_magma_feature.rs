//! Port of `net.minecraft.world.level.levelgen.feature.UnderwaterMagmaFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.underwatermagma`
//! manifest unit.
//!
//! Java: `Feature<UnderwaterMagmaConfiguration>` that scans the water column at
//! the origin (`Column.scan` with `insideColumn = is(WATER)`, `validEdge =
//! !is(WATER)`) in the exact UP-then-DOWN order and returns `false` when there
//! is no water column floor. Otherwise it builds the axis-aligned box around
//! the floor position
//! (`placementRadiusAroundFloor` on every axis), iterates its cells in
//! X/Y/Z-major order, and for every cell where `random.nextFloat() <
//! placementProbabilityPerValidPosition` places `MAGMA_BLOCK` when the cell is
//! a valid placement (a non-water/non-air cell whose faces are not visible from
//! outside, judged per direction via Paper's exact
//! `isShapeFullBlock(getFaceOcclusionShape)` verdict). The feature returns
//! `true` iff at least one cell was placed.
//!
//! The scan is ported through the `WorldGenLevel::is_state_at_position` seam
//! (the `LevelReader.isStateAtPosition` Java call `Column.scan` performs) and
//! the box iteration through the lazy `BlockBox::iter` (the
//! `betweenClosedStream` Java surface, x-fastest/y/z). The RNG is consumed for
//! every cell of the box BEFORE the placement validity check — the stream
//! `.filter` order — so the exact Java draw sequence is preserved without
//! materializing positions ahead of the stream consumer.

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
        for pos in bounds.iter() {
            // Java consumes a `nextFloat` for every cell of the lazy stream,
            // before the placement-validity check — the first stream `.filter`
            // runs before the second, so a rejected cell still draws.
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
/// UnderwaterMagmaConfiguration)` — the `Column.scan` water-column floor scan.
///
/// Java first checks the origin, then runs `Column.scanDirection` upward and
/// reads that direction's valid edge, before resetting to the origin and doing
/// the same downward scan. The returned floor is only the DOWN result; the UP
/// result is still observed because its reads are part of the behavior.
fn get_floor_y(
    level: &mut dyn WorldGenLevel,
    origin: &BlockPos,
    config: &UnderwaterMagmaConfiguration,
) -> Option<i32> {
    let inside_column = |state: &BlockState| state.block() == Blocks::WATER.id();
    let valid_edge = |state: &BlockState| state.block() != Blocks::WATER.id();

    if !level.is_state_at_position(origin, &inside_column) {
        return None;
    }

    let nearest_empty_y = origin.get_y();
    let mut y = nearest_empty_y;
    let mut i = 1;
    while i < config.floor_search_range
        && level.is_state_at_position(&origin.at_y(y), &inside_column)
    {
        y = y.wrapping_add(1);
        i += 1;
    }
    let _ceiling = level.is_state_at_position(&origin.at_y(y), &valid_edge);

    y = nearest_empty_y;
    i = 1;
    while i < config.floor_search_range
        && level.is_state_at_position(&origin.at_y(y), &inside_column)
    {
        y = y.wrapping_sub(1);
        i += 1;
    }
    if level.is_state_at_position(&origin.at_y(y), &valid_edge) {
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
/// Paper's `occlusionShapesByFace[direction]` is, after `initCache`,
/// `EMPTY_OCCLUSION_SHAPES` when the full occlusion shape is empty, or
/// `FULL_BLOCK_OCCLUSION_SHAPES` when `isSolidRender()`, or otherwise the
/// exact `occlusionShape.getFaceShape(direction)`. Java's visible-from-outside
/// verdict is therefore, per direction, `!Block.isShapeFullBlock(
/// getFaceOcclusionShape(direction))` — exactly the per-direction bit the
/// #653 occlusion query (`WorldGenLevel::is_occlusion_face_full`, which in turn
/// answers `occlusion_face_mask` from the pinned probe data) provides. This is
/// the faithful per-direction verdict for every case (empty/full/partial
/// occlusion), so the earlier `!solid_render()` shortcut (which was wrong for
/// partial-occlusion faces) is replaced by the dynamic shape support/occlusion
/// facilities from #653.
fn is_visible_from_outside(
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
    covered_direction: &Direction,
) -> bool {
    let state = level.get_block_state(pos);
    !level.is_occlusion_face_full(pos, &state, covered_direction)
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

    /// Fill an inclusive box with stone — the solid-volume helper for the
    /// fully-enclosed placement tests.
    fn fill_stone(level: &mut TestLevel, min: BlockPos, max: BlockPos) {
        for x in min.get_x()..=max.get_x() {
            for y in min.get_y()..=max.get_y() {
                for z in min.get_z()..=max.get_z() {
                    level.states.insert(BlockPos::new(x, y, z), stone());
                }
            }
        }
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
    /// floor. This observes the selected floor directly through the write: with
    /// `radius = 0` the box is the single floor cell, and `probability = 1.0`
    /// makes every draw pass the filter, so a write happens exactly when the
    /// floor cell is a valid placement. The level makes (0, 9, 0) — and only
    /// that cell — a valid placement (stone with stone below and solid
    /// horizontal neighbours):
    ///
    /// * correct floor y=9 → writes `MAGMA_BLOCK` at (0, 9, 0), verdict `true`;
    /// * a buggy scan that never descends (floor = origin y=10) → the box cell
    ///   is water, `is_water_or_air` rejects → `false`, no write;
    /// * a buggy over-descent (floor = y=8) → the box cell is stone but its
    ///   `pos.below()` (0, 7, 0) is air/visible → rejected → `false`, no write.
    #[test]
    fn floor_scan_lands_on_the_first_non_water_cell_below_the_column() {
        let mut level = water_on_stone_level();
        // (0, 8, 0) below the floor and the four horizontal neighbours of the
        // floor cell are stone, so (0, 9, 0) is the only valid placement.
        level.states.insert(BlockPos::new(0, 8, 0), stone());
        for neighbour in Plane::Horizontal.faces() {
            level
                .states
                .insert(BlockPos::new(0, 9, 0).relative(neighbour), stone());
        }
        let config = UnderwaterMagmaConfiguration::new(10, 0, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(verdict);
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 9, 0));
        assert_eq!(level.writes[0].1.block(), Blocks::MAGMA_BLOCK.id());
        assert_eq!(level.writes_flags[0], UPDATE_CLIENTS);
        // A single box cell -> exactly one draw.
        assert_eq!(random.calls, vec![RngCall::Float]);
    }

    /// `floorSearchRange` bounds the scan: from the origin the DOWN scan walks
    /// at most `range - 1` steps (y=30 -> y=26 with range 5), so the reached
    /// cell is still water and fails `validEdge` — no floor, `false`, and — the
    /// observable pin — zero RNG draws. A buggy scan that ignored the range
    /// would keep descending to the stone at y=5, find a floor, and start
    /// drawing the box, so `random.calls.is_empty()` catches it.
    #[test]
    fn floor_search_range_bounds_the_walk() {
        let mut level = TestLevel::over(access());
        // Water from y=6 up through y=30 (the scan's reach), with the first
        // non-water cell at y=5 — just beyond the `floorSearchRange = 5`
        // boundary from the origin at y=30.
        for y in 6..=30 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        level.states.insert(BlockPos::new(0, 5, 0), stone());
        let config = UnderwaterMagmaConfiguration::new(5, 1, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 30, 0), &config);
        assert!(!verdict);
        assert!(random.calls.is_empty());
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
    /// rejected by the `!isVisibleFromOutside(level, pos.below(), UP)` check.
    /// The candidate's four horizontal neighbours are solid stone, so the ONLY
    /// face that can reject it is the water cell below — deleting the
    /// `pos.below()` check would let it write, so the empty-writes assertion
    /// isolates that check. Water does not solid-render, so it is "visible from
    /// outside".
    #[test]
    fn cell_visible_from_below_is_rejected() {
        let mut level = TestLevel::over(access());
        // Column: origin (0,10,0) is water with water at (0,9,0), the floor is
        // stone at (0,8,0), its four horizontal neighbours are solid stone, and
        // the cell below stays water (visible from outside).
        for y in 9..=11 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        level.states.insert(BlockPos::new(0, 8, 0), stone());
        for neighbour in Plane::Horizontal.faces() {
            level
                .states
                .insert(BlockPos::new(0, 8, 0).relative(neighbour), stone());
        }
        level.states.insert(BlockPos::new(0, 7, 0), water());
        let config = UnderwaterMagmaConfiguration::new(10, 2, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
        // The 5x5x5 box still consumes every draw even though the floor cell is
        // rejected — consume-before-validate on the full box.
        assert_eq!(random.calls.len(), 125);
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

    /// A fully-solid box writes every valid cell in exact X/Y/Z-major box
    /// order, associating each draw with its cell: the 27 box cells each
    /// consume one `nextFloat`, and the cells that survive `is_valid_placement`
    /// are written in the same order the box iterates. In this geometry the box
    /// (radius 1 around the floor at (0,9,0)) spans (-1,8,-1)..(1,10,1); the
    /// water column's base cell (0,10,0) is unavoidably inside it (the origin's
    /// column meets the floor), so it — and its four horizontal neighbours at
    /// y=10, which each see a water face — are rejected while every other cell
    /// is written. The observable write positions thus pin the iteration order,
    /// and 27 draws for 22 writes pins consume-before-validate.
    #[test]
    fn writes_every_valid_cell_in_box_order() {
        let mut level = TestLevel::over(access());
        // Solid box plus its horizontal halo (neighbours reach x,z in -2..=2)
        // at y=8..=10, and the y=7 layer below the box, so every non-water
        // cell is a valid placement.
        fill_stone(
            &mut level,
            BlockPos::new(-2, 8, -2),
            BlockPos::new(2, 10, 2),
        );
        fill_stone(&mut level, BlockPos::new(-1, 7, -1), BlockPos::new(1, 7, 1));
        // Punch the water column back in: the origin above the box and the base
        // of the column inside it.
        level.states.insert(BlockPos::new(0, 10, 0), water());
        level.states.insert(BlockPos::new(0, 11, 0), water());

        let config = UnderwaterMagmaConfiguration::new(10, 1, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 11, 0), &config);

        assert!(verdict);
        // One `nextFloat` per box cell, in box order.
        assert_eq!(random.calls, vec![RngCall::Float; 27]);

        // The 22 valid cells in box order (z outermost, then y, then x), with
        // the water column base (0,10,0) and its four y=10 horizontal
        // neighbours omitted.
        let expected = vec![
            BlockPos::new(-1, 8, -1),
            BlockPos::new(0, 8, -1),
            BlockPos::new(1, 8, -1),
            BlockPos::new(-1, 9, -1),
            BlockPos::new(0, 9, -1),
            BlockPos::new(1, 9, -1),
            BlockPos::new(-1, 10, -1),
            BlockPos::new(1, 10, -1),
            BlockPos::new(-1, 8, 0),
            BlockPos::new(0, 8, 0),
            BlockPos::new(1, 8, 0),
            BlockPos::new(-1, 9, 0),
            BlockPos::new(0, 9, 0),
            BlockPos::new(1, 9, 0),
            BlockPos::new(-1, 8, 1),
            BlockPos::new(0, 8, 1),
            BlockPos::new(1, 8, 1),
            BlockPos::new(-1, 9, 1),
            BlockPos::new(0, 9, 1),
            BlockPos::new(1, 9, 1),
            BlockPos::new(-1, 10, 1),
            BlockPos::new(1, 10, 1),
        ];
        let positions: Vec<BlockPos> = level.writes.iter().map(|(pos, _)| *pos).collect();
        assert_eq!(positions, expected);
        for (_, state) in &level.writes {
            assert_eq!(state.block(), Blocks::MAGMA_BLOCK.id());
        }
        assert!(level.writes_flags.iter().all(|&f| f == UPDATE_CLIENTS));
    }

    /// A cell with solid support below but one exposed horizontal face is
    /// rejected: `isValidPlacement` walks the four `Direction.Plane.HORIZONTAL`
    /// faces, and an air neighbour is visible from outside. Radius 0 centres
    /// the box on the single floor cell, so the verdict is `false` with no
    /// writes — and the one draw was still consumed before validity.
    #[test]
    fn exposed_horizontal_neighbour_rejects_the_cell() {
        let mut level = water_on_stone_level();
        // Floor (0,9,0) with solid below (0,8,0) and three solid horizontal
        // neighbours; the east neighbour stays air (visible from outside).
        level.states.insert(BlockPos::new(0, 8, 0), stone());
        for neighbour in Plane::Horizontal.faces() {
            if *neighbour != Direction::East {
                level
                    .states
                    .insert(BlockPos::new(0, 9, 0).relative(neighbour), stone());
            }
        }
        let config = UnderwaterMagmaConfiguration::new(10, 0, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
        // One box cell -> one draw, consumed before the validity check.
        assert_eq!(random.calls, vec![RngCall::Float]);
    }

    /// A partial-occlusion block whose face toward the candidate is NOT full
    /// leaves that face visible from outside, so the candidate is rejected. As
    /// the EAST neighbour of the floor cell, `minecraft:oak_shelf`'s West face
    /// (`getFaceOcclusionShape`, occ_mask = South only) is not a full block, so
    /// `isVisibleFromOutside` is true and the cell fails validity — the exact
    /// per-direction verdict, not the whole-block conservative shortcut.
    #[test]
    fn partial_occlusion_non_full_face_is_visible() {
        let oak_shelf = BlockState::of(BlockId::from_name("minecraft:oak_shelf").unwrap());
        assert!(!oak_shelf.solid_render());

        let mut level = water_on_stone_level();
        // Floor (0,9,0) with solid below and three solid horizontal neighbours;
        // the east neighbour is the oak_shelf (West face not full).
        level.states.insert(BlockPos::new(0, 8, 0), stone());
        for neighbour in Plane::Horizontal.faces() {
            if *neighbour != Direction::East {
                level
                    .states
                    .insert(BlockPos::new(0, 9, 0).relative(neighbour), stone());
            }
        }
        level.states.insert(BlockPos::new(1, 9, 0), oak_shelf);

        let config = UnderwaterMagmaConfiguration::new(10, 0, 1.0);
        let (verdict, _random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(!verdict);
        assert!(level.writes.is_empty());
    }

    /// Mixed `0 < p < 1` on a fully-solid box: every cell draws exactly one
    /// `nextFloat` (consume-before-validate, so 27 draws for the 27-cell box),
    /// and the cells that survive BOTH the probability filter and the
    /// placement-validity check are written in box order. The deterministic
    /// seed-42 draw sequence decides which cells pass the `nextFloat < 0.5`
    /// filter; the written positions pin that surviving set AND the
    /// consume-before-validate interleaving (a rejected cell still drew, so the
    /// count is 27 even though only 13 write). The `reads` log pins the
    /// interleaving at the other seam: each cell's placement-validity world
    /// reads (`pos`, `pos.below()`, and the four horizontal neighbours) are
    /// interleaved between the RNG draws, so a `nextFloat` is consumed before
    /// the validity world reads for every cell — the exact Java stream
    /// `.filter(...).filter(...)` order.
    #[test]
    fn mixed_probability_interleaves_draws_before_validity_reads() {
        // Full-solid box radius 1 around floor at (0,9,0) + halo — the same
        // geometry as `writes_every_valid_cell_in_box_order`, but filtered by
        // p=0.5 instead of 1.0.
        let mut level = TestLevel::over(access());
        fill_stone(
            &mut level,
            BlockPos::new(-2, 8, -2),
            BlockPos::new(2, 10, 2),
        );
        fill_stone(&mut level, BlockPos::new(-1, 7, -1), BlockPos::new(1, 7, 1));
        level.states.insert(BlockPos::new(0, 10, 0), water());
        level.states.insert(BlockPos::new(0, 11, 0), water());

        let config = UnderwaterMagmaConfiguration::new(10, 1, 0.5);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 11, 0), &config);

        assert!(verdict);
        // One `nextFloat` per box cell, in box order — all consumed before the
        // validity check, so rejected cells still drew.
        assert_eq!(random.calls, vec![RngCall::Float; 27]);

        // The seed-42 deterministic surviving set, in box order: the cells that
        // pass both the p=0.5 filter and validity. (Determined by the pinned
        // LegacyRandomSource seed 42; the count 13 < 27 over the same geometry
        // as the p=1.0 test above pins the probability filter removing cells.)
        let expected = vec![
            BlockPos::new(0, 8, -1),
            BlockPos::new(-1, 9, -1),
            BlockPos::new(0, 9, -1),
            BlockPos::new(-1, 10, -1),
            BlockPos::new(-1, 8, 0),
            BlockPos::new(1, 8, 0),
            BlockPos::new(-1, 9, 0),
            BlockPos::new(0, 9, 0),
            BlockPos::new(1, 9, 0),
            BlockPos::new(-1, 9, 1),
            BlockPos::new(0, 9, 1),
            BlockPos::new(1, 9, 1),
            BlockPos::new(1, 10, 1),
        ];
        let positions: Vec<BlockPos> = level.writes.iter().map(|(pos, _)| *pos).collect();
        assert_eq!(positions, expected);
        for (_, state) in &level.writes {
            assert_eq!(state.block(), Blocks::MAGMA_BLOCK.id());
        }
        assert!(level.writes_flags.iter().all(|&f| f == UPDATE_CLIENTS));

        // Interleaving: the `Column.scan` floor reads (8 reads) come FIRST and
        // consume no RNG—the floor search completes before the box loop draws.
        // Then, per box cell in X/Y/Z-major order, the RNG draws once BEFORE
        // the placement-validity world reads. A cell that the probability
        // filter rejects (float >= p) performs NO validity reads — its draw was
        // consumed but the stream short-circuited before the second `.filter`
        // — while a cell that survives the filter performs its validity reads
        // (own position, below, then horizontal faces until the first visible
        // face). The exact seed-42 log below pins this interleaving read-by-read
        // against the draw sequence: the reads are grouped by box cell in box
        // order, each group preceded by one `nextFloat`, and rejected cells
        // contribute no reads.
        //
        // To keep the log readable we assert the full deterministic read log
        // (floor-scan prefix + box-validity reads) exactly. The floor-scan
        // prefix is the same 8-read sequence the hostile
        // `hostile_column_scan_read_log_is_paper_order` test pins; the box
        // groups follow the seed-42 filtered cell set.
        let reads: Vec<BlockPos> = level.reads.borrow().clone();
        // The exact deterministic read log (dumped from this test's fixed
        // geometry and seed 42). The 8-read floor-scan prefix matches the
        // hostile `hostile_column_scan_read_log_is_paper_order` sequence; the
        // box groups then interleave one draw per cell with that cell's
        // validity reads, with probability-rejected cells contributing no reads.
        assert_eq!(
            reads,
            [
                BlockPos::new(0, 11, 0),
                BlockPos::new(0, 11, 0),
                BlockPos::new(0, 12, 0),
                BlockPos::new(0, 12, 0),
                BlockPos::new(0, 11, 0),
                BlockPos::new(0, 10, 0),
                BlockPos::new(0, 9, 0),
                BlockPos::new(0, 9, 0),
                BlockPos::new(0, 8, -1),
                BlockPos::new(0, 7, -1),
                BlockPos::new(0, 8, -2),
                BlockPos::new(1, 8, -1),
                BlockPos::new(0, 8, 0),
                BlockPos::new(-1, 8, -1),
                BlockPos::new(-1, 9, -1),
                BlockPos::new(-1, 8, -1),
                BlockPos::new(-1, 9, -2),
                BlockPos::new(0, 9, -1),
                BlockPos::new(-1, 9, 0),
                BlockPos::new(-2, 9, -1),
                BlockPos::new(0, 9, -1),
                BlockPos::new(0, 8, -1),
                BlockPos::new(0, 9, -2),
                BlockPos::new(1, 9, -1),
                BlockPos::new(0, 9, 0),
                BlockPos::new(-1, 9, -1),
                BlockPos::new(-1, 10, -1),
                BlockPos::new(-1, 9, -1),
                BlockPos::new(-1, 10, -2),
                BlockPos::new(0, 10, -1),
                BlockPos::new(-1, 10, 0),
                BlockPos::new(-2, 10, -1),
                BlockPos::new(-1, 8, 0),
                BlockPos::new(-1, 7, 0),
                BlockPos::new(-1, 8, -1),
                BlockPos::new(0, 8, 0),
                BlockPos::new(-1, 8, 1),
                BlockPos::new(-2, 8, 0),
                BlockPos::new(1, 8, 0),
                BlockPos::new(1, 7, 0),
                BlockPos::new(1, 8, -1),
                BlockPos::new(2, 8, 0),
                BlockPos::new(1, 8, 1),
                BlockPos::new(0, 8, 0),
                BlockPos::new(-1, 9, 0),
                BlockPos::new(-1, 8, 0),
                BlockPos::new(-1, 9, -1),
                BlockPos::new(0, 9, 0),
                BlockPos::new(-1, 9, 1),
                BlockPos::new(-2, 9, 0),
                BlockPos::new(0, 9, 0),
                BlockPos::new(0, 8, 0),
                BlockPos::new(0, 9, -1),
                BlockPos::new(1, 9, 0),
                BlockPos::new(0, 9, 1),
                BlockPos::new(-1, 9, 0),
                BlockPos::new(1, 9, 0),
                BlockPos::new(1, 8, 0),
                BlockPos::new(1, 9, -1),
                BlockPos::new(2, 9, 0),
                BlockPos::new(1, 9, 1),
                BlockPos::new(0, 9, 0),
                BlockPos::new(0, 10, 0),
                BlockPos::new(-1, 9, 1),
                BlockPos::new(-1, 8, 1),
                BlockPos::new(-1, 9, 0),
                BlockPos::new(0, 9, 1),
                BlockPos::new(-1, 9, 2),
                BlockPos::new(-2, 9, 1),
                BlockPos::new(0, 9, 1),
                BlockPos::new(0, 8, 1),
                BlockPos::new(0, 9, 0),
                BlockPos::new(1, 9, 1),
                BlockPos::new(0, 9, 2),
                BlockPos::new(-1, 9, 1),
                BlockPos::new(1, 9, 1),
                BlockPos::new(1, 8, 1),
                BlockPos::new(1, 9, 0),
                BlockPos::new(2, 9, 1),
                BlockPos::new(1, 9, 2),
                BlockPos::new(0, 9, 1),
                BlockPos::new(1, 10, 1),
                BlockPos::new(1, 9, 1),
                BlockPos::new(1, 10, 0),
                BlockPos::new(2, 10, 1),
                BlockPos::new(1, 10, 2),
                BlockPos::new(0, 10, 1),
            ]
        );
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

    /// Hostile: the read log pins `Column.scan`'s origin read, complete UP
    /// scan (including its valid-edge read), origin reset, and complete DOWN
    /// scan (including its valid-edge read). The zero probability prevents the
    /// placement loop from adding any later world reads to the sequence.
    #[test]
    fn hostile_column_scan_read_log_is_paper_order() {
        let mut level = TestLevel::over(access());
        for y in 10..=11 {
            level.states.insert(BlockPos::new(0, y, 0), water());
        }
        level.states.insert(BlockPos::new(0, 9, 0), stone());
        let config = UnderwaterMagmaConfiguration::new(4, 0, 0.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);

        assert!(!verdict);
        assert_eq!(random.calls, vec![RngCall::Float]);
        assert_eq!(
            level.reads.borrow().as_slice(),
            [
                BlockPos::new(0, 10, 0),
                BlockPos::new(0, 10, 0),
                BlockPos::new(0, 11, 0),
                BlockPos::new(0, 12, 0),
                BlockPos::new(0, 12, 0),
                BlockPos::new(0, 10, 0),
                BlockPos::new(0, 9, 0),
                BlockPos::new(0, 9, 0),
            ]
        );
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

    /// A partial-occlusion block with a FULL occlusion face across a face still
    /// occludes that face: `minecraft:oak_shelf` is `can_occlude` but not
    /// `solid_render`, and its `getFaceOcclusionShape(SOUTH)` returns a full
    /// block shape while its other faces do not (occ_mask = South only). As the
    /// NORTH neighbour of the floor cell, its South face closes the floor cell,
    /// so the exact per-direction occlusion verdict accepts the placement — the
    /// old `!solid_render()` shortcut would have answered "visible from
    /// outside" on every face and rejected it. This pins that the seam now
    /// computes Paper's exact per-direction `isShapeFullBlock(
    /// getFaceOcclusionShape(dir))` instead of the whole-block conservative
    /// shortcut.
    #[test]
    fn partial_occlusion_full_face_still_occludes() {
        let oak_shelf = BlockState::of(BlockId::from_name("minecraft:oak_shelf").unwrap());
        // Guard the seam premise: oak_shelf is genuinely a partial-occlusion
        // block whose South occlusion face is full in this model's generated
        // tables — if the behavior tables ever change this, the test's premise
        // needs re-examining.
        assert!(!oak_shelf.solid_render());
        let full_faces = oak_shelf.occlusion_face_mask();
        // South (bit 3) full, exactly one full face expected.
        assert_eq!(full_faces, 1 << Direction::South.get_3d_data_value() as u8);

        let mut level = water_on_stone_level();
        level.states.insert(BlockPos::new(0, 8, 0), stone());
        for neighbour in Plane::Horizontal.faces() {
            if *neighbour != Direction::North {
                level
                    .states
                    .insert(BlockPos::new(0, 9, 0).relative(neighbour), stone());
            }
        }
        level.states.insert(BlockPos::new(0, 9, -1), oak_shelf);

        let config = UnderwaterMagmaConfiguration::new(10, 0, 1.0);
        let (verdict, random) = place_with(&mut level, BlockPos::new(0, 10, 0), &config);
        assert!(verdict);
        assert_eq!(
            level.writes,
            vec![(
                BlockPos::new(0, 9, 0),
                Blocks::MAGMA_BLOCK.default_block_state()
            )]
        );
        assert_eq!(random.calls, vec![RngCall::Float]);
    }
}

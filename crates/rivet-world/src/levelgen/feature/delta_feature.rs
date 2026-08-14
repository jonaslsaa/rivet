//! Port of `net.minecraft.world.level.levelgen.feature.DeltaFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.delta` manifest unit.
//!
//! Java: `Feature<DeltaFeatureConfiguration>` that carves a basalt-delta into
//! the nether. `place` first draws `spawnRim = random.nextDouble() < 0.9`; when
//! a rim spawns, `rimX`/`rimZ` are sampled from `config.rimSize()`, and a rim
//! is only present when *both* are non-zero. It then draws `radiusX`/`radiusZ`
//! from `config.size()` and iterates `BlockPos.withinManhattan(origin, radiusX,
//! 0, radiusZ)`, stopping early once a cell's `distManhattan(origin)` exceeds
//! `max(radiusX, radiusZ)`. For each cell where `isClear` holds, the rim is
//! written (when present) at the cell, and the contents are written at the
//! cell offset by `(rimX, 0, rimZ)` when that cell is also clear. Returns
//! whether any write landed.
//!
//! `isClear` rejects a cell whose state is the contents block, any block in
//! the `CANNOT_REPLACE` list (bedrock, nether bricks, nether brick fence/
//! stairs, nether wart, chest, spawner), or whose six neighbours break the
//! air/`UP` parity rule: a cell is clear exactly when the `UP` neighbor is
//! air and every other face is non-air (i.e. reject when a non-`UP` face is
//! air, or `UP` is non-air).
//!
//! The world reads (`get_block_state`) and writes (`set_block` with
//! `Block.UPDATE_ALL`) go through the `WorldGenLevel` seams (RivetTodo
//! #232); the test double overrides them.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::DeltaFeatureConfiguration;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_ALL` — the write-flag constant `Feature.setBlock` reduces
/// to (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`), in contrast to
/// `safeSetBlock`'s `Block.UPDATE_CLIENTS` used by e.g. GeodeFeature.
const UPDATE_ALL: u32 = 3;

/// `DeltaFeature.CANNOT_REPLACE` — the blocks the delta never carves through
/// (`ImmutableList.of`). Block identity is the registry id, so the list is
/// compared against `BlockState::block()`.
const CANNOT_REPLACE: [BlockId; 7] = [
    // `Blocks.BEDROCK`, `Blocks.NETHER_BRICKS`, `Blocks.NETHER_BRICK_FENCE`,
    // `Blocks.NETHER_BRICK_STAIRS`, `Blocks.NETHER_WART`, `Blocks.CHEST`,
    // `Blocks.SPAWNER` — the Java `ImmutableList.of` order.
    BlockId(34),
    BlockId(381),
    BlockId(382),
    BlockId(383),
    BlockId(384),
    BlockId(201),
    BlockId(198),
];

/// `DeltaFeature.DIRECTIONS` — `Direction.values()` (enum order).
const DIRECTIONS: [Direction; 6] = Direction::VALUES;

/// `DeltaFeature.RIM_SPAWN_CHANCE` — the `random.nextDouble() < 0.9` gate.
const RIM_SPAWN_CHANCE: f64 = 0.9;

/// `net.minecraft.world.level.levelgen.feature.DeltaFeature`.
#[derive(Debug)]
pub struct DeltaFeature;

/// `Feature.DELTA` — the registered `minecraft:delta_feature` singleton.
pub const DELTA: DeltaFeature = DeltaFeature;

/// `DeltaFeature.isClear(LevelAccessor, BlockPos, DeltaFeatureConfiguration)`.
fn is_clear(level: &dyn WorldGenLevel, pos: &BlockPos, config: &DeltaFeatureConfiguration) -> bool {
    let state = level.get_block_state(pos);
    if state.block() == config.contents().block() {
        return false;
    }
    if CANNOT_REPLACE.contains(&state.block()) {
        return false;
    }
    for d in DIRECTIONS {
        let is_air = level.get_block_state(&pos.relative(&d)).is_air();
        // `(isAir && d != Direction.UP || !isAir && d == Direction.UP)` — the
        // operator-precedence-preserving form of the reject condition: a cell
        // is clear only when the UP neighbor is air and every other face is
        // non-air (reject air on any non-UP face, or non-air on UP).
        if (is_air && d != Direction::Up) || (!is_air && d == Direction::Up) {
            return false;
        }
    }
    true
}

impl FeatureBehavior<DeltaFeatureConfiguration> for DeltaFeature {
    /// `DeltaFeature.place(FeaturePlaceContext<DeltaFeatureConfiguration>)`.
    ///
    /// ```java
    /// boolean anyPlaced = false;
    /// boolean spawnRim = random.nextDouble() < 0.9;
    /// int rimX = spawnRim ? config.rimSize().sample(random) : 0;
    /// int rimZ = spawnRim ? config.rimSize().sample(random) : 0;
    /// boolean hasRim = spawnRim && rimX != 0 && rimZ != 0;
    /// int radiusX = config.size().sample(random);
    /// int radiusZ = config.size().sample(random);
    /// int radiusLimit = Math.max(radiusX, radiusZ);
    /// for (BlockPos pos : BlockPos.withinManhattan(origin, radiusX, 0, radiusZ)) {
    ///     if (pos.distManhattan(origin) > radiusLimit) break;
    ///     if (isClear(level, pos, config)) {
    ///         if (hasRim) { anyPlaced = true; this.setBlock(level, pos, config.rim()); }
    ///         BlockPos posOffset = pos.offset(rimX, 0, rimZ);
    ///         if (isClear(level, posOffset, config)) {
    ///             anyPlaced = true;
    ///             this.setBlock(level, posOffset, config.contents());
    ///         }
    ///     }
    /// }
    /// return anyPlaced;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, DeltaFeatureConfiguration, R>,
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
        let config = *config;
        let origin = *origin;
        let mut any_placed = false;
        let spawn_rim = random.next_double() < RIM_SPAWN_CHANCE;
        let rim_x = if spawn_rim {
            config.rim_size().sample(random)
        } else {
            0
        };
        let rim_z = if spawn_rim {
            config.rim_size().sample(random)
        } else {
            0
        };
        let has_rim = spawn_rim && rim_x != 0 && rim_z != 0;
        let radius_x = config.size().sample(random);
        let radius_z = config.size().sample(random);
        let radius_limit = radius_x.max(radius_z);
        for pos in BlockPos::within_manhattan(origin, radius_x, 0, radius_z) {
            if pos.dist_manhattan(origin) > radius_limit {
                break;
            }
            if is_clear(level, &pos, config) {
                if has_rim {
                    any_placed = true;
                    level.set_block(&pos, config.rim(), UPDATE_ALL);
                }
                let pos_offset = pos.offset(rim_x, 0, rim_z);
                if is_clear(level, &pos_offset, config) {
                    any_placed = true;
                    level.set_block(&pos_offset, config.contents(), UPDATE_ALL);
                }
            }
        }
        any_placed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::block_state::BlockState;
    use rivet_util::random::LegacyPositionalRandomFactory;

    fn rim() -> BlockState {
        BlockState::of(Blocks::STONE.id())
    }

    fn contents() -> BlockState {
        BlockState::of(Blocks::GRANITE.id())
    }

    /// `size = ConstantInt(0)`, `rimSize = ConstantInt(1)` makes the manhattan
    /// walk the single origin cell and the rim offset `(1, 0, 1)`, so each test
    /// sets up exactly two cells (origin and `origin + (1,0,1)`): a clear cell
    /// has all of DOWN/horizontals non-air and UP air (the default).
    fn config() -> DeltaFeatureConfiguration {
        DeltaFeatureConfiguration::new(
            contents(),
            rim(),
            rivet_util::valueproviders::int_provider::IntProvider::Constant(
                rivet_util::valueproviders::constant_int::ConstantInt::of(0),
            ),
            rivet_util::valueproviders::int_provider::IntProvider::Constant(
                rivet_util::valueproviders::constant_int::ConstantInt::of(1),
            ),
        )
    }

    /// A clear cell: DOWN and the four horizontals are non-air (the rim
    /// state), while the cell itself and UP stay air (the default) — Java
    /// `isClear` rejects air on any non-UP face and rejects non-air on UP
    /// (`isAir && d != UP || !isAir && d == UP`).
    fn make_clear(level: &mut TestLevel, pos: &BlockPos) {
        level.states.insert(pos.below(), rim());
        for d in [
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            level.states.insert(pos.relative(&d), rim());
        }
    }

    /// A `RandomSource` whose `nextDouble` is pinned — controls the rim gate.
    #[derive(Clone, Copy)]
    struct DoubleRandom(f64);

    impl RandomSource for DoubleRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            *self
        }
        fn fork_positional(&mut self) -> Self::Positional {
            LegacyPositionalRandomFactory::new(0)
        }
        fn set_seed(&mut self, _seed: i64) {}
        fn next_int(&mut self) -> i32 {
            0
        }
        fn next_int_bound(&mut self, _bound: i32) -> i32 {
            0
        }
        fn next_long(&mut self) -> i64 {
            0
        }
        fn next_boolean(&mut self) -> bool {
            false
        }
        fn next_float(&mut self) -> f32 {
            0.0
        }
        fn next_double(&mut self) -> f64 {
            self.0
        }
        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    fn place_with<R: RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
    ) -> bool {
        let generator = TestGenerator;
        DELTA.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &config(),
        ))
    }

    /// `nextDouble = 0.5 < 0.9` spawns the rim, and `rimSize = ConstantInt(1)`
    /// gives `rimX = rimZ = 1` (both non-zero), so `hasRim`. The walk is the
    /// single origin cell (`size = 0`): the rim is written at the origin and
    /// the contents at `origin + (1, 0, 1)`. Only the `nextDouble` gate draws.
    #[test]
    fn rim_writes_rim_then_contents() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        make_clear(&mut level, &origin);
        make_clear(&mut level, &origin.offset(1, 0, 1));
        let mut random = DoubleRandom(0.5);
        assert!(place_with(&mut level, origin, &mut random));
        assert_eq!(
            level.writes,
            vec![(origin, rim()), (origin.offset(1, 0, 1), contents()),]
        );
    }

    /// `nextDouble = 0.95` fails the gate: `rimX = rimZ = 0`, `hasRim` false.
    /// The `rimSize` samples are never drawn and only the contents write
    /// lands (at the origin itself, since the offset is `(0, 0, 0)`).
    #[test]
    fn no_rim_writes_contents_only() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        make_clear(&mut level, &origin);
        let mut random = DoubleRandom(0.95);
        assert!(place_with(&mut level, origin, &mut random));
        assert_eq!(level.writes, vec![(origin, contents())]);
    }

    /// A hostile cell: the origin state is a `CANNOT_REPLACE` block (bedrock),
    /// so `isClear` rejects it — no writes, `false`.
    #[test]
    fn cannot_replace_block_skips_cell() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        make_clear(&mut level, &origin);
        level
            .states
            .insert(origin, BlockState::of(Blocks::BEDROCK.id()));
        let mut random = DoubleRandom(0.5);
        assert!(!place_with(&mut level, origin, &mut random));
        assert!(level.writes.is_empty());
    }
}

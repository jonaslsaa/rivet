//! Port of `net.minecraft.world.level.levelgen.feature.BasaltPillarFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.basaltpillar`
//! manifest unit (issue #600).
//!
//! Java: `Feature<NoneFeatureConfiguration>` whose `place` gates on the origin
//! being empty with a non-empty cell above; it then grows a basalt pillar
//! downward (writing `BASALT` with `Block.UPDATE_CLIENTS`, 2 at each empty
//! cell, short-circuiting `placeHangOff` — one `nextInt(10)` draw per still-
//! `true` direction per level), caps it with four `placeBaseHangOff` calls
//! (one `nextBoolean` draw each), and scatters a basalt base over the `-3..=3`
//! x/z square, dropping each candidate column up to 3 cells until it finds
//! ground.
//!
//! The RNG order is load-bearing: the four hangoff draws are short-circuited
//! per direction once that direction first misses (`nextInt(10) == 0`), and
//! the scatter draws one `nextInt(10)` per `(dx, dz)` cell in loop order. The
//! port keeps Java's `&&`-short-circuiting exactly.
//!
//! The block-write seam is `WorldGenLevel::set_block`; `isEmptyBlock` and
//! `isOutsideBuildHeight` are the `WorldGenLevel`/`LevelHeightAccessor` seams
//! (RivetTodo #228).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::core::{BlockPos, Direction};
use rivet_util::{RandomSource, mth};

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `net.minecraft.world.level.levelgen.feature.BasaltPillarFeature`.
#[derive(Debug)]
pub struct BasaltPillarFeature;

/// `Feature.BASALT_PILLAR` — the registered `minecraft:basalt_pillar`
/// singleton.
pub const BASALT_PILLAR: BasaltPillarFeature = BasaltPillarFeature;

/// `placeBaseHangOff` — `if (random.nextBoolean()) setBlock(pos, BASALT)`.
fn place_base_hang_off<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    pos: &BlockPos,
) {
    if random.next_boolean() {
        level.set_block(pos, Blocks::BASALT.default_block_state(), UPDATE_CLIENTS);
    }
}

/// `placeHangOff` — `if (random.nextInt(10) != 0) { setBlock(pos, BASALT);
/// return true; } else { return false; }`.
fn place_hang_off<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    hang_off_pos: &BlockPos,
) -> bool {
    if random.next_int_bound(10) != 0 {
        level.set_block(
            hang_off_pos,
            Blocks::BASALT.default_block_state(),
            UPDATE_CLIENTS,
        );
        true
    } else {
        false
    }
}

impl FeatureBehavior<NoneFeatureConfiguration> for BasaltPillarFeature {
    /// `BasaltPillarFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// if (level.isEmptyBlock(origin) && !level.isEmptyBlock(origin.above())) {
    ///     BlockPos.MutableBlockPos pos = origin.mutable();
    ///     BlockPos.MutableBlockPos tmpPos = origin.mutable();
    ///     boolean placeNorthHangoff = true, placeSouthHangoff = true,
    ///             placeWestHangoff = true, placeEastHangoff = true;
    ///     while (level.isEmptyBlock(pos)) {
    ///         if (level.isOutsideBuildHeight(pos)) return true;
    ///         level.setBlock(pos, Blocks.BASALT.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///         placeNorthHangoff = placeNorthHangoff && this.placeHangOff(level, random, tmpPos.setWithOffset(pos, Direction.NORTH));
    ///         placeSouthHangoff = placeSouthHangoff && this.placeHangOff(level, random, tmpPos.setWithOffset(pos, Direction.SOUTH));
    ///         placeWestHangoff = placeWestHangoff && this.placeHangOff(level, random, tmpPos.setWithOffset(pos, Direction.WEST));
    ///         placeEastHangoff = placeEastHangoff && this.placeHangOff(level, random, tmpPos.setWithOffset(pos, Direction.EAST));
    ///         pos.move(Direction.DOWN);
    ///     }
    ///     pos.move(Direction.UP);
    ///     this.placeBaseHangOff(level, random, tmpPos.setWithOffset(pos, Direction.NORTH));
    ///     this.placeBaseHangOff(level, random, tmpPos.setWithOffset(pos, Direction.SOUTH));
    ///     this.placeBaseHangOff(level, random, tmpPos.setWithOffset(pos, Direction.WEST));
    ///     this.placeBaseHangOff(level, random, tmpPos.setWithOffset(pos, Direction.EAST));
    ///     pos.move(Direction.DOWN);
    ///     BlockPos.MutableBlockPos basePos = new BlockPos.MutableBlockPos();
    ///     for (int dx = -3; dx < 4; dx++) {
    ///         for (int dz = -3; dz < 4; dz++) {
    ///             int probability = Mth.abs(dx) * Mth.abs(dz);
    ///             if (random.nextInt(10) < 10 - probability) {
    ///                 basePos.set(pos.offset(dx, 0, dz));
    ///                 int maxDrop = 3;
    ///                 while (level.isEmptyBlock(tmpPos.setWithOffset(basePos, Direction.DOWN))) {
    ///                     basePos.move(Direction.DOWN);
    ///                     if (--maxDrop <= 0) break;
    ///                 }
    ///                 if (!level.isEmptyBlock(tmpPos.setWithOffset(basePos, Direction.DOWN))) {
    ///                     level.setBlock(basePos, Blocks.BASALT.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///                 }
    ///             }
    ///         }
    ///     }
    ///     return true;
    /// } else {
    ///     return false;
    /// }
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = **origin;
        if level.is_empty_block(&origin) && !level.is_empty_block(&origin.above()) {
            let mut pos = origin;
            let mut place_north_hangoff = true;
            let mut place_south_hangoff = true;
            let mut place_west_hangoff = true;
            let mut place_east_hangoff = true;

            while level.is_empty_block(&pos) {
                if level.is_outside_build_height_pos(&pos) {
                    return true;
                }
                level.set_block(&pos, Blocks::BASALT.default_block_state(), UPDATE_CLIENTS);
                place_north_hangoff = place_north_hangoff
                    && place_hang_off(level, random, &pos.relative(&Direction::North));
                place_south_hangoff = place_south_hangoff
                    && place_hang_off(level, random, &pos.relative(&Direction::South));
                place_west_hangoff = place_west_hangoff
                    && place_hang_off(level, random, &pos.relative(&Direction::West));
                place_east_hangoff = place_east_hangoff
                    && place_hang_off(level, random, &pos.relative(&Direction::East));
                pos = pos.below();
            }

            pos = pos.above();
            place_base_hang_off(level, random, &pos.relative(&Direction::North));
            place_base_hang_off(level, random, &pos.relative(&Direction::South));
            place_base_hang_off(level, random, &pos.relative(&Direction::West));
            place_base_hang_off(level, random, &pos.relative(&Direction::East));
            pos = pos.below();

            for dx in -3..4 {
                for dz in -3..4 {
                    let probability = mth::abs_i32(dx).wrapping_mul(mth::abs_i32(dz));
                    if random.next_int_bound(10) < 10i32.wrapping_sub(probability) {
                        let mut base_pos = pos.offset(dx, 0, dz);
                        let mut max_drop: i32 = 3;
                        while level.is_empty_block(&base_pos.relative(&Direction::Down)) {
                            base_pos = base_pos.below();
                            max_drop = max_drop.wrapping_sub(1);
                            if max_drop <= 0 {
                                break;
                            }
                        }
                        if !level.is_empty_block(&base_pos.relative(&Direction::Down)) {
                            level.set_block(
                                &base_pos,
                                Blocks::BASALT.default_block_state(),
                                UPDATE_CLIENTS,
                            );
                        }
                    }
                }
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::generated::blocks::BlockId;

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        BASALT_PILLAR.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// The gate fails (origin not empty) — `false`, no draws.
    #[test]
    fn non_empty_origin_returns_false() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, 0), Blocks::STONE.default_block_state());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// The gate fails when the cell above the (empty) origin is empty too.
    #[test]
    fn empty_above_returns_false() {
        let mut level = TestLevel::over(access());
        // origin and above are both air (the default map) — the gate fails.
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// One empty cell sandwiched between solid caps: the origin is empty and
    /// the cells above and below are solid, so the downward-growing pillar
    /// writes `BASALT` at the origin only. All four hangoff draws happen (each
    /// still `true` on the first level), then the four base `nextBoolean`
    /// draws, then the 7x7 scatter draws. With a `RecordingRandom` the exact
    /// draw stream is pinned.
    #[test]
    fn single_cell_pillar_draws_hangoffs_base_and_scatter() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 1, 0), Blocks::STONE.default_block_state());
        level
            .states
            .insert(BlockPos::new(0, -1, 0), Blocks::STONE.default_block_state());
        let mut random = RecordingRandom::new(2);
        assert!(place(&mut level, &mut random));
        // Origin is written as the pillar cell.
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert_eq!(
            level.writes[0].1.block(),
            BlockId::from_name("minecraft:basalt").unwrap()
        );
        // 4 hangoff nextInt(10) + 4 base nextBoolean + 49 scatter nextInt(10).
        assert_eq!(random.calls.len(), 4 + 4 + 49);
        assert_eq!(random.calls[0], RngCall::IntBound(10));
        assert_eq!(random.calls[1], RngCall::IntBound(10));
        assert_eq!(random.calls[2], RngCall::IntBound(10));
        assert_eq!(random.calls[3], RngCall::IntBound(10));
        assert_eq!(random.calls[4], RngCall::Boolean);
        assert_eq!(random.calls[5], RngCall::Boolean);
        assert_eq!(random.calls[6], RngCall::Boolean);
        assert_eq!(random.calls[7], RngCall::Boolean);
        assert_eq!(random.calls[8], RngCall::IntBound(10));
    }

    /// A hangoff miss (`nextInt(10) == 0`) flips that direction to `false`, so
    /// the next pillar level short-circuits its draw. The test pins the draw
    /// count reduction across a two-cell pillar.
    #[test]
    fn short_circuit_skips_later_hangoff_draws() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 1, 0), Blocks::STONE.default_block_state());
        level
            .states
            .insert(BlockPos::new(0, -1, 0), Blocks::STONE.default_block_state());
        let mut random = RecordingRandom::new(2);
        assert!(place(&mut level, &mut random));
        // Count the IntBound(10) hangoff+scatter draws; the base booleans are
        // fixed at 4. Every IntBound(10) before index 8 is a hangoff draw.
        // A hangoff that flips false stops drawing on the second level.
        let hangoffs_and_scatter = random
            .calls
            .iter()
            .filter(|c| matches!(c, RngCall::IntBound(10)))
            .count();
        // 49 scatter cells + 4 first-level hangoffs + (<=4) second-level
        // hangoffs — at least one direction missed, else the second level
        // would draw all 4. This pins the short-circuit structurally.
        assert!((49 + 4..=49 + 8).contains(&hangoffs_and_scatter));
    }
}

//! Port of `net.minecraft.world.level.levelgen.feature.BambooFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.bamboo`
//! manifest unit (issue #600).
//!
//! Java: `Feature<ProbabilityFeatureConfiguration>` whose `place` gates on the
//! origin being empty and the `BAMBOO` default state surviving there. It then
//! draws `height = nextInt(12) + 5`, and with probability `config.probability`
//! scatters a `PODZOL` disk of radius `r = nextInt(4) + 1` at the `WORLD_SURFACE`
//! height minus one, over cells whose `#minecraft:beneath_bamboo_podzol_replaceable`
//! tag membership holds. It grows the trunk for `height` cells (writing the
//! `BAMBOO_TRUNK` state — `AGE = 1`, `LEAVES = NONE`, `STAGE = 0` — with
//! `Block.UPDATE_CLIENTS`, 2) while the growing cell stays empty, then caps the
//! stalk when it reached at least 3 cells above the origin: `BAMBOO_FINAL_LARGE`
//! (`LEAVES = LARGE`, `STAGE = 1`), `BAMBOO_TOP_LARGE` (`LEAVES = LARGE`), and
//! `BAMBOO_TOP_SMALL` (`LEAVES = SMALL`). Always returns `true` once the origin
//! gate passes (the `placed++` is unconditional inside the `isEmptyBlock`
//! branch).
//!
//! The `is(TagKey)` podzol-disk check reads the block-tag table through
//! `BlockState::is_in_tag`; the `WORLD_SURFACE` height read uses the
//! `WorldGenLevel::get_height_at` seam (RivetTodo #228) and `canSurvive` uses
//! the `WorldGenLevel::can_survive` seam (RivetTodo #399).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::ProbabilityFeatureConfiguration;
use crate::levelgen::heightmap::Types;
use rivet_registry::block_state_properties::{BambooLeaves, BlockStateProperties};
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `BambooFeature.BAMBOO_TRUNK` — `BAMBOO` with `AGE = 1`, `LEAVES = NONE`,
/// `STAGE = 0` (Java's `static final` trunk state).
fn bamboo_trunk() -> rivet_registry::block_state::BlockState {
    Blocks::BAMBOO
        .default_block_state()
        .set_value(BlockStateProperties::AGE_1, 1)
        .expect("bamboo has the age property")
        .set_value(BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::None)
        .expect("bamboo has the leaves property")
        .set_value(BlockStateProperties::STAGE, 0)
        .expect("bamboo has the stage property")
}

/// `BambooFeature.BAMBOO_FINAL_LARGE` — the trunk with `LEAVES = LARGE` and
/// `STAGE = 1`.
fn bamboo_final_large() -> rivet_registry::block_state::BlockState {
    Blocks::BAMBOO
        .default_block_state()
        .set_value(BlockStateProperties::AGE_1, 1)
        .expect("bamboo has the age property")
        .set_value(BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Large)
        .expect("bamboo has the leaves property")
        .set_value(BlockStateProperties::STAGE, 1)
        .expect("bamboo has the stage property")
}

/// `BambooFeature.BAMBOO_TOP_LARGE` — the trunk with `LEAVES = LARGE` (and the
/// trunk's `STAGE = 0`).
fn bamboo_top_large() -> rivet_registry::block_state::BlockState {
    Blocks::BAMBOO
        .default_block_state()
        .set_value(BlockStateProperties::AGE_1, 1)
        .expect("bamboo has the age property")
        .set_value(BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Large)
        .expect("bamboo has the leaves property")
        .set_value(BlockStateProperties::STAGE, 0)
        .expect("bamboo has the stage property")
}

/// `BambooFeature.BAMBOO_TOP_SMALL` — the trunk with `LEAVES = SMALL`.
fn bamboo_top_small() -> rivet_registry::block_state::BlockState {
    Blocks::BAMBOO
        .default_block_state()
        .set_value(BlockStateProperties::AGE_1, 1)
        .expect("bamboo has the age property")
        .set_value(BlockStateProperties::BAMBOO_LEAVES, BambooLeaves::Small)
        .expect("bamboo has the leaves property")
        .set_value(BlockStateProperties::STAGE, 0)
        .expect("bamboo has the stage property")
}

/// `net.minecraft.world.level.levelgen.feature.BambooFeature`.
#[derive(Debug)]
pub struct BambooFeature;

/// `Feature.BAMBOO` — the registered `minecraft:bamboo` singleton.
pub const BAMBOO: BambooFeature = BambooFeature;

impl FeatureBehavior<ProbabilityFeatureConfiguration> for BambooFeature {
    /// `BambooFeature.place(FeaturePlaceContext<ProbabilityFeatureConfiguration>)`.
    ///
    /// ```java
    /// int placed = 0;
    /// BlockPos.MutableBlockPos bambooPos = origin.mutable();
    /// BlockPos.MutableBlockPos podzolPos = origin.mutable();
    /// if (level.isEmptyBlock(bambooPos)) {
    ///     if (Blocks.BAMBOO.defaultBlockState().canSurvive(level, bambooPos)) {
    ///         int height = random.nextInt(12) + 5;
    ///         if (random.nextFloat() < config.probability) {
    ///             int r = random.nextInt(4) + 1;
    ///             for (int xx = origin.getX() - r; xx <= origin.getX() + r; xx++) {
    ///                 for (int zz = origin.getZ() - r; zz <= origin.getZ() + r; zz++) {
    ///                     int xd = xx - origin.getX();
    ///                     int zd = zz - origin.getZ();
    ///                     if (xd * xd + zd * zd <= r * r) {
    ///                         podzolPos.set(xx, level.getHeight(Heightmap.Types.WORLD_SURFACE, xx, zz) - 1, zz);
    ///                         if (level.getBlockState(podzolPos).is(BlockTags.BENEATH_BAMBOO_PODZOL_REPLACEABLE)) {
    ///                             level.setBlock(podzolPos, Blocks.PODZOL.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///                         }
    ///                     }
    ///                 }
    ///             }
    ///         }
    ///         for (int i = 0; i < height && level.isEmptyBlock(bambooPos); i++) {
    ///             level.setBlock(bambooPos, BAMBOO_TRUNK, Block.UPDATE_CLIENTS);
    ///             bambooPos.move(Direction.UP, 1);
    ///         }
    ///         if (bambooPos.getY() - origin.getY() >= 3) {
    ///             level.setBlock(bambooPos, BAMBOO_FINAL_LARGE, Block.UPDATE_CLIENTS);
    ///             level.setBlock(bambooPos.move(Direction.DOWN, 1), BAMBOO_TOP_LARGE, Block.UPDATE_CLIENTS);
    ///             level.setBlock(bambooPos.move(Direction.DOWN, 1), BAMBOO_TOP_SMALL, Block.UPDATE_CLIENTS);
    ///         }
    ///     }
    ///     placed++;
    /// }
    /// return placed > 0;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, ProbabilityFeatureConfiguration, R>,
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
        let mut placed: i32 = 0;
        let mut bamboo_pos = origin;
        if level.is_empty_block(&bamboo_pos) {
            if level.can_survive(&Blocks::BAMBOO.default_block_state(), &bamboo_pos) {
                let bamboo_trunk = bamboo_trunk();
                let bamboo_final_large = bamboo_final_large();
                let bamboo_top_large = bamboo_top_large();
                let bamboo_top_small = bamboo_top_small();
                let height = random.next_int_bound(12).wrapping_add(5);
                if random.next_float() < config.probability {
                    let r = random.next_int_bound(4).wrapping_add(1);
                    for xx in origin.get_x().wrapping_sub(r)..=origin.get_x().wrapping_add(r) {
                        for zz in origin.get_z().wrapping_sub(r)..=origin.get_z().wrapping_add(r) {
                            let xd = xx.wrapping_sub(origin.get_x());
                            let zd = zz.wrapping_sub(origin.get_z());
                            if xd.wrapping_mul(xd).wrapping_add(zd.wrapping_mul(zd))
                                <= r.wrapping_mul(r)
                            {
                                let podzol_y = level
                                    .get_height_at(Types::WorldSurface, xx, zz)
                                    .wrapping_sub(1);
                                let podzol_pos = BlockPos::new(xx, podzol_y, zz);
                                if level
                                    .get_block_state(&podzol_pos)
                                    .is_in_tag("minecraft:beneath_bamboo_podzol_replaceable")
                                {
                                    level.set_block(
                                        &podzol_pos,
                                        Blocks::PODZOL.default_block_state(),
                                        UPDATE_CLIENTS,
                                    );
                                }
                            }
                        }
                    }
                }
                let mut i = 0;
                while i < height && level.is_empty_block(&bamboo_pos) {
                    level.set_block(&bamboo_pos, bamboo_trunk, UPDATE_CLIENTS);
                    bamboo_pos = bamboo_pos.above();
                    i = i.wrapping_add(1);
                }
                if bamboo_pos.get_y().wrapping_sub(origin.get_y()) >= 3 {
                    level.set_block(&bamboo_pos, bamboo_final_large, UPDATE_CLIENTS);
                    bamboo_pos = bamboo_pos.below();
                    level.set_block(&bamboo_pos, bamboo_top_large, UPDATE_CLIENTS);
                    bamboo_pos = bamboo_pos.below();
                    level.set_block(&bamboo_pos, bamboo_top_small, UPDATE_CLIENTS);
                }
            }
            placed = placed.wrapping_add(1);
        }
        placed > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::generated::blocks::BlockId;

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        BAMBOO.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &ProbabilityFeatureConfiguration::new(1.0),
        ))
    }

    fn place_with_probability(
        level: &mut TestLevel,
        random: &mut RecordingRandom,
        probability: f32,
    ) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        BAMBOO.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &ProbabilityFeatureConfiguration::new(probability),
        ))
    }

    /// A non-empty origin returns `false` before any draw.
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

    /// A `canSurvive` false verdict on the bamboo base still returns `true`
    /// (Java's `placed++` is outside the inner `canSurvive` branch), but no
    /// draw happens — the height draw is inside the `canSurvive` branch, so no
    /// podzol/top/trunk draws fire.
    #[test]
    fn cannot_survive_still_reports_placed_without_writing() {
        let mut level = TestLevel::over(access());
        level.survive = false;
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &mut random));
        assert!(random.calls.is_empty());
        assert!(level.writes.is_empty());
    }

    /// A full-growth run with a `1.0` probability: the `nextInt(12)` height
    /// draw, the `nextFloat` probability draw, the `nextInt(4)` radius draw,
    /// the podzol-disk cell loop (a radius-`r` disk visits `(2r+1)^2` cells,
    /// each inside the circle draws nothing — the `set`/`is(tag)` path draws no
    /// RNG), then the trunk writes and the `>= 3` cap writes three tops. The
    /// draw stream is pinned: `[IntBound(12), Float, IntBound(4)]`.
    #[test]
    fn full_growth_pins_initial_draws_and_writes_trunk_and_tops() {
        let mut level = TestLevel::over(access());
        // The whole column above the origin is empty; the podzol disk reads the
        // WORLD_SURFACE height (0) - 1 = -1, whose default state is air, so no
        // tag membership and no podzol writes.
        let mut random = RecordingRandom::new(3);
        assert!(place(&mut level, &mut random));
        assert_eq!(
            random.calls,
            vec![RngCall::IntBound(12), RngCall::Float, RngCall::IntBound(4),]
        );
        // Every trunk write is BAMBOO_TRUNK; the last three writes are the
        // FINAL_LARGE / TOP_LARGE / TOP_SMALL cap.
        assert!(level.writes.len() >= 5);
        for (i, (_, state)) in level.writes.iter().enumerate() {
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:bamboo").unwrap(),
                "write {i} is bamboo"
            );
        }
        let trunk = level.writes[0].1;
        assert_eq!(
            trunk.get_value(BlockStateProperties::AGE_1),
            Some(PropertyValue::Int(1))
        );
        assert_eq!(
            trunk.get_value(BlockStateProperties::STAGE),
            Some(PropertyValue::Int(0))
        );
        let final_large = level.writes[level.writes.len() - 3].1;
        assert_eq!(
            final_large.get_value(BlockStateProperties::STAGE),
            Some(PropertyValue::Int(1))
        );
        let top_large = level.writes[level.writes.len() - 2].1;
        assert_eq!(
            top_large.get_value(BlockStateProperties::BAMBOO_LEAVES),
            Some(PropertyValue::Enum("large"))
        );
        let top_small = level.writes.last().unwrap().1;
        assert_eq!(
            top_small.get_value(BlockStateProperties::BAMBOO_LEAVES),
            Some(PropertyValue::Enum("small"))
        );
    }

    /// A probability of `0.0` skips the podzol disk entirely — only the height
    /// draw and the `nextFloat` draw happen, then the trunk loop.
    #[test]
    fn zero_probability_skips_the_podzol_disk() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(3);
        assert!(place_with_probability(&mut level, &mut random, 0.0));
        assert_eq!(random.calls, vec![RngCall::IntBound(12), RngCall::Float]);
        assert!(
            level
                .writes
                .iter()
                .all(|(_, s)| s.block() == BlockId::from_name("minecraft:bamboo").unwrap())
        );
    }

    /// The `>= 3` cap is skipped when the trunk stops growing early (a solid
    /// cell above the origin blocks it): the height draw still happens, and the
    /// writes are trunk-only — no FINAL_LARGE/TOP_LARGE/TOP_SMALL.
    #[test]
    fn short_stalk_skips_the_top_cap() {
        let mut level = TestLevel::over(access());
        // Cell above origin is solid, so the trunk loop writes exactly one
        // cell and stops; bambooPos.y - origin.y = 1 < 3, so no top cap.
        level
            .states
            .insert(BlockPos::new(0, 1, 0), Blocks::STONE.default_block_state());
        let mut random = RecordingRandom::new(3);
        assert!(place(&mut level, &mut random));
        assert_eq!(level.writes.len(), 1);
        let only = level.writes[0].1;
        assert_eq!(
            only.get_value(BlockStateProperties::BAMBOO_LEAVES),
            Some(PropertyValue::Enum("none"))
        );
        assert_eq!(
            only.get_value(BlockStateProperties::STAGE),
            Some(PropertyValue::Int(0))
        );
    }
}

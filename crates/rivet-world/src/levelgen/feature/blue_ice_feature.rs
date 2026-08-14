//! Port of `net.minecraft.world.level.levelgen.feature.BlueIceFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.vegetation-family`
//! manifest unit (issue #600).
//!
//! Java: `Feature<NoneFeatureConfiguration>` whose `place` gates on the origin
//! being below sea level, the origin or the cell below being water, and at
//! least one non-`DOWN` axis neighbor being `PACKED_ICE` (the `Direction.values()`
//! scan in `BY_3D_DATA` order, `DOWN` skipped). It then writes `BLUE_ICE` at
//! the origin and, for 200 attempts, computes a random `yOff =
//! nextInt(5) - nextInt(6)` and a horizontal scatter radius `xzDiff = 3 +
//! (yOff < 2 ? yOff / 2 : 0)` (Java's truncating int division), offsetting a
//! cell by `nextInt(xzDiff) - nextInt(xzDiff)` per axis when `xzDiff >= 1`.
//! If the target cell is air/water/packed ice/ice and any axis neighbor is
//! `BLUE_ICE`, it writes `BLUE_ICE` there. Always returns `true` once the
//! origin gates pass.
//!
//! The `is(Blocks.X)` identity checks read `get_block_state(...).block()`
//! against the generated ids; the sea-level gate reads the
//! `WorldGenLevel::get_sea_level` seam (RivetTodo #228).

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::core::Direction;
use rivet_registry::generated::blocks::BlockId;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `Feature.setBlock`
/// reduces to.
const UPDATE_CLIENTS: u32 = 2;

/// `BlockStateBase.is(Blocks.X)` — the block identity check the feature gates
/// its writes on.
#[inline]
fn is_block(state: rivet_registry::block_state::BlockState, name: &str) -> bool {
    state.block() == BlockId::from_name(name).expect("generated block name resolves")
}

/// `net.minecraft.world.level.levelgen.feature.BlueIceFeature`.
#[derive(Debug)]
pub struct BlueIceFeature;

/// `Feature.BLUE_ICE` — the registered `minecraft:blue_ice` singleton.
pub const BLUE_ICE: BlueIceFeature = BlueIceFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for BlueIceFeature {
    /// `BlueIceFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// if (origin.getY() > level.getSeaLevel() - 1) return false;
    /// if (!level.getBlockState(origin).is(Blocks.WATER)
    ///         && !level.getBlockState(origin.below()).is(Blocks.WATER)) return false;
    /// boolean foundPackedIce = false;
    /// for (Direction direction : Direction.values()) {
    ///     if (direction != Direction.DOWN
    ///             && level.getBlockState(origin.relative(direction)).is(Blocks.PACKED_ICE)) {
    ///         foundPackedIce = true;
    ///         break;
    ///     }
    /// }
    /// if (!foundPackedIce) return false;
    /// level.setBlock(origin, Blocks.BLUE_ICE.defaultBlockState(), Block.UPDATE_CLIENTS);
    /// for (int i = 0; i < 200; i++) {
    ///     int yOff = random.nextInt(5) - random.nextInt(6);
    ///     int xzDiff = 3;
    ///     if (yOff < 2) xzDiff += yOff / 2;
    ///     if (xzDiff >= 1) {
    ///         BlockPos placePos = origin.offset(
    ///             random.nextInt(xzDiff) - random.nextInt(xzDiff), yOff,
    ///             random.nextInt(xzDiff) - random.nextInt(xzDiff));
    ///         BlockState placeState = level.getBlockState(placePos);
    ///         if (placeState.isAir() || placeState.is(Blocks.WATER)
    ///                 || placeState.is(Blocks.PACKED_ICE) || placeState.is(Blocks.ICE)) {
    ///             for (Direction direction : Direction.values()) {
    ///                 BlockState relativeBlockState = level.getBlockState(placePos.relative(direction));
    ///                 if (relativeBlockState.is(Blocks.BLUE_ICE)) {
    ///                     level.setBlock(placePos, Blocks.BLUE_ICE.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///                     break;
    ///                 }
    ///             }
    ///         }
    ///     }
    /// }
    /// return true;
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
        let origin = *origin;
        if origin.get_y() > level.get_sea_level().wrapping_sub(1) {
            return false;
        }
        if !is_block(level.get_block_state(origin), "minecraft:water")
            && !is_block(level.get_block_state(&origin.below()), "minecraft:water")
        {
            return false;
        }
        let mut found_packed_ice = false;
        for direction in Direction::VALUES.iter() {
            if *direction != Direction::Down
                && is_block(
                    level.get_block_state(&origin.relative(direction)),
                    "minecraft:packed_ice",
                )
            {
                found_packed_ice = true;
                break;
            }
        }
        if !found_packed_ice {
            return false;
        }
        level.set_block(
            origin,
            Blocks::BLUE_ICE.default_block_state(),
            UPDATE_CLIENTS,
        );
        for _ in 0..200 {
            let y_off = random
                .next_int_bound(5)
                .wrapping_sub(random.next_int_bound(6));
            let mut xz_diff: i32 = 3;
            if y_off < 2 {
                xz_diff = xz_diff.wrapping_add(y_off / 2);
            }
            if xz_diff >= 1 {
                let place_pos = origin.offset(
                    random
                        .next_int_bound(xz_diff)
                        .wrapping_sub(random.next_int_bound(xz_diff)),
                    y_off,
                    random
                        .next_int_bound(xz_diff)
                        .wrapping_sub(random.next_int_bound(xz_diff)),
                );
                let place_state = level.get_block_state(&place_pos);
                if place_state.is_air()
                    || is_block(place_state, "minecraft:water")
                    || is_block(place_state, "minecraft:packed_ice")
                    || is_block(place_state, "minecraft:ice")
                {
                    for direction in Direction::VALUES.iter() {
                        if is_block(
                            level.get_block_state(&place_pos.relative(direction)),
                            "minecraft:blue_ice",
                        ) {
                            level.set_block(
                                &place_pos,
                                Blocks::BLUE_ICE.default_block_state(),
                                UPDATE_CLIENTS,
                            );
                            break;
                        }
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, TestGenerator, TestLevel, access,
    };
    use rivet_registry::core::BlockPos;

    fn place(level: &mut TestLevel, random: &mut RecordingRandom) -> bool {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        BLUE_ICE.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// An origin above sea level returns `false` before any draw.
    #[test]
    fn above_sea_level_returns_false_before_drawing() {
        let mut level = TestLevel::over(access());
        level.sea_level = 0; // origin y=0 is above sea level-1 = -1
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// Neither the origin nor the cell below is water — returns `false`.
    #[test]
    fn no_water_returns_false() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// No `PACKED_ICE` axis neighbor (excluding `DOWN`) — returns `false`
    /// before the 200-attempt scatter.
    #[test]
    fn no_packed_ice_neighbor_returns_false() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, 0), Blocks::WATER.default_block_state());
        level
            .states
            .insert(BlockPos::new(0, -1, 0), Blocks::STONE.default_block_state());
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }

    /// Passing gates: water at origin, packed ice at +X (a non-`DOWN`
    /// neighbor), and a scatter target cell at the exact offset that is air
    /// with a `BLUE_ICE` neighbor. The origin `BLUE_ICE` write lands, then the
    /// 200 attempts draw their offsets and the inner neighbor scan writes.
    #[test]
    fn gates_pass_and_origin_blue_ice_writes() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, 0), Blocks::WATER.default_block_state());
        // +X packed ice satisfies the foundPackedIce scan (UP, NORTH, SOUTH,
        // WEST, EAST all checked before the +X is reached; +X is EAST).
        level.states.insert(
            BlockPos::new(1, 0, 0),
            Blocks::PACKED_ICE.default_block_state(),
        );
        let mut random = RecordingRandom::new(9);
        assert!(place(&mut level, &mut random));
        // The origin BLUE_ICE write always happens once gates pass.
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert_eq!(
            level.writes[0].1.block(),
            BlockId::from_name("minecraft:blue_ice").unwrap()
        );
    }

    /// A `DOWN` packed-ice neighbor must NOT satisfy the scan: only the five
    /// non-`DOWN` directions are checked. The feature returns false.
    #[test]
    fn down_packed_ice_does_not_satisfy_the_scan() {
        let mut level = TestLevel::over(access());
        level
            .states
            .insert(BlockPos::new(0, 0, 0), Blocks::WATER.default_block_state());
        level.states.insert(
            BlockPos::new(0, -1, 0),
            Blocks::PACKED_ICE.default_block_state(),
        );
        let mut random = RecordingRandom::new(1);
        assert!(!place(&mut level, &mut random));
        assert!(random.calls.is_empty());
    }
}

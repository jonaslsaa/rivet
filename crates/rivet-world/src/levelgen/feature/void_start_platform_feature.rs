//! Port of `net.minecraft.world.level.levelgen.feature.VoidStartPlatformFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.voidstartplatform`
//! manifest unit (the end-leaves wave).
//!
//! Java: `Feature<NoneFeatureConfiguration>` that stamps a `COBBLESTONE`
//! center cell with a 16-block `STONE` radius into the void dimension's spawn
//! chunk. `place` gates on the origin's chunk being within one chunk (Chebyshev
//! distance) of `PLATFORM_ORIGIN_CHUNK` (`ChunkPos.containing(PLATFORM_OFFSET)`),
//! else returns `true` without writing. The platform center is
//! `PLATFORM_OFFSET.atY(origin.y + PLATFORM_OFFSET.y)` (`(8, y+3, 8)`); each
//! chunk cell whose Chebyshev distance to that center is `<= 16` is written
//! `COBBLESTONE` at the exact center and `STONE` elsewhere, all with
//! `Block.UPDATE_CLIENTS` (2). Always returns `true` once the gate passes.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::core::BlockPos;
use rivet_registry::core::ChunkPos;
use rivet_util::RandomSource;
use rivet_util::mth;

/// `VoidStartPlatformFeature.PLATFORM_OFFSET`.
pub const PLATFORM_OFFSET: BlockPos = BlockPos::new(8, 3, 8);
/// `VoidStartPlatformFeature.PLATFORM_ORIGIN_CHUNK` — `ChunkPos.containing(
/// PLATFORM_OFFSET)`; `SectionPos.blockToSectionCoord` is `blockCoord >> 4` (an
/// arithmetic shift that floors toward negative infinity), so `(8, 3, 8)` lands
/// in chunk `(0, 0)` (written out because `ChunkPos::containing` is not a const
/// fn).
pub const PLATFORM_ORIGIN_CHUNK: ChunkPos = ChunkPos::ZERO;
/// `VoidStartPlatformFeature.PLATFORM_RADIUS`.
pub const PLATFORM_RADIUS: i32 = 16;
/// `VoidStartPlatformFeature.PLATFORM_RADIUS_CHUNKS`.
pub const PLATFORM_RADIUS_CHUNKS: i32 = 1;

/// `VoidStartPlatformFeature.checkerboardDistance(int, int, int, int)` — the
/// Chebyshev distance `max(abs(xa - xb), abs(za - zb))`.
fn checkerboard_distance(xa: i32, za: i32, xb: i32, zb: i32) -> i32 {
    mth::abs_i32(xa.wrapping_sub(xb)).max(mth::abs_i32(za.wrapping_sub(zb)))
}

/// `net.minecraft.world.level.levelgen.feature.VoidStartPlatformFeature`.
#[derive(Debug)]
pub struct VoidStartPlatformFeature;

/// `Feature.VOID_START_PLATFORM` — the registered
/// `minecraft:void_start_platform` singleton (the feature registry's insertion
/// index 7).
pub const VOID_START_PLATFORM: VoidStartPlatformFeature = VoidStartPlatformFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for VoidStartPlatformFeature {
    /// `VoidStartPlatformFeature.place(FeaturePlaceContext<
    /// NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// ChunkPos currentChunkPos = ChunkPos.containing(context.origin());
    /// if (checkerboardDistance(currentChunkPos.x(), currentChunkPos.z(), PLATFORM_ORIGIN_CHUNK.x(), PLATFORM_ORIGIN_CHUNK.z()) > 1) {
    ///     return true;
    /// }
    /// BlockPos platformOrigin = PLATFORM_OFFSET.atY(context.origin().getY() + PLATFORM_OFFSET.getY());
    /// for (int z = currentChunkPos.getMinBlockZ(); z <= currentChunkPos.getMaxBlockZ(); z++) {
    ///     for (int x = currentChunkPos.getMinBlockX(); x <= currentChunkPos.getMaxBlockX(); x++) {
    ///         if (checkerboardDistance(platformOrigin.getX(), platformOrigin.getZ(), x, z) <= 16) {
    ///             blockPos.set(x, platformOrigin.getY(), z);
    ///             if (blockPos.equals(platformOrigin)) {
    ///                 level.setBlock(blockPos, Blocks.COBBLESTONE.defaultBlockState(), Block.UPDATE_CLIENTS);
    ///             } else {
    ///                 level.setBlock(blockPos, Blocks.STONE.defaultBlockState(), Block.UPDATE_CLIENTS);
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
        let FeaturePlaceContext { level, origin, .. } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let origin = *origin;
        let current_chunk_pos = ChunkPos::containing(origin);
        if checkerboard_distance(
            current_chunk_pos.x(),
            current_chunk_pos.z(),
            PLATFORM_ORIGIN_CHUNK.x(),
            PLATFORM_ORIGIN_CHUNK.z(),
        ) > 1
        {
            return true;
        }
        let platform_origin =
            PLATFORM_OFFSET.at_y(origin.get_y().wrapping_add(PLATFORM_OFFSET.get_y()));
        let mut block_pos = BlockPos::ZERO.mutable();
        for z in current_chunk_pos.get_min_block_z()..=current_chunk_pos.get_max_block_z() {
            for x in current_chunk_pos.get_min_block_x()..=current_chunk_pos.get_max_block_x() {
                if checkerboard_distance(platform_origin.get_x(), platform_origin.get_z(), x, z)
                    <= PLATFORM_RADIUS
                {
                    block_pos.set(x, platform_origin.get_y(), z);
                    if block_pos.immutable() == platform_origin {
                        level.set_block(
                            &block_pos.immutable(),
                            Blocks::COBBLESTONE.default_block_state(),
                            UPDATE_CLIENTS,
                        );
                    } else {
                        level.set_block(
                            &block_pos.immutable(),
                            Blocks::STONE.default_block_state(),
                            UPDATE_CLIENTS,
                        );
                    }
                }
            }
        }
        true
    }
}

/// `Block.UPDATE_CLIENTS` — the write-flag constant the platform writes use.
const UPDATE_CLIENTS: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;

    fn place(level: &mut TestLevel, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        VOID_START_PLATFORM.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// An origin in the platform chunk (block `(8, 3, 8)` = chunk `(0, 0)`)
    /// writes the whole chunk's 16x16 column at `origin.y + PLATFORM_OFFSET.y`.
    /// Every cell of chunk `(0, 0)` is within Chebyshev radius 16 of the
    /// center `(8, y, 8)` (the farthest corner is 8 away), so all 256 cells
    /// are written exactly once — the center cell `COBBLESTONE`, the other 255
    /// `STONE`, all with `UPDATE_CLIENTS` — and the feature returns `true`.
    #[test]
    fn origin_in_platform_chunk_writes_center_and_radius() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(8, 3, 8);
        let placed = place(&mut level, origin);
        assert!(placed);
        let platform_y = origin.get_y() + PLATFORM_OFFSET.get_y();
        assert_eq!(level.writes.len(), 16 * 16);
        let cobblestone = level
            .writes
            .iter()
            .filter(|(_, s)| s.block() == BlockId::from_name("minecraft:cobblestone").unwrap())
            .count();
        let stone = level
            .writes
            .iter()
            .filter(|(_, s)| s.block() == BlockId::from_name("minecraft:stone").unwrap())
            .count();
        assert_eq!(cobblestone, 1);
        assert_eq!(stone, 16 * 16 - 1);
        for (pos, state) in &level.writes {
            assert_eq!(pos.get_y(), platform_y);
            assert!(pos.get_x() >= 0 && pos.get_x() < 16);
            assert!(pos.get_z() >= 0 && pos.get_z() < 16);
            assert!(
                state.block() == BlockId::from_name("minecraft:cobblestone").unwrap()
                    || state.block() == BlockId::from_name("minecraft:stone").unwrap()
            );
        }
        // The single cobblestone cell is the platform center.
        assert_eq!(
            level.states[&BlockPos::new(8, platform_y, 8)].block(),
            BlockId::from_name("minecraft:cobblestone").unwrap()
        );
        assert_eq!(
            level.states[&BlockPos::new(8, platform_y, 7)].block(),
            BlockId::from_name("minecraft:stone").unwrap()
        );
    }

    /// An origin outside the platform chunk (a chunk > 1 Chebyshev cells away)
    /// returns `true` without writing anything.
    #[test]
    fn origin_far_from_platform_chunk_returns_true_without_writing() {
        let mut level = TestLevel::over(access());
        // Block (40, 0, 40) is chunk (2, 2) — Chebyshev distance 2 from (0, 0).
        let origin = BlockPos::new(40, 0, 40);
        let placed = place(&mut level, origin);
        assert!(placed);
        assert!(level.writes.is_empty());
    }

    /// A hostile origin at negative coordinates in the platform chunk writes
    /// the platform relative to that origin's y.
    #[test]
    fn negative_origin_in_platform_chunk_still_writes() {
        let mut level = TestLevel::over(access());
        // Block (-1, 64, -1) is chunk (-1, -1) (`SectionPos.blockToSectionCoord`
        // is `blockCoord >> 4`, an arithmetic shift that floors toward negative
        // infinity: -1 >> 4 = -1), within Chebyshev distance 1 of the platform
        // chunk (0, 0), so the `> 1` gate does not early-return.
        let origin = BlockPos::new(-1, 64, -1);
        let placed = place(&mut level, origin);
        assert!(placed);
        assert!(!level.writes.is_empty());
        let platform_y = origin.get_y().wrapping_add(PLATFORM_OFFSET.get_y());
        for (pos, _) in &level.writes {
            assert_eq!(pos.get_y(), platform_y);
        }
    }
}

//! Port of `net.minecraft.world.level.levelgen.feature.EndPlatformFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.endplatform`
//! manifest unit (the end-leaves wave).
//!
//! Java: `Feature<NoneFeatureConfiguration>` whose `place` delegates to
//! `createEndPlatform(level, origin, false)` — the 5x5x4 obsidian/air platform
//! the player spawns on in the End. The nested loops iterate `dz` (outer),
//! then `dx`, then `dy` (`-1..=2`); each cell is `OBSIDIAN` at `dy == -1`
//! and `AIR` above, and the cell is written (with `Block.UPDATE_ALL`) only when
//! its current state's block differs. The CraftBukkit `BlockStateListPopulator`
//! defers the writes until `placeBlocks()`, but with `dropResources=false` and
//! `entity=null` (the worldgen path) the deferred write is a plain
//! `level.setBlock(pos, state, UPDATE_ALL)` — the vanilla write order, block
//! states, and flags are what the port preserves. The `@Nullable Entity`
//! parameter is dropped (every call site passes `null`), so
//! `createEndPlatform(level, origin, dropResources)` carries only the
//! `dropResources` flag; the `destroyBlock` call on the `dropResources=true`
//! path reduces to the `WorldGenLevel::destroy_block` seam (RivetTodo #232).
//! `place` itself never destroys (it passes `false`), so the reachable behavior
//! is all writes.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::is_block;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `Block.UPDATE_ALL` — the write-flag constant the platform writes reduce to
/// (`UPDATE_NEIGHBORS | UPDATE_CLIENTS`).
const UPDATE_ALL: u32 = 3;

/// `net.minecraft.world.level.levelgen.feature.EndPlatformFeature`.
#[derive(Debug)]
pub struct EndPlatformFeature;

/// `Feature.END_PLATFORM` — the registered `minecraft:end_platform` singleton
/// (the feature registry's insertion index 29).
pub const END_PLATFORM: EndPlatformFeature = EndPlatformFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for EndPlatformFeature {
    /// `EndPlatformFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`
    /// — `createEndPlatform(context.level(), context.origin(), false)`, then
    /// `true`.
    ///
    /// ```java
    /// createEndPlatform(level, origin, false);
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext { level, origin, .. } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        create_end_platform(level, origin, false);
        true
    }
}

/// `EndPlatformFeature.createEndPlatform(ServerLevelAccessor, BlockPos,
/// boolean)` — the static platform builder. The worldgen `place` calls it with
/// `dropResources=false`, so no cell is destroyed; the `dropResources=true`
/// path (the respawn handler) first destroys the cell before overwriting it.
///
/// ```java
/// for (int dz = -2; dz <= 2; dz++) {
///     for (int dx = -2; dx <= 2; dx++) {
///         for (int dy = -1; dy < 3; dy++) {
///             BlockPos blockPos = pos.set(origin).move(dx, dy, dz);
///             Block block = dy == -1 ? Blocks.OBSIDIAN : Blocks.AIR;
///             if (!blockList.getBlockState(blockPos).is(block)) {
///                 if (dropResources) blockList.destroyBlock(blockPos, true, null);
///                 blockList.setBlock(blockPos, block.defaultBlockState(), Block.UPDATE_ALL);
///             }
///         }
///     }
/// }
/// ```
pub fn create_end_platform(level: &mut dyn WorldGenLevel, origin: &BlockPos, drop_resources: bool) {
    let mut pos = origin.mutable();
    for dz in -2..=2 {
        for dx in -2..=2 {
            for dy in -1..3 {
                pos.set(origin.get_x(), origin.get_y(), origin.get_z())
                    .move_xyz(dx, dy, dz);
                let block = if dy == -1 {
                    Blocks::OBSIDIAN
                } else {
                    Blocks::AIR
                };
                if !is_block(level.get_block_state(&pos.immutable()), block) {
                    if drop_resources {
                        level.destroy_block(&pos.immutable(), true);
                    }
                    level.set_block(&pos.immutable(), block.default_block_state(), UPDATE_ALL);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{TestGenerator, TestLevel, access};
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;

    fn place(level: &mut TestLevel, origin: BlockPos) -> bool {
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        END_PLATFORM.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            &mut random,
            &origin,
            &NoneFeatureConfiguration::INSTANCE,
        ))
    }

    /// On a default all-air `TestLevel` only the 5x5 obsidian floor cells at
    /// `y = origin.y - 1` differ from their target and are written (the
    /// `AIR` cells above already match and are skipped by Java's
    /// `if (!state.is(block))` gate). Each write uses `Block.UPDATE_ALL`, in
    /// `dz`-outer/`dx`/`dy` order. The final world has obsidian at the floor
    /// and air above.
    #[test]
    fn writes_only_the_differing_obsidian_floor_on_an_empty_level() {
        let mut level = TestLevel::over(access());
        let placed = place(&mut level, BlockPos::new(0, 0, 0));
        assert!(placed);
        assert_eq!(level.writes.len(), 25);
        for (pos, state) in &level.writes {
            assert_eq!(pos.get_y(), -1);
            assert_eq!(
                state.block(),
                BlockId::from_name("minecraft:obsidian").unwrap()
            );
        }
        // The final world has obsidian at the floor and air above (the
        // `AIR` cells were never written — they already matched — so they are
        // absent from the map and default to air on the `TestLevel`).
        assert_eq!(
            level.states[&BlockPos::new(0, -1, 0)].block(),
            BlockId::from_name("minecraft:obsidian").unwrap()
        );
        let above = level.states.get(&BlockPos::new(2, 2, -2));
        assert!(above.map(|s| s.is_air()).unwrap_or(true));
    }

    /// A pre-filled stone region exercises the air-writing path: the `AIR`
    /// cells above the floor now differ and are written, alongside the
    /// obsidian floor.
    #[test]
    fn stone_region_gets_air_filled_above_the_floor() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        for x in -2..=2 {
            for z in -2..=2 {
                for y in 0..=2 {
                    level
                        .states
                        .insert(origin.offset(x, y, z), Blocks::STONE.default_block_state());
                }
            }
        }
        let placed = place(&mut level, origin);
        assert!(placed);
        assert_eq!(level.writes.len(), 25 * 4);
        let air = level
            .writes
            .iter()
            .filter(|(p, s)| p.get_y() >= 0 && p.get_y() <= 2 && s.block() == BlockId(0))
            .count();
        assert_eq!(air, 25 * 3);
    }

    /// The platform centers on the origin horizontally: the floor spans
    /// `origin.x - 2 ..= origin.x + 2` on both axes.
    #[test]
    fn platform_is_centered_on_origin() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(8, 3, -12);
        place(&mut level, origin);
        for (pos, state) in &level.writes {
            if state.block() == BlockId::from_name("minecraft:obsidian").unwrap() {
                assert!(pos.get_x() >= origin.get_x() - 2 && pos.get_x() <= origin.get_x() + 2);
                assert!(pos.get_z() >= origin.get_z() - 2 && pos.get_z() <= origin.get_z() + 2);
            }
        }
    }

    /// A hostile partial world: cells already holding the target block are
    /// left untouched (Java's `if (!state.is(block))` gate), so an already-
    /// obsidian floor plus air above is not re-written at all.
    #[test]
    fn cells_already_matching_target_are_not_rewritten() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 0, 0);
        // Pre-fill the whole 5x5 obsidian floor (the only cells that would
        // differ on a default all-air level).
        for x in -2..=2 {
            for z in -2..=2 {
                level.states.insert(
                    origin.offset(x, -1, z),
                    Blocks::OBSIDIAN.default_block_state(),
                );
            }
        }
        let placed = place(&mut level, origin);
        assert!(placed);
        assert!(level.writes.is_empty());
    }
}

//! `net.minecraft.world.level.LevelReader` — read access to the level.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! LevelReader.java`. The #232 value slice ports the self-contained chunk-
//! presence defaults (`hasChunkAt`/`hasChunksAt`) and the value read flags;
//! the block/light/collision super-interfaces, chunk loading, height, biome,
//! brightness and registry-access surfaces defer.

use rivet_registry::core::{BlockPos, SectionPos};

use super::block_getter::BlockGetter;

/// `LevelReader` — the level's read surface.
///
/// RivetTodo(#232): the `BlockAndLightGetter`/`CollisionGetter`/`SignalGetter`
/// and `BiomeManager.NoiseBiomeSource` super-interfaces, `getChunk(...)`,
/// `getHeight(Heightmap.Types, ...)`, `getBiomeManager`/`getUncachedNoiseBiome`,
/// `dimensionType()` (the `getMinY`/`getHeight` indirection it drives),
/// `registryAccess`/`enabledFeatures`/`holderLookup`, `environmentAttributes`
/// and the brightness/light defaults all defer with the full `mc.world.level`
/// unit. Implementations here provide `getMinY`/`getHeight` directly.
pub trait LevelReader: BlockGetter {
    /// `hasChunk(int chunkX, int chunkZ)` (`@Deprecated` in Java).
    fn has_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// `getSkyDarken()`.
    fn get_sky_darken(&self) -> i32;

    /// `isClientSide()`.
    fn is_client_side(&self) -> bool;

    /// `getSeaLevel()`.
    fn get_sea_level(&self) -> i32;

    /// `hasChunkAt(int blockX, int blockZ)` (`@Deprecated`) — the section of
    /// the block coordinates must be loaded.
    fn has_chunk_at(&self, block_x: i32, block_z: i32) -> bool {
        self.has_chunk(
            SectionPos::block_to_section_coord(block_x),
            SectionPos::block_to_section_coord(block_z),
        )
    }

    /// `hasChunkAt(BlockPos)` (`@Deprecated`).
    fn has_chunk_at_pos(&self, pos: &BlockPos) -> bool {
        self.has_chunk_at(pos.get_x(), pos.get_z())
    }

    /// `hasChunksAt(BlockPos, BlockPos)` (`@Deprecated`) — every block-column
    /// section in the inclusive box must be loaded.
    fn has_chunks_at_box(&self, pos0: &BlockPos, pos1: &BlockPos) -> bool {
        self.has_chunks_at(
            pos0.get_x(),
            pos0.get_y(),
            pos0.get_z(),
            pos1.get_x(),
            pos1.get_y(),
            pos1.get_z(),
        )
    }

    /// `hasChunksAt(int x0, int y0, int z0, int x1, int y1, int z1)`
    /// (`@Deprecated`).
    fn has_chunks_at(&self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> bool {
        y1 >= self.get_min_y() && y0 <= self.get_max_y() && self.has_chunks_at_xz(x0, z0, x1, z1)
    }

    /// `hasChunksAt(int x0, int z0, int x1, int z1)` (`@Deprecated`) — every
    /// chunk in the inclusive horizontal span must be loaded.
    fn has_chunks_at_xz(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let chunk_x0 = SectionPos::block_to_section_coord(x0);
        let chunk_x1 = SectionPos::block_to_section_coord(x1);
        let chunk_z0 = SectionPos::block_to_section_coord(z0);
        let chunk_z1 = SectionPos::block_to_section_coord(z1);
        for chunk_x in chunk_x0..=chunk_x1 {
            for chunk_z in chunk_z0..=chunk_z1 {
                if !self.has_chunk(chunk_x, chunk_z) {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::LevelHeightAccessor;

    /// A fake `LevelReader` for testing the value defaults against Java's
    /// section-coordinate logic (no chunk storage involved).
    struct FakeLevel {
        chunks: std::collections::HashSet<(i32, i32)>,
        min_y: i32,
        height: i32,
    }

    impl LevelHeightAccessor for FakeLevel {
        fn get_height(&self) -> i32 {
            self.height
        }

        fn get_min_y(&self) -> i32 {
            self.min_y
        }
    }

    impl BlockGetter for FakeLevel {}

    impl LevelReader for FakeLevel {
        fn has_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
            self.chunks.contains(&(chunk_x, chunk_z))
        }

        fn get_sky_darken(&self) -> i32 {
            0
        }

        fn is_client_side(&self) -> bool {
            false
        }

        fn get_sea_level(&self) -> i32 {
            -63
        }
    }

    fn fake() -> FakeLevel {
        let mut chunks = std::collections::HashSet::new();
        // Chunks (0,0) and (1,0) are loaded; (0,1)/(1,1) are not.
        chunks.insert((0, 0));
        chunks.insert((1, 0));
        FakeLevel {
            chunks,
            min_y: -64,
            height: 384,
        }
    }

    #[test]
    fn has_chunk_at_maps_blocks_to_sections() {
        let level = fake();
        // Blocks [0,15] live in chunk 0; block 16 is the first of chunk 1.
        assert!(level.has_chunk_at(0, 0));
        assert!(level.has_chunk_at(15, 0));
        assert!(level.has_chunk_at(16, 0)); // chunk (1,0) loaded
        assert!(!level.has_chunk_at(0, 16)); // chunk (0,1) not loaded
        // Negative block coords map to negative sections: block -1 is in
        // section -1 (Java `-1 >> 4 == -1`).
        assert!(!level.has_chunk_at(-1, 0));
        assert!(level.has_chunk_at_pos(&BlockPos::new(5, 0, 0)));
    }

    #[test]
    fn has_chunks_at_requires_every_section() {
        let level = fake();
        // Horizontal span covering chunks (0,0) and (1,0) only — all loaded.
        assert!(level.has_chunks_at_xz(0, 0, 31, 15));
        // Span extending into chunk (0,1) — missing.
        assert!(!level.has_chunks_at_xz(0, 0, 31, 31));
    }

    #[test]
    fn has_chunks_at_checks_build_height_first() {
        let level = fake();
        // Below min Y / above max Y short-circuits to false regardless of
        // loaded chunks.
        assert!(!level.has_chunks_at(0, -100, 0, 31, -65, 15));
        assert!(!level.has_chunks_at(0, 320, 0, 31, 400, 15));
        // In-range box over loaded chunks (0,0) and (1,0).
        assert!(level.has_chunks_at(0, -64, 0, 31, 0, 15));
    }

    #[test]
    fn has_chunks_at_exact_build_height_boundaries() {
        let level = fake();
        // Java `hasChunksAt(x0,y0,z0,x1,y1,z1)` requires `y1 >= minY && y0 <=
        // maxY` — both inclusive. A single block exactly at max Y (319) and at
        // min Y (-64) is inside.
        assert!(level.has_chunks_at(0, 319, 0, 0, 319, 0));
        assert!(level.has_chunks_at(0, -64, 0, 0, -64, 0));
        // One block above max Y / below min Y fails.
        assert!(!level.has_chunks_at(0, 320, 0, 0, 320, 0));
        assert!(!level.has_chunks_at(0, -65, 0, 0, -65, 0));
        // Y in range is not enough: the horizontal span must be loaded too.
        // Chunk (0,1) is missing, so a box reaching z=16 fails.
        assert!(!level.has_chunks_at(0, 0, 0, 0, 0, 16));
    }

    #[test]
    fn has_chunks_at_xz_spans_negative_sections() {
        let level = fake();
        // Block x in [-16,-1] maps to section -1 (Java `-16 >> 4 == -1`),
        // which is not loaded — so a span crossing into it fails.
        assert!(!level.has_chunks_at_xz(-16, 0, 15, 0));
        // A span entirely within loaded section 0 passes.
        assert!(level.has_chunks_at_xz(0, 0, 15, 15));
    }
}

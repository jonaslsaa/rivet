//! Port of `net.minecraft.world.level.chunk.EmptyLevelChunk` (MC 26.2) — the
//! unloaded-chunk stand-in.
//!
//! Java: `EmptyLevelChunk.java` in `working/Paper`. `EmptyLevelChunk extends
//! LevelChunk` and overrides the block-state read to `VOID_AIR` everywhere,
//! `getNoiseBiome` to a single `Holder<Biome>`, the mutators to no-ops, and
//! `isEmpty`/`isYSpaceEmpty` to true. Per OWNERSHIP.md there is no inheritance,
//! so this chunk contains a `ChunkAccess` base like [`LevelChunk`][super::level_chunk].
//!
//! Deferred with their owning units: `getFluidState` (the `FluidState` type),
//! `getBlockEntity`/`setBlockEntity`/`addAndRegisterBlockEntity`/
//! `removeBlockEntity` (the block-entity unit), and the Starlight light
//! nibble/emptiness overrides (the lighting engine unit #184). Java's
//! constructor primes the `FINAL_HEIGHTMAPS` unprimed entries (inherited from
//! the `LevelChunk` constructor), but the empty chunk is never serialized and
//! its heightmaps are never queried, so the port omits the priming.
//!
//! The `debug` flag (`EmptyLevelChunk` forces `defaultBlockState` to
//! `VOID_AIR`) is inherent to the all-void reads; `isDebug` is a `Level` flag
//! the port's value slice does not model.

use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::paletted_container_factory::PalettedContainerFactory;
use crate::chunk::upgrade_data::UpgradeData;
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::StateFlags;
use rivet_registry::core::{BlockPos, ChunkPos};

/// `net.minecraft.world.level.chunk.EmptyLevelChunk`.
pub struct EmptyLevelChunk<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// The `ChunkAccess` base (the `LevelChunk`-side fields; the empty chunk
    /// keeps them so every accessor has a definition, exactly like Java's
    /// inherited members).
    base: ChunkAccess<T, B, S>,
    /// `biome` — the single `Holder<Biome>` `getNoiseBiome` returns for every
    /// quart coordinate.
    biome: B,
    /// `Blocks.VOID_AIR.defaultBlockState()` — the state `getBlockState`
    /// returns for every position.
    void_air: T,
}

impl<T, B, S> EmptyLevelChunk<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `EmptyLevelChunk(Level, ChunkPos, Holder<Biome>)` — the short
    /// `LevelChunk` constructor (`UpgradeData.EMPTY`, `inhabitedTime` 0, no
    /// sections).
    ///
    /// `void_air` is the read default. The `container_factory` builds the
    /// default (all-air) sections the base constructor requires. `resolve`
    /// classifies states for the heightmap predicates (see [`ChunkAccess::new`]).
    pub fn new(
        pos: ChunkPos,
        height_accessor: SimpleLevelHeightAccessor,
        container_factory: &PalettedContainerFactory<T, B>,
        biome: B,
        void_air: T,
        resolve: &'static (dyn Fn(&T) -> StateFlags + Sync),
    ) -> Self {
        EmptyLevelChunk {
            base: ChunkAccess::new(
                pos,
                UpgradeData::empty(height_accessor.get_sections_count() as usize),
                height_accessor,
                container_factory,
                0,
                None,
                resolve,
            ),
            biome,
            void_air,
        }
    }

    /// `EmptyLevelChunk.getBlockState(BlockPos)` (and the Paper `int` overload)
    /// — `Blocks.VOID_AIR.defaultBlockState()` for every position.
    pub fn get_block_state(&self, _x: i32, _y: i32, _z: i32) -> T {
        self.void_air.clone()
    }

    /// `EmptyLevelChunk.setBlockState(BlockPos, BlockState, int)` — returns
    /// `null` (a no-op; Java `return null`).
    pub fn set_block_state(&mut self, _pos: &BlockPos, _state: T) -> Option<T> {
        None
    }

    /// `EmptyLevelChunk.getLightEmission(BlockPos)` — 0.
    pub fn get_light_emission(&self, _pos: &BlockPos) -> i32 {
        0
    }

    /// `EmptyLevelChunk.getNoiseBiome(int, int, int)` — the single biome.
    pub fn get_noise_biome(&self, _quart_x: i32, _quart_y: i32, _quart_z: i32) -> B {
        self.biome.clone()
    }

    /// `EmptyLevelChunk.isEmpty()` — true.
    pub fn is_empty(&self) -> bool {
        true
    }

    /// `EmptyLevelChunk.isYSpaceEmpty(int, int)` — true.
    pub fn is_y_space_empty(&self, _y_start_inclusive: i32, _y_end_inclusive: i32) -> bool {
        true
    }

    /// `EmptyLevelChunk.getFullStatus()` — `FullChunkStatus.FULL`.
    pub fn get_persisted_status(&self) -> ChunkStatus {
        ChunkStatus::Full
    }

    /// `EmptyLevelChunk.removeBlockEntity(BlockPos)` — a no-op.
    pub fn remove_block_entity(&mut self, _pos: &BlockPos) {}

    /// `EmptyLevelChunk.getBlockEntityNbt(BlockPos)` — `null` (no pending
    /// block entities in the empty chunk).
    pub fn get_block_entity_nbt(
        &self,
        _pos: &BlockPos,
    ) -> Option<&rivet_nbt::compound_tag::CompoundTag> {
        None
    }

    /// `EmptyLevelChunk.setBlockEntityNbt(CompoundTag)` — a no-op.
    pub fn set_block_entity_nbt(&mut self, _entity_tag: rivet_nbt::compound_tag::CompoundTag) {}

    /// `EmptyLevelChunk.getPos()` — the wrapped chunk position.
    pub fn get_pos(&self) -> ChunkPos {
        self.base.get_pos()
    }

    /// `EmptyLevelChunk.getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.base.get_min_y()
    }

    /// `EmptyLevelChunk.getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.base.get_height()
    }

    /// The contained `levelHeightAccessor`.
    pub fn height_accessor(&self) -> SimpleLevelHeightAccessor {
        self.base.height_accessor()
    }

    /// `getSections()` — the (all-air) default sections.
    pub fn get_sections(&self) -> &[LevelChunkSection<T, B>] {
        self.base.get_sections()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::strategy::Strategy;
    use crate::level::height_accessor::create as create_accessor;

    #[derive(Clone, Copy)]
    struct TestGlobalMap;
    impl GlobalIdMap<u8> for TestGlobalMap {
        fn get_id(&self, value: &u8) -> i32 {
            *value as i32
        }
        fn by_id_or_throw(&self, id: i32) -> u8 {
            id as u8
        }
        fn size(&self) -> i32 {
            256
        }
        fn by_id(&self, id: i32) -> Option<u8> {
            Some(id as u8)
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8>> {
            Box::new(*self)
        }
    }

    fn block_strategy() -> Strategy<u8> {
        Strategy::create_for_block_states(Box::new(TestGlobalMap))
    }
    fn biome_strategy() -> Strategy<u8> {
        Strategy::create_for_biomes(Box::new(TestGlobalMap))
    }
    fn factory() -> PalettedContainerFactory<u8, u8> {
        PalettedContainerFactory::new(block_strategy(), 0, biome_strategy(), 0)
    }
    fn empty_chunk() -> EmptyLevelChunk<u8, u8, &'static str> {
        EmptyLevelChunk::new(
            ChunkPos::ZERO,
            create_accessor(-64, 384),
            &factory(),
            7,   // the single biome id.
            255, // void air.
            // Everything the chunk reads is void air: not air-in-test-terms
            // (255 != 0), blocks motion, no fluid, not leaves.
            &|_| StateFlags {
                is_air: false,
                blocks_motion: true,
                has_fluid: false,
                is_leaves: false,
            },
        )
    }

    #[test]
    fn reads_return_void_air_and_single_biome() {
        let chunk = empty_chunk();
        // Void air everywhere, regardless of the (empty) section contents.
        assert_eq!(chunk.get_block_state(0, 0, 0), 255);
        assert_eq!(chunk.get_block_state(3, -64, 9), 255);
        assert_eq!(chunk.get_block_state(15, 319, 15), 255);
        // The single biome for every quart coordinate, including outside the
        // worldgen column (Java clamps nothing — it returns the field).
        assert_eq!(chunk.get_noise_biome(0, 0, 0), 7);
        assert_eq!(chunk.get_noise_biome(7, -999, 12), 7);
    }

    #[test]
    fn mutators_are_no_ops() {
        let mut chunk = empty_chunk();
        assert_eq!(chunk.set_block_state(&BlockPos::new(0, 0, 0), 1), None);
        chunk.set_block_entity_nbt(Default::default());
        chunk.remove_block_entity(&BlockPos::new(0, 0, 0));
        // Reads are unaffected.
        assert_eq!(chunk.get_block_state(0, 0, 0), 255);
    }

    #[test]
    fn empty_and_space_checks_are_true_and_status_is_full() {
        let chunk = empty_chunk();
        assert!(chunk.is_empty());
        assert!(chunk.is_y_space_empty(-64, 319));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert_eq!(chunk.get_light_emission(&BlockPos::new(0, 0, 0)), 0);
    }

    #[test]
    fn sections_are_present_but_all_air() {
        let chunk = empty_chunk();
        // The base constructor still materializes the default (all-air)
        // sections, so accessors that walk sections never see a gap.
        assert_eq!(chunk.get_sections().len(), 24);
        assert_eq!(chunk.get_min_y(), -64);
        assert_eq!(chunk.get_height(), 384);
        assert_eq!(chunk.get_pos(), ChunkPos::ZERO);
    }
}

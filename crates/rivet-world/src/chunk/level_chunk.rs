//! Port of `net.minecraft.world.level.chunk.LevelChunk` (MC 26.2) — the loaded
//! chunk: the read spine (`getBlockStateFinal` — Paper's get-block-chunk
//! optimisation) over the `ChunkAccess` base, plus the concrete chunk surface
//! the server seam and `ImposterProtoChunk` wrap.
//!
//! Java: `LevelChunk.java` in `working/Paper`. The class adds the Paper
//! `getBlockStateFinal` fast path, the `FULL`-status heightmap priming in the
//! constructor, the `level`/`blockTicks`/`fluidTicks`/block-entity references,
//! and the tick/game-event machinery. The port keeps the read spine and the
//! heightmap priming; the rest defers with its owning units.
//!
//! The `getBlockStateFinal` fast path reads the section's storage at the flat
//! index `(y & 15) << 8 | (z & 15) << 4 | x & 15` — Paper's "reduce instructions"
//! bypass of `PalettedContainer.get`'s `Strategy` masking — and returns the
//! caller's air value when the section is out of range or all-air. The port
//! stores that air value (`Blocks.AIR.defaultBlockState()` in Java) because the
//! block-state type is the caller's `T`.
//!
//! Deferred with their owning units:
//! - `getBlockTicks`/`getFluidTicks`/`getTicksForSerialization` and the
//!   `PackedTicks` record (the `world.ticks` unit);
//! - the `blockEntities` map, `setBlockEntity`/`getBlockEntity` and the
//!   `getBlockEntityNbtForSaving` promote-then-save logic (the block-entity
//!   unit); the port keeps only the `pendingBlockEntities` map on the base,
//!   which is the runtime authority for loaded block entities (#537);
//! - `setBlockState`'s mutators (with #216);
//! - the game-event listener registries, tickers, and the Paper `unsavedListener`
//!   (`markUnsaved` is the bare base version — the listener is a Paper dirty-tick
//!   optimization deferred with the Moonrise scheduler);
//! - `getInhabitedTime`'s Paper `fixedChunkInhabitedTime` config override (the
//!   config surface is not ported; the base inhabited time is used);
//! - the `debug` flag / `defaultBlockState`/`getBlockState(BlockPos)` debug-world
//!   path (`Level.isDebug` + `DebugLevelSource` are unported; the port's
//!   `get_block_state` is always the final fast path, which Paper's `if (true)`
//!   also routes through).
//!
//! RivetTodo(#185): `getPersistedStatus()` returns `ChunkStatus.FULL`; the
//! `FullChunkStatus`/`fullStatus` supplier and the chunkmap pipeline surface are
//! not ported.

use bytes::BytesMut;

use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::paletted_container_factory::PalettedContainerFactory;
use crate::chunk::strategy::Strategy;
use crate::chunk::upgrade_data::UpgradeData;
use crate::level::height_accessor::SimpleLevelHeightAccessor;
use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, Heightmap, StateFlags, Types};
use crate::lighting::swmr_nibble_array::SwmrNibbleArray;
use indexmap::{IndexMap, IndexSet};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_registry::core::{BlockPos, ChunkPos};
use std::collections::HashMap;

/// `net.minecraft.world.level.chunk.LevelChunk` — the loaded chunk value.
pub struct LevelChunk<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// The `ChunkAccess` base — sections, heightmaps, flags, pending BEs.
    base: ChunkAccess<T, B, S>,
    /// `Blocks.AIR.defaultBlockState()` — the state `getBlockStateFinal` returns
    /// for an out-of-range / all-air section. Stored because the block-state
    /// type is the caller's `T`.
    air: T,
}

impl<T, B, S> LevelChunk<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `LevelChunk(Level, ChunkPos, UpgradeData, LevelChunkTicks, LevelChunkTicks,
    /// long inhabitedTime, LevelChunkSection[] sections, PostLoadProcessor,
    /// BlendingData)` — the base constructor plus the `FULL` heightmap priming:
    /// an unprimed entry for each `ChunkStatus.FULL.heightmapsAfter()` type.
    ///
    /// The ticks, `postLoad`, and `blendingData` parameters are omitted with
    /// their units. `air` is the default state for the read spine; `resolve`
    /// classifies states for the heightmap predicates (see [`ChunkAccess::new`]).
    #[allow(clippy::too_many_arguments)] // Java's constructor has 9 parameters.
    pub fn new(
        pos: ChunkPos,
        upgrade_data: UpgradeData,
        height_accessor: SimpleLevelHeightAccessor,
        container_factory: &PalettedContainerFactory<T, B>,
        inhabited_time: i64,
        sections: Option<Vec<LevelChunkSection<T, B>>>,
        air: T,
        resolve: &'static (dyn Fn(&T) -> StateFlags + Sync),
    ) -> Self {
        let mut base = ChunkAccess::new(
            pos,
            upgrade_data,
            height_accessor,
            container_factory,
            inhabited_time,
            sections,
            resolve,
        );
        for ty in FINAL_HEIGHTMAPS {
            base.get_or_create_heightmap_unprimed(ty);
        }
        LevelChunk { base, air }
    }

    /// Value-transform every block state and biome, preserving all other chunk
    /// state (sections, heightmaps, light nibbles, pending block entities,
    /// post-processing, flags). The #516 server bridge uses this to convert the
    /// reconstructed `LevelChunk<BlockState, BiomeId, ()>` into the server's
    /// `LevelChunk<StateId, BiomeId, ()>` — both value pairs are dense
    /// `u16`-backed ids in the same generated registry, so the
    /// `pack`/`unpack` re-encode against the target strategies preserves the
    /// wire-identical section buffers.
    ///
    /// The new base is rebuilt through the same path as [`new`](Self::new)
    /// (the FULL heightmap priming and the all-air fallback), then the owned
    /// state that `ChunkAccess::new` resets — unsaved, light-correct,
    /// post-processing, pending block entities, light nibbles, heightmaps,
    /// structure access — is reinstalled so the conversion is a pure re-type.
    #[allow(clippy::too_many_arguments)] // the full re-type surface, mirroring
    // `ChunkAccess::map_values` 1:1.
    pub fn map_values<T2, B2>(
        self,
        block_strategy: Strategy<T2>,
        biome_strategy: Strategy<B2>,
        air: T2,
        default_biome: B2,
        map_block: &impl Fn(&T) -> T2,
        map_biome: &impl Fn(&B) -> B2,
        resolve: &'static (dyn Fn(&T2) -> StateFlags + Sync),
    ) -> Result<LevelChunk<T2, B2, S>, String>
    where
        T2: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        B2: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    {
        let LevelChunk { base, air: _ } = self;
        let base = base.map_values(
            block_strategy,
            biome_strategy,
            air.clone(),
            default_biome,
            map_block,
            map_biome,
            resolve,
        )?;
        Ok(LevelChunk { base, air })
    }

    /// `LevelChunk.getBlockState(BlockPos)` — Paper routes it through
    /// `getBlockStateFinal` (`if (true) return getBlockStateFinal(...)`).
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> T {
        self.get_block_state_final(x, y, z)
    }

    /// `getBlockStateFinal(int, int, int)` — Paper's get-block-chunk
    /// optimisation: the section index guard (out of range or `nonEmptyBlockCount
    /// == 0` → air), then the storage read at the flat
    /// `(y & 15) << 8 | (z & 15) << 4 | x & 15` index (bypassing the
    /// `PalettedContainer.get` `Strategy` mask).
    pub fn get_block_state_final(&self, x: i32, y: i32, z: i32) -> T {
        let section_index = self.base.get_section_index(y);
        if section_index < 0
            || section_index as usize >= self.base.get_sections().len()
            || self
                .base
                .get_section(section_index as usize)
                .non_empty_block_count()
                == 0
        {
            return self.air.clone();
        }
        self.base
            .get_section(section_index as usize)
            .states()
            .get_index(((y & 15) << 8 | (z & 15) << 4 | x & 15) as usize)
    }

    /// `LevelChunk.getPos()`.
    pub fn get_pos(&self) -> ChunkPos {
        self.base.get_pos()
    }

    /// The contained `ChunkAccess` base (the `LightChunk`/`LightChunkGetter`
    /// view — Java's `ChunkAccess` implements `LightChunk`).
    pub fn get_base(&self) -> &ChunkAccess<T, B, S> {
        &self.base
    }

    /// `LevelChunk.getX()` — Paper's cached `locX`.
    pub fn get_x(&self) -> i32 {
        self.base.get_pos().x()
    }

    /// `LevelChunk.getZ()` — Paper's cached `locZ`.
    pub fn get_z(&self) -> i32 {
        self.base.get_pos().z()
    }

    /// `LevelChunk.getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.base.get_min_y()
    }

    /// `LevelChunk.getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.base.get_height()
    }

    /// The contained `levelHeightAccessor` (shared with wrapping chunks).
    pub fn height_accessor(&self) -> SimpleLevelHeightAccessor {
        self.base.height_accessor()
    }

    /// `LevelChunk.getSection(int)`.
    pub fn get_section(&self, section_index: usize) -> &LevelChunkSection<T, B> {
        self.base.get_section(section_index)
    }

    /// `LevelChunk.getSections()`.
    pub fn get_sections(&self) -> &[LevelChunkSection<T, B>] {
        self.base.get_sections()
    }

    /// `ChunkAccess.getNoiseBiome(int, int, int)`.
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> B {
        self.base.get_noise_biome(quart_x, quart_y, quart_z)
    }

    /// `ChunkAccess.getHeight(Types, int, int)` — the heightmap read (named
    /// `get_height_at` to avoid the `getHeight()` overload clash). Priming a
    /// missing entry walks the sections, so the read takes `&mut self`.
    pub fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        self.base.get_height_at(ty, x, z)
    }

    /// The heightmaps storage (read-only; the concrete chunk's base).
    pub fn heightmaps(&self) -> &[Option<Heightmap>; 6] {
        self.base.heightmaps()
    }

    /// Narrow loader seam for `SerializableChunkData.read`.
    pub(crate) fn base_mut(&mut self) -> &mut ChunkAccess<T, B, S> {
        &mut self.base
    }

    /// `Heightmap.primeHeightmaps(this, types)` during chunk load.
    pub(crate) fn prime_heightmaps(&mut self, types: &[Types]) {
        self.base.prime_heightmaps(types);
    }

    /// `ChunkAccess.addPackedPostProcess` during chunk load.
    pub(crate) fn add_packed_post_process(&mut self, offsets: &[i16], section_index: usize) {
        self.base.add_packed_post_process(offsets, section_index);
    }

    /// `ChunkAccess.setHeightmap(Types, long[])` — adopted by the chunk-load /
    /// `replaceWithPacketData` paths.
    pub fn set_heightmap(&mut self, key: Types, data: &[i64]) {
        self.base.set_heightmap(key, data);
    }

    /// `StarlightChunk.starlight$getBlockNibbles()`.
    pub fn block_nibbles(&self) -> &[SwmrNibbleArray] {
        self.base.block_nibbles()
    }

    /// `StarlightChunk.starlight$setBlockNibbles(SWMRNibbleArray[])`.
    pub fn set_block_nibbles(&mut self, nibbles: Vec<SwmrNibbleArray>) {
        self.base.set_block_nibbles(nibbles);
    }

    /// `StarlightChunk.starlight$getSkyNibbles()`.
    pub fn sky_nibbles(&self) -> &[SwmrNibbleArray] {
        self.base.sky_nibbles()
    }

    /// `StarlightChunk.starlight$setSkyNibbles(SWMRNibbleArray[])`.
    pub fn set_sky_nibbles(&mut self, nibbles: Vec<SwmrNibbleArray>) {
        self.base.set_sky_nibbles(nibbles);
    }

    /// `ChunkAccess.getOrCreateHeightmapUnprimed(Types)`.
    pub fn get_or_create_heightmap_unprimed(&mut self, ty: Types) -> &mut Heightmap {
        self.base.get_or_create_heightmap_unprimed(ty)
    }

    /// `LevelChunk.getInhabitedTime()` — the base value. Paper overrides this
    /// to consult `paperConfig().chunks.fixedChunkInhabitedTime`; the config
    /// surface is not ported, so the base is used (see the module doc).
    pub fn get_inhabited_time(&self) -> i64 {
        self.base.get_inhabited_time()
    }

    /// `ChunkAccess.setInhabitedTime(long)`.
    pub fn set_inhabited_time(&mut self, inhabited_time: i64) {
        self.base.set_inhabited_time(inhabited_time);
    }

    /// `ChunkAccess.incrementInhabitedTime(long)`.
    pub fn increment_inhabited_time(&mut self, inhabited_time_delta: i64) {
        self.base.increment_inhabited_time(inhabited_time_delta);
    }

    /// `ChunkAccess.markUnsaved()` — the bare base version; Paper's
    /// `unsavedListener` callback is deferred (module doc).
    pub fn mark_unsaved(&mut self) {
        self.base.mark_unsaved();
    }

    /// `ChunkAccess.tryMarkSaved()`.
    pub fn try_mark_saved(&mut self) -> bool {
        self.base.try_mark_saved()
    }

    /// `ChunkAccess.isUnsaved()`.
    pub fn is_unsaved(&self) -> bool {
        self.base.is_unsaved()
    }

    /// `ChunkAccess.setLightCorrect(boolean)`.
    pub fn set_light_correct(&mut self, light_correct: bool) {
        self.base.set_light_correct(light_correct);
    }

    /// `ChunkAccess.isLightCorrect()`.
    pub fn is_light_correct(&self) -> bool {
        self.base.is_light_correct()
    }

    /// `LevelChunk.getPersistedStatus()` — always `ChunkStatus.FULL`.
    pub fn get_persisted_status(&self) -> ChunkStatus {
        ChunkStatus::Full
    }

    /// `ChunkAccess.setBlockEntityNbt(CompoundTag)` — the pending-NBT carrier.
    pub fn set_block_entity_nbt(&mut self, entity_tag: CompoundTag) {
        self.base.set_block_entity_nbt(entity_tag);
    }

    /// `ChunkAccess.getBlockEntityNbt(BlockPos)` — the pending-NBT read.
    pub fn get_block_entity_nbt(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.base.get_block_entity_nbt(pos)
    }

    /// `pendingBlockEntities` — the read-only, insertion-ordered runtime
    /// authority (source order for the surviving positions, #537).
    pub fn pending_block_entities(&self) -> &IndexMap<BlockPos, CompoundTag> {
        self.base.pending_block_entities()
    }

    /// `ProtoChunk.removeBlockEntity(BlockPos)`'s pending half — removes the
    /// position from the runtime authority (#537).
    pub fn remove_block_entity_nbt(&mut self, pos: &BlockPos) -> Option<CompoundTag> {
        self.base.remove_block_entity_nbt(pos)
    }

    /// `LevelChunk.getBlockEntityNbtForSaving(BlockPos, HolderLookup)` — Java
    /// saves the materialized block entity (with `keepPacked false`) or falls
    /// back to the pending tag (with `keepPacked true`). The block-entity map
    /// and the registry-dependent save are unported, so the port returns the
    /// pending tag directly.
    ///
    /// The `blockEntities.containsKey` guard and the
    /// `saveWithFullMetadata`/`keepPacked` copy live with the block-entity unit.
    pub fn get_block_entity_nbt_for_saving(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.base.get_block_entity_nbt(pos)
    }

    /// `ChunkAccess.isYSpaceEmpty(int, int)`.
    pub fn is_y_space_empty(&self, y_start_inclusive: i32, y_end_inclusive: i32) -> bool {
        self.base
            .is_y_space_empty(y_start_inclusive, y_end_inclusive)
    }

    /// `ChunkAccess.findBlocks(Predicate, BiConsumer)`.
    pub fn find_blocks(&self, predicate: &dyn Fn(&T) -> bool, consumer: impl FnMut(BlockPos, T)) {
        self.base.find_blocks(predicate, consumer);
    }

    /// `ChunkAccess.getStartForStructure(Structure)`.
    pub fn get_start_for_structure(&self, structure: &S) -> Option<i64> {
        self.base.get_start_for_structure(structure)
    }

    /// `ChunkAccess.setStartForStructure(Structure, StructureStart)`.
    pub fn set_start_for_structure(&mut self, structure: S, start: i64) {
        self.base.set_start_for_structure(structure, start);
    }

    /// `ChunkAccess.getAllStarts()`.
    pub fn get_all_starts(&self) -> &HashMap<S, i64> {
        self.base.get_all_starts()
    }

    /// `ChunkAccess.setAllStarts(Map)`.
    pub fn set_all_starts(&mut self, starts: HashMap<S, i64>) {
        self.base.set_all_starts(starts);
    }

    /// `ChunkAccess.getReferencesForStructure(Structure)`.
    pub fn get_references_for_structure<'a>(
        &'a self,
        structure: &'a S,
    ) -> impl Iterator<Item = &'a u64> + 'a {
        self.base.get_references_for_structure(structure)
    }

    /// `ChunkAccess.addReferenceForStructure(Structure, long)`.
    pub fn add_reference_for_structure(&mut self, structure: S, reference: u64) {
        self.base.add_reference_for_structure(structure, reference);
    }

    /// `ChunkAccess.getAllReferences()`.
    pub fn get_all_references(&self) -> &IndexMap<S, IndexSet<u64>> {
        self.base.get_all_references()
    }

    /// `ChunkAccess.setAllReferences(Map)`.
    pub fn set_all_references<I: IntoIterator<Item = (S, Vec<u64>)>>(&mut self, data: I) {
        self.base.set_all_references(data);
    }

    /// The three `Usage.CLIENT` heightmaps as the `LevelChunkPacketData`
    /// `(HeightmapType, long[])` pairs, in the client `EnumMap` order
    /// (`WORLD_SURFACE`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES` — the
    /// wire-id order). Only primed entries are emitted.
    pub fn client_heightmaps(&self) -> Vec<(HeightmapType, Vec<i64>)> {
        let mut out = Vec::with_capacity(3);
        for ty in Types::all() {
            if ty.send_to_client()
                && let Some(hm) = &self.base.heightmaps()[ty as usize]
            {
                out.push((ty.as_protocol(), hm.get_raw_data().to_vec()));
            }
        }
        out
    }

    /// The opaque sections buffer — the `[bits][palette][raw]` wire bytes of
    /// every section concatenated (Java `calculateChunkSize` +
    /// `extractChunkData`). Mirrors `SuperflatChunkContent::sections_buffer`;
    /// asserts the exact-size invariant after writing.
    pub fn sections_buffer(&self) -> Vec<u8> {
        let mut buf = FriendlyByteBuf::new(BytesMut::new());
        for section in self.base.get_sections() {
            section.write(&mut buf);
        }
        let bytes = buf.into_inner().to_vec();
        let expected: i32 = self
            .base
            .get_sections()
            .iter()
            .map(|s| s.get_serialized_size())
            .sum();
        assert_eq!(
            bytes.len() as i32,
            expected,
            "section buffer must be exactly the sum of getSerializedSize()"
        );
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::strategy::Strategy;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types};

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
    fn accessor() -> SimpleLevelHeightAccessor {
        create_accessor(-64, 384)
    }
    fn factory() -> PalettedContainerFactory<u8, u8> {
        PalettedContainerFactory::new(block_strategy(), 0, biome_strategy(), 0)
    }

    /// The `BlockBehaviour` predicates for the test sections: air is `0`,
    /// nothing randomly ticks, everything is fluid-empty, nothing is
    /// special-colliding.
    fn is_air(s: &u8) -> bool {
        *s == 0
    }
    fn is_randomly_ticking(_s: &u8) -> bool {
        false
    }
    fn fluid_is_empty(_s: &u8) -> bool {
        true
    }
    fn fluid_is_randomly_ticking(_s: &u8) -> bool {
        false
    }
    fn is_special_colliding(_s: &u8) -> bool {
        false
    }

    /// A loaded chunk with a stone block at section-local (0,0,0) of section 0
    /// (absolute y -64).
    fn stone_chunk() -> LevelChunk<u8, u8, &'static str> {
        let mut sections = Vec::with_capacity(24);
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1u8);
        sections.push(LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
            is_randomly_ticking,
            fluid_is_empty,
            fluid_is_randomly_ticking,
            is_special_colliding,
        ));
        for _ in 1..24 {
            sections.push(LevelChunkSection::new(
                PalettedContainer::new(0u8, block_strategy()),
                PalettedContainer::new(0u8, biome_strategy()),
                is_air,
                is_randomly_ticking,
                fluid_is_empty,
                fluid_is_randomly_ticking,
                is_special_colliding,
            ));
        }
        LevelChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            accessor(),
            &factory(),
            0,
            Some(sections),
            0,
            // u8 tests: 0 is air, 1 is stone (blocks motion).
            &|s: &u8| StateFlags {
                is_air: *s == 0,
                blocks_motion: *s != 0,
                has_fluid: false,
                is_leaves: false,
            },
        )
    }

    #[test]
    fn get_block_state_final_reads_the_flat_index() {
        let chunk = stone_chunk();
        // Stone at absolute y -64 (section 0, local y 0).
        assert_eq!(chunk.get_block_state_final(0, -64, 0), 1);
        // The flat index `(y&15)<<8 | (z&15)<<4 | x&15` reads the same cell
        // the section stores at (0,0,0) of section 0.
        assert_eq!(chunk.get_block_state(3, -64, 5), 0); // air elsewhere.
        // Out of build height -> air (not a panic).
        assert_eq!(chunk.get_block_state_final(0, -65, 0), 0);
        assert_eq!(chunk.get_block_state_final(0, 320, 0), 0);
        assert_eq!(chunk.get_block_state_final(-1, 0, 0), 0); // x below min.
        // All-air section (index 4) -> air.
        assert_eq!(chunk.get_block_state_final(0, 0, 0), 0);
    }

    #[test]
    fn constructor_primes_exactly_final_heightmaps_unprimed() {
        let mut chunk = stone_chunk();
        for ty in Types::all() {
            assert_eq!(
                chunk.heightmaps()[ty as usize].is_some(),
                FINAL_HEIGHTMAPS.contains(&ty),
                "type {ty:?}"
            );
        }
        // The constructor creates the FINAL_HEIGHTMAPS entries as all-zero
        // storage, exactly like Java (`new Heightmap(this, type)`); because an
        // entry exists, `getHeight` does NOT prime — even the stone column
        // reads `0 + minY - 1` = -65. Java logs "Unprimed heightmap" here but
        // returns the same value.
        assert_eq!(chunk.get_height_at(Types::WorldSurface, 0, 0), -65);
        assert_eq!(chunk.get_height_at(Types::MotionBlocking, 7, 9), -65);
    }

    #[test]
    fn set_heightmap_then_read_height() {
        let mut chunk = stone_chunk();
        let raw: Vec<i64> = {
            let mut v = vec![0x0040_2010_0804_0201i64; 36];
            v.push(0x0000_0000_0804_0201i64);
            v
        };
        chunk.set_heightmap(Types::WorldSurface, &raw);
        // Stored offset 1 -> height -64 (a flat stone floor at minY).
        assert_eq!(chunk.get_height_at(Types::WorldSurface, 3, 7), -64);
    }

    #[test]
    fn client_heightmaps_emit_the_three_client_types_in_wire_order() {
        let mut chunk = stone_chunk();
        let raw: Vec<i64> = {
            let mut v = vec![0x0040_2010_0804_0201i64; 36];
            v.push(0x0000_0000_0804_0201i64);
            v
        };
        chunk.set_heightmap(Types::WorldSurface, &raw);
        chunk.set_heightmap(Types::MotionBlocking, &raw);
        chunk.set_heightmap(Types::MotionBlockingNoLeaves, &raw);
        let heightmaps = chunk.client_heightmaps();
        let types: Vec<HeightmapType> = heightmaps.iter().map(|(ty, _)| *ty).collect();
        assert_eq!(
            types,
            vec![
                HeightmapType::WorldSurface,
                HeightmapType::MotionBlocking,
                HeightmapType::MotionBlockingNoLeaves,
            ]
        );
        // The worldgen types are never sent, even when primed.
        chunk.set_heightmap(Types::WorldSurfaceWg, &raw);
        assert_eq!(chunk.client_heightmaps().len(), 3);
        for (_, raw) in &heightmaps {
            assert_eq!(raw.len(), 37);
        }
    }

    #[test]
    fn persisted_status_is_full() {
        assert_eq!(stone_chunk().get_persisted_status(), ChunkStatus::Full);
    }

    #[test]
    fn flag_and_time_and_pending_be_surface() {
        let mut chunk = stone_chunk();
        assert!(!chunk.is_unsaved());
        chunk.mark_unsaved();
        assert!(chunk.is_unsaved());
        assert!(chunk.try_mark_saved());
        assert!(!chunk.is_unsaved());

        assert_eq!(chunk.get_inhabited_time(), 0);
        chunk.increment_inhabited_time(42);
        assert_eq!(chunk.get_inhabited_time(), 42);
        chunk.set_inhabited_time(10);
        assert_eq!(chunk.get_inhabited_time(), 10);

        let mut tag = CompoundTag::new();
        tag.put_int("x", 1);
        tag.put_int("y", 2);
        tag.put_int("z", 3);
        chunk.set_block_entity_nbt(tag);
        let pos = BlockPos::new(1, 2, 3);
        assert!(chunk.get_block_entity_nbt(&pos).is_some());
        assert!(chunk.get_block_entity_nbt_for_saving(&pos).is_some());
    }

    #[test]
    fn sections_buffer_is_the_sum_of_serialized_sizes() {
        let chunk = stone_chunk();
        let buffer = chunk.sections_buffer();
        let expected: i32 = chunk
            .get_sections()
            .iter()
            .map(|s| s.get_serialized_size())
            .sum();
        assert_eq!(buffer.len() as i32, expected);
        // The stone section is a 4-bit linear palette [air, stone]: its
        // serialized size is nonzero, so the buffer is not empty.
        assert!(!buffer.is_empty());
    }
}

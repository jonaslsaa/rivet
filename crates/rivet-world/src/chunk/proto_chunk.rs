//! Port of `net.minecraft.world.level.chunk.ProtoChunk` (MC 26.2) — the
//! worldgen chunk: the `ChunkAccess` base plus the generation-phase carrier
//! (entity NBT list, persisted status, carving mask, packed post-processing
//! offsets).
//!
//! Java: `ProtoChunk.java` in `working/Paper`. The class adds the build-height
//! guard to the block-state read, the `entities` NBT list, the `status`
//! (a `ChunkStatus`), the `CarvingMask`, and the packed post-processing
//! coordinates. `getBlockState` returns `VOID_AIR` outside build height and
//! `AIR` for an all-air section.
//!
//! Deferred with their owning units:
//! - `getFluidState` (the `FluidState` type lives with the material/block-state
//!   units);
//! - `setBlockState`'s `INITIALIZE_LIGHT`-status light writes (the #216
//!   section write's light half; a worldgen `ProtoChunk` is always below that
//!   status). The section write itself and its heightmap half
//!   ([`ProtoChunk::update_heightmaps_after`], #287) are implemented here —
//!   the #216 section write calls `update_heightmaps_after` after the section's
//!   `setBlockState`;
//! - `setBlockEntity`/`getBlockEntity`/`getBlockEntities` (the block-entity
//!   unit); the port keeps `pendingBlockEntities` on the base and
//!   `removeBlockEntity`'s pending half;
//! - the `ProtoChunkTicks<Block>`/`ProtoChunkTicks<Fluid>` containers and
//!   `getBlockTicks`/`getFluidTicks`/`getTicksForSerialization` (the
//!   `world.ticks` unit);
//! - `setStartForStructure`'s `BelowZeroRetrogen` bound check and
//!   `setBelowZeroRetrogen`/`getBelowZeroRetrogen` (`BelowZeroRetrogen` is
//!   unported; the base's setter is used unchanged);
//! - `getHeightAccessorForGeneration`'s `UPGRADE_HEIGHT_ACCESSOR` branch
//!   (with `BelowZeroRetrogen`).
//!
//! RivetTodo(#185): `getNoiseBiome`'s `getHighestGeneratedStatus().isOrAfter(
//! ChunkStatus.BIOMES)` guard remains outside this read-only slice.
//! `markPosForPostProcessing`'s parent `postProcessGeneration` consumer and
//! `addPackedPostProcess`'s `ShortList` read path remain with that owning unit.

use crate::biome::biome_resolver::BiomeResolver;
use crate::biome::climate::Sampler;
use crate::block::BlockState;
use crate::block::blocks::Blocks;
use crate::chunk::carving_mask::CarvingMask;
use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::paletted_container_factory::PalettedContainerFactory;
use crate::chunk::storage::chunk_reconstruction::block_state_predicates;
use crate::chunk::upgrade_data::UpgradeData;
use crate::level::height_accessor::SimpleLevelHeightAccessor;
use crate::levelgen::carver::CarveChunk;
use crate::levelgen::heightmap::{Heightmap, StateFlags, Types};
use crate::levelgen::surface_rules::ChunkSurface;
use crate::lighting::swmr_nibble_array::SwmrNibbleArray;
use indexmap::{IndexMap, IndexSet};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_registry::holder::Holder;

/// `net.minecraft.world.level.chunk.ProtoChunk` — the worldgen chunk value.
pub struct ProtoChunk<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// The `ChunkAccess` base.
    base: ChunkAccess<T, B, S>,
    /// `entities` — serialized entities awaiting the chunk's load (Java
    /// `Lists.newArrayList()`).
    entities: Vec<CompoundTag>,
    /// `status` — `ChunkStatus.EMPTY` by default.
    status: ChunkStatus,
    /// `carvingMask`.
    carving_mask: Option<CarvingMask>,
    /// `Blocks.AIR.defaultBlockState()` — returned for an all-air section.
    air: T,
    /// `Blocks.VOID_AIR.defaultBlockState()` — returned outside build height.
    void_air: T,
}

impl<T, B, S> ProtoChunk<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `ProtoChunk(ChunkPos, UpgradeData, LevelHeightAccessor,
    /// PalettedContainerFactory, BlendingData)` — the base constructor with
    /// `inhabitedTime = 0`, an empty `status`, and no carving mask.
    ///
    /// `air`/`void_air` are the read defaults; `resolve` classifies states for
    /// the heightmap predicates (see [`ChunkAccess::new`]).
    #[allow(clippy::too_many_arguments)] // Java's constructor has 8 parameters.
    pub fn new(
        pos: ChunkPos,
        upgrade_data: UpgradeData,
        height_accessor: SimpleLevelHeightAccessor,
        container_factory: &PalettedContainerFactory<T, B>,
        sections: Option<Vec<LevelChunkSection<T, B>>>,
        air: T,
        void_air: T,
        resolve: &'static (dyn Fn(&T) -> StateFlags + Sync),
    ) -> Self {
        ProtoChunk {
            base: ChunkAccess::new(
                pos,
                upgrade_data,
                height_accessor,
                container_factory,
                0,
                sections,
                resolve,
            ),
            entities: Vec::new(),
            status: ChunkStatus::Empty,
            carving_mask: None,
            air,
            void_air,
        }
    }

    /// `ProtoChunk.setBlockState`'s heightmap half — Java collects the
    /// missing `getPersistedStatus().heightmapsAfter()` types, primes them,
    /// then runs `Heightmap.update` on every type in the set. The #216 section
    /// write calls this after the section's `setBlockState`; `placed` is the
    /// placed state's behavior flags.
    ///
    /// `ChunkStatus.EMPTY`'s `heightmapsAfter()` is `WORLDGEN_HEIGHTMAPS`
    /// (the two `Usage.WORLDGEN` types); `FULL` (the `LevelChunk` status) is
    /// `FINAL_HEIGHTMAPS`; the complete persisted status ladder selects the
    /// appropriate set at the `CARVERS` boundary.
    pub fn update_heightmaps_after(
        &mut self,
        local_x: i32,
        y: i32,
        local_z: i32,
        placed: StateFlags,
    ) {
        let after = self.status.heightmaps_after();
        self.base
            .update_heightmaps_after(after, local_x, y, local_z, placed);
    }

    /// `ProtoChunk.getBlockState(BlockPos)` — `VOID_AIR` outside build height,
    /// else `AIR` for an all-air section or the masked section read.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> T {
        if self.base.is_outside_build_height(y) {
            return self.void_air.clone();
        }
        let section = self
            .base
            .get_section(self.base.get_section_index(y) as usize);
        if section.has_only_air() {
            self.air.clone()
        } else {
            section.get_block_state(x & 15, y & 15, z & 15)
        }
    }

    /// `ProtoChunk.getPos()`.
    pub fn get_pos(&self) -> ChunkPos {
        self.base.get_pos()
    }

    /// `ProtoChunk.getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.base.get_min_y()
    }

    /// `ProtoChunk.getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.base.get_height()
    }

    /// The inherited `ChunkAccess.getHeightmaps()` storage.
    pub fn heightmaps(&self) -> &[Option<Heightmap>; 6] {
        self.base.heightmaps()
    }

    /// The inherited `ChunkAccess.setHeightmap(Types, long[])` load path.
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

    /// `ProtoChunk.getSections()`.
    pub fn get_sections(&self) -> &[LevelChunkSection<T, B>] {
        self.base.get_sections()
    }

    /// `ChunkAccess.getSection(int)`.
    pub fn get_section(&self, section_index: usize) -> &LevelChunkSection<T, B> {
        self.base.get_section(section_index)
    }

    /// `ChunkAccess.getSection(int)` — the mutable half, for the worldgen
    /// block writes (`NoiseBasedChunkGenerator.doFill`).
    pub fn get_section_mut(&mut self, section_index: usize) -> &mut LevelChunkSection<T, B> {
        self.base.get_section_mut(section_index)
    }

    /// `ChunkAccess.fillBiomesFromNoise(BiomeResolver, Climate.Sampler)` — the
    /// biomes step of the status ladder, forwarded to the base (`ProtoChunk`
    /// inherits the method from `ChunkAccess` in Java). `map_biome` converts
    /// each resolved `Holder<BiomeId>` into the section's stored element `B`
    /// (see [`ChunkAccess::fill_biomes_from_noise`]).
    pub fn fill_biomes_from_noise(
        &mut self,
        biome_resolver: &dyn BiomeResolver,
        sampler: &Sampler,
        map_biome: &impl Fn(&Holder<BiomeId>) -> B,
    ) {
        self.base
            .fill_biomes_from_noise(biome_resolver, sampler, map_biome);
    }

    /// `ChunkAccess.getSectionIndex(int blockY)`.
    pub fn get_section_index(&self, block_y: i32) -> i32 {
        self.base.get_section_index(block_y)
    }

    /// The contained `levelHeightAccessor` value — `ChunkAccess.getHeightAccessorForGeneration()`
    /// (the `UPGRADE_HEIGHT_ACCESSOR` branch defers with `BelowZeroRetrogen`).
    pub fn height_accessor(&self) -> SimpleLevelHeightAccessor {
        self.base.height_accessor()
    }

    /// `ChunkAccess.getOrCreateHeightmapUnprimed(Types)` — the worldgen
    /// `doFill` prologue creates the two `Usage.WORLDGEN` heightmaps here.
    pub fn get_or_create_heightmap_unprimed(&mut self, ty: Types) -> &mut Heightmap {
        self.base.get_or_create_heightmap_unprimed(ty)
    }

    /// The worldgen `doFill` per-block write (see
    /// [`ChunkAccess::write_worldgen_block`]).
    #[allow(clippy::too_many_arguments)] // the 5 coords/state + the 5 `BlockBehaviour` predicates.
    pub fn write_worldgen_block(
        &mut self,
        section_index: i32,
        x_in_section: i32,
        y_in_section: i32,
        z_in_section: i32,
        pos_y: i32,
        state: T,
        is_air: &dyn Fn(&T) -> bool,
        is_randomly_ticking: &dyn Fn(&T) -> bool,
        fluid_is_empty: &dyn Fn(&T) -> bool,
        fluid_is_randomly_ticking: &dyn Fn(&T) -> bool,
        is_special_colliding: &dyn Fn(&T) -> bool,
    ) {
        self.base.write_worldgen_block(
            section_index,
            x_in_section,
            y_in_section,
            z_in_section,
            pos_y,
            state,
            is_air,
            is_randomly_ticking,
            fluid_is_empty,
            fluid_is_randomly_ticking,
            is_special_colliding,
        );
    }

    /// `ChunkAccess.getNoiseBiome(int, int, int)` — the base read. Java guards
    /// with `getHighestGeneratedStatus().isOrAfter(ChunkStatus.BIOMES)` and
    /// throws otherwise; the guard defers with the status unit (#185).
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> B {
        self.base.get_noise_biome(quart_x, quart_y, quart_z)
    }

    /// `ProtoChunk.getPersistedStatus()`.
    pub fn get_persisted_status(&self) -> ChunkStatus {
        self.status
    }

    /// `ProtoChunk.setPersistedStatus(ChunkStatus)` — stores and marks unsaved
    /// (Java also clears `belowZeroRetrogen` once past its target status).
    pub fn set_persisted_status(&mut self, status: ChunkStatus) {
        self.status = status;
        self.base.mark_unsaved();
    }

    /// `ChunkAccess.isLightCorrect()` — the light-correct flag the LIGHT task
    /// toggles around `lightChunk` (`ChunkLightTask.LightTask`).
    pub fn is_light_correct(&self) -> bool {
        self.base.is_light_correct()
    }

    /// `ChunkAccess.setLightCorrect(boolean)` — forwarded to the base (which
    /// marks the chunk unsaved, matching `ChunkAccess.setLightCorrect`).
    pub fn set_light_correct(&mut self, light_correct: bool) {
        self.base.set_light_correct(light_correct);
    }

    /// `ProtoChunk.addEntity(CompoundTag)`.
    pub fn add_entity(&mut self, tag: CompoundTag) {
        self.entities.push(tag);
    }

    /// `ProtoChunk.getEntities()`.
    pub fn get_entities(&self) -> &[CompoundTag] {
        &self.entities
    }

    /// `ProtoChunk.packOffsetCoordinates(BlockPos)` — the packed section-relative
    /// `short` `(x & 15) | (y & 15) << 4 | (z & 15) << 8` (x low, z high — a
    /// different layout from `SectionPos.sectionRelativePos`).
    pub fn pack_offset_coordinates(pos: &BlockPos) -> i16 {
        let dx = pos.get_x() & 15;
        let dy = pos.get_y() & 15;
        let dz = pos.get_z() & 15;
        (dx | (dy << 4) | (dz << 8)) as i16
    }

    /// `ProtoChunk.unpackOffsetCoordinates(short, int sectionY, ChunkPos)` —
    /// the inverse: `sectionToBlockCoord(chunkX, packed & 15)` etc. The
    /// unsigned shifts read the low 16 bits, which the `& 15` masks keep exact
    /// regardless of the sign-extension of the `short` to `int`.
    pub fn unpack_offset_coordinates(
        packed_data: i16,
        section_y: i32,
        chunk_pos: &ChunkPos,
    ) -> BlockPos {
        let packed = packed_data as i32;
        BlockPos::new(
            SectionPos::section_to_block_coord_offset(chunk_pos.x(), packed & 15),
            SectionPos::section_to_block_coord(section_y) + ((packed >> 4) & 15),
            SectionPos::section_to_block_coord_offset(chunk_pos.z(), (packed >> 8) & 15),
        )
    }

    /// `ProtoChunk.markPosForPostProcessing(BlockPos)` — appends the packed
    /// offset to the block's section when the position is inside build height.
    pub fn mark_pos_for_post_processing(&mut self, block_pos: &BlockPos) {
        if self.base.is_inside_build_height(block_pos.get_y()) {
            let section_index = self.base.get_section_index(block_pos.get_y()) as usize;
            self.base
                .get_or_create_offset_list(section_index)
                .push(Self::pack_offset_coordinates(block_pos));
        }
    }

    /// `ProtoChunk.addPackedPostProcess(ShortList, int sectionIndex)`.
    pub fn add_packed_post_process(&mut self, packed_offsets: &[i16], section_index: usize) {
        self.base
            .add_packed_post_process(packed_offsets, section_index);
    }

    /// `ProtoChunk.getPostProcessing()` — the packed offset lists.
    pub fn get_post_processing(&self) -> &[Vec<i16>] {
        self.base.get_post_processing()
    }

    /// `ProtoChunk.getBlockEntityNbts()` — `Collections.unmodifiableMap(
    /// pendingBlockEntities)`; read-only, insertion-ordered view (#537).
    pub fn get_block_entity_nbts(&self) -> &IndexMap<BlockPos, CompoundTag> {
        self.base.pending_block_entities()
    }

    /// `ProtoChunk.getBlockEntityNbtForSaving(BlockPos, HolderLookup)` — Java
    /// saves the materialized block entity or falls back to the pending tag;
    /// the block-entity map is unported, so the port returns the pending tag.
    pub fn get_block_entity_nbt_for_saving(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.base.get_block_entity_nbt(pos)
    }

    /// `ProtoChunk.removeBlockEntity(BlockPos)` — removes from `blockEntities`
    /// and `pendingBlockEntities`; the port removes the pending entry (the
    /// materialized map is unported).
    pub fn remove_block_entity(&mut self, pos: &BlockPos) {
        self.base.remove_block_entity_nbt(pos);
    }

    /// `ProtoChunk.getCarvingMask()`.
    pub fn get_carving_mask(&self) -> Option<&CarvingMask> {
        self.carving_mask.as_ref()
    }

    /// `ProtoChunk.getOrCreateCarvingMask()` — `new CarvingMask(getHeight(),
    /// getMinY())` on first access.
    pub fn get_or_create_carving_mask(&mut self) -> &mut CarvingMask {
        if self.carving_mask.is_none() {
            self.carving_mask = Some(CarvingMask::new(
                self.base.get_height(),
                self.base.get_min_y(),
            ));
        }
        self.carving_mask.as_mut().expect("just created")
    }

    /// `ProtoChunk.setCarvingMask(CarvingMask)`.
    pub fn set_carving_mask(&mut self, data: CarvingMask) {
        self.carving_mask = Some(data);
    }

    /// Take the carving mask out, leaving `None` (Rust-only helper; no Java
    /// analogue). The CARVERS driver (`NoiseBasedChunkGenerator::apply_carvers`)
    /// needs both `&mut dyn CarveChunk` (the whole chunk) and `&mut CarvingMask`
    /// (a field of the chunk) for each carve, which a single `&mut ProtoChunk`
    /// cannot provide — it takes the mask out, drives it, and writes it back
    /// with [`Self::set_carving_mask`].
    pub fn take_carving_mask(&mut self) -> Option<CarvingMask> {
        self.carving_mask.take()
    }

    /// `ProtoChunk.setLightEngine(LevelLightEngine)` — the engine is not
    /// ported, so there is no field to store.
    ///
    /// STUB(mc.world.level.lighting.engine): the `LevelLightEngine` field and
    /// the worldgen light writes it drives are deferred with the lighting
    /// engine unit (#184); a real port stores the engine here.
    pub fn set_light_engine(&mut self) {
        // No field: the light engine is not ported (#184).
    }

    /// `ChunkAccess.getUpgradeData()`.
    pub fn get_upgrade_data(&self) -> &UpgradeData {
        self.base.get_upgrade_data()
    }

    /// `ChunkAccess.getStartForStructure(Structure)` — delegate to the base's
    /// `StructureAccess` (the `i64` stand-in for the unported `StructureStart`,
    /// #369).
    pub fn get_start_for_structure(&self, structure: &S) -> Option<i64> {
        self.base.get_start_for_structure(structure)
    }

    /// `ChunkAccess.setStartForStructure(Structure, StructureStart)` — delegate
    /// (the base marks the chunk unsaved).
    pub fn set_start_for_structure(&mut self, structure: S, start: i64) {
        self.base.set_start_for_structure(structure, start);
    }

    /// `ChunkAccess.getAllStarts()` — the base's typed structure-starts
    /// authority. Java's promotion copies it wholesale
    /// (`setAllStarts(protoChunk.getAllStarts())`).
    pub fn get_all_starts(&self) -> &std::collections::HashMap<S, i64> {
        self.base.get_all_starts()
    }

    /// `ChunkAccess.setAllStarts(Map)` — clear + putAll, then marks unsaved.
    pub fn set_all_starts(&mut self, starts: std::collections::HashMap<S, i64>) {
        self.base.set_all_starts(starts);
    }

    /// `ChunkAccess.getReferencesForStructure(Structure)` — delegate.
    pub fn get_references_for_structure<'a>(
        &'a self,
        structure: &'a S,
    ) -> impl Iterator<Item = &'a u64> + 'a {
        self.base.get_references_for_structure(structure)
    }

    /// `ChunkAccess.addReferenceForStructure(Structure, long)` — delegate.
    pub fn add_reference_for_structure(&mut self, structure: S, reference: u64) {
        self.base.add_reference_for_structure(structure, reference);
    }

    /// `ChunkAccess.getAllReferences()` — the insertion-ordered runtime
    /// authority (#537).
    pub fn get_all_references(&self) -> &IndexMap<S, IndexSet<u64>> {
        self.base.get_all_references()
    }

    /// `ChunkAccess.setAllReferences(Map)` — delegate. Java's promotion copies
    /// the reference map wholesale (`setAllReferences(protoChunk.getAllReferences())`).
    pub fn set_all_references<I: IntoIterator<Item = (S, Vec<u64>)>>(&mut self, data: I) {
        self.base.set_all_references(data);
    }

    /// Consume the proto and return its `ChunkAccess` base.
    ///
    /// Java's promotion path (`new LevelChunk(ServerLevel, ProtoChunk,
    /// PostLoadProcessor)`) hands the proto's owned base state — sections,
    /// heightmaps, light nibbles, flags, inhabited time, pending block
    /// entities, post-processing, structure access — to the `LevelChunk`
    /// constructor; the port keeps that a value move. The proto-only fields
    /// (`entities`, `status`, `carvingMask`) are not part of the base and are
    /// dropped by the caller's typed refusal when the persisted status is not
    /// genuine `FULL` (see the server `LevelChunk::try_from_full_proto`).
    pub fn into_base(self) -> ChunkAccess<T, B, S> {
        self.base
    }
}

// The worldgen surface-driver specialization: `ProtoChunk.setBlockState` and
// the `ChunkSurface` seam (`build_surface`'s block-column over the real chunk,
// issue #179/#185). `BlockState` satisfies the `T` bounds of the generic
// `ProtoChunk`; the biome type `B` is opaque to the surface write (only the
// heightmap `placed` flags and section predicates are resolved, through the
// generated `BlockState` behavior table — `block_state_predicates` — exactly
// like `NoiseBasedChunkGenerator.doFill`'s `write_worldgen_block`).

impl<B, S> ProtoChunk<BlockState, B, S>
where
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `ProtoChunk.setBlockState(BlockPos, BlockState, flags)` — the worldgen
    /// write path Java's `buildSurface` `BlockColumn.setBlock` calls.
    ///
    /// Java's full `setBlockState`: the build-height guard (returns `VOID_AIR`,
    /// no write), the `wasEmpty && state.is(Blocks.AIR)` fast path, the section
    /// write, then the `getPersistedStatus().heightmapsAfter()` heightmap
    /// updates (priming missing entries first). The light half (the
    /// `status.isOrAfter(ChunkStatus.INITIALIZE_LIGHT)` branch) defers with the
    /// lighting unit (#184/#216); a worldgen `ProtoChunk` is always below that
    /// status, so the omission is unreachable on this path. The section write
    /// uses the generated `BlockState` behavior predicates
    /// ([`block_state_predicates`]), the same set `doFill` passes to
    /// `write_worldgen_block`.
    ///
    /// Java's `state.is(Blocks.AIR)` is a block-identity check
    /// (`getBlock() == Blocks.AIR`), so `CAVE_AIR`/`VOID_AIR` do not take the
    /// fast path — hence the `state.block() == Blocks::AIR.id()` comparison
    /// rather than the behavioral `is_air` predicate. The worldgen carver loop
    /// (`NoiseBasedChunkGenerator::apply_carvers`) drives this same
    /// `set_block_state` through the `CarveChunk` impl (RivetTodo(#399)) and
    /// writes `CAVE_AIR` via the aquifer, so its writes deliberately bypass this
    /// `AIR` fast path — matching Java — while the surface driver's exact `AIR`
    /// writes take it.
    ///
    /// Returns the previous state, matching Java.
    pub fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: BlockState) -> BlockState {
        if self.base.is_outside_build_height(y) {
            return self.void_air;
        }
        let section_index = self.base.get_section_index(y) as usize;
        let was_empty = self.base.get_section(section_index).has_only_air();
        let predicates = block_state_predicates();
        if was_empty && state.block() == Blocks::AIR.id() {
            return state;
        }
        let local_x = x & 15;
        let local_y = y & 15;
        let local_z = z & 15;
        let placed = self.base.resolve_flags(&state);
        let old_state = self.base.get_section_mut(section_index).set_block_state(
            local_x,
            local_y,
            local_z,
            state,
            &predicates.is_air,
            &predicates.is_randomly_ticking,
            &predicates.fluid_is_empty,
            &predicates.fluid_is_randomly_ticking,
            &predicates.is_special_colliding,
        );
        self.update_heightmaps_after(local_x, y, local_z, placed);
        old_state
    }
}

impl<B, S> ChunkSurface for ProtoChunk<BlockState, B, S>
where
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    fn get_height(&self, x: i32, z: i32) -> i32 {
        // `ChunkAccess.getHeight(WORLD_SURFACE_WG, x, z)` — Java primes a
        // missing heightmap on demand (a `&mut` read). The surface driver
        // consumes a chunk whose worldgen heightmaps are already primed
        // (`doFill` creates them), so this reads the primed entry; an absent
        // entry decodes as `minY - 1`, the exact value Java's `getHeight`
        // yields for an all-air (never-opaque) chunk (issue #185 seam).
        let min_y = self.base.get_min_y();
        match &self.base.heightmaps()[Types::WorldSurfaceWg as usize] {
            Some(hm) => hm.get_height_at(x & 15, z & 15, min_y),
            None => min_y - 1,
        }
    }

    fn get_min_y(&self) -> i32 {
        self.base.get_min_y()
    }

    fn min_block_x(&self) -> i32 {
        self.base.get_pos().get_min_block_x()
    }

    fn min_block_z(&self) -> i32 {
        self.base.get_pos().get_min_block_z()
    }

    fn is_inside_build_height(&self, y: i32) -> bool {
        self.base.is_inside_build_height(y)
    }

    fn get_block_state(&self, x: i32, y: i32, z: i32) -> BlockState {
        ProtoChunk::get_block_state(self, x, y, z)
    }

    fn set_block_state(&mut self, x: i32, y: i32, z: i32, state: BlockState) {
        ProtoChunk::set_block_state(self, x, y, z, state);
    }

    fn mark_pos_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        ProtoChunk::mark_pos_for_post_processing(self, &BlockPos::new(x, y, z));
    }
}

// The `CarveChunk` block surface is the `ChunkAccess` write surface
// `applyCarvers` needs (tracked by RivetTodo(#399)). A worldgen `ProtoChunk`
// is a freshly generated chunk, so
// `isUpgrading` is fixed `false` — Java's `ChunkAccess.isUpgrading()` reflects
// `belowZeroRetrogen != null`, which the generation path never sets.

impl<B, S> CarveChunk for ProtoChunk<BlockState, B, S>
where
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send + Sync,
{
    fn get_pos(&self) -> ChunkPos {
        ProtoChunk::get_pos(self)
    }

    fn is_upgrading(&self) -> bool {
        false
    }

    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        ProtoChunk::get_block_state(self, pos.get_x(), pos.get_y(), pos.get_z())
    }

    fn set_block_state(&mut self, pos: &BlockPos, state: BlockState) {
        ProtoChunk::set_block_state(self, pos.get_x(), pos.get_y(), pos.get_z(), state);
    }

    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        ProtoChunk::mark_pos_for_post_processing(self, pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::Climate;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::strategy::Strategy;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, WORLDGEN_HEIGHTMAPS};
    use rivet_registry::core::Vec3iLike;
    use std::cell::RefCell;

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
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8> + Send + Sync> {
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

    /// A worldgen chunk with a stone block at (0, 0, 0) of section 0 and
    /// `air = 0`, `void_air = 255`.
    fn stone_proto() -> ProtoChunk<u8, u8, &'static str> {
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
        ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            accessor(),
            &factory(),
            Some(sections),
            0,
            255,
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
    fn get_block_state_respects_build_height() {
        let proto = stone_proto();
        // Stone at absolute y -64.
        assert_eq!(proto.get_block_state(0, -64, 0), 1);
        // All-air section -> air.
        assert_eq!(proto.get_block_state(0, 0, 0), 0);
        // Outside build height -> void air (255), the guard Java uses.
        assert_eq!(proto.get_block_state(0, -65, 0), 255);
        assert_eq!(proto.get_block_state(0, 320, 0), 255);
    }

    #[test]
    fn status_defaults_to_empty_and_marks_unsaved_on_set() {
        let mut proto = stone_proto();
        assert_eq!(proto.get_persisted_status(), ChunkStatus::Empty);
        assert!(!proto.base.is_unsaved());
        proto.set_persisted_status(ChunkStatus::Full);
        assert_eq!(proto.get_persisted_status(), ChunkStatus::Full);
        assert!(proto.base.is_unsaved());
    }

    #[test]
    fn entities_carrier_round_trips() {
        let mut proto = stone_proto();
        assert!(proto.get_entities().is_empty());
        let mut tag = CompoundTag::new();
        tag.put_string("id", "minecraft:pig");
        proto.add_entity(tag);
        assert_eq!(proto.get_entities().len(), 1);
    }

    #[test]
    fn pack_unpack_offset_coordinates_round_trip() {
        // Chunk (1, -2), section y 3. A block at absolute x 21 (offset 5),
        // y 61 (section-relative 13), z -27 (offset 5).
        let pos = BlockPos::new(21, 61, -27);
        let packed = ProtoChunk::<u8, u8, &str>::pack_offset_coordinates(&pos);
        assert_eq!(packed, 0x05D5); // x 5 | y 13<<4 | z 5<<8.
        let chunk_pos = ChunkPos::new(1, -2);
        let unpacked = ProtoChunk::<u8, u8, &str>::unpack_offset_coordinates(packed, 3, &chunk_pos);
        assert_eq!(unpacked.coords(), (21, 61, -27));
        // A block in a negative chunk re-anchors to that chunk's min block,
        // so the unpack needs the block's own chunk. (-3, 61, -4) lives in
        // chunk (-1, -1) (both coords floor at >> 4).
        let pos2 = BlockPos::new(-3, 61, -4);
        let packed2 = ProtoChunk::<u8, u8, &str>::pack_offset_coordinates(&pos2);
        assert_eq!(packed2, 0x0CDD); // x 13 | y 13<<4 | z 12<<8.
        let chunk_pos2 = ChunkPos::new(-1, -1);
        let unpacked2 =
            ProtoChunk::<u8, u8, &str>::unpack_offset_coordinates(packed2, 3, &chunk_pos2);
        assert_eq!(unpacked2.coords(), (-3, 61, -4));
    }

    #[test]
    fn mark_pos_for_post_processing_guards_build_height() {
        let mut proto = stone_proto();
        proto.mark_pos_for_post_processing(&BlockPos::new(3, 2, 5)); // y=2 inside.
        proto.mark_pos_for_post_processing(&BlockPos::new(0, 320, 0)); // outside.
        let offsets = proto.get_post_processing();
        assert_eq!(offsets.len(), 24);
        assert_eq!(offsets[4].len(), 1); // y=2 is section index 4.
        assert!(offsets[23].is_empty());
        // The packed offset for (3, 2, 5): x 3 | y 2<<4 | z 5<<8.
        assert_eq!(offsets[4][0], 0x0523);
    }

    #[test]
    fn carving_mask_reuses_the_existing_mask() {
        let mut proto = stone_proto();
        assert!(proto.get_carving_mask().is_none());
        proto.get_or_create_carving_mask().set(1, -64, 2);
        let mask = proto.get_carving_mask().expect("created");
        assert!(mask.get(1, -64, 2));
        // `getOrCreateCarvingMask` reuses the same mask.
        proto.get_or_create_carving_mask();
        assert!(proto.get_carving_mask().expect("created").get(1, -64, 2));
    }

    #[test]
    fn no_heightmaps_primed_by_worldgen_constructor() {
        // The `ProtoChunk` constructor calls the `ChunkAccess` constructor
        // (Java `super(...)`) and primes nothing: heightmap priming is done by
        // the `LevelChunk` FULL-status constructor only. So every heightmap
        // entry is absent, even the `FINAL_HEIGHTMAPS`.
        let proto = stone_proto();
        assert!(
            proto.base.heightmaps().iter().all(Option::is_none),
            "worldgen constructor must not prime any heightmap"
        );
    }

    #[test]
    fn get_noise_biome_reads_through_the_base() {
        let proto = stone_proto();
        // All default biomes (0): the base clamp path returns 0 everywhere.
        assert_eq!(proto.get_noise_biome(0, -16, 0), 0);
        assert_eq!(proto.get_noise_biome(0, 76, 0), 0);
    }

    #[test]
    fn update_heightmaps_after_dispatches_on_persisted_status() {
        // `ChunkStatus.EMPTY`'s `heightmapsAfter()` is the two
        // `WORLDGEN_HEIGHTMAPS`; `FULL`'s is `FINAL_HEIGHTMAPS`. The worldgen
        // constructor primed nothing, so the first update primes (creates) the
        // status's types and leaves the other four absent.
        let mut empty = stone_proto();
        assert_eq!(empty.get_persisted_status(), ChunkStatus::Empty);
        empty.update_heightmaps_after(
            0,
            -64,
            0,
            StateFlags {
                is_air: false,
                blocks_motion: true,
                has_fluid: false,
                is_leaves: false,
            },
        );
        for ty in WORLDGEN_HEIGHTMAPS {
            assert!(empty.base.has_primed_heightmap(ty), "EMPTY primes {ty:?}");
        }
        for ty in FINAL_HEIGHTMAPS {
            assert!(!empty.base.has_primed_heightmap(ty), "EMPTY skips {ty:?}");
        }
        // A `FULL`-status chunk updates the four `FINAL_HEIGHTMAPS` instead.
        let mut full = stone_proto();
        full.set_persisted_status(ChunkStatus::Full);
        full.update_heightmaps_after(
            0,
            -64,
            0,
            StateFlags {
                is_air: false,
                blocks_motion: true,
                has_fluid: false,
                is_leaves: false,
            },
        );
        for ty in FINAL_HEIGHTMAPS {
            assert!(full.base.has_primed_heightmap(ty), "FULL primes {ty:?}");
        }
        for ty in WORLDGEN_HEIGHTMAPS {
            assert!(!full.base.has_primed_heightmap(ty), "FULL skips {ty:?}");
        }
    }

    /// A `BiomeResolver` that records every quart request in order.
    struct RecordingResolver(RefCell<Vec<(i32, i32, i32)>>);

    impl BiomeResolver for RecordingResolver {
        fn get_noise_biome(
            &self,
            quart_x: i32,
            quart_y: i32,
            quart_z: i32,
            _sampler: &Sampler,
        ) -> Holder<BiomeId> {
            self.0.borrow_mut().push((quart_x, quart_y, quart_z));
            Holder::direct(BiomeId::from_id(0))
        }
    }

    fn map_biome(holder: &Holder<BiomeId>) -> u8 {
        match holder {
            Holder::Direct(biome) => biome.id() as u8,
            Holder::Reference { id, .. } => *id as u8,
        }
    }

    /// `ProtoChunk.fillBiomesFromNoise` forwards to the base (`ProtoChunk`
    /// inherits `ChunkAccess.fillBiomesFromNoise` in Java): the same quart
    /// routing drives the base's section fill.
    #[test]
    fn fill_biomes_from_noise_forwards_to_the_base() {
        let resolver = RecordingResolver(RefCell::new(Vec::new()));
        let mut proto = stone_proto();
        let sampler = Climate::empty();
        proto.fill_biomes_from_noise(&resolver, &sampler, &map_biome);

        let calls = resolver.0.into_inner();
        assert_eq!(calls.len(), 24 * 64);
        // ChunkPos::ZERO: quartMinX = 0, quartMinZ = 0; bottom section (Y -4)
        // has quartMinY = -16.
        assert_eq!(calls.first().copied(), Some((0, -16, 0)));
        assert_eq!(calls.last().copied(), Some((3, 79, 3)));
    }
}

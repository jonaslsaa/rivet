//! Port of `net.minecraft.world.level.chunk.ChunkAccess` (MC 26.2) — the
//! in-memory chunk base: sections, heightmaps, structure starts/references,
//! post-processing offsets, inhabited time, unsaved/light-correct flags, the
//! pending block-entity NBT map, and the read spine (`getNoiseBiome`,
//! `findBlocks`, `isYSpaceEmpty`, section access).
//!
//! Java: `ChunkAccess.java` in `working/Paper`. The class is the abstract
//! supertype of `LevelChunk`/`ProtoChunk`/`EmptyLevelChunk`. The port models
//! it as a generic value struct `ChunkAccess<T, B, S>` — `T` the block-state
//! type, `B` the biome type, `S` the caller's structure key (the
//! `StructureAccess<S>` map key, standing in for the unported `Structure`) —
//! and the concrete chunk types *contain* a base (OWNERSHIP.md — no
//! inheritance in the port). The base fixes no behavior: the abstract
//! `getBlockState`/`setBlockState`/`getPersistedStatus` live on the concrete
//! types, matching Java's abstract surface.
//!
//! Heightmaps are keyed on the world `Heightmap.Types` ordinal (declaration)
//! order as a fixed `[Option<Heightmap>; 6]` — never the protocol enum's
//! variant index (see `levelgen::heightmap`). `LevelChunk`'s constructor
//! creates the four `FINAL_HEIGHTMAPS` entries unprimed.
//!
//! Deferred with their owning units:
//! - the `blockEntities` map and `setBlockEntity`/`getBlockEntity` (the block-
//!   entity unit, `mc.world.level.block.entity`); the port carries only the
//!   `pendingBlockEntities` NBT map;
//! - the Starlight light arrays (`blockNibbles`/`skyNibbles`/emptiness maps),
//!   `setBlockEntityNbt`'s `!blockEntities.containsKey` guard, and the
//!   `blendingData` field — all with the lighting/blending units (#184);
//! - the `tick`/`getBlockTicks`/`getFluidTicks`/`getTicksForSerialization`
//!   surface and the `PackedTicks` record (the `world.ticks` unit);
//! - `getBlockState`/`setBlockState` — overridden per concrete type;
//!   `setBlockState`'s mutators defer with #216.
//!
//! RivetTodo(#184): the Starlight light arrays and `blendingData` are not
//! ported — `LevelLightEngine`/`StarLightEngine` live with the
//! `mc.world.level.lighting` units and `BlendingData` with the blending unit.
//! Issue #287 Part A adds `getHeight`'s on-demand `primeHeightmaps` and the
//! per-block `update` walk: [`get_height_at`] primes a missing entry
//! (Java `Heightmap.primeHeightmaps(this, EnumSet.of(type))`) and
//! [`prime_heightmaps`]/[`update_heightmaps_after`] drive the worldgen/live
//! compute. The per-state predicates resolve through the caller-supplied
//! `resolve: &dyn Fn(&T) -> StateFlags` (the world sites use `rivet-registry`'s
//! `BlockState` behavior table; the superflat/server sites supply their own
//! flag predicates), so no predicate is stored on the base (OWNERSHIP.md).
//! RivetTodo(#185): the
//! `mc.world.level.chunk.status` unit replaces the slice-local `ChunkStatus`
//! with the real status ladder and `isOrAfter`. RivetTodo(#216): the
//! `setBlockState` mutators (section set-block/fluid) defer with the
//! chunk-storage epic — the heightmap `update` half is ported here
//! ([`update_heightmaps_after`]).

use std::collections::HashMap;

use indexmap::IndexSet;

use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::light_chunk::LightChunk;
use crate::chunk::paletted_container_factory::PalettedContainerFactory;
use crate::chunk::structure_access::StructureAccess;
use crate::chunk::upgrade_data::UpgradeData;
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::{Heightmap, StateFlags, Types};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};

/// `ChunkAccess.NO_FILLED_SECTION` — `getHighestFilledSectionIndex()` for a
/// chunk with no non-air section.
pub const NO_FILLED_SECTION: i32 = -1;

/// `net.minecraft.world.level.chunk.status.ChunkStatus` — the persisted chunk
/// status, slice-local: the real status ladder (with `isOrAfter`/`heightmapsAfter`)
/// lives in the `mc.world.level.chunk.status` unit (#185). This slice needs
/// the two statuses its chunks reach — a fresh `ProtoChunk` is `EMPTY`, a
/// loaded `LevelChunk`/`EmptyLevelChunk` is `FULL`.
///
/// RivetTodo(#185): the full `ChunkStatus` ladder (and `BelowZeroRetrogen`,
/// which `getHighestGeneratedStatus` folds in) is not ported; this enum is a
/// stand-in the status unit replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkStatus {
    /// `ChunkStatus.EMPTY` — the `ProtoChunk` default.
    Empty,
    /// `ChunkStatus.FULL` — `LevelChunk`/`EmptyLevelChunk`.
    Full,
}

/// `net.minecraft.world.level.chunk.ChunkAccess` — the generic base value.
pub struct ChunkAccess<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `chunkPos` (plus Paper's cached `coordinateKey`/`locX`/`locZ`).
    pos: ChunkPos,
    /// `upgradeData`.
    upgrade_data: UpgradeData,
    /// `levelHeightAccessor` — the world's vertical extent (the `Level`
    /// reference Java stores is resolved through the accessor value).
    height_accessor: SimpleLevelHeightAccessor,
    /// `sections` — one `LevelChunkSection` per world section, lowest Y first.
    sections: Vec<LevelChunkSection<T, B>>,
    /// `postProcessing` — per-section packed post-processing offsets (a
    /// `ShortList[]` in Java; the port models every section as an empty list
    /// rather than `null` — the null/empty distinction is only observable
    /// through `getPostProcessing()[i] == null`, which the deferred
    /// `postProcessGeneration` skips either way).
    post_processing: Vec<Vec<i16>>,
    /// `unsaved`.
    unsaved: bool,
    /// `isLightCorrect`.
    light_correct: bool,
    /// `inhabitedTime`.
    inhabited_time: i64,
    /// `structureStarts`/`structuresRefences` — the `StructureAccess`
    /// implementation.
    structure_access: StructureAccess<S>,
    /// `pendingBlockEntities` — block-entity NBT awaiting materialization.
    pending_block_entities: HashMap<BlockPos, CompoundTag>,
    /// `heightmaps` — the `EnumMap<Heightmap.Types, Heightmap>`, keyed by the
    /// world `Types` ordinal (see the module doc).
    heightmaps: [Option<Heightmap>; 6],
    /// The per-state behavior flags the heightmap predicates need. Java's
    /// `Heightmap` holds a `ChunkAccess` and calls `state.isAir()`/
    /// `state.blocksMotion()`/`getFluidState()`/`instanceof LeavesBlock`; the
    /// base stores the caller's `&dyn Fn(&T) -> StateFlags` so `getHeight`'s
    /// on-demand prime and the `update` walk can classify (OWNERSHIP.md — no
    /// stored `&ChunkAccess`). The resolver must be `Sync` (in addition to
    /// `Send`) so a chunk stays `Send`: `ChunkMap` moves chunks to the tick
    /// thread. The concrete closures are stateless.
    resolve: &'static (dyn Fn(&T) -> StateFlags + Sync),
}

impl<T, B, S> ChunkAccess<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `ChunkAccess(ChunkPos, UpgradeData, LevelHeightAccessor,
    /// PalettedContainerFactory, long inhabitedTime, LevelChunkSection[]
    /// sections, BlendingData)` — the base constructor.
    ///
    /// `sections` is the caller's section array (`None` for Java's `null`):
    /// when present with the accessor's section count it is adopted, otherwise
    /// Java logs a warning and keeps the default array. Either way every
    /// section is filled with a default air container via `replaceMissingSections`
    /// (Java's `LevelChunkSection(containerFactory, level, chunkPos, sectionY)`).
    /// The factory's air default is guaranteed all-air, so the replacement
    /// sections are `new_all_air` (issue #216) — no recalc predicates needed.
    /// `blendingData` is omitted (deferred, #184).
    ///
    /// `resolve` classifies states for the heightmap predicates (`isOpaque`).
    /// Java reaches the same flags through `BlockState` methods; the port
    /// stores the caller's stateless predicate so on-demand and live heightmap
    /// updates can classify section states.
    pub fn new(
        pos: ChunkPos,
        upgrade_data: UpgradeData,
        height_accessor: SimpleLevelHeightAccessor,
        container_factory: &PalettedContainerFactory<T, B>,
        inhabited_time: i64,
        sections: Option<Vec<LevelChunkSection<T, B>>>,
        resolve: &'static (dyn Fn(&T) -> StateFlags + Sync),
    ) -> Self {
        let count = height_accessor.get_sections_count() as usize;
        let mut sections_vec = match sections {
            Some(s) if s.len() == count => s,
            Some(s) => {
                // Java logs a warning and keeps the all-default array.
                eprintln!(
                    "Could not set level chunk sections, array length is {} instead of {count}",
                    s.len()
                );
                Vec::new()
            }
            None => Vec::new(),
        };
        for _ in sections_vec.len()..count {
            sections_vec.push(LevelChunkSection::new_all_air(
                container_factory.create_for_block_states(),
                container_factory.create_for_biomes(),
            ));
        }
        ChunkAccess {
            pos,
            upgrade_data,
            height_accessor,
            sections: sections_vec,
            post_processing: vec![Vec::new(); count],
            unsaved: false,
            light_correct: false,
            inhabited_time,
            structure_access: StructureAccess::new(),
            pending_block_entities: HashMap::new(),
            heightmaps: [None, None, None, None, None, None],
            resolve,
        }
    }

    /// `ChunkAccess.getPos()`.
    pub fn get_pos(&self) -> ChunkPos {
        self.pos
    }

    /// `ChunkAccess.getUpgradeData()`.
    pub fn get_upgrade_data(&self) -> &UpgradeData {
        &self.upgrade_data
    }

    /// `ChunkAccess.getMinY()` — `levelHeightAccessor.getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.height_accessor.get_min_y()
    }

    /// `ChunkAccess.getHeight()` — `levelHeightAccessor.getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.height_accessor.get_height()
    }

    /// The contained `levelHeightAccessor` value (Java's `LevelHeightAccessor`
    /// field; exposed so a wrapping chunk can share its vertical extent).
    pub fn height_accessor(&self) -> SimpleLevelHeightAccessor {
        self.height_accessor
    }

    /// `LevelHeightAccessor.getMaxY()`.
    pub fn get_max_y(&self) -> i32 {
        self.height_accessor.get_max_y()
    }

    /// `LevelHeightAccessor.isOutsideBuildHeight(int)`.
    pub fn is_outside_build_height(&self, block_y: i32) -> bool {
        self.height_accessor.is_outside_build_height(block_y)
    }

    /// `LevelHeightAccessor.isInsideBuildHeight(int)`.
    pub fn is_inside_build_height(&self, block_y: i32) -> bool {
        self.height_accessor.is_inside_build_height(block_y)
    }

    /// `ChunkAccess.getSections()`.
    pub fn get_sections(&self) -> &[LevelChunkSection<T, B>] {
        &self.sections
    }

    /// `ChunkAccess.getSection(int sectionIndex)` — indexes the array (Java
    /// throws `ArrayIndexOutOfBoundsException` out of range; the port panics).
    pub fn get_section(&self, section_index: usize) -> &LevelChunkSection<T, B> {
        &self.sections[section_index]
    }

    /// `ChunkAccess.getSectionIndex(int blockY)` — delegates to the accessor.
    pub fn get_section_index(&self, block_y: i32) -> i32 {
        self.height_accessor.get_section_index(block_y)
    }

    /// `ChunkAccess.getSectionIndexFromSectionY(int)`.
    pub fn get_section_index_from_section_y(&self, section_y: i32) -> i32 {
        self.height_accessor
            .get_section_index_from_section_y(section_y)
    }

    /// `ChunkAccess.getSectionYFromSectionIndex(int)`.
    pub fn get_section_y_from_section_index(&self, section_index: i32) -> i32 {
        self.height_accessor
            .get_section_y_from_section_index(section_index)
    }

    /// `ChunkAccess.getHighestFilledSectionIndex()` — the highest section with
    /// any non-air block, or `NO_FILLED_SECTION`.
    pub fn get_highest_filled_section_index(&self) -> i32 {
        for (index, section) in self.sections.iter().enumerate().rev() {
            if !section.has_only_air() {
                return index as i32;
            }
        }
        NO_FILLED_SECTION
    }

    /// `ChunkAccess.isYSpaceEmpty(int yStartInclusive, int yEndInclusive)` —
    /// every section overlapping `[start, end]` (clamped to build height) is
    /// all-air.
    pub fn is_y_space_empty(&self, y_start_inclusive: i32, y_end_inclusive: i32) -> bool {
        let mut y_start = y_start_inclusive;
        let mut y_end = y_end_inclusive;
        if y_start < self.get_min_y() {
            y_start = self.get_min_y();
        }
        if y_end > self.get_max_y() {
            y_end = self.get_max_y();
        }
        let mut y = y_start;
        while y <= y_end {
            if !self
                .get_section(self.get_section_index(y) as usize)
                .has_only_air()
            {
                return false;
            }
            y += 16;
        }
        true
    }

    /// `ChunkAccess.getHeightmaps()` — the whole `EnumMap` (read-only).
    pub fn heightmaps(&self) -> &[Option<Heightmap>; 6] {
        &self.heightmaps
    }

    /// `ChunkAccess.getOrCreateHeightmapUnprimed(Types)` — `computeIfAbsent`:
    /// returns the existing heightmap or creates one (all-zero storage).
    pub fn get_or_create_heightmap_unprimed(&mut self, ty: Types) -> &mut Heightmap {
        self.heightmaps[ty as usize]
            .get_or_insert_with(|| Heightmap::new(self.height_accessor.get_height()))
    }

    /// `ChunkAccess.hasPrimedHeightmap(Types)` — whether an entry exists
    /// (Java's "primed" = an entry is present).
    pub fn has_primed_heightmap(&self, ty: Types) -> bool {
        self.heightmaps[ty as usize].is_some()
    }

    /// `Heightmap.update`'s `chunk.getBlockState(x, y, z)` read — resolves the
    /// per-state flags at an absolute `(x, y, z)` through the section stack
    /// (Java reads the concrete chunk's `getBlockState`; the base has the
    /// sections and the caller's resolver).
    fn flags_at(&self, x: i32, y: i32, z: i32) -> StateFlags {
        flags_at(&self.sections, &self.height_accessor, self.resolve, x, y, z)
    }

    /// `ChunkAccess.primeHeightmaps(Set<Types>)` — the on-demand and
    /// `setBlockState` priming walk (Java `Heightmap.primeHeightmaps`): for
    /// each of the requested types, walk every column from the highest filled
    /// section down to `getMinY()`, and set the first `isOpaque` block's
    /// `y + 1` as the column height. A column with no opaque block keeps its
    /// entry at 0 (decodes as `minY`).
    pub fn prime_heightmaps(&mut self, types: &[Types]) {
        if types.is_empty() {
            return;
        }
        let min_y = self.get_min_y();
        let highest_section_position = match self.get_highest_filled_section_index() {
            NO_FILLED_SECTION => min_y + 15,
            index => {
                let section_y = self.get_section_y_from_section_index(index);
                SectionPos::section_to_block_coord(section_y + 1) - 1
            }
        };
        for ty in types {
            self.get_or_create_heightmap_unprimed(*ty);
        }
        for x in 0..16 {
            for z in 0..16 {
                for ty in types {
                    for y in (min_y..=highest_section_position).rev() {
                        let flags = self.flags_at(x, y, z);
                        if Heightmap::is_opaque(*ty, flags) {
                            self.heightmaps[*ty as usize]
                                .as_mut()
                                .expect("just created")
                                .set_height(x, z, y + 1, min_y);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// `ChunkAccess.getHeight(Types, x, z)` — `getFirstAvailable(x & 15, z & 15)
    /// - 1`. Java primes a missing heightmap on demand
    /// (`Heightmap.primeHeightmaps(this, EnumSet.of(type))`) and logs in IDE
    /// for a `LevelChunk`; the port primes without the IDE-only log.
    ///
    /// Named `get_height_at` (Java has two `getHeight` overloads — the
    /// no-arg `LevelHeightAccessor` accessor and this heightmap read; Rust
    /// cannot overload, so the heightmap read takes the `_at` suffix like
    /// `LevelChunk::get_height_at`).
    pub fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        if self.heightmaps[ty as usize].is_none() {
            self.prime_heightmaps(&[ty]);
        }
        let min_y = self.get_min_y();
        self.heightmaps[ty as usize]
            .as_ref()
            .map_or(min_y - 1, |hm| hm.get_height_at(x & 15, z & 15, min_y))
    }

    /// `ChunkAccess.getHighestSectionPosition()` — `getMinY()` when no section
    /// is filled, else `sectionToBlockCoord(getSectionYFromSectionIndex(
    /// getHighestFilledSectionIndex()))`.
    pub fn get_highest_section_position(&self) -> i32 {
        match self.get_highest_filled_section_index() {
            NO_FILLED_SECTION => self.get_min_y(),
            index => {
                let section_y = self.get_section_y_from_section_index(index);
                SectionPos::section_to_block_coord(section_y)
            }
        }
    }

    /// `setBlockState`'s heightmap `update` loop — Java walks
    /// `getPersistedStatus().heightmapsAfter()` and calls `update(localX, y,
    /// localZ, state)` on each present heightmap, priming missing entries
    /// first. The port's `setBlockState` (the #216 section write half) will
    /// call this after the section write; `types` is the caller's
    /// `heightmapsAfter()` set (`WORLDGEN_HEIGHTMAPS` or `FINAL_HEIGHTMAPS`).
    pub fn update_heightmaps_after(
        &mut self,
        types: &[Types],
        local_x: i32,
        y: i32,
        local_z: i32,
        placed: StateFlags,
    ) {
        let missing: Vec<Types> = types
            .iter()
            .copied()
            .filter(|ty| self.heightmaps[*ty as usize].is_none())
            .collect();
        if !missing.is_empty() {
            self.prime_heightmaps(&missing);
        }
        let min_y = self.get_min_y();
        // The update walk's re-scan (`Heightmap.update`'s downward `getBlockState`
        // read) resolves flags through the free `flags_at` so it only borrows the
        // sections/accessor, leaving the heightmap entry (mutably borrowed by
        // `.update`) unborrowed.
        let (sections, accessor, resolve) = (&self.sections, &self.height_accessor, self.resolve);
        for ty in types {
            self.heightmaps[*ty as usize]
                .as_mut()
                .expect("primed above")
                .update(local_x, y, local_z, *ty, placed, min_y, |abs_y| {
                    flags_at(sections, accessor, resolve, local_x, abs_y, local_z)
                });
        }
    }

    /// `ChunkAccess.setHeightmap(Types, long[])` — `getOrCreateHeightmapUnprimed(key)
    /// .setRawData(this, key, data)`.
    pub fn set_heightmap(&mut self, key: Types, data: &[i64]) {
        self.get_or_create_heightmap_unprimed(key)
            .set_raw_data(data);
    }

    /// `ChunkAccess.getNoiseBiome(int, int, int)` — Paper's get-block-chunk
    /// optimisation: `sectionY = (quartY >> 2) - minSection`, with the quart
    /// relative Y clamped to the section stack and the section's `getNoiseBiome`
    /// called with masked quart coords. `minSection` is
    /// `WorldUtil.getMinSection(levelHeightAccessor)` = `getMinSectionY()`.
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> B {
        let min_section = self.height_accessor.get_min_section_y();
        let mut section_y = (quart_y >> 2) - min_section;
        let mut rel = quart_y & 3;
        let len = self.sections.len() as i32;
        if section_y < 0 {
            section_y = 0;
            rel = 0;
        } else if section_y >= len {
            section_y = len - 1;
            rel = 3;
        }
        self.sections[section_y as usize].get_noise_biome(quart_x & 3, rel, quart_z & 3)
    }

    /// `ChunkAccess.getBlockEntityNbt(BlockPos)` — `pendingBlockEntities.get(pos)`.
    pub fn get_block_entity_nbt(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.pending_block_entities.get(pos)
    }

    /// `pendingBlockEntities` — the read-only map (`ProtoChunk.getBlockEntityNbts`
    /// returns `Collections.unmodifiableMap(...)`).
    pub fn pending_block_entities(&self) -> &HashMap<BlockPos, CompoundTag> {
        &self.pending_block_entities
    }

    /// `ProtoChunk.removeBlockEntity(BlockPos)`'s pending half —
    /// `pendingBlockEntities.remove(pos)` (the `blockEntities` half is not
    /// ported with the block-entity unit).
    pub fn remove_block_entity_nbt(&mut self, pos: &BlockPos) -> Option<CompoundTag> {
        self.pending_block_entities.remove(pos)
    }

    /// `ChunkAccess.setBlockEntityNbt(CompoundTag)` — computes the position
    /// from the tag (`BlockEntity.getPosFromTag`) and stores the NBT pending.
    /// Java's `!blockEntities.containsKey(posFromTag)` guard is omitted with
    /// the block-entity map (#216).
    pub fn set_block_entity_nbt(&mut self, entity_tag: CompoundTag) {
        let pos_from_tag = get_pos_from_tag(Some(&self.pos), &entity_tag);
        self.pending_block_entities.insert(pos_from_tag, entity_tag);
    }

    /// `ChunkAccess.getPostProcessing()` — the per-section packed-offset lists.
    pub fn get_post_processing(&self) -> &[Vec<i16>] {
        &self.post_processing
    }

    /// `ChunkAccess.getOrCreateOffsetList(ShortList[], int)` — the growable
    /// per-section list. Java distinguishes `null` (absent) from empty; the
    /// port's `Vec<Vec<i16>>` always has an entry, so this is an indexed
    /// `&mut` into the existing list.
    pub fn get_or_create_offset_list(&mut self, section_index: usize) -> &mut Vec<i16> {
        &mut self.post_processing[section_index]
    }

    /// `ChunkAccess.addPackedPostProcess(ShortList, int)`.
    pub fn add_packed_post_process(&mut self, packed_offsets: &[i16], section_index: usize) {
        self.get_or_create_offset_list(section_index)
            .extend_from_slice(packed_offsets);
    }

    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — the base warns and
    /// does nothing (`ProtoChunk` overrides it).
    pub fn mark_pos_for_post_processing(&self, _block_pos: &BlockPos) {
        // Java's `LOGGER.warn("... but this operation is not supported.")`.
        eprintln!(
            "Trying to mark a block for post processing, but this operation is not supported."
        );
    }

    /// `ChunkAccess.findBlocks(Predicate<BlockState>, BiConsumer<BlockPos,
    /// BlockState>)` — visits every block matching `predicate` with its
    /// absolute position.
    pub fn find_blocks(
        &self,
        predicate: &dyn Fn(&T) -> bool,
        mut consumer: impl FnMut(BlockPos, T),
    ) {
        let min_section_y = self.height_accessor.get_min_section_y();
        let max_section_y = self.height_accessor.get_max_section_y();
        for section_y in min_section_y..=max_section_y {
            let section_index = self.get_section_index_from_section_y(section_y);
            let section = &self.sections[section_index as usize];
            if section.states().maybe_has(predicate) {
                let origin = SectionPos::of_chunk_pos(&self.pos, section_y).origin();
                for y in 0..16 {
                    for z in 0..16 {
                        for x in 0..16 {
                            let state = section.get_block_state(x, y, z);
                            if predicate(&state) {
                                consumer(origin.offset(x, y, z), state);
                            }
                        }
                    }
                }
            }
        }
    }

    /// `ChunkAccess.findBlockLightSources(BiConsumer<BlockPos, BlockState>)` —
    /// `findBlocks(state -> state.getLightEmission() != 0, consumer)`; the
    /// light-emitting predicate is resolved per state by the caller.
    pub fn find_block_light_sources(
        &self,
        is_light_source: &dyn Fn(&T) -> bool,
        consumer: impl FnMut(BlockPos, T),
    ) {
        self.find_blocks(is_light_source, consumer);
    }

    /// `ChunkAccess.markUnsaved()`.
    pub fn mark_unsaved(&mut self) {
        self.unsaved = true;
    }

    /// `ChunkAccess.tryMarkSaved()` — clears `unsaved`, reporting whether it
    /// was set.
    pub fn try_mark_saved(&mut self) -> bool {
        if self.unsaved {
            self.unsaved = false;
            true
        } else {
            false
        }
    }

    /// `ChunkAccess.isUnsaved()` (without the CraftBukkit PDC dirty flag).
    pub fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    /// `ChunkAccess.isLightCorrect()`.
    pub fn is_light_correct(&self) -> bool {
        self.light_correct
    }

    /// `ChunkAccess.setLightCorrect(boolean)` — sets the flag and marks unsaved.
    pub fn set_light_correct(&mut self, light_correct: bool) {
        self.light_correct = light_correct;
        self.mark_unsaved();
    }

    /// `ChunkAccess.getInhabitedTime()`.
    pub fn get_inhabited_time(&self) -> i64 {
        self.inhabited_time
    }

    /// `ChunkAccess.incrementInhabitedTime(long)`.
    pub fn increment_inhabited_time(&mut self, inhabited_time_delta: i64) {
        self.inhabited_time += inhabited_time_delta;
    }

    /// `ChunkAccess.setInhabitedTime(long)`.
    pub fn set_inhabited_time(&mut self, inhabited_time: i64) {
        self.inhabited_time = inhabited_time;
    }

    /// `ChunkAccess.canBeSerialized()` — the base returns `true`
    /// (`ImposterProtoChunk` overrides it to `false`).
    pub fn can_be_serialized(&self) -> bool {
        true
    }

    /// `ChunkAccess.getStartForStructure(Structure)` — delegate to the
    /// structure access (no `markUnsaved` on reads).
    pub fn get_start_for_structure(&self, structure: &S) -> Option<i64> {
        self.structure_access.get_start_for_structure(structure)
    }

    /// `ChunkAccess.setStartForStructure(Structure, StructureStart)` — sets
    /// and marks unsaved.
    pub fn set_start_for_structure(&mut self, structure: S, start: i64) {
        self.structure_access
            .set_start_for_structure(structure, start);
        self.mark_unsaved();
    }

    /// `ChunkAccess.getAllStarts()`.
    pub fn get_all_starts(&self) -> &HashMap<S, i64> {
        self.structure_access.get_all_starts()
    }

    /// `ChunkAccess.setAllStarts(Map)` — clear + putAll, then marks unsaved.
    pub fn set_all_starts(&mut self, starts: HashMap<S, i64>) {
        self.structure_access.set_all_starts(starts);
        self.mark_unsaved();
    }

    /// `ChunkAccess.getReferencesForStructure(Structure)`.
    pub fn get_references_for_structure<'a>(
        &'a self,
        structure: &'a S,
    ) -> impl Iterator<Item = &'a u64> + 'a {
        self.structure_access
            .get_references_for_structure(structure)
    }

    /// `ChunkAccess.addReferenceForStructure(Structure, long)` — adds and
    /// marks unsaved.
    pub fn add_reference_for_structure(&mut self, structure: S, reference: u64) {
        self.structure_access
            .add_reference_for_structure(structure, reference);
        self.mark_unsaved();
    }

    /// `ChunkAccess.getAllReferences()`.
    pub fn get_all_references(&self) -> &HashMap<S, IndexSet<u64>> {
        self.structure_access.get_all_references()
    }

    /// `ChunkAccess.setAllReferences(Map)` — clear + putAll, then marks
    /// unsaved.
    pub fn set_all_references(&mut self, data: HashMap<S, Vec<u64>>) {
        self.structure_access.set_all_references(data);
        self.mark_unsaved();
    }
}

impl<T, B, S> LightChunk<T> for ChunkAccess<T, B, S>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `findBlockLightSources` — delegates to the inherent `find_blocks` walk.
    fn find_block_light_sources(
        &self,
        is_light_source: &dyn Fn(&T) -> bool,
        consumer: impl FnMut(BlockPos, T),
    ) {
        self.find_blocks(is_light_source, consumer);
    }
}

/// `Heightmap`'s `chunk.getBlockState(x, y, z)` read, resolved to the per-state
/// behavior flags. Takes the section stack, accessor, and resolver as
/// parameters so `update_heightmaps_after` can call it while a heightmap entry
/// is mutably borrowed (the free function borrows only the sections).
fn flags_at<T, B>(
    sections: &[LevelChunkSection<T, B>],
    height_accessor: &SimpleLevelHeightAccessor,
    resolve: &(dyn Fn(&T) -> StateFlags + Sync),
    x: i32,
    y: i32,
    z: i32,
) -> StateFlags
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
{
    if height_accessor.is_outside_build_height(y) {
        // `Blocks.VOID_AIR` — `isAir`, not opaque.
        return StateFlags {
            is_air: true,
            blocks_motion: false,
            has_fluid: false,
            is_leaves: false,
        };
    }
    let index = height_accessor.get_section_index(y);
    let section = &sections[index as usize];
    if section.has_only_air() {
        // `Blocks.AIR` — the same `isAir`, not opaque, classification.
        return StateFlags {
            is_air: true,
            blocks_motion: false,
            has_fluid: false,
            is_leaves: false,
        };
    }
    let state = section.get_block_state(x & 15, y & 15, z & 15);
    resolve(&state)
}

/// `BlockEntity.getPosFromTag(ChunkPos base, CompoundTag)` — the position a
/// block-entity tag stores, with Paper's wrong-chunk correction: when the
/// tag's `x`/`z` land in a different chunk than `base`, they are re-anchored
/// to `base`'s chunk using the section-relative offsets. A `None` base skips
/// the correction (Paper's nullable base, used for items).
pub fn get_pos_from_tag(base: Option<&ChunkPos>, entity_tag: &CompoundTag) -> BlockPos {
    let mut x = entity_tag.get_int_or("x", 0);
    let y = entity_tag.get_int_or("y", 0);
    let mut z = entity_tag.get_int_or("z", 0);
    if let Some(base) = base {
        let section_x = SectionPos::block_to_section_coord(x);
        let section_z = SectionPos::block_to_section_coord(z);
        if section_x != base.x() || section_z != base.z() {
            x = base.get_block_x(SectionPos::section_relative(x));
            z = base.get_block_z(SectionPos::section_relative(z));
        }
    }
    BlockPos::new(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::strategy::Strategy;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, Types};

    /// A value-map where the global id is the value (`u8`).
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

    /// The overworld superflat accessor (minY -64, height 384, 24 sections).
    fn accessor() -> SimpleLevelHeightAccessor {
        create_accessor(-64, 384)
    }

    /// A factory whose defaults are air (block) and plains (biome) — both id 0.
    fn factory() -> PalettedContainerFactory<u8, u8> {
        PalettedContainerFactory::new(block_strategy(), 0, biome_strategy(), 0)
    }

    /// The per-state behavior flags for the u8 test values: 0 is air, anything
    /// else is opaque (blocks motion).
    fn test_flags(s: &u8) -> StateFlags {
        StateFlags {
            is_air: *s == 0,
            blocks_motion: *s != 0,
            has_fluid: false,
            is_leaves: false,
        }
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

    /// A base with all-default air sections.
    fn default_base() -> ChunkAccess<u8, u8, &'static str> {
        ChunkAccess::new(
            ChunkPos::ZERO,
            UpgradeData::empty(accessor().get_sections_count() as usize),
            accessor(),
            &factory(),
            0,
            None,
            &test_flags,
        )
    }

    #[test]
    fn constructor_fills_default_sections_for_none() {
        let base = default_base();
        assert_eq!(base.get_sections().len(), 24);
        // All-default sections are all-air (the `new_all_air` defaults).
        assert!(base.get_sections().iter().all(|s| s.has_only_air()));
        // No heightmap entries yet (the concrete types prime them).
        assert!(base.heightmaps().iter().all(Option::is_none));
    }

    #[test]
    fn constructor_adopts_matching_sections_and_rejects_mismatches() {
        let factory = factory();
        // A stone section (id 1, not air).
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1u8);
        let stone = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            &is_air,
            &is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &is_special_colliding,
        );
        // A mismatched-length array is rejected wholesale, exactly like Java's
        // `arraycopy` guard: it logs a warning and keeps the all-default array
        // (Java logs; the port prints to stderr), then `replaceMissingSections`
        // fills defaults. The stone section is discarded.
        let base = ChunkAccess::<u8, u8, &str>::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            accessor(),
            &factory,
            0,
            Some(vec![stone]),
            &test_flags,
        );
        assert_eq!(base.get_sections().len(), 24);
        assert!(base.get_sections().iter().all(|s| s.has_only_air()));
        assert_eq!(base.get_highest_filled_section_index(), NO_FILLED_SECTION);

        // A matching-length array is adopted.
        let mut sections = Vec::with_capacity(24);
        for _ in 0..24 {
            sections.push(LevelChunkSection::new(
                PalettedContainer::new(0u8, block_strategy()),
                PalettedContainer::new(0u8, biome_strategy()),
                &is_air,
                &is_randomly_ticking,
                &fluid_is_empty,
                &fluid_is_randomly_ticking,
                &is_special_colliding,
            ));
        }
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1u8);
        sections[0] = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            &is_air,
            &is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &is_special_colliding,
        );
        let base = ChunkAccess::<u8, u8, &str>::new(
            ChunkPos::ZERO,
            UpgradeData::empty(24),
            accessor(),
            &factory,
            0,
            Some(sections),
            &test_flags,
        );
        assert_eq!(base.get_sections().len(), 24);
        assert!(!base.get_sections()[0].has_only_air());
        assert!(base.get_sections()[1..].iter().all(|s| s.has_only_air()));
        // `get_highest_filled_section_index` finds section 0.
        assert_eq!(base.get_highest_filled_section_index(), 0);
    }

    #[test]
    fn highest_filled_section_index_is_minus_one_for_all_air() {
        assert_eq!(
            default_base().get_highest_filled_section_index(),
            NO_FILLED_SECTION
        );
    }

    #[test]
    fn get_noise_biome_clamps_like_the_paper_optimisation() {
        // Default biome id 0 in every 4x4x4 quart: reads clamp quart Y outside
        // the section stack to the boundary section (top quart rel 3, bottom
        // rel 0) instead of panicking.
        let base = default_base();
        // Quart Y of the bottom section: quartMinY = minY >> 2 = -16.
        assert_eq!(base.get_noise_biome(0, -16, 0), 0);
        // Far below the world: clamps to section 0, rel 0.
        assert_eq!(base.get_noise_biome(1, -1000, 2), 0);
        // Quart Y of the top section (sectionY 19): quartMinY = 19*4 = 76.
        assert_eq!(base.get_noise_biome(0, 76, 0), 0);
        // Far above: clamps to the top section, rel 3.
        assert_eq!(base.get_noise_biome(3, 1000, 3), 0);
    }

    #[test]
    fn heightmap_storage_round_trips_by_world_ordinal() {
        let mut base = default_base();
        // `set_heightmap` creates the entry and copies the storage.
        let raw: Vec<i64> = prime_like_offsets();
        base.set_heightmap(Types::WorldSurface, &raw);
        assert!(base.has_primed_heightmap(Types::WorldSurface));
        // Reading back the height: stored offset 1 -> minY.
        assert_eq!(base.get_height_at(Types::WorldSurface, 3, 7), -64);
        // `get_or_create_heightmap_unprimed` is idempotent (computeIfAbsent).
        base.get_or_create_heightmap_unprimed(Types::WorldSurface);
        assert_eq!(base.heightmaps().iter().filter(|h| h.is_some()).count(), 1);
    }

    #[test]
    fn missing_heightmap_read_primes_then_returns_the_empty_column_default() {
        // A base with no heightmap entries (the concrete types prime them).
        let mut base = default_base();
        assert!(base.heightmaps().iter().all(Option::is_none));
        // Java `getHeight` primes a missing heightmap on demand
        // (`Heightmap.primeHeightmaps(this, EnumSet.of(type))`), then reads.
        // An all-air column primes to stored 0, which decodes as `minY - 1`.
        assert_eq!(base.get_height_at(Types::MotionBlocking, 0, 0), -65);
        assert_eq!(base.get_height_at(Types::WorldSurfaceWg, 15, 15), -65);
        // The create-on-read side effect: the entries now exist.
        assert!(base.has_primed_heightmap(Types::MotionBlocking));
        assert!(base.has_primed_heightmap(Types::WorldSurfaceWg));
    }

    #[test]
    fn unsaved_and_light_correct_flags() {
        let mut base = default_base();
        assert!(!base.is_unsaved());
        assert!(!base.is_light_correct());
        base.mark_unsaved();
        assert!(base.is_unsaved());
        assert!(base.try_mark_saved());
        assert!(!base.is_unsaved());
        assert!(!base.try_mark_saved());
        // `setLightCorrect` also marks unsaved (Java).
        base.set_light_correct(true);
        assert!(base.is_light_correct());
        assert!(base.is_unsaved());
    }

    #[test]
    fn inhabited_time_increments_like_java() {
        let mut base = default_base();
        assert_eq!(base.get_inhabited_time(), 0);
        base.increment_inhabited_time(37);
        base.increment_inhabited_time(5);
        assert_eq!(base.get_inhabited_time(), 42);
        base.set_inhabited_time(100);
        assert_eq!(base.get_inhabited_time(), 100);
    }

    #[test]
    fn pending_block_entity_nbt_carries_position() {
        let mut base = default_base();
        let mut tag = CompoundTag::new();
        tag.put_byte("x", 3);
        tag.put_byte("y", 2);
        tag.put_byte("z", 5);
        // ChunkPos::ZERO holds block x/z 0..15, so no wrong-chunk correction.
        base.set_block_entity_nbt(tag);
        let pos = BlockPos::new(3, 2, 5);
        assert_eq!(
            base.get_block_entity_nbt(&pos)
                .map(|t| t.get_byte_or("x", -1)),
            Some(3)
        );
        // A tag in a wrong chunk is re-anchored to the chunk's column.
        let mut wrong = CompoundTag::new();
        wrong.put_int("x", 20); // section 1, chunk x 1
        wrong.put_byte("y", 2);
        wrong.put_int("z", 0);
        base.set_block_entity_nbt(wrong);
        let reanchored = BlockPos::new(4, 2, 0); // 1*16 + (20 & 15)
        assert!(base.get_block_entity_nbt(&reanchored).is_some());
        assert!(
            base.get_block_entity_nbt(&BlockPos::new(20, 2, 0))
                .is_none()
        );
    }

    #[test]
    fn structure_mutations_mark_unsaved() {
        let mut base = default_base();
        base.set_start_for_structure("monument", 7);
        assert!(base.is_unsaved());
        base.try_mark_saved();
        base.add_reference_for_structure("monument", 0x10);
        assert!(base.is_unsaved());
        assert_eq!(base.get_start_for_structure(&"monument"), Some(7));
        assert_eq!(base.get_all_starts().len(), 1);
        base.set_all_starts(HashMap::new());
        assert!(base.get_all_starts().is_empty());
        assert!(base.is_unsaved());
    }

    #[test]
    fn get_pos_from_tag_corrects_wrong_chunk_like_java() {
        use rivet_registry::core::Vec3iLike;
        // `BlockEntity.getPosFromTag` with a chunk base at (2, -3): x spans
        // [32, 47], z spans [-48, -33].
        let base = ChunkPos::new(2, -3);
        // x = 5 (section 0) and z = -21 (section -2) are in genuinely different
        // chunks than the base (sections 2 and -3), so both coordinates are
        // re-anchored to the base chunk's local offsets: 2*16 + (5 & 15) = 37,
        // -3*16 + (-21 & 15) = -48 + 11 = -37.
        let mut tag = CompoundTag::new();
        tag.put_int("x", 5);
        tag.put_int("y", 64);
        tag.put_int("z", -21);
        let pos = get_pos_from_tag(Some(&base), &tag);
        assert_eq!(pos.coords(), (37, 64, -37));
        // When only one coordinate is in the wrong chunk, Java still re-anchors
        // both (`i != base.x() || j != base.z()`); z = -37 is already in the
        // base chunk, so re-anchoring it is a no-op at value level.
        let mut tag_z_ok = CompoundTag::new();
        tag_z_ok.put_int("x", 5);
        tag_z_ok.put_int("y", 64);
        tag_z_ok.put_int("z", -37); // section -3 == base.z(), offset 11
        assert_eq!(
            get_pos_from_tag(Some(&base), &tag_z_ok).coords(),
            (37, 64, -37)
        );
        // A `None` base (items) skips the correction.
        let pos = get_pos_from_tag(None, &tag);
        assert_eq!(pos.coords(), (5, 64, -21));
    }

    /// A 37-long 9-bit storage of all-`1` offsets — what a flat stone floor
    /// primes (matches the superflat fixture's first heightmap).
    fn prime_like_offsets() -> Vec<i64> {
        let mut raw = vec![0x0040_2010_0804_0201i64; 36];
        raw.push(0x0000_0000_0804_0201i64);
        raw
    }

    #[test]
    fn final_heightmaps_are_primed_by_the_concrete_chunk() {
        // The concrete `LevelChunk` constructor primes exactly FINAL_HEIGHTMAPS;
        // this asserts the base storage keys them by world ordinal.
        let mut base = default_base();
        for ty in FINAL_HEIGHTMAPS {
            base.get_or_create_heightmap_unprimed(ty);
        }
        for ty in Types::all() {
            assert_eq!(base.has_primed_heightmap(ty), ty.in_final_heightmaps());
        }
    }

    #[test]
    fn is_y_space_empty_clamps_and_steps_by_section() {
        let mut base = default_base();
        // All-default (all-air): every range is empty.
        assert!(base.is_y_space_empty(-64, 319));
        assert!(base.is_y_space_empty(-1000, 1000));
        // A non-air block in section 0 makes the overlapping range non-empty.
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1u8);
        base.sections[0] = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            &is_air,
            &is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &is_special_colliding,
        );
        assert!(!base.is_y_space_empty(-64, 0));
        assert!(!base.is_y_space_empty(-1000, 1000));
        // A range entirely above section 0 stays empty.
        assert!(base.is_y_space_empty(16, 319));
    }

    #[test]
    fn find_blocks_visits_matching_absolute_positions() {
        let mut base = default_base();
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(1, 2, 3, 7u8);
        base.sections[0] = LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            &is_air,
            &is_randomly_ticking,
            &fluid_is_empty,
            &fluid_is_randomly_ticking,
            &is_special_colliding,
        );
        let mut found = Vec::new();
        base.find_block_light_sources(&|s| *s != 0, |pos, state| found.push((pos, state)));
        // ChunkPos::ZERO, section Y -4: the block is at absolute (-64 + 2).
        use rivet_registry::core::Vec3iLike;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, 7);
        assert_eq!(found[0].0.coords(), (1, -62, 3));
    }
}

//! Port of `net.minecraft.world.level.chunk.ImposterProtoChunk` (MC 26.2) — the
//! worldgen-phase view over an already-loaded `LevelChunk`.
//!
//! Java: `ImposterProtoChunk.java` in `working/Paper`. `ImposterProtoChunk
//! extends ProtoChunk` but wraps a `LevelChunk` and delegates the reads to it,
//! gating every mutator behind `allowWrites`. Per OWNERSHIP.md there is no
//! inheritance, so this chunk holds the wrapped `LevelChunk` and its
//! `allowWrites` flag, plus the `ProtoChunk`-side carriers Java's `super.*`
//! calls touch (the `status` field and `carvingMask`). The `entities` list and
//! the base's default all-air sections are never read, so they are not stored.
//!
//! `getSection` is Java's `allowWrites ? wrapped.getSection : super.getSection`,
//! but `super.getSection(i)` is `this.getSections()[i]` — a virtual call that
//! dispatches to the `getSections` override (`wrapped.getSections()`), and
//! `LevelChunk` does not override `getSection`, so the write branch reads the
//! same `wrapped.getSections()[i]`. The two branches are behaviorally
//! identical: a read-only caller sees the wrapped chunk's contents, and the
//! base `this.sections` all-air defaults that `super(...)`'s
//! `replaceMissingSections` fills are unreachable through `getSection`.
//!
//! The heightmap `fixType` maps the worldgen-only `WORLD_SURFACE_WG`/
//! `OCEAN_FLOOR_WG` types to their client-facing forms because the wrapped
//! `LevelChunk` only stores the `FINAL_HEIGHTMAPS` types.
//!
//! Deferred with their owning units: `setBlockState`'s write path (the
//! mutator unit #216 — Java's `wrapped.setBlockState` gate is a no-op until
//! then); `getFluidState` (the `FluidState` type); the block-entity accessors;
//! `getBlockTicks`/`getFluidTicks`/the `BlackholeTickAccess` gate (the ticks
//! unit); `fillBiomesFromNoise`/`initializeLightSources`/`getSkyLightSources`
//! (biome resolution and lighting); and the Starlight light overrides
//! (lighting engine #184).
//!
//! `setHeightmap`/`setStartForStructure`/`setAllStarts`/
//! `addReferenceForStructure`/`setAllReferences`/`removeBlockEntity`/
//! `markPosForPostProcessing`/`setBlockEntityNbt` are no-ops in Java —
//! ported as such.
//!
//! `getCarvingMask`/`getOrCreateCarvingMask` throw `UnsupportedOperationException`
//! when writes are disallowed (Java `Util.pauseInIde`); the port panics in
//! that case.
//!
//! `setBlockState`'s `allowWrites ? wrapped.setBlockState : null` gate is not
//! ported — the `LevelChunk` write path defers with the mutator unit.

use crate::chunk::carving_mask::CarvingMask;
use crate::chunk::chunk_access::ChunkStatus;
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::level::height_accessor::SimpleLevelHeightAccessor;
use crate::levelgen::heightmap::Types;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::{BlockPos, ChunkPos};

/// `net.minecraft.world.level.chunk.ImposterProtoChunk`.
pub struct ImposterProtoChunk<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `wrapped` — the `LevelChunk` all reads delegate to.
    wrapped: LevelChunk<T, B, S>,
    /// `allowWrites` — gates every mutator.
    allow_writes: bool,
    /// The `ProtoChunk`-side `status` Java's `super.setPersistedStatus` writes
    /// (never read back — `getPersistedStatus` delegates to the wrapped chunk).
    status: ChunkStatus,
    /// The `ProtoChunk`-side `carvingMask`, written only when writes are
    /// allowed (Java's `super.getOrCreateCarvingMask()`).
    carving_mask: Option<CarvingMask>,
}

impl<T, B, S> ImposterProtoChunk<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// `ImposterProtoChunk(LevelChunk, boolean allowWrites)`.
    pub fn new(wrapped: LevelChunk<T, B, S>, allow_writes: bool) -> Self {
        ImposterProtoChunk {
            wrapped,
            allow_writes,
            status: ChunkStatus::Empty,
            carving_mask: None,
        }
    }

    /// `fixType(Heightmap.Types)` — the WG → non-WG mapping the wrapped chunk
    /// expects (`WORLD_SURFACE_WG` → `WORLD_SURFACE`, `OCEAN_FLOOR_WG` →
    /// `OCEAN_FLOOR`, else the type unchanged).
    pub fn fix_type(ty: Types) -> Types {
        match ty {
            Types::WorldSurfaceWg => Types::WorldSurface,
            Types::OceanFloorWg => Types::OceanFloor,
            other => other,
        }
    }

    /// `getWrapped()`.
    pub fn get_wrapped(&self) -> &LevelChunk<T, B, S> {
        &self.wrapped
    }

    /// Consume the imposter when Paper's FULL task reuses an already loaded
    /// `LevelChunk` instead of constructing a second one.
    pub fn into_wrapped(self) -> LevelChunk<T, B, S> {
        self.wrapped
    }

    /// `getBlockState(int, int, int)` (Paper's `final` overload) — the wrapped
    /// chunk's `getBlockStateFinal`.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> T {
        self.wrapped.get_block_state_final(x, y, z)
    }

    /// `getBlockState(BlockPos)` — the wrapped chunk's read.
    pub fn get_block_state_pos(&self, pos: &BlockPos) -> T {
        self.wrapped
            .get_block_state(pos.get_x(), pos.get_y(), pos.get_z())
    }

    /// `getSection(int)` — Java's `allowWrites ? wrapped.getSection :
    /// super.getSection`, which resolves to `wrapped.getSections()[i]` on both
    /// branches (see the module docs): always the wrapped chunk's section.
    pub fn get_section(&self, section_index: usize) -> &LevelChunkSection<T, B> {
        self.wrapped.get_section(section_index)
    }

    /// `getSections()` — always the wrapped chunk's sections (Java's override).
    pub fn get_sections(&self) -> &[LevelChunkSection<T, B>] {
        self.wrapped.get_sections()
    }

    /// `getHeight(Heightmap.Types, int, int)` — the wrapped chunk, with the
    /// type `fixType`d first. Named `get_height_at` (the `getHeight()` overload
    /// clash — see `ChunkAccess::get_height_at`). The wrapped chunk primes a
    /// missing entry on read, so the imposter takes `&mut self`.
    pub fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        self.wrapped.get_height_at(Self::fix_type(ty), x, z)
    }

    /// `getOrCreateHeightmapUnprimed(Heightmap.Types)` — delegates to the
    /// wrapped chunk (Java does not `fixType` here).
    pub fn get_or_create_heightmap_unprimed(&mut self, ty: Types) {
        self.wrapped.get_or_create_heightmap_unprimed(ty);
    }

    /// `setHeightmap(Heightmap.Types, long[])` — a no-op.
    pub fn set_heightmap(&mut self, _key: Types, _data: &[i64]) {}

    /// `getNoiseBiome(int, int, int)` — the wrapped chunk's read.
    pub fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> B {
        self.wrapped.get_noise_biome(quart_x, quart_y, quart_z)
    }

    /// `getPos()`.
    pub fn get_pos(&self) -> ChunkPos {
        self.wrapped.get_pos()
    }

    /// `getMinY()`.
    pub fn get_min_y(&self) -> i32 {
        self.wrapped.get_min_y()
    }

    /// `getHeight()`.
    pub fn get_height(&self) -> i32 {
        self.wrapped.get_height()
    }

    /// The contained `levelHeightAccessor`.
    pub fn height_accessor(&self) -> SimpleLevelHeightAccessor {
        self.wrapped.height_accessor()
    }

    /// `markUnsaved()` — forwards to the wrapped chunk.
    pub fn mark_unsaved(&mut self) {
        self.wrapped.mark_unsaved();
    }

    /// `canBeSerialized()` — false.
    pub fn can_be_serialized(&self) -> bool {
        false
    }

    /// `tryMarkSaved()` — false.
    pub fn try_mark_saved(&mut self) -> bool {
        false
    }

    /// `isUnsaved()` — false.
    pub fn is_unsaved(&self) -> bool {
        false
    }

    /// `isLightCorrect()` — the wrapped chunk's flag.
    pub fn is_light_correct(&self) -> bool {
        self.wrapped.is_light_correct()
    }

    /// `setLightCorrect(boolean)` — forwards to the wrapped chunk.
    pub fn set_light_correct(&mut self, is_light_correct: bool) {
        self.wrapped.set_light_correct(is_light_correct);
    }

    /// `getPersistedStatus()` — the wrapped chunk's status (a `LevelChunk` is
    /// always `FULL`). Java reads `this.wrapped.getPersistedStatus()`, so the
    /// `ProtoChunk`-side `status` field written by [`set_persisted_status`] is
    /// never read back.
    pub fn get_persisted_status(&self) -> ChunkStatus {
        self.wrapped.get_persisted_status()
    }

    /// `setPersistedStatus(ChunkStatus)` — writes only when allowed; Java's
    /// `super.setPersistedStatus` stores the `ProtoChunk`-side status and then
    /// calls the virtual `markUnsaved()`, which the imposter forwards to the
    /// wrapped chunk.
    pub fn set_persisted_status(&mut self, status: ChunkStatus) {
        if self.allow_writes {
            self.status = status;
            self.wrapped.mark_unsaved();
        }
    }

    /// `getStartForStructure(Structure)` — the wrapped chunk's starts.
    pub fn get_start_for_structure(&self, structure: &S) -> Option<i64> {
        self.wrapped.get_start_for_structure(structure)
    }

    /// `setStartForStructure(Structure, StructureStart)` — a no-op.
    pub fn set_start_for_structure(&mut self, _structure: S, _start: i64) {}

    /// `getAllStarts()` — the wrapped chunk's starts map.
    pub fn get_all_starts(&self) -> &std::collections::HashMap<S, i64> {
        self.wrapped.get_all_starts()
    }

    /// `setAllStarts(Map)` — a no-op.
    pub fn set_all_starts(&mut self, _starts: std::collections::HashMap<S, i64>) {}

    /// `getReferencesForStructure(Structure)` — the wrapped chunk's references.
    pub fn get_references_for_structure<'a>(
        &'a self,
        structure: &'a S,
    ) -> impl Iterator<Item = &'a u64> + 'a {
        self.wrapped.get_references_for_structure(structure)
    }

    /// `addReferenceForStructure(Structure, long)` — a no-op.
    pub fn add_reference_for_structure(&mut self, _structure: S, _reference: u64) {}

    /// `getAllReferences()` — the wrapped chunk's references map
    /// (insertion-ordered, #537).
    pub fn get_all_references(&self) -> &indexmap::IndexMap<S, indexmap::IndexSet<u64>> {
        self.wrapped.get_all_references()
    }

    /// `setAllReferences(Map)` — a no-op.
    pub fn set_all_references<I: IntoIterator<Item = (S, Vec<u64>)>>(&mut self, _data: I) {}

    /// `removeBlockEntity(BlockPos)` — a no-op.
    pub fn remove_block_entity(&mut self, _pos: &BlockPos) {}

    /// `markPosForPostProcessing(BlockPos)` — a no-op.
    pub fn mark_pos_for_post_processing(&mut self, _block_pos: &BlockPos) {}

    /// `setBlockEntityNbt(CompoundTag)` — a no-op.
    pub fn set_block_entity_nbt(&mut self, _entity_tag: CompoundTag) {}

    /// `getBlockEntityNbt(BlockPos)` — the wrapped chunk's pending NBT.
    pub fn get_block_entity_nbt(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.wrapped.get_block_entity_nbt(pos)
    }

    /// `getBlockEntityNbtForSaving(BlockPos, HolderLookup)` — the wrapped
    /// chunk's saving NBT.
    pub fn get_block_entity_nbt_for_saving(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.wrapped.get_block_entity_nbt_for_saving(pos)
    }

    /// `findBlocks(Predicate, BiConsumer)` — the wrapped chunk's walk.
    pub fn find_blocks(&self, predicate: &dyn Fn(&T) -> bool, consumer: impl FnMut(BlockPos, T)) {
        self.wrapped.find_blocks(predicate, consumer);
    }

    /// `getCarvingMask()` — the `ProtoChunk`-side mask when writes are allowed,
    /// else Java throws `UnsupportedOperationException("Meaningless in this
    /// context")`.
    pub fn get_carving_mask(&self) -> Option<&CarvingMask> {
        if self.allow_writes {
            self.carving_mask.as_ref()
        } else {
            panic!("Meaningless in this context");
        }
    }

    /// `getOrCreateCarvingMask()` — same gate as [`get_carving_mask`].
    pub fn get_or_create_carving_mask(&mut self) -> &mut CarvingMask {
        if !self.allow_writes {
            panic!("Meaningless in this context");
        }
        if self.carving_mask.is_none() {
            self.carving_mask = Some(CarvingMask::new(
                self.wrapped.get_height(),
                self.wrapped.get_min_y(),
            ));
        }
        self.carving_mask.as_mut().expect("just created")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container::PalettedContainer;
    use crate::chunk::paletted_container_factory::PalettedContainerFactory;
    use crate::chunk::strategy::Strategy;
    use crate::chunk::upgrade_data::UpgradeData;
    use crate::level::height_accessor::create as create_accessor;
    use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, StateFlags};

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

    /// An imposter over the wrapped chunk (see [`wrapped_chunk`]).
    fn imposter(
        wrapped: LevelChunk<u8, u8, &'static str>,
        allow_writes: bool,
    ) -> ImposterProtoChunk<u8, u8, &'static str> {
        ImposterProtoChunk::new(wrapped, allow_writes)
    }

    /// A wrapped chunk with stone (1) at (0, 0, 0) of section 0 and air (0)
    /// elsewhere.
    fn wrapped_chunk() -> LevelChunk<u8, u8, &'static str> {
        let mut sections = Vec::with_capacity(24);
        let mut states = PalettedContainer::new(0u8, block_strategy());
        states.set(0, 0, 0, 1u8);
        sections.push(crate::chunk::level_chunk_section::LevelChunkSection::new(
            states,
            PalettedContainer::new(0u8, biome_strategy()),
            is_air,
            is_randomly_ticking,
            fluid_is_empty,
            fluid_is_randomly_ticking,
            is_special_colliding,
        ));
        for _ in 1..24 {
            sections.push(crate::chunk::level_chunk_section::LevelChunkSection::new(
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
            create_accessor(-64, 384),
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
    fn reads_delegate_to_the_wrapped_chunk() {
        let imposter = imposter(wrapped_chunk(), false);
        assert_eq!(imposter.get_block_state(0, -64, 0), 1);
        assert_eq!(imposter.get_block_state_pos(&BlockPos::new(0, -64, 0)), 1);
        assert_eq!(imposter.get_block_state(0, 0, 0), 0);
        assert_eq!(imposter.get_pos(), ChunkPos::ZERO);
        assert_eq!(imposter.get_min_y(), -64);
        assert_eq!(imposter.get_height(), 384);
        assert_eq!(imposter.get_sections().len(), 24);
    }

    #[test]
    fn get_section_always_returns_the_wrapped_chunk_sections() {
        // Java's `getSection` is `allowWrites ? wrapped.getSection :
        // super.getSection`, and `super.getSection(i)` is
        // `this.getSections()[i]` — a virtual call that dispatches to the
        // `getSections` override (`wrapped.getSections()`). `LevelChunk` does
        // not override `getSection`, so the write branch reads the same
        // `wrapped.getSections()[i]`: both branches return the wrapped chunk's
        // section (the base all-air defaults are unreachable through
        // `getSection`).
        let read_only = imposter(wrapped_chunk(), false);
        assert_eq!(read_only.get_section(0).get_block_state(0, 0, 0), 1);
        assert_eq!(read_only.get_section(0).non_empty_block_count(), 1);
        let writable = imposter(wrapped_chunk(), true);
        assert_eq!(writable.get_section(0).get_block_state(0, 0, 0), 1);
        assert_eq!(writable.get_section(0).non_empty_block_count(), 1);
    }

    #[test]
    fn set_persisted_status_is_gated_and_marks_the_wrapped_unsaved() {
        // Read-only: no-op (Java's `if (this.allowWrites)` guard).
        let mut read_only = imposter(wrapped_chunk(), false);
        read_only.set_persisted_status(ChunkStatus::Full);
        assert!(!read_only.get_wrapped().is_unsaved());

        // Write-allowed: Java's `super.setPersistedStatus` writes the
        // ProtoChunk-side status and the virtual `markUnsaved` forwards to the
        // wrapped chunk. The written status is never read back.
        let mut writable = imposter(wrapped_chunk(), true);
        assert!(!writable.get_wrapped().is_unsaved());
        writable.set_persisted_status(ChunkStatus::Full);
        assert!(writable.get_wrapped().is_unsaved());
        assert_eq!(writable.get_persisted_status(), ChunkStatus::Full);
    }

    #[test]
    fn fix_type_maps_worldgen_types_to_client_forms() {
        assert_eq!(
            ImposterProtoChunk::<u8, u8, &str>::fix_type(Types::WorldSurfaceWg),
            Types::WorldSurface
        );
        assert_eq!(
            ImposterProtoChunk::<u8, u8, &str>::fix_type(Types::OceanFloorWg),
            Types::OceanFloor
        );
        assert_eq!(
            ImposterProtoChunk::<u8, u8, &str>::fix_type(Types::MotionBlocking),
            Types::MotionBlocking
        );
        assert_eq!(
            ImposterProtoChunk::<u8, u8, &str>::fix_type(Types::WorldSurface),
            Types::WorldSurface
        );
    }

    #[test]
    fn serialization_flags_match_java() {
        let mut imposter = imposter(wrapped_chunk(), false);
        assert!(!imposter.can_be_serialized());
        assert!(!imposter.try_mark_saved());
        assert!(!imposter.is_unsaved());
        imposter.mark_unsaved();
        assert!(!imposter.is_unsaved());
    }

    #[test]
    fn height_queries_go_through_fix_type() {
        // The wrapped `LevelChunk`'s `FINAL_HEIGHTMAPS` entries pre-exist as
        // all-zero storage, so the reads do not prime and every column reads
        // `minY - 1` = -65 (Java's fresh-chunk behavior). The WG types map to
        // their client forms first (WorldSurfaceWg -> WorldSurface,
        // OceanFloorWg -> OceanFloor), which the `-65` reads exercise.
        let mut imposter = imposter(wrapped_chunk(), false);
        assert_eq!(imposter.get_height_at(Types::WorldSurfaceWg, 0, 0), -65);
        assert_eq!(imposter.get_height_at(Types::WorldSurface, 0, 0), -65);
        assert_eq!(imposter.get_height_at(Types::OceanFloorWg, 5, 7), -65);
    }

    #[test]
    #[should_panic]
    fn carving_mask_throws_when_writes_disallowed() {
        let imposter = imposter(wrapped_chunk(), false);
        let _ = imposter.get_carving_mask();
    }

    #[test]
    fn carving_mask_reuses_when_writes_allowed() {
        let mut imposter = imposter(wrapped_chunk(), true);
        imposter.get_or_create_carving_mask().set(1, -64, 2);
        let mask = imposter.get_carving_mask().expect("created");
        assert!(mask.get(1, -64, 2));
        imposter.get_or_create_carving_mask();
        assert!(imposter.get_carving_mask().expect("created").get(1, -64, 2));
    }

    #[test]
    fn noop_setters_do_not_touch_the_wrapped_chunk() {
        let mut imposter = imposter(wrapped_chunk(), true);
        imposter.set_heightmap(Types::WorldSurface, &[]);
        imposter.mark_pos_for_post_processing(&BlockPos::new(0, 0, 0));
        imposter.set_block_entity_nbt(Default::default());
        imposter.remove_block_entity(&BlockPos::new(0, 0, 0));
        imposter.set_start_for_structure("monument", 7);
        imposter.add_reference_for_structure("monument", 0x1234);
        assert!(imposter.get_start_for_structure(&"monument").is_none());
        assert!(
            imposter
                .get_references_for_structure(&"monument")
                .next()
                .is_none()
        );
    }

    #[test]
    fn heightmaps_are_primed_via_the_wrapped_chunk() {
        // `getOrCreateHeightmapUnprimed` delegates to the wrapped chunk. The
        // `LevelChunk` constructor creates an (unprimed) entry for exactly the
        // `FINAL_HEIGHTMAPS` types; calling through the imposter must leave
        // every one of those entries present and `Some` (idempotent, Java's
        // `computeIfAbsent`), and must never spuriously create an entry for a
        // type that was not requested.
        let mut imposter = imposter(wrapped_chunk(), true);
        for ty in FINAL_HEIGHTMAPS {
            imposter.get_or_create_heightmap_unprimed(ty);
        }
        let heightmaps = imposter.get_wrapped().heightmaps();
        // Each `FINAL_HEIGHTMAPS` entry exists (created by the `LevelChunk`
        // constructor and returned through the imposter's delegation).
        for ty in FINAL_HEIGHTMAPS {
            assert!(
                heightmaps[ty as usize].is_some(),
                "FINAL_HEIGHTMAPS entry {ty:?} is primed through the imposter"
            );
        }
        // The worldgen-only types (`WORLD_SURFACE_WG`/`OCEAN_FLOOR_WG`) are
        // not in `FINAL_HEIGHTMAPS` and were never requested, so they stay
        // unprimed — proving the delegation creates entries only on request.
        for ty in [Types::WorldSurfaceWg, Types::OceanFloorWg] {
            assert!(
                heightmaps[ty as usize].is_none(),
                "worldgen-only {ty:?} stays unprimed"
            );
        }
    }

    #[test]
    fn light_correct_flag_forwards_to_the_wrapped_chunk() {
        let mut imposter = imposter(wrapped_chunk(), false);
        assert!(!imposter.is_light_correct());
        imposter.set_light_correct(true);
        assert!(imposter.is_light_correct());
    }
}

//! `ca.spottedleaf.moonrise.patches.starlight.light` — the `rivet-server` side
//! of the cycle-breaking light-provider seam.
//!
//! Java: `StarLightInterface.java` in `working/Paper`. The compute engine lives
//! in `rivet-server` (the manifest unit owns it), while the facade it plugs
//! into — [`LevelLightEngine`] — lives in `rivet-world`. Java's reference
//! direction (`ThreadedLevelLightEngine` → `StarLightLightingProvider` →
//! `StarLightInterface`, which holds the `LevelLightEngine` back) becomes: this
//! crate implements [`StarLightProvider`] (the `starlight$getLightEngine()`
//! surface) and hands the boxed impl to `LevelLightEngine::with_provider`. No
//! crate cycle: `rivet-server` already depends on `rivet-world`, and the trait
//! lives in `rivet-world`.
//!
//! [`SkyLightProvider`] replaces the phase-A stub no-op with a real synchronous
//! compute layer: it owns a
//! [`SkyStarLightEngine`] (the Starlight flood-fill core) and drives it on an
//! explicitly supplied in-progress chunk through [`SkyLightProvider::light_chunk_with`],
//! publishing the engine's computed sky nibbles and sky-emptiness map back onto
//! the chunk (Java's `ChunkLightTask` → `setNibbles`/`setEmptinessMap`).
//!
//! The engine resolves neighbour chunks through the provider's *narrow chunk
//! access* — a take/put closure over the caller's chunk storage, mirroring
//! Java's `LightChunkGetter` without a global `ChunkMap` lookup or any shared
//! authoritative state (OWNERSHIP.md). The caller returns an owned chunk from
//! a `None` take operation and consumes a put slot only after committing it to
//! storage. The provider buffers neighbours it resolves during a run and
//! returns them afterwards — on the panic path too, where the engine's `light`
//! re-unwinds after its finally-equivalent cache clear. A put callback panic
//! retains the uncommitted chunk for a later retry; no neighbour is fabricated
//! and none is dropped.
//!
//! The pos-only trait seam ([`StarLightProvider::light_chunk`]) is the take/put
//! round-trip: take the chunk at the position, light it, put it back. The
//! readers' pos-only variants cannot resolve a chunk (they run on `&self`, and
//! the take/put closure is exclusive `&mut`), so they faithfully report Java's
//! null-chunk branches; the real reads live on the resolved-chunk
//! [`SkyLightProvider::get_sky_light_value_in`] /
//! [`SkyLightProvider::get_block_light_value_in`] /
//! [`SkyLightProvider::get_data_layer_data_in`].
//!
//! What defers with #184: block light (the `block_nibbles` stay empty), live
//! `blockChange`/`sectionChange`/`relightChunks`/`checkChunkEdges`, the client
//! notify path, and the final status/pipeline wiring (the frozen
//! `world_gen_context.rs` caller drives `try_light_chunk` through the trait seam;
//! the concrete chunk storage it would resolve into lands with the chunk map).

use std::collections::{HashMap, HashSet};

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::data_layer::DataLayer;
use rivet_world::level::LevelHeightAccessor;
use rivet_world::level::height_accessor::SimpleLevelHeightAccessor;
use rivet_world::lighting::star_light_provider::{LightProviderError, StarLightProvider};

use super::star_light_engine::{ChunkAccessor, SkyStarLightEngine};
use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StateId, StructureKey};

/// The runtime chunk value a generated LIGHT bridge hands to Starlight.
pub type LightChunk = ChunkAccess<StateId, ServerBiomeId, StructureKey>;

/// The narrow chunk access the engine lights through: a take/put closure over
/// the caller's tick-thread chunk storage. A `None` operation takes the chunk
/// at `(x, z)` and returns it; a `Some(&mut slot)` operation puts the owned
/// value back. Put callbacks should consume the slot only after committing the
/// chunk to storage, so a panic before commit leaves the value recoverable.
pub type ChunkAccessFn = Box<
    dyn for<'a> FnMut(i32, i32, Option<&'a mut Option<LightChunk>>) -> Option<LightChunk> + Send,
>;

/// The `SkyStarLightEngine`-backed synchronous light provider — the concrete
/// impl `rivet-server` hands `LevelLightEngine::with_provider`.
pub struct SkyLightProvider {
    /// The Starlight compute engine, built with the world's vertical extent.
    engine: SkyStarLightEngine,
    /// `WorldUtil.getMinSection(levelHeightAccessor)` — for the reader bounds.
    min_section: i32,
    /// `WorldUtil.getMaxSection(levelHeightAccessor)` (inclusive).
    max_section: i32,
    /// `minLightSection` — `getMinSectionY() - 1`.
    min_light_section: i32,
    /// `maxLightSection` — `getMaxSectionY() + 1` (inclusive).
    max_light_section: i32,
    /// `hasSkyLight` — whether the sky reader is active.
    has_sky_light: bool,
    /// `hasBlockLight` — whether the block reader is active.
    has_block_light: bool,
    /// The narrow take/put chunk access (see [`ChunkAccessFn`]).
    chunks: ChunkAccessFn,
    /// Chunks whose storage callback panicked before accepting ownership.
    pending_restores: Vec<(i32, i32, LightChunk)>,
}

impl SkyLightProvider {
    /// Build the provider for the world with the given vertical extent and
    /// light flags, lighting through `chunks` — a take/put closure over the
    /// caller's chunk storage (see [`ChunkAccessFn`] for the contract).
    pub fn new(
        height_accessor: SimpleLevelHeightAccessor,
        has_sky_light: bool,
        has_block_light: bool,
        chunks: ChunkAccessFn,
    ) -> Self {
        let min_section = height_accessor.get_min_section_y();
        let max_section = height_accessor.get_max_section_y();
        SkyLightProvider {
            engine: SkyStarLightEngine::new(&height_accessor),
            min_section,
            max_section,
            min_light_section: min_section - 1,
            max_light_section: max_section + 1,
            has_sky_light,
            has_block_light,
            chunks,
            pending_restores: Vec::new(),
        }
    }

    /// Take the chunk at `pos` as owned from the caller's storage (`None` when
    /// it is not present).
    fn take_chunk(
        &mut self,
        pos: ChunkPos,
    ) -> Result<Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>>, LightProviderError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.chunks)(pos.x(), pos.z(), None)
        }))
        .map_err(|_| LightProviderError::CallbackPanicked)
    }

    /// Put an owned chunk back at `pos`.
    fn put_chunk(
        &mut self,
        pos: ChunkPos,
        chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) -> Result<(), LightProviderError> {
        let mut slot = Some(chunk);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.chunks)(pos.x(), pos.z(), Some(&mut slot));
        }));
        if result.is_err() || slot.is_some() {
            if let Some(chunk) = slot {
                self.pending_restores.push((pos.x(), pos.z(), chunk));
            }
            return Err(LightProviderError::CallbackPanicked);
        }
        Ok(())
    }

    fn flush_pending_restores(&mut self) -> Result<(), LightProviderError> {
        let pending = std::mem::take(&mut self.pending_restores);
        let mut callback_panicked = false;
        for (x, z, chunk) in pending {
            callback_panicked |= self.put_chunk(ChunkPos::new(x, z), chunk).is_err();
        }
        if callback_panicked {
            Err(LightProviderError::CallbackPanicked)
        } else {
            Ok(())
        }
    }

    /// Attempt every restoration even when one storage callback panics. A
    /// caller-supplied engine panic is handled separately and always wins.
    fn restore_chunks(
        &mut self,
        taken: HashMap<(i32, i32), ChunkAccess<StateId, ServerBiomeId, StructureKey>>,
    ) -> bool {
        let mut callback_panicked = false;
        for ((x, z), chunk) in taken {
            callback_panicked |= self.put_chunk(ChunkPos::new(x, z), chunk).is_err();
        }
        callback_panicked
    }

    /// `SkyStarLightEngine.lightChunk(chunk, emptySections)` on an explicitly
    /// supplied in-progress chunk — the primary path. Drives the engine with
    /// the chunk and the per-section emptiness mask, then publishes the
    /// computed sky nibbles and sky-emptiness map back onto the chunk (Java's
    /// `setNibbles`/`setEmptinessMap` write-back in `ChunkLightTask`).
    ///
    /// Neighbours resolve through the take/put callback (each taken, buffered,
    /// and put back); the engine tolerates missing neighbours (`relaxed`
    /// cache setup), so a chunk lit in isolation still computes the correct
    /// center light.
    pub fn light_chunk_with(
        &mut self,
        chunk: &mut ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        // Restore any neighbour a previous run took but could not put back
        // (a storage callback panicked mid-put). The value bridge calls this
        // directly and never goes through the position seams that flush these,
        // so a stranded chunk from a failed bridge run must be returned to its
        // original slot before the next run — never left owned by the provider.
        self.flush_pending_restores()?;
        let mut accessor = CallbackAccessor {
            chunks: &mut self.chunks,
            taken: HashMap::new(),
        };
        // The engine's `light` re-unwinds a panicking run only after its
        // finally-equivalent (`destroyCaches`). The caller's storage contract
        // ("no neighbour fabricated, none dropped") still holds on that path: a
        // run that panics must return the neighbours it took. Catching here lets
        // the put-back run while the original payload is still recoverable, then
        // re-throws it — no neighbour dropped, no second panic mid-unwind.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.engine.light(&mut accessor, chunk, empty_sections);
            if let Some(nibbles) = self.engine.take_pending_nibbles() {
                chunk.set_sky_nibbles(nibbles);
            }
            if let Some(map) = self.engine.take_pending_emptiness_map() {
                chunk.set_sky_emptiness_map(Some(map));
            }
        }));
        if let Err(payload) = result {
            // Deterministic restore before the original panic resumes — the
            // put-back ends the accessor's borrow of the callback first, so
            // `self.chunks` is available again.
            let taken = std::mem::take(&mut accessor.taken);
            drop(accessor);
            self.restore_chunks(taken);
            std::panic::resume_unwind(payload);
        }
        // Return the neighbours the run resolved — drop the accessor to end its
        // borrow of the callback, then put each buffered chunk back.
        let taken = std::mem::take(&mut accessor.taken);
        drop(accessor);
        if self.restore_chunks(taken) {
            return Err(LightProviderError::CallbackPanicked);
        }
        Ok(())
    }

    /// The fallible position seam used by status generation. Missing centers
    /// are refusals, not successful no-ops that could promote the ProtoChunk.
    pub fn try_light_chunk(
        &mut self,
        pos: ChunkPos,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        self.flush_pending_restores()?;
        let Some(mut chunk) = self.take_chunk(pos)? else {
            return Err(LightProviderError::MissingChunk(pos));
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.light_chunk_with(&mut chunk, empty_sections)
        }));
        let put_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.put_chunk(pos, chunk)));
        match result {
            Err(payload) => {
                let _ = put_result;
                std::panic::resume_unwind(payload);
            }
            Ok(inner) => match put_result {
                Err(_) | Ok(Err(LightProviderError::CallbackPanicked)) => {
                    Err(LightProviderError::CallbackPanicked)
                }
                Ok(Ok(())) => inner,
                Ok(Err(error)) => Err(error),
            },
        }
    }

    /// The fallible persisted-load seam. It must resolve and restore the
    /// center even though the operation itself does not recompute light.
    pub fn try_force_load_in_chunk(
        &mut self,
        pos: ChunkPos,
        _empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        self.flush_pending_restores()?;
        let Some(chunk) = self.take_chunk(pos)? else {
            return Err(LightProviderError::MissingChunk(pos));
        };
        let put_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.put_chunk(pos, chunk)));
        match put_result {
            Err(_) | Ok(Err(LightProviderError::CallbackPanicked)) => {
                Err(LightProviderError::CallbackPanicked)
            }
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error),
        }
    }

    /// The idempotent re-light of a chunk whose neighbours already carry their
    /// final light — `relightChunks`' per-neighbour
    /// `lightChunk(lightAccess, chunk, false)`. The no-edge-checks path pulls
    /// the neighbours' lateral light into the increase queue
    /// (`propagate_neighbour_levels`), so a committed interior chunk lit
    /// against committed neighbours reproduces Paper's byte-identical fixed
    /// point. The differential test drives the committed seed-42 interior
    /// through here.
    pub fn relight_chunk_with(
        &mut self,
        chunk: &mut ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        // Same invariants as [`Self::light_chunk_with`]: return any chunk a
        // previous run stranded before this run begins.
        self.flush_pending_restores()?;
        let mut accessor = CallbackAccessor {
            chunks: &mut self.chunks,
            taken: HashMap::new(),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.engine.relight(&mut accessor, chunk, empty_sections);
            if let Some(nibbles) = self.engine.take_pending_nibbles() {
                chunk.set_sky_nibbles(nibbles);
            }
            if let Some(map) = self.engine.take_pending_emptiness_map() {
                chunk.set_sky_emptiness_map(Some(map));
            }
        }));
        if let Err(payload) = result {
            let taken = std::mem::take(&mut accessor.taken);
            drop(accessor);
            self.restore_chunks(taken);
            std::panic::resume_unwind(payload);
        }
        let taken = std::mem::take(&mut accessor.taken);
        drop(accessor);
        if self.restore_chunks(taken) {
            return Err(LightProviderError::CallbackPanicked);
        }
        Ok(())
    }

    /// `StarLightInterface.getSkyLightValue(blockPos, chunk)` with the chunk
    /// already resolved. The status-LIGHT gate (`getPersistedStatus().isOrAfter`)
    /// defers (#185) — `ChunkAccess` carries no persisted status — so the
    /// not-usable gate is `isLightCorrect` plus the null chunk, matching Java's
    /// server branch.
    pub fn get_sky_light_value_in(
        &self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        pos: BlockPos,
    ) -> i32 {
        if !self.has_sky_light {
            return 0;
        }
        let x = pos.get_x();
        let mut y = pos.get_y();
        let z = pos.get_z();
        let min_section = self.min_section;
        let max_section = self.max_section;
        let min_light_section = self.min_light_section;
        let max_light_section = self.max_light_section;

        if !chunk.is_light_correct() {
            return 15;
        }

        let mut section_y = y >> 4;
        if section_y > max_light_section {
            return 15;
        }
        if section_y < min_light_section {
            section_y = min_light_section;
            y = section_y << 4;
        }

        let nibbles = chunk.sky_nibbles();
        let immediate = &nibbles[(section_y - min_light_section) as usize];
        if !immediate.is_null_nibble_visible() {
            return immediate.get_visible(x, y, z);
        }

        let Some(emptiness_map) = chunk.sky_emptiness_map() else {
            return 15;
        };

        // Are we above this chunk's lowest empty section? Walk the world
        // sections from the top down for the lowest non-empty one.
        let mut lowest_y = min_light_section - 1;
        for curr_y in (min_section..=max_section).rev() {
            if emptiness_map[(curr_y - min_section) as usize] {
                continue;
            }
            lowest_y = curr_y;
            break;
        }

        if section_y > lowest_y {
            return 15;
        }

        // This nibble depends solely on the skylight data above it: find the
        // first non-null data above (one exists, as the walk just found it).
        for curr_y in (section_y + 1)..=max_light_section {
            let nibble = &nibbles[(curr_y - min_light_section) as usize];
            if !nibble.is_null_nibble_visible() {
                return nibble.get_visible(x, 0, z);
            }
        }

        15
    }

    /// `StarLightInterface.getBlockLightValue(blockPos, chunk)` with the chunk
    /// already resolved. Java checks neither `isLightCorrect` nor status here —
    /// only the light flag, the section bounds, and the null chunk.
    pub fn get_block_light_value_in(
        &self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        pos: BlockPos,
    ) -> i32 {
        if !self.has_block_light {
            return 0;
        }
        let y = pos.get_y();
        let cy = y >> 4;
        if cy < self.min_light_section || cy > self.max_light_section {
            return 0;
        }
        let nibble = &chunk.block_nibbles()[(cy - self.min_light_section) as usize];
        nibble.get_visible(pos.get_x(), y, pos.get_z())
    }

    /// `LayerLightEventListener.getDataLayerData(SectionPos)` (the sky reader)
    /// with the chunk already resolved. The status-LIGHT gate defers (#185);
    /// the `isLightCorrect` gate, the section bounds, and the sky-emptiness-map
    /// requirement mirror Java.
    pub fn get_data_layer_data_in(
        &self,
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
        pos: SectionPos,
    ) -> Option<DataLayer> {
        if !chunk.is_light_correct() {
            return None;
        }
        let section_y = pos.y();
        if section_y > self.max_light_section || section_y < self.min_light_section {
            return None;
        }
        chunk.sky_emptiness_map()?;
        chunk.sky_nibbles()[(section_y - self.min_light_section) as usize].to_vanilla_nibble()
    }
}

/// The engine's [`ChunkAccessor`] over the provider's narrow take/put callback.
/// Each neighbour the engine resolves during a run is taken as owned, buffered,
/// and handed out as a reference; [`SkyLightProvider::light_chunk_with`] puts
/// the buffered chunks back once the run completes — on the panic path too, so
/// the caller's storage always gets every chunk it handed out back.
struct CallbackAccessor<'a> {
    chunks: &'a mut ChunkAccessFn,
    taken: HashMap<(i32, i32), ChunkAccess<StateId, ServerBiomeId, StructureKey>>,
}

impl ChunkAccessor for CallbackAccessor<'_> {
    fn get_chunk_for_lighting(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Option<&ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
        if !self.taken.contains_key(&(chunk_x, chunk_z)) {
            let chunk = (self.chunks)(chunk_x, chunk_z, None)?;
            self.taken.insert((chunk_x, chunk_z), chunk);
        }
        self.taken.get(&(chunk_x, chunk_z))
    }
}

impl StarLightProvider for SkyLightProvider {
    fn block_change(&mut self, _pos: BlockPos) {
        // `StarLightInterface.blockChange` queues a block-light relight; live
        // relighting defers with the block engine and the light queue (#184).
    }

    fn section_change(&mut self, _pos: SectionPos, _new_empty_value: bool) {
        // `StarLightInterface.sectionChange` queues a section-emptiness change;
        // live relighting defers with the light queue (#184).
    }

    fn light_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]) {
        // Preserve the legacy no-op for a missing center; status generation
        // uses `try_light_chunk` below when absence must be observable.
        match self.try_light_chunk(pos, empty_sections) {
            Ok(()) | Err(LightProviderError::MissingChunk(_)) => {}
            Err(LightProviderError::CallbackPanicked) => {
                panic!("light-provider storage callback panicked");
            }
        }
    }

    fn try_light_chunk(
        &mut self,
        pos: ChunkPos,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        SkyLightProvider::try_light_chunk(self, pos, empty_sections)
    }

    fn force_load_in_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]) {
        match self.try_force_load_in_chunk(pos, empty_sections) {
            Ok(()) | Err(LightProviderError::MissingChunk(_)) => {}
            Err(LightProviderError::CallbackPanicked) => {
                panic!("light-provider storage callback panicked");
            }
        }
    }

    fn try_force_load_in_chunk(
        &mut self,
        pos: ChunkPos,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        SkyLightProvider::try_force_load_in_chunk(self, pos, empty_sections)
    }

    fn relight_chunks(&mut self, _chunks: &HashSet<ChunkPos>) {
        // `StarLightInterface.relightChunks` recomputes a set of chunks; the
        // completion callbacks defer with the light queue (#184).
    }

    fn check_chunk_edges(&mut self, _pos: ChunkPos) {
        // `StarLightInterface.checkChunkEdges` re-checks a chunk's edge light;
        // the edge-check unit defers (#184).
    }

    fn get_sky_light_value(&self, _pos: BlockPos) -> i32 {
        // The readers run on `&self`, so they cannot resolve a chunk through
        // the exclusive take/put callback; Java's null-chunk branch is reported
        // faithfully (`getSkyLightValue` returns 15 for a null chunk when sky
        // light is enabled). Callers with a resolved chunk use
        // [`Self::get_sky_light_value_in`].
        if self.has_sky_light { 15 } else { 0 }
    }

    fn get_block_light_value(&self, _pos: BlockPos) -> i32 {
        // `getBlockLightValue` returns 0 for a null chunk (and with no block
        // light). Real reads go through [`Self::get_block_light_value_in`].
        0
    }

    fn get_data_layer_data(&self, _pos: SectionPos) -> Option<DataLayer> {
        // `getDataLayerData` returns null for a null chunk. Real reads go
        // through [`Self::get_data_layer_data_in`].
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_world::level::height_accessor::create as create_accessor;
    use rivet_world::lighting::level_light_engine::LevelLightEngine;
    use rivet_world::lighting::swmr_nibble_array::SwmrNibbleArray;
    use rivet_world::superflat::{SECTION_COUNT, SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    use crate::server::level::level_chunk::{
        container_factory, state_flags, strategies, superflat_content,
    };
    use rivet_world::chunk::upgrade_data::UpgradeData;

    /// The shared chunk map backing a [`storage_closure`] take/put closure.
    type SharedStorage =
        Arc<Mutex<HashMap<(i32, i32), ChunkAccess<StateId, ServerBiomeId, StructureKey>>>>;

    /// The overworld superflat accessor (minY -64, height 384, 24 sections).
    fn overworld() -> SimpleLevelHeightAccessor {
        create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT)
    }

    /// A take/put closure over a shared chunk map — the narrow chunk access
    /// the provider's tests drive. The closure is owned (`'static`, as the
    /// facade boxes the provider), so it holds the map through an
    /// `Arc<Mutex>`; the returned handle lets the test inspect what the
    /// provider took and put back. `storage` is drained into the shared map.
    fn storage_closure(
        storage: &mut HashMap<(i32, i32), ChunkAccess<StateId, ServerBiomeId, StructureKey>>,
    ) -> (ChunkAccessFn, SharedStorage) {
        let shared = Arc::new(Mutex::new(std::mem::take(storage)));
        let closure_shared = Arc::clone(&shared);
        let closure =
            Box::new(
                move |x: i32,
                      z: i32,
                      put: Option<
                    &mut Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>>,
                >| {
                    let mut map = closure_shared.lock().unwrap();
                    if let Some(slot) = put {
                        if let Some(chunk) = slot.take() {
                            map.insert((x, z), chunk);
                        }
                        None
                    } else {
                        map.remove(&(x, z))
                    }
                },
            );
        (closure, shared)
    }

    /// A take/put closure over empty storage — the provider's hostile
    /// "no loaded neighbours" case (the engine's `relaxed` cache setup
    /// tolerates it).
    fn no_chunks() -> ChunkAccessFn {
        Box::new(|_x, _z, _put| None)
    }

    /// The server's superflat chunk at `pos` — a single stone layer at block
    /// y=-64, air everywhere above (mirrors the engine's test helper).
    fn superflat_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let content = superflat_content();
        let height_accessor = overworld();
        ChunkAccess::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            Some(content.sections),
            &|state: &StateId| state_flags(*state),
        )
    }

    /// An all-air chunk at `pos`.
    fn all_air_chunk(pos: ChunkPos) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let height_accessor = overworld();
        ChunkAccess::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            None,
            &|state: &StateId| state_flags(*state),
        )
    }

    /// The `emptySections` argument for a superflat chunk: the stone section
    /// (index 0, world section -4) is non-empty, every other section derives.
    fn superflat_empty_sections() -> Vec<Option<bool>> {
        let mut empty = vec![None; SECTION_COUNT];
        empty[0] = Some(false);
        empty
    }

    fn all_air_empty_sections() -> Vec<Option<bool>> {
        vec![Some(true); SECTION_COUNT]
    }

    /// The primary path end to end: an explicitly supplied in-progress chunk is
    /// lit through the engine, and the computed sky nibbles + emptiness map are
    /// published back onto the chunk — the M1 superflat sky contract (the floor
    /// light section is byte-exact `128 zeros + 1920 0xFF`, the section above
    /// all `0xFF`, the section below all 0).
    #[test]
    fn light_chunk_with_publishes_the_paper_sky_contract_onto_the_chunk() {
        let mut provider = SkyLightProvider::new(overworld(), true, true, no_chunks());
        let mut chunk = superflat_chunk(ChunkPos::new(0, 0));

        provider
            .light_chunk_with(&mut chunk, &superflat_empty_sections())
            .expect("lighting succeeds");

        let nibbles = chunk.sky_nibbles();
        assert_eq!(nibbles.len(), SECTION_COUNT + 2);
        let floor = nibbles[1].to_vanilla_nibble().expect("floor initialised");
        let floor_data = floor.get_data();
        assert_eq!(&floor_data[..128], &[0u8; 128][..]);
        assert_eq!(&floor_data[128..], &[0xFFu8; 1920][..]);
        assert_eq!(
            nibbles[2]
                .to_vanilla_nibble()
                .expect("above initialised")
                .get_data(),
            vec![0xFFu8; 2048]
        );
        // The section below the world floor is untouched.
        for y in 0..16 {
            for x in 0..16 {
                for z in 0..16 {
                    assert_eq!(nibbles[0].get_visible(x, y, z), 0);
                }
            }
        }
        // The emptiness map: the stone section non-empty, every air section empty.
        let map = chunk.sky_emptiness_map().expect("published");
        assert!(!map[0]);
        assert!(map[1..].iter().all(|&empty| empty));
    }

    /// A fully-empty chunk produces no light: the sky nibbles stay `Null` and
    /// the emptiness map is uniformly empty (the fully-exposed sky is
    /// represented by null sections).
    #[test]
    fn light_chunk_with_on_all_air_keeps_null_nibbles_and_all_empty_map() {
        let mut provider = SkyLightProvider::new(overworld(), true, true, no_chunks());
        let mut chunk = all_air_chunk(ChunkPos::new(0, 0));

        provider
            .light_chunk_with(&mut chunk, &all_air_empty_sections())
            .expect("lighting succeeds");

        for nibble in chunk.sky_nibbles() {
            assert!(nibble.is_null_nibble_visible());
        }
        assert!(
            chunk
                .sky_emptiness_map()
                .expect("published")
                .iter()
                .all(|&e| e)
        );
    }

    /// The trait path is the take/put round-trip through the narrow callback:
    /// the chunk at the position is taken, lit, and put back carrying the
    /// computed light. A position with no chunk is a no-op (the pipeline cannot
    /// light nothing).
    #[test]
    fn light_chunk_trait_path_takes_lights_and_puts_back() {
        let mut storage = HashMap::new();
        let center = ChunkPos::new(0, 0);
        storage.insert((center.x(), center.z()), superflat_chunk(center));
        let (chunks, shared) = storage_closure(&mut storage);
        let mut provider = SkyLightProvider::new(overworld(), true, true, chunks);

        provider.light_chunk(center, &superflat_empty_sections());

        let map = shared.lock().unwrap();
        let put_back = map
            .get(&(center.x(), center.z()))
            .expect("chunk put back into the caller's storage");
        let floor_data = put_back.sky_nibbles()[1]
            .to_vanilla_nibble()
            .expect("floor initialised")
            .get_data();
        assert_eq!(&floor_data[..128], &[0u8; 128][..]);
        assert_eq!(&floor_data[128..], &[0xFFu8; 1920][..]);
        assert!(
            !put_back.sky_emptiness_map().expect("published")[0],
            "stone section is non-empty"
        );
        drop(map);

        // A position with no chunk is a no-op: no panic, storage unchanged.
        let before: Vec<_> = shared.lock().unwrap().keys().copied().collect();
        provider.light_chunk(ChunkPos::new(5, 5), &superflat_empty_sections());
        let after: Vec<_> = shared.lock().unwrap().keys().copied().collect();
        assert_eq!(before, after);
    }

    #[test]
    fn checked_light_refuses_a_missing_center_without_promotion() {
        let mut provider = SkyLightProvider::new(overworld(), true, true, no_chunks());
        assert_eq!(
            provider.try_light_chunk(ChunkPos::ZERO, &superflat_empty_sections()),
            Err(LightProviderError::MissingChunk(ChunkPos::ZERO))
        );
    }

    #[test]
    fn put_callback_panic_retains_the_center_for_retry() {
        let first_put = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let shared = Arc::new(Mutex::new(HashMap::from([(
            (0, 0),
            superflat_chunk(ChunkPos::ZERO),
        )])));
        let closure_shared = Arc::clone(&shared);
        let chunks: ChunkAccessFn = Box::new(move |x, z, put| {
            if put.is_some() && first_put.swap(false, Ordering::SeqCst) {
                panic!("put callback failed once");
            }
            let mut storage = closure_shared.lock().unwrap();
            if let Some(slot) = put {
                if let Some(chunk) = slot.take() {
                    storage.insert((x, z), chunk);
                }
                None
            } else {
                storage.remove(&(x, z))
            }
        });
        let mut provider = SkyLightProvider::new(overworld(), true, true, chunks);

        assert_eq!(
            provider.try_light_chunk(ChunkPos::ZERO, &superflat_empty_sections()),
            Err(LightProviderError::CallbackPanicked)
        );
        assert_eq!(provider.pending_restores.len(), 1);
        assert!(!shared.lock().unwrap().contains_key(&(0, 0)));

        provider
            .try_light_chunk(ChunkPos::ZERO, &superflat_empty_sections())
            .expect("retry after storage repair succeeds");
        assert!(shared.lock().unwrap().contains_key(&(0, 0)));
    }

    /// A neighbour whose put callback panicked on one run is stranded in
    /// `pending_restores` (the centre's put retained it). The *value bridge*
    /// calls `light_chunk_with` directly — which now flushes pending restores
    /// before the next run — so a retry returns the stranded neighbour to its
    /// original slot before lighting again. This is the bridge-retry resource
    /// invariant the reviewer flagged.
    #[test]
    fn light_chunk_with_flushes_a_stranded_neighbour_before_the_next_run() {
        // Panic once when putting the neighbour back; the centre's own put is
        // on the caller's storage too, so the one-time panic strands whichever
        // put arrives first. The centre must remain present for the retry.
        let first_put = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mut storage = HashMap::new();
        let center = ChunkPos::new(0, 0);
        storage.insert((center.x(), center.z()), superflat_chunk(center));
        let mut neighbour = superflat_chunk(ChunkPos::new(1, 0));
        neighbour.set_light_correct(true);
        storage.insert((1, 0), neighbour);
        let (closure_fn, shared) = {
            let first_put = Arc::clone(&first_put);
            let shared = Arc::new(Mutex::new(storage));
            let closure_shared = Arc::clone(&shared);
            let closure: ChunkAccessFn = Box::new(move |x, z, put| {
                if put.is_some() && first_put.swap(false, Ordering::SeqCst) {
                    panic!("put callback failed once");
                }
                let mut storage = closure_shared.lock().unwrap();
                if let Some(slot) = put {
                    if let Some(chunk) = slot.take() {
                        storage.insert((x, z), chunk);
                    }
                    None
                } else {
                    storage.remove(&(x, z))
                }
            });
            (closure, shared)
        };
        let mut provider = SkyLightProvider::new(overworld(), true, true, closure_fn);
        let mut chunk = superflat_chunk(center);

        // First run: one put panics and strands its chunk in pending_restores.
        assert_eq!(
            provider.light_chunk_with(&mut chunk, &superflat_empty_sections()),
            Err(LightProviderError::CallbackPanicked)
        );
        assert_eq!(provider.pending_restores.len(), 1);

        // Retry through the same value path: the stranded neighbour is flushed
        // and put back into the caller's storage before the next run begins.
        provider
            .light_chunk_with(&mut chunk, &superflat_empty_sections())
            .expect("retry succeeds");
        let map = shared.lock().unwrap();
        assert!(
            map.contains_key(&(0, 0)),
            "centre restored to its original slot"
        );
        assert!(
            map.contains_key(&(1, 0)),
            "stranded neighbour returned to its original slot"
        );
        assert!(
            provider.pending_restores.is_empty(),
            "no chunk remains owned by the provider"
        );
    }

    /// A light-correct neighbour present in the caller's storage is resolved by
    /// the engine during the run, buffered, and put back — not dropped, not
    /// fabricated. The centre chunk's own light is unaffected.
    #[test]
    fn neighbours_are_taken_and_put_back_around_the_run() {
        let mut storage = HashMap::new();
        let center = ChunkPos::new(0, 0);
        let center_chunk = superflat_chunk(center);
        let mut neighbour = superflat_chunk(ChunkPos::new(1, 0));
        neighbour.set_light_correct(true);
        storage.insert((center.x(), center.z()), center_chunk);
        storage.insert(
            (neighbour.get_pos().x(), neighbour.get_pos().z()),
            neighbour,
        );
        let (chunks, shared) = storage_closure(&mut storage);
        let mut provider = SkyLightProvider::new(overworld(), true, true, chunks);

        provider.light_chunk(center, &superflat_empty_sections());

        let map = shared.lock().unwrap();
        // The neighbour survived the run in the caller's storage, still
        // light-correct, its sections intact (nothing fabricated).
        let neighbour_back = map.get(&(1, 0)).expect("neighbour put back after the run");
        assert!(neighbour_back.is_light_correct());
        assert_eq!(neighbour_back.get_sections().len(), SECTION_COUNT);
        // And the centre chunk was lit through the engine (its floor byte
        // contract holds with the neighbour present).
        let floor_data = map[&(0, 0)].sky_nibbles()[1]
            .to_vanilla_nibble()
            .expect("floor initialised")
            .get_data();
        assert_eq!(&floor_data[128..], &[0xFFu8; 1920][..]);
    }

    /// A panicking engine run must not leak caller-storage chunks: the engine's
    /// finally-equivalent clears its caches, `light_chunk_with` returns every
    /// taken neighbour, `light_chunk` returns the taken centre, and the
    /// original panic reaches the caller.
    ///
    /// The panic is triggered from inside the engine's `light` try-body, not by
    /// the test's storage closure: a light-correct radius-1 neighbour whose
    /// sky-nibble array is truncated leaves the missing light sections' cache
    /// slots empty, so the emptiness-change init (`initNibble` with
    /// `initRemovedNibbles=false`) hits Starlight's own "nibble removed while
    /// not requested" panic.
    #[test]
    fn panicking_run_returns_every_taken_chunk_and_preserves_the_panic() {
        let mut storage = HashMap::new();
        let center = ChunkPos::new(0, 0);
        storage.insert((center.x(), center.z()), superflat_chunk(center));
        let mut neighbour = superflat_chunk(ChunkPos::new(1, 0));
        neighbour.set_light_correct(true);
        neighbour.set_sky_nibbles(vec![SwmrNibbleArray::new()]);
        storage.insert((1, 0), neighbour);
        let (chunks, shared) = storage_closure(&mut storage);
        let mut provider = SkyLightProvider::new(overworld(), true, true, chunks);

        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            provider.light_chunk(center, &superflat_empty_sections());
        }))
        .expect_err("the engine's internal panic must propagate out of the provider");

        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .expect("Starlight's static-str panic message");
        assert!(
            message.contains("nibble removed while not requested"),
            "the original engine panic reaches the caller, got: {message:?}"
        );

        // The engine's finally-equivalent cleared the per-run caches even
        // though the run unwound mid-body.
        assert!(
            provider.engine.per_run_caches_are_clear(),
            "destroyCaches runs before light re-unwinds"
        );

        // Every chunk taken from the caller's storage is back, unchanged: the
        // centre was never lit and the neighbour kept its light-correct flag
        // and its truncated nibble array.
        let map = shared.lock().unwrap();
        let keys: std::collections::BTreeSet<(i32, i32)> = map.keys().copied().collect();
        assert_eq!(
            keys,
            [(center.x(), center.z()), (1, 0)].into_iter().collect()
        );
        let centre_back = &map[&(center.x(), center.z())];
        assert_eq!(centre_back.get_sections().len(), SECTION_COUNT);
        assert!(
            centre_back
                .sky_nibbles()
                .iter()
                .all(SwmrNibbleArray::is_null_nibble_visible),
            "the centre was never lit — its nibbles stay untouched"
        );
        let neighbour_back = &map[&(1, 0)];
        assert!(neighbour_back.is_light_correct());
        assert_eq!(
            neighbour_back.sky_nibbles().len(),
            1,
            "the neighbour returns unchanged"
        );
    }

    /// The readers' pos-only variants faithfully report Java's null-chunk
    /// branches (the `&self` readers cannot resolve a chunk through the
    /// exclusive take/put callback).
    #[test]
    fn readers_report_the_null_chunk_branch_faithfully() {
        let provider = SkyLightProvider::new(overworld(), true, true, no_chunks());
        let pos = BlockPos::new(0, 64, 0);
        // `getSkyLightValue` with a null chunk returns 15 (sky enabled).
        assert_eq!(provider.get_sky_light_value(pos), 15);
        // `getBlockLightValue` with a null chunk returns 0.
        assert_eq!(provider.get_block_light_value(pos), 0);
        // `getDataLayerData` with a null chunk returns null.
        assert!(
            provider
                .get_data_layer_data(SectionPos::of(0, 4, 0))
                .is_none()
        );

        // With no sky light the sky reader returns 0 before the chunk check.
        let block_only = SkyLightProvider::new(overworld(), false, true, no_chunks());
        assert_eq!(block_only.get_sky_light_value(pos), 0);
        assert_eq!(block_only.get_block_light_value(pos), 0);
    }

    /// The resolved-chunk readers read the computed nibbles: the sky reader
    /// returns 0 at the stone floor and 15 above it, the data-layer reader
    /// returns the floor's byte-exact layer, and the block reader stays 0
    /// (block light is deferred, #184).
    #[test]
    fn resolved_chunk_readers_see_the_computed_light() {
        let mut provider = SkyLightProvider::new(overworld(), true, true, no_chunks());
        let mut chunk = superflat_chunk(ChunkPos::new(0, 0));
        provider
            .light_chunk_with(&mut chunk, &superflat_empty_sections())
            .expect("lighting succeeds");
        chunk.set_light_correct(true);

        // Stone at y=-64 blocks sky: 0 at the floor, full sky above it.
        assert_eq!(
            provider.get_sky_light_value_in(&chunk, BlockPos::new(0, -64, 0)),
            0
        );
        assert_eq!(
            provider.get_sky_light_value_in(&chunk, BlockPos::new(0, -63, 0)),
            15
        );
        assert_eq!(
            provider.get_sky_light_value_in(&chunk, BlockPos::new(0, 64, 0)),
            15
        );
        // A not-light-correct chunk reads as 15 (Java's server gate).
        let raw = superflat_chunk(ChunkPos::new(0, 0));
        assert_eq!(
            provider.get_sky_light_value_in(&raw, BlockPos::new(0, 64, 0)),
            15
        );

        // The data-layer reader returns the floor's byte-exact layer.
        let floor = provider
            .get_data_layer_data_in(&chunk, SectionPos::of(0, -4, 0))
            .expect("floor layer present");
        let data = floor.get_data();
        assert_eq!(&data[..128], &[0u8; 128][..]);
        assert_eq!(&data[128..], &[0xFFu8; 1920][..]);
        // Block light is deferred: the reader reports 0 / no layer.
        assert_eq!(
            provider.get_block_light_value_in(&chunk, BlockPos::new(0, 64, 0)),
            0
        );

        // An all-air chunk reads as open sky everywhere (15 via the
        // emptiness-map fallback), and its data layer is null (all null nibbles).
        let mut air = all_air_chunk(ChunkPos::new(0, 0));
        provider
            .light_chunk_with(&mut air, &all_air_empty_sections())
            .expect("lighting succeeds");
        air.set_light_correct(true);
        assert_eq!(
            provider.get_sky_light_value_in(&air, BlockPos::new(0, 64, 0)),
            15
        );
        assert!(
            provider
                .get_data_layer_data_in(&air, SectionPos::of(0, 4, 0))
                .is_none()
        );
    }

    /// The provider boxes for the facade's `with_provider` and stays exclusive:
    /// `Send` (moves onto the tick thread) but never `Sync` — the OWNERSHIP.md
    /// single-owner confinement.
    #[test]
    fn provider_boxes_for_the_facade_and_is_send_not_sync() {
        let mut storage = HashMap::new();
        let (chunks, _shared) = storage_closure(&mut storage);
        let mut engine = LevelLightEngine::with_provider(
            Box::new(overworld()),
            true,
            true,
            Box::new(SkyLightProvider::new(overworld(), true, true, chunks)),
        );
        assert!(engine.provider().is_some());
        assert!(engine.provider_mut().is_some());
        // The impl mutates through the exclusive `provider_mut` seam without
        // panicking.
        engine
            .provider_mut()
            .expect("attached")
            .block_change(BlockPos::new(1, 64, 2));

        fn assert_send<T: Send>() {}
        assert_send::<SkyLightProvider>();
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            struct Invalid;
            impl<T: ?Sized + Sync> AmbiguousIfImpl<Invalid> for T {}
            // Resolves only if `SkyLightProvider` does NOT implement Sync.
            let _ = <SkyLightProvider as AmbiguousIfImpl<_>>::some_item;
        };
    }

    // --- seed-42 LIGHT differential (Paper 26.2, commit 0a99345) ---

    use rivet_nbt::nbt_io;
    use rivet_registry::block_state::BlockState;
    use rivet_util::DataInputStream;
    use rivet_world::chunk::storage::reconstruct_runtime_chunk;
    use rivet_world::chunk::storage::section_reconstruction::BiomeId as WorldgenBiomeId;
    use rivet_world::chunk::storage::serializable_chunk_data::SerializableChunkData;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;

    /// The committed `light.json` golden — the shape `light_stage.rs` captures.
    /// The test mirrors the oracle's truth struct locally because `rivet-server`
    /// cannot depend on `rivet-oracle` (the derive is dev-only).
    #[derive(serde::Deserialize)]
    struct LightGolden {
        seed: i64,
        format: u32,
        chunks: std::collections::BTreeMap<String, ChunkLightTruth>,
    }

    #[derive(serde::Deserialize)]
    struct ChunkLightTruth {
        stored_pos: [i32; 2],
        status: String,
        light_correct: bool,
        min_light_section: i32,
        max_light_section: i32,
        sky_nibbles: std::collections::BTreeMap<i32, Option<Vec<u8>>>,
        sky_emptiness: Vec<bool>,
    }

    fn load_light_golden() -> LightGolden {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/light/light.json");
        let bytes = fs::read(path).expect("seed-42 light.json fixture readable");
        serde_json::from_slice(&bytes).expect("seed-42 light.json parses")
    }

    /// The committed forced-grid positions ({18..22}²) and the committed
    /// interior ({19..21}²) — the same coordinate contracts `light_stage.rs`
    /// pins. The 5x5 grid is the exact set Paper lit the interior against; the
    /// differential reproduces that loaded set verbatim.
    fn forced_coordinates() -> Vec<(i32, i32)> {
        (18..=22)
            .flat_map(|x| (18..=22).map(move |z| (x, z)))
            .collect()
    }

    fn committed_coordinates() -> Vec<(i32, i32)> {
        (19..=21)
            .flat_map(|x| (19..=21).map(move |z| (x, z)))
            .collect()
    }

    /// Rebuild one committed seed-42 LIGHT chunk NBT into the server chunk the
    /// provider lights: the boot's `SerializableChunkData.read` path
    /// (`reconstruct_runtime_chunk`), the `from_bridge` value re-encode into
    /// `StateId`, then the base is moved out via the `LevelChunk::into_base`
    /// seam. The captured post-light state carries the neighbour light the
    /// engine consumes as its initial conditions.
    fn rebuild_light_fixture_chunk(
        cx: i32,
        cz: i32,
        height_accessor: SimpleLevelHeightAccessor,
    ) -> ChunkAccess<StateId, ServerBiomeId, StructureKey> {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../tools/rivet-oracle/fixtures/light/chunks/{cx}.{cz}.nbt"
        ));
        let bytes = fs::read(&fixture).expect("seed-42 light chunk fixture readable");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        let tag = nbt_io::read_unlimited(&mut input).expect("seed-42 light chunk fixture parses");
        let data = SerializableChunkData::parse(height_accessor, &tag)
            .expect("seed-42 light chunk parses")
            .expect("seed-42 light chunk carries Status");
        let reconstruction =
            reconstruct_runtime_chunk(ChunkPos::new(cx, cz), data, height_accessor, true)
                .expect("seed-42 light chunk reconstructs");
        let (block_strategy, biome_strategy) = strategies();
        reconstruction
            .chunk
            .map_values(
                block_strategy,
                biome_strategy,
                StateId(0),
                ServerBiomeId(40),
                &|state: &BlockState| state.id(),
                &|biome: &WorldgenBiomeId| ServerBiomeId(biome.0),
                &|state: &StateId| state_flags(*state),
            )
            .expect("seed-42 light chunk value re-encode")
            .into_base()
    }

    /// The chunk's in-memory sky emptiness map: per world section `has_only_air`
    /// (`absent or only air → empty`). Paper does not serialize the emptiness
    /// map and the reconstruction does not rebuild it, so the test derives it
    /// exactly like `light_stage.rs` (and Paper's `getEmptySectionsForChunk`).
    fn sky_emptiness_from_sections(
        chunk: &ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) -> Vec<bool> {
        chunk
            .get_sections()
            .iter()
            .map(|s| s.has_only_air())
            .collect()
    }

    /// The full differential: rebuild the forced 5x5 from the committed NBTs
    /// (each carrying its captured post-light neighbour state), then re-light
    /// the committed 3x3 interior through the real engine and compare the
    /// published sky nibbles + emptiness map byte-exact against the Paper
    /// checkpoint.
    ///
    /// The captured light is a fixed point: Paper computed every committed
    /// chunk over exactly this loaded set, and the idempotent re-light
    /// (`relightChunks`' no-edge-checks path — `propagate_neighbour_levels`
    /// pulls the neighbours' lateral light into the increase queue) reproduces
    /// that same final light. So the fixture truth is the expected engine
    /// output, not merely a stored echo. (`light()`'s edge-checks path would
    /// not: it re-fills sky from above and only decreases edges, so it cannot
    /// reproduce the east-neighbour water pull at the boundary columns.)
    #[test]
    fn seed42_light_engine_matches_the_paper_light_checkpoint() {
        let golden = load_light_golden();
        assert_eq!(golden.seed, 42);
        assert_eq!(golden.format, 1);

        let height_accessor = overworld();
        let min_light = height_accessor.get_min_section_y() - 1;
        let max_light = height_accessor.get_max_section_y() + 1;

        let mut storage = HashMap::new();
        for (cx, cz) in forced_coordinates() {
            let mut chunk = rebuild_light_fixture_chunk(cx, cz, height_accessor);
            chunk.set_sky_emptiness_map(Some(sky_emptiness_from_sections(&chunk)));
            storage.insert((cx, cz), chunk);
        }

        let (chunks, shared) = storage_closure(&mut storage);
        let mut provider = SkyLightProvider::new(height_accessor, true, true, chunks);

        let mut compared_light_sections = 0usize;
        for (cx, cz) in committed_coordinates() {
            let key = format!("{cx},{cz}");
            let truth = golden
                .chunks
                .get(&key)
                .unwrap_or_else(|| panic!("committed seed-42 truth {key} present"));
            assert_eq!(truth.stored_pos, [cx, cz]);
            assert_eq!(truth.status, "minecraft:full");
            assert_eq!(truth.min_light_section, min_light);
            assert_eq!(truth.max_light_section, max_light);
            assert!(truth.light_correct);

            let mut center = shared
                .lock()
                .unwrap()
                .remove(&(cx, cz))
                .expect("center chunk present");
            provider
                .relight_chunk_with(&mut center, &[None; SECTION_COUNT])
                .expect("relighting succeeds");
            shared.lock().unwrap().insert((cx, cz), center);

            let shared_guard = shared.lock().unwrap();
            let lit = shared_guard
                .get(&(cx, cz))
                .expect("center chunk present after run");
            for (index, y) in (min_light..=max_light).enumerate() {
                let published = lit.sky_nibbles()[index]
                    .to_vanilla_nibble()
                    .map(|layer| layer.get_data());
                let captured = truth
                    .sky_nibbles
                    .get(&y)
                    .unwrap_or_else(|| panic!("captured section {y} for {key}"));
                assert_eq!(
                    &published, captured,
                    "chunk {key} section {y} published sky light differs from Paper"
                );
                if published.is_some() {
                    compared_light_sections += 1;
                }
            }
            assert_eq!(
                lit.sky_emptiness_map(),
                Some(truth.sky_emptiness.as_slice()),
                "chunk {key} emptiness map"
            );
            assert_eq!(lit.is_light_correct(), truth.light_correct, "chunk {key}");
        }
        assert!(
            compared_light_sections > 0,
            "the engine published no light — the comparison is vacuous"
        );
    }
}

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
//! [`SkyLightProvider`] replaces the phase-A [`StubStarLightProvider`] no-op
//! with a real synchronous compute layer: it owns a
//! [`SkyStarLightEngine`] (the Starlight flood-fill core) and drives it on an
//! explicitly supplied in-progress chunk through [`SkyLightProvider::light_chunk_with`],
//! publishing the engine's computed sky nibbles and sky-emptiness map back onto
//! the chunk (Java's `ChunkLightTask` → `setNibbles`/`setEmptinessMap`).
//!
//! The engine resolves neighbour chunks through the provider's *narrow chunk
//! access* — a take/put closure over the caller's chunk storage, mirroring
//! Java's `LightChunkGetter` without a global `ChunkMap` lookup or any shared
//! authoritative state (OWNERSHIP.md). The caller hands an owned chunk out on
//! take (`None` payload) and receives it back on put (`Some` payload); the
//! provider buffers the neighbours it resolves during a run and returns them
//! afterwards. No neighbour is fabricated and none is dropped.
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
//! `world_gen_context.rs` caller drives `light_chunk` through the trait seam;
//! the concrete chunk storage it would resolve into lands with the chunk map).

use std::collections::{HashMap, HashSet};

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::data_layer::DataLayer;
use rivet_world::level::LevelHeightAccessor;
use rivet_world::level::height_accessor::SimpleLevelHeightAccessor;
use rivet_world::lighting::star_light_provider::StarLightProvider;

use super::star_light_engine::{ChunkAccessor, SkyStarLightEngine};
use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, StateId, StructureKey};

/// The narrow chunk access the engine lights through: a take/put closure over
/// the caller's chunk storage. A `None` payload *takes* the chunk at `(x, z)`
/// (removing it from the caller's storage); a `Some(chunk)` payload *puts* it
/// back. The provider takes the chunks it lights, buffers any neighbours the
/// engine resolves during the run, and returns them all afterwards — no global
/// `ChunkMap` lookup, no shared authoritative state, no fabricated neighbours.
type ChunkAccessFn = Box<
    dyn FnMut(
            i32,
            i32,
            Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>>,
        ) -> Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>>
        + Send,
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
        }
    }

    /// Take the chunk at `pos` as owned from the caller's storage (`None` when
    /// it is not present).
    fn take_chunk(
        &mut self,
        pos: ChunkPos,
    ) -> Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>> {
        (self.chunks)(pos.x(), pos.z(), None)
    }

    /// Put an owned chunk back at `pos`.
    fn put_chunk(
        &mut self,
        pos: ChunkPos,
        chunk: ChunkAccess<StateId, ServerBiomeId, StructureKey>,
    ) {
        (self.chunks)(pos.x(), pos.z(), Some(chunk));
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
    ) {
        let mut accessor = CallbackAccessor {
            chunks: &mut self.chunks,
            taken: HashMap::new(),
        };
        self.engine.light(&mut accessor, chunk, empty_sections);
        if let Some(nibbles) = self.engine.take_pending_nibbles() {
            chunk.set_sky_nibbles(nibbles);
        }
        if let Some(map) = self.engine.take_pending_emptiness_map() {
            chunk.set_sky_emptiness_map(Some(map));
        }
        // Return the neighbours the run resolved — drop the accessor to end its
        // borrow of the callback, then put each buffered chunk back.
        let taken = std::mem::take(&mut accessor.taken);
        drop(accessor);
        for ((x, z), neighbour) in taken {
            (self.chunks)(x, z, Some(neighbour));
        }
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
/// the buffered chunks back once the run completes.
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
        // `StarLightInterface.lightChunk(chunk, emptySections)` — the pos-only
        // seam resolves the chunk through the take/put callback (Java's
        // `ChunkLightTask` passes the chunk directly; the seam passes the
        // position and the impl resolves it). A chunk absent from the caller's
        // storage is a no-op — nothing to light.
        let Some(mut chunk) = self.take_chunk(pos) else {
            return;
        };
        self.light_chunk_with(&mut chunk, empty_sections);
        self.put_chunk(pos, chunk);
    }

    fn force_load_in_chunk(&mut self, pos: ChunkPos, _empty_sections: &[Option<bool>]) {
        // `StarLightInterface.forceLoadInChunk` confirms an already-lighted
        // chunk in place without recomputing (the LIGHT task's already-lighted
        // branch). This slice has no loaded-chunk registry to add to, so the
        // confirmation is the take/put round-trip; the registry effect defers
        // with the chunk map (#184).
        let Some(chunk) = self.take_chunk(pos) else {
            return;
        };
        self.put_chunk(pos, chunk);
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
    use rivet_world::superflat::{SECTION_COUNT, SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y};
    use std::sync::{Arc, Mutex};

    use crate::server::level::level_chunk::{container_factory, state_flags, superflat_content};
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
        let closure = Box::new(
            move |x: i32,
                  z: i32,
                  put: Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>>| {
                let mut map = closure_shared.lock().unwrap();
                if let Some(chunk) = put {
                    map.insert((x, z), chunk);
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
        Box::new(|_x, _z, put: Option<ChunkAccess<StateId, ServerBiomeId, StructureKey>>| put)
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

        provider.light_chunk_with(&mut chunk, &superflat_empty_sections());

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

        provider.light_chunk_with(&mut chunk, &all_air_empty_sections());

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
        provider.light_chunk_with(&mut chunk, &superflat_empty_sections());
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
        provider.light_chunk_with(&mut air, &all_air_empty_sections());
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
}

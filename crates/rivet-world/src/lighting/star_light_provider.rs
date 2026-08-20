//! The light-provider seam that breaks the `rivet-world` ↔ `rivet-server`
//! crate cycle — the port's stand-in for Paper's `StarLightLightingProvider`
//! + `StarLightInterface` surface.
//!
//! Java: `StarLightInterface.java` and `StarLightLightingProvider.java` in
//! `working/Paper`. In Java the cycle is broken by the *direction of the
//! reference*: Paper's `ThreadedLevelLightEngine` (a `LevelLightEngine`
//! subclass, server layer) implements `StarLightLightingProvider`, exposing
//! `starlight$getLightEngine() -> StarLightInterface`, and `StarLightInterface`
//! (the compute engine) holds a `public final LevelLightEngine lightEngine`
//! back-reference.
//!
//! The port keeps that reference direction but splits it across the crate
//! boundary so no crate cycle forms. `rivet-world` owns this `StarLightProvider`
//! trait — the mutator/reader surface the facade calls on
//! `starlight$getLightEngine()` — and `rivet-server` owns the concrete impl
//! (the `ca.spottedleaf.moonrise.patches.starlight.light` manifest unit;
//! `star_light_provider_impl::SkyLightProvider`, a real synchronous layer over
//! the `SkyStarLightEngine` compute core). [`LevelLightEngine`](crate::lighting::level_light_engine)
//! holds an `Option<Box<dyn StarLightProvider + Send>>`; without the trait, a
//! concrete `StarLightInterface` (compute, rivet-server) inside
//! `LevelLightEngine` (facade, rivet-world) would create a
//! `rivet-world -> rivet-server` dependency edge.
//!
//! The op surface mirrors `StarLightInterface`: `blockChange`, `sectionChange`,
//! `lightChunk`, `forceLoadInChunk`, `relightChunks`, `checkChunkEdges`, plus
//! the sky/block readers (`getSkyLightValue`, `getBlockLightValue`,
//! `getDataLayerData`). The trait is
//! object-safe (mutators take `&mut self`, readers `&self`, no generic
//! parameters) so the facade can hold it as `Box<dyn>`, and ownership is
//! exclusive: the facade owns the provider and hands out `&mut`/`&` — never
//! shared, never `Sync` (the OWNERSHIP.md single-owner tick-thread model).
//!
//! RivetTodo(#184): the light queue and the block engine, live
//! `blockChange`/`sectionChange`/`relightChunks`/`checkChunkEdges`, the client
//! notify path, and the final generated-serving pipeline wiring are not ported.
//! The ops Java routes through a `ChunkAccess`/callbacks take the chunk
//! *position* here — the impl resolves the chunk through its own narrow light
//! access (as `StarLightInterface` holds a `LightChunkGetter`), and the
//! `relightChunks` completion callbacks return with the light-queue port.

use std::collections::HashSet;

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};

use crate::chunk::data_layer::DataLayer;

/// A light-provider operation could not complete without fabricating state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightProviderError {
    /// The provider's runtime storage did not contain the requested center.
    MissingChunk(ChunkPos),
    /// A provider-owned storage callback panicked while taking or restoring a chunk.
    CallbackPanicked,
}

/// The Starlight engine surface — `StarLightInterface`'s public ops, object
/// safe so `LevelLightEngine` can own it as `Box<dyn StarLightProvider>`.
pub trait StarLightProvider {
    /// `StarLightInterface.blockChange(BlockPos)` — queue a block-light change
    /// at `pos`. Java returns a `LightQueue.ChunkTasks` task handle; the port
    /// drops the return (the light-queue defers with the engine units, #184).
    fn block_change(&mut self, pos: BlockPos);

    /// `StarLightInterface.sectionChange(SectionPos, boolean)` — queue a
    /// section-emptiness change for `pos` (Java's `newEmptyValue`). Java
    /// returns a `LightQueue.ChunkTasks` task handle; the port drops the return
    /// (the light-queue defers with the engine units, #184).
    fn section_change(&mut self, pos: SectionPos, new_empty_value: bool);

    /// `StarLightInterface.lightChunk(ChunkAccess, Boolean[])` — light a chunk
    /// whose per-section emptiness is `empty_sections`. Java's `Boolean[]` is
    /// tri-state, mirrored as `Option<bool>`: `Some(true)`/`Some(false)` are
    /// `Boolean.TRUE`/`Boolean.FALSE` (section empty / has blocks), `None` is
    /// Java `null` (unspecified — `StarLightEngine` derives it from the section
    /// on first load and leaves it unchanged for an existing chunk). Java passes
    /// the `ChunkAccess`; the object-safe seam passes the chunk *position* and
    /// the impl resolves the chunk through its own light access (as
    /// `StarLightInterface` holds a `LightChunkGetter`).
    fn light_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]);

    /// Fallible variant used by status generation when a missing center must
    /// prevent promotion. Legacy providers default to the infallible seam.
    fn try_light_chunk(
        &mut self,
        pos: ChunkPos,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        self.light_chunk(pos, empty_sections);
        Ok(())
    }

    /// `StarLightInterface.forceLoadInChunk(int, int, Boolean[])` — register a
    /// chunk as loaded for the light engine (its per-section emptiness
    /// `empty_sections`) without recomputing its light. The LIGHT task's
    /// already-lighted branch calls this instead of `lightChunk` (see
    /// `ChunkLightTask.LightTask`), so a chunk that is light-correct and at/after
    /// `LIGHT` is confirmed in place rather than relit.
    fn force_load_in_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]);

    /// Fallible variant used by status generation when a missing center must
    /// prevent promotion. Legacy providers default to the infallible seam.
    fn try_force_load_in_chunk(
        &mut self,
        pos: ChunkPos,
        empty_sections: &[Option<bool>],
    ) -> Result<(), LightProviderError> {
        self.force_load_in_chunk(pos, empty_sections);
        Ok(())
    }

    /// `StarLightInterface.relightChunks(Set<ChunkPos>, Consumer<ChunkPos>,
    /// IntConsumer)` — recompute light for the given chunks. The completion
    /// callbacks are deferred with the light-queue port.
    fn relight_chunks(&mut self, chunks: &HashSet<ChunkPos>);

    /// `StarLightInterface.checkChunkEdges(int, int)` — re-check the chunk's
    /// light at its edges (Java takes the two chunk coordinates).
    fn check_chunk_edges(&mut self, pos: ChunkPos);

    /// Whether [`force_load_in_chunk`](Self::force_load_in_chunk) and
    /// [`check_chunk_edges`](Self::check_chunk_edges) fully reconcile a
    /// persisted light-correct chunk. Providers that only expose the phase-A
    /// callback/no-op seam must return `false`; callers then refuse the load
    /// rather than claiming a chunk is ready without edge correction.
    fn supports_persisted_light_load(&self) -> bool {
        false
    }

    /// `StarLightInterface.getSkyLightValue(BlockPos, ChunkAccess)` — the sky
    /// light at `pos` (Java also takes the already-resolved chunk; the impl
    /// resolves it here).
    fn get_sky_light_value(&self, pos: BlockPos) -> i32;

    /// `StarLightInterface.getBlockLightValue(BlockPos, ChunkAccess)`.
    fn get_block_light_value(&self, pos: BlockPos) -> i32;

    /// `LayerLightEventListener.getDataLayerData(SectionPos)` — the sky/block
    /// `DataLayer` at `pos`, or `None` when the engine has none (Java `null`).
    fn get_data_layer_data(&self, pos: SectionPos) -> Option<DataLayer>;
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// What the recording provider observed, shared so a test holding only the
    /// `Box<dyn StarLightProvider + Send>` can assert what the `dyn` calls did.
    /// The `Arc<Mutex>` is test plumbing only — the production facade never
    /// shares the provider; the provider stays `Send` (no `Rc`/`Cell`).
    #[derive(Default)]
    struct RecordingLog {
        block_changes: Vec<BlockPos>,
        section_changes: Vec<(SectionPos, bool)>,
        lit_chunks: Vec<(ChunkPos, Vec<Option<bool>>)>,
        force_loaded_chunks: Vec<(ChunkPos, Vec<Option<bool>>)>,
        relit_chunks: Vec<ChunkPos>,
        edge_checks: Vec<ChunkPos>,
    }

    /// A concrete `StarLightProvider` recording every call into the shared log.
    #[derive(Clone)]
    struct RecordingProvider {
        log: Arc<Mutex<RecordingLog>>,
    }

    impl RecordingProvider {
        fn new() -> Self {
            RecordingProvider {
                log: Arc::new(Mutex::new(RecordingLog::default())),
            }
        }
    }

    impl StarLightProvider for RecordingProvider {
        fn block_change(&mut self, pos: BlockPos) {
            self.log.lock().unwrap().block_changes.push(pos);
        }
        fn section_change(&mut self, pos: SectionPos, new_empty_value: bool) {
            self.log
                .lock()
                .unwrap()
                .section_changes
                .push((pos, new_empty_value));
        }
        fn light_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]) {
            self.log
                .lock()
                .unwrap()
                .lit_chunks
                .push((pos, empty_sections.to_vec()));
        }
        fn force_load_in_chunk(&mut self, pos: ChunkPos, empty_sections: &[Option<bool>]) {
            self.log
                .lock()
                .unwrap()
                .force_loaded_chunks
                .push((pos, empty_sections.to_vec()));
        }
        fn relight_chunks(&mut self, chunks: &HashSet<ChunkPos>) {
            self.log
                .lock()
                .unwrap()
                .relit_chunks
                .extend(chunks.iter().copied());
        }
        fn check_chunk_edges(&mut self, pos: ChunkPos) {
            self.log.lock().unwrap().edge_checks.push(pos);
        }
        fn supports_persisted_light_load(&self) -> bool {
            true
        }
        fn get_sky_light_value(&self, _pos: BlockPos) -> i32 {
            0
        }
        fn get_block_light_value(&self, _pos: BlockPos) -> i32 {
            0
        }
        fn get_data_layer_data(&self, _pos: SectionPos) -> Option<DataLayer> {
            None
        }
    }

    /// The trait must be usable as a `Box<dyn StarLightProvider + Send>` holder
    /// (object safety) and mutating calls through the `dyn` reference must reach
    /// the concrete impl (delegation). `light_chunk`'s `empty_sections` mirror
    /// Java's `Boolean[]` tri-state, so the call passes all three values and the
    /// log proves they survive the `dyn` boundary unchanged.
    #[test]
    fn object_safe_and_delegates_through_dyn() {
        let recording = RecordingProvider::new();
        let log = Arc::clone(&recording.log);
        let mut provider: Box<dyn StarLightProvider + Send> = Box::new(recording);

        provider.block_change(BlockPos::new(1, 2, 3));
        let pos = SectionPos::of(4, 5, 6);
        provider.section_change(pos, true);
        provider.light_chunk(ChunkPos::new(7, 8), &[Some(false), None, Some(true)]);
        provider.force_load_in_chunk(ChunkPos::new(8, 9), &[Some(true), None, Some(false)]);
        let mut chunks = HashSet::new();
        chunks.insert(ChunkPos::new(9, 10));
        provider.relight_chunks(&chunks);
        provider.check_chunk_edges(ChunkPos::new(11, 12));

        let seen = log.lock().unwrap();
        assert_eq!(seen.block_changes, vec![BlockPos::new(1, 2, 3)]);
        assert_eq!(seen.section_changes, vec![(pos, true)]);
        assert_eq!(
            seen.lit_chunks,
            vec![(ChunkPos::new(7, 8), vec![Some(false), None, Some(true)])]
        );
        assert_eq!(
            seen.force_loaded_chunks,
            vec![(ChunkPos::new(8, 9), vec![Some(true), None, Some(false)])]
        );
        assert_eq!(seen.relit_chunks, vec![ChunkPos::new(9, 10)]);
        assert_eq!(seen.edge_checks, vec![ChunkPos::new(11, 12)]);
    }

    /// A provider also works behind a plain `&mut dyn StarLightProvider` (the
    /// facade's `provider_mut` borrow shape), with readers reachable through
    /// `&dyn StarLightProvider`.
    #[test]
    fn works_behind_plain_dyn_borrow() {
        let mut recording = RecordingProvider::new();
        let log = Arc::clone(&recording.log);
        {
            let provider: &mut dyn StarLightProvider = &mut recording;
            provider.block_change(BlockPos::new(0, 64, 0));
            assert_eq!(provider.get_sky_light_value(BlockPos::new(0, 64, 0)), 0);
            assert!(
                provider
                    .get_data_layer_data(SectionPos::of(0, 4, 0))
                    .is_none()
            );
        }
        // The `&mut dyn` borrow ended: the concrete recording is observable again.
        assert_eq!(
            log.lock().unwrap().block_changes,
            vec![BlockPos::new(0, 64, 0)]
        );
    }
}

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
//! [`StubStarLightProvider`] is the concrete impl for Phase A — a no-op engine
//! (mutators do nothing, readers return 0/`None`). It exists so the seam can be
//! exercised end to end inside the acyclic dependency graph before any
//! propagation is ported. The reader defaults are the stub's own: they do not
//! reproduce every Java branch (e.g. `getSkyLightValue` returns 15 for a null
//! chunk when `hasSkyLight` is true — see its method doc).
//!
//! STUB(ca.spottedleaf.moonrise.patches.starlight.light): the real
//! `StarLightInterface` and its sky/block propagation engines are not ported;
//! this no-op stub stands in until the Starlight unit lands.

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_world::chunk::data_layer::DataLayer;
use rivet_world::lighting::star_light_provider::StarLightProvider;

/// A no-op [`StarLightProvider`] for the phase-A seam — the concrete impl
/// `rivet-server` hands `LevelLightEngine::with_provider` until the real
/// Starlight engine is ported.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubStarLightProvider;

impl StarLightProvider for StubStarLightProvider {
    fn block_change(&mut self, _pos: BlockPos) {
        // `StarLightInterface.blockChange` on an empty world returns without
        // queueing; the real engine queues a block-light task here.
    }

    fn section_change(&mut self, _pos: SectionPos, _new_empty_value: bool) {
        // `StarLightInterface.sectionChange` on an empty world returns without
        // queueing.
    }

    fn light_chunk(&mut self, _pos: ChunkPos, _empty_sections: &[Option<bool>]) {
        // `StarLightInterface.lightChunk` propagates light across the chunk's
        // sections; the stub lights nothing.
    }

    fn relight_chunks(&mut self, _chunks: &std::collections::HashSet<ChunkPos>) {
        // `StarLightInterface.relightChunks` recomputes light for the chunks
        // and invokes the completion callbacks; the stub recomputes nothing.
    }

    fn check_chunk_edges(&mut self, _pos: ChunkPos) {
        // `StarLightInterface.checkChunkEdges` re-checks the chunk's light at
        // its edges; the stub checks nothing.
    }

    fn get_sky_light_value(&self, _pos: BlockPos) -> i32 {
        // `StarLightInterface.getSkyLightValue` with `!hasSkyLight` returns 0.
        // The stub cannot reproduce the `hasSkyLight && chunk == null` branch
        // (Java returns 15 there) because it resolves no chunk, so it returns 0
        // in that case too — a stub simplification, not a Java behavior.
        0
    }

    fn get_block_light_value(&self, _pos: BlockPos) -> i32 {
        // `StarLightInterface.getBlockLightValue` with `!hasBlockLight` (or no
        // world) returns 0.
        0
    }

    fn get_data_layer_data(&self, _pos: SectionPos) -> Option<DataLayer> {
        // `LayerLightEventListener.getDataLayerData` returns null when no
        // chunk/engine has a layer for the section.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_world::level::height_accessor::create as create_accessor;
    use rivet_world::lighting::level_light_engine::LevelLightEngine;

    /// The overworld superflat accessor (minY -64, height 384, 24 sections).
    fn overworld() -> Box<dyn rivet_world::level::height_accessor::LevelHeightAccessor + Send> {
        Box::new(create_accessor(-64, 384))
    }

    /// The phase-A seam end to end, across the crate boundary: a `rivet-world`
    /// facade owning a `rivet-server` provider. The stub has no observable
    /// state, so the test proves the contract — the impl boxes as
    /// `dyn StarLightProvider + Send` (object safety), the facade mutates it
    /// through `provider_mut` (delegation, exclusive ownership), and the
    /// readers report the stub's empty-engine defaults.
    #[test]
    fn stub_provider_plugs_into_the_rivet_world_facade() {
        let mut engine = LevelLightEngine::with_provider(
            overworld(),
            true,
            true,
            Box::new(StubStarLightProvider),
        );
        // The facade's section arithmetic still resolves through the boxed
        // accessor even with the server-side provider attached.
        assert_eq!(engine.get_light_section_count(), 26);
        assert_eq!(engine.get_min_light_section(), -5);
        assert_eq!(engine.get_max_light_section(), 21);

        // Mutators are reachable through the exclusive `provider_mut` seam and
        // do not panic.
        let provider = engine.provider_mut().expect("attached");
        provider.block_change(BlockPos::new(1, 64, 2));
        provider.section_change(SectionPos::of(0, 4, 0), true);
        provider.light_chunk(ChunkPos::new(0, 0), &[Some(false), None]);
        provider.relight_chunks(&Default::default());
        provider.check_chunk_edges(ChunkPos::new(0, 0));

        // Readers report the stub's empty-engine defaults: the sky-light 0 is
        // a stub simplification (Java returns 15 for a null chunk when
        // hasSkyLight — see the method doc), while the block-light 0 is
        // faithful to Java (it too returns 0 for a null chunk), and the data
        // layer is None, Java's null.
        assert_eq!(provider.get_sky_light_value(BlockPos::new(0, 64, 0)), 0);
        assert_eq!(provider.get_block_light_value(BlockPos::new(0, 64, 0)), 0);
        assert!(
            provider
                .get_data_layer_data(SectionPos::of(0, 4, 0))
                .is_none()
        );
    }

    /// The impl also boxes for the facade's `provider`/`provider_mut` borrow
    /// shapes, and the unit struct constructs the same no-op directly.
    #[test]
    fn stub_boxes_for_the_facade_borrow_shapes() {
        let mut engine = LevelLightEngine::with_provider(
            overworld(),
            false,
            true,
            Box::new(StubStarLightProvider),
        );
        assert!(engine.provider().is_some());
        assert!(engine.provider_mut().is_some());
    }
}

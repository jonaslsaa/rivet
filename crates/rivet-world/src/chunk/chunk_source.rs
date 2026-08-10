//! Port of `net.minecraft.world.level.chunk.ChunkSource` (MC 26.2) — the chunk
//! provider seam.
//!
//! Java: `ChunkSource.java` in `working/Paper`. `ChunkSource` is the abstract
//! base of every chunk provider (`ServerChunkCache`, `ClientChunkSource`, ...).
//! Its concrete surface here is the read spine: the `getChunk` convenience
//! overloads, `getChunkNow`, `hasChunk`, `getChunkForLighting` (the
//! `LightChunkGetter` member), `getLightEngine` (deferred), and the no-op
//! `onSectionEmptinessChanged`. The provider implementations that back the
//! abstract `getChunk(x, z, status, load)` resolve through a per-instance
//! closure — the pure-value seam (OWNERSHIP.md — no stored `&Level`, no trait
//! object for the provider). The resolver borrows the caller's chunk storage,
//! exactly like `LightChunkGetter` stores its resolver by value with no
//! `'static` bound.
//!
//! Deferred with their owning units: `tick(BooleanSupplier, boolean)`,
//! `gatherStats()`, `getLoadedChunksCount()`, `close()` (the server chunk
//! cache / `mc.world.level.chunk.storage`); `getLightEngine()` (`LevelLightEngine`,
//! the lighting engine unit #184); `setSpawnSettings`, `updateChunkForced`,
//! `getForceLoadedChunks` (the force-loaded-chunks / spawn-settings units);
//! and the abstract `getChunk(x, z, ChunkStatus, bool)` itself — the port
//! models it as the resolver closure's contract rather than a trait method,
//! matching how `ChunkMap` in `rivet-server` provides chunks.
//!
//! RivetTodo(#185): the convenience getters resolve `ChunkStatus.FULL` and
//! `getChunkForLighting` resolves `ChunkStatus.EMPTY`; generation scheduling
//! for the remaining persisted statuses stays with the chunk pipeline.
//! RivetTodo(#184): `getChunkForLighting` returns the chunk's `LightChunk`
//! view (Java casts the `ChunkAccess` to `LightChunk`); the port resolves the
//! `ChunkAccess` base directly, and the light-engine consumers that need the
//! `LightChunk` methods constrain the view when the lighting unit lands.

use crate::chunk::chunk_access::ChunkAccess;
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::light_chunk_getter::LightChunkGetter;

/// `net.minecraft.world.level.chunk.ChunkSource` — a chunk provider.
///
/// The chunk resolver is a generic `Fn` stored by value (not a boxed `dyn`),
/// mirroring `LightChunkGetter`: `get_chunk`/`get_chunk_now`/`has_chunk`/
/// `get_chunk_for_lighting` all delegate to it with the `load` flag Java
/// threads through. The closure returns a borrowed chunk, so the provider
/// borrows its chunk storage for `'a`.
pub struct ChunkSource<'a, C, T, B, S>
where
    C: Fn(i32, i32, bool) -> Option<&'a LevelChunk<T, B, S>>,
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// The resolver backing the abstract `getChunk(x, z, status, load)`:
    /// chunk coords + `loadOrGenerate` → the loaded `LevelChunk` or `None`
    /// (Java `null`). The caller's closure encodes the status it serves.
    chunk_resolver: C,
    _chunk: std::marker::PhantomData<&'a LevelChunk<T, B, S>>,
}

impl<'a, C, T, B, S> ChunkSource<'a, C, T, B, S>
where
    C: Fn(i32, i32, bool) -> Option<&'a LevelChunk<T, B, S>>,
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    /// Wraps the caller's chunk-resolution closure.
    pub fn new(chunk_resolver: C) -> Self {
        ChunkSource {
            chunk_resolver,
            _chunk: std::marker::PhantomData,
        }
    }

    /// `ChunkSource.getChunk(int x, int z, boolean loadOrGenerate)` — resolves
    /// `ChunkStatus.FULL`; `None` when no chunk is available (Java `null`).
    pub fn get_chunk(
        &self,
        x: i32,
        z: i32,
        load_or_generate: bool,
    ) -> Option<&'a LevelChunk<T, B, S>> {
        (self.chunk_resolver)(x, z, load_or_generate)
    }

    /// `ChunkSource.getChunkNow(int x, int z)` — `getChunk(x, z, false)`.
    pub fn get_chunk_now(&self, x: i32, z: i32) -> Option<&'a LevelChunk<T, B, S>> {
        (self.chunk_resolver)(x, z, false)
    }

    /// `ChunkSource.hasChunk(int x, int z)` — `getChunk(x, z, FULL, false) !=
    /// null`.
    pub fn has_chunk(&self, x: i32, z: i32) -> bool {
        (self.chunk_resolver)(x, z, false).is_some()
    }

    /// `ChunkSource.onSectionEmptinessChanged(int, int, int, boolean)` — a
    /// no-op (the callback that re-schedules section work).
    pub fn on_section_emptiness_changed(
        &self,
        _section_x: i32,
        _section_y: i32,
        _section_z: i32,
        _empty: bool,
    ) {
    }

    /// `ChunkSource.getLightEngine()` — `LevelLightEngine` is not ported.
    ///
    /// STUB(mc.world.level.lighting.engine): the `LevelLightEngine` return
    /// value and the engine surface are deferred with the lighting engine unit
    /// (#184); a real port returns the engine here.
    pub fn get_light_engine(&self) {
        // No engine: the light engine is not ported (#184).
    }

    /// `LightChunkGetter.getChunkForLighting(int x, int z)` — resolves
    /// `ChunkStatus.EMPTY` and returns the chunk's `LightChunk` view (Java
    /// casts the `ChunkAccess` to `LightChunk`). The port returns the
    /// `ChunkAccess` base.
    pub fn get_chunk_for_lighting(&self, x: i32, z: i32) -> Option<&'a ChunkAccess<T, B, S>> {
        (self.chunk_resolver)(x, z, false).map(|chunk| chunk.get_base())
    }

    /// The `LightChunkGetter` adapter — Java's `ChunkSource implements
    /// LightChunkGetter`, and the light engines are handed a `LightChunkGetter`
    /// (`LevelLightEngine.getChunkSource`). The adapter re-resolves through
    /// this provider, returning the `ChunkAccess` base as the chunk view.
    #[allow(clippy::type_complexity)] // Java's `LightChunkGetter` surface.
    pub fn light_chunk_getter(
        &self,
    ) -> LightChunkGetter<
        impl Fn(i32, i32) -> Option<&'a ChunkAccess<T, B, S>>,
        &'a ChunkAccess<T, B, S>,
    > {
        LightChunkGetter::new(move |x, z| (self.chunk_resolver)(x, z, false).map(|c| c.get_base()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::palette::GlobalIdMap;
    use crate::chunk::paletted_container_factory::PalettedContainerFactory;
    use crate::chunk::strategy::Strategy;
    use crate::chunk::upgrade_data::UpgradeData;
    use crate::level::height_accessor::create as create_accessor;
    use rivet_registry::core::ChunkPos;

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
    fn chunk_at(pos: ChunkPos) -> LevelChunk<u8, u8, &'static str> {
        LevelChunk::new(
            pos,
            UpgradeData::empty(24),
            create_accessor(-64, 384),
            &factory(),
            0,
            None,
            0,
            // u8 tests: 0 is air, anything else blocks motion.
            &|s: &u8| crate::levelgen::heightmap::StateFlags {
                is_air: *s == 0,
                blocks_motion: *s != 0,
                has_fluid: false,
                is_leaves: false,
            },
        )
    }

    #[test]
    fn getters_resolve_through_the_provider_closure() {
        let chunk1 = chunk_at(ChunkPos::ZERO);
        let chunk2 = chunk_at(ChunkPos::new(1, 0));
        let mut chunks = std::collections::HashMap::new();
        chunks.insert(ChunkPos::ZERO, &chunk1);
        chunks.insert(ChunkPos::new(1, 0), &chunk2);

        let source = ChunkSource::new(move |x, z, _load| chunks.get(&ChunkPos::new(x, z)).copied());
        assert_eq!(
            source.get_chunk_now(0, 0).map(|c| c.get_pos()),
            Some(ChunkPos::ZERO)
        );
        assert!(source.has_chunk(1, 0));
        assert!(!source.has_chunk(5, 5));
        assert_eq!(
            source.get_chunk(1, 0, true).map(|c| c.get_pos()),
            Some(ChunkPos::new(1, 0))
        );
        assert!(source.get_chunk(9, 9, false).is_none());
    }

    #[test]
    fn light_chunk_getter_returns_the_base_view() {
        let chunk1 = chunk_at(ChunkPos::ZERO);
        let mut chunks = std::collections::HashMap::new();
        chunks.insert(ChunkPos::ZERO, &chunk1);

        let source = ChunkSource::new(move |x, z, _load| chunks.get(&ChunkPos::new(x, z)).copied());
        // Direct `getChunkForLighting` returns the `ChunkAccess` base.
        let base = source.get_chunk_for_lighting(0, 0).expect("loaded");
        assert_eq!(base.get_pos(), ChunkPos::ZERO);
        assert!(source.get_chunk_for_lighting(3, 3).is_none());

        // The `LightChunkGetter` adapter resolves the same view.
        let getter = source.light_chunk_getter();
        let view = getter.get_chunk_for_lighting(0, 0).expect("loaded");
        assert_eq!(view.get_pos(), ChunkPos::ZERO);
        assert!(getter.get_chunk_for_lighting(3, 3).is_none());
    }
}

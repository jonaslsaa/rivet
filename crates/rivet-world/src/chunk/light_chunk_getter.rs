//! Port of `net.minecraft.world.level.chunk.LightChunkGetter` (MC 26.2).
//!
//! Java: `LightChunkGetter.java` in `working/Paper`. A chunk provider for the
//! light engines: `getChunkForLighting(int x, int z)` (nullable), a default
//! `onLightUpdate(LightLayer, SectionPos)` no-op, and `getLevel()`.
//!
//! The port keeps `get_chunk_for_lighting`. `onLightUpdate(LightLayer,
//! SectionPos)` is a default no-op, but `LightLayer` and `SectionPos` are not
//! ported yet, so the method is omitted rather than stubbed with substitutes;
//! the owning unit re-adds it. `getLevel()` (`BlockGetter`) is omitted with
//! the world/access units.
//!
//! RivetTodo(#184): `onLightUpdate(LightLayer, SectionPos)` and `getLevel()`
//! (`BlockGetter`) are omitted — `LightLayer`/`SectionPos` are deferred with
//! the lighting engine unit (#184) and `BlockGetter` with the world/access
//! unit (#232), so this module ports `getChunkForLighting` alone; the owning
//! units re-add the methods when those types land.
//!
//! The value is a `Fn(&ChunkPos)`-style closure resolving the `Option<C>`
//! chunk back-reference, following the pure-value pattern of the heightmap
//! module (OWNERSHIP.md — no stored `&ChunkAccess`).

/// `net.minecraft.world.level.chunk.LightChunkGetter`.
pub struct LightChunkGetter<C> {
    /// `getChunkForLighting(int x, int z)` — resolves the chunk (or `None`)
    /// for a chunk-coordinate pair. Java returns `null` when no chunk is
    /// loaded at `(x, z)`.
    chunk_for_lighting: Box<dyn Fn(i32, i32) -> Option<C>>,
}

impl<C> LightChunkGetter<C> {
    /// `LightChunkGetter` — wraps the caller's chunk-resolution closure.
    pub fn new(chunk_for_lighting: impl Fn(i32, i32) -> Option<C> + 'static) -> Self {
        LightChunkGetter {
            chunk_for_lighting: Box::new(chunk_for_lighting),
        }
    }

    /// `getChunkForLighting(int x, int z)`.
    pub fn get_chunk_for_lighting(&self, x: i32, z: i32) -> Option<C> {
        (self.chunk_for_lighting)(x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::LightChunkGetter;

    #[test]
    fn resolves_loaded_chunks_and_absent_others() {
        let getter =
            LightChunkGetter::new(|x, z| (x == 1 && z == -2).then_some(format!("{x},{z}")));
        assert_eq!(getter.get_chunk_for_lighting(1, -2), Some("1,-2".into()));
        assert_eq!(getter.get_chunk_for_lighting(0, 0), None);
    }
}

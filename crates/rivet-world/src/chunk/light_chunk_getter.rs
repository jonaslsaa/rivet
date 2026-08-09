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
//! The value is a `Fn(i32, i32) -> Option<T>` closure resolving the chunk
//! back-reference by chunk coordinates, following the pure-value pattern of
//! the heightmap module (OWNERSHIP.md — no stored `&ChunkAccess`).
//!
//! The chunk type `T` is deliberately left unconstrained: Java's
//! `getChunkForLighting` returns `@Nullable LightChunk`, but the light-engine
//! consumer that would constrain `T: LightChunk<B>` does not exist yet (#184),
//! and forcing the constraint here would require a phantom block-state type
//! parameter callers could not infer. When the light engine lands it constrains
//! the chunk type at its call site, matching Java's `@Nullable LightChunk`.
//! Note Java's `LightChunk` is not generic (`LightChunk extends BlockGetter`);
//! the `LightChunk<T>` type parameter is the port's own adaptation — it models
//! `BlockState` as the caller's `T` — and `T` here is the getter's *chunk*
//! type, distinct from that block-state parameter.

/// `net.minecraft.world.level.chunk.LightChunkGetter`.
///
/// The resolver is a generic `Fn` stored by value (not a boxed `dyn`, no
/// `'static` bound), so a caller can capture a borrowed reference — e.g.
/// `LightChunkGetter::new(|x, z| level.get_chunk_for_lighting(x, z))`
/// borrowing a `&Level`. This follows the pure-value pattern of the heightmap
/// module: no stored `&ChunkAccess`, no heap allocation per getter.
pub struct LightChunkGetter<C, T> {
    /// `getChunkForLighting(int x, int z)` — resolves the chunk (or `None`)
    /// for a chunk-coordinate pair. Java returns `null` when no chunk is
    /// loaded at `(x, z)`.
    chunk_for_lighting: C,
    _chunk: std::marker::PhantomData<T>,
}

impl<C, T> LightChunkGetter<C, T>
where
    C: Fn(i32, i32) -> Option<T>,
{
    /// `LightChunkGetter` — wraps the caller's chunk-resolution closure.
    pub fn new(chunk_for_lighting: C) -> Self {
        LightChunkGetter {
            chunk_for_lighting,
            _chunk: std::marker::PhantomData,
        }
    }

    /// `getChunkForLighting(int x, int z)`.
    pub fn get_chunk_for_lighting(&self, x: i32, z: i32) -> Option<T> {
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

//! Port of `net.minecraft.world.level.chunk.LightChunk` (MC 26.2) — the
//! lighting-facing slice of a chunk.
//!
//! Java: `LightChunk.java` in `working/Paper`. Extends `BlockGetter` and adds
//! `findBlockLightSources(BiConsumer<BlockPos, BlockState>)` plus
//! `getSkyLightSources()` (`ChunkSkyLightSources`). `ChunkAccess` implements it.
//!
//! The port keeps the two lighting hooks. The `BlockGetter` superinterface and
//! `ChunkSkyLightSources` are not ported (BlockGetter is deferred with the
//! world/access units; `ChunkSkyLightSources` is owned by the
//! `mc.world.level.lighting` unit), so those methods are omitted rather than
//! stubbed. The block-state type is the caller's `T`, and block light sources
//! are resolved through the closure `is_light_source` — the light engines that
//! consult them are deferred (#184).
//!
//! RivetTodo(#184): the light engines (`LightEngine` and the section storages)
//! that drive `findBlockLightSources`/`getSkyLightSources` are not ported;
//! this module ports the chunk-side surface the engines call.

use rivet_registry::core::BlockPos;

/// `net.minecraft.world.level.chunk.LightChunk`.
pub trait LightChunk<T> {
    /// `findBlockLightSources(BiConsumer<BlockPos, BlockState>)` — visits every
    /// block that emits light (`state.getLightEmission() > 0` in Java). The
    /// light-emitting predicate is resolved per state by the caller.
    fn find_block_light_sources(
        &self,
        is_light_source: &dyn Fn(&T) -> bool,
        consumer: impl FnMut(BlockPos, T),
    );
}

#[cfg(test)]
mod tests {
    use super::LightChunk;
    use rivet_registry::core::{BlockPos, Vec3iLike};

    struct StubChunk {
        states: Vec<(i32, i32, i32, u8)>,
    }

    impl LightChunk<u8> for StubChunk {
        fn find_block_light_sources(
            &self,
            is_light_source: &dyn Fn(&u8) -> bool,
            mut consumer: impl FnMut(BlockPos, u8),
        ) {
            for (x, y, z, state) in &self.states {
                if is_light_source(state) {
                    consumer(BlockPos::new(*x, *y, *z), *state);
                }
            }
        }
    }

    #[test]
    fn only_light_emitting_blocks_are_visited() {
        let chunk = StubChunk {
            states: vec![(1, 2, 3, 15), (4, 5, 6, 0)],
        };
        let mut visited = Vec::new();
        chunk.find_block_light_sources(&|s| *s > 0, |pos, state| {
            visited.push((pos.coords(), state));
        });
        assert_eq!(visited, vec![((1, 2, 3), 15)]);
    }
}

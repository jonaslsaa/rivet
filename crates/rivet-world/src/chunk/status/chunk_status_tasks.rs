//! Port of `net.minecraft.world.level.chunk.status.ChunkStatusTasks` (MC 26.2)
//! — the per-status task bodies, as the pure value-layer functions the
//! executor seam (`world_gen_context.rs`) dispatches.
//!
//! Java: `ChunkStatusTasks.java` in `working/Paper`. The real bodies touch the
//! `ChunkGenerator`/`ServerLevel`/`WorldGenRegion` surfaces that defer with the
//! `mc.world.level.chunk.generator` wave, so the STRUCTURE_STARTS and
//! STRUCTURE_REFERENCES bodies are pass-throughs here, and the BIOMES/NOISE
//! bodies are the seam hooks invoked by the `WorldGenContext` executor. The
//! `ChunkStatusTask` enum preserves the Java method-name identities for
//! greppability.

use crate::chunk::proto_chunk::ProtoChunk;

/// `ChunkStatusTasks.passThrough` — the chunk is returned unchanged.
pub fn pass_through<T, B, S>(_chunk: &mut ProtoChunk<T, B, S>)
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
}

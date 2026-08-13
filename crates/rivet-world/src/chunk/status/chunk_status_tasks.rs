//! Port of `net.minecraft.world.level.chunk.status.ChunkStatusTasks` (MC 26.2)
//! — the per-status task bodies, as the pure value-layer functions the
//! executor seam (`world_gen_context.rs`) dispatches.
//!
//! Java: `ChunkStatusTasks.java` in `working/Paper`. The real bodies touch the
//! `ChunkGenerator`/`ServerLevel`/`WorldGenRegion` surfaces that defer with the
//! `mc.world.level.chunk.generator` wave, so the STRUCTURE_STARTS and
//! STRUCTURE_REFERENCES bodies are pass-throughs here (the pyramid still
//! advances the persisted status; the actual structure work is marked with a
//! sparse RivetTodo). The BIOMES and NOISE bodies are the seam hooks — the
//! `WorldGenContext` closures carry the real work; they are invoked by the
//! executor, not duplicated here.
//!
//! The persisted-status advance through these pass-through stubs is Java's
//! `ChunkStep.apply`/`completeChunkGeneration` behavior (the status is advanced
//! after *any* task body when the chunk was below the target) — it is the
//! value-layer ordering contract, and the executor seam is a test/demo surface,
//! not the production pipeline. When #185 wires the real bodies, the
//! holder-driven `ChunkGenerationTask` path replaces this seam wholesale, so a
//! chunk promoted through the stubs is never fed back through the real
//! `ChunkStep.apply` (whose `isBefore` guard would skip the deferred work).

use crate::chunk::proto_chunk::ProtoChunk;

/// `ChunkStatusTasks.passThrough` — the chunk is returned unchanged.
pub fn pass_through<T, B, S>(_chunk: &mut ProtoChunk<T, B, S>)
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
}

/// `ChunkStatusTasks.generateStructureStarts` — pass-through in the value layer.
///
/// RivetTodo(#185): the real body calls `generator.createStructures(...)` and
/// `level.onStructureStartsAvailable(chunk)`; both defer with the generator
/// wave.
pub fn generate_structure_starts<T, B, S>(_chunk: &mut ProtoChunk<T, B, S>)
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
}

/// `ChunkStatusTasks.loadStructureStarts` (LOADING pyramid) — pass-through in
/// the value layer. RivetTodo(#185): the real body calls
/// `level.onStructureStartsAvailable(chunk)`.
pub fn load_structure_starts<T, B, S>(_chunk: &mut ProtoChunk<T, B, S>)
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
}

/// `ChunkStatusTasks.generateStructureReferences` — pass-through in the value
/// layer. RivetTodo(#185): the real body builds a `WorldGenRegion` and calls
/// `generator.createReferences(...)` (the generator wave).
pub fn generate_structure_references<T, B, S>(_chunk: &mut ProtoChunk<T, B, S>)
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
}

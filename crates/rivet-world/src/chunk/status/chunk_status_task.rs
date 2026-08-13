//! Port of `net.minecraft.world.level.chunk.status.ChunkStatusTask` (MC 26.2) —
//! the `@FunctionalInterface` that names a step's task body.
//!
//! Java: `ChunkStatusTask.java` in `working/Paper`. Java stores the task as a
//! method reference (`ChunkStatusTasks::generateNoise`); the port mirrors the
//! identity with an enum over the `ChunkStatusTasks` method names. The
//! *dispatch* (which task runs a step) lives in the executor seam
//! (`WorldGenContext::run_step` / `generate_through`, `world_gen_context.rs`);
//! the pure-value pyramid only needs the identities here.

/// `net.minecraft.world.level.chunk.status.ChunkStatusTask` — a step's task
/// identity, mirroring the `ChunkStatusTasks` static method names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkStatusTask {
    /// `ChunkStatusTasks::passThrough`.
    PassThrough,
    /// `ChunkStatusTasks::generateStructureStarts` (GENERATION).
    GenerateStructureStarts,
    /// `ChunkStatusTasks::loadStructureStarts` (LOADING).
    LoadStructureStarts,
    /// `ChunkStatusTasks::generateStructureReferences`.
    GenerateStructureReferences,
    /// `ChunkStatusTasks::generateBiomes`.
    GenerateBiomes,
    /// `ChunkStatusTasks::generateNoise`.
    GenerateNoise,
    /// `ChunkStatusTasks::generateSurface`.
    GenerateSurface,
    /// `ChunkStatusTasks::generateCarvers`.
    GenerateCarvers,
    /// `ChunkStatusTasks::generateFeatures`.
    GenerateFeatures,
    /// `ChunkStatusTasks::initializeLight`.
    InitializeLight,
    /// `ChunkStatusTasks::light`.
    Light,
    /// `ChunkStatusTasks::generateSpawn`.
    GenerateSpawn,
    /// `ChunkStatusTasks::full`.
    Full,
}

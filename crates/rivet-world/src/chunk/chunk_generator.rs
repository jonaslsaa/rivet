//! STUB(mc.world.level.chunk.generator) — `net.minecraft.world.level.chunk.ChunkGenerator`.
//!
//! The abstract `ChunkGenerator` (the worldgen seed/settings provider behind
//! every feature placement) is owned by the `world.level.chunk` worldgen unit.
//! This core unit only passes it through as an opaque parameter
//! (`&dyn ChunkGenerator`) to `ConfiguredFeature.place` / `Feature.place` /
//! `PlacedFeature.place`, and `WorldGenerationContext` reads its `getMinY` /
//! `getGenDepth`. Both are `abstract` in Java with no default bodies, so the
//! Rust trait requires them (no fabricated constants); the full generator
//! (biome source, codec dispatch, per-step feature sorter) lands with its
//! owning unit.

/// `net.minecraft.world.level.chunk.ChunkGenerator` — the chunk generator
/// behind a feature placement. Marker-only until the owning unit lands.
pub trait ChunkGenerator: Send + Sync + 'static {
    /// `ChunkGenerator.getMinY()` — abstract in Java (no default).
    fn get_min_y(&self) -> i32;

    /// `ChunkGenerator.getGenDepth()` — abstract in Java (no default).
    fn get_gen_depth(&self) -> i32;
}

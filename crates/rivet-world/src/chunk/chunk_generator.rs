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

    /// `ChunkGenerator.getBiomeGenerationSettings(Holder<Biome>).hasFeature(
    /// PlacedFeature)` — the biome-membership read `BiomeFilter.shouldPlace`
    /// performs (`context.generator().getBiomeGenerationSettings(biome)
    /// .hasFeature(feature)`).
    ///
    /// STUB(mc.world.level.biome.core) — `BiomeGenerationSettings` and its
    /// `featureSet`/`hasFeature` memo are owned by the `#178` biome-core unit,
    /// so the read fails explicitly rather than fabricating a membership
    /// result (the same capability-unavailable seam as `WorldGenLevel::get_biome`).
    fn get_biome_generation_settings_has_feature(
        &self,
        _biome: &rivet_registry::holder::Holder<rivet_registry::biome_id::BiomeId>,
        _feature: &crate::levelgen::placement::PlacedFeature,
    ) -> bool {
        panic!(
            "ChunkGenerator.getBiomeGenerationSettings(...).hasFeature is not implemented (RivetTodo #178)"
        )
    }
}

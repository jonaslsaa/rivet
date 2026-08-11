//! Port of `net.minecraft.world.level.levelgen.feature.FeaturePlaceContext`
//! (class, 26.2).
//!
//! A plain field holder for a feature placement in progress: the enclosing
//! configured feature (the top feature whose `place` was entered), the world,
//! chunk generator, random source, origin position, and the feature's
//! configuration. Java is generic over `FC extends FeatureConfiguration` and
//! holds a `RandomSource` field; the Rust port mirrors that (`FeatureBehavior`
//! receives a `FeaturePlaceContext<Self::Config, R>`). The
//! world/chunk-generator/random references are borrowed through the caller
//! (`&mut dyn WorldGenLevel` / `&dyn ChunkGenerator` / `&mut R`), matching the
//! ownership model: a feature placement context lives only for the duration of
//! a `place` call.

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.FeaturePlaceContext<FC, R>`.
///
/// Java is a class with private final fields and accessor methods
/// (`topFeature()`, `level()`, …); the Rust port exposes the fields directly
/// (same data, no hidden state) while keeping the Java accessor names as field
/// names for greppability.
pub struct FeaturePlaceContext<'a, FC: FeatureConfiguration, R: RandomSource> {
    /// `topFeature` — `Optional<ConfiguredFeature<?, ?>>`, the enclosing
    /// configured feature when placed through `ConfiguredFeature.place`; `None`
    /// at the `Feature.place(FC, …)` entry.
    pub top_feature: Option<ConfiguredFeatureErased>,
    /// `level` — the world generation level.
    pub level: &'a mut dyn WorldGenLevel,
    /// `chunkGenerator` — the generator supplying structure/placement input.
    pub chunk_generator: &'a dyn ChunkGenerator,
    /// `random` — the random source for this placement.
    pub random: &'a mut R,
    /// `origin` — the placement origin block position.
    pub origin: &'a BlockPos,
    /// `config` — the feature configuration.
    pub config: &'a FC,
}

impl<'a, FC: FeatureConfiguration, R: RandomSource> FeaturePlaceContext<'a, FC, R> {
    /// `new FeaturePlaceContext(Optional<ConfiguredFeature<?, ?>>, WorldGenLevel,
    /// ChunkGenerator, RandomSource, BlockPos, FC)`.
    pub fn new(
        top_feature: Option<ConfiguredFeatureErased>,
        level: &'a mut dyn WorldGenLevel,
        chunk_generator: &'a dyn ChunkGenerator,
        random: &'a mut R,
        origin: &'a BlockPos,
        config: &'a FC,
    ) -> Self {
        FeaturePlaceContext {
            top_feature,
            level,
            chunk_generator,
            random,
            origin,
            config,
        }
    }

    /// `topFeature()` — the enclosing configured feature, if any.
    pub fn top_feature(&self) -> Option<ConfiguredFeatureErased> {
        self.top_feature.clone()
    }

    /// `level()` — Java returns the mutable `WorldGenLevel` reference concrete
    /// features use to call `setBlock`/`getBlockState`/`getChunk`, so the Rust
    /// accessor re-borrows the field mutably to match that contract.
    pub fn level(&mut self) -> &mut dyn WorldGenLevel {
        self.level
    }

    /// `chunkGenerator()`.
    pub fn chunk_generator(&self) -> &dyn ChunkGenerator {
        self.chunk_generator
    }

    /// `random()`.
    pub fn random(&mut self) -> &mut R {
        self.random
    }

    /// `origin()`.
    pub fn origin(&self) -> &BlockPos {
        self.origin
    }

    /// `config()`.
    pub fn config(&self) -> &FC {
        self.config
    }
}

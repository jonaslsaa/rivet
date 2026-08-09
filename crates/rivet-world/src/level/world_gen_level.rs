//! STUB(mc.world.level) — `net.minecraft.world.level.WorldGenLevel`.
//!
//! `WorldGenLevel extends ServerLevelAccessor` is the world surface feature
//! placement runs against (`getSeed`, `ensureCanWrite` plus the inherited
//! `getBlockState`/`setBlock`/`getChunk` read-write surface). It is owned by
//! the `world.level` unit; this core unit declares the minimal surface it needs
//! so `FeaturePlaceContext`/`ConfiguredFeature.place`/`PlacedFeature.place`
//! type-check. The block-touching surface (`getBlockState`/`setBlock`/
//! `getChunk`/`markPosForPostProcessing`) defers with the `BlockState` type
//! (RivetTodo #228) and the `ChunkAccess` spine.
//!
//! RivetTodo(#232): `setCurrentlyGenerating(Supplier<String>)` is omitted — the
//! Java default is a no-op body and only `WorldGenRegion` (server.level)
//! overrides it for debug narration, so no current consumer reads it.

use crate::level::height_accessor::LevelHeightAccessor;
use rivet_registry::core::BlockPos;

/// `net.minecraft.world.level.WorldGenLevel` — the world generation level.
///
/// Marker-plus-height surface until the owning `world.level` unit lands; the
/// rest of the Java `ServerLevelAccessor` ancestor chain (`LevelAccessor`/
/// `LevelReader`/`BlockGetter`, plus the `LevelWriter` write surface) is ported
/// by the owning unit.
pub trait WorldGenLevel: LevelHeightAccessor + Send + Sync + 'static {
    /// `WorldGenLevel.getSeed()`.
    fn get_seed(&self) -> i64;

    /// `WorldGenLevel.ensureCanWrite(BlockPos)` — the writability gate every
    /// `Feature.place` entry checks; Java defaults to `true`.
    fn ensure_can_write(&self, _pos: &BlockPos) -> bool {
        true
    }
}

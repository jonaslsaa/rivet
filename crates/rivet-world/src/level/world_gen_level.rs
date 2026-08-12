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
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::holder::Holder;

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

    /// `BlockGetter.getBlockState(BlockPos)` — the minimal block-state read
    /// seam the `blockpredicates` `StateTestingPredicate` base consumes.
    ///
    /// RivetTodo(#399): the real world-access implementation is not ported yet,
    /// so no production `WorldGenLevel` provides it and every caller must fail
    /// explicitly rather than fabricate a state. Concrete worlds and test
    /// doubles implement the real behavior when they land; until then the
    /// capability is unavailable and callers panic (the same explicit seam the
    /// `#181` dispatch stubs use).
    fn get_block_state(&self, pos: &BlockPos) -> BlockState;

    /// `LevelReader.getBiome(BlockPos)` — the biome read seam the
    /// `MatchingBiomesPredicate` consumes (`this.biomes.contains(level.getBiome(
    /// pos))`).
    ///
    /// RivetTodo(#399): like `get_block_state`, the real world-access
    /// implementation is not ported, so the default fails explicitly (panics)
    /// rather than fabricating a biome — the same capability-unavailable seam.
    fn get_biome(&self, _pos: &BlockPos) -> Holder<BiomeId> {
        panic!("WorldGenLevel.getBiome is not implemented (RivetTodo #399)")
    }

    /// `LevelAccessor.isUnobstructed(@Nullable Entity, VoxelShape)` — the
    /// "is the shape unobstructed" seam the `UnobstructedPredicate` consumes
    /// (`worldGenLevel.isUnobstructed(null, Shapes.block().move(pos))`).
    ///
    /// RivetTodo(#399): the collision world-access implementation is not
    /// ported; the default fails explicitly rather than fabricating a result.
    fn is_unobstructed(&self, _pos: &BlockPos) -> bool {
        panic!("WorldGenLevel.isUnobstructed is not implemented (RivetTodo #399)")
    }

    /// `BlockStateBase.isFaceSturdy(BlockGetter, BlockPos, Direction)` — the
    /// face-sturdiness seam the `HasSturdyFacePredicate` consumes
    /// (`getBlockState(pos).isFaceSturdy(level, pos, direction)`).
    ///
    /// RivetTodo(#399): the shape world-access implementation is not ported;
    /// the default fails explicitly rather than fabricating a result.
    fn is_face_sturdy(&self, _pos: &BlockPos, _state: &BlockState, _direction: &Direction) -> bool {
        panic!("BlockStateBase.isFaceSturdy is not implemented (RivetTodo #399)")
    }

    /// `BlockBehaviour.BlockStateBase.canSurvive(BlockGetter, BlockPos)` — the
    /// survival seam the `WouldSurvivePredicate` consumes
    /// (`state.canSurvive(level, origin.offset(offset))`).
    ///
    /// RivetTodo(#399): the world-context survival check is not ported; the
    /// default fails explicitly rather than fabricating a result.
    fn can_survive(&self, _state: &BlockState, _pos: &BlockPos) -> bool {
        panic!("BlockStateBase.canSurvive is not implemented (RivetTodo #399)")
    }
}

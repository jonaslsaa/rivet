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
//!
//! The trait is `Send` but deliberately NOT `Sync`: the worldgen level is
//! exclusively `&mut`-borrowed by the feature placement stack on the sync tick
//! thread (OWNERSHIP.md), and `WorldGenRegion` owns non-`Sync` `ChunkAccess`
//! values (the paletted-container `dyn` internals are `Send`-only). A `Sync`
//! bound would force a shared worldgen view that the ownership model forbids.

use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::heightmap::Types;
use rivet_registry::access::RegistryAccess;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_registry::fluid_id::FluidId;
use rivet_registry::holder::Holder;

/// `net.minecraft.world.level.WorldGenLevel` — the world generation level.
///
/// Marker-plus-height surface until the owning `world.level` unit lands; the
/// rest of the Java `ServerLevelAccessor` ancestor chain (`LevelAccessor`/
/// `LevelReader`/`BlockGetter`, plus the `LevelWriter` write surface) is ported
/// by the owning unit.
pub trait WorldGenLevel: LevelHeightAccessor + Send + 'static {
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

    /// `LevelReader.getHeight(Heightmap.Types, int, int)` — the column-height
    /// read `PlacementContext.getHeight` delegates to (`this.level.getHeight(
    /// type, x, z)`), consumed by the surface-relative placement filters.
    ///
    /// Named `get_height_at` because Rust cannot overload: Java's 0-arg
    /// `LevelHeightAccessor.getHeight()` (the world's vertical extent, already
    /// on this trait's supertrait) and this heightmap read collide on the Java
    /// name — the same `_at` suffix `ChunkAccess::get_height_at` uses for
    /// exactly this collision.
    ///
    /// RivetTodo(#228): the worldgen `LevelReader` heightmap read is not ported
    /// yet, so the default fails explicitly (panics) rather than fabricating a
    /// surface — the same capability-unavailable seam as `get_biome`. Concrete
    /// worlds and test doubles override it with real behavior when they land.
    fn get_height_at(&self, _ty: Types, _x: i32, _z: i32) -> i32 {
        panic!("WorldGenLevel.getHeight is not implemented (RivetTodo #228)")
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

    /// `LevelReader.isStateAtPosition(BlockPos, Predicate<BlockState>)` — the
    /// state-testing seam the tree family consumes (`TreeFeature.isVine`,
    /// `FoliagePlacer.tryPlaceLeaf`, `TrunkPlacer.isFree`, `BeehiveDecorator`).
    ///
    /// The default resolves the offset state through the `get_block_state`
    /// seam and applies the predicate, so the capability-unavailable behavior
    /// is exactly `get_block_state`'s (RivetTodo #399): no production world
    /// provides it yet, so the call fails loudly rather than fabricating a
    /// state. Concrete worlds override when they land.
    fn is_state_at_position(&self, pos: &BlockPos, test: &dyn Fn(&BlockState) -> bool) -> bool {
        test(&self.get_block_state(pos))
    }

    /// `LevelReader.isFluidAtPosition(BlockPos, Predicate<FluidState>)` — the
    /// fluid-state-testing seam the tree family consumes
    /// (`FoliagePlacer.tryPlaceLeaf` waterlogging, `RootPlacer.
    /// getPotentiallyWaterloggedState`).
    ///
    /// The default resolves the position's fluid through the `get_block_state`
    /// seam (`BlockState.fluid_id()`, the state's fluid registry id) and
    /// applies the predicate, so the capability-unavailable behavior is
    /// exactly `get_block_state`'s (RivetTodo #399). Concrete worlds override
    /// when they land.
    fn is_fluid_at_position(&self, pos: &BlockPos, test: &dyn Fn(&FluidId) -> bool) -> bool {
        let state = self.get_block_state(pos);
        test(&FluidId::from_id(state.fluid_id()))
    }

    /// `LevelWriter.setBlock(BlockPos, BlockState, int)` — the block-write
    /// seam the tree family consumes (`TreeFeature.setBlockKnownShape`,
    /// `FallenTreeFeature.placeLogBlock`, every decorator's `setBlock`).
    ///
    /// RivetTodo(#399): the real world-write implementation is not ported, so
    /// the default fails explicitly rather than fabricating a write. Concrete
    /// worlds and test doubles implement the real behavior when they land.
    fn set_block(&mut self, _pos: &BlockPos, _state: &BlockState, _flags: i32) -> bool {
        panic!("WorldGenLevel.setBlock is not implemented (RivetTodo #399)")
    }

    /// `ServerLevelAccessor.markAboveForPostProcessing(BlockPos)` — the
    /// post-processing seam `FallenTreeFeature.placeLogBlock` calls after each
    /// placed log.
    ///
    /// RivetTodo(#399): the real post-processing marking is not ported, so the
    /// default fails explicitly rather than silently skipping. Concrete worlds
    /// override when they land.
    fn mark_above_for_post_processing(&mut self, _pos: &BlockPos) {
        panic!("WorldGenLevel.markAboveForPostProcessing is not implemented (RivetTodo #399)")
    }

    /// `LevelReader.getHeightmapPos(Heightmap.Types, BlockPos)` — the heightmap
    /// position read `PlaceOnGroundDecorator.attemptToPlaceBlockAbove` consumes
    /// (`level.getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY() <=
    /// abovePos.getY()`).
    ///
    /// RivetTodo(#228): the worldgen `LevelReader` heightmap read is not ported,
    /// so the default fails explicitly rather than fabricating a surface — the
    /// same capability-unavailable seam as `get_height_at`. Concrete worlds and
    /// test doubles override it with real behavior when they land.
    fn get_heightmap_pos(&self, _ty: Types, _pos: &BlockPos) -> BlockPos {
        panic!("WorldGenLevel.getHeightmapPos is not implemented (RivetTodo #228)")
    }

    /// `ServerLevelAccessor.registryAccess()` — the registry-access read
    /// `PaleMossDecorator.place` consumes to look up the
    /// `configured_feature` registry.
    ///
    /// RivetTodo(#399): the world's registry access is not wired yet, so the
    /// default fails explicitly rather than fabricating an access. Concrete
    /// worlds override when they land.
    fn registry_access(&self) -> RegistryAccess {
        panic!("WorldGenLevel.registryAccess is not implemented (RivetTodo #399)")
    }

    /// `Level.getBlockEntity(BlockPos, BlockEntityType)` — the block-entity
    /// read `BeehiveDecorator.place` consumes to store the generated bees.
    ///
    /// RivetTodo(#399): the block-entity world access is not ported, so the
    /// default fails explicitly rather than fabricating an entity. Concrete
    /// worlds override when they land.
    fn get_block_entity(&self, _pos: &BlockPos) -> Option<()> {
        panic!("WorldGenLevel.getBlockEntity is not implemented (RivetTodo #399)")
    }
}

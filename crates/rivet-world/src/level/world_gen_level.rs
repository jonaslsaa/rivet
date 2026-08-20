//! STUB(mc.world.level) — `net.minecraft.world.level.WorldGenLevel`.
//!
//! `WorldGenLevel extends ServerLevelAccessor` is the world surface feature
//! placement runs against (`getSeed`, `ensureCanWrite` plus the inherited
//! `getBlockState`/`setBlock`/`getChunk` read-write surface). It is owned by
//! the `world.level` unit; this core unit declares the minimal surface it needs
//! so `FeaturePlaceContext`/`ConfiguredFeature.place`/`PlacedFeature.place`
//! type-check. The block-touching surface (`getBlockState`/`setBlock`/
//! `getChunk`/`markPosForPostProcessing`) defers with the `BlockState` type
//! (RivetTodo #232) and the `ChunkAccess` spine.
//!
//! RivetTodo(#232): `setCurrentlyGenerating(Supplier<String>)` is omitted — the
//! Java default is a no-op body and only `WorldGenRegion` (server.level)
//! overrides it for debug narration, so no current consumer reads it.
//!
//! The trait is `Send` but deliberately NOT `Sync` and NOT `'static`: the
//! worldgen level is exclusively `&mut`-borrowed by the feature placement stack
//! on the sync tick thread (OWNERSHIP.md), and `WorldGenRegion` owns non-`Sync`
//! `ChunkAccess` values (the paletted-container `dyn` internals are
//! `Send`-only). A `Sync` bound would force a shared worldgen view that the
//! ownership model forbids, and a `'static` bound would forbid borrowing a
//! worldgen level that owns data with a shorter lifetime.
//!
//! The value types are free: the executor's dense region runs `StateId` block
//! values while the FEATURES-pass region runs `BlockState` values, so the
//! production impls specialize the `WorldGenRegion` type parameters rather than
//! the trait.

use crate::level::block_shape_context::{
    BlockShapeContext, ShapeQuery, ShapeQueryError, SupportType, dynamic_shape_fallback,
};
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
pub trait WorldGenLevel: LevelHeightAccessor + Send {
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
    /// RivetTodo(#232): the real world-access implementation is not ported yet,
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
    /// RivetTodo(#232): like `get_block_state`, the real world-access
    /// implementation is not ported, so the default fails explicitly (panics)
    /// rather than fabricating a biome — the same capability-unavailable seam.
    fn get_biome(&self, _pos: &BlockPos) -> Holder<BiomeId> {
        panic!("WorldGenLevel.getBiome is not implemented (RivetTodo #232)")
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
    /// The default remains an explicit capability failure for worlds that do
    /// not own chunk heightmaps. Concrete regions override it with the mutable
    /// Java behavior: a missing heightmap is primed and persisted before the
    /// first-available height is returned.
    fn get_height_at(&mut self, _ty: Types, _x: i32, _z: i32) -> i32 {
        panic!("WorldGenLevel.getHeight is not implemented (RivetTodo #232)")
    }

    /// `LevelAccessor.isUnobstructed(@Nullable Entity, VoxelShape)` — the
    /// "is the shape unobstructed" seam the `UnobstructedPredicate` consumes
    /// (`worldGenLevel.isUnobstructed(null, Shapes.block().move(pos))`).
    ///
    /// RivetTodo(#232): the collision world-access implementation is not
    /// ported; the default fails explicitly rather than fabricating a result.
    fn is_unobstructed(&self, _pos: &BlockPos) -> bool {
        panic!("WorldGenLevel.isUnobstructed is not implemented (RivetTodo #232)")
    }

    /// A live WorldGenRegion implementation supplies this context when its
    /// typed block-entity bridge lands under #185/#341. Test doubles use the
    /// detached context in `block_shape_context`; the default intentionally
    /// exposes the capability boundary instead of inventing block entities.
    fn shape_context(&self) -> Option<&dyn BlockShapeContext> {
        None
    }

    /// Resolve the support, collision, and occlusion faces for a state. Static
    /// states use generated Paper tables; dynamic states must go through the
    /// live or detached context and never read those zero-context samples.
    fn shape_query(
        &self,
        pos: &BlockPos,
        state: &BlockState,
    ) -> Result<ShapeQuery, ShapeQueryError> {
        if !state.has_dynamic_shape() {
            return Ok(ShapeQuery::from_static_state(*state));
        }

        let fallback =
            || dynamic_shape_fallback(state).ok_or(ShapeQueryError::DynamicShapeUnsupported);
        let context = self
            .shape_context()
            .ok_or(ShapeQueryError::DynamicShapeContextUnavailable)?;
        match context.shape_query(state, pos) {
            Ok(query) => Ok(query),
            Err(ShapeQueryError::DynamicShapeContextMissing { .. }) => fallback(),
            Err(error) => Err(error),
        }
    }

    /// `BlockStateBase.isFaceSturdy(BlockGetter, BlockPos, Direction)` with an
    /// explicit `SupportType`.
    fn is_face_sturdy_with(
        &self,
        pos: &BlockPos,
        state: &BlockState,
        direction: &Direction,
        support_type: SupportType,
    ) -> Result<bool, ShapeQueryError> {
        Ok(self
            .shape_query(pos, state)?
            .is_supporting(support_type, *direction))
    }

    /// `SupportType.FULL` face sturdiness used by worldgen predicates.
    fn is_face_sturdy(&self, pos: &BlockPos, state: &BlockState, direction: &Direction) -> bool {
        self.is_face_sturdy_with(pos, state, direction, SupportType::Full)
            .unwrap_or_else(|error| panic!("WorldGenLevel.isFaceSturdy: {error}"))
    }

    /// Full collision-face query used by `MultifaceBlock.canAttachTo`.
    fn is_collision_face_full(
        &self,
        pos: &BlockPos,
        state: &BlockState,
        direction: &Direction,
    ) -> bool {
        self.shape_query(pos, state)
            .unwrap_or_else(|error| panic!("WorldGenLevel.isCollisionFaceFull: {error}"))
            .is_collision_face_full(*direction)
    }

    /// Full occlusion-face query used by face-occlusion callers.
    fn is_occlusion_face_full(
        &self,
        pos: &BlockPos,
        state: &BlockState,
        direction: &Direction,
    ) -> bool {
        self.shape_query(pos, state)
            .unwrap_or_else(|error| panic!("WorldGenLevel.getFaceOcclusionShape: {error}"))
            .is_occlusion_face_full(*direction)
    }

    /// `MultifaceBlock.canAttachTo` — support OR full collision on the
    /// opposite neighbour face. Leaves therefore remain attachable even though
    /// their support face is not full.
    fn can_attach_to(&self, pos: &BlockPos, state: &BlockState, direction: &Direction) -> bool {
        let query = self
            .shape_query(pos, state)
            .unwrap_or_else(|error| panic!("WorldGenLevel.canAttachTo: {error}"));
        query.is_supporting(SupportType::Full, *direction)
            || query.is_collision_face_full(*direction)
    }

    /// `BlockBehaviour.BlockStateBase.canSurvive(BlockGetter, BlockPos)` — the
    /// survival seam the `WouldSurvivePredicate` consumes
    /// (`state.canSurvive(level, origin.offset(offset))`).
    ///
    /// RivetTodo(#232): the world-context survival check is not ported; the
    /// default fails explicitly rather than fabricating a result.
    fn can_survive(&self, _state: &BlockState, _pos: &BlockPos) -> bool {
        panic!("BlockStateBase.canSurvive is not implemented (RivetTodo #232)")
    }

    // -----------------------------------------------------------------------
    // The world seams the vegetation/aquatic/selector feature family consumes
    // (`is_empty_block`, `get_sea_level`, `set_block`). Each defaults to an
    // explicit failure (the same capability-unavailable seam as
    // `get_biome`/`get_height_at`, marked `RivetTodo #232`) until the owning
    // `mc.world.level` unit lands; the concrete features and their test
    // doubles override them with real behavior.
    // -----------------------------------------------------------------------

    /// `LevelReader.isEmptyBlock(BlockPos)` — the empty-cell read the
    /// vegetation features gate on (`VinesFeature`, `BambooFeature`,
    /// `NetherForestVegetationFeature`, `ChorusPlantFeature`, …).
    ///
    /// RivetTodo(#232): the world-access implementation is not ported; the
    /// default fails explicitly rather than fabricating a state.
    fn is_empty_block(&self, _pos: &BlockPos) -> bool {
        panic!("LevelReader.isEmptyBlock is not implemented (RivetTodo #232)")
    }

    /// `LevelReader.getSeaLevel()` — the sea-level read `BlueIceFeature`
    /// gates on (`origin.getY() > level.getSeaLevel() - 1`). Java lives on
    /// `LevelReader`, so it sits here (the `ChunkGenerator`-side
    /// `get_sea_level` seam exists separately for `BasaltColumnsFeature`).
    ///
    /// RivetTodo(#232): the world-access implementation is not ported; the
    /// default fails explicitly rather than fabricating a level.
    fn get_sea_level(&self) -> i32 {
        panic!("LevelReader.getSeaLevel is not implemented (RivetTodo #232)")
    }

    /// `LevelWriter.setBlock(BlockPos, BlockState, int)` — the block write
    /// seam `Feature.setBlock`/`safeSetBlock` reduce to
    /// (`level.setBlock(pos, state, flags)` with the `Block.UPDATE_*`
    /// constants). `&mut self` mirrors the `LevelWriter` write contract (the
    /// worldgen level is exclusively `&mut`-borrowed, OWNERSHIP.md).
    ///
    /// RivetTodo(#232): the chunk-write implementation is not ported; the
    /// default fails explicitly rather than fabricating a write.
    fn set_block(&mut self, _pos: &BlockPos, _state: BlockState, _flags: u32) -> bool {
        panic!("LevelWriter.setBlock is not implemented (RivetTodo #232)")
    }

    /// `LevelAccessor.scheduleTick(BlockPos, Fluid, int)` — the scheduled-tick
    /// seam `SpringFeature.place` consumes (`level.scheduleTick(origin,
    /// config.state.getType(), 0)` schedules the placed spring's fluid to flow).
    /// `&mut self` mirrors the tick-write contract like `set_block`.
    ///
    /// RivetTodo(#232): the scheduled-tick machinery is not ported; the default
    /// fails explicitly rather than fabricating a tick.
    fn schedule_tick(&mut self, _pos: &BlockPos, _fluid: FluidId, _delay: i32) {
        panic!("LevelAccessor.scheduleTick is not implemented (RivetTodo #232)")
    }

    /// `LevelAccessor.scheduleTick(BlockPos, Block, int)` — the block
    /// scheduled-tick seam, consumed by `SimpleBlockFeature.place` when
    /// `config.scheduleTick()` is set (`level.scheduleTick(origin,
    /// level.getBlockState(origin).getBlock(), 1)`) and by `LakeFeature.place`
    /// (`level.scheduleTick(placePos, AIR.getBlock(), 0)` schedules the placed
    /// cave-air to tick). `&mut self` mirrors the tick-write contract like
    /// `schedule_tick`.
    ///
    /// RivetTodo(#232): the scheduled-tick machinery is not ported; the default
    /// fails explicitly rather than fabricating a tick.
    fn schedule_block_tick(&mut self, _pos: &BlockPos, _block: crate::block::Block, _delay: i32) {
        panic!("LevelAccessor.scheduleTick(Block) is not implemented (RivetTodo #232)")
    }

    /// `ChunkAccess.markPosForPostProcessing(BlockPos)` — the post-processing
    /// mark seam `Feature.markAboveForPostProcessing` reduces to
    /// (`level.getChunk(pos).markPosForPostProcessing(pos)`). The chunk-access
    /// hop is folded into this one seam (the smallest typed form the
    /// geology/cave leaves need).
    ///
    /// RivetTodo(#232): the chunk-access implementation is not ported; the
    /// default fails explicitly rather than fabricating the mark.
    fn mark_pos_for_post_processing(&mut self, _pos: &BlockPos) {
        panic!("ChunkAccess.markPosForPostProcessing is not implemented (RivetTodo #232)")
    }

    /// `Biome.shouldFreeze(LevelReader, BlockPos, boolean)` — the biome
    /// freeze verdict `SnowAndFreezeFeature` consumes (`level.getBiome(topPos)
    /// .value().shouldFreeze(level, belowPos, false)`).
    ///
    /// `freeze_pos` is the FREEZE position — the cell the verdict is evaluated
    /// on, the one `SnowAndFreezeFeature` turns to ice (`belowPos`, one block
    /// below the `MOTION_BLOCKING` column height). Java samples the biome at
    /// `topPos` (`freeze_pos.above()`), so a faithful implementation MUST
    /// resolve the biome at `freeze_pos.above()`, never at `freeze_pos` — the
    /// `SnowAndFreezeFeature` topPos/belowPos split. This seam is only faithful
    /// for that single caller, and the offset is part of the contract.
    ///
    /// STUB(mc.world.level): the biome
    /// value is not reachable through `WorldGenLevel::get_biome` (it resolves a
    /// `Holder<BiomeId>`, id only) and `shouldFreeze` reads the `LevelReader`
    /// brightness/fluid surface (#232), so the verdict is a dedicated seam; the
    /// default fails explicitly rather than fabricating a freeze. Test doubles
    /// override it with a fixed verdict.
    fn should_freeze(&self, _freeze_pos: &BlockPos, _check_neighbors: bool) -> bool {
        panic!("Biome.shouldFreeze is not implemented (RivetTodo #232)")
    }

    /// `Biome.shouldSnow(LevelReader, BlockPos)` — the biome snow verdict
    /// `SnowAndFreezeFeature` consumes (`level.getBiome(topPos).value()
    /// .shouldSnow(level, topPos)`).
    ///
    /// Here `pos` IS the biome-sample position (`topPos`) — unlike
    /// `should_freeze`, the sample and the evaluated cell are the same, so no
    /// offset.
    ///
    /// STUB(mc.world.level): like
    /// `should_freeze`, the verdict reads the unported `LevelReader` surface
    /// (brightness, snow survival); the default fails explicitly rather than
    /// fabricating a snowfall. Test doubles override it with a fixed verdict.
    fn should_snow(&self, _pos: &BlockPos) -> bool {
        panic!("Biome.shouldSnow is not implemented (RivetTodo #232)")
    }

    /// `LevelAccessor.destroyBlock(BlockPos, boolean, @Nullable Entity)` — the
    /// block-destruction seam `EndPodiumFeature.dropPreviousAndSetBlock` and
    /// `EndPlatformFeature`'s `dropResources` path reduce to
    /// (`level.destroyBlock(pos, true, null)`). The `@Nullable Entity`
    /// argument is dropped: every reachable caller passes `null` (the worldgen
    /// platform/podium call sites), so the signature carries only the `drop`
    /// flag.
    ///
    /// RivetTodo(#232): the block-destruction implementation is not ported; the
    /// default fails explicitly rather than fabricating a destruction.
    fn destroy_block(&mut self, _pos: &BlockPos, _drop: bool) -> bool {
        panic!("LevelAccessor.destroyBlock is not implemented (RivetTodo #232)")
    }

    /// Whether `getBlockEntity(pos)` is a `RandomizableContainer`.
    ///
    /// Java's `RandomizableContainer.setBlockEntityLootTable` draws
    /// `random.nextLong()` only after this `instanceof` succeeds. Keeping the
    /// query before the draw preserves the feature's RNG stream when a chest
    /// write is rejected or the position has no matching block entity.
    ///
    /// RivetTodo(#232): the default remains an explicit failure for worlds that
    /// have not implemented the block-entity surface; `WorldGenRegion` overrides
    /// it for the FEATURES placement path.
    fn is_randomizable_container(&self, _pos: &BlockPos) -> bool {
        panic!(
            "BlockGetter.getBlockEntity(RandomizableContainer) is not implemented (RivetTodo #232)"
        )
    }

    /// `RandomizableContainer.setBlockEntityLootTable(BlockGetter, RandomSource,
    /// BlockPos, ResourceKey<LootTable>)` — the chest-loot seam the
    /// `.feature.monsterroom` leaf consumes. The feature draws the seed after
    /// [`is_randomizable_container`] succeeds and passes it here because the
    /// trait is not generic over `R`; `loot_table` is the NBT `LootTable` value
    /// (e.g. `minecraft:chests/simple_dungeon`).
    ///
    /// RivetTodo(#232): the default remains an explicit failure for worlds that
    /// have not implemented the block-entity surface; `WorldGenRegion` overrides
    /// it for the FEATURES placement path.
    fn set_block_entity_loot_table(&mut self, _pos: &BlockPos, _seed: i64, _loot_table: &str) {
        panic!("RandomizableContainer.setBlockEntityLootTable is not implemented (RivetTodo #232)")
    }

    /// Whether `getBlockEntity(pos)` is a `SpawnerBlockEntity`.
    ///
    /// Java evaluates this `instanceof` before calling `randomEntityId`, so a
    /// missing spawner entity consumes no mob-selection RNG draw. This query
    /// keeps that short-circuit at the feature boundary.
    ///
    /// RivetTodo(#232): the default remains an explicit failure for worlds that
    /// have not implemented the block-entity surface; `WorldGenRegion` overrides
    /// it for the FEATURES placement path.
    fn is_spawner_block_entity(&self, _pos: &BlockPos) -> bool {
        panic!("BlockGetter.getBlockEntity(SpawnerBlockEntity) is not implemented (RivetTodo #232)")
    }

    /// `BaseSpawner.getOrCreateNextSpawnData`'s weighted-list draw. `Some(total)`
    /// means that this spawner has no current `SpawnData` and a non-empty
    /// `SpawnPotentials`; the feature must consume exactly one
    /// `random.nextInt(total)` before calling [`set_spawner_entity`]. `None`
    /// means Java does not draw here (an existing `SpawnData`, or an empty
    /// potential list).
    ///
    /// The returned total is deliberately a primitive seam: `RandomSource` is
    /// not object-safe, so the feature owns the exact RNG call while the level
    /// owns weighted-list state and the state transition.
    fn spawner_potential_weight(&self, _pos: &BlockPos) -> Option<i32> {
        panic!("BaseSpawner.getOrCreateNextSpawnData is not implemented (RivetTodo #232)")
    }

    /// `SpawnerBlockEntity.setEntityId(EntityType, RandomSource)` — materialize
    /// the selected entity id after the optional weighted-list roll. When
    /// `potential_roll` is `Some`, the level selects that weighted entry before
    /// replacing its entity id; in every case Java clears `spawnPotentials`.
    ///
    /// RivetTodo(#232): the default remains an explicit failure for worlds that
    /// have not implemented the block-entity surface; `WorldGenRegion` overrides
    /// it for the FEATURES placement path.
    fn set_spawner_entity(
        &mut self,
        _pos: &BlockPos,
        _entity_id: &str,
        _potential_roll: Option<i32>,
    ) {
        panic!("SpawnerBlockEntity.setEntityId is not implemented (RivetTodo #232)")
    }

    /// The registry-access back-reference seam. Java `Holder.value()` needs no
    /// lookup (the holder stores its value); the Rust port's `Reference` is a
    /// pure `(RegistryId, id)` pair, so resolving one — and threading the
    /// `&dyn HolderLookup` that `PlacedFeature::place` takes — requires the
    /// owning `RegistryAccess`. Selector and composite features
    /// (`RandomSelectorFeature`, `VegetationPatchFeature`, …) resolve their
    /// `Holder<PlacedFeature>` and pass the configured-feature lookup down.
    ///
    /// The access is returned owned (a cheap `Arc` clone of the shared entry
    /// list) so the resolved registries borrow from the local clone, never
    /// from the `&mut` level. This is a port-threading artifact, not a Java
    /// surface: Java features never touch the access here.
    ///
    /// STUB(mc.world.level): no production `WorldGenLevel` provides it yet;
    /// the default fails explicitly (the `RivetTodo(#232)` panic below). Test
    /// doubles build a `RegistryAccess` over the placed/configured-feature
    /// registries and override.
    fn registry_access(&self) -> RegistryAccess {
        panic!("WorldGenLevel.registryAccess is not implemented (RivetTodo #232)")
    }

    /// `LevelReader.isStateAtPosition(BlockPos, Predicate<BlockState>)` — the
    /// state-testing seam the foliage-placer slice consumes
    /// (`FoliagePlacer.tryPlaceLeaf`, `tree_feature::valid_tree_pos`).
    ///
    /// The default resolves the offset state through the `get_block_state`
    /// seam and applies the predicate, so the read is exactly
    /// `get_block_state`'s (`WorldGenRegion` provides it on the gated chunk
    /// read).
    fn is_state_at_position(&self, pos: &BlockPos, test: &dyn Fn(&BlockState) -> bool) -> bool {
        test(&self.get_block_state(pos))
    }

    /// `LevelReader.isFluidAtPosition(BlockPos, Predicate<FluidState>)` — the
    /// fluid-state-testing seam the foliage-placer slice consumes
    /// (`FoliagePlacer.tryPlaceLeaf` waterlogging decision).
    ///
    /// The default resolves the position's fluid through the `get_block_state`
    /// seam (`BlockState.fluid_id()`, the state's fluid registry id) and
    /// applies the predicate — the same `get_block_state` read `WorldGenRegion`
    /// provides (the gated chunk read).
    fn is_fluid_at_position(&self, pos: &BlockPos, test: &dyn Fn(&FluidId) -> bool) -> bool {
        let state = self.get_block_state(pos);
        test(&FluidId::from_id(state.fluid_id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{DetachedShapeContext, ShulkerBoxShape};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    struct ShapeGuardLevel;

    impl LevelHeightAccessor for ShapeGuardLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for ShapeGuardLevel {
        fn get_seed(&self) -> i64 {
            42
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name("minecraft:air").unwrap(),
            )
        }
    }

    struct DetachedShapeLevel {
        context: DetachedShapeContext,
    }

    impl LevelHeightAccessor for DetachedShapeLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for DetachedShapeLevel {
        fn get_seed(&self) -> i64 {
            42
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name("minecraft:air").unwrap(),
            )
        }

        fn shape_context(&self) -> Option<&dyn BlockShapeContext> {
            Some(&self.context)
        }
    }

    #[test]
    fn static_shape_queries_keep_cached_fast_path() {
        let level = ShapeGuardLevel;
        let stone = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
        );
        let pos = BlockPos::new(0, 0, 0);
        assert!(level.is_face_sturdy(&pos, &stone, &Direction::Up));
        assert!(level.can_attach_to(&pos, &stone, &Direction::Up));
    }

    #[test]
    fn dynamic_shape_queries_use_detached_context() {
        let pos = BlockPos::new(0, 0, 0);
        let mut context = DetachedShapeContext::default();
        context.insert_shulker_box(pos, ShulkerBoxShape::open(Direction::Up));
        let level = DetachedShapeLevel { context };
        let state = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:shulker_box").unwrap(),
        );
        assert!(!level.is_face_sturdy(&pos, &state, &Direction::Up));
        assert!(level.is_face_sturdy(&pos, &state, &Direction::Down));
        assert!(level.can_attach_to(&pos, &state, &Direction::Up));
        assert!(!level.is_occlusion_face_full(&pos, &state, &Direction::Up));
    }

    #[test]
    fn dynamic_shape_queries_use_paper_missing_entity_fallbacks() {
        let pos = BlockPos::new(0, 0, 0);
        let level = DetachedShapeLevel {
            context: DetachedShapeContext::default(),
        };
        let shulker = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:shulker_box").unwrap(),
        );
        let shulker_query = level.shape_query(&pos, &shulker).unwrap();
        for direction in Direction::VALUES {
            assert!(shulker_query.is_supporting(crate::level::SupportType::Full, direction));
            assert!(shulker_query.is_collision_face_full(direction));
            assert!(!shulker_query.is_occlusion_face_full(direction));
        }

        let piston = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:moving_piston")
                .unwrap(),
        );
        let piston_query = level.shape_query(&pos, &piston).unwrap();
        for direction in Direction::VALUES {
            assert!(!piston_query.is_supporting(crate::level::SupportType::Full, direction));
            assert!(!piston_query.is_collision_face_full(direction));
            assert!(!piston_query.is_occlusion_face_full(direction));
        }
    }

    #[test]
    fn dynamic_shape_queries_fail_fast_without_context() {
        let level = ShapeGuardLevel;
        let pos = BlockPos::new(0, 0, 0);
        for block in ["minecraft:shulker_box", "minecraft:moving_piston"] {
            let state = BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name(block).unwrap(),
            );
            assert!(state.has_dynamic_shape(), "{block} must be dynamic");
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    level.is_face_sturdy(&pos, &state, &Direction::Up)
                }))
                .is_err()
            );
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    level.can_attach_to(&pos, &state, &Direction::Up)
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn dynamic_shape_context_state_mismatch_does_not_use_missing_entity_fallback() {
        let pos = BlockPos::new(0, 0, 0);
        let mut context = DetachedShapeContext::default();
        context.insert_shulker_box(pos, ShulkerBoxShape::closed(Direction::Up));
        let level = DetachedShapeLevel { context };
        let piston = BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:moving_piston")
                .unwrap(),
        );

        assert_eq!(
            level.shape_query(&pos, &piston),
            Err(ShapeQueryError::DynamicShapeContextStateMismatch)
        );
    }

    #[test]
    fn unsupported_dynamic_shape_families_return_typed_errors() {
        let level = DetachedShapeLevel {
            context: DetachedShapeContext::default(),
        };
        let pos = BlockPos::new(0, 0, 0);
        for block in [
            "minecraft:bamboo",
            "minecraft:scaffolding",
            "minecraft:powder_snow",
            "minecraft:pointed_dripstone",
            "minecraft:sulfur_spike",
        ] {
            let state = BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name(block).unwrap(),
            );
            assert!(state.has_dynamic_shape(), "{block} must be dynamic");
            assert_eq!(
                level.shape_query(&pos, &state),
                Err(ShapeQueryError::DynamicShapeUnsupported),
                "{block}"
            );
        }
    }
}

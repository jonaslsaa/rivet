//! Region-bound native structure placement. World/entity/plugin authority is
//! supplied explicitly by the owning world; no empty/default backend is used.
use super::{
    generated_world::OverworldGenerator,
    structure_generate_events::StructureGenerateAuthority,
    structure_placement::{
        StructureDecorationAccess, StructureDecorationPlan, StructurePlacementError,
        StructurePlacementFailure,
    },
    structure_spawn_events::StructureWorldIdentity,
    structure_transformer::{CraftBlockSnapshot, StructureTransformBackend, StructureTransformer},
    world_gen_region::WorldGenRegion,
};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{
    Holder, Identifier, RegistryAccess,
    biome_id::BiomeId,
    block_state::BlockState,
    core::{BlockPos, ChunkPos},
    fluid_id::FluidId,
};
use rivet_util::{RandomSource, worldgen_random::WorldgenRandom};
use rivet_world::{
    chunk::{
        storage::section_reconstruction::BiomeId as WorldgenBiomeId,
        structure_access::StructureStartEntry,
    },
    entity::chest_minecart::NativeChestMinecart,
    level::{WorldGenLevel, height_accessor::LevelHeightAccessor},
    levelgen::structure::StructureError,
    levelgen::{
        heightmap::Types,
        structure::{BoundingBox, StructureDefinition, StructurePlacementAuthority},
    },
};
use std::sync::Arc;

/// The actual world entity constructor. A null result is allowed only when
/// real CHEST_MINECART enabled-feature checks decline construction. An absent
/// constructor, UUID, global-ID source or live occupancy query is an error.
pub trait StructureEntityCreation: Send + Sync {
    fn require_creation_authority(&self) -> Result<(), StructureError>;
    fn create_chest_minecart(&self) -> Result<Option<NativeChestMinecart>, StructureError>;
}

pub struct StructurePlacementRuntime {
    pub(super) generator: Arc<OverworldGenerator>,
    pub(super) world: StructureWorldIdentity,
    pub(super) events: StructureGenerateAuthority,
    pub(super) entities: Arc<dyn StructureEntityCreation>,
    pub(super) plan: StructureDecorationPlan,
}

impl StructurePlacementRuntime {
    pub fn new(
        generator: Arc<OverworldGenerator>,
        world: StructureWorldIdentity,
        events: StructureGenerateAuthority,
        entities: Arc<dyn StructureEntityCreation>,
    ) -> Result<Self, StructureError> {
        if generator.seed() != world.seed() {
            return Err(StructureError::PlacementUnavailable(
                "structure placement identity belongs to another seed".into(),
            ));
        }
        let plan = StructureDecorationPlan::new(generator.registry_access())
            .map_err(|error| StructureError::PlacementUnavailable(error.to_string()))?;
        Ok(Self {
            generator,
            world,
            events,
            entities,
            plan,
        })
    }
    pub fn belongs_to(&self, generator: &Arc<OverworldGenerator>) -> bool {
        Arc::ptr_eq(&self.generator, generator)
    }
    pub(super) fn place_step<R: RandomSource + 'static>(
        &self,
        region: &mut WorldGenRegion<'_, BlockState, WorldgenBiomeId, Identifier>,
        random: &mut WorldgenRandom<R>,
        decoration_seed: i64,
        step: usize,
    ) -> Result<(), StructurePlacementError> {
        let center = region.get_center();
        let writable = super::structure_placement::writable_area(center, region);
        let mut adapter = RegionStructurePlacement {
            region,
            runtime: self,
            transformer: None,
        };
        self.plan.place_step(
            &mut adapter,
            random,
            decoration_seed,
            step,
            center,
            writable,
        )
    }
}

struct RegionTransformBackend<'a, 'r> {
    region: &'a mut WorldGenRegion<'r, BlockState, WorldgenBiomeId, Identifier>,
    factory: &'a dyn StructureEntityCreation,
}
impl LevelHeightAccessor for RegionTransformBackend<'_, '_> {
    fn get_min_y(&self) -> i32 {
        self.region.get_min_y()
    }
    fn get_height(&self) -> i32 {
        self.region.get_height()
    }
}
impl WorldGenLevel for RegionTransformBackend<'_, '_> {
    fn get_seed(&self) -> i64 {
        self.region.get_seed()
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.region.get_block_state(pos)
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        WorldGenLevel::set_block(self.region, pos, state, flags)
    }
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.region.schedule_tick(pos, fluid, delay);
    }
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        self.region.mark_pos_for_post_processing(pos);
    }
}
impl StructureTransformBackend<NativeChestMinecart> for RegionTransformBackend<'_, '_> {
    fn fresh_snapshot(
        &mut self,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<CraftBlockSnapshot, StructureError> {
        self.region.structure_fresh_snapshot(pos, state)
    }
    fn world_snapshot(&mut self, pos: BlockPos) -> Result<CraftBlockSnapshot, StructureError> {
        self.region.structure_world_snapshot(pos)
    }
    fn load_snapshot(&mut self, pos: BlockPos, data: &CompoundTag) -> Result<(), StructureError> {
        self.region.structure_load_snapshot(pos, data)
    }
    fn add_entity_with_passengers(
        &mut self,
        entity: NativeChestMinecart,
    ) -> Result<bool, StructureError> {
        self.region.add_fresh_chest_minecart(entity)
    }
    fn load_chunk_entities(
        &mut self,
        chunk: ChunkPos,
    ) -> Result<Vec<NativeChestMinecart>, StructureError> {
        let tags = self.region.structure_chunk_entity_tags(chunk)?;
        let mut loaded = Vec::new();
        for tag in tags {
            if let Some(entity) =
                super::structure_entity_loading::load_native_entity(&tag, self.factory)?
            {
                loaded.push(entity);
            }
        }
        Ok(loaded)
    }
    fn clear_chunk_entities(&mut self, chunk: ChunkPos) -> Result<(), StructureError> {
        self.region.clear_structure_chunk_entities(chunk)
    }
}

struct RegionStructurePlacement<'a, 'b> {
    region: &'a mut WorldGenRegion<'b, BlockState, WorldgenBiomeId, Identifier>,
    runtime: &'a StructurePlacementRuntime,
    transformer: Option<StructureTransformer<NativeChestMinecart>>,
}

impl RegionStructurePlacement<'_, '_> {
    fn active_transformer(
        &mut self,
    ) -> Result<&mut StructureTransformer<NativeChestMinecart>, StructureError> {
        self.transformer.as_mut().ok_or_else(|| {
            StructureError::PlacementUnavailable("no active structure transformer scope".into())
        })
    }
    fn place_snapshot(
        &mut self,
        pos: BlockPos,
        snapshot: CraftBlockSnapshot,
        flags: u32,
    ) -> Result<bool, StructureError> {
        let transformer = self.transformer.as_mut().ok_or_else(|| {
            StructureError::PlacementUnavailable("no active structure transformer scope".into())
        })?;
        let mut backend = RegionTransformBackend {
            region: self.region,
            factory: self.runtime.entities.as_ref(),
        };
        transformer.place_snapshot(&mut backend, pos, snapshot, flags)
    }
}

impl StructurePlacementAuthority for RegionStructurePlacement<'_, '_> {
    fn level(&mut self) -> &mut dyn WorldGenLevel {
        self
    }
    fn begin_structure(
        &mut self,
        structure: &StructureDefinition,
        bounds: BoundingBox,
        center: ChunkPos,
    ) -> Result<(), StructureError> {
        if self.transformer.is_some() {
            return Err(StructureError::PlacementUnavailable(
                "nested structure transformer scope".into(),
            ));
        }
        let key = Identifier::try_parse(structure.registry_id())
            .ok_or(StructureError::RegistryUnavailable)?;
        let payload = super::structure_spawn_events::StructureSpawnPayload::new(
            self.runtime.world.clone(),
            self.runtime.generator.registry_access(),
            key,
            bounds,
            center,
        )
        .map_err(|error| StructureError::PlacementUnavailable(error.to_string()))?;
        let (blocks, entities) = self
            .runtime
            .events
            .dispatch(
                super::structure_generate_events::StructureGeneratePayload::new(
                    payload,
                    super::structure_generate_events::StructureGenerateCause::WorldGeneration,
                ),
            )?
            .into_lists();
        self.transformer = Some(StructureTransformer::new(
            center,
            self.region.get_min_y(),
            self.region.get_height(),
            blocks,
            entities,
        ));
        Ok(())
    }
    fn finish_structure(&mut self) -> Result<(), StructureError> {
        let mut transformer = self.transformer.take().ok_or_else(|| {
            StructureError::PlacementUnavailable("no active structure transformer scope".into())
        })?;
        let mut backend = RegionTransformBackend {
            region: self.region,
            factory: self.runtime.entities.as_ref(),
        };
        let result = transformer.finish(&mut backend);
        if result.is_err() {
            transformer.abort();
        }
        result
    }
    fn abort_structure(&mut self) {
        if let Some(mut transformer) = self.transformer.take() {
            transformer.abort();
        }
    }
    fn structure_updates_handled(&self) -> bool {
        self.transformer
            .as_ref()
            .is_some_and(StructureTransformer::can_transform_blocks)
    }
    fn require_mineshaft_placement(&self) -> Result<(), StructureError> {
        // All methods below have a real block/snapshot/admission backend and
        // an explicitly installed entity factory. Preflight consumes no RNG.
        self.runtime.entities.require_creation_authority()
    }
    fn place_loot_chest(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        loot: &str,
        seed: i64,
    ) -> Result<(), StructureError> {
        self.active_transformer()?;
        let snapshot = super::structure_snapshot_data::loot_chest_snapshot(pos, state, loot, seed)?;
        self.place_snapshot(pos, snapshot, 2)?;
        Ok(())
    }
    fn place_cave_spider_spawner(
        &mut self,
        pos: BlockPos,
        flags: u32,
    ) -> Result<(), StructureError> {
        self.active_transformer()?;
        let snapshot = super::structure_snapshot_data::cave_spider_spawner_snapshot(pos)?;
        self.place_snapshot(pos, snapshot, flags)?;
        Ok(())
    }
    fn place_loot_minecart<R: RandomSource>(
        &mut self,
        pos: BlockPos,
        loot: &str,
        random: &mut R,
    ) -> Result<(), StructureError> {
        self.active_transformer()?;
        let Some(mut entity) = self.runtime.entities.create_chest_minecart()? else {
            return Ok(());
        };
        entity.set_initial_pos(rivet_registry::core::Vec3::new(
            f64::from(pos.get_x()) + 0.5,
            f64::from(pos.get_y()) + 0.5,
            f64::from(pos.get_z()) + 0.5,
        ));
        entity.set_loot_table(
            Identifier::try_parse(loot).ok_or_else(|| {
                StructureError::PlacementUnavailable("invalid minecart loot table".into())
            })?,
            random.next_long(),
        );
        let mut backend = RegionTransformBackend {
            region: self.region,
            factory: self.runtime.entities.as_ref(),
        };
        if self
            .transformer
            .as_mut()
            .expect("checked active scope")
            .transform_entity(&mut backend, &mut entity)?
        {
            backend.region.add_fresh_chest_minecart(entity)?;
        }
        Ok(())
    }
}

impl StructureDecorationAccess for RegionStructurePlacement<'_, '_> {
    fn referenced_chunks(
        &self,
        center: ChunkPos,
        key: &Identifier,
    ) -> Result<Vec<ChunkPos>, StructurePlacementError> {
        assert_eq!(center, self.region.get_center());
        self.region.placement_reference_chunks(key)
    }
    fn start_entry(
        &self,
        source: ChunkPos,
        key: &Identifier,
    ) -> Result<Option<&StructureStartEntry>, StructurePlacementError> {
        self.region.placement_start_entry(source, key)
    }
    fn take_start_entry(
        &mut self,
        source: ChunkPos,
        key: &Identifier,
    ) -> Result<Option<StructureStartEntry>, StructurePlacementError> {
        self.region.placement_take_start_entry(source, key)
    }
    fn return_start_entry(
        &mut self,
        source: ChunkPos,
        key: Identifier,
        entry: StructureStartEntry,
    ) {
        self.region
            .placement_return_start_entry(source, &key, entry);
    }
}

impl LevelHeightAccessor for RegionStructurePlacement<'_, '_> {
    fn get_min_y(&self) -> i32 {
        self.region.get_min_y()
    }
    fn get_height(&self) -> i32 {
        self.region.get_height()
    }
}

impl WorldGenLevel for RegionStructurePlacement<'_, '_> {
    fn get_seed(&self) -> i64 {
        self.region.get_seed()
    }
    fn ensure_can_write(&self, pos: &BlockPos) -> bool {
        self.region.ensure_can_write(pos)
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.region.get_block_state(pos)
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        let result = self
            .transformer
            .as_mut()
            .ok_or_else(|| {
                StructureError::PlacementUnavailable(
                    "structure write outside transformer scope".into(),
                )
            })
            .and_then(|transformer| {
                let mut backend = RegionTransformBackend {
                    region: self.region,
                    factory: self.runtime.entities.as_ref(),
                };
                transformer.set_block(&mut backend, *pos, state, flags)
            });
        result.unwrap_or_else(|error| std::panic::panic_any(StructurePlacementFailure(error)))
    }
    fn get_biome(&self, pos: &BlockPos) -> Holder<BiomeId> {
        self.region.get_biome(pos)
    }
    fn get_height_at(&mut self, ty: Types, x: i32, z: i32) -> i32 {
        WorldGenLevel::get_height_at(self.region, ty, x, z)
    }
    fn is_unobstructed(&self, pos: &BlockPos) -> bool {
        self.region.is_unobstructed(pos)
    }
    fn shape_context(
        &self,
    ) -> Option<&dyn rivet_world::level::block_shape_context::BlockShapeContext> {
        self.region.shape_context()
    }
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        self.region.can_survive(state, pos)
    }
    fn get_sea_level(&self) -> i32 {
        self.region.get_sea_level()
    }
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.region.schedule_tick(pos, fluid, delay);
    }
    fn schedule_block_tick(
        &mut self,
        pos: &BlockPos,
        block: rivet_world::block::Block,
        delay: i32,
    ) {
        self.region.schedule_block_tick(pos, block, delay);
    }
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        self.region.mark_pos_for_post_processing(pos);
    }
    fn get_block_brightness(&self, pos: &BlockPos) -> i32 {
        self.region.get_block_brightness(pos)
    }
    fn registry_access(&self) -> RegistryAccess {
        WorldGenLevel::registry_access(self.region)
    }
}

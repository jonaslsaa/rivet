//! CraftStructureTransformer/TransformerLevelAccessor value and lifecycle core.
//! A concrete backend owns actual region storage and entity admission. Missing
//! entity loading or snapshot codecs must return an error, never an empty list.
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos},
    fluid_id::FluidId,
};
use rivet_world::{level::WorldGenLevel, levelgen::structure::StructureError};
use std::sync::Arc;

#[cfg(test)]
#[path = "structure_transformer_test.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub struct CraftBlockSnapshot {
    pub position: BlockPos,
    pub state: BlockState,
    pub block_entity: Option<CompoundTag>,
}

/// The same live entity is passed through every transformer. Native backends
/// implement this on real constructed entities, not an NBT-shaped fake entity.
pub trait StructureEntity: Send {
    fn position(&self) -> [f64; 3];
    fn is_alive(&self) -> bool;
    fn set_generation(&mut self, generation: bool);
}
impl StructureEntity for rivet_world::entity::chest_minecart::NativeChestMinecart {
    fn position(&self) -> [f64; 3] {
        let p = self.position();
        [p.x, p.y, p.z]
    }
    fn is_alive(&self) -> bool {
        self.is_alive()
    }
    fn set_generation(&mut self, generation: bool) {
        self.set_generation(generation);
    }
}

pub trait StructureTransformBackend<E: StructureEntity>: WorldGenLevel {
    /// CraftBlockStates.getBlockState(level,pos,desiredState,null): fresh BE.
    fn fresh_snapshot(
        &mut self,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<CraftBlockSnapshot, StructureError>;
    /// LimitedRegion.getBlockState: snapshot of actual existing world state/BE.
    fn world_snapshot(&mut self, pos: BlockPos) -> Result<CraftBlockSnapshot, StructureError>;
    /// loadWithComponents into the actual block entity AFTER the block write.
    /// No actual block entity at the destination is the source's no-op case.
    fn load_snapshot(&mut self, pos: BlockPos, data: &CompoundTag) -> Result<(), StructureError>;
    fn add_entity_with_passengers(&mut self, entity: E) -> Result<bool, StructureError>;
    /// EntityType.loadEntityRecursive(LOAD,false), including passenger visitation.
    fn load_chunk_entities(&mut self, chunk: ChunkPos) -> Result<Vec<E>, StructureError>;
    fn clear_chunk_entities(&mut self, chunk: ChunkPos) -> Result<(), StructureError>;
}

/// Lazy copies are shared inside one transformer invocation, then discarded.
/// Changes to getOriginal/getWorld cannot leak into the next transformer.
pub struct TransformationState {
    original: CraftBlockSnapshot,
    world: CraftBlockSnapshot,
    original_copy: Option<CraftBlockSnapshot>,
    world_copy: Option<CraftBlockSnapshot>,
}
impl TransformationState {
    pub fn original(&mut self) -> &mut CraftBlockSnapshot {
        self.original_copy
            .get_or_insert_with(|| self.original.clone())
    }
    pub fn world(&mut self) -> &mut CraftBlockSnapshot {
        self.world_copy.get_or_insert_with(|| self.world.clone())
    }
    fn destroy_copies(&mut self) {
        self.original_copy = None;
        self.world_copy = None;
    }
}

pub type BlockTransformer<E> = Arc<
    dyn Fn(
            &mut LimitedRegion<'_, E>,
            BlockPos,
            CraftBlockSnapshot,
            &mut TransformationState,
        ) -> Result<CraftBlockSnapshot, StructureError>
        + Send
        + Sync,
>;
pub type EntityTransformer<E> = Arc<
    dyn Fn(&mut LimitedRegion<'_, E>, BlockPos, &mut E, bool) -> Result<bool, StructureError>
        + Send
        + Sync,
>;

struct LimitedRegionState<E> {
    center: ChunkPos,
    // Bukkit BoundingBox is min-inclusive/max-exclusive, unlike native boxes.
    min: [f64; 3],
    max: [f64; 3],
    loaded: bool,
    entities: Vec<E>,
    outside_entities: Vec<E>,
}
impl<E: StructureEntity> LimitedRegionState<E> {
    fn contains(&self, p: [f64; 3]) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] < self.max[i])
    }
}

pub struct LimitedRegion<'a, E: StructureEntity> {
    backend: &'a mut dyn StructureTransformBackend<E>,
    state: &'a mut LimitedRegionState<E>,
}
impl<E: StructureEntity> LimitedRegion<'_, E> {
    pub fn contains(&self, position: [f64; 3]) -> bool {
        self.state.contains(position)
    }
    pub fn get_block_state(&mut self, pos: BlockPos) -> Result<CraftBlockSnapshot, StructureError> {
        self.check_position([
            f64::from(pos.get_x()),
            f64::from(pos.get_y()),
            f64::from(pos.get_z()),
        ])?;
        self.backend.world_snapshot(pos)
    }
    /// CraftLimitedRegion.setBlockState updates tile data only. It does not
    /// place a block and requires matching BlockData at the requested position.
    pub fn set_block_state(&mut self, snapshot: CraftBlockSnapshot) -> Result<(), StructureError> {
        let pos = snapshot.position;
        self.check_position([
            f64::from(pos.get_x()),
            f64::from(pos.get_y()),
            f64::from(pos.get_z()),
        ])?;
        let data = snapshot.block_entity.as_ref().ok_or_else(|| {
            StructureError::PlacementUnavailable(
                "state is not a valid block entity (TileState)".into(),
            )
        })?;
        if self.backend.get_block_state(&pos) != snapshot.state {
            return Err(StructureError::PlacementUnavailable(
                "BlockData does not match".into(),
            ));
        }
        self.backend.load_snapshot(pos, data)
    }
    pub fn set_block_data(
        &mut self,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<bool, StructureError> {
        self.check_position([
            f64::from(pos.get_x()),
            f64::from(pos.get_y()),
            f64::from(pos.get_z()),
        ])?;
        Ok(self.backend.set_block(&pos, state, 3))
    }
    fn check_position(&self, pos: [f64; 3]) -> Result<(), StructureError> {
        if self.contains(pos) {
            Ok(())
        } else {
            Err(StructureError::PlacementUnavailable(format!(
                "coordinates {pos:?} are not in the limited region"
            )))
        }
    }
    /// LimitedRegion.spawn/addEntityToWorld stages actual constructed entities.
    pub fn stage_entity(&mut self, entity: E) -> Result<(), StructureError> {
        self.check_position(entity.position())?;
        self.state.entities.push(entity);
        Ok(())
    }
    /// Lazy existing-entity loading is explicit; no unloaded==empty fallback.
    pub fn entities(&mut self) -> Result<&mut [E], StructureError> {
        if !self.state.loaded {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let chunk = ChunkPos::new(
                        self.state.center.x().wrapping_add(dx),
                        self.state.center.z().wrapping_add(dz),
                    );
                    for mut entity in self.backend.load_chunk_entities(chunk)? {
                        if self.state.contains(entity.position()) {
                            entity.set_generation(true);
                            self.state.entities.push(entity);
                        } else {
                            self.state.outside_entities.push(entity);
                        }
                    }
                }
            }
            self.state.loaded = true;
        }
        Ok(&mut self.state.entities)
    }
}

pub struct StructureTransformer<E: StructureEntity> {
    blocks: Vec<BlockTransformer<E>>,
    entities: Vec<EntityTransformer<E>>,
    region: Option<LimitedRegionState<E>>,
}
impl<E: StructureEntity> StructureTransformer<E> {
    /// Called only AFTER genuine AsyncStructureGenerateEvent dispatch resolves
    /// its transformer maps. An unavailable event host must refuse before here.
    pub fn new(
        center: ChunkPos,
        min_y: i32,
        height: i32,
        blocks: Vec<BlockTransformer<E>>,
        entities: Vec<EntityTransformer<E>>,
    ) -> Self {
        let x = center.x().wrapping_shl(4);
        let z = center.z().wrapping_shl(4);
        let first = [
            f64::from(x.wrapping_sub(16)),
            f64::from(min_y),
            f64::from(z.wrapping_sub(16)),
        ];
        let second = [
            f64::from(x.wrapping_add(32)),
            f64::from(min_y.wrapping_add(height)),
            f64::from(z.wrapping_add(32)),
        ];
        Self {
            blocks,
            entities,
            region: Some(LimitedRegionState {
                center,
                // Bukkit BoundingBox.resize normalizes even wrapped integer
                // endpoints, unlike the native structure BoundingBox type.
                min: std::array::from_fn(|i| first[i].min(second[i])),
                max: std::array::from_fn(|i| first[i].max(second[i])),
                loaded: false,
                entities: Vec::new(),
                outside_entities: Vec::new(),
            }),
        }
    }
    pub fn can_transform_blocks(&self) -> bool {
        !self.blocks.is_empty() && self.region.is_some()
    }
    pub fn set_block(
        &mut self,
        backend: &mut dyn StructureTransformBackend<E>,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Result<bool, StructureError> {
        if !self.can_transform_blocks() {
            return Ok(backend.set_block(&pos, state, flags));
        }
        let snapshot = backend.fresh_snapshot(pos, state)?;
        self.place_snapshot(backend, pos, snapshot, flags)
    }
    pub fn place_snapshot(
        &mut self,
        backend: &mut dyn StructureTransformBackend<E>,
        pos: BlockPos,
        mut snapshot: CraftBlockSnapshot,
        flags: u32,
    ) -> Result<bool, StructureError> {
        let transform = self.can_transform_blocks();
        if transform {
            let mut region = LimitedRegion {
                backend,
                state: self.region.as_mut().expect("checked linked region"),
            };
            let world = region.get_block_state(snapshot.position)?;
            let mut changes = TransformationState {
                original: snapshot.clone(),
                world,
                original_copy: None,
                world_copy: None,
            };
            for transformer in &self.blocks {
                snapshot = transformer(
                    &mut region,
                    changes.original.position,
                    snapshot,
                    &mut changes,
                )?;
                changes.destroy_copies();
            }
        }
        let result = backend.set_block(&pos, snapshot.state, flags);
        // TransformerLevelAccessor owns these effects only on the transforming
        // path. The direct placeCraftBlockEntity path does not schedule them.
        if transform {
            let fluid = FluidId::from_id(backend.get_block_state(&pos).fluid_id());
            if fluid != FluidId::EMPTY {
                backend.schedule_tick(&pos, fluid, 0);
            }
            if shape_check(snapshot.state) {
                backend.mark_pos_for_post_processing(&pos);
            }
        }
        if let Some(data) = &snapshot.block_entity {
            backend.load_snapshot(pos, data)?;
        }
        Ok(result)
    }
    /// Accessor.addFreshEntity transforms then immediately delegates. It does
    /// not use the LimitedRegion's staged entity list.
    pub fn transform_entity(
        &mut self,
        backend: &mut dyn StructureTransformBackend<E>,
        entity: &mut E,
    ) -> Result<bool, StructureError> {
        if self.entities.is_empty() || self.region.is_none() {
            return Ok(true);
        }
        entity.set_generation(true);
        let p = entity.position();
        let pos = BlockPos::new(
            rivet_util::mth::floor_d(p[0]),
            rivet_util::mth::floor_d(p[1]),
            rivet_util::mth::floor_d(p[2]),
        );
        let mut allowed = true;
        let mut region = LimitedRegion {
            backend,
            state: self.region.as_mut().expect("checked linked region"),
        };
        for transformer in &self.entities {
            allowed = transformer(&mut region, pos, entity, allowed)?;
        }
        Ok(allowed)
    }
    pub fn finish(
        &mut self,
        backend: &mut dyn StructureTransformBackend<E>,
    ) -> Result<(), StructureError> {
        let Some(state) = self.region.as_mut() else {
            return Ok(());
        };
        if state.loaded {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    backend.clear_chunk_entities(ChunkPos::new(
                        state.center.x().wrapping_add(dx),
                        state.center.z().wrapping_add(dz),
                    ))?;
                }
            }
        }
        // Keep uncommitted entries on an error; abort discards them without
        // admitting more entities. Already admitted entities are not undone.
        while !state.entities.is_empty() {
            if state.entities[0].is_alive() {
                if !state.contains(state.entities[0].position()) {
                    return Err(StructureError::PlacementUnavailable(
                        "entity is not in the limited region".into(),
                    ));
                }
                backend.add_entity_with_passengers(state.entities.remove(0))?;
            } else {
                state.entities.remove(0);
            }
        }
        while !state.outside_entities.is_empty() {
            backend.add_entity_with_passengers(state.outside_entities.remove(0))?;
        }
        self.abort();
        Ok(())
    }
    pub fn abort(&mut self) {
        self.region = None;
        self.blocks.clear();
        self.entities.clear();
    }
}
fn shape_check(state: BlockState) -> bool {
    matches!(
        state.block().name(),
        "minecraft:nether_brick_fence"
            | "minecraft:torch"
            | "minecraft:wall_torch"
            | "minecraft:oak_fence"
            | "minecraft:spruce_fence"
            | "minecraft:dark_oak_fence"
            | "minecraft:pale_oak_fence"
            | "minecraft:acacia_fence"
            | "minecraft:birch_fence"
            | "minecraft:jungle_fence"
            | "minecraft:ladder"
            | "minecraft:iron_bars"
    )
}

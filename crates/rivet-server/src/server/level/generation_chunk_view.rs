//! Borrowed worldgen access over either a real proto or a canonical runtime chunk.
//! Runtime implementations mirror ImposterProtoChunk(allowWrites=false), not a snapshot.
use crate::server::level::level_chunk::{BiomeId as ServerBiomeId, LevelChunk, StructureKey};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::{block_state::BlockState, core::BlockPos, fluid_id::FluidId};
use rivet_world::{
    block::Block,
    block_entity::NativeBlockEntity,
    chunk::{
        chunk_access::ChunkAccess, storage::section_reconstruction::BiomeId,
        structure_access::StructureStartEntry,
    },
    levelgen::heightmap::Types,
    ticks::ScheduledTick,
};

pub trait GenerationChunkView<T, B, S>: Send
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn proto_base_mut(&mut self) -> Option<&mut ChunkAccess<T, B, S>>;
    fn load_pending_native(&mut self, _pos: &BlockPos) -> Option<bool> {
        None
    }
    fn block_in_bounds(&self, pos: &BlockPos) -> T;
    fn biome_ids(&self) -> Vec<B>;
    fn references(&self, key: &S) -> Vec<u64>;
    fn is_outside_build_height(&self, y: i32) -> bool;
    fn get_section_index(&self, y: i32) -> i32;
    fn get_noise_biome(&self, x: i32, y: i32, z: i32) -> B;
    fn is_light_correct(&self) -> bool;
    fn get_block_light_at(&self, x: i32, y: i32, z: i32) -> i32;
    fn get_sky_light_at(&self, x: i32, y: i32, z: i32) -> Option<i32>;
    fn get_block_entity_nbt(&self, pos: &BlockPos) -> Option<&CompoundTag>;
    fn get_block_entity(&self, pos: &BlockPos) -> Option<&NativeBlockEntity>;
    fn has_unkeyed_starts(&self) -> bool;
    fn get_start_for_structure(&self, key: &S) -> Option<&StructureStartEntry>;
    fn get_height_at(&mut self, kind: Types, x: i32, z: i32) -> i32;
    fn get_block_entity_mut(&mut self, pos: &BlockPos) -> Option<&mut NativeBlockEntity>;
    fn set_block_entity(&mut self, entity: NativeBlockEntity);
    fn remove_block_entity(&mut self, pos: &BlockPos);
    fn set_block_entity_nbt(&mut self, tag: CompoundTag);
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos);
    fn schedule_block_tick(&mut self, tick: ScheduledTick<Block>);
    fn schedule_fluid_tick(&mut self, tick: ScheduledTick<FluidId>);
    fn get_start_for_structure_mut(&mut self, key: &S) -> Option<&mut StructureStartEntry>;
}
impl<T, B, S> GenerationChunkView<T, B, S> for ChunkAccess<T, B, S>
where
    T: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + Sync + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash + Send,
{
    fn proto_base_mut(&mut self) -> Option<&mut Self> {
        Some(self)
    }
    fn block_in_bounds(&self, pos: &BlockPos) -> T {
        self.get_section(self.get_section_index(pos.get_y()) as usize)
            .get_block_state(pos.get_x() & 15, pos.get_y() & 15, pos.get_z() & 15)
    }
    fn biome_ids(&self) -> Vec<B> {
        let mut values = Vec::new();
        for section in self.get_sections() {
            section.biomes().get_all(|biome| values.push(biome));
        }
        values
    }
    fn references(&self, key: &S) -> Vec<u64> {
        self.get_references_for_structure(key).copied().collect()
    }
    fn is_outside_build_height(&self, y: i32) -> bool {
        ChunkAccess::is_outside_build_height(self, y)
    }
    fn get_section_index(&self, y: i32) -> i32 {
        ChunkAccess::get_section_index(self, y)
    }
    fn get_noise_biome(&self, x: i32, y: i32, z: i32) -> B {
        ChunkAccess::get_noise_biome(self, x, y, z)
    }
    fn is_light_correct(&self) -> bool {
        ChunkAccess::is_light_correct(self)
    }
    fn get_block_light_at(&self, x: i32, y: i32, z: i32) -> i32 {
        ChunkAccess::get_block_light_at(self, x, y, z)
    }
    fn get_sky_light_at(&self, x: i32, y: i32, z: i32) -> Option<i32> {
        ChunkAccess::get_sky_light_at(self, x, y, z)
    }
    fn get_block_entity_nbt(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        ChunkAccess::get_block_entity_nbt(self, pos)
    }
    fn get_block_entity(&self, pos: &BlockPos) -> Option<&NativeBlockEntity> {
        ChunkAccess::get_block_entity(self, pos)
    }
    fn has_unkeyed_starts(&self) -> bool {
        ChunkAccess::has_unkeyed_starts(self)
    }
    fn get_start_for_structure(&self, key: &S) -> Option<&StructureStartEntry> {
        ChunkAccess::get_start_for_structure(self, key)
    }
    fn get_height_at(&mut self, kind: Types, x: i32, z: i32) -> i32 {
        ChunkAccess::get_height_at(self, kind, x, z)
    }
    fn get_block_entity_mut(&mut self, pos: &BlockPos) -> Option<&mut NativeBlockEntity> {
        ChunkAccess::get_block_entity_mut(self, pos)
    }
    fn set_block_entity(&mut self, entity: NativeBlockEntity) {
        ChunkAccess::set_block_entity(self, entity)
    }
    fn remove_block_entity(&mut self, pos: &BlockPos) {
        ChunkAccess::remove_block_entity(self, pos)
    }
    fn set_block_entity_nbt(&mut self, tag: CompoundTag) {
        ChunkAccess::set_block_entity_nbt(self, tag)
    }
    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        ChunkAccess::mark_pos_for_post_processing(self, pos)
    }
    fn schedule_block_tick(&mut self, tick: ScheduledTick<Block>) {
        ChunkAccess::schedule_block_tick(self, tick)
    }
    fn schedule_fluid_tick(&mut self, tick: ScheduledTick<FluidId>) {
        ChunkAccess::schedule_fluid_tick(self, tick)
    }
    fn get_start_for_structure_mut(&mut self, key: &S) -> Option<&mut StructureStartEntry> {
        ChunkAccess::get_start_for_structure_mut(self, key)
    }
}
impl GenerationChunkView<BlockState, BiomeId, StructureKey> for LevelChunk {
    fn load_pending_native(&mut self, pos: &BlockPos) -> Option<bool> {
        if LevelChunk::get_block_entity(self, pos).is_some() {
            return Some(true);
        }
        if self.generation_base().get_block_entity_nbt(pos).is_some() {
            std::panic::panic_any(rivet_world::block_entity::NativeBlockEntityError::Unsupported("runtime lazy block-entity load requires actual listener/ticker registration authority".into()));
        }
        Some(false)
    }
    fn proto_base_mut(&mut self) -> Option<&mut ChunkAccess<BlockState, BiomeId, StructureKey>> {
        None
    }
    fn block_in_bounds(&self, pos: &BlockPos) -> BlockState {
        BlockState::new(
            self.generation_base()
                .get_section(self.generation_base().get_section_index(pos.get_y()) as usize)
                .get_block_state(pos.get_x() & 15, pos.get_y() & 15, pos.get_z() & 15),
        )
    }
    fn biome_ids(&self) -> Vec<BiomeId> {
        let mut values = Vec::new();
        for section in self.get_sections() {
            section
                .biomes()
                .get_all(|biome: ServerBiomeId| values.push(BiomeId(biome.raw())));
        }
        values
    }
    fn references(&self, key: &StructureKey) -> Vec<u64> {
        self.generation_base()
            .get_references_for_structure(key)
            .copied()
            .collect()
    }
    fn is_outside_build_height(&self, y: i32) -> bool {
        self.generation_base().is_outside_build_height(y)
    }
    fn get_section_index(&self, y: i32) -> i32 {
        self.generation_base().get_section_index(y)
    }
    fn get_noise_biome(&self, x: i32, y: i32, z: i32) -> BiomeId {
        BiomeId(self.generation_base().get_noise_biome(x, y, z).raw())
    }
    fn is_light_correct(&self) -> bool {
        self.generation_base().is_light_correct()
    }
    fn get_block_light_at(&self, x: i32, y: i32, z: i32) -> i32 {
        self.generation_base().get_block_light_at(x, y, z)
    }
    fn get_sky_light_at(&self, x: i32, y: i32, z: i32) -> Option<i32> {
        self.generation_base().get_sky_light_at(x, y, z)
    }
    fn get_block_entity_nbt(&self, pos: &BlockPos) -> Option<&CompoundTag> {
        self.generation_base().get_block_entity_nbt(pos)
    }
    fn get_block_entity(&self, pos: &BlockPos) -> Option<&NativeBlockEntity> {
        LevelChunk::get_block_entity(self, pos)
    }
    fn has_unkeyed_starts(&self) -> bool {
        self.generation_base().has_unkeyed_starts()
    }
    fn get_start_for_structure(&self, key: &StructureKey) -> Option<&StructureStartEntry> {
        self.generation_base().get_start_for_structure(key)
    }
    fn get_height_at(&mut self, kind: Types, x: i32, z: i32) -> i32 {
        let kind = match kind {
            Types::WorldSurfaceWg => Types::WorldSurface,
            Types::OceanFloorWg => Types::OceanFloor,
            other => other,
        };
        self.generation_base_mut().get_height_at(kind, x, z)
    }
    fn get_block_entity_mut(&mut self, pos: &BlockPos) -> Option<&mut NativeBlockEntity> {
        self.generation_base_mut().get_block_entity_mut(pos)
    }
    fn set_block_entity(&mut self, _entity: NativeBlockEntity) {}
    fn remove_block_entity(&mut self, _pos: &BlockPos) {}
    fn set_block_entity_nbt(&mut self, _tag: CompoundTag) {}
    fn mark_pos_for_post_processing(&mut self, _pos: &BlockPos) {}
    fn schedule_block_tick(&mut self, _tick: ScheduledTick<Block>) {}
    fn schedule_fluid_tick(&mut self, _tick: ScheduledTick<FluidId>) {}
    fn get_start_for_structure_mut(
        &mut self,
        key: &StructureKey,
    ) -> Option<&mut StructureStartEntry> {
        self.generation_base_mut().get_start_for_structure_mut(key)
    }
}

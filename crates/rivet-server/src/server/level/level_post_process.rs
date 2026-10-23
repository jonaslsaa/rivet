//! LevelChunk.postProcessGeneration over canonical runtime chunks. The ordered
//! traversal, real neighbour-shape handlers, scheduler, lazy BE owner and done
//! flag are executable. Fluid/liquid ticks and Level.setBlock's full effects
//! require actual runtime bodies; writing terrain alone is not equivalent.
use super::{
    BlockEntityWorldServices, ServerLevel, block_entity_backend::NativeBlockEntityCreation,
};
use crate::server::level::{
    block_shape_update::update_from_neighbour_shapes,
    level_block_entity_runtime::BlockEntityTickerError, world_gen_region::region_can_survive,
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos},
    fluid_id::FluidId,
};
use rivet_world::{
    block::Block,
    level::{WorldGenLevel, height_accessor::LevelHeightAccessor},
    ticks::ScheduledTick,
};
#[cfg(test)]
#[path = "level_post_process_test.rs"]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum GenerationPostProcessError {
    #[error("post-processing requires canonical chunk {0}")]
    MissingChunk(ChunkPos),
    #[error("post-processing requires retained world runtime settings")]
    MissingSettings,
    #[error("post-processing authority unavailable: {0}")]
    Unavailable(String),
    #[error("nonempty UpgradeData block fixers are unavailable for {0}")]
    UpgradeFixers(ChunkPos),
    #[error(transparent)]
    BlockEntity(#[from] BlockEntityTickerError),
}

/// These effects have no native implementations in the current fluid/block
/// runtime. Implementors must execute them against this same canonical level;
/// no optional/default successful handlers or detached world copies exist.
pub trait GenerationPostProcessServices: BlockEntityWorldServices + Send {
    fn fluid_tick(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), GenerationPostProcessError>;
    fn liquid_block_tick(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), GenerationPostProcessError>;
    fn set_block(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
    ) -> Result<bool, GenerationPostProcessError>;
}

struct LivePostProcessLevel<'a, S> {
    level: &'a mut ServerLevel,
    services: &'a mut S,
    ticking_enabled: bool,
}
fn unwrap_world<T>(result: Result<T, GenerationPostProcessError>) -> T {
    result.unwrap_or_else(|error| std::panic::panic_any(error))
}
impl<S> LevelHeightAccessor for LivePostProcessLevel<'_, S> {
    fn get_height(&self) -> i32 {
        self.level.get_height()
    }
    fn get_min_y(&self) -> i32 {
        self.level.get_min_y()
    }
}
impl<S: GenerationPostProcessServices> WorldGenLevel for LivePostProcessLevel<'_, S> {
    fn get_seed(&self) -> i64 {
        self.level.seed()
    }
    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        let chunk_pos = ChunkPos::containing(pos);
        // Level.isInValidBounds uses ChunkPos.isValid (the dependency-safe
        // chunk-pyramid limit), NOT the distinct ±30M spawn/world bounds.
        if self.is_outside_build_height_pos(pos) || !chunk_pos.is_valid() {
            return Block::from_name("minecraft:void_air")
                .unwrap()
                .default_block_state();
        }
        let chunk = unwrap_world(
            self.level
                .chunk_map
                .get_chunk(chunk_pos)
                .ok_or(GenerationPostProcessError::MissingChunk(chunk_pos)),
        );
        BlockState::new(chunk.get_block_state(pos.get_x(), pos.get_y(), pos.get_z()))
    }
    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state(pos).is_air()
    }
    fn can_survive(&self, state: &BlockState, pos: &BlockPos) -> bool {
        region_can_survive(self, state, pos)
    }
    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        unwrap_world(self.services.set_block(self.level, *pos, state, flags))
    }
    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        let tick = ScheduledTick::new_normal(
            fluid,
            *pos,
            self.level.game_time().wrapping_add(i64::from(delay)),
            self.level.next_sub_tick_count(),
        );
        self.level.schedule_fluid_tick(tick);
    }
    fn schedule_block_tick(&mut self, pos: &BlockPos, block: Block, delay: i32) {
        let tick = ScheduledTick::new_normal(
            block,
            *pos,
            self.level.game_time().wrapping_add(i64::from(delay)),
            self.level.next_sub_tick_count(),
        );
        self.level.schedule_block_tick(tick);
    }
    fn has_beehive_block_entity(&mut self, pos: &BlockPos) -> bool {
        unwrap_world(
            self.level
                .load_native_block_entity(
                    *pos,
                    NativeBlockEntityCreation::Immediate,
                    self.ticking_enabled,
                    self.services,
                )
                .map(|entity| {
                    entity.is_some_and(|entity| {
                        matches!(
                            entity.data(),
                            rivet_world::block_entity::NativeBlockEntityData::Beehive(_)
                        )
                    })
                })
                .map_err(GenerationPostProcessError::from),
        )
    }
}

impl ServerLevel {
    /// Clear a section only AFTER every source entry succeeds. A failure keeps
    /// that section's whole list (including earlier executed effects), exactly
    /// as Java's loop interrupted before ShortList.clear. No rollback/retry is
    /// implied. This method itself does not skip when already done; the source
    /// onChunkTicking caller owns that guard.
    pub fn post_process_generation<S: GenerationPostProcessServices>(
        &mut self,
        pos: ChunkPos,
        services: &mut S,
    ) -> Result<(), GenerationPostProcessError> {
        let ticking_enabled = self
            .runtime_settings()
            .ok_or(GenerationPostProcessError::MissingSettings)?
            .block_entity_ticking;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.post_process_generation_inner(pos, ticking_enabled, services)
        }));
        match result {
            Ok(result) => result,
            Err(panic) => match panic.downcast::<GenerationPostProcessError>() {
                Ok(error) => Err(*error),
                Err(panic) => std::panic::resume_unwind(panic),
            },
        }
    }
    fn post_process_generation_inner<S: GenerationPostProcessServices>(
        &mut self,
        pos: ChunkPos,
        ticking_enabled: bool,
        services: &mut S,
    ) -> Result<(), GenerationPostProcessError> {
        let sections = self
            .chunk_map
            .get_chunk(pos)
            .ok_or(GenerationPostProcessError::MissingChunk(pos))?
            .post_processing()
            .len();
        for section in 0..sections {
            let mut index = 0;
            loop {
                let chunk = self
                    .chunk_map
                    .get_chunk(pos)
                    .ok_or(GenerationPostProcessError::MissingChunk(pos))?;
                let Some(&packed) = chunk.post_processing()[section].get(index) else {
                    break;
                };
                let section_y = (self.get_min_y() >> 4).wrapping_add(section as i32);
                let packed = i32::from(packed);
                let block_pos = BlockPos::new(
                    pos.x().wrapping_shl(4).wrapping_add(packed & 15),
                    section_y.wrapping_shl(4).wrapping_add((packed >> 4) & 15),
                    pos.z().wrapping_shl(4).wrapping_add((packed >> 8) & 15),
                );
                let state = BlockState::new(chunk.get_block_state(
                    block_pos.get_x(),
                    block_pos.get_y(),
                    block_pos.get_z(),
                ));
                if !state.fluid_empty() {
                    services.fluid_tick(self, block_pos, state)?;
                }
                // Use the ORIGINAL state, even when fluid.tick changed terrain.
                if matches!(state.block().name(), "minecraft:water" | "minecraft:lava") {
                    services.liquid_block_tick(self, block_pos, state)?;
                } else {
                    let mut view = LivePostProcessLevel {
                        level: self,
                        services,
                        ticking_enabled,
                    };
                    let updated = update_from_neighbour_shapes(&mut view, state, &block_pos);
                    if updated != state {
                        view.services
                            .set_block(view.level, block_pos, updated, 276)?;
                    }
                }
                index += 1;
            }
            self.chunk_map
                .get_chunk_mut(pos)
                .unwrap()
                .generation_base_mut()
                .get_or_create_offset_list(section)
                .clear();
        }
        // ImmutableList.copyOf(Java HashMap.keySet), not insertion-order NBT.
        let pending = self
            .chunk_map
            .get_chunk(pos)
            .unwrap()
            .generation_base()
            .pending_block_entity_positions_in_source_order();
        for block_pos in pending {
            self.load_native_block_entity(
                block_pos,
                NativeBlockEntityCreation::Immediate,
                ticking_enabled,
                services,
            )?;
        }
        self.chunk_map
            .get_chunk_mut(pos)
            .unwrap()
            .generation_base_mut()
            .clear_pending_block_entities();
        if !self
            .chunk_map
            .get_chunk(pos)
            .unwrap()
            .generation_base()
            .get_upgrade_data()
            .is_empty()
        {
            return Err(GenerationPostProcessError::UpgradeFixers(pos));
        }
        // Fresh canonical generated chunks carry actual UpgradeData.EMPTY:
        // no indices/sides, no neighbour ticks (persisted unsupported neighbour
        // lists refuse earlier reconstruction), no populated fixer queues.
        self.chunk_map
            .get_chunk_mut(pos)
            .unwrap()
            .mark_post_processing_done();
        Ok(())
    }
}

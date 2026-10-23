//! The non-capturing server Level.setBlock / LevelChunk.setBlockState path.
//! Terrain, section counts, heightmaps, light queues, native BE lifecycle and
//! POI membership use their canonical owners. Reentrant block callbacks and
//! external notifications must execute real bodies through the supplied world
//! services. An error keeps the source's already-applied effects.
use super::{
    BlockEntityWorldServices, ServerLevel, block_entity_backend::NativeBlockEntityCreation,
};
use crate::server::level::{
    full_chunk_status::FullChunkStatus, level_block_entity_runtime::BlockEntityTickerError,
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, ChunkPos, SectionPos},
};
use rivet_world::{
    block_entity::NativeBlockEntityData,
    levelgen::heightmap::{StateFlags, Types},
    poi::{PoiRecord, PoiType},
};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

#[cfg(test)]
#[path = "level_block_mutation_test.rs"]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum LiveBlockMutationError {
    #[error("live block mutation requires canonical chunk {0}")]
    MissingChunk(ChunkPos),
    #[error("live block mutation requires retained world runtime settings")]
    MissingSettings,
    #[error("live block mutation authority unavailable: {0}")]
    Unavailable(String),
    #[error(transparent)]
    BlockEntity(#[from] BlockEntityTickerError),
    #[error(transparent)]
    Light(#[from] crate::server::lighting::LiveLightError),
}

/// Source operations whose packet, navigation, event or declaring-block bodies
/// need the actual world's services. The enum records source call boundaries,
/// not precomputed successes. A callback may recursively mutate this level.
pub enum LiveBlockEffect {
    OnPlace {
        pos: BlockPos,
        state: BlockState,
        old: BlockState,
        moved: bool,
    },
    AfterRemoval {
        pos: BlockPos,
        old: BlockState,
        moved: bool,
    },
    PreRemoveBlockEntity {
        pos: BlockPos,
        old: BlockState,
    },
    PacketBlockChange {
        pos: BlockPos,
        state: BlockState,
        old: Option<BlockState>,
        flags: u32,
        limit: i32,
    },
    SendBlockUpdated {
        pos: BlockPos,
        old: BlockState,
        state: BlockState,
        flags: u32,
    },
    Neighbors {
        pos: BlockPos,
        old: BlockState,
    },
    OutputSignal {
        pos: BlockPos,
        state: BlockState,
    },
    IndirectShapes {
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        limit: i32,
    },
    NeighborShapes {
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        limit: i32,
    },
    DropPoi {
        pos: BlockPos,
    },
    RegisterPoi {
        record: Arc<PoiRecord>,
    },
}

pub trait LiveBlockMutationServices: BlockEntityWorldServices + Send {
    /// BlockBehaviour.onPlace's actual AsyncCatcher check. This must validate
    /// the server's tick-thread authority, not merely return success because
    /// the base block otherwise has no onPlace work.
    fn check_block_place_thread(&self) -> Result<(), LiveBlockMutationError>;
    fn block_effect(
        &mut self,
        level: &mut ServerLevel,
        effect: LiveBlockEffect,
    ) -> Result<(), LiveBlockMutationError>;
    /// Runs the real listener channel, including its no-listener decision.
    /// Returns true only when BlockPhysicsEvent cancels the next two calls.
    fn block_physics_event(
        &mut self,
        level: &mut ServerLevel,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<bool, LiveBlockMutationError>;
    fn changed_state_keeps_block_entity(
        &mut self,
        state: BlockState,
        old: BlockState,
    ) -> Result<bool, LiveBlockMutationError>;
}

// Pinned Java reflection/CollisionUtil cache outputs. Unlike guessed family
// lists, these distinguish inherited empty bodies from nonempty overrides and
// cache-null collision states from static collision boxes. Regenerate with
// run_block_mutation_metadata_probe.sh (Paper 0a99345).
static CALLBACKS: LazyLock<HashMap<&'static str, [&'static str; 4]>> = LazyLock::new(|| {
    include_str!("block_mutation_callbacks.tsv")
        .lines()
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 5);
            (fields[0], [fields[1], fields[2], fields[3], fields[4]])
        })
        .collect()
});
fn nonempty_callback(state: BlockState, index: usize) -> bool {
    CALLBACKS
        .get(state.block().name())
        .is_some_and(|owners| owners[index] != "BlockBehaviour")
}
fn different_light(old: BlockState, new: BlockState) -> bool {
    old != new
        && (old.light_dampening() != new.light_dampening()
            || old.light_emission() != new.light_emission()
            || old.use_shape_for_light_occlusion()
            || new.use_shape_for_light_occlusion())
}

impl ServerLevel {
    fn mutation_state(&self, pos: BlockPos) -> Result<BlockState, LiveBlockMutationError> {
        self.chunk_map
            .get_chunk(ChunkPos::containing(&pos))
            .map(|chunk| {
                BlockState::new(chunk.get_block_state(pos.get_x(), pos.get_y(), pos.get_z()))
            })
            .ok_or(LiveBlockMutationError::MissingChunk(ChunkPos::containing(
                &pos,
            )))
    }

    /// Source non-debug/non-capturing live path. This is not the separate
    /// tree/block snapshot capture workflow. No missing chunk is manufactured,
    /// and no unavailable effect is treated as a successful palette write.
    pub fn set_block_live<S: LiveBlockMutationServices>(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        update_limit: i32,
        services: &mut S,
    ) -> Result<bool, LiveBlockMutationError> {
        if !ChunkPos::containing(&pos).is_valid()
            || pos.get_y() < self.get_min_y()
            || i64::from(pos.get_y()) >= i64::from(self.get_min_y()) + i64::from(self.get_height())
        {
            return Ok(false);
        }
        let old = self.set_chunk_block_live(pos, state, flags, services)?;
        // AntiXray's onBlockChange runs even for an unchanged/failed chunk
        // write. Its real world controller, not an assumed disabled default,
        // decides whether this source callback has work.
        services.block_effect(
            self,
            LiveBlockEffect::PacketBlockChange {
                pos,
                state,
                old,
                flags,
                limit: update_limit,
            },
        )?;
        let Some(old) = old else {
            return Ok(false);
        };
        let current = self.mutation_state(pos)?;
        if current != state {
            return Ok(true);
        }
        // Level.setBlocksDirty is genuinely empty on ServerLevel.
        if flags & 2 != 0
            && self
                .chunk_map
                .get_chunk(ChunkPos::containing(&pos))
                .and_then(|chunk| chunk.get_full_status())
                .is_some_and(|s| s.is_or_after(FullChunkStatus::Full))
        {
            services.block_effect(
                self,
                LiveBlockEffect::SendBlockUpdated {
                    pos,
                    old,
                    state,
                    flags,
                },
            )?;
        }
        if flags & 1 != 0 {
            services.block_effect(self, LiveBlockEffect::Neighbors { pos, old })?;
            if state.has_analog_output_signal() {
                services.block_effect(self, LiveBlockEffect::OutputSignal { pos, state })?;
            }
        }
        if flags & 16 == 0 && update_limit > 0 {
            let neighbor_flags = flags & !(32 | 1);
            let limit = update_limit - 1;
            if nonempty_callback(old, 3) {
                services.block_effect(
                    self,
                    LiveBlockEffect::IndirectShapes {
                        pos,
                        state: old,
                        flags: neighbor_flags,
                        limit,
                    },
                )?;
            }
            if !services.block_physics_event(self, pos, state)? {
                services.block_effect(
                    self,
                    LiveBlockEffect::NeighborShapes {
                        pos,
                        state,
                        flags: neighbor_flags,
                        limit,
                    },
                )?;
                if nonempty_callback(state, 3) {
                    services.block_effect(
                        self,
                        LiveBlockEffect::IndirectShapes {
                            pos,
                            state,
                            flags: neighbor_flags,
                            limit,
                        },
                    )?;
                }
            }
        }
        if flags & 4096 == 0 {
            self.mutation_poi(pos, old, current, services)?;
        }
        Ok(true)
    }

    fn set_chunk_block_live<S: LiveBlockMutationServices>(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        flags: u32,
        services: &mut S,
    ) -> Result<Option<BlockState>, LiveBlockMutationError> {
        let chunk_pos = ChunkPos::containing(&pos);
        let (old, changed_emptiness) = {
            let chunk = self
                .chunk_map
                .get_chunk_mut(chunk_pos)
                .ok_or(LiveBlockMutationError::MissingChunk(chunk_pos))?;
            let base = chunk.generation_base_mut();
            let index = base.get_section_index(pos.get_y()) as usize;
            let section = base.get_section_mut(index);
            let was_empty = section.has_only_air();
            if was_empty && state.is_air() {
                return Ok(None);
            }
            let old = BlockState::new(section.set_block_state(
                pos.get_x() & 15,
                pos.get_y() & 15,
                pos.get_z() & 15,
                state.id(),
                &|s| BlockState::new(*s).is_air(),
                &|s| BlockState::new(*s).random_ticking(),
                &|s| BlockState::new(*s).fluid_empty(),
                &|s| BlockState::new(*s).fluid_randomly_ticking(),
                &|s| BlockState::new(*s).is_special_colliding(),
            ));
            if old == state {
                return Ok(None);
            }
            let is_empty = section.has_only_air();
            base.update_heightmaps_after(
                &[
                    Types::MotionBlocking,
                    Types::MotionBlockingNoLeaves,
                    Types::OceanFloor,
                    Types::WorldSurface,
                ],
                pos.get_x() & 15,
                pos.get_y(),
                pos.get_z() & 15,
                StateFlags {
                    is_air: state.is_air(),
                    blocks_motion: state.blocks_motion(),
                    has_fluid: !state.fluid_empty(),
                    is_leaves: state.is_in_tag("minecraft:leaves"),
                },
            );
            (old, (was_empty != is_empty).then_some(is_empty))
        };
        if let Some(is_empty) = changed_emptiness {
            self.queue_light_section_change(SectionPos::of_block_pos(&pos), is_empty)?;
            // ChunkSource.onSectionEmptinessChanged has an empty server body.
        }
        if different_light(old, state) {
            self.queue_light_block_change(pos)?;
        }
        let changed_block = old.block() != state.block();
        let moved = flags & 64 != 0;
        if changed_block
            && super::super::structure_snapshot_data::has_block_entity(old)
            && !(nonempty_callback(state, 2)
                && services.changed_state_keeps_block_entity(state, old)?)
        {
            if flags & 256 == 0 {
                let ticking = self
                    .runtime_settings()
                    .ok_or(LiveBlockMutationError::MissingSettings)?
                    .block_entity_ticking;
                let needs_effect = self
                    .load_native_block_entity(
                        pos,
                        NativeBlockEntityCreation::Immediate,
                        ticking,
                        services,
                    )?
                    .is_some_and(|entity| {
                        matches!(
                            entity.data(),
                            NativeBlockEntityData::Chest(_)
                                | NativeBlockEntityData::CreakingHeart(_)
                        )
                    });
                // Base BlockEntity preRemove only drops Containers. Hive and
                // spawner are not Containers; heart has its own real body.
                if needs_effect {
                    services
                        .block_effect(self, LiveBlockEffect::PreRemoveBlockEntity { pos, old })?;
                }
            }
            self.remove_native_block_entity(pos, services)?;
        }
        if (changed_block || state.is_in_tag("minecraft:rails"))
            && (flags & 1 != 0 || moved)
            && nonempty_callback(old, 1)
        {
            services.block_effect(self, LiveBlockEffect::AfterRemoval { pos, old, moved })?;
        }
        if self.mutation_state(pos)?.block() != state.block() {
            return Ok(None);
        }
        if flags & 512 == 0 {
            if nonempty_callback(state, 0) {
                services.block_effect(
                    self,
                    LiveBlockEffect::OnPlace {
                        pos,
                        state,
                        old,
                        moved,
                    },
                )?;
            } else {
                services.check_block_place_thread()?;
            }
        }
        if super::super::structure_snapshot_data::has_block_entity(state)
            && self.mutation_state(pos)?.block() == state.block()
        {
            let ticking = self
                .runtime_settings()
                .ok_or(LiveBlockMutationError::MissingSettings)?
                .block_entity_ticking;
            let existing = self.load_native_block_entity(
                pos,
                NativeBlockEntityCreation::Check,
                ticking,
                services,
            )?;
            let present = existing.is_some();
            let invalid = existing.is_some_and(|entity| !entity.is_valid_block_state(state));
            if invalid {
                self.remove_native_block_entity(pos, services)?;
            }
            if !present || invalid {
                self.load_native_block_entity(
                    pos,
                    NativeBlockEntityCreation::Immediate,
                    ticking,
                    services,
                )?;
            } else {
                self.chunk_map
                    .get_chunk_mut(chunk_pos)
                    .unwrap()
                    .generation_base_mut()
                    .get_block_entity_mut(&pos)
                    .unwrap()
                    .set_block_state(state);
                self.update_native_block_entity_ticker(pos, ticking, services)?;
            }
        }
        self.chunk_map
            .get_chunk_mut(chunk_pos)
            .unwrap()
            .mark_unsaved();
        Ok(Some(old))
    }

    #[cfg(unix)]
    fn mutation_poi<S: LiveBlockMutationServices>(
        &mut self,
        pos: BlockPos,
        old: BlockState,
        new: BlockState,
        services: &mut S,
    ) -> Result<(), LiveBlockMutationError> {
        let old_type = PoiType::for_state(old);
        let new_type = PoiType::for_state(new);
        if old_type == new_type {
            return Ok(());
        }
        // This bounded live owner executes primary-thread server.execute
        // work inline. Off-thread calls require the actual server dispatcher;
        // never mutate POIs inline merely because SKIP_ON_PLACE bypassed the
        // base block's AsyncCatcher check earlier in setBlockState.
        services.check_block_place_thread()?;
        // Primary-thread BlockableEventLoop.execute runs the source operation
        // immediately; no invented deferred queue or second POI map exists.
        let unavailable = |error: String| LiveBlockMutationError::Unavailable(error);
        if old_type.is_some() {
            let storage = self
                .poi_storage
                .as_mut()
                .ok_or_else(|| unavailable("POI storage is not installed".into()))?;
            storage
                .load_chunk(ChunkPos::containing(&pos))
                .map_err(|e| unavailable(e.to_string()))?;
            storage
                .manager_mut()
                .remove(pos)
                .map_err(|e| unavailable(e.to_string()))?;
            if !self.debug_synchronizers.is_sleeping() {
                services.block_effect(self, LiveBlockEffect::DropPoi { pos })?;
            }
        }
        if let Some(kind) = new_type {
            let storage = self
                .poi_storage
                .as_mut()
                .ok_or_else(|| unavailable("POI storage is not installed".into()))?;
            storage
                .load_chunk(ChunkPos::containing(&pos))
                .map_err(|e| unavailable(e.to_string()))?;
            let manager = storage.manager_mut();
            if old_type.is_none() && manager.record_at(pos).is_some() {
                manager
                    .remove(pos)
                    .map_err(|e| unavailable(e.to_string()))?;
            }
            let holder = manager
                .holder_for_type(kind)
                .map_err(|e| unavailable(e.to_string()))?;
            let record = manager
                .add_record(pos, holder)
                .map_err(|e| unavailable(e.to_string()))?;
            if let Some(record) = record
                && !self.debug_synchronizers.is_sleeping()
            {
                services.block_effect(self, LiveBlockEffect::RegisterPoi { record })?;
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    fn mutation_poi<S: LiveBlockMutationServices>(
        &mut self,
        _: BlockPos,
        old: BlockState,
        new: BlockState,
        _: &mut S,
    ) -> Result<(), LiveBlockMutationError> {
        if PoiType::for_state(old) == PoiType::for_state(new) {
            Ok(())
        } else {
            Err(LiveBlockMutationError::Unavailable(
                "POI storage is unavailable on this platform".into(),
            ))
        }
    }
}

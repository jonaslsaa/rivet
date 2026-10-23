//! Tick-owned NewChunkHolder.handleFullStatusChange orchestration. Actual
//! entity visibility and saved-tick unpacking execute through existing owners;
//! Paper platform hooks/listeners and autosave membership remain mandatory
//! authorities, not optional successful no-ops.
use super::{LevelEntityError, ServerLevel, TickRuntimeError};
use crate::server::level::{
    entity_tracking::TrackingAuthority, full_chunk_status::FullChunkStatus,
};
use rivet_registry::core::ChunkPos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HolderTransitionEffect {
    PreBorder,
    EnsureAutosave,
    Border,
    /// Add ticking membership and perform actual postProcessGeneration when
    /// needed. ServerLevel invokes real startTickingChunk AFTER this returns.
    Ticking,
    EntityTicking,
    NotEntityTicking,
    NotTicking,
    NotBorder,
    PostNotBorder,
    CompleteConsumers(FullChunkStatus),
    CheckUnload,
}
#[derive(Debug, thiserror::Error)]
pub enum HolderTransitionError {
    #[error("no canonical activated holder for {0}")]
    NotActivated(ChunkPos),
    #[error("holder transition authority unavailable: {0}")]
    Authority(String),
    #[error(transparent)]
    Entities(#[from] LevelEntityError),
    #[error(transparent)]
    Ticks(#[from] TickRuntimeError),
}
pub trait HolderTransitionAuthority {
    /// Refuse absent listener/runtime authority BEFORE changing current status.
    /// This must validate the actual world's lifecycle, not an empty registry
    /// invented solely to permit generated walking.
    fn validate(&self, world: &ServerLevel, pos: ChunkPos) -> Result<(), HolderTransitionError>;
    fn apply(
        &mut self,
        world: &mut ServerLevel,
        pos: ChunkPos,
        effect: HolderTransitionEffect,
    ) -> Result<(), HolderTransitionError>;
}

impl ServerLevel {
    /// Caller chooses the real source queued holder order. Notifications update
    /// pending neighbours, never execute their callbacks recursively. Errors
    /// preserve committed source effects; panics resume after finally cleanup.
    pub fn process_generated_holder_status(
        &mut self,
        pos: ChunkPos,
        authority: &mut dyn HolderTransitionAuthority,
        tracking: &mut dyn TrackingAuthority,
    ) -> Result<bool, HolderTransitionError> {
        if !self
            .generated_chunk_owner
            .as_ref()
            .is_some_and(|owner| owner.is_activated(pos))
            || self.chunk_map.get_chunk(pos).is_none()
        {
            return Err(HolderTransitionError::NotActivated(pos));
        }
        authority.validate(self, pos)?;
        if self.entity_tracking.is_none() {
            return Err(LevelEntityError::TrackingNotInstalled.into());
        }
        if self.entity_lookup.is_none() {
            return Err(LevelEntityError::MissingOwner.into());
        }
        if !self
            .generated_chunk_owner
            .as_mut()
            .unwrap()
            .begin_full_status_change(pos)
        {
            return Ok(false);
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.process_generated_holder_status_inner(pos, authority, tracking)
        }));
        self.generated_chunk_owner
            .as_mut()
            .unwrap()
            .end_full_status_change(pos);
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    fn change_holder_entity_status(
        &mut self,
        pos: ChunkPos,
        status: FullChunkStatus,
        tracking: &mut dyn TrackingAuthority,
    ) -> Result<(), HolderTransitionError> {
        let tracker = self.entity_tracking.as_mut().unwrap();
        self.entity_lookup
            .as_mut()
            .unwrap()
            .set_chunk_status(pos, status, &mut tracker.lifecycle(tracking))
            .map_err(LevelEntityError::from)?;
        Ok(())
    }
    fn publish_holder_current(&mut self, pos: ChunkPos, status: FullChunkStatus) {
        assert!(
            self.generated_chunk_owner
                .as_mut()
                .unwrap()
                .publish_full_status(pos, status),
            "source adjacent CURRENT status transition"
        );
    }
    fn process_generated_holder_status_inner(
        &mut self,
        pos: ChunkPos,
        authority: &mut dyn HolderTransitionAuthority,
        tracking: &mut dyn TrackingAuthority,
    ) -> Result<bool, HolderTransitionError> {
        let mut changed = false;
        loop {
            let owner = self.generated_chunk_owner.as_ref().unwrap();
            let current = owner.current_full_status(pos);
            let pending = owner
                .pending_full_status(pos)
                .expect("actual canonical holder");
            if current == pending {
                if current == FullChunkStatus::Inaccessible {
                    authority.apply(self, pos, HolderTransitionEffect::CheckUnload)?;
                }
                return Ok(changed);
            }
            changed = true;
            // Source snapshots pending/current for the whole upgrade/downgrade
            // ladder, then re-reads after plugin hooks rather than mid-ladder.
            if pending.is_or_after(current) {
                for ordinal in current as usize + 1..=pending as usize {
                    let status = FullChunkStatus::ALL[ordinal];
                    self.publish_holder_current(pos, status);
                    match status {
                        FullChunkStatus::Full => {
                            authority.apply(self, pos, HolderTransitionEffect::PreBorder)?;
                            authority.apply(self, pos, HolderTransitionEffect::EnsureAutosave)?;
                            self.change_holder_entity_status(pos, status, tracking)?;
                            authority.apply(self, pos, HolderTransitionEffect::Border)?;
                            self.generated_chunk_owner
                                .as_mut()
                                .unwrap()
                                .publish_full_border(pos, true);
                        }
                        FullChunkStatus::BlockTicking => {
                            self.change_holder_entity_status(pos, status, tracking)?;
                            authority.apply(self, pos, HolderTransitionEffect::Ticking)?;
                            self.activate_chunk_ticks(pos)?;
                        }
                        FullChunkStatus::EntityTicking => {
                            self.change_holder_entity_status(pos, status, tracking)?;
                            authority.apply(self, pos, HolderTransitionEffect::EntityTicking)?;
                        }
                        FullChunkStatus::Inaccessible => unreachable!(),
                    }
                    authority.apply(
                        self,
                        pos,
                        HolderTransitionEffect::CompleteConsumers(status),
                    )?;
                }
            } else {
                for ordinal in (pending as usize..current as usize).rev() {
                    let status = FullChunkStatus::ALL[ordinal];
                    match status {
                        FullChunkStatus::BlockTicking => {
                            self.change_holder_entity_status(pos, status, tracking)?;
                            authority.apply(self, pos, HolderTransitionEffect::NotEntityTicking)?;
                        }
                        FullChunkStatus::Full => {
                            self.change_holder_entity_status(pos, status, tracking)?;
                            authority.apply(self, pos, HolderTransitionEffect::NotTicking)?;
                        }
                        FullChunkStatus::Inaccessible => {
                            self.generated_chunk_owner
                                .as_mut()
                                .unwrap()
                                .publish_full_border(pos, false);
                            self.change_holder_entity_status(pos, status, tracking)?;
                            authority.apply(self, pos, HolderTransitionEffect::NotBorder)?;
                            authority.apply(self, pos, HolderTransitionEffect::PostNotBorder)?;
                        }
                        FullChunkStatus::EntityTicking => unreachable!(),
                    }
                    self.publish_holder_current(pos, status);
                }
            }
        }
    }
}

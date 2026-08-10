//! Port of `net.minecraft.world.level.lighting.LightEventListener` (MC 26.2,
//! Paper) — the live light-engine listener surface.
//!
//! Java: `LightEventListener.java` in `working/Paper`. The interface the level
//! and chunk pipelines call to notify the light engine of block/section
//! changes and to drain its work. Paper's vanilla implementation
//! (`LevelLightEngine`) routes these ops into Starlight via the provider seam;
//! the port's `LevelLightEngine` facade is the same class, so this interface
//! is the contract that facade and its consumers share.
//!
//! `updateSectionStatus(BlockPos, boolean)` and `updateSectionStatus(SectionPos,
//! boolean)` are two overloads in Java; Rust has no overloading, so the
//! `BlockPos` form is provided as a separate defaulted method
//! [`LightEventListener::update_section_status_pos`] that decomposes to the
//! `SectionPos` form, exactly as Java's default method delegates.

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};

/// `net.minecraft.world.level.lighting.LightEventListener`.
///
/// The listener is owned by the level's light engine, not shared: `&mut self`
/// for mutating ops, `&self` for readers. There is no `Sync` requirement — the
/// tick thread's single-owner model (OWNERSHIP.md) applies to every implementor.
pub trait LightEventListener {
    /// `checkBlock(BlockPos)` — a block change at `pos` may require light
    /// recalculation; the engine queues the work.
    fn check_block(&mut self, pos: BlockPos);

    /// `hasLightWork()` — whether the engine has queued light updates.
    fn has_light_work(&self) -> bool;

    /// `runLightUpdates()` — run queued light updates; returns the number of
    /// updates run (Java returns the leftover budget from `runUpdates`).
    fn run_light_updates(&mut self) -> i32;

    /// `updateSectionStatus(SectionPos, boolean)` — the section at `pos` became
    /// `section_empty` (a section-emptiness change).
    fn update_section_status(&mut self, pos: SectionPos, section_empty: bool);

    /// `setLightEnabled(ChunkPos, boolean)` — toggle whether light updates are
    /// processed for the chunk at `pos`.
    fn set_light_enabled(&mut self, pos: ChunkPos, enable: bool);

    /// `propagateLightSources(ChunkPos)` — the chunk's light sources changed;
    /// the engine recomputes propagation around `pos`.
    fn propagate_light_sources(&mut self, pos: ChunkPos);

    /// `updateSectionStatus(BlockPos, boolean)` — the default method that
    /// resolves the block position to its section, exactly as Java.
    fn update_section_status_pos(&mut self, pos: BlockPos, section_empty: bool) {
        self.update_section_status(SectionPos::of_block_pos(&pos), section_empty);
    }
}

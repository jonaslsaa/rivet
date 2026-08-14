//! `net.minecraft.world.level.border.BorderChangeListener` — the listener
//! interface notified on every world-border mutation.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! border/BorderChangeListener.java`.
//!
//! The border copies its listener list before iterating, so a listener
//! adding/removing during notification does not affect the current pass; the
//! Rust surface mirrors that by iterating an owned clone (`getListeners()`).

use super::world_border::WorldBorder;

/// `BorderChangeListener` — receives border-change notifications.
pub trait BorderChangeListener: Send + Sync {
    /// `onSetSize(WorldBorder, double newSize)`.
    fn on_set_size(&self, border: &WorldBorder, new_size: f64);

    /// `onLerpSize(WorldBorder, double fromSize, double targetSize, long
    /// ticks, long gameTime)`.
    fn on_lerp_size(
        &self,
        border: &WorldBorder,
        from_size: f64,
        target_size: f64,
        ticks: i64,
        game_time: i64,
    );

    /// `onSetCenter(WorldBorder, double x, double z)`.
    fn on_set_center(&self, border: &WorldBorder, x: f64, z: f64);

    /// `onSetWarningTime(WorldBorder, int time)`.
    fn on_set_warning_time(&self, border: &WorldBorder, time: i32);

    /// `onSetWarningBlocks(WorldBorder, int blocks)`.
    fn on_set_warning_blocks(&self, border: &WorldBorder, blocks: i32);

    /// `onSetDamagePerBlock(WorldBorder, double damagePerBlock)`.
    fn on_set_damage_per_block(&self, border: &WorldBorder, damage_per_block: f64);

    /// `onSetSafeZone(WorldBorder, double safeZone)`.
    fn on_set_safe_zone(&self, border: &WorldBorder, safe_zone: f64);
}

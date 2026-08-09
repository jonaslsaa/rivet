//! `net.minecraft.world.level.LightLayer` — which of the two light grids a
//! layer serves.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/LightLayer.java`. The Java class is a 2-line enum (`SKY`,
//! `BLOCK`). Its Java package is `world.level`; the port houses it under
//! `lighting` with the engines that consume it (#184).

/// `net.minecraft.world.level.LightLayer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightLayer {
    /// `LightLayer.SKY`.
    Sky,
    /// `LightLayer.BLOCK`.
    Block,
}

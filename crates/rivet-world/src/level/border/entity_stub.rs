//! STUB(mc.world.entity) — minimal `Entity` seam for the pending
//! `net.minecraft.world.entity` unit, created only so `WorldBorder`'s
//! `getDistanceToBorder(Entity)`/`isInsideCloseToBorder(Entity, AABB)` read
//! `entity.getX()`/`entity.getZ()`. The real `Entity` (in `rivet-entity`)
//! replaces this when the entity unit lands.
//!
//! Note: the full `EntityType` id-handle STUB already lives in
//! `crate::entity` (the biome cluster's prerequisite) — this module carries
//! only the position surface `WorldBorder` needs.

/// `net.minecraft.world.entity.Entity` — the position handle.
///
/// STUB(mc.world.entity): `getX()`/`getZ()` read the two coordinates the
/// border's distance checks need; everything else defers with the entity unit.
#[derive(Debug, Clone, Copy)]
pub struct Entity {
    /// `Entity.getX()`.
    pub x: f64,
    /// `Entity.getY()`.
    pub y: f64,
    /// `Entity.getZ()`.
    pub z: f64,
}

impl Entity {
    /// `new Entity(...)` — a position handle.
    pub const fn new(x: f64, y: f64, z: f64) -> Entity {
        Entity { x, y, z }
    }

    /// `Entity.getX()`.
    pub fn get_x(&self) -> f64 {
        self.x
    }

    /// `Entity.getZ()`.
    pub fn get_z(&self) -> f64 {
        self.z
    }
}

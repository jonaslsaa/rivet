//! Entity capability facade types and tick-thread commands (#508).
//!
//! Mirrors Paper's `org.bukkit.entity.Entity` boundary using generational
//! [`EntityHandle`] values so discarded or dead entities never alias newly
//! spawned entities in reused arena slots.

use super::super::handle::Handle;
use super::world::WorldId;

/// Marker type for generational handles referencing entities.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct EntityMarker;

/// Opaque generational handle to an authoritative entity.
pub type EntityHandle = Handle<EntityMarker>;

/// Value-copy snapshot of an entity's observable state.
#[derive(Clone, PartialEq, Debug)]
pub struct EntitySnapshot {
    pub handle: EntityHandle,
    pub world: WorldId,
    pub entity_type: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub alive: bool,
    pub custom_name: Option<String>,
}

/// Typed tick-thread mutation commands targeting entity state.
#[derive(Clone, PartialEq, Debug)]
pub enum EntityCommand {
    /// Teleports a live entity to `(world, x, y, z, yaw, pitch)`.
    /// Rejects non-finite coordinates, unloaded target worlds, and dead/stale entities.
    Teleport {
        handle: EntityHandle,
        world: WorldId,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    },
    /// Sets or clears a live entity's custom name.
    SetCustomName {
        handle: EntityHandle,
        name: Option<String>,
    },
    /// Discards (removes) a live entity, marking it dead and revoking its handle
    /// on subsequent mutations.
    Discard { handle: EntityHandle },
}

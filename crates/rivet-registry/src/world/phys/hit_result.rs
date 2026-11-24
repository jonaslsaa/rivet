//! `net.minecraft.world.phys.HitResult` — the abstract hit result.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/HitResult.java`.
//!
//! `HitResult` has subclasses, so it follows OWNERSHIP.md §Class hierarchies:
//! the struct holds the abstract class's own field (`location`), is embedded in
//! each subclass, and the value trait [`HitResultBehavior`] carries the virtual
//! `getType()` and the inherited `getLocation()`. `distanceTo(Entity)` takes a
//! live type and is `HitResultLive::distance_to` in `rivet-server` at the same
//! mirrored path (D15 split rule).

use std::any::Any;

use super::vec3::Vec3;

/// `HitResult.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Miss,
    Block,
    Entity,
}

/// `HitResult`'s own state — `protected final Vec3 location`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitResult {
    pub(crate) location: Vec3,
}

impl HitResult {
    /// `HitResult(Vec3)`.
    pub fn new(location: Vec3) -> HitResult {
        HitResult { location }
    }

    /// `HitResult.getLocation()`.
    pub fn get_location(&self) -> Vec3 {
        self.location
    }
}

/// The value trait of the `HitResult` hierarchy: `BlockHitResult` and
/// `EntityHitResult` implement it. `instanceof BlockHitResult` is
/// `as_any().downcast_ref::<BlockHitResult>()` (neither subclass has
/// subclasses).
pub trait HitResultBehavior: Any {
    /// The embedded `HitResult` state.
    fn hit_result(&self) -> &HitResult;

    /// `HitResult.getType()` — abstract.
    fn get_type(&self) -> Type;

    /// `HitResult.getLocation()`.
    fn get_location(&self) -> Vec3 {
        self.hit_result().get_location()
    }

    /// `instanceof` support.
    fn as_any(&self) -> &dyn Any;
}

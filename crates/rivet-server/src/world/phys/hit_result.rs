//! `net.minecraft.world.phys.HitResult` — the live member (D15 split rule).
//! The struct and its value members are
//! `rivet_registry::world::phys::hit_result`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/HitResult.java`.

use rivet_registry::world::phys::HitResult;

use crate::world::entity::entity::Entity;

/// The `Entity`-taking members of `HitResult`.
pub trait HitResultLive {
    /// `HitResult.distanceTo(Entity)` — the squared distance from the entity's
    /// position to the hit location.
    fn distance_to(&self, entity: &Entity) -> f64;
}

impl HitResultLive for HitResult {
    fn distance_to(&self, entity: &Entity) -> f64 {
        let location = self.get_location();
        let xd = location.x - entity.pos.x;
        let yd = location.y - entity.pos.y;
        let zd = location.z - entity.pos.z;
        xd * xd + yd * yd + zd * zd
    }
}

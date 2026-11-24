//! `net.minecraft.world.phys.EntityHitResult` — a hit on an entity.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/EntityHitResult.java`.
//!
//! The Java `Entity` reference is the entity handle `EntityId`; `OWNERSHIP.md`
//! calls for an arena `EntityKey`, which does not exist yet, so the existing
//! single `EntityId` handle stands in.

use std::any::Any;

use rivet_registry::world::phys::hit_result::Type;
use rivet_registry::world::phys::{HitResult, HitResultBehavior, Vec3};

use crate::world::entity::entity_id::EntityId;

/// `EntityHitResult extends HitResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityHitResult {
    hit_result: HitResult,
    entity: EntityId,
}

impl EntityHitResult {
    /// `EntityHitResult(Entity, Vec3)`; `EntityHitResult(Entity)` is
    /// `new(entity, entity.position())`.
    pub fn new(entity: EntityId, location: Vec3) -> EntityHitResult {
        EntityHitResult {
            hit_result: HitResult::new(location),
            entity,
        }
    }

    /// `EntityHitResult.getEntity()`.
    pub fn get_entity(&self) -> EntityId {
        self.entity
    }
}

impl HitResultBehavior for EntityHitResult {
    fn hit_result(&self) -> &HitResult {
        &self.hit_result
    }

    /// `EntityHitResult.getType()` — `Type.ENTITY`.
    fn get_type(&self) -> Type {
        Type::Entity
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_hit_result_reports_entity_type_and_location() {
        let hit = EntityHitResult::new(EntityId(7), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(hit.get_type(), Type::Entity);
        assert_eq!(hit.get_location(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(hit.get_entity(), EntityId(7));
    }
}

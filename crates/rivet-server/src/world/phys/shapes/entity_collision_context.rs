//! `net.minecraft.world.phys.shapes.EntityCollisionContext` (and its nested
//! `Empty`). The class holds an `Entity`, so it is whole in `rivet-server`
//! (D15); it implements `rivet_world`'s `CollisionContext`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/EntityCollisionContext.java`.
//!
//! The `Entity` reference is the entity handle `EntityId` (see
//! `phys::entity_hit_result`).
//!
//! STUB(mc.world.level.material) `EntityCollisionContext.canStandOnFluid(FluidState, FluidState)`.
//! STUB(mc.world.level.block.state) `EntityCollisionContext.getCollisionShape(BlockState, CollisionGetter, BlockPos)`.
//! STUB(mc.world.entity) `EntityCollisionContext(Entity, boolean, boolean)`, the `@Deprecated` constructor that reads the entity's descending flag, `getY()` and main-hand item.

use rivet_registry::core::{Axis, BlockPos};
use rivet_world::item::item_id::ItemId;
use rivet_world::item::item_stack::{ItemInstance, ItemStack};
use rivet_world::phys::shapes::collision_context::CollisionContext;
use rivet_world::phys::shapes::voxel_shape::VoxelShape;

use crate::world::entity::entity_id::EntityId;

/// `EntityCollisionContext`; `Empty` is [`EntityCollisionContext::empty`].
#[derive(Clone, Debug)]
pub struct EntityCollisionContext {
    descending: bool,
    entity_bottom: f64,
    placement: bool,
    held_item: ItemStack,
    always_collide_with_fluid: bool,
    entity: Option<EntityId>,
    /// `this instanceof EntityCollisionContext.Empty`.
    is_empty_context: bool,
}

impl EntityCollisionContext {
    /// The six-argument constructor.
    pub fn new(
        descending: bool,
        placement: bool,
        entity_bottom: f64,
        held_item: ItemStack,
        always_collide_with_fluid: bool,
        entity: Option<EntityId>,
    ) -> EntityCollisionContext {
        EntityCollisionContext {
            descending,
            entity_bottom,
            placement,
            held_item,
            always_collide_with_fluid,
            entity,
            is_empty_context: false,
        }
    }

    /// `new EntityCollisionContext.Empty(boolean alwaysCollideWithFluid)`.
    pub fn empty(always_collide_with_fluid: bool) -> EntityCollisionContext {
        EntityCollisionContext {
            is_empty_context: true,
            ..EntityCollisionContext::new(
                false,
                false,
                -f64::MAX,
                ItemStack::empty(),
                always_collide_with_fluid,
                None,
            )
        }
    }

    /// `EntityCollisionContext.getEntity()`.
    pub fn get_entity(&self) -> Option<EntityId> {
        self.entity
    }
}

impl CollisionContext for EntityCollisionContext {
    fn is_holding_item(&self, item: ItemId) -> bool {
        self.held_item.is(item)
    }

    fn always_collide_with_fluid(&self) -> bool {
        self.always_collide_with_fluid
    }

    fn is_descending(&self) -> bool {
        self.descending
    }

    /// `Empty.isAbove` is `defaultValue`; otherwise
    /// `entityBottom > pos.getY() + shape.max(Y) - 1.0E-5F`.
    fn is_above(&self, shape: &VoxelShape, pos: &BlockPos, default_value: bool) -> bool {
        if self.is_empty_context {
            return default_value;
        }
        self.entity_bottom > pos.get_y() as f64 + shape.max(Axis::Y) - 1.0E-5f32 as f64
    }

    fn is_placement(&self) -> bool {
        self.placement
    }
}

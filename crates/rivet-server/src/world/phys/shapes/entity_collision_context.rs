//! `net.minecraft.world.phys.shapes.EntityCollisionContext` (and its nested
//! `Empty`, a struct embedding it). The class holds an `Entity`, so it is whole in `rivet-server`
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

/// `EntityCollisionContext`; the nested subclass `Empty` is [`Empty`].
#[derive(Clone, Debug)]
pub struct EntityCollisionContext {
    descending: bool,
    entity_bottom: f64,
    placement: bool,
    held_item: ItemStack,
    always_collide_with_fluid: bool,
    entity: Option<EntityId>,
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

    /// `entityBottom > pos.getY() + shape.max(Y) - 1.0E-5F` (the `float`
    /// literal widens).
    fn is_above(&self, shape: &VoxelShape, pos: &BlockPos, _default_value: bool) -> bool {
        self.entity_bottom > pos.get_y() as f64 + shape.max(Axis::Y) - 1.0E-5f32 as f64
    }

    fn is_placement(&self) -> bool {
        self.placement
    }
}

/// `EntityCollisionContext.Empty` — `extends EntityCollisionContext`, so it
/// embeds its parent (OWNERSHIP.md §Class hierarchies) and overrides `isAbove`.
#[derive(Clone, Debug)]
pub struct Empty {
    entity_collision_context: EntityCollisionContext,
}

impl Empty {
    /// `new EntityCollisionContext.Empty(boolean alwaysCollideWithFluid)`.
    pub fn new(always_collide_with_fluid: bool) -> Empty {
        Empty {
            entity_collision_context: EntityCollisionContext::new(
                false,
                false,
                -f64::MAX,
                ItemStack::empty(),
                always_collide_with_fluid,
                None,
            ),
        }
    }

    /// The embedded `EntityCollisionContext`.
    pub fn entity_collision_context(&self) -> &EntityCollisionContext {
        &self.entity_collision_context
    }
}

impl CollisionContext for Empty {
    fn is_holding_item(&self, item: ItemId) -> bool {
        self.entity_collision_context.is_holding_item(item)
    }

    fn always_collide_with_fluid(&self) -> bool {
        self.entity_collision_context.always_collide_with_fluid()
    }

    fn is_descending(&self) -> bool {
        self.entity_collision_context.is_descending()
    }

    /// `Empty.isAbove` — `defaultValue`.
    fn is_above(&self, _shape: &VoxelShape, _pos: &BlockPos, default_value: bool) -> bool {
        default_value
    }

    fn is_placement(&self) -> bool {
        self.entity_collision_context.is_placement()
    }
}

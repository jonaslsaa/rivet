//! Real native loaded values retained by world registration. This is not a
//! tag-backed substitute for an entity and does not imply AI ticking support.
use super::{chest_minecart::NativeChestMinecart, generation_creature::NativeGenerationCreature};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_registry::core::Vec3;
use rivet_util::mth::Uuid;

#[derive(Debug)]
pub enum NativeLoadedEntity {
    ChestMinecart(NativeChestMinecart),
    Creature(NativeGenerationCreature),
}
impl NativeLoadedEntity {
    pub fn entity_id(&self) -> i32 {
        match self {
            Self::ChestMinecart(e) => e.entity_id(),
            Self::Creature(e) => e.entity().identity().entity_id,
        }
    }
    pub fn uuid(&self) -> Uuid {
        match self {
            Self::ChestMinecart(e) => e.uuid(),
            Self::Creature(e) => e.entity().identity().uuid,
        }
    }
    pub fn world_uuid(&self) -> Uuid {
        match self {
            Self::ChestMinecart(e) => e.world_uuid(),
            Self::Creature(e) => e.entity().identity().world_uuid,
        }
    }
    pub fn position(&self) -> Vec3 {
        match self {
            Self::ChestMinecart(e) => e.position(),
            Self::Creature(e) => e.entity().position(),
        }
    }
    pub fn old_position(&self) -> Vec3 {
        match self {
            Self::ChestMinecart(e) => e.old_position(),
            Self::Creature(e) => e.entity().old_position(),
        }
    }
    pub fn is_alive(&self) -> bool {
        match self {
            Self::ChestMinecart(e) => e.is_alive(),
            Self::Creature(e) => e.is_alive(),
        }
    }
    pub fn generation(&self) -> bool {
        match self {
            Self::ChestMinecart(e) => e.generation(),
            Self::Creature(e) => e.entity().generation(),
        }
    }
    pub fn set_generation(&mut self, value: bool) {
        match self {
            Self::ChestMinecart(e) => e.set_generation(value),
            Self::Creature(e) => e.entity_mut().set_generation(value),
        }
    }
    pub fn discard(&mut self) {
        match self {
            Self::ChestMinecart(e) => e.discard(),
            Self::Creature(e) => e.entity_mut().discard(),
        }
    }
    pub fn save(&self) -> Option<CompoundTag> {
        match self {
            Self::ChestMinecart(e) => e.save(),
            Self::Creature(e) => e.save(),
        }
    }
}

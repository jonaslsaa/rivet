//! `net.minecraft.world.InteractionHand` — the live member (D15 split rule).
//! The enum is `rivet_registry::world::interaction_hand::InteractionHand`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/InteractionHand.java`.

use rivet_registry::world::interaction_hand::InteractionHand;

use crate::world::entity::equipment_slot::EquipmentSlot;

/// The `EquipmentSlot`-typed members of `InteractionHand`.
pub trait InteractionHandLive {
    /// `InteractionHand.asEquipmentSlot()`.
    fn as_equipment_slot(&self) -> EquipmentSlot;
}

impl InteractionHandLive for InteractionHand {
    fn as_equipment_slot(&self) -> EquipmentSlot {
        match self {
            InteractionHand::MainHand => EquipmentSlot::MainHand,
            InteractionHand::OffHand => EquipmentSlot::OffHand,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_equipment_slot_maps_the_hands() {
        assert_eq!(
            InteractionHand::MainHand.as_equipment_slot(),
            EquipmentSlot::MainHand
        );
        assert_eq!(
            InteractionHand::OffHand.as_equipment_slot(),
            EquipmentSlot::OffHand
        );
    }
}

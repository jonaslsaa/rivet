//! Paper `net.minecraft.world.WorldlyContainer` — sided inventory contract.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/WorldlyContainer.java`.

use rivet_registry::core::Direction;

use crate::inventory::{Container, InventoryItemStack};

/// Paper `net.minecraft.world.WorldlyContainer`.
///
/// Sided containers expose face-dependent slot indices and directional
/// insertion/extraction predicates consumed by hoppers and other transfer
/// mechanisms.
pub trait WorldlyContainer: Container {
    /// `WorldlyContainer.getSlotsForFace(Direction)` — ordered slot indices
    /// accessible from `direction`.
    fn get_slots_for_face(&self, direction: Direction) -> &[usize];

    /// `WorldlyContainer.canPlaceItemThroughFace(int, ItemStack, @Nullable Direction)`.
    fn can_place_item_through_face(
        &self,
        slot: usize,
        stack: &InventoryItemStack,
        direction: Option<Direction>,
    ) -> bool;

    /// `WorldlyContainer.canTakeItemThroughFace(int, ItemStack, Direction)`.
    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &InventoryItemStack,
        direction: Direction,
    ) -> bool;
}

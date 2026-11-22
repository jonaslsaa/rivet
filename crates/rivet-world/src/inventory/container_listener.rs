//! Paper `net.minecraft.world.inventory.ContainerListener`.

use super::item_stack::InventoryItemStack;

/// Stable handle for a registered [`MenuContainerListener`], preserving
/// registration order and deduplication in `AbstractContainerMenu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(pub u64);

/// Paper `net.minecraft.world.inventory.ContainerListener`.
pub trait MenuContainerListener {
    /// `ContainerListener.slotChanged(AbstractContainerMenu, int, ItemStack)`.
    fn slot_changed(&mut self, container_id: i32, slot_index: usize, stack: &InventoryItemStack);

    /// `ContainerListener.dataChanged(AbstractContainerMenu, int, int)`.
    fn data_changed(&mut self, container_id: i32, data_slot_index: usize, value: i32);
}

//! Paper `net.minecraft.world.inventory.ContainerSynchronizer`.

use super::item_stack::InventoryItemStack;

/// Paper `net.minecraft.world.inventory.ContainerSynchronizer`.
pub trait ContainerSynchronizer {
    /// `ContainerSynchronizer.sendInitialData(AbstractContainerMenu, List<ItemStack>, ItemStack, int[])`.
    fn send_initial_data(
        &mut self,
        container_id: i32,
        items: &[InventoryItemStack],
        carried: &InventoryItemStack,
        initial_data: &[i32],
        state_id: i32,
    );

    /// `ContainerSynchronizer.sendSlotChange(AbstractContainerMenu, int, ItemStack)`.
    fn send_slot_change(
        &mut self,
        container_id: i32,
        slot: usize,
        stack: &InventoryItemStack,
        state_id: i32,
    );

    /// `ContainerSynchronizer.sendCarriedChange(AbstractContainerMenu, ItemStack)`.
    fn send_carried_change(&mut self, container_id: i32, stack: &InventoryItemStack, state_id: i32);

    /// `ContainerSynchronizer.sendDataChange(AbstractContainerMenu, int, int)`.
    fn send_data_change(&mut self, container_id: i32, id: usize, value: i32);
}

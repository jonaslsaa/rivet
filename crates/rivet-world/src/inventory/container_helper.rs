//! Paper `net.minecraft.world.ContainerHelper`.

use super::item_stack::InventoryItemStack;

/// Paper `net.minecraft.world.ContainerHelper`.
pub struct ContainerHelper;

impl ContainerHelper {
    /// `ContainerHelper.removeItem(List<ItemStack>, int, int)`.
    pub fn remove_item(
        items: &mut [InventoryItemStack],
        slot: usize,
        count: i32,
    ) -> InventoryItemStack {
        if slot < items.len() && !items[slot].is_empty() && count > 0 {
            items[slot].split(count)
        } else {
            InventoryItemStack::empty()
        }
    }

    /// `ContainerHelper.takeItem(List<ItemStack>, int)`.
    pub fn take_item(items: &mut [InventoryItemStack], slot: usize) -> InventoryItemStack {
        if slot < items.len() {
            std::mem::take(&mut items[slot])
        } else {
            InventoryItemStack::empty()
        }
    }
}

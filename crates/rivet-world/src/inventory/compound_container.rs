//! Paper `net.minecraft.world.CompoundContainer`.

use super::container::{Container, ContainerAccessContext};
use super::item_stack::InventoryItemStack;

/// Paper `net.minecraft.world.CompoundContainer`.
#[derive(Debug, Clone)]
pub struct CompoundContainer<C1: Container, C2: Container> {
    pub container1: C1,
    pub container2: C2,
}

impl<C1: Container, C2: Container> CompoundContainer<C1, C2> {
    pub fn new(container1: C1, container2: C2) -> Self {
        Self {
            container1,
            container2,
        }
    }
}

impl<C1: Container, C2: Container> Container for CompoundContainer<C1, C2> {
    fn get_container_size(&self) -> usize {
        self.container1.get_container_size() + self.container2.get_container_size()
    }

    fn is_empty(&self) -> bool {
        self.container1.is_empty() && self.container2.is_empty()
    }

    fn get_item(&self, slot: usize) -> &InventoryItemStack {
        let first_size = self.container1.get_container_size();
        if slot >= first_size {
            self.container2.get_item(slot - first_size)
        } else {
            self.container1.get_item(slot)
        }
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> InventoryItemStack {
        let first_size = self.container1.get_container_size();
        if slot >= first_size {
            self.container2.remove_item(slot - first_size, count)
        } else {
            self.container1.remove_item(slot, count)
        }
    }

    fn remove_item_no_update(&mut self, slot: usize) -> InventoryItemStack {
        let first_size = self.container1.get_container_size();
        if slot >= first_size {
            self.container2.remove_item_no_update(slot - first_size)
        } else {
            self.container1.remove_item_no_update(slot)
        }
    }

    fn set_item(&mut self, slot: usize, stack: InventoryItemStack) {
        let first_size = self.container1.get_container_size();
        if slot >= first_size {
            self.container2.set_item(slot - first_size, stack);
        } else {
            self.container1.set_item(slot, stack);
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        self.container1
            .get_max_stack_size()
            .min(self.container2.get_max_stack_size())
    }

    fn set_changed(&mut self) {
        self.container1.set_changed();
        self.container2.set_changed();
    }

    fn still_valid(&self, ctx: &ContainerAccessContext) -> bool {
        self.container1.still_valid(ctx) && self.container2.still_valid(ctx)
    }

    fn start_open(&mut self, ctx: &ContainerAccessContext) {
        self.container1.start_open(ctx);
        self.container2.start_open(ctx);
    }

    fn stop_open(&mut self, ctx: &ContainerAccessContext) {
        self.container1.stop_open(ctx);
        self.container2.stop_open(ctx);
    }

    fn can_place_item(&self, slot: usize, stack: &InventoryItemStack) -> bool {
        let first_size = self.container1.get_container_size();
        if slot >= first_size {
            self.container2.can_place_item(slot - first_size, stack)
        } else {
            self.container1.can_place_item(slot, stack)
        }
    }

    fn can_take_item(&self, slot: usize, stack: &InventoryItemStack) -> bool {
        let first_size = self.container1.get_container_size();
        if slot >= first_size {
            self.container2.can_take_item(slot - first_size, stack)
        } else {
            self.container1.can_take_item(slot, stack)
        }
    }

    fn clear_content(&mut self) {
        self.container1.clear_content();
        self.container2.clear_content();
    }
}

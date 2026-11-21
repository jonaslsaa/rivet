//! Paper `net.minecraft.world.SimpleContainer`.

use std::sync::LazyLock;

use super::container::{Container, ContainerAccessContext, DEFAULT_MAX_STACK_SIZE};
use super::container_helper::ContainerHelper;
use super::item_stack::{InventoryItemStack, ItemId};

static EMPTY_STACK: LazyLock<InventoryItemStack> = LazyLock::new(InventoryItemStack::empty);

/// Paper `net.minecraft.world.SimpleContainer`.
#[derive(Debug, Clone)]
pub struct SimpleContainer {
    items: Vec<InventoryItemStack>,
    max_stack_size: i32,
    changed_count: u64,
    still_valid: bool,
}

impl SimpleContainer {
    /// `new SimpleContainer(int)`.
    pub fn new(size: usize) -> Self {
        Self {
            items: vec![InventoryItemStack::empty(); size],
            max_stack_size: DEFAULT_MAX_STACK_SIZE,
            changed_count: 0,
            still_valid: true,
        }
    }

    /// `new SimpleContainer(ItemStack...)`.
    pub fn from_items(items: Vec<InventoryItemStack>) -> Self {
        Self {
            items,
            max_stack_size: DEFAULT_MAX_STACK_SIZE,
            changed_count: 0,
            still_valid: true,
        }
    }

    /// Construct a `SimpleContainer` with an explicit max stack size.
    pub fn with_max_stack_size(size: usize, max_stack_size: i32) -> Self {
        Self {
            items: vec![InventoryItemStack::empty(); size],
            max_stack_size: max_stack_size.max(1),
            changed_count: 0,
            still_valid: true,
        }
    }

    /// Configure whether `still_valid` returns true (useful for block-entity
    /// teardown and negative tests).
    pub fn set_still_valid(&mut self, valid: bool) {
        self.still_valid = valid;
    }

    /// Number of `set_changed()` notifications recorded on this container.
    pub fn changed_count(&self) -> u64 {
        self.changed_count
    }

    /// `SimpleContainer.getItems()`.
    pub fn items(&self) -> &[InventoryItemStack] {
        &self.items
    }

    /// `SimpleContainer.addItem(ItemStack)` — merges into occupied matching
    /// slots first, then places into empty slots, returning any leftover stack.
    pub fn add_item(&mut self, stack: InventoryItemStack) -> InventoryItemStack {
        if stack.is_empty() {
            return InventoryItemStack::empty();
        }
        let mut remaining = stack;
        self.move_item_to_occupied_slots_with_same_type(&mut remaining);
        if remaining.is_empty() {
            return InventoryItemStack::empty();
        }
        self.move_item_to_empty_slots(&mut remaining);
        if remaining.is_empty() {
            InventoryItemStack::empty()
        } else {
            remaining
        }
    }

    /// `SimpleContainer.canAddItem(ItemStack)`.
    pub fn can_add_item(&self, stack: &InventoryItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        for target in &self.items {
            if target.is_empty()
                || (InventoryItemStack::is_same_item_same_components(target, stack)
                    && target.get_count() < self.get_max_stack_size_for_item(target))
            {
                return true;
            }
        }
        false
    }

    /// `SimpleContainer.removeItemType(Item, int)` — scans in reverse slot
    /// order matching Paper's `SimpleContainer.removeItemType`.
    pub fn remove_item_type(&mut self, item: ItemId, count: i32) -> InventoryItemStack {
        if item.is_air() || count <= 0 {
            return InventoryItemStack::empty();
        }
        let mut collected = InventoryItemStack::empty();
        let mut needed = count;
        for slot in (0..self.items.len()).rev() {
            if self.items[slot].item() == item && !self.items[slot].is_empty() {
                let taken = self.remove_item(slot, needed);
                if !taken.is_empty() {
                    let taken_count = taken.get_count();
                    if collected.is_empty() {
                        collected = taken;
                    } else {
                        collected.grow(taken_count);
                    }
                    needed -= taken_count;
                    if needed <= 0 {
                        break;
                    }
                }
            }
        }
        collected
    }

    /// `SimpleContainer.removeAllItems()`.
    pub fn remove_all_items(&mut self) -> Vec<InventoryItemStack> {
        let non_empty: Vec<InventoryItemStack> = self
            .items
            .iter()
            .filter(|stack| !stack.is_empty())
            .cloned()
            .collect();
        self.clear_content();
        non_empty
    }

    fn move_item_to_occupied_slots_with_same_type(&mut self, source: &mut InventoryItemStack) {
        for slot in 0..self.items.len() {
            if InventoryItemStack::is_same_item_same_components(&self.items[slot], source) {
                self.move_items_between_stacks(source, slot);
                if source.is_empty() {
                    return;
                }
            }
        }
    }

    fn move_item_to_empty_slots(&mut self, source: &mut InventoryItemStack) {
        for slot in 0..self.items.len() {
            if self.items[slot].is_empty() {
                let max = self.get_max_stack_size_for_item(source);
                let to_move = source.split(max);
                self.set_item(slot, to_move);
                if source.is_empty() {
                    return;
                }
            }
        }
    }

    fn move_items_between_stacks(&mut self, source: &mut InventoryItemStack, target_slot: usize) {
        let max_size = self.get_max_stack_size_for_item(&self.items[target_slot]);
        let transfer = source
            .get_count()
            .min(max_size - self.items[target_slot].get_count());
        if transfer > 0 {
            self.items[target_slot].grow(transfer);
            source.shrink(transfer);
            self.set_changed();
        }
    }
}

impl Container for SimpleContainer {
    fn get_container_size(&self) -> usize {
        self.items.len()
    }

    fn is_empty(&self) -> bool {
        self.items.iter().all(InventoryItemStack::is_empty)
    }

    fn get_item(&self, slot: usize) -> &InventoryItemStack {
        self.items.get(slot).unwrap_or(&EMPTY_STACK)
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> InventoryItemStack {
        let result = ContainerHelper::remove_item(&mut self.items, slot, count);
        if !result.is_empty() {
            self.set_changed();
        }
        result
    }

    fn remove_item_no_update(&mut self, slot: usize) -> InventoryItemStack {
        if slot >= self.items.len() || self.items[slot].is_empty() {
            return InventoryItemStack::empty();
        }
        ContainerHelper::take_item(&mut self.items, slot)
    }

    fn set_item(&mut self, slot: usize, mut stack: InventoryItemStack) {
        if slot >= self.items.len() {
            return;
        }
        let limit = self.get_max_stack_size_for_item(&stack);
        stack.limit_size(limit);
        self.items[slot] = stack;
        self.set_changed();
    }

    fn get_max_stack_size(&self) -> i32 {
        self.max_stack_size
    }

    fn set_changed(&mut self) {
        self.changed_count = self.changed_count.wrapping_add(1);
    }

    fn still_valid(&self, ctx: &ContainerAccessContext) -> bool {
        self.still_valid && !ctx.is_removed && ctx.can_interact
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
        self.set_changed();
    }
}

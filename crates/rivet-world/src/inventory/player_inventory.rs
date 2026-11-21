//! Paper `net.minecraft.world.entity.player.Inventory` foundation.
//!
//! Covers the 36 non-equipment inventory slots (9 hotbar + 27 main),
//! offhand/equipment slots, selected hotbar slot validation, remaining-space
//! priority (`selected` -> `offhand` -> `0..36`), item addition, item
//! conservation on `place_item_back_in_inventory`, and `times_changed`.
//! Player/ServerPlayer entity ownership remains with issue #445.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use thiserror::Error;

use super::container::{
    Container, ContainerAccessContext, ContainerHelper, DEFAULT_MAX_STACK_SIZE,
};
use super::item_stack::InventoryItemStack;

/// `Inventory.INVENTORY_SIZE` (9 hotbar + 27 main inventory slots).
pub const INVENTORY_SIZE: usize = 36;

/// `Inventory.SELECTION_SIZE` (hotbar slots `0..9`).
pub const SELECTION_SIZE: usize = 9;

/// `Inventory.SLOT_OFFHAND`.
pub const SLOT_OFFHAND: usize = 40;

/// `Inventory.SLOT_BODY_ARMOR`.
pub const SLOT_BODY_ARMOR: usize = 41;

/// `Inventory.SLOT_SADDLE`.
pub const SLOT_SADDLE: usize = 42;

/// `Inventory.NOT_FOUND_INDEX`.
pub const NOT_FOUND_INDEX: i32 = -1;

/// Equipment slots mapped to indices `36..=40` (`FEET`, `LEGS`, `CHEST`, `HEAD`, `OFFHAND`).
pub const EQUIPMENT_SLOT_START: usize = 36;
pub const EQUIPMENT_SLOT_END_INCLUSIVE: usize = SLOT_OFFHAND;

static EMPTY_STACK: LazyLock<InventoryItemStack> = LazyLock::new(InventoryItemStack::empty);

/// Error returned when setting an out-of-range hotbar selection index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("Invalid hotbar slot: {0} (expected 0..9)")]
pub struct InvalidHotbarSlot(pub i32);

/// Paper `net.minecraft.world.entity.player.Inventory` storage and behavior.
#[derive(Debug, Clone)]
pub struct PlayerInventory {
    items: Vec<InventoryItemStack>,
    equipment: BTreeMap<usize, InventoryItemStack>,
    selected: usize,
    times_changed: i32,
    max_stack_size: i32,
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerInventory {
    /// Construct an empty player inventory with 36 main slots and 5 equipment
    /// slots (`36..=40`).
    pub fn new() -> Self {
        let mut equipment = BTreeMap::new();
        for slot in EQUIPMENT_SLOT_START..=EQUIPMENT_SLOT_END_INCLUSIVE {
            equipment.insert(slot, InventoryItemStack::empty());
        }
        Self {
            items: vec![InventoryItemStack::empty(); INVENTORY_SIZE],
            equipment,
            selected: 0,
            times_changed: 0,
            max_stack_size: DEFAULT_MAX_STACK_SIZE,
        }
    }

    /// `Inventory.isHotbarSlot(int)`.
    pub const fn is_hotbar_slot(slot: i32) -> bool {
        slot >= 0 && slot < SELECTION_SIZE as i32
    }

    /// `Inventory.getSelectedSlot()`.
    pub const fn get_selected_slot(&self) -> usize {
        self.selected
    }

    /// `Inventory.setSelectedSlot(int)`.
    pub fn set_selected_slot(&mut self, slot: i32) -> Result<(), InvalidHotbarSlot> {
        if !Self::is_hotbar_slot(slot) {
            return Err(InvalidHotbarSlot(slot));
        }
        self.selected = slot as usize;
        Ok(())
    }

    /// `Inventory.getSelectedItem()`.
    pub fn get_selected_item(&self) -> &InventoryItemStack {
        &self.items[self.selected]
    }

    /// `Inventory.setSelectedItem(ItemStack)`.
    pub fn set_selected_item(&mut self, stack: InventoryItemStack) -> InventoryItemStack {
        let old = std::mem::replace(&mut self.items[self.selected], stack);
        self.set_changed();
        old
    }

    /// `Inventory.getTimesChanged()`.
    pub const fn get_times_changed(&self) -> i32 {
        self.times_changed
    }

    /// `Inventory.getNonEquipmentItems()`.
    pub fn non_equipment_items(&self) -> &[InventoryItemStack] {
        &self.items
    }

    /// `Inventory.getFreeSlot()`.
    pub fn get_free_slot(&self) -> i32 {
        for (i, stack) in self.items.iter().enumerate() {
            if stack.is_empty() {
                return i as i32;
            }
        }
        NOT_FOUND_INDEX
    }

    /// `Inventory.hasRemainingSpaceForItem(ItemStack, ItemStack)`.
    fn has_remaining_space_for_item(
        &self,
        destination: &InventoryItemStack,
        source: &InventoryItemStack,
    ) -> bool {
        !destination.is_empty()
            && InventoryItemStack::is_same_item_same_components(destination, source)
            && destination.is_stackable()
            && destination.get_count() < self.get_max_stack_size_for_item(destination)
    }

    /// `Inventory.getSlotWithRemainingSpace(ItemStack)` — checks `selected`,
    /// then `SLOT_OFFHAND`, then `0..36`, matching Paper's exact order.
    pub fn get_slot_with_remaining_space(&self, stack: &InventoryItemStack) -> i32 {
        if self.has_remaining_space_for_item(&self.items[self.selected], stack) {
            return self.selected as i32;
        }
        if let Some(offhand) = self.equipment.get(&SLOT_OFFHAND)
            && self.has_remaining_space_for_item(offhand, stack)
        {
            return SLOT_OFFHAND as i32;
        }
        for (i, slot_stack) in self.items.iter().enumerate() {
            if self.has_remaining_space_for_item(slot_stack, stack) {
                return i as i32;
            }
        }
        NOT_FOUND_INDEX
    }

    /// `Inventory.findSlotMatchingItem(ItemStack)`.
    pub fn find_slot_matching_item(&self, stack: &InventoryItemStack) -> i32 {
        for (i, slot_stack) in self.items.iter().enumerate() {
            if !slot_stack.is_empty()
                && InventoryItemStack::is_same_item_same_components(slot_stack, stack)
            {
                return i as i32;
            }
        }
        NOT_FOUND_INDEX
    }

    /// `Inventory.getSuitableHotbarSlot()`.
    pub fn get_suitable_hotbar_slot(&self) -> usize {
        for offset in 0..SELECTION_SIZE {
            let slot = (self.selected + offset) % SELECTION_SIZE;
            if self.items[slot].is_empty() {
                return slot;
            }
        }
        self.selected
    }

    /// `Inventory.add(ItemStack)`.
    pub fn add(&mut self, stack: &mut InventoryItemStack) -> bool {
        self.add_at_slot(NOT_FOUND_INDEX, stack)
    }

    fn is_valid_slot(&self, slot: usize) -> bool {
        slot < self.items.len() || self.equipment.contains_key(&slot)
    }

    /// `Inventory.add(int, ItemStack)` — exact Paper flow:
    /// 1. Empty stack -> `false`.
    /// 2. If stack is non-stackable / damaged -> place directly in `slot` or free slot.
    /// 3. Otherwise loop `add_resource` until empty or no progress.
    pub fn add_at_slot(&mut self, mut slot: i32, stack: &mut InventoryItemStack) -> bool {
        if stack.is_empty() {
            return false;
        }
        if slot != NOT_FOUND_INDEX && (slot < 0 || !self.is_valid_slot(slot as usize)) {
            return false;
        }
        if !stack.is_stackable() {
            if slot == NOT_FOUND_INDEX {
                slot = self.get_free_slot();
            }
            if slot == NOT_FOUND_INDEX {
                return false;
            }
            let target = slot as usize;
            if target < self.items.len() && self.items[target].is_empty() {
                let limit = self.get_max_stack_size_for_item(stack);
                if limit <= 0 {
                    return false;
                }
                let moved = stack.split(limit);
                self.set_item(target, moved);
                return true;
            }
            return false;
        }

        let initial_count = stack.get_count();
        loop {
            let before = stack.get_count();
            let remaining = if slot == NOT_FOUND_INDEX {
                self.add_resource_auto(stack)
            } else {
                self.add_resource_in_slot(slot as usize, stack)
            };
            stack.set_count(remaining);
            if stack.is_empty() || stack.get_count() >= before {
                break;
            }
        }
        stack.get_count() < initial_count
    }

    /// `Inventory.placeItemBackInInventory(ItemStack)` — tries to merge into
    /// existing matching slots and then empty slots. Returns `Some(remainder)`
    /// if the inventory could not fit the entire stack (caller must drop it to
    /// preserve item conservation).
    pub fn place_item_back_in_inventory(
        &mut self,
        mut stack: InventoryItemStack,
    ) -> Option<InventoryItemStack> {
        while !stack.is_empty() {
            let mut slot = self.get_slot_with_remaining_space(&stack);
            if slot == NOT_FOUND_INDEX {
                slot = self.get_free_slot();
            }
            if slot == NOT_FOUND_INDEX {
                return Some(stack);
            }
            let target = slot as usize;
            let current_count = self.get_item(target).get_count();
            let space = self.get_max_stack_size_for_item(&stack) - current_count;
            if space <= 0 {
                return Some(stack);
            }
            let mut to_insert = stack.split(space);
            if !self.add_at_slot(slot, &mut to_insert) && !to_insert.is_empty() {
                // Restore anything that could not be inserted so no items are lost.
                if stack.is_empty() {
                    return Some(to_insert);
                }
                stack.grow(to_insert.get_count());
                return Some(stack);
            }
        }
        None
    }

    fn add_resource_auto(&mut self, stack: &InventoryItemStack) -> i32 {
        let mut slot = self.get_slot_with_remaining_space(stack);
        if slot == NOT_FOUND_INDEX {
            slot = self.get_free_slot();
        }
        if slot == NOT_FOUND_INDEX {
            stack.get_count()
        } else {
            self.add_resource_in_slot(slot as usize, stack)
        }
    }

    fn add_resource_in_slot(&mut self, slot: usize, stack: &InventoryItemStack) -> i32 {
        let mut count = stack.get_count();
        if !self.is_valid_slot(slot) {
            return count;
        }
        let current = self.get_item(slot).clone();
        if current.is_empty() {
            let max = self.get_max_stack_size_for_item(stack);
            let place = count.min(max);
            if place > 0 {
                self.set_item(slot, stack.copy_with_count(place));
                count -= place;
            }
            return count;
        }
        if !InventoryItemStack::is_same_item_same_components(&current, stack) {
            return count;
        }
        let max = self.get_max_stack_size_for_item(&current);
        let space = max - current.get_count();
        let place = count.min(space);
        if place <= 0 {
            return count;
        }
        let mut updated = current;
        updated.grow(place);
        self.set_item(slot, updated);
        count - place
    }
}

impl Container for PlayerInventory {
    fn get_container_size(&self) -> usize {
        self.items.len() + self.equipment.len()
    }

    fn is_empty(&self) -> bool {
        self.items.iter().all(InventoryItemStack::is_empty)
            && self.equipment.values().all(InventoryItemStack::is_empty)
    }

    fn get_item(&self, slot: usize) -> &InventoryItemStack {
        if slot < self.items.len() {
            &self.items[slot]
        } else {
            self.equipment.get(&slot).unwrap_or(&EMPTY_STACK)
        }
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> InventoryItemStack {
        let result = if slot < self.items.len() {
            ContainerHelper::remove_item(&mut self.items, slot, count)
        } else if let Some(entry) = self.equipment.get_mut(&slot) {
            if !entry.is_empty() && count > 0 {
                entry.split(count)
            } else {
                InventoryItemStack::empty()
            }
        } else {
            InventoryItemStack::empty()
        };
        if !result.is_empty() {
            self.set_changed();
        }
        result
    }

    fn remove_item_no_update(&mut self, slot: usize) -> InventoryItemStack {
        if slot < self.items.len() {
            ContainerHelper::take_item(&mut self.items, slot)
        } else if let Some(entry) = self.equipment.get_mut(&slot) {
            std::mem::take(entry)
        } else {
            InventoryItemStack::empty()
        }
    }

    fn set_item(&mut self, slot: usize, mut stack: InventoryItemStack) {
        let limit = self.get_max_stack_size_for_item(&stack);
        stack.limit_size(limit);
        if slot < self.items.len() {
            self.items[slot] = stack;
            self.set_changed();
        } else if let Some(entry) = self.equipment.get_mut(&slot) {
            *entry = stack;
            self.set_changed();
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        self.max_stack_size
    }

    fn set_changed(&mut self) {
        self.times_changed = self.times_changed.wrapping_add(1);
    }

    fn still_valid(&self, ctx: &ContainerAccessContext) -> bool {
        !ctx.is_removed && ctx.can_interact
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
        for slot in self.equipment.values_mut() {
            *slot = InventoryItemStack::empty();
        }
        self.set_changed();
    }
}

//! Paper `net.minecraft.world.level.block.entity.CrafterBlockEntity`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/CrafterBlockEntity.java`.

use rivet_nbt::{compound_tag::CompoundTag, int_array_tag::IntArrayTag, tag::Tag};

use crate::inventory::{
    Container, ContainerAccessContext, ContainerData, ContainerHelper, InventoryItemStack,
};

use super::nbt::{BaseContainerState, load_all_items, save_all_items};
use super::randomizable::RandomizableContainer;

/// `CrafterBlockEntity.CONTAINER_WIDTH = 3`.
pub const CONTAINER_WIDTH: usize = 3;
/// `CrafterBlockEntity.CONTAINER_HEIGHT = 3`.
pub const CONTAINER_HEIGHT: usize = 3;
/// `CrafterBlockEntity.CONTAINER_SIZE = 9`.
pub const CONTAINER_SIZE: usize = 9;
/// `CrafterBlockEntity.SLOT_DISABLED = 1`.
pub const SLOT_DISABLED: i32 = 1;
/// `CrafterBlockEntity.SLOT_ENABLED = 0`.
pub const SLOT_ENABLED: i32 = 0;
/// `CrafterBlockEntity.DATA_TRIGGERED = 9`.
pub const DATA_TRIGGERED: usize = 9;
/// `CrafterBlockEntity.NUM_DATA = 10`.
pub const NUM_DATA: usize = 10;
/// `CrafterBlockEntity.DEFAULT_CRAFTING_TICKS_REMAINING = 0`.
pub const DEFAULT_CRAFTING_TICKS_REMAINING: i32 = 0;
/// `CrafterBlockEntity.DEFAULT_TRIGGERED = 0`.
pub const DEFAULT_TRIGGERED: i32 = 0;

/// Paper `net.minecraft.world.level.block.entity.CrafterBlockEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct CrafterBlockEntity {
    pub base: BaseContainerState,
    items: [InventoryItemStack; CONTAINER_SIZE],
    disabled_slots: [bool; CONTAINER_SIZE],
    triggered: bool,
    pub crafting_ticks_remaining: i32,
}

impl Default for CrafterBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl CrafterBlockEntity {
    /// Construct an empty 3x3 crafter with all 9 slots enabled.
    pub fn new() -> Self {
        Self {
            base: BaseContainerState::new(),
            items: std::array::from_fn(|_| InventoryItemStack::empty()),
            disabled_slots: [false; CONTAINER_SIZE],
            triggered: false,
            crafting_ticks_remaining: DEFAULT_CRAFTING_TICKS_REMAINING,
        }
    }

    /// `CrafterBlockEntity.isSlotDisabled(int)`.
    pub fn is_slot_disabled(&self, slot: usize) -> bool {
        slot < CONTAINER_SIZE && self.disabled_slots[slot]
    }

    /// `CrafterBlockEntity.slotCanBeDisabled(int)` — only empty slots may be toggled.
    pub fn slot_can_be_disabled(&self, slot: usize) -> bool {
        slot < CONTAINER_SIZE && self.items[slot].is_empty()
    }

    /// `CrafterBlockEntity.setSlotState(int, boolean)` — only empty slots may
    /// be toggled; an applied toggle always marks the block entity changed.
    ///
    /// Returns whether the slot could be toggled (`slotCanBeDisabled`).
    pub fn set_slot_state(&mut self, slot: usize, enabled: bool) -> bool {
        if !self.slot_can_be_disabled(slot) {
            return false;
        }
        self.disabled_slots[slot] = !enabled;
        self.set_changed();
        true
    }

    /// `CrafterBlockEntity.setTriggered(boolean)`.
    pub fn set_triggered(&mut self, triggered: bool) {
        self.triggered = triggered;
    }

    /// `CrafterBlockEntity.isTriggered()`.
    pub const fn is_triggered(&self) -> bool {
        self.triggered
    }

    /// `CrafterBlockEntity.setCraftingTicksRemaining(int)`.
    pub fn set_crafting_ticks_remaining(&mut self, ticks: i32) {
        self.crafting_ticks_remaining = ticks;
    }

    /// `CrafterBlockEntity.getRedstoneSignal()` — counts occupied or disabled slots (`0..=9`).
    pub fn get_redstone_signal(&self) -> i32 {
        let mut count = 0_i32;
        for i in 0..CONTAINER_SIZE {
            if !self.items[i].is_empty() || self.is_slot_disabled(i) {
                count = count.wrapping_add(1);
            }
        }
        count
    }

    /// `CrafterBlockEntity.serverTick(...)`.
    ///
    /// Returns `true` on the tick `crafting_ticks_remaining` reaches `0`
    /// (signaling the caller to clear `CrafterBlock.CRAFTING = false`).
    pub fn server_tick(&mut self) -> bool {
        let next = self.crafting_ticks_remaining.wrapping_sub(1);
        if next >= 0 {
            self.crafting_ticks_remaining = next;
            return next == 0;
        }
        false
    }

    /// `CrafterBlockEntity.smallerStackExist(int, ItemStack, int)`.
    fn smaller_stack_exist(
        &self,
        base_size: i32,
        base_item: &InventoryItemStack,
        base_slot: usize,
    ) -> bool {
        for i in (base_slot + 1)..CONTAINER_SIZE {
            if !self.is_slot_disabled(i) {
                let slot_stack = &self.items[i];
                if slot_stack.is_empty()
                    || (slot_stack.get_count() < base_size
                        && InventoryItemStack::is_same_item_same_components(slot_stack, base_item))
                {
                    return true;
                }
            }
        }
        false
    }

    /// `CrafterBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_additional(&self, output: &mut CompoundTag) {
        self.base.save_base(output);
        output.put_int("crafting_ticks_remaining", self.crafting_ticks_remaining);
        // `trySaveLootTable` returns `false` for Paper block entities (they carry
        // `lootableData`), so `Items` is always written.
        self.base.save_loot_table(output);
        save_all_items(output, &self.items, true);
        let disabled_indices: Vec<i32> = (0..CONTAINER_SIZE)
            .filter(|&i| self.disabled_slots[i])
            .map(|i| i as i32)
            .collect();
        output.put(
            "disabled_slots".into(),
            Tag::IntArray(IntArrayTag::new(disabled_indices)),
        );
        output.put_int("triggered", i32::from(self.triggered));
    }

    /// `CrafterBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        self.crafting_ticks_remaining =
            input.get_int_or("crafting_ticks_remaining", DEFAULT_CRAFTING_TICKS_REMAINING);
        // `tryLoadLootTable` returns `false` for Paper block entities (they carry
        // `lootableData`), so `Items` is always read.
        self.base.load_loot_table(input);
        load_all_items(input, &mut self.items);
        self.disabled_slots = [false; CONTAINER_SIZE];
        if let Some(Tag::IntArray(arr)) = input.get("disabled_slots") {
            for &idx in &arr.data {
                // `slotCanBeDisabled(ix)`: in range and the loaded slot is empty.
                if let Ok(slot) = usize::try_from(idx)
                    && self.slot_can_be_disabled(slot)
                {
                    self.disabled_slots[slot] = true;
                }
            }
        }
        self.triggered = input.get_int_or("triggered", DEFAULT_TRIGGERED) == 1;
    }
}

impl Container for CrafterBlockEntity {
    fn get_container_size(&self) -> usize {
        CONTAINER_SIZE
    }

    fn is_empty(&self) -> bool {
        self.items.iter().all(InventoryItemStack::is_empty)
    }

    fn get_item(&self, slot: usize) -> &InventoryItemStack {
        self.items.get(slot).unwrap_or_else(|| &EMPTY_STACK)
    }

    fn remove_item(&mut self, slot: usize, count: i32) -> InventoryItemStack {
        let result = ContainerHelper::remove_item(&mut self.items, slot, count);
        if !result.is_empty() {
            self.set_changed();
        }
        result
    }

    fn remove_item_no_update(&mut self, slot: usize) -> InventoryItemStack {
        ContainerHelper::take_item(&mut self.items, slot)
    }

    fn set_item(&mut self, slot: usize, mut stack: InventoryItemStack) {
        if slot >= CONTAINER_SIZE {
            return;
        }
        if self.is_slot_disabled(slot) {
            self.set_slot_state(slot, true);
        }
        let max_size = self.get_max_stack_size_for_item(&stack);
        stack.limit_size(max_size);
        self.items[slot] = stack;
        self.set_changed();
    }

    fn set_changed(&mut self) {
        self.base.set_changed();
    }

    fn still_valid(&self, _ctx: &ContainerAccessContext) -> bool {
        true
    }

    fn can_place_item(&self, slot: usize, _stack: &InventoryItemStack) -> bool {
        if slot >= CONTAINER_SIZE || self.is_slot_disabled(slot) {
            return false;
        }
        let slot_stack = &self.items[slot];
        let current_size = slot_stack.get_count();
        current_size < slot_stack.get_max_stack_size()
            && (slot_stack.is_empty() || !self.smaller_stack_exist(current_size, slot_stack, slot))
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

impl ContainerData for CrafterBlockEntity {
    fn get(&self, index: usize) -> i32 {
        if index < CONTAINER_SIZE {
            if self.disabled_slots[index] {
                SLOT_DISABLED
            } else {
                SLOT_ENABLED
            }
        } else if index == DATA_TRIGGERED {
            i32::from(self.triggered)
        } else {
            0
        }
    }

    fn set(&mut self, index: usize, value: i32) {
        if index < CONTAINER_SIZE {
            self.disabled_slots[index] = value == SLOT_DISABLED;
        } else if index == DATA_TRIGGERED {
            self.triggered = value == 1;
        }
    }

    fn get_count(&self) -> usize {
        NUM_DATA
    }
}

impl RandomizableContainer for CrafterBlockEntity {
    fn container_state(&self) -> &BaseContainerState {
        &self.base
    }

    fn container_state_mut(&mut self) -> &mut BaseContainerState {
        &mut self.base
    }
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);

//! Paper `net.minecraft.world.level.block.entity.BarrelBlockEntity`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/BarrelBlockEntity.java`.

use rivet_nbt::compound_tag::CompoundTag;

use crate::inventory::{Container, ContainerAccessContext, ContainerHelper, InventoryItemStack};

use super::nbt::{BaseContainerState, load_all_items, save_all_items};

/// `BarrelBlockEntity` container size (`27`).
pub const BARREL_CONTAINER_SIZE: usize = 27;

/// Paper `net.minecraft.world.level.block.entity.BarrelBlockEntity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarrelBlockEntity {
    pub base: BaseContainerState,
    items: Vec<InventoryItemStack>,
    open_count: i32,
    is_open: bool,
}

impl Default for BarrelBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl BarrelBlockEntity {
    /// Construct an empty 27-slot barrel block entity.
    pub fn new() -> Self {
        Self {
            base: BaseContainerState::new(),
            items: vec![InventoryItemStack::empty(); BARREL_CONTAINER_SIZE],
            open_count: 0,
            is_open: false,
        }
    }

    /// Number of currently viewing players.
    pub fn open_count(&self) -> i32 {
        self.open_count
    }

    /// Whether `BarrelBlock.OPEN` is currently `true`.
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// `BarrelBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_additional(&self, output: &mut CompoundTag) {
        self.base.save_base(output);
        if !self.base.try_save_loot_table(output) {
            save_all_items(output, &self.items, true);
        }
    }

    /// `BarrelBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        self.items = vec![InventoryItemStack::empty(); BARREL_CONTAINER_SIZE];
        if !self.base.try_load_loot_table(input) {
            load_all_items(input, &mut self.items);
        }
    }
}

impl Container for BarrelBlockEntity {
    fn get_container_size(&self) -> usize {
        BARREL_CONTAINER_SIZE
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
        if slot >= self.items.len() {
            return;
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

    fn start_open(&mut self, _ctx: &ContainerAccessContext) {
        let prev = self.open_count;
        self.open_count = self.open_count.wrapping_add(1);
        if prev == 0 {
            self.is_open = true;
        }
    }

    fn stop_open(&mut self, _ctx: &ContainerAccessContext) {
        self.open_count = self.open_count.wrapping_sub(1).max(0);
        if self.open_count == 0 {
            self.is_open = false;
        }
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);

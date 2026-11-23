//! Paper `net.minecraft.world.level.block.entity.ChestBlockEntity`.
//!
//! Mirrors `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/entity/ChestBlockEntity.java`.

use rivet_nbt::compound_tag::CompoundTag;

use crate::inventory::{
    CompoundContainer, Container, ContainerAccessContext, ContainerHelper, InventoryItemStack,
};

use super::nbt::{BaseContainerState, load_all_items, save_all_items};
use super::randomizable::RandomizableContainer;

/// `ChestBlockEntity.DEFAULT_ROWS * 9 = 27`.
pub const CHEST_CONTAINER_SIZE: usize = 27;

/// `ChestBlockEntity.EVENT_SET_OPEN_COUNT = 1`.
pub const EVENT_SET_OPEN_COUNT: i32 = 1;

/// Paper `net.minecraft.world.level.block.entity.ChestBlockEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChestBlockEntity {
    pub base: BaseContainerState,
    items: Vec<InventoryItemStack>,
    open_count: i32,
    openness: f32,
    o_openness: f32,
}

impl Default for ChestBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl ChestBlockEntity {
    /// Construct an empty 27-slot chest block entity.
    pub fn new() -> Self {
        Self {
            base: BaseContainerState::new(),
            items: vec![InventoryItemStack::empty(); CHEST_CONTAINER_SIZE],
            open_count: 0,
            openness: 0.0,
            o_openness: 0.0,
        }
    }

    /// Number of currently viewing players.
    pub fn open_count(&self) -> i32 {
        self.open_count
    }

    /// Current lid openness in `[0.0, 1.0]` (`ChestLidController.getOpenness`).
    pub fn openness(&self) -> f32 {
        self.openness
    }

    /// Previous tick's lid openness.
    pub fn o_openness(&self) -> f32 {
        self.o_openness
    }

    /// `ChestBlockEntity.triggerEvent(int, int)`.
    pub fn trigger_event(&mut self, event_id: i32, param: i32) -> bool {
        if event_id == EVENT_SET_OPEN_COUNT {
            self.open_count = param.max(0);
            true
        } else {
            false
        }
    }

    /// `ChestLidController.tickLid()` — advances lid animation by one tick.
    pub fn tick_lid(&mut self) {
        self.o_openness = self.openness;
        const SPEED: f32 = 0.1;
        if self.open_count > 0 {
            self.openness = (self.openness + SPEED).min(1.0);
        } else {
            self.openness = (self.openness - SPEED).max(0.0);
        }
    }

    /// Combine two chests into a 54-slot double chest (`CompoundContainer`).
    pub fn double_chest(
        first: ChestBlockEntity,
        second: ChestBlockEntity,
    ) -> CompoundContainer<ChestBlockEntity, ChestBlockEntity> {
        CompoundContainer::new(first, second)
    }

    /// `ChestBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_additional(&self, output: &mut CompoundTag) {
        self.base.save_base(output);
        // `trySaveLootTable` returns `false` for Paper block entities (they carry
        // `lootableData`), so `Items` is always written.
        self.base.save_loot_table(output);
        save_all_items(output, &self.items, true);
    }

    /// `ChestBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_additional(&mut self, input: &CompoundTag) {
        self.base.load_base(input);
        self.items = vec![InventoryItemStack::empty(); CHEST_CONTAINER_SIZE];
        // `tryLoadLootTable` returns `false` for Paper block entities (they carry
        // `lootableData`), so `Items` is always read.
        self.base.load_loot_table(input);
        load_all_items(input, &mut self.items);
    }
}

impl Container for ChestBlockEntity {
    fn get_container_size(&self) -> usize {
        CHEST_CONTAINER_SIZE
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
        self.open_count = self.open_count.wrapping_add(1);
    }

    fn stop_open(&mut self, _ctx: &ContainerAccessContext) {
        self.open_count = self.open_count.wrapping_sub(1).max(0);
    }

    fn clear_content(&mut self) {
        for slot in &mut self.items {
            *slot = InventoryItemStack::empty();
        }
    }
}

impl RandomizableContainer for ChestBlockEntity {
    fn container_state(&self) -> &BaseContainerState {
        &self.base
    }

    fn container_state_mut(&mut self) -> &mut BaseContainerState {
        &mut self.base
    }
}

static EMPTY_STACK: std::sync::LazyLock<InventoryItemStack> =
    std::sync::LazyLock::new(InventoryItemStack::empty);

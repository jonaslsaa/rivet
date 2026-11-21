//! Paper `net.minecraft.world.Container`, `ContainerHelper`,
//! `SimpleContainer`, and `CompoundContainer`.

use std::sync::LazyLock;

use super::item_stack::{InventoryItemStack, ItemId};

/// `Container.LARGE_MAX_STACK_SIZE` (Paper 26.2).
pub const LARGE_MAX_STACK_SIZE: i32 = 99;

/// `Container.DEFAULT_MAX_STACK_SIZE` (Paper `Container.getMaxStackSize()` default is 99 in 26.2;
/// `SimpleContainer` defaults to `Container.DEFAULT_MAX_STACK_SIZE` = 99 via `Container`).
pub const DEFAULT_MAX_STACK_SIZE: i32 = LARGE_MAX_STACK_SIZE;

/// `Container.DEFAULT_DISTANCE_BUFFER`.
pub const DEFAULT_DISTANCE_BUFFER: f32 = 4.0;

static EMPTY_STACK: LazyLock<InventoryItemStack> = LazyLock::new(InventoryItemStack::empty);

/// Minimal player/interaction context for `Container::still_valid`,
/// `Slot::may_pickup`, and menu clicks without coupling to #445's `Player`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerAccessContext {
    pub player_id: u64,
    pub is_removed: bool,
    pub can_interact: bool,
    pub creative_mode: bool,
}

impl ContainerAccessContext {
    /// Survival-mode interactive player context.
    pub const fn survival(player_id: u64) -> Self {
        Self {
            player_id,
            is_removed: false,
            can_interact: true,
            creative_mode: false,
        }
    }

    /// Creative-mode interactive player context.
    pub const fn creative(player_id: u64) -> Self {
        Self {
            player_id,
            is_removed: false,
            can_interact: true,
            creative_mode: true,
        }
    }
}

/// Paper `net.minecraft.world.Container`.
pub trait Container {
    /// `Container.getContainerSize()`.
    fn get_container_size(&self) -> usize;

    /// `Container.isEmpty()`.
    fn is_empty(&self) -> bool;

    /// `Container.getItem(int)`.
    fn get_item(&self, slot: usize) -> &InventoryItemStack;

    /// `Container.removeItem(int, int)`.
    fn remove_item(&mut self, slot: usize, count: i32) -> InventoryItemStack;

    /// `Container.removeItemNoUpdate(int)`.
    fn remove_item_no_update(&mut self, slot: usize) -> InventoryItemStack;

    /// `Container.setItem(int, ItemStack)`.
    fn set_item(&mut self, slot: usize, stack: InventoryItemStack);

    /// `Container.getMaxStackSize()`.
    fn get_max_stack_size(&self) -> i32 {
        DEFAULT_MAX_STACK_SIZE
    }

    /// `Container.getMaxStackSize(ItemStack)`.
    fn get_max_stack_size_for_item(&self, stack: &InventoryItemStack) -> i32 {
        self.get_max_stack_size().min(stack.get_max_stack_size())
    }

    /// `Container.setChanged()`.
    fn set_changed(&mut self);

    /// `Container.stillValid(Player)`.
    fn still_valid(&self, ctx: &ContainerAccessContext) -> bool;

    /// `Container.startOpen(ContainerUser)`.
    fn start_open(&mut self, _ctx: &ContainerAccessContext) {}

    /// `Container.stopOpen(ContainerUser)`.
    fn stop_open(&mut self, _ctx: &ContainerAccessContext) {}

    /// `Container.canPlaceItem(int, ItemStack)`.
    fn can_place_item(&self, _slot: usize, _stack: &InventoryItemStack) -> bool {
        true
    }

    /// `Container.canTakeItem(Container, int, ItemStack)`.
    fn can_take_item(&self, _slot: usize, _stack: &InventoryItemStack) -> bool {
        true
    }

    /// `Container.clearContent()`.
    fn clear_content(&mut self);
}

/// Extension helpers matching Paper `Container` default methods that cannot be
/// object-safe or benefit from generic predicates.
pub trait ContainerExt: Container {
    /// `Container.countItem(Item)`.
    fn count_item(&self, item: ItemId) -> i32 {
        let mut count: i32 = 0;
        for slot in 0..self.get_container_size() {
            let stack = self.get_item(slot);
            if !stack.is_empty() && stack.item() == item {
                count = count.wrapping_add(stack.get_count());
            }
        }
        count
    }

    /// `Container.hasAnyOf(Set<Item>)`.
    fn has_any_of(&self, items: &[ItemId]) -> bool {
        self.has_any_matching(|stack| !stack.is_empty() && items.contains(&stack.item()))
    }

    /// `Container.hasAnyMatching(Predicate<ItemStack>)`.
    fn has_any_matching<F: FnMut(&InventoryItemStack) -> bool>(&self, mut predicate: F) -> bool {
        for slot in 0..self.get_container_size() {
            if predicate(self.get_item(slot)) {
                return true;
            }
        }
        false
    }

    /// `Container.iterator()` — captures initial size and yields slots in
    /// ascending index order.
    fn iter_items(&self) -> ContainerIter<'_>
    where
        Self: Sized,
    {
        ContainerIter {
            container: self,
            index: 0,
            size: self.get_container_size(),
        }
    }
}

impl<C: Container + ?Sized> ContainerExt for C {}

/// Ascending-slot iterator matching Paper's `Container.Iterator`.
pub struct ContainerIter<'a> {
    container: &'a dyn Container,
    index: usize,
    size: usize,
}

impl<'a> Iterator for ContainerIter<'a> {
    type Item = &'a InventoryItemStack;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.size {
            return None;
        }
        let item = self.container.get_item(self.index);
        self.index += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.size.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ContainerIter<'_> {}

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

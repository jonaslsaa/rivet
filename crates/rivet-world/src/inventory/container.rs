//! Paper `net.minecraft.world.Container`.

use super::item_stack::{InventoryItemStack, ItemId};

/// `Container.LARGE_MAX_STACK_SIZE` (Paper 26.2).
pub const LARGE_MAX_STACK_SIZE: i32 = 99;

/// `Container.DEFAULT_MAX_STACK_SIZE` (Paper `Container.getMaxStackSize()` default is 99 in 26.2;
/// `SimpleContainer` defaults to `Container.DEFAULT_MAX_STACK_SIZE` = 99 via `Container`).
pub const DEFAULT_MAX_STACK_SIZE: i32 = LARGE_MAX_STACK_SIZE;

/// `Container.DEFAULT_DISTANCE_BUFFER`.
pub const DEFAULT_DISTANCE_BUFFER: f32 = 4.0;

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

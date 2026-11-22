//! Paper `net.minecraft.world.inventory.Slot`.

use super::container::{Container, ContainerAccessContext};
use super::item_stack::{InventoryItemStack, ItemId};

/// Behavioral policy for a [`Slot`], replacing Java `Slot` subclass overrides
/// without introducing ownership cycles.
pub trait SlotPolicy: Send + Sync {
    /// `Slot.mayPlace(ItemStack)`.
    fn may_place(&self, _stack: &InventoryItemStack) -> bool {
        true
    }

    /// `Slot.mayPickup(Player)`.
    fn may_pickup(&self, _ctx: &ContainerAccessContext) -> bool {
        true
    }

    /// `Slot.getMaxStackSize()`.
    fn get_max_stack_size(&self, container_max: i32) -> i32 {
        container_max
    }

    /// `Slot.getMaxStackSize(ItemStack)`.
    fn get_max_stack_size_for_item(&self, container_max: i32, stack: &InventoryItemStack) -> i32 {
        self.get_max_stack_size(container_max)
            .min(stack.get_max_stack_size())
    }

    /// `Slot.isActive()`.
    fn is_active(&self) -> bool {
        true
    }

    /// `Slot.isHighlightable()`.
    fn is_highlightable(&self) -> bool {
        true
    }

    /// `Slot.isFake()`.
    fn is_fake(&self) -> bool {
        false
    }
}

/// Default `SlotPolicy` matching Paper's base `Slot`.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultSlotPolicy;

impl SlotPolicy for DefaultSlotPolicy {}

/// Configurable `SlotPolicy` for slot limits, placement/pickup restrictions,
/// and result/output-style slots.
#[derive(Debug, Clone)]
pub struct CustomSlotPolicy {
    pub max_stack_size: Option<i32>,
    pub allow_place: bool,
    pub allow_pickup: bool,
    pub allowed_items: Option<Vec<ItemId>>,
    pub active: bool,
    pub highlightable: bool,
    pub fake: bool,
}

impl Default for CustomSlotPolicy {
    fn default() -> Self {
        Self {
            max_stack_size: None,
            allow_place: true,
            allow_pickup: true,
            allowed_items: None,
            active: true,
            highlightable: true,
            fake: false,
        }
    }
}

impl SlotPolicy for CustomSlotPolicy {
    fn may_place(&self, stack: &InventoryItemStack) -> bool {
        if !self.allow_place {
            return false;
        }
        match &self.allowed_items {
            Some(allowed) => !stack.is_empty() && allowed.contains(&stack.item()),
            None => true,
        }
    }

    fn may_pickup(&self, _ctx: &ContainerAccessContext) -> bool {
        self.allow_pickup
    }

    fn get_max_stack_size(&self, container_max: i32) -> i32 {
        match self.max_stack_size {
            Some(limit) => limit.min(container_max),
            None => container_max,
        }
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn is_highlightable(&self) -> bool {
        self.highlightable
    }

    fn is_fake(&self) -> bool {
        self.fake
    }
}

/// Paper `net.minecraft.world.inventory.Slot`.
///
/// References its backing container by `container_id` (menu-local container
/// table index) and `slot` (index within that container) rather than holding a
/// shared mutable pointer, matching `OWNERSHIP.md`.
pub struct Slot {
    pub container_id: usize,
    pub slot: usize,
    pub index: usize,
    pub x: i32,
    pub y: i32,
    policy: Box<dyn SlotPolicy>,
}

impl Slot {
    /// Construct a slot with Paper's default `Slot` behavior.
    pub fn new(container_id: usize, slot: usize, x: i32, y: i32) -> Self {
        Self::with_policy(container_id, slot, x, y, Box::new(DefaultSlotPolicy))
    }

    /// Construct a slot with a custom [`SlotPolicy`].
    pub fn with_policy(
        container_id: usize,
        slot: usize,
        x: i32,
        y: i32,
        policy: Box<dyn SlotPolicy>,
    ) -> Self {
        Self {
            container_id,
            slot,
            index: 0,
            x,
            y,
            policy,
        }
    }

    /// `Slot.getContainerSlot()`.
    pub const fn get_container_slot(&self) -> usize {
        self.slot
    }

    /// `Slot.getItem()`.
    pub fn get_item<'a>(&self, container: &'a dyn Container) -> &'a InventoryItemStack {
        container.get_item(self.slot)
    }

    /// `Slot.hasItem()`.
    pub fn has_item(&self, container: &dyn Container) -> bool {
        !self.get_item(container).is_empty()
    }

    /// `Slot.set(ItemStack)`.
    pub fn set(&self, container: &mut dyn Container, stack: InventoryItemStack) {
        container.set_item(self.slot, stack);
    }

    /// `Slot.setByPlayer(ItemStack)`.
    pub fn set_by_player(&self, container: &mut dyn Container, stack: InventoryItemStack) {
        self.set(container, stack);
    }

    /// `Slot.setChanged()`.
    pub fn set_changed(&self, container: &mut dyn Container) {
        container.set_changed();
    }

    /// `Slot.remove(int)`.
    pub fn remove(&self, container: &mut dyn Container, amount: i32) -> InventoryItemStack {
        container.remove_item(self.slot, amount)
    }

    /// `Slot.mayPlace(ItemStack)`.
    pub fn may_place(&self, stack: &InventoryItemStack) -> bool {
        self.policy.may_place(stack)
    }

    /// `Slot.mayPickup(Player)`.
    pub fn may_pickup(&self, ctx: &ContainerAccessContext) -> bool {
        self.policy.may_pickup(ctx)
    }

    /// `Slot.allowModification(Player)` — `mayPickup(player) && mayPlace(getItem())`.
    pub fn allow_modification(
        &self,
        container: &dyn Container,
        ctx: &ContainerAccessContext,
    ) -> bool {
        self.may_pickup(ctx) && self.may_place(self.get_item(container))
    }

    /// `Slot.getMaxStackSize()`.
    pub fn get_max_stack_size(&self, container: &dyn Container) -> i32 {
        self.policy
            .get_max_stack_size(container.get_max_stack_size())
    }

    /// `Slot.getMaxStackSize(ItemStack)`.
    pub fn get_max_stack_size_for_item(
        &self,
        container: &dyn Container,
        stack: &InventoryItemStack,
    ) -> i32 {
        self.policy
            .get_max_stack_size_for_item(container.get_max_stack_size(), stack)
    }

    /// `Slot.isActive()`.
    pub fn is_active(&self) -> bool {
        self.policy.is_active()
    }

    /// `Slot.isHighlightable()`.
    pub fn is_highlightable(&self) -> bool {
        self.policy.is_highlightable()
    }

    /// `Slot.isFake()`.
    pub fn is_fake(&self) -> bool {
        self.policy.is_fake()
    }

    /// `Slot.tryRemove(int, int, Player)`.
    pub fn try_remove(
        &self,
        container: &mut dyn Container,
        amount: i32,
        max_amount: i32,
        ctx: &ContainerAccessContext,
    ) -> Option<InventoryItemStack> {
        if !self.may_pickup(ctx) {
            return None;
        }
        if !self.allow_modification(container, ctx)
            && max_amount < self.get_item(container).get_count()
        {
            return None;
        }
        let clamped = amount.min(max_amount);
        let result = self.remove(container, clamped);
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// `Slot.safeTake(int, int, Player)`.
    pub fn safe_take(
        &self,
        container: &mut dyn Container,
        amount: i32,
        max_amount: i32,
        ctx: &ContainerAccessContext,
    ) -> InventoryItemStack {
        self.try_remove(container, amount, max_amount, ctx)
            .unwrap_or_else(InventoryItemStack::empty)
    }

    /// `Slot.safeInsert(ItemStack)`.
    pub fn safe_insert(
        &self,
        container: &mut dyn Container,
        input_stack: InventoryItemStack,
    ) -> InventoryItemStack {
        let count = input_stack.get_count();
        self.safe_insert_amount(container, input_stack, count)
    }

    /// `Slot.safeInsert(ItemStack, int)`.
    pub fn safe_insert_amount(
        &self,
        container: &mut dyn Container,
        mut input_stack: InventoryItemStack,
        input_amount: i32,
    ) -> InventoryItemStack {
        if input_stack.is_empty() || !self.may_place(&input_stack) {
            return input_stack;
        }
        let slot_stack = self.get_item(container).clone();
        let transferable = input_amount.min(input_stack.get_count()).min(
            self.get_max_stack_size_for_item(container, &input_stack) - slot_stack.get_count(),
        );
        if transferable <= 0 {
            return input_stack;
        }
        if slot_stack.is_empty() {
            let moved = input_stack.split(transferable);
            self.set_by_player(container, moved);
        } else if InventoryItemStack::is_same_item_same_components(&slot_stack, &input_stack) {
            input_stack.shrink(transferable);
            let mut grown = slot_stack;
            grown.grow(transferable);
            self.set_by_player(container, grown);
        }
        input_stack
    }
}

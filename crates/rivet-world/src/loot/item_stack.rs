//! Minimal Paper-faithful item stack and container surfaces for the loot
//! runtime (`net.minecraft.world.item.ItemStack` / `net.minecraft.world.Container`
//! subset needed by `net.minecraft.world.level.storage.loot`).
//!
//! `rivet-world` does not yet carry a full `ItemStack` port; this value type
//! preserves the exact observable fields and arithmetic the loot runtime
//! exercises (item identifier, count, max stack size, enchantments, splitting,
//! and feature-flag gating).

use indexmap::IndexMap;
use rivet_registry::Identifier;

/// Default maximum stack size for standard Minecraft items (`Item.DEFAULT_MAX_STACK_SIZE`).
pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;

/// A loot-produced item stack with Paper-exact count and splitting semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LootItemStack {
    item: Identifier,
    count: i32,
    max_stack_size: i32,
    enchantments: IndexMap<Identifier, i32>,
}

impl LootItemStack {
    /// Creates a stack of count `1` with [`DEFAULT_MAX_STACK_SIZE`].
    #[must_use]
    pub fn new(item: Identifier) -> Self {
        Self {
            item,
            count: 1,
            max_stack_size: DEFAULT_MAX_STACK_SIZE,
            enchantments: IndexMap::new(),
        }
    }

    /// Creates a stack with an explicit count and [`DEFAULT_MAX_STACK_SIZE`].
    #[must_use]
    pub fn with_count(item: Identifier, count: i32) -> Self {
        Self {
            item,
            count,
            max_stack_size: DEFAULT_MAX_STACK_SIZE,
            enchantments: IndexMap::new(),
        }
    }

    /// Creates a stack with an explicit count and max stack size.
    #[must_use]
    pub fn with_count_and_max(item: Identifier, count: i32, max_stack_size: i32) -> Self {
        Self {
            item,
            count,
            max_stack_size,
            enchantments: IndexMap::new(),
        }
    }

    /// Returns the empty stack (`minecraft:air`, count 0).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            item: Identifier::with_default_namespace("air"),
            count: 0,
            max_stack_size: DEFAULT_MAX_STACK_SIZE,
            enchantments: IndexMap::new(),
        }
    }

    /// Adds an enchantment level to this stack (builder-style).
    #[must_use]
    pub fn with_enchantment(mut self, enchantment: Identifier, level: i32) -> Self {
        self.enchantments.insert(enchantment, level);
        self
    }

    /// The item identifier.
    #[must_use]
    pub fn item(&self) -> &Identifier {
        &self.item
    }

    /// `ItemStack.getCount()` — returns `0` when the stack is empty (including `minecraft:air`).
    #[must_use]
    pub fn count(&self) -> i32 {
        if self.is_empty() { 0 } else { self.count }
    }

    /// `ItemStack.setCount(int)`.
    pub fn set_count(&mut self, count: i32) {
        self.count = count;
    }

    /// `ItemStack.getMaxStackSize()`.
    #[must_use]
    pub fn max_stack_size(&self) -> i32 {
        self.max_stack_size
    }

    /// `ItemStack.isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || (self.item.namespace() == "minecraft" && self.item.path() == "air")
    }

    /// Returns the enchantment level for `enchantment`, or `0` if absent
    /// (`EnchantmentHelper.getItemEnchantmentLevel`).
    #[must_use]
    pub fn enchantment_level(&self, enchantment: &Identifier) -> i32 {
        self.enchantments.get(enchantment).copied().unwrap_or(0)
    }

    /// `ItemStack.copyWithCount(int)`.
    #[must_use]
    pub fn copy_with_count(&self, count: i32) -> Self {
        if self.is_empty() {
            return Self::empty();
        }
        let mut copy = self.clone();
        copy.set_count(count);
        copy
    }

    /// `ItemStack.split(int amount)` — removes `min(amount, count)` from `self`
    /// and returns a new stack with that count.
    #[must_use]
    pub fn split(&mut self, amount: i32) -> Self {
        let taken = amount.min(self.count);
        let copy = self.copy_with_count(taken);
        self.count = self.count.wrapping_sub(taken);
        copy
    }
}

/// Minimal `net.minecraft.world.Container` surface for [`super::LootTable::fill`].
pub trait LootContainer {
    /// `Container.getContainerSize()`.
    fn container_size(&self) -> usize;

    /// `Container.getItem(int slot)`.
    fn item(&self, slot: usize) -> &LootItemStack;

    /// `Container.setItem(int slot, ItemStack stack)`.
    fn set_item(&mut self, slot: usize, stack: LootItemStack);
}

/// Fixed-size slot array implementing [`LootContainer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleLootContainer {
    slots: Vec<LootItemStack>,
}

impl SimpleLootContainer {
    /// Creates an empty container with `size` slots.
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            slots: vec![LootItemStack::empty(); size],
        }
    }

    /// Exposes the underlying slot slice.
    #[must_use]
    pub fn slots(&self) -> &[LootItemStack] {
        &self.slots
    }
}

impl LootContainer for SimpleLootContainer {
    fn container_size(&self) -> usize {
        self.slots.len()
    }

    fn item(&self, slot: usize) -> &LootItemStack {
        &self.slots[slot]
    }

    fn set_item(&mut self, slot: usize, stack: LootItemStack) {
        self.slots[slot] = stack;
    }
}

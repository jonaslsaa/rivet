//! Minimal Paper-faithful item stack and hashed stack seam for inventory,
//! container, slot, and menu foundations (issue #460).
//!
//! Full `ItemStack`, `DataComponentMap`, and `DataComponentPatch` ownership
//! lives with issue #431. This module provides the exact Paper `ItemStack`
//! value invariants (`isEmpty`, `split`, `grow`, `shrink`, `limitSize`,
//! `copyWithCount`, `copyAndClear`, `isSameItem`, `isSameItemSameComponents`,
//! `matches`) and `HashedStack` sync seam grounded in `rivet-registry`'s
//! generated `ITEM_BY_NAME` / `ITEM_BY_ID` tables.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use rivet_registry::generated::registries::{ITEM_BY_ID, ITEM_BY_NAME};

/// Default stack size for standard stackable items (`Item.DEFAULT_MAX_STACK_SIZE`).
pub const DEFAULT_ITEM_MAX_STACK_SIZE: i32 = 64;

/// Registry-backed item identifier (`BuiltInRegistries.ITEM` index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(pub u16);

impl ItemId {
    /// `Items.AIR` — index `0` in `BuiltInRegistries.ITEM`.
    pub const AIR: Self = Self(0);

    /// Resolve a namespaced item key (e.g. `"minecraft:stone"`) against the
    /// generated 26.2 `ITEM_BY_NAME` table.
    pub fn from_name(name: &str) -> Option<Self> {
        ITEM_BY_NAME.get(name).copied().map(Self)
    }

    /// Look up the canonical namespaced key in `ITEM_BY_ID`.
    pub fn name(self) -> Option<&'static str> {
        ITEM_BY_ID.get(usize::from(self.0)).copied()
    }

    /// Whether this id is `minecraft:air`.
    pub const fn is_air(self) -> bool {
        self.0 == Self::AIR.0
    }
}

/// Ordered component-patch representation for deterministic component equality
/// and hashing until #431's full `DataComponentPatch` lands.
pub type ComponentMap = BTreeMap<String, String>;

/// Paper `net.minecraft.world.item.ItemStack` value seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItemStack {
    item: ItemId,
    count: i32,
    max_stack_size: i32,
    components: ComponentMap,
}

impl Default for InventoryItemStack {
    fn default() -> Self {
        Self::empty()
    }
}

impl InventoryItemStack {
    /// Construct an empty stack (`ItemStack.EMPTY`).
    pub fn empty() -> Self {
        Self {
            item: ItemId::AIR,
            count: 0,
            max_stack_size: DEFAULT_ITEM_MAX_STACK_SIZE,
            components: ComponentMap::new(),
        }
    }

    /// Construct a stack with the default max stack size (`64`).
    pub fn new(item: ItemId, count: i32) -> Self {
        Self::with_max_stack_size(item, count, DEFAULT_ITEM_MAX_STACK_SIZE)
    }

    /// Construct a stack with an explicit item max stack size (e.g. `16` for
    /// ender pearls or `1` for tools).
    pub fn with_max_stack_size(item: ItemId, count: i32, max_stack_size: i32) -> Self {
        if item.is_air() || count <= 0 {
            return Self::empty();
        }
        Self {
            item,
            count,
            max_stack_size: max_stack_size.max(1),
            components: ComponentMap::new(),
        }
    }

    /// Attach component entries to this stack.
    pub fn with_components(mut self, components: ComponentMap) -> Self {
        if !self.is_empty() {
            self.components = components;
        }
        self
    }

    /// Insert a single component entry on this stack.
    pub fn with_component(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if !self.is_empty() {
            self.components.insert(key.into(), value.into());
        }
        self
    }

    /// `ItemStack.getItem()` — returns `ItemId::AIR` when empty.
    pub fn item(&self) -> ItemId {
        if self.is_empty() {
            ItemId::AIR
        } else {
            self.item
        }
    }

    /// `ItemStack.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.item.is_air() || self.count <= 0
    }

    /// `ItemStack.getCount()`.
    pub fn get_count(&self) -> i32 {
        if self.is_empty() { 0 } else { self.count }
    }

    /// `ItemStack.setCount(int)`.
    pub fn set_count(&mut self, count: i32) {
        if self.item.is_air() || count <= 0 {
            *self = Self::empty();
        } else {
            self.count = count;
        }
    }

    /// `ItemStack.grow(int)` using Java wrapping `i32` addition.
    pub fn grow(&mut self, amount: i32) {
        if self.item.is_air() {
            return;
        }
        self.set_count(self.count.wrapping_add(amount));
    }

    /// `ItemStack.shrink(int)`.
    pub fn shrink(&mut self, amount: i32) {
        self.grow(amount.wrapping_neg());
    }

    /// `ItemStack.limitSize(int)`.
    pub fn limit_size(&mut self, max_stack_size: i32) {
        if !self.is_empty() && self.get_count() > max_stack_size {
            self.set_count(max_stack_size);
        }
    }

    /// `ItemStack.getMaxStackSize()`.
    pub fn get_max_stack_size(&self) -> i32 {
        self.max_stack_size
    }

    /// `ItemStack.isStackable()`.
    pub fn is_stackable(&self) -> bool {
        self.get_max_stack_size() > 1
    }

    /// Read the component map.
    pub fn components(&self) -> &ComponentMap {
        &self.components
    }

    /// `ItemStack.split(int)`.
    pub fn split(&mut self, amount: i32) -> Self {
        let take = amount.min(self.get_count());
        if take <= 0 {
            return Self::empty();
        }
        let copy = self.copy_with_count(take);
        self.shrink(take);
        copy
    }

    /// `ItemStack.copyWithCount(int)`.
    pub fn copy_with_count(&self, count: i32) -> Self {
        if self.is_empty() || count <= 0 {
            return Self::empty();
        }
        let mut copy = self.clone();
        copy.set_count(count);
        copy
    }

    /// `ItemStack.copyAndClear()`.
    pub fn copy_and_clear(&mut self) -> Self {
        if self.is_empty() {
            return Self::empty();
        }
        let copy = self.clone();
        self.set_count(0);
        copy
    }

    /// `ItemStack.isSameItem(ItemStack, ItemStack)`.
    pub fn is_same_item(a: &Self, b: &Self) -> bool {
        a.item() == b.item()
    }

    /// `ItemStack.isSameItemSameComponents(ItemStack, ItemStack)`.
    pub fn is_same_item_same_components(a: &Self, b: &Self) -> bool {
        if !Self::is_same_item(a, b) {
            return false;
        }
        if a.is_empty() && b.is_empty() {
            return true;
        }
        a.components == b.components
    }

    /// `ItemStack.matches(ItemStack, ItemStack)`.
    pub fn matches(a: &Self, b: &Self) -> bool {
        if a.is_empty() && b.is_empty() {
            return true;
        }
        if a.is_empty() || b.is_empty() {
            return false;
        }
        a.get_count() == b.get_count() && a.item() == b.item() && a.components == b.components
    }

    /// Deterministic hash over component entries for `HashedStack` comparisons.
    pub fn component_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.components.hash(&mut hasher);
        hasher.finish()
    }
}

/// Paper `net.minecraft.network.HashedStack` seam used by `RemoteSlot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashedStack {
    /// `HashedStack.EMPTY`.
    Empty,
    /// `HashedStack.ActualItem`.
    ActualItem {
        item: ItemId,
        count: i32,
        component_hash: u64,
    },
}

impl HashedStack {
    /// `HashedStack.create(ItemStack, HashGenerator)`.
    pub fn create(stack: &InventoryItemStack) -> Self {
        if stack.is_empty() {
            Self::Empty
        } else {
            Self::ActualItem {
                item: stack.item(),
                count: stack.get_count(),
                component_hash: stack.component_hash(),
            }
        }
    }

    /// `HashedStack.matches(ItemStack, HashGenerator)`.
    pub fn matches(&self, stack: &InventoryItemStack) -> bool {
        match self {
            Self::Empty => stack.is_empty(),
            Self::ActualItem {
                item,
                count,
                component_hash,
            } => {
                !stack.is_empty()
                    && stack.item() == *item
                    && stack.get_count() == *count
                    && stack.component_hash() == *component_hash
            }
        }
    }
}

//! Port of `net.minecraft.world.item.enchantment.ItemEnchantments`,
//! `ItemEnchantments.Mutable`, `Enchantable`, and `Repairable` (MC 26.2).

use std::collections::BTreeMap;

use rivet_registry::Identifier;

use crate::enchantment::definition::{MAX_LEVEL, NamedHolderSet};

/// `net.minecraft.world.item.enchantment.Enchantable` — item enchantability
/// component. Rejects non-positive values (`<= 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Enchantable {
    value: i32,
}

/// Error returned when constructing an [`Enchantable`] with a non-positive value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Enchantment value must be positive, but was {0}")]
pub struct InvalidEnchantableValue(pub i32);

impl Enchantable {
    /// `new Enchantable(int value)` — validates `value > 0`.
    pub fn new(value: i32) -> Result<Self, InvalidEnchantableValue> {
        if value <= 0 {
            return Err(InvalidEnchantableValue(value));
        }
        Ok(Self { value })
    }

    pub const fn value(self) -> i32 {
        self.value
    }
}

/// `net.minecraft.world.item.enchantment.Repairable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repairable {
    pub items: NamedHolderSet,
}

impl Repairable {
    pub fn new(items: NamedHolderSet) -> Self {
        Self { items }
    }

    /// `Repairable.isValidRepairItem(ItemStack)`.
    pub fn is_valid_repair_item(&self, item_id: &Identifier) -> bool {
        self.items.contains_item(item_id)
    }
}

/// Error returned when constructing [`ItemEnchantments`] with an out-of-range level.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Enchantment {enchantment} has invalid level {level}")]
pub struct InvalidEnchantmentLevel {
    pub enchantment: Identifier,
    pub level: i32,
}

/// `net.minecraft.world.item.enchantment.ItemEnchantments`.
///
/// Preserves deterministic sorted order by registered enchantment identifier,
/// matching Paper's `Object2IntAVLTreeMap<Holder<Enchantment>>(ENCHANTMENT_ORDER)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemEnchantments {
    enchantments: BTreeMap<Identifier, i32>,
}

impl ItemEnchantments {
    /// `ItemEnchantments.EMPTY`.
    pub fn empty() -> Self {
        Self {
            enchantments: BTreeMap::new(),
        }
    }

    /// Constructs a validated `ItemEnchantments` map, enforcing Paper's
    /// `0 <= level <= 255` invariant (levels `<= 0` are omitted by `Mutable`,
    /// while direct construction validates `[0, 255]`).
    pub fn try_from_map(
        enchantments: BTreeMap<Identifier, i32>,
    ) -> Result<Self, InvalidEnchantmentLevel> {
        for (id, &level) in &enchantments {
            if !(0..=MAX_LEVEL).contains(&level) {
                return Err(InvalidEnchantmentLevel {
                    enchantment: id.clone(),
                    level,
                });
            }
        }
        Ok(Self { enchantments })
    }

    /// `ItemEnchantments.getLevel(Holder<Enchantment>)` — returns `0` when absent.
    pub fn get_level(&self, enchantment: &Identifier) -> i32 {
        self.enchantments.get(enchantment).copied().unwrap_or(0)
    }

    /// `ItemEnchantments.keySet()` in deterministic registered-name order.
    pub fn key_set(&self) -> impl Iterator<Item = &Identifier> {
        self.enchantments.keys()
    }

    /// `ItemEnchantments.entrySet()` in deterministic registered-name order.
    pub fn entry_set(&self) -> impl Iterator<Item = (&Identifier, i32)> {
        self.enchantments.iter().map(|(id, &lvl)| (id, lvl))
    }

    /// `ItemEnchantments.size()`.
    pub fn size(&self) -> usize {
        self.enchantments.len()
    }

    /// `ItemEnchantments.isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.enchantments.is_empty()
    }

    /// Returns a [`MutableItemEnchantments`] builder initialized from `self`.
    pub fn to_mutable(&self) -> MutableItemEnchantments {
        MutableItemEnchantments::from_immutable(self)
    }
}

/// `net.minecraft.world.item.enchantment.ItemEnchantments.Mutable`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MutableItemEnchantments {
    enchantments: BTreeMap<Identifier, i32>,
}

impl MutableItemEnchantments {
    /// `new ItemEnchantments.Mutable(ItemEnchantments)`.
    pub fn from_immutable(enchantments: &ItemEnchantments) -> Self {
        Self {
            enchantments: enchantments.enchantments.clone(),
        }
    }

    /// `Mutable.set(Holder<Enchantment>, int level)` — removes the entry if
    /// `level <= 0`, otherwise stores `Math.min(level, 255)`.
    pub fn set(&mut self, enchantment: Identifier, level: i32) {
        if level <= 0 {
            self.enchantments.remove(&enchantment);
        } else {
            self.enchantments.insert(enchantment, level.min(MAX_LEVEL));
        }
    }

    /// `Mutable.upgrade(Holder<Enchantment>, int level)` — if `level > 0`,
    /// stores `max(existing, min(level, 255))`.
    pub fn upgrade(&mut self, enchantment: Identifier, level: i32) {
        if level > 0 {
            let clamped = level.min(MAX_LEVEL);
            self.enchantments
                .entry(enchantment)
                .and_modify(|existing| *existing = (*existing).max(clamped))
                .or_insert(clamped);
        }
    }

    /// `Mutable.removeIf(Predicate<Holder<Enchantment>>)`.
    pub fn remove_if(&mut self, mut predicate: impl FnMut(&Identifier) -> bool) {
        self.enchantments.retain(|id, _| !predicate(id));
    }

    /// `Mutable.getLevel(Holder<Enchantment>)`.
    pub fn get_level(&self, enchantment: &Identifier) -> i32 {
        self.enchantments.get(enchantment).copied().unwrap_or(0)
    }

    /// `Mutable.keySet()`.
    pub fn key_set(&self) -> impl Iterator<Item = &Identifier> {
        self.enchantments.keys()
    }

    /// `Mutable.toImmutable()`.
    pub fn to_immutable(self) -> ItemEnchantments {
        ItemEnchantments {
            enchantments: self.enchantments,
        }
    }
}

/// Lightweight item representation for item-facing enchantment operations
/// (cost calculation, offer selection, eligibility checks, mutation, and
/// modifier/effect iteration) before a full `ItemStack` data-component store
/// exists in `rivet-world`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantableItemStack {
    /// Registered item identifier (e.g. `minecraft:diamond_sword`, `minecraft:book`,
    /// `minecraft:enchanted_book`).
    pub item_id: Identifier,
    /// Item stack count (`0` means empty).
    pub count: i32,
    /// `DataComponents.ENCHANTABLE` (if the item is enchantable).
    pub enchantable: Option<Enchantable>,
    /// `DataComponents.ENCHANTMENTS` (active enchantments on tools/weapons/armor).
    pub enchantments: Option<ItemEnchantments>,
    /// `DataComponents.STORED_ENCHANTMENTS` (used when `item_id == minecraft:enchanted_book`).
    pub stored_enchantments: Option<ItemEnchantments>,
    /// `DataComponents.REPAIRABLE`.
    pub repairable: Option<Repairable>,
}

impl EnchantableItemStack {
    pub fn new(item_id: Identifier, enchantable: Option<Enchantable>) -> Self {
        Self {
            item_id,
            count: 1,
            enchantable,
            enchantments: Some(ItemEnchantments::empty()),
            stored_enchantments: None,
            repairable: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item_id.to_string() == "minecraft:air"
    }

    pub fn is_book(&self) -> bool {
        self.item_id.namespace() == "minecraft" && self.item_id.path() == "book"
    }

    pub fn is_enchanted_book(&self) -> bool {
        self.item_id.namespace() == "minecraft" && self.item_id.path() == "enchanted_book"
    }

    /// Returns active enchantments (`DataComponents.ENCHANTMENTS`), or empty if absent.
    pub fn active_enchantments(&self) -> Option<&ItemEnchantments> {
        self.enchantments.as_ref()
    }

    /// Returns the component used by `EnchantmentHelper.getComponentType(ItemStack)`:
    /// `STORED_ENCHANTMENTS` for `minecraft:enchanted_book`, `ENCHANTMENTS` otherwise.
    pub fn component_enchantments(&self) -> Option<&ItemEnchantments> {
        if self.is_enchanted_book() {
            self.stored_enchantments.as_ref()
        } else {
            self.enchantments.as_ref()
        }
    }

    /// Sets the component selected by `EnchantmentHelper.getComponentType(ItemStack)`.
    pub fn set_component_enchantments(&mut self, value: ItemEnchantments) {
        if self.is_enchanted_book() {
            self.stored_enchantments = Some(value);
        } else {
            self.enchantments = Some(value);
        }
    }
}

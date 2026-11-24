//! Port of `net.minecraft.world.entity.EntityEquipment` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

use crate::world::entity::equipment_slot::EquipmentSlot;

/// Narrow item-stack representation for `EntityEquipment` lifecycle hooks.
///
/// Mirrors the observable fields needed by `EntityEquipment` and `LivingEntity`
/// equipment change detection (`isEmpty`, `isBroken`, `matches`, `split`) without
/// taking ownership of `net.minecraft.world.item.ItemStack`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EquipmentItem {
    pub item_id: Option<String>,
    pub count: i32,
    pub broken: bool,
}

impl EquipmentItem {
    /// Empty stack (`ItemStack.EMPTY`).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            item_id: None,
            count: 0,
            broken: false,
        }
    }

    /// Creates a non-empty equipment item stack.
    #[must_use]
    pub fn new(item_id: impl Into<String>, count: i32) -> Self {
        if count <= 0 {
            return Self::empty();
        }
        Self {
            item_id: Some(item_id.into()),
            count,
            broken: false,
        }
    }

    /// Marks the item as broken (prevents attribute modifier application in
    /// `LivingEntity#collectEquipmentChanges`).
    #[must_use]
    pub fn with_broken(mut self, broken: bool) -> Self {
        self.broken = broken;
        self
    }

    /// Paper `ItemStack#isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item_id.is_none() || self.count <= 0
    }

    /// Paper `ItemStack#isBroken()`.
    #[must_use]
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// Paper `ItemStack.matches(a, b)`.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        if self.is_empty() && other.is_empty() {
            return true;
        }
        self.item_id == other.item_id && self.count == other.count && self.broken == other.broken
    }

    /// Paper `ItemStack.isSameItemSameComponents(a, b)` (count-insensitive).
    ///
    /// An empty stack's item is `minecraft:air`, so two empty stacks compare equal and an
    /// empty stack never matches a non-empty one. `broken` stands in for the damage
    /// components that make a stack broken.
    #[must_use]
    pub fn is_same_item_same_components(&self, other: &Self) -> bool {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => true,
            (false, false) => self.item_id == other.item_id && self.broken == other.broken,
            _ => false,
        }
    }

    /// Paper `EquipmentSlot#limit(ItemStack)` -> `countLimit > 0 ? toEquip.split(countLimit) : toEquip`.
    pub fn limit_for_slot(&mut self, slot: EquipmentSlot) -> Self {
        let limit = slot.count_limit();
        if limit > 0 && !self.is_empty() {
            let taken = self.count.min(limit);
            self.count = self.count.wrapping_sub(taken);
            let taken_item = Self {
                item_id: self.item_id.clone(),
                count: taken,
                broken: self.broken,
            };
            if self.count <= 0 {
                *self = Self::empty();
            }
            taken_item
        } else {
            std::mem::replace(self, Self::empty())
        }
    }
}

/// Port of `net.minecraft.world.entity.EntityEquipment`.
///
/// Uses a fixed 8-slot array indexed by [`EquipmentSlot`] ordinal to preserve
/// exact `EnumMap<EquipmentSlot, ItemStack>` iteration order (`EquipmentSlot.VALUES`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityEquipment {
    items: [Option<EquipmentItem>; 8],
}

impl Default for EntityEquipment {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityEquipment {
    /// Paper `new EntityEquipment()`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            items: [None, None, None, None, None, None, None, None],
        }
    }

    /// Paper `EntityEquipment#set(EquipmentSlot, ItemStack)`.
    ///
    /// Returns the previous stack (or `EquipmentItem::empty()` if absent).
    pub fn set(&mut self, slot: EquipmentSlot, item: EquipmentItem) -> EquipmentItem {
        self.items[slot as usize]
            .replace(item)
            .unwrap_or_else(EquipmentItem::empty)
    }

    /// Paper `EntityEquipment#get(EquipmentSlot)`.
    #[must_use]
    pub fn get(&self, slot: EquipmentSlot) -> EquipmentItem {
        self.items[slot as usize]
            .clone()
            .unwrap_or_else(EquipmentItem::empty)
    }

    /// Paper `EntityEquipment#has(EquipmentSlot)` (Paper EntityDeathEvent patch).
    #[must_use]
    pub fn has(&self, slot: EquipmentSlot) -> bool {
        self.items[slot as usize].is_some()
    }

    /// Paper `EntityEquipment#isEmpty()`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items
            .iter()
            .all(|slot| slot.as_ref().is_none_or(EquipmentItem::is_empty))
    }

    /// Paper `EntityEquipment#setAll(EntityEquipment)`.
    pub fn set_all(&mut self, other: &Self) {
        self.items = other.items.clone();
    }

    /// Paper `EntityEquipment#clear()` — replaces every present entry with `ItemStack.EMPTY`.
    pub fn clear(&mut self) {
        for slot in &mut self.items {
            if slot.is_some() {
                *slot = Some(EquipmentItem::empty());
            }
        }
    }

    /// Paper `EntityEquipment#dropAll(LivingEntity)` — returns non-empty items in
    /// `EquipmentSlot.VALUES` order and clears present slots.
    pub fn drop_all(&mut self) -> Vec<(EquipmentSlot, EquipmentItem)> {
        let mut dropped = Vec::new();
        for slot in EquipmentSlot::VALUES {
            if let Some(item) = &self.items[slot as usize]
                && !item.is_empty()
            {
                dropped.push((slot, item.clone()));
            }
        }
        self.clear();
        dropped
    }

    /// Returns all non-empty `(slot, item)` pairs in `EquipmentSlot.VALUES` order
    /// for `EntityEquipment#tick(Entity)`.
    #[must_use]
    pub fn non_empty_slots(&self) -> Vec<(EquipmentSlot, EquipmentItem)> {
        let mut out = Vec::new();
        for slot in EquipmentSlot::VALUES {
            if let Some(item) = &self.items[slot as usize]
                && !item.is_empty()
            {
                out.push((slot, item.clone()));
            }
        }
        out
    }
}

/// Recorded equipment change from `LivingEntity#detectEquipmentUpdates`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentSlotChange {
    pub slot: EquipmentSlot,
    pub old_item: EquipmentItem,
    pub new_item: EquipmentItem,
}

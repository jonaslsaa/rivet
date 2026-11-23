//! Port of `net.minecraft.world.entity.EquipmentSlot`, `EntityEquipment`, and `HumanoidArm`
//! (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

/// Port of `net.minecraft.world.entity.HumanoidArm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HumanoidArm {
    Left,
    #[default]
    Right,
}

impl HumanoidArm {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Port of `net.minecraft.world.entity.EquipmentSlot.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlotType {
    Hand,
    HumanoidArmor,
    AnimalArmor,
    Saddle,
}

/// Port of `net.minecraft.world.entity.EquipmentSlot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(usize)]
pub enum EquipmentSlot {
    MainHand = 0,
    OffHand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
    Body = 6,
    Saddle = 7,
}

impl EquipmentSlot {
    /// Paper `EquipmentSlot.NO_COUNT_LIMIT`.
    pub const NO_COUNT_LIMIT: i32 = 0;

    /// Paper `EquipmentSlot.VALUES` in exact enum declaration order.
    pub const VALUES: [EquipmentSlot; 8] = [
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Feet,
        EquipmentSlot::Legs,
        EquipmentSlot::Chest,
        EquipmentSlot::Head,
        EquipmentSlot::Body,
        EquipmentSlot::Saddle,
    ];

    /// Paper `EquipmentSlot#getType()`.
    #[must_use]
    pub const fn slot_type(self) -> EquipmentSlotType {
        match self {
            Self::MainHand | Self::OffHand => EquipmentSlotType::Hand,
            Self::Feet | Self::Legs | Self::Chest | Self::Head => EquipmentSlotType::HumanoidArmor,
            Self::Body => EquipmentSlotType::AnimalArmor,
            Self::Saddle => EquipmentSlotType::Saddle,
        }
    }

    /// Paper `EquipmentSlot#getIndex()`.
    #[must_use]
    pub const fn index(self) -> i32 {
        match self {
            Self::MainHand | Self::Feet | Self::Body | Self::Saddle => 0,
            Self::OffHand | Self::Legs => 1,
            Self::Chest => 2,
            Self::Head => 3,
        }
    }

    /// Paper `EquipmentSlot#getIndex(int base)`.
    #[must_use]
    pub const fn index_with_base(self, base: i32) -> i32 {
        base.wrapping_add(self.index())
    }

    /// Paper `EquipmentSlot.countLimit`.
    #[must_use]
    pub const fn count_limit(self) -> i32 {
        match self {
            Self::MainHand | Self::OffHand => Self::NO_COUNT_LIMIT,
            Self::Feet | Self::Legs | Self::Chest | Self::Head | Self::Body | Self::Saddle => 1,
        }
    }

    /// Paper `EquipmentSlot#getId()`.
    ///
    /// Note that `id` differs from enum ordinal:
    /// `MainHand=0, Feet=1, Legs=2, Chest=3, Head=4, OffHand=5, Body=6, Saddle=7`.
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::MainHand => 0,
            Self::Feet => 1,
            Self::Legs => 2,
            Self::Chest => 3,
            Self::Head => 4,
            Self::OffHand => 5,
            Self::Body => 6,
            Self::Saddle => 7,
        }
    }

    /// Paper `EquipmentSlot#getFilterBit(int offset)`.
    #[must_use]
    pub const fn filter_bit(self, offset: i32) -> i32 {
        self.id().wrapping_add(offset)
    }

    /// Paper `EquipmentSlot#getName()` / `getSerializedName()`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MainHand => "mainhand",
            Self::OffHand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    /// Paper `EquipmentSlot#isArmor()`.
    #[must_use]
    pub const fn is_armor(self) -> bool {
        matches!(
            self.slot_type(),
            EquipmentSlotType::HumanoidArmor | EquipmentSlotType::AnimalArmor
        )
    }

    /// Paper `EquipmentSlot#canIncreaseExperience()`.
    #[must_use]
    pub const fn can_increase_experience(self) -> bool {
        !matches!(self.slot_type(), EquipmentSlotType::Saddle)
    }

    /// Paper `EquipmentSlot.BY_ID` (`OutOfBoundsStrategy.ZERO` -> `MainHand`).
    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            0 => Self::MainHand,
            1 => Self::Feet,
            2 => Self::Legs,
            3 => Self::Chest,
            4 => Self::Head,
            5 => Self::OffHand,
            6 => Self::Body,
            7 => Self::Saddle,
            _ => Self::MainHand,
        }
    }

    /// Paper `EquipmentSlot.byName(String)`.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "mainhand" => Some(Self::MainHand),
            "offhand" => Some(Self::OffHand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }
}

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

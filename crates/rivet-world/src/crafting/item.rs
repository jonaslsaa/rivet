//! Lightweight item & item-stack value types for the crafting/recipe runtime.
//!
//! Backed directly by `rivet_registry::generated::registries::{ITEM_BY_NAME, ITEM_BY_ID}`
//! until the full `net.minecraft.world.item` crate/module lands.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rivet_registry::Identifier;
use rivet_registry::generated::registries::{ITEM_BY_ID, ITEM_BY_NAME};

/// Compact item registry ID backed by the generated `minecraft:item` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u16);

impl ItemId {
    /// `minecraft:air` (ID 0 in vanilla 26.2).
    pub const AIR: Self = Self(0);

    /// Look up an item by its full `minecraft:<path>` or short `<path>` name.
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(&id) = ITEM_BY_NAME.get(name) {
            return Some(Self(id));
        }
        if !name.contains(':') {
            let qualified = format!("minecraft:{name}");
            return ITEM_BY_NAME.get(qualified.as_str()).copied().map(Self);
        }
        None
    }

    /// Look up an item by its `Identifier`.
    pub fn from_identifier(id: &Identifier) -> Option<Self> {
        Self::from_name(&id.to_string())
    }

    /// Look up an item by its raw registry ID.
    pub fn from_raw(raw: u16) -> Option<Self> {
        if (raw as usize) < ITEM_BY_ID.len() {
            Some(Self(raw))
        } else {
            None
        }
    }

    /// Canonical `minecraft:<path>` registry name for this item.
    pub fn name(self) -> &'static str {
        ITEM_BY_ID
            .get(self.0 as usize)
            .copied()
            .unwrap_or("minecraft:air")
    }

    /// Whether this item is `minecraft:air`.
    pub const fn is_air(self) -> bool {
        self.0 == Self::AIR.0
    }

    /// `Item.getDefaultMaxStackSize()` for standard vanilla stack-size buckets
    /// (1 for unstackable items, 16 for 16-stack items, 64 default).
    pub fn default_max_stack_size(self) -> i32 {
        let path = self
            .name()
            .strip_prefix("minecraft:")
            .unwrap_or(self.name());
        if path == "air" {
            return 0;
        }
        if matches!(
            path,
            "ender_pearl"
                | "egg"
                | "blue_egg"
                | "brown_egg"
                | "snowball"
                | "bucket"
                | "honey_bottle"
                | "armor_stand"
                | "written_book"
        ) || path.ends_with("_sign")
            || path.ends_with("_banner")
        {
            return 16;
        }
        if matches!(
            path,
            "shulker_box"
                | "bow"
                | "crossbow"
                | "trident"
                | "mace"
                | "shield"
                | "elytra"
                | "fishing_rod"
                | "carrot_on_a_stick"
                | "warped_fungus_on_a_stick"
                | "flint_and_steel"
                | "shears"
                | "brush"
                | "spyglass"
                | "goat_horn"
                | "bundle"
                | "totem_of_undying"
                | "enchanted_book"
                | "writable_book"
                | "knowledge_book"
                | "potion"
                | "splash_potion"
                | "lingering_potion"
                | "mushroom_stew"
                | "rabbit_stew"
                | "beetroot_soup"
                | "suspicious_stew"
                | "cake"
                | "saddle"
                | "minecart"
                | "turtle_helmet"
                | "wolf_armor"
                | "debug_stick"
        ) || path.ends_with("_shulker_box")
            || path.ends_with("_sword")
            || path.ends_with("_pickaxe")
            || path.ends_with("_axe")
            || path.ends_with("_shovel")
            || path.ends_with("_hoe")
            || path.ends_with("_helmet")
            || path.ends_with("_chestplate")
            || path.ends_with("_leggings")
            || path.ends_with("_boots")
            || path.ends_with("_horse_armor")
            || path.ends_with("_nautilus_armor")
            || path.ends_with("_harness")
            || path.ends_with("_bed")
            || path.ends_with("_boat")
            || path.ends_with("_chest_boat")
            || path.ends_with("_raft")
            || path.ends_with("_chest_raft")
            || path.ends_with("_minecart")
            || path.ends_with("_bucket")
            || path.ends_with("_bundle")
            || path.starts_with("music_disc_")
            || path.ends_with("_banner_pattern")
            || path.ends_with("_spear")
        {
            return 1;
        }
        64
    }

    /// `Item.getCraftingRemainder()` for standard vanilla container items.
    pub fn crafting_remainder(self) -> Option<ItemStackTemplate> {
        let remainder_name = match self.name() {
            "minecraft:water_bucket"
            | "minecraft:lava_bucket"
            | "minecraft:milk_bucket"
            | "minecraft:powder_snow_bucket" => "minecraft:bucket",
            "minecraft:honey_bottle" | "minecraft:dragon_breath" => "minecraft:glass_bottle",
            _ => return None,
        };
        Self::from_name(remainder_name).map(|id| ItemStackTemplate::new(id, 1))
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// `net.minecraft.world.item.ItemStack` value representation for crafting,
/// cooking, stonecutting, smithing, and recipe-book matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemStack {
    item: ItemId,
    count: i32,
    components: BTreeMap<Identifier, String>,
}

impl ItemStack {
    /// `ItemStack.EMPTY`.
    pub const fn empty() -> Self {
        Self {
            item: ItemId::AIR,
            count: 0,
            components: BTreeMap::new(),
        }
    }

    /// Create a stack of `count` items. Non-positive count or air yields an
    /// empty stack (matching `ItemStack.isEmpty()`).
    pub fn new(item: ItemId, count: i32) -> Self {
        if item.is_air() || count <= 0 {
            Self::empty()
        } else {
            Self {
                item,
                count,
                components: BTreeMap::new(),
            }
        }
    }

    /// Create a stack with explicit data components (used for exact-choice
    /// ingredients and smithing component preservation).
    pub fn with_components(
        item: ItemId,
        count: i32,
        components: BTreeMap<Identifier, String>,
    ) -> Self {
        if item.is_air() || count <= 0 {
            Self::empty()
        } else {
            Self {
                item,
                count,
                components,
            }
        }
    }

    pub const fn item(&self) -> ItemId {
        self.item
    }

    pub const fn count(&self) -> i32 {
        self.count
    }

    pub fn components(&self) -> &BTreeMap<Identifier, String> {
        &self.components
    }

    pub fn components_mut(&mut self) -> &mut BTreeMap<Identifier, String> {
        &mut self.components
    }

    /// `ItemStack.getMaxStackSize()` — uses `minecraft:max_stack_size`
    /// component override when present, otherwise the item's default limit.
    pub fn max_stack_size(&self) -> i32 {
        if let Some(raw) = self
            .components
            .get(&Identifier::with_default_namespace("max_stack_size"))
            && let Ok(v) = raw.trim().parse::<i32>()
            && (1..=99).contains(&v)
        {
            return v;
        }
        self.item.default_max_stack_size()
    }

    /// `ItemStack.isEmpty()` — true iff item is air or count <= 0.
    pub const fn is_empty(&self) -> bool {
        self.item.is_air() || self.count <= 0
    }

    /// `ItemStack.isSameItem(a, b)`.
    pub const fn is_same_item(&self, other: &Self) -> bool {
        self.item.0 == other.item.0
    }

    /// `ItemStack.isSameItemSameComponents(a, b)`.
    pub fn is_same_item_same_components(&self, other: &Self) -> bool {
        if !self.is_same_item(other) {
            return false;
        }
        if self.is_empty() && other.is_empty() {
            return true;
        }
        self.components == other.components
    }

    /// `ItemStack.copyWithCount(int)`.
    pub fn copy_with_count(&self, count: i32) -> Self {
        if self.is_empty() || count <= 0 {
            Self::empty()
        } else {
            Self {
                item: self.item,
                count,
                components: self.components.clone(),
            }
        }
    }
}

impl Default for ItemStack {
    fn default() -> Self {
        Self::empty()
    }
}

/// `net.minecraft.world.item.ItemStackTemplate` — immutable recipe result
/// template that produces fresh `ItemStack` instances on `create()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemStackTemplate {
    pub item: ItemId,
    pub count: i32,
    pub components: BTreeMap<Identifier, String>,
    /// Component removals from a `DataComponentPatch` (`!<id>` entries in JSON).
    pub removed_components: BTreeSet<Identifier>,
}

impl ItemStackTemplate {
    pub fn new(item: ItemId, count: i32) -> Self {
        Self {
            item,
            count,
            components: BTreeMap::new(),
            removed_components: BTreeSet::new(),
        }
    }

    pub fn with_components(
        item: ItemId,
        count: i32,
        components: BTreeMap<Identifier, String>,
    ) -> Self {
        Self {
            item,
            count,
            components,
            removed_components: BTreeSet::new(),
        }
    }

    pub fn with_component_patch(
        item: ItemId,
        count: i32,
        components: BTreeMap<Identifier, String>,
        removed_components: BTreeSet<Identifier>,
    ) -> Self {
        Self {
            item,
            count,
            components,
            removed_components,
        }
    }

    fn validate(result: ItemStack) -> ItemStack {
        if result.is_empty() || result.count() > result.max_stack_size() {
            ItemStack::empty()
        } else {
            result
        }
    }

    /// `ItemStackTemplate.create()`.
    pub fn create(&self) -> ItemStack {
        Self::validate(ItemStack::with_components(
            self.item,
            self.count,
            self.components.clone(),
        ))
    }

    /// `TransmuteRecipe.createWithOriginalComponents(result, input)` — creates
    /// a stack of `self.item` with `self.count`, applying this template's
    /// component patch (removals and overrides) on top of `original`'s
    /// components and validating against `max_stack_size`.
    pub fn create_with_original_components(&self, original: &ItemStack) -> ItemStack {
        self.create_with_original_components_extra(original, 0)
    }

    /// `TransmuteRecipe.createWithOriginalComponents(result, input, extraCount)`
    /// — `result.apply(result.count() + extraCount, input.getComponentsPatch())`.
    pub fn create_with_original_components_extra(
        &self,
        original: &ItemStack,
        extra_count: i32,
    ) -> ItemStack {
        let mut merged = original.components().clone();
        for removed in &self.removed_components {
            merged.remove(removed);
        }
        for (k, v) in &self.components {
            merged.insert(k.clone(), v.clone());
        }
        let count = self.count.wrapping_add(extra_count);
        Self::validate(ItemStack::with_components(self.item, count, merged))
    }
}

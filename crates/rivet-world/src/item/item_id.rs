//! Port of `net.minecraft.world.item.Item` identity, `ItemLike`, and the
//! `Holder<Item>` registry seam over the generated `minecraft:item` tables.
//!
//! All 1,537 vanilla item identities are sourced directly from
//! `rivet_registry::generated::registries::{ITEM_BY_ID, ITEM_BY_NAME, ITEM_DEFAULT}`.
//! No hand-maintained item list is duplicated here.

use std::fmt;
use std::sync::Arc;

use rivet_registry::Identifier;
use rivet_registry::generated::registries::{ITEM_BY_ID, ITEM_BY_NAME, ITEM_DEFAULT};
use rivet_registry::identifier::identifier_codec;
use rivet_serialization::{Codec, DataResult, DynamicOps, codec};

use crate::item::component_map::{DataComponentGetter, DataComponentMap, DataComponentMapBuilder};
use crate::item::component_type::{DataComponentTypeId, DataComponentValue, Rarity};

/// `Item.DEFAULT_MAX_STACK_SIZE` (64).
pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;

/// `Item.ABSOLUTE_MAX_STACK_SIZE` (99).
pub const ABSOLUTE_MAX_STACK_SIZE: i32 = 99;

/// `Item.ABSOLUTE_MAX_SWING_DURATION` (200).
pub const ABSOLUTE_MAX_SWING_DURATION: i32 = 200;

/// `Item.MAX_BAR_WIDTH` (13).
pub const MAX_BAR_WIDTH: i32 = 13;

/// Compact identity handle for a registered `minecraft:item` entry.
///
/// Equality and hashing are strictly by registry identity (`u16` ID), matching
/// Paper's `Item` reference identity semantics (`a.is(b)` / `item == Items.AIR`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ItemId(pub u16);

impl ItemId {
    /// `Items.AIR` — registry id 0 (`minecraft:air`).
    pub const AIR: ItemId = ItemId(0);

    /// Total number of registered vanilla items in the generated table.
    pub const COUNT: usize = ITEM_BY_ID.len();

    /// Constructs an `ItemId` from a raw registry id if it is within the
    /// generated `minecraft:item` table.
    #[inline]
    pub const fn from_id(id: u16) -> Option<Self> {
        if (id as usize) < ITEM_BY_ID.len() {
            Some(Self(id))
        } else {
            None
        }
    }

    /// Constructs an `ItemId` without bounds checking.
    #[inline]
    pub const fn from_id_unchecked(id: u16) -> Self {
        Self(id)
    }

    /// Looks up an item by its namespaced string (e.g. `"minecraft:stone"`) or
    /// default-namespace path (e.g. `"stone"`), matching `Identifier.parse`.
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(&id) = ITEM_BY_NAME.get(name) {
            return Some(Self(id));
        }
        let parsed = Identifier::try_parse(name)?;
        Self::from_identifier(&parsed)
    }

    /// Looks up an item by `Identifier`.
    pub fn from_identifier(id: &Identifier) -> Option<Self> {
        let full = id.to_string();
        ITEM_BY_NAME.get(full.as_str()).copied().map(Self)
    }

    /// Returns the registered namespaced string (e.g. `"minecraft:stone"`),
    /// falling back to `ITEM_DEFAULT` (`"minecraft:air"`) if out of range.
    #[inline]
    pub fn name(self) -> &'static str {
        ITEM_BY_ID
            .get(self.0 as usize)
            .copied()
            .unwrap_or(ITEM_DEFAULT)
    }

    /// Returns the `Identifier` for this item.
    pub fn identifier(self) -> Identifier {
        Identifier::parse(self.name())
    }

    /// Returns the raw numeric registry ID.
    #[inline]
    pub const fn id(self) -> u16 {
        self.0
    }

    /// Returns `true` if this item is `minecraft:air`.
    #[inline]
    pub const fn is_air(self) -> bool {
        self.0 == Self::AIR.0
    }

    /// Iterates over every generated vanilla item in registry ID order.
    pub fn iter() -> impl ExactSizeIterator<Item = ItemId> {
        (0..ITEM_BY_ID.len() as u16).map(ItemId)
    }

    /// Returns the default prototype `DataComponentMap` for this item.
    ///
    /// Mirrors `Item.components()` (`builtInRegistryHolder.components()`).
    pub fn default_components(self) -> DataComponentMap {
        default_components_for(self)
    }

    /// Returns the default max stack size from this item's prototype components.
    ///
    /// Mirrors `Item.getDefaultMaxStackSize()`.
    pub fn default_max_stack_size(self) -> i32 {
        self.default_components()
            .get_i32(DataComponentTypeId::MAX_STACK_SIZE)
            .unwrap_or(1)
    }

    /// Returns a `Holder<Item>`-equivalent handle for this item.
    pub fn holder(self) -> ItemHolder {
        ItemHolder::new(self)
    }

    /// Java-compatible identity hash code for use in `ItemStackLinkedSet`.
    ///
    /// Stable per-item hash derived from the registry ID (matching Paper's
    /// identity-based `Item.hashCode()` contract where equal items always have
    /// equal hash codes and distinct items have distinct hash codes).
    #[inline]
    pub const fn java_hash_code(self) -> i32 {
        self.0 as i32
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Port of `net.minecraft.world.level.ItemLike`.
pub trait ItemLike {
    fn as_item(&self) -> ItemId;
}

impl ItemLike for ItemId {
    #[inline]
    fn as_item(&self) -> ItemId {
        *self
    }
}

/// Port of `Holder<Item>` for `ItemStack` and `ItemInstance` identity seams.
///
/// Wraps an `ItemId` together with its prototype `DataComponentMap` so
/// downstream systems (inventory, entities, loot, persistence) can inspect
/// item identity and prototype defaults without owning global mutable state.
#[derive(Clone, Debug)]
pub struct ItemHolder {
    item: ItemId,
    components: DataComponentMap,
}

impl ItemHolder {
    /// Creates a holder for `item` using its default prototype components.
    pub fn new(item: ItemId) -> Self {
        Self {
            item,
            components: default_components_for(item),
        }
    }

    /// Creates a holder for `item` with an explicit prototype component map
    /// (for tests or custom-configured item prototypes).
    pub fn with_components(item: ItemId, components: DataComponentMap) -> Self {
        Self { item, components }
    }

    /// Returns the underlying `ItemId` (`Holder.value()`).
    #[inline]
    pub const fn value(&self) -> ItemId {
        self.item
    }

    /// Returns the registered namespaced name (`Holder.getRegisteredName()`).
    #[inline]
    pub fn registered_name(&self) -> &'static str {
        self.item.name()
    }

    /// Returns the prototype `DataComponentMap` bound to this holder.
    #[inline]
    pub fn components(&self) -> &DataComponentMap {
        &self.components
    }

    /// Returns `true` if this holder refers to `item`.
    #[inline]
    pub const fn is(&self, item: ItemId) -> bool {
        self.item.0 == item.0
    }
}

impl PartialEq for ItemHolder {
    fn eq(&self, other: &Self) -> bool {
        self.item == other.item
    }
}

impl Eq for ItemHolder {}

impl std::hash::Hash for ItemHolder {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.item.hash(state);
    }
}

impl ItemLike for ItemHolder {
    #[inline]
    fn as_item(&self) -> ItemId {
        self.item
    }
}

impl fmt::Display for ItemHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Reference{{{}}}", self.item.name())
    }
}

/// Port of `Item.Properties` for building validated prototype component maps.
#[derive(Clone, Debug)]
pub struct ItemProperties {
    builder: DataComponentMapBuilder,
}

impl Default for ItemProperties {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemProperties {
    /// Starts from `DataComponents.COMMON_ITEM_COMPONENTS` (max_stack_size = 64,
    /// repair_cost = 0, rarity = Common).
    pub fn new() -> Self {
        let mut builder = DataComponentMapBuilder::new();
        builder.add_all(&common_item_components());
        Self { builder }
    }

    /// `Item.Properties.stacksTo(int)` — sets `MAX_STACK_SIZE`.
    pub fn stacks_to(mut self, max_stack_size: i32) -> Self {
        self.builder.set(
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentValue::Int(max_stack_size),
        );
        self
    }

    /// `Item.Properties.durability(int)` — sets `MAX_DAMAGE`, `MAX_STACK_SIZE = 1`,
    /// and `DAMAGE = 0`.
    pub fn durability(mut self, max_damage: i32) -> Self {
        self.builder.set(
            DataComponentTypeId::MAX_DAMAGE,
            DataComponentValue::Int(max_damage),
        );
        self.builder.set(
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentValue::Int(1),
        );
        self.builder
            .set(DataComponentTypeId::DAMAGE, DataComponentValue::Int(0));
        self
    }

    /// `Item.Properties.rarity(Rarity)` — sets `RARITY`.
    pub fn rarity(mut self, rarity: Rarity) -> Self {
        self.builder.set(
            DataComponentTypeId::RARITY,
            DataComponentValue::Rarity(rarity),
        );
        self
    }

    /// `Item.Properties.component(DataComponentType, value)`.
    pub fn component(
        mut self,
        component_type: DataComponentTypeId,
        value: DataComponentValue,
    ) -> Self {
        self.builder.set(component_type, value);
        self
    }

    /// Finalizes the prototype `DataComponentMap` for `item`, applying
    /// `ITEM_MODEL` and Paper's `Item.Properties.finalizeInitializer` validator:
    /// `"Item cannot have both durability and be stackable"`.
    pub fn build_for(mut self, item: ItemId) -> Result<DataComponentMap, String> {
        self.builder.set(
            DataComponentTypeId::ITEM_MODEL,
            DataComponentValue::Identifier(item.identifier()),
        );
        self.builder.add_validator(|map| {
            if map.has(DataComponentTypeId::DAMAGE)
                && map
                    .get_i32(DataComponentTypeId::MAX_STACK_SIZE)
                    .unwrap_or(1)
                    > 1
            {
                Err("Item cannot have both durability and be stackable".to_string())
            } else {
                Ok(())
            }
        });
        self.builder.build()
    }
}

/// Port of `DataComponents.COMMON_ITEM_COMPONENTS` (foundation subset).
///
/// Includes `MAX_STACK_SIZE = 64`, `REPAIR_COST = 0`, `RARITY = Common`.
pub fn common_item_components() -> DataComponentMap {
    let mut b = DataComponentMapBuilder::new();
    b.set(
        DataComponentTypeId::MAX_STACK_SIZE,
        DataComponentValue::Int(DEFAULT_MAX_STACK_SIZE),
    );
    b.set(DataComponentTypeId::REPAIR_COST, DataComponentValue::Int(0));
    b.set(
        DataComponentTypeId::RARITY,
        DataComponentValue::Rarity(Rarity::Common),
    );
    b.build().expect("common item components are valid")
}

/// Returns the default prototype `DataComponentMap` for a generated item.
///
/// Mirrors `Item.components()`: every item starts from `COMMON_ITEM_COMPONENTS`
/// plus its `ITEM_MODEL` (`item.identifier()`). `minecraft:air` keeps the
/// standard common prototype (and `ItemStack::is_empty()` returns
/// `DataComponentMap::EMPTY` for air stacks, matching Paper).
pub fn default_components_for(item: ItemId) -> DataComponentMap {
    let mut b = DataComponentMapBuilder::new();
    b.add_all(&common_item_components());
    b.set(
        DataComponentTypeId::ITEM_MODEL,
        DataComponentValue::Identifier(item.identifier()),
    );
    b.build().expect("default item components are valid")
}

/// Port of `Item.CODEC` (`BuiltInRegistries.ITEM.holderByNameCodec().validate(...)`).
///
/// - Parses and encodes an `Identifier` against the generated `minecraft:item` table.
/// - Rejects unknown item identifiers with `"Unknown registry key in ResourceKey[minecraft:root / minecraft:item]: <id>"`.
/// - Rejects `minecraft:air` with `"Item must not be minecraft:air"`.
pub fn item_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ItemId, Ops>> {
    let by_name = codec::flat_xmap(
        identifier_codec::<Ops>(),
        Arc::new(|id: &Identifier| match ItemId::from_identifier(id) {
            Some(item) => DataResult::success(item),
            None => DataResult::error(format!(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:item]: {id}"
            )),
        }),
        Arc::new(|item: &ItemId| DataResult::success(item.identifier())),
    );
    codec::validate(
        by_name,
        Arc::new(|item: &ItemId| {
            if item.is_air() {
                DataResult::error("Item must not be minecraft:air")
            } else {
                DataResult::success(*item)
            }
        }),
    )
}

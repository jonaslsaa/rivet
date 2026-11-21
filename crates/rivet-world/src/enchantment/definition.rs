//! Port of `net.minecraft.world.item.enchantment.Enchantment` and
//! `EnchantmentInstance` (MC 26.2).

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::generated::tags::{ENCHANTMENT_TAG_BY_NAME, ITEM_TAG_BY_NAME};

use crate::enchantment::effects::{EffectDecodeError, EnchantmentEffects};
use crate::enchantment::level_based_value::get_string;
use crate::enchantment::{EquipmentSlot, EquipmentSlotGroup};

/// `Enchantment.MAX_LEVEL` (`255`).
pub const MAX_LEVEL: i32 = 255;

/// Errors returned when decoding an [`Enchantment`] from NBT.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnchantmentDecodeError {
    #[error("missing required field '{0}' in Enchantment")]
    MissingField(&'static str),
    #[error("invalid field '{0}' in Enchantment")]
    InvalidField(&'static str),
    #[error("unknown equipment slot group '{0}'")]
    UnknownSlotGroup(String),
    #[error("invalid identifier '{0}'")]
    InvalidIdentifier(String),
    #[error("effect decode error: {0}")]
    Effect(#[from] EffectDecodeError),
}

/// Tag-or-direct membership set matching `HolderSet<Item>` / `HolderSet<Enchantment>`
/// in serialized registry payloads (`"#namespace:tag"`, `"namespace:id"`, or list of IDs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NamedHolderSet {
    /// Empty set (e.g. default `exclusive_set`).
    #[default]
    Empty,
    /// Named tag (stored without leading `#`, e.g. `"minecraft:enchantable/sword"`).
    Tag(String),
    /// Explicit ordered list of identifiers.
    Direct(Vec<Identifier>),
}

impl NamedHolderSet {
    /// Decodes a `HolderSet` encoded in NBT as a string (`"#tag"` or `"id"`) or
    /// a list of identifier strings.
    pub fn from_nbt(tag: &Tag) -> Result<Self, EnchantmentDecodeError> {
        match tag {
            Tag::String(s) => {
                if let Some(tag_name) = s.value.strip_prefix('#') {
                    Ok(Self::Tag(tag_name.to_owned()))
                } else {
                    let id = Identifier::try_parse(&s.value).ok_or_else(|| {
                        EnchantmentDecodeError::InvalidIdentifier(s.value.clone())
                    })?;
                    Ok(Self::Direct(vec![id]))
                }
            }
            Tag::List(list) => {
                let mut ids = Vec::with_capacity(list.list.len());
                for item in &list.list {
                    let Tag::String(s) = item else {
                        return Err(EnchantmentDecodeError::InvalidField("holder_set"));
                    };
                    let id = Identifier::try_parse(&s.value).ok_or_else(|| {
                        EnchantmentDecodeError::InvalidIdentifier(s.value.clone())
                    })?;
                    ids.push(id);
                }
                Ok(Self::Direct(ids))
            }
            _ => Err(EnchantmentDecodeError::InvalidField("holder_set")),
        }
    }

    /// Checks whether an item identifier is a member of this set using the
    /// generated `ITEM_TAG_BY_NAME` table.
    pub fn contains_item(&self, item_id: &Identifier) -> bool {
        match self {
            Self::Empty => false,
            Self::Direct(ids) => ids.iter().any(|id| id == item_id),
            Self::Tag(tag_name) => {
                let item_str = item_id.to_string();
                ITEM_TAG_BY_NAME
                    .get(tag_name.as_str())
                    .is_some_and(|members| members.contains(&item_str.as_str()))
            }
        }
    }

    /// Checks whether an enchantment identifier is a member of this set using
    /// the generated `ENCHANTMENT_TAG_BY_NAME` table (or an optional custom tag
    /// lookup).
    pub fn contains_enchantment(&self, enchantment_id: &Identifier) -> bool {
        match self {
            Self::Empty => false,
            Self::Direct(ids) => ids.iter().any(|id| id == enchantment_id),
            Self::Tag(tag_name) => {
                let ench_str = enchantment_id.to_string();
                ENCHANTMENT_TAG_BY_NAME
                    .get(tag_name.as_str())
                    .is_some_and(|members| members.contains(&ench_str.as_str()))
            }
        }
    }
}

/// `Enchantment.Cost` — linear cost curve for enchanting-table level selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub base: i32,
    pub per_level_above_first: i32,
}

impl Cost {
    /// `Enchantment.constantCost(int base)`.
    pub const fn constant(base: i32) -> Self {
        Self {
            base,
            per_level_above_first: 0,
        }
    }

    /// `Enchantment.dynamicCost(int base, int perLevel)`.
    pub const fn dynamic(base: i32, per_level_above_first: i32) -> Self {
        Self {
            base,
            per_level_above_first,
        }
    }

    /// `Cost.calculate(int level)` — `this.base + this.perLevelAboveFirst * (level - 1)`
    /// using Java wrapping `i32` arithmetic.
    pub const fn calculate(self, level: i32) -> i32 {
        self.base.wrapping_add(
            self.per_level_above_first
                .wrapping_mul(level.wrapping_sub(1)),
        )
    }

    fn from_nbt(compound: &CompoundTag) -> Result<Self, EnchantmentDecodeError> {
        let base = get_i32(compound, "base")?;
        let per_level_above_first = get_i32(compound, "per_level_above_first")?;
        Ok(Self {
            base,
            per_level_above_first,
        })
    }
}

/// `Enchantment.EnchantmentDefinition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantmentDefinition {
    pub supported_items: NamedHolderSet,
    pub primary_items: Option<NamedHolderSet>,
    pub weight: i32,
    pub max_level: i32,
    pub min_cost: Cost,
    pub max_cost: Cost,
    pub anvil_cost: i32,
    pub slots: Vec<EquipmentSlotGroup>,
}

/// `net.minecraft.world.item.enchantment.Enchantment`.
#[derive(Debug, Clone, PartialEq)]
pub struct Enchantment {
    /// Registered identifier of this enchantment (e.g. `minecraft:sharpness`).
    pub id: Identifier,
    /// Translation key or plain text description (e.g. `enchantment.minecraft.sharpness`).
    pub description: String,
    pub definition: EnchantmentDefinition,
    pub exclusive_set: NamedHolderSet,
    pub effects: EnchantmentEffects,
}

impl Enchantment {
    /// `Enchantment.areCompatible(Holder<Enchantment>, Holder<Enchantment>)`.
    ///
    /// Symmetric check: two enchantments are compatible iff they are distinct
    /// and neither's `exclusive_set` contains the other.
    pub fn are_compatible(a: &Enchantment, b: &Enchantment) -> bool {
        a.id != b.id
            && !a.exclusive_set.contains_enchantment(&b.id)
            && !b.exclusive_set.contains_enchantment(&a.id)
    }

    /// `Enchantment.matchingSlot(EquipmentSlot)`.
    pub fn matching_slot(&self, slot: EquipmentSlot) -> bool {
        self.definition.slots.iter().any(|group| group.test(slot))
    }

    /// `Enchantment.isPrimaryItem(ItemStack)` — falls back to `isSupportedItem`
    /// when `primary_items` is absent.
    pub fn is_primary_item(&self, item_id: &Identifier) -> bool {
        self.is_supported_item(item_id)
            && self
                .definition
                .primary_items
                .as_ref()
                .is_none_or(|primary| primary.contains_item(item_id))
    }

    /// `Enchantment.isSupportedItem(ItemStack)`.
    pub fn is_supported_item(&self, item_id: &Identifier) -> bool {
        self.definition.supported_items.contains_item(item_id)
    }

    /// `Enchantment.getWeight()`.
    pub const fn weight(&self) -> i32 {
        self.definition.weight
    }

    /// `Enchantment.getAnvilCost()`.
    pub const fn anvil_cost(&self) -> i32 {
        self.definition.anvil_cost
    }

    /// `Enchantment.getMinLevel()` — always `1` in vanilla Paper.
    pub const fn min_level(&self) -> i32 {
        1
    }

    /// `Enchantment.getMaxLevel()`.
    pub const fn max_level(&self) -> i32 {
        self.definition.max_level
    }

    /// `Enchantment.getMinCost(int level)`.
    pub const fn min_cost(&self, level: i32) -> i32 {
        self.definition.min_cost.calculate(level)
    }

    /// `Enchantment.getMaxCost(int level)`.
    pub const fn max_cost(&self, level: i32) -> i32 {
        self.definition.max_cost.calculate(level)
    }

    /// Decodes an [`Enchantment`] from its synchronized registry NBT payload.
    pub fn from_nbt(
        id: Identifier,
        compound: &CompoundTag,
    ) -> Result<Self, EnchantmentDecodeError> {
        let description = match compound.tags.get("description") {
            Some(Tag::Compound(desc)) => get_string(desc, "translate")
                .or_else(|| get_string(desc, "text"))
                .unwrap_or_default()
                .to_owned(),
            Some(Tag::String(s)) => s.value.clone(),
            _ => String::new(),
        };

        let supported_items = NamedHolderSet::from_nbt(
            compound
                .tags
                .get("supported_items")
                .ok_or(EnchantmentDecodeError::MissingField("supported_items"))?,
        )?;

        let primary_items = match compound.tags.get("primary_items") {
            Some(tag) => Some(NamedHolderSet::from_nbt(tag)?),
            None => None,
        };

        let weight = get_i32(compound, "weight")?;
        let max_level = get_i32(compound, "max_level")?;
        let anvil_cost = get_i32(compound, "anvil_cost")?;

        let min_cost = match compound.tags.get("min_cost") {
            Some(Tag::Compound(c)) => Cost::from_nbt(c)?,
            Some(_) => return Err(EnchantmentDecodeError::InvalidField("min_cost")),
            None => return Err(EnchantmentDecodeError::MissingField("min_cost")),
        };
        let max_cost = match compound.tags.get("max_cost") {
            Some(Tag::Compound(c)) => Cost::from_nbt(c)?,
            Some(_) => return Err(EnchantmentDecodeError::InvalidField("max_cost")),
            None => return Err(EnchantmentDecodeError::MissingField("max_cost")),
        };

        let slots = match compound.tags.get("slots") {
            Some(Tag::List(list)) => {
                let mut out = Vec::with_capacity(list.list.len());
                for item in &list.list {
                    let Tag::String(s) = item else {
                        return Err(EnchantmentDecodeError::InvalidField("slots"));
                    };
                    let group = EquipmentSlotGroup::from_serialized_name(&s.value)
                        .ok_or_else(|| EnchantmentDecodeError::UnknownSlotGroup(s.value.clone()))?;
                    out.push(group);
                }
                out
            }
            Some(_) => return Err(EnchantmentDecodeError::InvalidField("slots")),
            None => return Err(EnchantmentDecodeError::MissingField("slots")),
        };

        let exclusive_set = match compound.tags.get("exclusive_set") {
            Some(tag) => NamedHolderSet::from_nbt(tag)?,
            None => NamedHolderSet::Empty,
        };

        let effects = match compound.tags.get("effects") {
            Some(Tag::Compound(c)) => EnchantmentEffects::from_nbt(c)?,
            Some(_) => return Err(EnchantmentDecodeError::InvalidField("effects")),
            None => EnchantmentEffects::default(),
        };

        Ok(Self {
            id,
            description,
            definition: EnchantmentDefinition {
                supported_items,
                primary_items,
                weight,
                max_level,
                min_cost,
                max_cost,
                anvil_cost,
                slots,
            },
            exclusive_set,
            effects,
        })
    }
}

/// `net.minecraft.world.item.enchantment.EnchantmentInstance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantmentInstance {
    pub enchantment: Identifier,
    pub level: i32,
}

impl EnchantmentInstance {
    pub fn new(enchantment: Identifier, level: i32) -> Self {
        Self { enchantment, level }
    }
}

fn get_i32(compound: &CompoundTag, key: &'static str) -> Result<i32, EnchantmentDecodeError> {
    match compound.tags.get(key) {
        Some(Tag::Int(i)) => Ok(i.value),
        Some(Tag::Short(s)) => Ok(i32::from(s.value)),
        Some(Tag::Byte(b)) => Ok(i32::from(b.value)),
        Some(_) => Err(EnchantmentDecodeError::InvalidField(key)),
        None => Err(EnchantmentDecodeError::MissingField(key)),
    }
}

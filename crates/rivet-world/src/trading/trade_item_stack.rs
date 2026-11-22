//! Lightweight Paper-faithful `ItemStack` value type for trading cost matching,
//! payment consumption, offer results, and NBT persistence.

use indexmap::IndexMap;
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::identifier::Identifier;

use super::data_component_exact_predicate::{DataComponentExactPredicate, TradingCodecError};

/// Lightweight Paper-faithful `ItemStack` value type for trading cost matching,
/// payment consumption, offer results, and NBT persistence.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeItemStack {
    item: Identifier,
    count: i32,
    base_max_stack_size: i32,
    components: IndexMap<Identifier, Tag>,
}

impl TradeItemStack {
    pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;
    pub const ABSOLUTE_MAX_STACK_SIZE: i32 = 99;

    fn max_stack_size_override(components: &IndexMap<Identifier, Tag>) -> Option<i32> {
        match components.get(&Identifier::with_default_namespace("max_stack_size")) {
            Some(Tag::Int(tag)) => Some(tag.value.clamp(1, Self::ABSOLUTE_MAX_STACK_SIZE)),
            _ => None,
        }
    }

    /// `ItemStack.EMPTY` (`minecraft:air`, count `0`).
    pub fn empty() -> Self {
        Self {
            item: Identifier::with_default_namespace("air"),
            count: 0,
            base_max_stack_size: Self::DEFAULT_MAX_STACK_SIZE,
            components: IndexMap::new(),
        }
    }

    /// Creates an item stack with the default max stack size (64).
    pub fn new(item: Identifier, count: i32) -> Self {
        Self::with_max_stack_size(item, count, Self::DEFAULT_MAX_STACK_SIZE)
    }

    /// Creates an item stack with an explicit max stack size (clamped to `>= 1`).
    pub fn with_max_stack_size(item: Identifier, count: i32, max_stack_size: i32) -> Self {
        Self {
            item,
            count,
            base_max_stack_size: max_stack_size.max(1),
            components: IndexMap::new(),
        }
    }

    /// Attaches exact component entries to this stack.
    pub fn with_components(
        mut self,
        components: impl IntoIterator<Item = (Identifier, Tag)>,
    ) -> Self {
        self.components = components.into_iter().collect();
        self
    }

    pub fn item(&self) -> &Identifier {
        &self.item
    }

    pub fn count(&self) -> i32 {
        if self.is_empty() { 0 } else { self.count }
    }

    pub fn max_stack_size(&self) -> i32 {
        Self::max_stack_size_override(&self.components).unwrap_or(self.base_max_stack_size)
    }

    pub fn components(&self) -> &IndexMap<Identifier, Tag> {
        &self.components
    }

    /// Mirrors `ItemStack.isEmpty()`: `count <= 0` or item is `minecraft:air`.
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item == Identifier::with_default_namespace("air")
    }

    /// Mirrors `ItemStack.shrink(int)`: subtracts `amount` using wrapping i32
    /// arithmetic and clears to empty when `<= 0`.
    pub fn shrink(&mut self, amount: i32) {
        if self.is_empty() {
            return;
        }
        let next = self.count.wrapping_sub(amount);
        if next <= 0 {
            *self = Self::empty();
        } else {
            self.count = next;
        }
    }

    /// Mirrors `ItemStack.copyWithCount(int)`.
    pub fn copy_with_count(&self, count: i32) -> Self {
        if self.is_empty() || count <= 0 {
            return Self::empty();
        }
        let mut copy = self.clone();
        copy.count = count;
        copy
    }

    /// Mirrors `ItemStack.isSameItemSameComponents`.
    pub fn is_same_item_same_components(&self, other: &Self) -> bool {
        self.item == other.item && self.components == other.components
    }

    /// Encodes as MC 26.2 `ItemStack.CODEC` NBT (`id`, `count`, optional `components`).
    pub fn to_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_string("id", &self.item.to_string());
        tag.put_int("count", self.count);
        if !self.components.is_empty() {
            let mut comp_tag = CompoundTag::new();
            for (k, v) in &self.components {
                comp_tag.put(k.to_string(), v.clone());
            }
            tag.put("components".to_string(), Tag::Compound(comp_tag));
        }
        tag
    }

    /// Decodes from MC 26.2 `ItemStack.CODEC` NBT using `DEFAULT_MAX_STACK_SIZE`
    /// when neither `minecraft:max_stack_size` nor an item-type resolver is supplied.
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        Self::from_nbt_with_item_max_stack(tag, |_| Self::DEFAULT_MAX_STACK_SIZE)
    }

    /// Decodes from MC 26.2 `ItemStack.CODEC` NBT, resolving each item's default
    /// max stack size via `item_max_stack` and honoring any `minecraft:max_stack_size`
    /// component override (matching `ItemInstance.getMaxStackSize()`).
    pub fn from_nbt_with_item_max_stack<F>(
        tag: &CompoundTag,
        mut item_max_stack: F,
    ) -> Result<Self, TradingCodecError>
    where
        F: FnMut(&Identifier) -> i32,
    {
        let id_str = tag
            .get_string("id")
            .ok_or(TradingCodecError::MissingField("id"))?;
        let item =
            Identifier::try_parse(id_str).ok_or_else(|| TradingCodecError::InvalidField {
                field: "id",
                reason: format!("invalid item identifier `{id_str}`"),
            })?;
        let count = tag.get_int("count").unwrap_or(1);
        if count <= 0 {
            return Err(TradingCodecError::InvalidField {
                field: "count",
                reason: format!("item stack count must be positive, got {count}"),
            });
        }
        let components = match tag.get_compound("components") {
            Some(comp_tag) => DataComponentExactPredicate::from_nbt(comp_tag)?.expected,
            None => IndexMap::new(),
        };
        let base_max_stack_size = item_max_stack(&item).max(1);
        Ok(Self {
            item,
            count,
            base_max_stack_size,
            components,
        })
    }
}

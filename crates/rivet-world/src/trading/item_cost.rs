//! `net.minecraft.world.item.trading.ItemCost` and its item/component matching
//! value types.

use indexmap::IndexMap;
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::identifier::Identifier;

/// Error returned when decoding trading NBT payloads.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TradingCodecError {
    #[error("missing required NBT field `{0}`")]
    MissingField(&'static str),
    #[error("invalid NBT field `{field}`: {reason}")]
    InvalidField { field: &'static str, reason: String },
}

/// Exact data-component predicate mirroring Paper's `DataComponentExactPredicate`.
///
/// Uses `IndexMap` so iteration and serialized key order remain deterministic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataComponentExactPredicate {
    expected: IndexMap<Identifier, Tag>,
}

impl DataComponentExactPredicate {
    /// Empty predicate — matches any item stack regardless of its components.
    pub fn empty() -> Self {
        Self {
            expected: IndexMap::new(),
        }
    }

    /// Creates a predicate requiring all `(key, value)` pairs to match exactly.
    pub fn from_entries(entries: impl IntoIterator<Item = (Identifier, Tag)>) -> Self {
        Self {
            expected: entries.into_iter().collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.expected.is_empty()
    }

    pub fn entries(&self) -> &IndexMap<Identifier, Tag> {
        &self.expected
    }

    /// Inserts an expected component value, preserving insertion order.
    pub fn expect(mut self, key: Identifier, value: Tag) -> Self {
        self.expected.insert(key, value);
        self
    }

    /// Returns `true` iff every expected component is present on `stack` with an
    /// equal value (`DataComponentExactPredicate.test`).
    pub fn test(&self, stack: &TradeItemStack) -> bool {
        self.expected
            .iter()
            .all(|(key, expected_value)| stack.components.get(key) == Some(expected_value))
    }

    /// Encodes the predicate as an NBT compound tag (`id -> Tag`).
    pub fn to_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        for (key, value) in &self.expected {
            tag.put(key.to_string(), value.clone());
        }
        tag
    }

    /// Decodes a predicate from an NBT compound tag.
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        let mut expected = IndexMap::with_capacity(tag.size());
        for (key, value) in tag.tags.iter() {
            let id = Identifier::try_parse(key).ok_or_else(|| TradingCodecError::InvalidField {
                field: "components",
                reason: format!("invalid component identifier `{key}`"),
            })?;
            expected.insert(id, value.clone());
        }
        Ok(Self { expected })
    }
}

/// Lightweight Paper-faithful `ItemStack` value type for trading cost matching,
/// payment consumption, offer results, and NBT persistence.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeItemStack {
    item: Identifier,
    count: i32,
    max_stack_size: i32,
    components: IndexMap<Identifier, Tag>,
}

impl TradeItemStack {
    pub const DEFAULT_MAX_STACK_SIZE: i32 = 64;

    /// `ItemStack.EMPTY` (`minecraft:air`, count `0`).
    pub fn empty() -> Self {
        Self {
            item: Identifier::with_default_namespace("air"),
            count: 0,
            max_stack_size: Self::DEFAULT_MAX_STACK_SIZE,
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
            max_stack_size: max_stack_size.max(1),
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
        self.max_stack_size
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
        if count <= 0 || self.item == Identifier::with_default_namespace("air") {
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

    /// Decodes from MC 26.2 `ItemStack.CODEC` NBT.
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
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
        Ok(Self {
            item,
            count,
            max_stack_size: Self::DEFAULT_MAX_STACK_SIZE,
            components,
        })
    }
}

/// Mirrors `net.minecraft.world.item.trading.ItemCost`.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemCost {
    item: Identifier,
    count: i32,
    components: DataComponentExactPredicate,
    item_stack: TradeItemStack,
}

impl ItemCost {
    /// Creates an `ItemCost` with an empty component predicate (`ItemCost(item, count)`).
    pub fn new(item: Identifier, count: i32) -> Self {
        Self::with_components(item, count, DataComponentExactPredicate::empty())
    }

    /// Creates an `ItemCost` with an explicit max stack size for the derived stack.
    pub fn with_max_stack_size(item: Identifier, count: i32, max_stack_size: i32) -> Self {
        Self::with_components_and_max_stack(
            item,
            count,
            DataComponentExactPredicate::empty(),
            max_stack_size,
        )
    }

    /// Creates an `ItemCost` with an exact component predicate (`ItemCost(item, count, components)`).
    pub fn with_components(
        item: Identifier,
        count: i32,
        components: DataComponentExactPredicate,
    ) -> Self {
        Self::with_components_and_max_stack(
            item,
            count,
            components,
            TradeItemStack::DEFAULT_MAX_STACK_SIZE,
        )
    }

    /// Creates an `ItemCost` with exact component predicate and explicit max stack size.
    pub fn with_components_and_max_stack(
        item: Identifier,
        count: i32,
        components: DataComponentExactPredicate,
        max_stack_size: i32,
    ) -> Self {
        let item_stack = TradeItemStack::with_max_stack_size(item.clone(), count, max_stack_size)
            .with_components(
                components
                    .entries()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        Self {
            item,
            count,
            components,
            item_stack,
        }
    }

    pub fn item(&self) -> &Identifier {
        &self.item
    }

    pub fn count(&self) -> i32 {
        self.count
    }

    pub fn components(&self) -> &DataComponentExactPredicate {
        &self.components
    }

    pub fn item_stack(&self) -> &TradeItemStack {
        &self.item_stack
    }

    /// Mirrors `ItemCost.test(ItemStack)`: checks item identity and component
    /// predicate (does NOT check count; count is checked by `MerchantOffer.satisfiedBy`).
    pub fn test(&self, stack: &TradeItemStack) -> bool {
        !stack.is_empty() && stack.item() == &self.item && self.components.test(stack)
    }

    /// Encodes as MC 26.2 `ItemCost.CODEC` NBT (`id`, `count`, optional `components`).
    pub fn to_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_string("id", &self.item.to_string());
        tag.put_int("count", self.count);
        if !self.components.is_empty() {
            tag.put(
                "components".to_string(),
                Tag::Compound(self.components.to_nbt()),
            );
        }
        tag
    }

    /// Decodes from MC 26.2 `ItemCost.CODEC` NBT (`count` defaults to `1`).
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
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
                reason: format!("item cost count must be positive, got {count}"),
            });
        }
        let components = match tag.get_compound("components") {
            Some(comp_tag) => DataComponentExactPredicate::from_nbt(comp_tag)?,
            None => DataComponentExactPredicate::empty(),
        };
        Ok(Self::with_components(item, count, components))
    }
}

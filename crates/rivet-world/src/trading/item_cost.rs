//! `net.minecraft.world.item.trading.ItemCost`.

use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::identifier::Identifier;

use super::data_component_exact_predicate::{DataComponentExactPredicate, TradingCodecError};
use super::trade_item_stack::TradeItemStack;

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
        Self::from_nbt_with_item_max_stack(tag, |_| TradeItemStack::DEFAULT_MAX_STACK_SIZE)
    }

    /// Decodes from MC 26.2 `ItemCost.CODEC` NBT, resolving each item's default
    /// max stack size via `item_max_stack` and honoring any `minecraft:max_stack_size`
    /// component override.
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
                reason: format!("item cost count must be positive, got {count}"),
            });
        }
        let components = match tag.get_compound("components") {
            Some(comp_tag) => DataComponentExactPredicate::from_nbt(comp_tag)?,
            None => DataComponentExactPredicate::empty(),
        };
        let default_max = item_max_stack(&item).max(1);
        Ok(Self::with_components_and_max_stack(
            item,
            count,
            components,
            default_max,
        ))
    }
}

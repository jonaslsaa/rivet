//! `net.minecraft.world.item.trading.MerchantOffers`.

use rivet_nbt::{compound_tag::CompoundTag, list_tag::ListTag, tag::Tag};

use super::item_cost::{TradeItemStack, TradingCodecError};
use super::merchant_offer::MerchantOffer;

/// Ordered collection of merchant offers mirroring
/// `net.minecraft.world.item.trading.MerchantOffers`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MerchantOffers {
    offers: Vec<MerchantOffer>,
}

impl MerchantOffers {
    pub fn new() -> Self {
        Self { offers: Vec::new() }
    }

    pub fn from_vec(offers: Vec<MerchantOffer>) -> Self {
        Self { offers }
    }

    pub fn push(&mut self, offer: MerchantOffer) {
        self.offers.push(offer);
    }

    pub fn len(&self) -> usize {
        self.offers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&MerchantOffer> {
        self.offers.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut MerchantOffer> {
        self.offers.get_mut(index)
    }

    pub fn as_slice(&self) -> &[MerchantOffer] {
        &self.offers
    }

    pub fn as_mut_slice(&mut self) -> &mut [MerchantOffer] {
        &mut self.offers
    }

    pub fn iter(&self) -> std::slice::Iter<'_, MerchantOffer> {
        self.offers.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, MerchantOffer> {
        self.offers.iter_mut()
    }

    /// Mirrors `MerchantOffers.getRecipeFor(ItemStack buyA, ItemStack buyB, int selectionHint)`
    /// returning the matching offer's index.
    ///
    /// Exact Paper edge semantics:
    /// - `selection_hint > 0 && (selection_hint as usize) < self.len()`:
    ///   checks ONLY that offer; returns `None` if it is not satisfied (does NOT
    ///   fall back to scanning the rest of the list).
    /// - `selection_hint <= 0` (including `0`) or `selection_hint >= self.len()`:
    ///   scans `0..self.len()` in order and returns the first satisfied offer.
    pub fn get_recipe_index_for(
        &self,
        buy_a: &TradeItemStack,
        buy_b: &TradeItemStack,
        selection_hint: i32,
    ) -> Option<usize> {
        if selection_hint > 0 && (selection_hint as usize) < self.offers.len() {
            let idx = selection_hint as usize;
            return self.offers[idx].satisfied_by(buy_a, buy_b).then_some(idx);
        }

        for (i, offer) in self.offers.iter().enumerate() {
            if offer.satisfied_by(buy_a, buy_b) {
                return Some(i);
            }
        }

        None
    }

    /// Mirrors `MerchantOffers.getRecipeFor(ItemStack buyA, ItemStack buyB, int selectionHint)`.
    pub fn get_recipe_for(
        &self,
        buy_a: &TradeItemStack,
        buy_b: &TradeItemStack,
        selection_hint: i32,
    ) -> Option<&MerchantOffer> {
        self.get_recipe_index_for(buy_a, buy_b, selection_hint)
            .map(|idx| &self.offers[idx])
    }

    /// Mirrors `MerchantOffers.copy()`.
    pub fn copy(&self) -> Self {
        Self {
            offers: self.offers.iter().map(MerchantOffer::copy).collect(),
        }
    }

    /// Encodes as Paper 26.2 `MerchantOffers.CODEC` NBT (`{ "Recipes": [...] }`).
    pub fn to_nbt(&self) -> CompoundTag {
        let mut recipes = ListTag::new();
        for offer in &self.offers {
            recipes.add(Tag::Compound(offer.to_nbt()));
        }
        let mut tag = CompoundTag::new();
        tag.put("Recipes".to_string(), Tag::List(recipes));
        tag
    }

    /// Decodes from Paper 26.2 `MerchantOffers.CODEC` NBT. Missing `Recipes`
    /// decodes as an empty `MerchantOffers` (`optionalFieldOf("Recipes", List.of())`).
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        Self::from_nbt_with_item_max_stack(tag, |_| {
            super::TradeItemStack::DEFAULT_MAX_STACK_SIZE
        })
    }

    /// Decodes from Paper 26.2 `MerchantOffers.CODEC` NBT with an item max-stack resolver.
    pub fn from_nbt_with_item_max_stack<F>(
        tag: &CompoundTag,
        mut item_max_stack: F,
    ) -> Result<Self, TradingCodecError>
    where
        F: FnMut(&rivet_registry::identifier::Identifier) -> i32,
    {
        let Some(recipes) = tag.get_list("Recipes") else {
            return Ok(Self::new());
        };
        let mut offers = Vec::with_capacity(recipes.list.len());
        for entry in recipes.list.iter() {
            let Tag::Compound(offer_tag) = entry else {
                return Err(TradingCodecError::InvalidField {
                    field: "Recipes",
                    reason: "expected compound tag entry".to_string(),
                });
            };
            offers.push(MerchantOffer::from_nbt_with_item_max_stack(
                offer_tag,
                &mut item_max_stack,
            )?);
        }
        Ok(Self { offers })
    }
}

//! `net.minecraft.world.inventory.MerchantContainer` and `MerchantResultSlot.onTake`.

use super::merchant::Merchant;
use super::merchant_offer::MerchantOffer;
use super::merchant_offers::MerchantOffers;
use super::trade_item_stack::TradeItemStack;

/// Completed trade result returned by [`MerchantContainer::take_result`].
#[derive(Debug, Clone, PartialEq)]
pub struct TradeTransaction {
    pub offer_index: usize,
    pub result: TradeItemStack,
    pub awarded_xp: i32,
}

/// Three-slot merchant container mirroring `net.minecraft.world.inventory.MerchantContainer`
/// and `MerchantResultSlot.onTake`.
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantContainer {
    pub payment_slot_0: TradeItemStack,
    pub payment_slot_1: TradeItemStack,
    pub result_slot: TradeItemStack,
    selection_hint: i32,
    active_offer_index: Option<usize>,
    future_xp: i32,
}

impl Default for MerchantContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl MerchantContainer {
    pub fn new() -> Self {
        Self {
            payment_slot_0: TradeItemStack::empty(),
            payment_slot_1: TradeItemStack::empty(),
            result_slot: TradeItemStack::empty(),
            selection_hint: 0,
            active_offer_index: None,
            future_xp: 0,
        }
    }

    pub fn selection_hint(&self) -> i32 {
        self.selection_hint
    }

    pub fn active_offer_index(&self) -> Option<usize> {
        self.active_offer_index
    }

    pub fn future_xp(&self) -> i32 {
        self.future_xp
    }

    /// Mirrors `MerchantContainer.setSelectionHint(int)`.
    pub fn set_selection_hint(&mut self, selection_hint: i32, offers: &MerchantOffers) {
        self.selection_hint = selection_hint;
        self.update_sell_item(offers);
    }

    /// Mirrors `MerchantContainer.updateSellItem()` (Paper 26.2):
    /// - If slot 0 is empty, slot 1 becomes `buyA` and `buyB` is empty;
    ///   otherwise slot 0 is `buyA` and slot 1 is `buyB`.
    /// - If `buyA` is empty, clears result and `future_xp`.
    /// - Looks up `offers.get_recipe_index_for(buyA, buyB, selection_hint)`.
    /// - If `None` or the matched offer is out of stock, stores that offer as
    ///   `active_offer_index` and retries with reversed `(buyB, buyA, selection_hint)`.
    /// - Only an in-stock offer populates `result_slot` and `future_xp`.
    pub fn update_sell_item(&mut self, offers: &MerchantOffers) {
        let (buy_a, buy_b) = if self.payment_slot_0.is_empty() {
            (&self.payment_slot_1, TradeItemStack::empty())
        } else {
            (&self.payment_slot_0, self.payment_slot_1.clone())
        };

        if buy_a.is_empty() {
            self.active_offer_index = None;
            self.result_slot = TradeItemStack::empty();
            self.future_xp = 0;
            return;
        }

        if offers.is_empty() {
            self.active_offer_index = None;
            self.result_slot = TradeItemStack::empty();
            self.future_xp = 0;
            return;
        }

        let mut offer_idx = offers.get_recipe_index_for(buy_a, &buy_b, self.selection_hint);
        let needs_reversed = match offer_idx {
            None => true,
            Some(idx) => offers.get(idx).is_none_or(MerchantOffer::is_out_of_stock),
        };

        if needs_reversed {
            self.active_offer_index = offer_idx;
            offer_idx = offers.get_recipe_index_for(&buy_b, buy_a, self.selection_hint);
        }

        if let Some(idx) = offer_idx
            && let Some(offer) = offers.get(idx)
            && !offer.is_out_of_stock()
        {
            self.active_offer_index = Some(idx);
            self.result_slot = offer.assemble();
            self.future_xp = offer.xp();
            return;
        }

        if offer_idx.is_some() {
            self.active_offer_index = offer_idx;
        }
        self.result_slot = TradeItemStack::empty();
        self.future_xp = 0;
    }

    /// Mirrors `MerchantResultSlot.onTake` (Paper 26.2):
    /// - Requires an active, in-stock offer.
    /// - `allow_purchase` models Paper's cancellable `PlayerPurchaseEvent` /
    ///   `PlayerTradeEvent`; returning `false` aborts without consuming items.
    /// - Tries `offer.take(slot0, slot1) || offer.take(slot1, slot0)`.
    /// - Calls `merchant.notify_trade(offer_index)` and refreshes `update_sell_item`.
    pub fn take_result<M, F>(
        &mut self,
        merchant: &mut M,
        allow_purchase: F,
    ) -> Option<TradeTransaction>
    where
        M: Merchant,
        F: FnOnce(&MerchantOffer) -> bool,
    {
        let offer_idx = self.active_offer_index?;
        let offer = merchant.offers().get(offer_idx)?;
        if offer.is_out_of_stock() {
            return None;
        }
        if !allow_purchase(offer) {
            return None;
        }

        let taken = offer.take(&mut self.payment_slot_0, &mut self.payment_slot_1)
            || offer.take(&mut self.payment_slot_1, &mut self.payment_slot_0);
        if !taken {
            return None;
        }

        let result = offer.assemble();
        let awarded_xp = if offer.should_reward_exp() {
            offer.xp()
        } else {
            0
        };

        merchant.notify_trade(offer_idx);
        self.update_sell_item(merchant.offers());

        Some(TradeTransaction {
            offer_index: offer_idx,
            result,
            awarded_xp,
        })
    }
}

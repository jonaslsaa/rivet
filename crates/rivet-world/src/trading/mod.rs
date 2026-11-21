//! `net.minecraft.world.item.trading` + merchant runtime foundations (issue #461, M3).
//!
//! Paper 26.2 (pinned commit `0a993450f129c4942c2a9ed45ba047412b4667cf`) is the
//! behavioral source of truth for this module:
//! - [`item_cost`]: [`TradeItemStack`], [`DataComponentExactPredicate`], and
//!   [`ItemCost`] (`ItemCost.java`).
//! - [`merchant_offer`]: [`MerchantOffer`] (`MerchantOffer.java`) — modified
//!   cost calculation, demand updates (including Paper's
//!   `preventNegativeVillagerDemand`), special-price discounts, `ignoreDiscounts`,
//!   payment satisfaction/consumption, and NBT save/load.
//! - [`merchant_offers`]: [`MerchantOffers`] (`MerchantOffers.java`) — ordered
//!   offer list, `get_recipe_for` selection-hint rules, and `Recipes` NBT codec.
//! - [`trade_generation`]: [`TradeCost`], [`VillagerTrade`], [`TradeSet`],
//!   [`TradeSets`], and deterministic `add_offers_from_trade_set`
//!   (`TradeCost.java`, `VillagerTrade.java`, `TradeSet.java`, `TradeSets.java`,
//!   `AbstractVillager.addOffersFromTradeSet`, `WanderingTrader.updateTrades`).
//! - [`merchant_runtime`]: [`Merchant`], [`MerchantContainer`],
//!   [`TradeTransaction`], [`VillagerMerchantState`], and
//!   [`WanderingTraderMerchantState`] (`Merchant.java`, `MerchantContainer.java`,
//!   `MerchantResultSlot.java`, `Villager.java`, `WanderingTrader.java`).

pub mod item_cost;
pub mod merchant_offer;
pub mod merchant_offers;
pub mod merchant_runtime;
pub mod trade_generation;

pub use item_cost::{DataComponentExactPredicate, ItemCost, TradeItemStack, TradingCodecError};
pub use merchant_offer::MerchantOffer;
pub use merchant_offers::MerchantOffers;
pub use merchant_runtime::{
    Merchant, MerchantContainer, TradeTransaction, VillagerMerchantState,
    WanderingTraderMerchantState,
};
pub use trade_generation::{
    TradeCost, TradeNumberProvider, TradeSet, TradeSets, VillagerTrade, add_offers_from_trade_set,
};

#[cfg(test)]
mod tests;

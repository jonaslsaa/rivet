//! `net.minecraft.world.item.trading` + merchant runtime foundations (issue #461, M3).
//!
//! Paper 26.2 (pinned commit `0a993450f129c4942c2a9ed45ba047412b4667cf`) is the
//! behavioral source of truth for this module, organized one public class per
//! module per `PORTING.md`:
//! - [`data_component_exact_predicate`] — [`DataComponentExactPredicate`], [`TradingCodecError`]
//! - [`trade_item_stack`] — [`TradeItemStack`]
//! - [`item_cost`] — [`ItemCost`]
//! - [`merchant_offer`] — [`MerchantOffer`]
//! - [`merchant_offers`] — [`MerchantOffers`]
//! - [`merchant`] — [`Merchant`]
//! - [`merchant_container`] — [`MerchantContainer`], [`TradeTransaction`]
//! - [`villager_merchant_state`] — [`VillagerMerchantState`]
//! - [`wandering_trader_merchant_state`] — [`WanderingTraderMerchantState`]
//! - [`trade_cost`] — [`TradeCost`], [`TradeNumberProvider`]
//! - [`villager_trade`] — [`VillagerTrade`]
//! - [`trade_set`] — [`TradeSet`], [`add_offers_from_trade_set`]
//! - [`trade_sets`] — [`TradeSets`]

pub mod data_component_exact_predicate;
pub mod item_cost;
pub mod merchant;
pub mod merchant_container;
pub mod merchant_offer;
pub mod merchant_offers;
pub mod trade_cost;
pub mod trade_item_stack;
pub mod trade_set;
pub mod trade_sets;
pub mod villager_merchant_state;
pub mod villager_trade;
pub mod wandering_trader_merchant_state;

pub use data_component_exact_predicate::{DataComponentExactPredicate, TradingCodecError};
pub use item_cost::ItemCost;
pub use merchant::Merchant;
pub use merchant_container::{MerchantContainer, TradeTransaction};
pub use merchant_offer::MerchantOffer;
pub use merchant_offers::MerchantOffers;
pub use trade_cost::{TradeCost, TradeNumberProvider};
pub use trade_item_stack::TradeItemStack;
pub use trade_set::{TradeSet, add_offers_from_trade_set};
pub use trade_sets::TradeSets;
pub use villager_merchant_state::VillagerMerchantState;
pub use villager_trade::VillagerTrade;
pub use wandering_trader_merchant_state::WanderingTraderMerchantState;

#[cfg(test)]
mod tests;

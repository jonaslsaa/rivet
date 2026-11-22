//! `net.minecraft.world.item.trading.VillagerTrade` (Paper 26.2).

use rivet_util::random::RandomSource;

use super::merchant_offer::MerchantOffer;
use super::trade_cost::{TradeCost, TradeNumberProvider};
use super::trade_item_stack::TradeItemStack;

/// Mirrors `net.minecraft.world.item.trading.VillagerTrade` (Paper 26.2).
#[derive(Debug, Clone, PartialEq)]
pub struct VillagerTrade {
    pub wants: TradeCost,
    pub additional_wants: Option<TradeCost>,
    pub gives: TradeItemStack,
    pub max_uses: TradeNumberProvider,
    pub reputation_discount: TradeNumberProvider,
    pub xp: i32,
    /// Additional cost contributed by item modifiers (e.g. `EnchantWithLevels`).
    pub additional_cost: i32,
    /// Mirrors `VillagerTrade.doubleTradePriceEnchantments` check on the result.
    pub double_trade_price_enchanted: bool,
    /// Result of evaluating the optional `merchantPredicate` (`true` when absent).
    pub merchant_predicate_passes: bool,
}

impl VillagerTrade {
    pub fn new(wants: TradeCost, gives: TradeItemStack) -> Self {
        Self {
            wants,
            additional_wants: None,
            gives,
            max_uses: TradeNumberProvider::exactly(4.0),
            reputation_discount: TradeNumberProvider::exactly(0.05),
            xp: 1,
            additional_cost: 0,
            double_trade_price_enchanted: false,
            merchant_predicate_passes: true,
        }
    }

    /// Mirrors `VillagerTrade.getOffer(LootContext)` validation and clamping.
    pub fn get_offer<R: RandomSource>(&self, rng: &mut R) -> Option<MerchantOffer> {
        if !self.merchant_predicate_passes {
            return None;
        }
        if self.gives.is_empty() {
            return None;
        }

        let mut additional_cost = self.additional_cost;
        if self.double_trade_price_enchanted {
            additional_cost = additional_cost.wrapping_mul(2);
        }

        let wanted_item = self.wants.to_item_cost(rng, additional_cost);
        if wanted_item.count() < 1 {
            return None;
        }

        let additional_wanted = match &self.additional_wants {
            Some(cost_b) => {
                let item_cost = cost_b.to_item_cost(rng, 0);
                if item_cost.count() < 1 {
                    return None;
                }
                Some(item_cost)
            }
            None => None,
        };

        let max_uses = self.max_uses.get_int(rng).max(1);
        let xp = self.xp.max(0);
        let reputation_discount = self.reputation_discount.get_float(rng).max(0.0);

        Some(MerchantOffer::with_cost_b(
            wanted_item,
            additional_wanted,
            self.gives.clone(),
            0,
            max_uses,
            xp,
            reputation_discount,
            0,
        ))
    }
}

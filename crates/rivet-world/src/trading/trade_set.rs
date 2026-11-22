//! `net.minecraft.world.item.trading.TradeSet` and `AbstractVillager.addOffersFromTradeSet`.

use rivet_registry::identifier::Identifier;
use rivet_util::random::RandomSource;

use super::merchant_offer::MerchantOffer;
use super::merchant_offers::MerchantOffers;
use super::trade_cost::TradeNumberProvider;
use super::villager_trade::VillagerTrade;

/// Mirrors `net.minecraft.world.item.trading.TradeSet`.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeSet {
    pub trades: Vec<VillagerTrade>,
    pub amount: TradeNumberProvider,
    pub allow_duplicates: bool,
    pub random_sequence: Option<Identifier>,
}

impl TradeSet {
    pub fn new(
        trades: Vec<VillagerTrade>,
        amount: TradeNumberProvider,
        allow_duplicates: bool,
        random_sequence: Option<Identifier>,
    ) -> Self {
        Self {
            trades,
            amount,
            allow_duplicates,
            random_sequence,
        }
    }

    /// Mirrors `TradeSet.calculateNumberOfTrades(LootContext)`.
    pub fn calculate_number_of_trades<R: RandomSource>(&self, rng: &mut R) -> i32 {
        self.amount.get_int(rng)
    }
}

/// Mirrors `AbstractVillager.addOffersFromTradeSet`:
/// selects up to `trade_set.calculate_number_of_trades(rng)` offers deterministically.
///
/// - When `!trade_set.allow_duplicates`, removes the rolled candidate before
///   calling `get_offer`.
/// - When `trade_set.allow_duplicates`, keeps the candidate on success and only
///   removes it if `get_offer` returns `None`.
/// - `accept_offer` models Paper's `VillagerAcquireTradeEvent` hook (return
///   `false` to cancel adding that offer; note Paper still increments
///   `offersFound` after `tryAcquireTrade`).
pub fn add_offers_from_trade_set<R, F>(
    trade_set: &TradeSet,
    offers: &mut MerchantOffers,
    rng: &mut R,
    mut accept_offer: F,
) where
    R: RandomSource,
    F: FnMut(&mut MerchantOffer) -> bool,
{
    let number_of_offers = trade_set.calculate_number_of_trades(rng);
    let mut potential: Vec<&VillagerTrade> = trade_set.trades.iter().collect();
    let mut offers_found = 0i32;

    while offers_found < number_of_offers && !potential.is_empty() {
        let roll = rng.next_int_bound(potential.len() as i32) as usize;
        let trade = if trade_set.allow_duplicates {
            potential[roll]
        } else {
            potential.remove(roll)
        };

        match trade.get_offer(rng) {
            None => {
                if trade_set.allow_duplicates {
                    potential.remove(roll);
                }
            }
            Some(mut offer) => {
                if accept_offer(&mut offer) {
                    offers.push(offer);
                }
                offers_found = offers_found.wrapping_add(1);
            }
        }
    }
}

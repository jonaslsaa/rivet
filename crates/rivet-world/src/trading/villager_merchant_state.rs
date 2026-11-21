//! Villager merchant state mirroring Paper 26.2 `Villager` trading, restocking,
//! demand, special-price discounts, and NBT persistence.

use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_util::mth;

use super::data_component_exact_predicate::TradingCodecError;
use super::merchant::Merchant;
use super::merchant_offer::MerchantOffer;
use super::merchant_offers::MerchantOffers;

/// Villager merchant state mirroring Paper 26.2 `Villager` trading, restocking,
/// demand, special-price discounts, and NBT persistence.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VillagerMerchantState {
    pub offers: MerchantOffers,
    pub villager_xp: i32,
    pub last_restock_game_time: i64,
    pub number_of_restocks_today: i32,
    pub last_restock_check_day: i64,
    /// Mirrors Paper's `GlobalConfiguration.get().misc.preventNegativeVillagerDemand`.
    pub prevent_negative_villager_demand: bool,
}

impl VillagerMerchantState {
    pub const RESTOCK_COOLDOWN_TICKS: i64 = 2400;
    pub const HALF_DAY_TICKS: i64 = 12000;
    pub const MAX_RESTOCKS_PER_DAY: i32 = 2;

    pub fn new(offers: MerchantOffers) -> Self {
        Self {
            offers,
            villager_xp: 0,
            last_restock_game_time: 0,
            number_of_restocks_today: 0,
            last_restock_check_day: 0,
            prevent_negative_villager_demand: false,
        }
    }

    /// Mirrors `Villager.needsToRestock()`.
    pub fn needs_to_restock(&self) -> bool {
        self.offers.iter().any(MerchantOffer::needs_restock)
    }

    /// Mirrors `Villager.allowedToRestock()`.
    pub fn allowed_to_restock(&self, game_time: i64) -> bool {
        self.number_of_restocks_today == 0
            || (self.number_of_restocks_today < Self::MAX_RESTOCKS_PER_DAY
                && game_time
                    > self
                        .last_restock_game_time
                        .wrapping_add(Self::RESTOCK_COOLDOWN_TICKS))
    }

    /// Mirrors `Villager.shouldRestock(ServerLevel)`.
    pub fn should_restock(&mut self, game_time: i64, current_day: i64) -> bool {
        self.should_restock_with_event(game_time, current_day, |_| true)
    }

    /// Mirrors `Villager.shouldRestock(ServerLevel)` with a Paper
    /// `VillagerReplenishTradeEvent` predicate invoked during `catchUpDemand()`.
    pub fn should_restock_with_event<F>(
        &mut self,
        game_time: i64,
        current_day: i64,
        allow_replenish: F,
    ) -> bool
    where
        F: FnMut(&MerchantOffer) -> bool,
    {
        let half_day_passed_time = self
            .last_restock_game_time
            .wrapping_add(Self::HALF_DAY_TICKS);
        let mut is_new_day = game_time > half_day_passed_time;
        is_new_day |= self.last_restock_check_day > 0 && current_day > self.last_restock_check_day;
        self.last_restock_check_day = current_day;
        if is_new_day {
            self.last_restock_game_time = game_time;
            self.reset_number_of_restocks(allow_replenish);
        }
        self.allowed_to_restock(game_time) && self.needs_to_restock()
    }

    /// Mirrors `Villager.resetNumberOfRestocks()`.
    fn reset_number_of_restocks<F>(&mut self, allow_replenish: F)
    where
        F: FnMut(&MerchantOffer) -> bool,
    {
        self.catch_up_demand(allow_replenish);
        self.number_of_restocks_today = 0;
    }

    /// Mirrors `Villager.updateDemand()`.
    pub fn update_demand(&mut self) {
        let prevent_neg = self.prevent_negative_villager_demand;
        for offer in self.offers.iter_mut() {
            offer.update_demand(prevent_neg);
        }
    }

    /// Mirrors `Villager.restock()`, with a Paper `VillagerReplenishTradeEvent`
    /// predicate per offer.
    pub fn restock<F>(&mut self, game_time: i64, mut allow_replenish: F)
    where
        F: FnMut(&MerchantOffer) -> bool,
    {
        self.update_demand();
        for offer in self.offers.iter_mut() {
            if allow_replenish(offer) {
                offer.reset_uses();
            }
        }
        self.last_restock_game_time = game_time;
        self.number_of_restocks_today = self.number_of_restocks_today.wrapping_add(1);
    }

    /// Mirrors `Villager.catchUpDemand()`.
    pub fn catch_up_demand<F>(&mut self, mut allow_replenish: F)
    where
        F: FnMut(&MerchantOffer) -> bool,
    {
        let missed_updates = Self::MAX_RESTOCKS_PER_DAY.wrapping_sub(self.number_of_restocks_today);
        if missed_updates > 0 {
            for offer in self.offers.iter_mut() {
                if allow_replenish(offer) {
                    offer.reset_uses();
                }
            }
        }
        for _ in 0..missed_updates {
            self.update_demand();
        }
    }

    /// Mirrors `Villager.updateSpecialPrices(Player)` (Paper 26.2):
    /// - Skips any offer with `offer.ignore_discounts == true`.
    /// - Applies reputation discount `-Mth.floor(reputation * offer.priceMultiplier)`
    ///   when `reputation != 0`.
    /// - Applies Hero of the Village discount when `hero_of_the_village_amplifier`
    ///   is `Some(amp)`:
    ///   `double modifier = 0.3 + 0.0625 * amp;`
    ///   `int costReduction = (int) Math.floor(modifier * offer.getBaseCostA().getCount());`
    ///   `offer.addToSpecialPriceDiff(-Math.max(costReduction, 1));`
    pub fn update_special_prices(
        &mut self,
        reputation: i32,
        hero_of_the_village_amplifier: Option<i32>,
    ) {
        if reputation != 0 {
            for offer in self.offers.iter_mut() {
                if offer.ignore_discounts {
                    continue;
                }
                let diff = -mth::floor(reputation as f32 * offer.price_multiplier());
                offer.add_to_special_price_diff(diff);
            }
        }

        if let Some(amplifier) = hero_of_the_village_amplifier {
            let modifier = 0.3_f64 + 0.0625_f64 * f64::from(amplifier);
            for offer in self.offers.iter_mut() {
                if offer.ignore_discounts {
                    continue;
                }
                let cost_reduction =
                    (modifier * f64::from(offer.base_cost_a().count())).floor() as i32;
                offer.add_to_special_price_diff(-cost_reduction.max(1));
            }
        }
    }

    /// Resets `special_price_diff` on all offers (`Villager.stopTrading`).
    pub fn reset_special_prices(&mut self) {
        for offer in self.offers.iter_mut() {
            offer.reset_special_price_diff();
        }
    }

    /// Saves villager trading/restock fields (`Offers`, `Xp`, `LastRestock`, `RestocksToday`).
    pub fn save_to_nbt(&self, tag: &mut CompoundTag) {
        if !self.offers.is_empty() {
            tag.put("Offers".to_string(), Tag::Compound(self.offers.to_nbt()));
        }
        tag.put_int("Xp", self.villager_xp);
        tag.put_long("LastRestock", self.last_restock_game_time);
        tag.put_int("RestocksToday", self.number_of_restocks_today);
    }

    /// Loads villager trading/restock fields from NBT.
    pub fn load_from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        let offers = match tag.get_compound("Offers") {
            Some(offers_tag) => MerchantOffers::from_nbt(offers_tag)?,
            None => MerchantOffers::new(),
        };
        Ok(Self {
            offers,
            villager_xp: tag.get_int("Xp").unwrap_or(0),
            last_restock_game_time: tag.get_long("LastRestock").unwrap_or(0),
            number_of_restocks_today: tag.get_int("RestocksToday").unwrap_or(0),
            last_restock_check_day: 0,
            prevent_negative_villager_demand: false,
        })
    }
}

impl Merchant for VillagerMerchantState {
    fn offers(&self) -> &MerchantOffers {
        &self.offers
    }

    fn offers_mut(&mut self) -> &mut MerchantOffers {
        &mut self.offers
    }

    fn notify_trade(&mut self, offer_index: usize) {
        if let Some(offer) = self.offers.get_mut(offer_index) {
            offer.increase_uses();
            // Matches Villager.rewardTradeXp: villager progression XP is always added;
            // `offer.should_reward_exp()` only gates spawning the player's ExperienceOrb.
            self.villager_xp = self.villager_xp.wrapping_add(offer.xp());
        }
    }

    fn villager_xp(&self) -> i32 {
        self.villager_xp
    }

    fn show_progress_bar(&self) -> bool {
        true
    }

    fn can_restock(&self) -> bool {
        true
    }
}

//! `TradeCost`, `VillagerTrade`, `TradeSet`, `TradeSets`, and deterministic
//! offer generation (`AbstractVillager.addOffersFromTradeSet`).

use rivet_registry::identifier::Identifier;
use rivet_util::{mth, random::RandomSource};

use super::item_cost::{DataComponentExactPredicate, ItemCost, TradeItemStack};
use super::merchant_offer::MerchantOffer;
use super::merchant_offers::MerchantOffers;

/// Loot number provider subset used by MC 26.2 `TradeCost` / `VillagerTrade` / `TradeSet`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeNumberProvider {
    /// `ConstantValue.exactly(value)`.
    Constant(f32),
    /// `UniformGenerator.between(min, max)`.
    Uniform { min: f32, max: f32 },
}

impl TradeNumberProvider {
    pub const fn exactly(value: f32) -> Self {
        Self::Constant(value)
    }

    pub const fn between(min: f32, max: f32) -> Self {
        Self::Uniform { min, max }
    }

    /// Mirrors `NumberProvider.getInt(LootContext)`.
    pub fn get_int<R: RandomSource>(&self, rng: &mut R) -> i32 {
        match *self {
            Self::Constant(v) => v.round() as i32,
            Self::Uniform { min, max } => {
                let min_i = min.round() as i32;
                let max_i = max.round() as i32;
                if min_i >= max_i {
                    min_i
                } else {
                    mth::next_int(rng, min_i, max_i)
                }
            }
        }
    }

    /// Mirrors `NumberProvider.getFloat(LootContext)`.
    pub fn get_float<R: RandomSource>(&self, rng: &mut R) -> f32 {
        match *self {
            Self::Constant(v) => v,
            Self::Uniform { min, max } => {
                if min >= max {
                    min
                } else {
                    mth::next_float(rng, min, max)
                }
            }
        }
    }
}

/// Mirrors `net.minecraft.world.item.trading.TradeCost`.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeCost {
    pub item: Identifier,
    pub count: TradeNumberProvider,
    pub components: DataComponentExactPredicate,
    pub max_stack_size: i32,
}

impl TradeCost {
    pub fn new(item: Identifier, count: TradeNumberProvider) -> Self {
        Self {
            item,
            count,
            components: DataComponentExactPredicate::empty(),
            max_stack_size: TradeItemStack::DEFAULT_MAX_STACK_SIZE,
        }
    }

    pub fn with_components(mut self, components: DataComponentExactPredicate) -> Self {
        self.components = components;
        self
    }

    pub fn with_max_stack_size(mut self, max_stack_size: i32) -> Self {
        self.max_stack_size = max_stack_size.max(1);
        self
    }

    /// Mirrors `TradeCost.toItemCost(LootContext, int additionalCost)`:
    /// ```java
    /// int count = Mth.clamp(this.count().getInt(lootContext) + additionalCost, 0, this.item().value().getDefaultMaxStackSize());
    /// return new ItemCost(this.item(), count, this.components());
    /// ```
    pub fn to_item_cost<R: RandomSource>(&self, rng: &mut R, additional_cost: i32) -> ItemCost {
        let count = mth::clamp(
            self.count.get_int(rng).wrapping_add(additional_cost),
            0,
            self.max_stack_size,
        );
        ItemCost::with_components_and_max_stack(
            self.item.clone(),
            count,
            self.components.clone(),
            self.max_stack_size,
        )
    }
}

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

/// Canonical MC 26.2 `TradeSets` resource identifiers (`net.minecraft.world.item.trading.TradeSets`).
pub struct TradeSets;

impl TradeSets {
    pub const WANDERING_TRADER_BUYING: &'static str = "wandering_trader/buying";
    pub const WANDERING_TRADER_COMMON: &'static str = "wandering_trader/common";
    pub const WANDERING_TRADER_UNCOMMON: &'static str = "wandering_trader/uncommon";

    /// Exact order used by `WanderingTrader.updateTrades`:
    /// `WANDERING_TRADER_BUYING`, `WANDERING_TRADER_UNCOMMON`, `WANDERING_TRADER_COMMON`.
    pub const WANDERING_TRADER_ORDER: [&'static str; 3] = [
        Self::WANDERING_TRADER_BUYING,
        Self::WANDERING_TRADER_UNCOMMON,
        Self::WANDERING_TRADER_COMMON,
    ];

    /// All 13 villager professions in `TradeSets.java` order.
    pub const VILLAGER_PROFESSIONS: [&'static str; 13] = [
        "armorer",
        "butcher",
        "cartographer",
        "cleric",
        "farmer",
        "fisherman",
        "fletcher",
        "leatherworker",
        "librarian",
        "mason",
        "shepherd",
        "toolsmith",
        "weaponsmith",
    ];

    /// Returns the `minecraft:<path>` identifier for a trade set path.
    pub fn id(path: &str) -> Identifier {
        Identifier::with_default_namespace(path)
    }

    /// Returns the trade set identifier for a villager profession and level (`1..=5`).
    pub fn villager_level(profession: &str, level: u8) -> Option<Identifier> {
        if !(1..=5).contains(&level) || !Self::VILLAGER_PROFESSIONS.contains(&profession) {
            return None;
        }
        let path = format!("{profession}/level_{level}");
        Some(Identifier::with_default_namespace(&path))
    }

    /// Default trade count per trade set from `TradeSets.bootstrap`:
    /// - `wandering_trader/common` -> `5.0`
    /// - `librarian/level_5` -> `3.0`
    /// - all others -> `2.0`
    pub fn default_trade_count(path: &str) -> TradeNumberProvider {
        match path {
            Self::WANDERING_TRADER_COMMON => TradeNumberProvider::exactly(5.0),
            "librarian/level_5" => TradeNumberProvider::exactly(3.0),
            _ => TradeNumberProvider::exactly(2.0),
        }
    }
}

//! `net.minecraft.world.item.trading.TradeSets`.

use rivet_registry::identifier::Identifier;

use super::trade_cost::TradeNumberProvider;

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

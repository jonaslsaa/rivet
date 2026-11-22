//! `net.minecraft.world.item.trading.TradeCost` and its loot number provider subset.

use rivet_registry::identifier::Identifier;
use rivet_util::{mth, random::RandomSource};

use super::data_component_exact_predicate::DataComponentExactPredicate;
use super::item_cost::ItemCost;
use super::trade_item_stack::TradeItemStack;

/// Loot number provider subset used by MC 26.2 `TradeCost` / `VillagerTrade` / `TradeSet`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeNumberProvider {
    /// `ConstantValue.exactly(value)`.
    Constant(f32),
    /// `UniformGenerator.between(min, max)`.
    Uniform { min: f32, max: f32 },
}

/// Mirrors Java `Math.round(float)`: ties round toward `+infinity`
/// (`Math.floor(a + 0.5)` with `NaN -> 0` and saturation to `i32::MIN..=i32::MAX`).
fn java_round_f32(v: f32) -> i32 {
    if v.is_nan() {
        return 0;
    }
    (f64::from(v) + 0.5_f64).floor() as i32
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
            Self::Constant(v) => java_round_f32(v),
            Self::Uniform { min, max } => {
                let min_i = java_round_f32(min);
                let max_i = java_round_f32(max);
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

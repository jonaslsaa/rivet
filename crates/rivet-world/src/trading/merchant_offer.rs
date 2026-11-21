//! `net.minecraft.world.item.trading.MerchantOffer`.

use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_util::mth;

use super::item_cost::{ItemCost, TradeItemStack, TradingCodecError};

/// Mirrors `net.minecraft.world.item.trading.MerchantOffer` (Paper 26.2).
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantOffer {
    pub base_cost_a: ItemCost,
    pub cost_b: Option<ItemCost>,
    result: TradeItemStack,
    pub uses: i32,
    pub max_uses: i32,
    pub reward_exp: bool,
    special_price_diff: i32,
    pub demand: i32,
    pub price_multiplier: f32,
    pub xp: i32,
    /// Paper API field (`Paper.IgnoreDiscounts` in NBT).
    pub ignore_discounts: bool,
}

impl MerchantOffer {
    /// Mirrors `MerchantOffer(ItemCost buy, ItemStack result, int maxUses, int xp, float priceMultiplier)`.
    pub fn new(
        buy: ItemCost,
        result: TradeItemStack,
        max_uses: i32,
        xp: i32,
        price_multiplier: f32,
    ) -> Self {
        Self::with_cost_b(buy, None, result, 0, max_uses, xp, price_multiplier, 0)
    }

    /// Mirrors `MerchantOffer(baseCostA, costB, result, uses, maxUses, xp, priceMultiplier, demand)`.
    #[allow(clippy::too_many_arguments)]
    pub fn with_cost_b(
        base_cost_a: ItemCost,
        cost_b: Option<ItemCost>,
        result: TradeItemStack,
        uses: i32,
        max_uses: i32,
        xp: i32,
        price_multiplier: f32,
        demand: i32,
    ) -> Self {
        Self::from_parts(
            base_cost_a,
            cost_b,
            result,
            uses,
            max_uses,
            true,
            0,
            demand,
            price_multiplier,
            xp,
            false,
        )
    }

    /// Full constructor matching `MerchantOffer.CODEC` field order.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        base_cost_a: ItemCost,
        cost_b: Option<ItemCost>,
        result: TradeItemStack,
        uses: i32,
        max_uses: i32,
        reward_exp: bool,
        special_price_diff: i32,
        demand: i32,
        price_multiplier: f32,
        xp: i32,
        ignore_discounts: bool,
    ) -> Self {
        Self {
            base_cost_a,
            cost_b,
            result,
            uses,
            max_uses,
            reward_exp,
            special_price_diff,
            demand,
            price_multiplier,
            xp,
            ignore_discounts,
        }
    }

    pub fn base_cost_a(&self) -> &TradeItemStack {
        self.base_cost_a.item_stack()
    }

    /// Mirrors `MerchantOffer.getCostA()`.
    pub fn cost_a(&self) -> TradeItemStack {
        self.base_cost_a
            .item_stack()
            .copy_with_count(self.modified_cost_count(&self.base_cost_a))
    }

    /// Mirrors `MerchantOffer.getModifiedCostCount(ItemCost)`:
    /// ```java
    /// int basePrice = cost.count();
    /// int demandDiff = Math.max(0, Mth.floor(basePrice * this.demand * this.priceMultiplier));
    /// return Mth.clamp(basePrice + demandDiff + this.specialPriceDiff, 1, cost.itemStack().getMaxStackSize());
    /// ```
    pub fn modified_cost_count(&self, cost: &ItemCost) -> i32 {
        let base_price = cost.count();
        let demand_product = base_price.wrapping_mul(self.demand) as f32 * self.price_multiplier;
        let demand_diff = 0i32.max(mth::floor(demand_product));
        mth::clamp(
            base_price
                .wrapping_add(demand_diff)
                .wrapping_add(self.special_price_diff),
            1,
            cost.item_stack().max_stack_size(),
        )
    }

    /// Mirrors `MerchantOffer.getCostB()`.
    pub fn cost_b(&self) -> TradeItemStack {
        self.cost_b
            .as_ref()
            .map(|c| c.item_stack().clone())
            .unwrap_or_else(TradeItemStack::empty)
    }

    pub fn item_cost_a(&self) -> &ItemCost {
        &self.base_cost_a
    }

    pub fn item_cost_b(&self) -> Option<&ItemCost> {
        self.cost_b.as_ref()
    }

    pub fn result(&self) -> &TradeItemStack {
        &self.result
    }

    /// Mirrors `MerchantOffer.updateDemand()` (Paper 26.2):
    /// ```java
    /// this.demand = this.demand + this.uses - (this.maxUses - this.uses);
    /// if (preventNegativeVillagerDemand) this.demand = Math.max(0, this.demand);
    /// ```
    pub fn update_demand(&mut self, prevent_negative_villager_demand: bool) {
        self.demand = self
            .demand
            .wrapping_add(self.uses)
            .wrapping_sub(self.max_uses.wrapping_sub(self.uses));
        if prevent_negative_villager_demand {
            self.demand = self.demand.max(0);
        }
    }

    /// Mirrors `MerchantOffer.assemble()`.
    pub fn assemble(&self) -> TradeItemStack {
        self.result.clone()
    }

    pub fn uses(&self) -> i32 {
        self.uses
    }

    pub fn reset_uses(&mut self) {
        self.uses = 0;
    }

    pub fn max_uses(&self) -> i32 {
        self.max_uses
    }

    pub fn increase_uses(&mut self) {
        self.uses = self.uses.wrapping_add(1);
    }

    pub fn demand(&self) -> i32 {
        self.demand
    }

    pub fn add_to_special_price_diff(&mut self, add: i32) {
        self.special_price_diff = self.special_price_diff.wrapping_add(add);
    }

    pub fn reset_special_price_diff(&mut self) {
        self.special_price_diff = 0;
    }

    pub fn special_price_diff(&self) -> i32 {
        self.special_price_diff
    }

    pub fn set_special_price_diff(&mut self, value: i32) {
        self.special_price_diff = value;
    }

    pub fn price_multiplier(&self) -> f32 {
        self.price_multiplier
    }

    pub fn xp(&self) -> i32 {
        self.xp
    }

    pub fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    pub fn set_to_out_of_stock(&mut self) {
        self.uses = self.max_uses;
    }

    pub fn needs_restock(&self) -> bool {
        self.uses > 0
    }

    pub fn should_reward_exp(&self) -> bool {
        self.reward_exp
    }

    /// Mirrors `MerchantOffer.satisfiedBy(ItemStack buyA, ItemStack buyB)`.
    pub fn satisfied_by(&self, buy_a: &TradeItemStack, buy_b: &TradeItemStack) -> bool {
        if !self.base_cost_a.test(buy_a)
            || buy_a.count() < self.modified_cost_count(&self.base_cost_a)
        {
            return false;
        }
        match &self.cost_b {
            None => buy_b.is_empty(),
            Some(cost_b) => cost_b.test(buy_b) && buy_b.count() >= cost_b.count(),
        }
    }

    /// Mirrors `MerchantOffer.take(ItemStack buyA, ItemStack buyB)`.
    pub fn take(&self, buy_a: &mut TradeItemStack, buy_b: &mut TradeItemStack) -> bool {
        if !self.satisfied_by(buy_a, buy_b) {
            return false;
        }
        let cost_a = self.cost_a();
        if !cost_a.is_empty() {
            buy_a.shrink(cost_a.count());
        }
        let cost_b = self.cost_b();
        if !cost_b.is_empty() {
            buy_b.shrink(cost_b.count());
        }
        true
    }

    /// Mirrors `MerchantOffer.copy()`.
    pub fn copy(&self) -> Self {
        self.clone()
    }

    /// Encodes as Paper 26.2 `MerchantOffer.CODEC` NBT.
    pub fn to_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put("buy".to_string(), Tag::Compound(self.base_cost_a.to_nbt()));
        if let Some(cost_b) = &self.cost_b {
            tag.put("buyB".to_string(), Tag::Compound(cost_b.to_nbt()));
        }
        tag.put("sell".to_string(), Tag::Compound(self.result.to_nbt()));
        tag.put_int("uses", self.uses);
        tag.put_int("maxUses", self.max_uses);
        tag.put_boolean("rewardExp", self.reward_exp);
        tag.put_int("specialPrice", self.special_price_diff);
        tag.put_int("demand", self.demand);
        tag.put_float("priceMultiplier", self.price_multiplier);
        tag.put_int("xp", self.xp);
        tag.put_boolean("Paper.IgnoreDiscounts", self.ignore_discounts);
        tag
    }

    /// Decodes from Paper 26.2 `MerchantOffer.CODEC` NBT with exact defaults.
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        let buy_tag = tag
            .get_compound("buy")
            .ok_or(TradingCodecError::MissingField("buy"))?;
        let base_cost_a = ItemCost::from_nbt(buy_tag)?;

        let cost_b = match tag.get_compound("buyB") {
            Some(buy_b_tag) => Some(ItemCost::from_nbt(buy_b_tag)?),
            None => None,
        };

        let sell_tag = tag
            .get_compound("sell")
            .ok_or(TradingCodecError::MissingField("sell"))?;
        let result = TradeItemStack::from_nbt(sell_tag)?;

        let uses = tag.get_int("uses").unwrap_or(0);
        let max_uses = tag.get_int("maxUses").unwrap_or(4);
        let reward_exp = tag.get_boolean("rewardExp").unwrap_or(true);
        let special_price_diff = tag.get_int("specialPrice").unwrap_or(0);
        let demand = tag.get_int("demand").unwrap_or(0);
        let price_multiplier = tag.get_float("priceMultiplier").unwrap_or(0.0);
        let xp = tag.get_int("xp").unwrap_or(1);
        let ignore_discounts = tag.get_boolean("Paper.IgnoreDiscounts").unwrap_or(false);

        Ok(Self::from_parts(
            base_cost_a,
            cost_b,
            result,
            uses,
            max_uses,
            reward_exp,
            special_price_diff,
            demand,
            price_multiplier,
            xp,
            ignore_discounts,
        ))
    }
}

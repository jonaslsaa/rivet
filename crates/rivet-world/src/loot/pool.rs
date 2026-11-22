//! Port of `net.minecraft.world.level.storage.loot.LootPool`.
//!
//! Java source (Paper 26.2):
//! - `net/minecraft/world/level/storage/loot/LootPool.java`

use rivet_util::mth;
use rivet_util::random::RandomSource;

use super::context::LootContext;
use super::entries::{ExpandedLootPoolEntry, LootPoolEntryContainer};
use super::functions::LootItemFunction;
use super::item_stack::LootItemStack;
use super::predicates::LootItemCondition;
use super::providers::NumberProvider;
use super::validation::{Validatable, ValidationContext, validate_field, validate_list};

/// `net.minecraft.world.level.storage.loot.LootPool`.
#[derive(Debug, Clone, PartialEq)]
pub struct LootPool {
    entries: Vec<LootPoolEntryContainer>,
    conditions: Vec<LootItemCondition>,
    functions: Vec<LootItemFunction>,
    rolls: NumberProvider,
    bonus_rolls: NumberProvider,
}

impl LootPool {
    /// Starts a [`LootPoolBuilder`] (`LootPool.lootPool()`).
    #[must_use]
    pub fn builder() -> LootPoolBuilder {
        LootPoolBuilder::new()
    }

    /// Pool entries in list order.
    #[must_use]
    pub fn entries(&self) -> &[LootPoolEntryContainer] {
        &self.entries
    }

    /// Pool conditions in list order.
    #[must_use]
    pub fn conditions(&self) -> &[LootItemCondition] {
        &self.conditions
    }

    /// Pool functions in list order.
    #[must_use]
    pub fn functions(&self) -> &[LootItemFunction] {
        &self.functions
    }

    /// Base roll count provider.
    #[must_use]
    pub fn rolls(&self) -> &NumberProvider {
        &self.rolls
    }

    /// Bonus roll count provider (scaled by `context.luck()`).
    #[must_use]
    pub fn bonus_rolls(&self) -> &NumberProvider {
        &self.bonus_rolls
    }

    /// `LootPool.addRandomItem(Consumer<ItemStack>, LootContext)` — expands
    /// entries in list order, filters out `weight <= 0`, skips `next_int` when
    /// only 1 valid entry exists, otherwise selects by `next_int_bound(total_weight)`.
    fn add_random_item(
        &self,
        result: &mut dyn FnMut(LootItemStack, &mut LootContext<'_>),
        context: &mut LootContext<'_>,
    ) {
        let mut valid_entries: Vec<ExpandedLootPoolEntry<'_>> = Vec::new();
        let mut total_weight: i32 = 0;

        let luck = context.luck();
        for entry in &self.entries {
            entry.expand(context, &mut |expanded| {
                let weight = expanded.weight(luck);
                if weight > 0 {
                    valid_entries.push(expanded);
                    total_weight = total_weight.wrapping_add(weight);
                }
            });
        }

        let entry_count = valid_entries.len();
        if total_weight != 0 && entry_count != 0 {
            if entry_count == 1 {
                valid_entries[0].create_item_stack(result, context);
            } else {
                let mut index = context.random_mut().next_int_bound(total_weight);
                for entry in &valid_entries {
                    index = index.wrapping_sub(entry.weight(context.luck()));
                    if index < 0 {
                        entry.create_item_stack(result, context);
                        return;
                    }
                }
            }
        }
    }

    /// `LootPool.addRandomItems(Consumer<ItemStack>, LootContext)` — evaluates
    /// pool conditions BEFORE consuming roll/bonus-roll randomness, then runs
    /// `count` rolls with output decorated by `self.functions`.
    pub fn add_random_items(
        &self,
        result: &mut dyn FnMut(LootItemStack, &mut LootContext<'_>),
        context: &mut LootContext<'_>,
    ) {
        if LootItemCondition::test_all(&self.conditions, context) {
            let functions = &self.functions;
            let mut decorated = |stack: LootItemStack, ctx: &mut LootContext<'_>| {
                let transformed = LootItemFunction::apply_all(functions, stack, ctx);
                result(transformed, ctx);
            };

            let base_rolls = self.rolls.get_int(context);
            let bonus = mth::floor(self.bonus_rolls.get_float(context) * context.luck());
            let count = base_rolls.wrapping_add(bonus);

            for _ in 0..count {
                self.add_random_item(&mut decorated, context);
            }
        }
    }
}

impl Validatable for LootPool {
    /// `LootPool.validate(ValidationContext)` — exact Paper order:
    /// `conditions`, `functions`, `entries`, `rolls`, `bonus_rolls`.
    fn validate(&self, context: &ValidationContext<'_>) {
        validate_list(context, "conditions", &self.conditions);
        validate_list(context, "functions", &self.functions);
        validate_list(context, "entries", &self.entries);
        validate_field(context, "rolls", &self.rolls);
        validate_field(context, "bonus_rolls", &self.bonus_rolls);
    }
}

/// Builder for [`LootPool`], mirroring `LootPool.Builder`.
#[derive(Debug, Clone)]
pub struct LootPoolBuilder {
    entries: Vec<LootPoolEntryContainer>,
    conditions: Vec<LootItemCondition>,
    functions: Vec<LootItemFunction>,
    rolls: NumberProvider,
    bonus_rolls: NumberProvider,
}

impl Default for LootPoolBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LootPoolBuilder {
    /// Creates a builder with `rolls = ConstantValue(1.0)` and `bonus_rolls = ConstantValue(0.0)`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            conditions: Vec::new(),
            functions: Vec::new(),
            rolls: NumberProvider::constant(1.0),
            bonus_rolls: NumberProvider::constant(0.0),
        }
    }

    /// `Builder.setRolls(NumberProvider)`.
    #[must_use]
    pub fn set_rolls(mut self, rolls: NumberProvider) -> Self {
        self.rolls = rolls;
        self
    }

    /// `Builder.setBonusRolls(NumberProvider)`.
    #[must_use]
    pub fn set_bonus_rolls(mut self, bonus_rolls: NumberProvider) -> Self {
        self.bonus_rolls = bonus_rolls;
        self
    }

    /// `Builder.add(LootPoolEntryContainer)`.
    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn add(mut self, entry: LootPoolEntryContainer) -> Self {
        self.entries.push(entry);
        self
    }

    /// `Builder.when(LootItemCondition)`.
    #[must_use]
    pub fn when(mut self, condition: LootItemCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    /// `Builder.apply(LootItemFunction)`.
    #[must_use]
    pub fn apply(mut self, function: LootItemFunction) -> Self {
        self.functions.push(function);
        self
    }

    /// Builds the [`LootPool`].
    #[must_use]
    pub fn build(self) -> LootPool {
        LootPool {
            entries: self.entries,
            conditions: self.conditions,
            functions: self.functions,
            rolls: self.rolls,
            bonus_rolls: self.bonus_rolls,
        }
    }
}

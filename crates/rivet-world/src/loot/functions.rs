//! Port of `net.minecraft.world.level.storage.loot.functions.*`.
//!
//! Java source (Paper 26.2):
//! - `functions/LootItemFunction.java`
//! - `functions/LootItemConditionalFunction.java`
//! - `functions/LootItemFunctions.java`
//! - `functions/SetItemCountFunction.java`
//! - `functions/LimitCount.java`
//! - `functions/ApplyExplosionDecay.java`
//! - `functions/ApplyBonusCount.java`
//! - `functions/SequenceFunction.java`
//! - `functions/FunctionReference.java`

use indexmap::IndexSet;
use rivet_registry::Identifier;
use rivet_util::random::RandomSource;

use super::context::{LootContext, VisitedEntry};
use super::int_range::IntRange;
use super::item_stack::LootItemStack;
use super::parameters::{ContextKey, LootContextParams};
use super::predicates::LootItemCondition;
use super::providers::NumberProvider;
use super::validation::{
    LootContextUser, LootReferenceKey, Validatable, ValidationContext, validate_field,
    validate_list,
};

/// `ApplyBonusCount.Formula` — exact Paper 26.2 bonus-count formulas.
#[derive(Debug, Clone, PartialEq)]
pub enum BonusFormula {
    /// `ApplyBonusCount.BinomialWithBonusCount`:
    /// loops `0..(enchantment_level + extra_rounds)` and increments count when
    /// `random.nextFloat() < probability`.
    BinomialWithBonusCount { extra_rounds: i32, probability: f32 },
    /// `ApplyBonusCount.OreDrops`:
    /// if `enchantment_level > 0`, `i = random.nextInt(enchantment_level + 2) - 1`;
    /// if `i < 0 { i = 0 }`; returns `count * (i + 1)`.
    OreDrops,
    /// `ApplyBonusCount.UniformBonusCount`:
    /// returns `count + random.nextInt(bonus_multiplier * enchantment_level + 1)`.
    UniformBonusCount { bonus_multiplier: i32 },
}

impl BonusFormula {
    /// `Formula.calculateNewCount(RandomSource, int itemStackCount, int enchantmentLevel)`.
    pub fn calculate_new_count<R: RandomSource>(
        &self,
        random: &mut R,
        mut item_stack_count: i32,
        enchantment_level: i32,
    ) -> i32 {
        match *self {
            Self::BinomialWithBonusCount {
                extra_rounds,
                probability,
            } => {
                let rounds = enchantment_level.wrapping_add(extra_rounds);
                for _ in 0..rounds {
                    if random.next_float() < probability {
                        item_stack_count = item_stack_count.wrapping_add(1);
                    }
                }
                item_stack_count
            }
            Self::OreDrops => {
                if enchantment_level > 0 {
                    let mut bonus = random
                        .next_int_bound(enchantment_level.wrapping_add(2))
                        .wrapping_sub(1);
                    if bonus < 0 {
                        bonus = 0;
                    }
                    item_stack_count.wrapping_mul(bonus.wrapping_add(1))
                } else {
                    item_stack_count
                }
            }
            Self::UniformBonusCount { bonus_multiplier } => {
                let bound = bonus_multiplier
                    .wrapping_mul(enchantment_level)
                    .wrapping_add(1);
                item_stack_count.wrapping_add(random.next_int_bound(bound))
            }
        }
    }
}

/// `net.minecraft.world.level.storage.loot.functions.LootItemFunction`.
#[derive(Debug, Clone, PartialEq)]
pub enum LootItemFunction {
    /// `SetItemCountFunction`.
    SetCount {
        conditions: Vec<LootItemCondition>,
        value: NumberProvider,
        add: bool,
    },
    /// `LimitCount`.
    LimitCount {
        conditions: Vec<LootItemCondition>,
        limit: IntRange,
    },
    /// `ApplyExplosionDecay`.
    ExplosionDecay { conditions: Vec<LootItemCondition> },
    /// `ApplyBonusCount`.
    ApplyBonus {
        conditions: Vec<LootItemCondition>,
        enchantment: Identifier,
        formula: BonusFormula,
    },
    /// `SequenceFunction`.
    Sequence(Vec<LootItemFunction>),
    /// `FunctionReference`.
    Reference {
        conditions: Vec<LootItemCondition>,
        name: Identifier,
    },
}

impl LootItemFunction {
    /// `SetItemCountFunction.setCount(NumberProvider)` (`add = false`).
    #[must_use]
    pub fn set_count(value: NumberProvider) -> Self {
        Self::SetCount {
            conditions: Vec::new(),
            value,
            add: false,
        }
    }

    /// `SetItemCountFunction.setCount(NumberProvider, boolean add)`.
    #[must_use]
    pub fn set_count_with_add(value: NumberProvider, add: bool) -> Self {
        Self::SetCount {
            conditions: Vec::new(),
            value,
            add,
        }
    }

    /// `LimitCount.limitCount(IntRange)`.
    #[must_use]
    pub fn limit_count(limit: IntRange) -> Self {
        Self::LimitCount {
            conditions: Vec::new(),
            limit,
        }
    }

    /// `ApplyExplosionDecay.explosionDecay()`.
    #[must_use]
    pub fn explosion_decay() -> Self {
        Self::ExplosionDecay {
            conditions: Vec::new(),
        }
    }

    /// `ApplyBonusCount.addOreBonusCount(enchantment)`.
    #[must_use]
    pub fn apply_ore_drops(enchantment: Identifier) -> Self {
        Self::ApplyBonus {
            conditions: Vec::new(),
            enchantment,
            formula: BonusFormula::OreDrops,
        }
    }

    /// `ApplyBonusCount.addUniformBonusCount(enchantment, bonusMultiplier)`.
    #[must_use]
    pub fn apply_uniform_bonus(enchantment: Identifier, bonus_multiplier: i32) -> Self {
        Self::ApplyBonus {
            conditions: Vec::new(),
            enchantment,
            formula: BonusFormula::UniformBonusCount { bonus_multiplier },
        }
    }

    /// `ApplyBonusCount.addBonusBinomialDistributionCount(enchantment, probability, extraRounds)`.
    #[must_use]
    pub fn apply_binomial_bonus(
        enchantment: Identifier,
        probability: f32,
        extra_rounds: i32,
    ) -> Self {
        Self::ApplyBonus {
            conditions: Vec::new(),
            enchantment,
            formula: BonusFormula::BinomialWithBonusCount {
                extra_rounds,
                probability,
            },
        }
    }

    /// `SequenceFunction.of(List<LootItemFunction>)`.
    #[must_use]
    pub fn sequence(functions: Vec<LootItemFunction>) -> Self {
        Self::Sequence(functions)
    }

    /// `FunctionReference.functionReference(ResourceKey)`.
    #[must_use]
    pub fn reference(name: Identifier) -> Self {
        Self::Reference {
            conditions: Vec::new(),
            name,
        }
    }

    /// Attaches an additional condition to a conditional function.
    #[must_use]
    pub fn when(mut self, condition: LootItemCondition) -> Self {
        match &mut self {
            Self::SetCount { conditions, .. }
            | Self::LimitCount { conditions, .. }
            | Self::ExplosionDecay { conditions }
            | Self::ApplyBonus { conditions, .. }
            | Self::Reference { conditions, .. } => {
                conditions.push(condition);
            }
            Self::Sequence(_) => {
                panic!("SequenceFunction does not carry top-level conditions");
            }
        }
        self
    }

    /// Applies a slice of functions in list order (`LootItemFunctions.compose`).
    pub fn apply_all(
        functions: &[LootItemFunction],
        mut stack: LootItemStack,
        context: &mut LootContext<'_>,
    ) -> LootItemStack {
        for func in functions {
            stack = func.apply(stack, context);
        }
        stack
    }

    /// `LootItemFunction.apply(ItemStack, LootContext)`.
    pub fn apply(&self, mut stack: LootItemStack, context: &mut LootContext<'_>) -> LootItemStack {
        match self {
            Self::SetCount {
                conditions,
                value,
                add,
            } => {
                if !LootItemCondition::test_all(conditions, context) {
                    return stack;
                }
                let base = if *add { stack.count() } else { 0 };
                let delta = value.get_int(context);
                stack.set_count(base.wrapping_add(delta));
                stack
            }
            Self::LimitCount { conditions, limit } => {
                if !LootItemCondition::test_all(conditions, context) {
                    return stack;
                }
                let clamped = limit.clamp(context, stack.count());
                stack.set_count(clamped);
                stack
            }
            Self::ExplosionDecay { conditions } => {
                if !LootItemCondition::test_all(conditions, context) {
                    return stack;
                }
                let Some(radius) = context
                    .params()
                    .params()
                    .get_float(&LootContextParams::explosion_radius())
                else {
                    return stack;
                };
                let survival_chance = 1.0_f32 / radius;
                let count = stack.count();
                let mut surviving: i32 = 0;
                for _ in 0..count {
                    if context.random_mut().next_float() <= survival_chance {
                        surviving = surviving.wrapping_add(1);
                    }
                }
                stack.set_count(surviving);
                stack
            }
            Self::ApplyBonus {
                conditions,
                enchantment,
                formula,
            } => {
                if !LootItemCondition::test_all(conditions, context) {
                    return stack;
                }
                let Some(tool) = context
                    .params()
                    .params()
                    .get_tool(&LootContextParams::tool())
                else {
                    return stack;
                };
                let level = tool.enchantment_level(enchantment);
                let new_count =
                    formula.calculate_new_count(context.random_mut(), stack.count(), level);
                stack.set_count(new_count);
                stack
            }
            Self::Sequence(functions) => Self::apply_all(functions, stack, context),
            Self::Reference { conditions, name } => {
                if !LootItemCondition::test_all(conditions, context) {
                    return stack;
                }
                let Some(referenced) = context.resolver().get_function(name).cloned() else {
                    return stack;
                };
                let breadcrumb = VisitedEntry::FunctionId(name.clone());
                if context.push_visited_element(breadcrumb.clone()) {
                    let result = referenced.apply(stack, context);
                    context.pop_visited_element(&breadcrumb);
                    result
                } else {
                    stack
                }
            }
        }
    }
}

impl Validatable for LootItemFunction {
    fn validate(&self, context: &ValidationContext<'_>) {
        context.validate_context_usage(self);
        match self {
            Self::SetCount {
                conditions, value, ..
            } => {
                validate_list(context, "conditions", conditions);
                validate_field(context, "count", value);
            }
            Self::LimitCount { conditions, limit } => {
                validate_list(context, "conditions", conditions);
                validate_field(context, "limit", limit);
            }
            Self::ExplosionDecay { conditions } | Self::ApplyBonus { conditions, .. } => {
                validate_list(context, "conditions", conditions);
            }
            Self::Sequence(functions) => {
                validate_list(context, "functions", functions);
            }
            Self::Reference { conditions, name } => {
                validate_list(context, "conditions", conditions);
                context.validate_reference(LootReferenceKey::Function(name.clone()));
            }
        }
    }
}

impl LootContextUser for LootItemFunction {
    fn referenced_context_params(&self) -> IndexSet<ContextKey> {
        let mut set = IndexSet::new();
        match self {
            Self::SetCount { value, .. } => {
                set.extend(value.referenced_context_params());
            }
            Self::LimitCount { limit, .. } => {
                set.extend(limit.referenced_context_params());
            }
            Self::ExplosionDecay { .. } => {}
            Self::ApplyBonus { .. } => {
                set.insert(LootContextParams::tool());
            }
            Self::Sequence(_) | Self::Reference { .. } => {}
        }
        set
    }
}

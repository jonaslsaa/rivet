//! Port of `net.minecraft.world.level.storage.loot.predicates.*`.
//!
//! Java source (Paper 26.2):
//! - `predicates/LootItemCondition.java`
//! - `predicates/CompositeLootItemCondition.java`
//! - `predicates/AllOfCondition.java`
//! - `predicates/AnyOfCondition.java`
//! - `predicates/InvertedLootItemCondition.java`
//! - `predicates/LootItemRandomChanceCondition.java`
//! - `predicates/ExplosionCondition.java`
//! - `predicates/ConditionReference.java`
//! - `predicates/ValueCheckCondition.java`
//! - `predicates/BonusLevelTableCondition.java`

use indexmap::IndexSet;
use rivet_registry::Identifier;
use rivet_util::random::RandomSource;

use super::context::{LootContext, VisitedEntry};
use super::int_range::IntRange;
use super::parameters::{ContextKey, LootContextParams};
use super::providers::NumberProvider;
use super::validation::{
    LootContextUser, LootReferenceKey, Validatable, ValidationContext, validate_field,
    validate_list,
};

/// `net.minecraft.world.level.storage.loot.predicates.LootItemCondition`.
#[derive(Debug, Clone, PartialEq)]
pub enum LootItemCondition {
    /// `AllOfCondition` — short-circuit logical AND over `terms` in order.
    AllOf(Vec<LootItemCondition>),
    /// `AnyOfCondition` — short-circuit logical OR over `terms` in order.
    AnyOf(Vec<LootItemCondition>),
    /// `InvertedLootItemCondition` — logical NOT of `term`.
    Inverted(Box<LootItemCondition>),
    /// `LootItemRandomChanceCondition` — `random.nextFloat() < chance.getFloat(ctx)`.
    RandomChance(NumberProvider),
    /// `ExplosionCondition` — if `EXPLOSION_RADIUS` is absent, returns `true`
    /// without consuming RNG; otherwise `random.nextFloat() <= 1.0F / radius`.
    SurvivesExplosion,
    /// `ConditionReference` — resolves a predicate from `LootRegistry` with
    /// runtime visited-breadcrumb cycle detection.
    Reference(Identifier),
    /// `ValueCheckCondition` — `range.test(ctx, value.getInt(ctx))`.
    ValueCheck {
        value: NumberProvider,
        range: IntRange,
    },
    /// `BonusLevelTableCondition` — reads `enchantment` level from optional
    /// `TOOL` (default 0), clamps index to `values.len() - 1`, tests
    /// `random.nextFloat() < values[idx]`.
    TableBonus {
        enchantment: Identifier,
        values: Vec<f32>,
    },
    /// Checks whether a specific [`ContextKey`] is present in the context.
    ParamPresent(ContextKey),
}

impl LootItemCondition {
    /// `LootItemRandomChanceCondition.randomChance(float)`.
    #[must_use]
    pub fn random_chance(probability: f32) -> Self {
        Self::RandomChance(NumberProvider::constant(probability))
    }

    /// `LootItemRandomChanceCondition.randomChance(NumberProvider)`.
    #[must_use]
    pub fn random_chance_provider(provider: NumberProvider) -> Self {
        Self::RandomChance(provider)
    }

    /// `ExplosionCondition.survivesExplosion()`.
    #[must_use]
    pub fn survives_explosion() -> Self {
        Self::SurvivesExplosion
    }

    /// `ConditionReference.conditionReference(ResourceKey)`.
    #[must_use]
    pub fn reference(name: Identifier) -> Self {
        Self::Reference(name)
    }

    /// `InvertedLootItemCondition.invert(...)`.
    #[must_use]
    pub fn inverted(term: LootItemCondition) -> Self {
        Self::Inverted(Box::new(term))
    }

    /// `ValueCheckCondition.hasValue(NumberProvider, IntRange)`.
    #[must_use]
    pub fn value_check(value: NumberProvider, range: IntRange) -> Self {
        Self::ValueCheck { value, range }
    }

    /// `BonusLevelTableCondition.bonusLevelFlatChance(enchantment, values)`.
    ///
    /// # Panics
    /// Panics if `values` is empty (matching Paper's `ExtraCodecs.nonEmptyList`).
    #[must_use]
    pub fn table_bonus(enchantment: Identifier, values: Vec<f32>) -> Self {
        assert!(!values.is_empty(), "TableBonus values must be non-empty");
        Self::TableBonus {
            enchantment,
            values,
        }
    }

    /// Evaluates a slice of conditions as `Util.allOf(conditions)` (short-circuit AND).
    pub fn test_all(conditions: &[LootItemCondition], context: &mut LootContext<'_>) -> bool {
        for condition in conditions {
            if !condition.test(context) {
                return false;
            }
        }
        true
    }

    /// `LootItemCondition.test(LootContext)`.
    pub fn test(&self, context: &mut LootContext<'_>) -> bool {
        match self {
            Self::AllOf(terms) => Self::test_all(terms, context),
            Self::AnyOf(terms) => {
                for term in terms {
                    if term.test(context) {
                        return true;
                    }
                }
                false
            }
            Self::Inverted(term) => !term.test(context),
            Self::RandomChance(chance) => {
                let probability = chance.get_float(context);
                context.random_mut().next_float() < probability
            }
            Self::SurvivesExplosion => {
                let Some(radius) = context
                    .params()
                    .params()
                    .get_float(&LootContextParams::explosion_radius())
                else {
                    return true;
                };
                let probability = 1.0_f32 / radius;
                context.random_mut().next_float() <= probability
            }
            Self::Reference(name) => {
                let Some(referenced) = context.resolver().get_condition(name).cloned() else {
                    return false;
                };
                let breadcrumb = VisitedEntry::ConditionId(name.clone());
                if context.push_visited_element(breadcrumb.clone()) {
                    let result = referenced.test(context);
                    context.pop_visited_element(&breadcrumb);
                    result
                } else {
                    false
                }
            }
            Self::ValueCheck { value, range } => {
                let v = value.get_int(context);
                range.test(context, v)
            }
            Self::TableBonus {
                enchantment,
                values,
            } => {
                let level = context
                    .params()
                    .params()
                    .get_tool(&LootContextParams::tool())
                    .map_or(0, |tool| tool.enchantment_level(enchantment));
                let idx = (level.max(0) as usize).min(values.len() - 1);
                let chance = values[idx];
                context.random_mut().next_float() < chance
            }
            Self::ParamPresent(key) => context.has_parameter(key),
        }
    }
}

impl Validatable for LootItemCondition {
    fn validate(&self, context: &ValidationContext<'_>) {
        context.validate_context_usage(self);
        match self {
            Self::AllOf(terms) | Self::AnyOf(terms) => {
                validate_list(context, "terms", terms);
            }
            Self::Inverted(term) => {
                validate_field(context, "term", term.as_ref());
            }
            Self::RandomChance(chance) => {
                validate_field(context, "chance", chance);
            }
            Self::SurvivesExplosion | Self::TableBonus { .. } | Self::ParamPresent(_) => {}
            Self::Reference(name) => {
                context.validate_reference(LootReferenceKey::Condition(name.clone()));
            }
            Self::ValueCheck { value, range } => {
                validate_field(context, "value", value);
                validate_field(context, "range", range);
            }
        }
    }
}

impl LootContextUser for LootItemCondition {
    fn referenced_context_params(&self) -> IndexSet<ContextKey> {
        let mut set = IndexSet::new();
        match self {
            Self::AllOf(_) | Self::AnyOf(_) | Self::Reference(_) => {}
            Self::Inverted(term) => {
                set.extend(term.referenced_context_params());
            }
            Self::RandomChance(chance) => {
                set.extend(chance.referenced_context_params());
            }
            Self::SurvivesExplosion => {
                set.insert(LootContextParams::explosion_radius());
            }
            Self::ValueCheck { value, range } => {
                set.extend(value.referenced_context_params());
                set.extend(range.referenced_context_params());
            }
            Self::TableBonus { .. } => {
                set.insert(LootContextParams::tool());
            }
            Self::ParamPresent(key) => {
                set.insert(key.clone());
            }
        }
        set
    }
}

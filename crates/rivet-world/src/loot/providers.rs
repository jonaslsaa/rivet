//! Port of `net.minecraft.world.level.storage.loot.providers.number.*`.
//!
//! Java source (Paper 26.2):
//! - `providers/number/NumberProvider.java`
//! - `providers/number/ConstantValue.java`
//! - `providers/number/UniformGenerator.java`
//! - `providers/number/BinomialDistributionGenerator.java`
//! - `providers/number/ScoreboardValue.java`

use indexmap::IndexSet;
use rivet_util::mth;
use rivet_util::random::RandomSource;

use super::context::LootContext;
use super::java_round_f32;
use super::parameters::{ContextKey, LootContextParams};
use super::validation::{LootContextUser, Validatable, ValidationContext, validate_field};

/// Target entity/holder selector for [`NumberProvider::Score`]
/// (`net.minecraft.world.level.storage.loot.providers.score.ScoreboardNameProvider`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreTarget {
    /// `FixedScoreboardNameProvider` — fixed holder name.
    Fixed(String),
    /// `ContextScoreboardNameProvider` — entity holder from a loot context key
    /// (e.g. `THIS_ENTITY`, `ATTACKING_ENTITY`, `LAST_DAMAGE_PLAYER`).
    Context(ContextKey),
}

impl ScoreTarget {
    /// Resolves the scoreboard holder name from `context`.
    #[must_use]
    pub fn resolve<'a>(&'a self, context: &'a LootContext<'_>) -> Option<&'a str> {
        match self {
            Self::Fixed(name) => Some(name.as_str()),
            Self::Context(key) => context.params().params().get_entity(key),
        }
    }

    /// Context parameters referenced by this score target.
    #[must_use]
    pub fn referenced_context_params(&self) -> IndexSet<ContextKey> {
        let mut set = IndexSet::new();
        if let Self::Context(key) = self {
            set.insert(key.clone());
        }
        set
    }
}

/// `net.minecraft.world.level.storage.loot.providers.number.NumberProvider`.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberProvider {
    /// `ConstantValue`.
    Constant(f32),
    /// `UniformGenerator` (`min`, `max`).
    Uniform {
        min: Box<NumberProvider>,
        max: Box<NumberProvider>,
    },
    /// `BinomialDistributionGenerator` (`n`, `p`).
    Binomial {
        n: Box<NumberProvider>,
        p: Box<NumberProvider>,
    },
    /// `ScoreboardValue` (`target`, `score`, `scale`).
    Score {
        target: ScoreTarget,
        score: String,
        scale: f32,
    },
    /// `EnchantmentLevelProvider` — linear `base + per_level_above_first * (level - 1)`.
    EnchantmentLevel {
        base: f32,
        per_level_above_first: f32,
    },
}

impl NumberProvider {
    /// `ConstantValue.exactly(float)`.
    #[must_use]
    pub fn constant(value: f32) -> Self {
        Self::Constant(value)
    }

    /// `UniformGenerator.between(float, float)`.
    #[must_use]
    pub fn uniform_between(min: f32, max: f32) -> Self {
        Self::Uniform {
            min: Box::new(Self::Constant(min)),
            max: Box::new(Self::Constant(max)),
        }
    }

    /// `UniformGenerator` with arbitrary nested `min`/`max` providers.
    #[must_use]
    pub fn uniform(min: NumberProvider, max: NumberProvider) -> Self {
        Self::Uniform {
            min: Box::new(min),
            max: Box::new(max),
        }
    }

    /// `BinomialDistributionGenerator.binomial(int, float)`.
    #[must_use]
    pub fn binomial(n: i32, p: f32) -> Self {
        Self::Binomial {
            n: Box::new(Self::Constant(n as f32)),
            p: Box::new(Self::Constant(p)),
        }
    }

    /// `BinomialDistributionGenerator` with arbitrary `n`/`p` providers.
    #[must_use]
    pub fn binomial_providers(n: NumberProvider, p: NumberProvider) -> Self {
        Self::Binomial {
            n: Box::new(n),
            p: Box::new(p),
        }
    }

    /// `ScoreboardValue`.
    #[must_use]
    pub fn score(target: ScoreTarget, score: impl Into<String>, scale: f32) -> Self {
        Self::Score {
            target,
            score: score.into(),
            scale,
        }
    }

    /// `NumberProvider.getFloat(LootContext)`.
    pub fn get_float(&self, context: &mut LootContext<'_>) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Uniform { min, max } => {
                let min_v = min.get_float(context);
                let max_v = max.get_float(context);
                mth::next_float(context.random_mut(), min_v, max_v)
            }
            Self::Binomial { .. } => self.get_int(context) as f32,
            Self::Score {
                target,
                score,
                scale,
            } => {
                let Some(holder) = target.resolve(context) else {
                    return 0.0;
                };
                match context.params().get_score(holder, score) {
                    Some(val) => val as f32 * *scale,
                    None => 0.0,
                }
            }
            Self::EnchantmentLevel {
                base,
                per_level_above_first,
            } => {
                let level = context
                    .params()
                    .params()
                    .get_int(&LootContextParams::enchantment_level())
                    .unwrap_or(0);
                *base + *per_level_above_first * (level.wrapping_sub(1)) as f32
            }
        }
    }

    /// `NumberProvider.getInt(LootContext)` — defaults to Java `Math.round(getFloat(ctx))`,
    /// overridden by `UniformGenerator` and `BinomialDistributionGenerator` to match Paper.
    pub fn get_int(&self, context: &mut LootContext<'_>) -> i32 {
        match self {
            Self::Uniform { min, max } => {
                let min_v = min.get_int(context);
                let max_v = max.get_int(context);
                mth::next_int(context.random_mut(), min_v, max_v)
            }
            Self::Binomial { n, p } => {
                let trials = n.get_int(context);
                let probability = p.get_float(context);
                let mut result: i32 = 0;
                for _ in 0..trials {
                    if context.random_mut().next_float() < probability {
                        result = result.wrapping_add(1);
                    }
                }
                result
            }
            Self::Constant(_) | Self::Score { .. } | Self::EnchantmentLevel { .. } => {
                java_round_f32(self.get_float(context))
            }
        }
    }
}

impl Validatable for NumberProvider {
    fn validate(&self, context: &ValidationContext<'_>) {
        context.validate_context_usage(self);
        match self {
            Self::Constant(_) | Self::Score { .. } | Self::EnchantmentLevel { .. } => {}
            Self::Uniform { min, max } => {
                validate_field(context, "min", min.as_ref());
                validate_field(context, "max", max.as_ref());
            }
            Self::Binomial { n, p } => {
                validate_field(context, "n", n.as_ref());
                validate_field(context, "p", p.as_ref());
            }
        }
    }
}

impl LootContextUser for NumberProvider {
    fn referenced_context_params(&self) -> IndexSet<ContextKey> {
        let mut set = IndexSet::new();
        match self {
            Self::Constant(_) => {}
            Self::Uniform { min, max } => {
                set.extend(min.referenced_context_params());
                set.extend(max.referenced_context_params());
            }
            Self::Binomial { n, p } => {
                set.extend(n.referenced_context_params());
                set.extend(p.referenced_context_params());
            }
            Self::Score { target, .. } => {
                set.extend(target.referenced_context_params());
            }
            Self::EnchantmentLevel { .. } => {
                set.insert(LootContextParams::enchantment_level());
            }
        }
        set
    }
}

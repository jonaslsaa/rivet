//! Port of `net.minecraft.world.level.storage.loot.IntRange`.
//!
//! Java source (Paper 26.2):
//! - `net/minecraft/world/level/storage/loot/IntRange.java`

use indexmap::IndexSet;
use rivet_util::mth;

use super::context::LootContext;
use super::parameters::ContextKey;
use super::providers::NumberProvider;
use super::validation::{LootContextUser, Validatable, ValidationContext, validate_optional_field};

/// `net.minecraft.world.level.storage.loot.IntRange` — an optional lower/upper
/// bound evaluated dynamically from [`NumberProvider`]s on each clamp/test call.
#[derive(Debug, Clone, PartialEq)]
pub struct IntRange {
    min: Option<NumberProvider>,
    max: Option<NumberProvider>,
}

impl IntRange {
    /// Creates an `IntRange` with optional `min` and `max` providers.
    #[must_use]
    pub fn new(min: Option<NumberProvider>, max: Option<NumberProvider>) -> Self {
        Self { min, max }
    }

    /// `IntRange.exact(int)` — `[value, value]`.
    #[must_use]
    pub fn exact(value: i32) -> Self {
        let p = NumberProvider::constant(value as f32);
        Self {
            min: Some(p.clone()),
            max: Some(p),
        }
    }

    /// `IntRange.range(int, int)` — `[min, max]`.
    #[must_use]
    pub fn range(min: i32, max: i32) -> Self {
        Self {
            min: Some(NumberProvider::constant(min as f32)),
            max: Some(NumberProvider::constant(max as f32)),
        }
    }

    /// `IntRange.lowerBound(int)` — `[min, +inf)`.
    #[must_use]
    pub fn lower_bound(min: i32) -> Self {
        Self {
            min: Some(NumberProvider::constant(min as f32)),
            max: None,
        }
    }

    /// `IntRange.upperBound(int)` — `(-inf, max]`.
    #[must_use]
    pub fn upper_bound(max: i32) -> Self {
        Self {
            min: None,
            max: Some(NumberProvider::constant(max as f32)),
        }
    }

    /// Returns the optional lower bound provider.
    #[must_use]
    pub fn min(&self) -> Option<&NumberProvider> {
        self.min.as_ref()
    }

    /// Returns the optional upper bound provider.
    #[must_use]
    pub fn max(&self) -> Option<&NumberProvider> {
        self.max.as_ref()
    }

    /// `IntRange.unpackExact()` — returns `Some(v)` iff both `min` and `max` are
    /// identical constant integral values.
    #[must_use]
    pub fn unpack_exact(&self) -> Option<i32> {
        if let (Some(NumberProvider::Constant(min_v)), Some(NumberProvider::Constant(max_v))) =
            (&self.min, &self.max)
            && min_v == max_v
            && mth::floor(*min_v) as f32 == *min_v
        {
            return Some(*min_v as i32);
        }
        None
    }

    /// `IntRange.clamp(LootContext, int)` — evaluates bounds in Paper's exact
    /// order (`min` then `max` when both present; only the present bound
    /// otherwise; identity when neither is set).
    pub fn clamp(&self, context: &mut LootContext<'_>, value: i32) -> i32 {
        match (&self.min, &self.max) {
            (None, None) => value,
            (None, Some(max)) => max.get_int(context).min(value),
            (Some(min), None) => min.get_int(context).max(value),
            (Some(min), Some(max)) => {
                let min_v = min.get_int(context);
                let max_v = max.get_int(context);
                mth::clamp(value, min_v, max_v)
            }
        }
    }

    /// `IntRange.test(LootContext, int)` — inclusive range check with Paper's
    /// exact short-circuit evaluation order (`min` checked before `max`).
    pub fn test(&self, context: &mut LootContext<'_>, value: i32) -> bool {
        match (&self.min, &self.max) {
            (None, None) => true,
            (None, Some(max)) => value <= max.get_int(context),
            (Some(min), None) => value >= min.get_int(context),
            (Some(min), Some(max)) => {
                value >= min.get_int(context) && value <= max.get_int(context)
            }
        }
    }
}

impl Validatable for IntRange {
    fn validate(&self, context: &ValidationContext<'_>) {
        context.validate_context_usage(self);
        validate_optional_field(context, "min", self.min.as_ref());
        validate_optional_field(context, "max", self.max.as_ref());
    }
}

impl LootContextUser for IntRange {
    fn referenced_context_params(&self) -> IndexSet<ContextKey> {
        let mut set = IndexSet::new();
        if let Some(min) = &self.min {
            set.extend(min.referenced_context_params());
        }
        if let Some(max) = &self.max {
            set.extend(max.referenced_context_params());
        }
        set
    }
}

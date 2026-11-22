//! `net.minecraft.world.level.storage.loot` — Paper 26.2 loot runtime (issue #469).
//!
//! Faithful Rust port of the executable loot runtime with exact Paper RNG
//! consumption, deterministic iteration order, validation, and recursion
//! detection:
//!
//! - [`table::LootTable`] — table execution, stack splitting (`splitOverstackedLoot`),
//!   container filling (`fill` / `shuffleAndSplitItems`), and recursion breadcrumbs.
//! - [`pool::LootPool`] — condition gating before roll RNG, roll + bonus-roll
//!   computation, single-entry RNG skip, weighted selection, and pool functions.
//! - [`entries::LootPoolEntryContainer`] — singleton (`Item`, `Empty`, `Dynamic`,
//!   `Tag`, `LootTable`) and composite (`Alternatives`, `Group`, `Sequence`) entries.
//! - [`providers::NumberProvider`] & [`int_range::IntRange`] — `Constant`,
//!   `Uniform`, `Binomial`, `Score`, `EnchantmentLevel`, and dynamic clamping/testing.
//! - [`predicates::LootItemCondition`] — `AllOf`, `AnyOf`, `Inverted`,
//!   `RandomChance`, `SurvivesExplosion`, `Reference`, `ValueCheck`, `TableBonus`.
//! - [`functions::LootItemFunction`] — `SetCount`, `LimitCount`, `ExplosionDecay`,
//!   `ApplyBonus` (`BinomialWithBonusCount`, `OreDrops`, `UniformBonusCount`),
//!   `Sequence`, and `Reference`.
//! - [`parameters`] — `ContextKey`, `ContextKeySet`, `ContextMap`,
//!   `LootContextParams`, and `LootContextParamSets`.
//! - [`context`] — `LootParams`, `LootContext`, `LootRandom`, `LootRegistry`,
//!   and `VisitedEntry`.
//! - [`validation`] — `ValidationContext`, `Validatable`, `LootContextUser`,
//!   and structured `ProblemReporter` diagnostics.

pub mod context;
pub mod entries;
pub mod functions;
pub mod int_range;
pub mod item_stack;
pub mod parameters;
pub mod pool;
pub mod predicates;
pub mod providers;
pub mod table;
pub mod validation;

#[cfg(test)]
mod tests;

pub use context::{
    DynamicDropFn, LootContext, LootContextBuilder, LootParams, LootParamsBuilder, LootRandom,
    LootRandomState, LootRegistry, RandomSequenceSeed, VisitedEntry,
};
pub use entries::{
    DEFAULT_QUALITY, DEFAULT_WEIGHT, ExpandedLootPoolEntry, LootPoolEntryContainer, NestedTableRef,
};
pub use functions::{BonusFormula, LootItemFunction};
pub use int_range::IntRange;
pub use item_stack::{DEFAULT_MAX_STACK_SIZE, LootContainer, LootItemStack, SimpleLootContainer};
pub use parameters::{
    ContextKey, ContextKeySet, ContextKeySetBuilder, ContextMap, ContextMapBuilder,
    ContextMapError, ContextValue, LootContextParamSets, LootContextParams,
};
pub use pool::{LootPool, LootPoolBuilder};
pub use predicates::LootItemCondition;
pub use providers::{NumberProvider, ScoreTarget};
pub use table::{LootTable, LootTableBuilder, RANDOMIZE_SEED};
pub use validation::{
    ElementReferencePathElement, InlineLootTablePathElement, LootContextUser, LootReferenceKey,
    MapEntryPathElement, MissingReferenceProblem, ParametersNotProvidedProblem,
    RecursiveReferenceProblem, ReferenceNotAllowedProblem, UnreachableEntryProblem, Validatable,
    ValidationContext, validate_field, validate_list, validate_optional_field,
};

/// Java `Math.round(float)` semantics: `floor(x + 0.5f)` saturating to `i32`
/// (`NaN -> 0`, `>= i32::MAX -> i32::MAX`, `<= i32::MIN -> i32::MIN`).
#[must_use]
pub(crate) fn java_round_f32(x: f32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    let shifted = (x as f64 + 0.5_f64).floor();
    if shifted >= i32::MAX as f64 {
        i32::MAX
    } else if shifted <= i32::MIN as f64 {
        i32::MIN
    } else {
        shifted as i32
    }
}

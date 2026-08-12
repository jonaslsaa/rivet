//! Port of `com.mojang.datafixers.DataFix`.
//!
//! The builder foundation needs the `getVersionKey`/`getRule`/`getInputSchema`/
//! `getOutputSchema` surface and the `changesType` flag. The `fixTypeEverywhere`
//! family of helpers builds rules from typed functions; those are deferred with
//! the optics layer (they construct `NamedFunctionWrapper` `PointFree` nodes via
//! `View.create`), so only the shell and the `writeAndRead`-style skeleton are
//! ported. Java's `getRule` memoizes `makeRule()`.

use crate::datafixers::schemas::Schema;
use crate::datafixers::type_rewrite_rule;
use crate::dynamic_ops::DynamicOps;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.datafixers.DataFix`.
///
/// Java subclasses override `makeRule()`; the port stores the rule as a factory
/// closure so a fix can be constructed with its rewrite inline (no subclassing
/// in Rust). The factory is memoized by `getRule`'s `OnceLock`.
pub struct DataFix<Ops: DynamicOps + 'static> {
    pub output_schema: Arc<Schema<Ops>>,
    pub changes_type: bool,
    /// `makeRule()` — the subclass-provided rewrite, as a factory.
    pub rule_factory:
        Arc<dyn Fn() -> Arc<dyn type_rewrite_rule::TypeRewriteRule<Ops>> + Send + Sync>,
    /// Lazy `makeRule()` memo.
    pub rule: std::sync::OnceLock<Arc<dyn type_rewrite_rule::TypeRewriteRule<Ops>>>,
}

impl<Ops: DynamicOps + 'static> Debug for DataFix<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DataFix[{}]", self.get_version_key())
    }
}

impl<Ops: DynamicOps + 'static> DataFix<Ops> {
    /// `new DataFix(outputSchema, changesType)` — `makeRule()` returns nop.
    pub fn new(output_schema: Arc<Schema<Ops>>, changes_type: bool) -> Self {
        DataFix::with_rule_factory(
            output_schema,
            changes_type,
            Arc::new(|| type_rewrite_rule::nop()),
        )
    }

    /// `new DataFix(...)` with an explicit `makeRule()`.
    pub fn with_rule_factory(
        output_schema: Arc<Schema<Ops>>,
        changes_type: bool,
        rule_factory: Arc<
            dyn Fn() -> Arc<dyn type_rewrite_rule::TypeRewriteRule<Ops>> + Send + Sync,
        >,
    ) -> Self {
        DataFix {
            output_schema,
            changes_type,
            rule_factory,
            rule: std::sync::OnceLock::new(),
        }
    }

    /// `DataFix.getOutputSchema()`.
    pub fn get_output_schema(&self) -> Arc<Schema<Ops>> {
        self.output_schema.clone()
    }

    /// `DataFix.getInputSchema()`.
    pub fn get_input_schema(&self) -> Arc<Schema<Ops>> {
        if self.changes_type {
            self.output_schema
                .get_parent()
                .expect("output schema has a parent when changesType")
        } else {
            self.output_schema.clone()
        }
    }

    /// `DataFix.getVersionKey()`.
    pub fn get_version_key(&self) -> i32 {
        self.output_schema.get_version_key()
    }

    /// `DataFix.getRule()` — memoized `makeRule()`.
    pub fn get_rule(&self) -> Arc<dyn type_rewrite_rule::TypeRewriteRule<Ops>> {
        self.rule.get_or_init(|| (self.rule_factory)()).clone()
    }
}

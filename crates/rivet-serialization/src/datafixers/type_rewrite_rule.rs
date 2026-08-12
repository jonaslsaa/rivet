//! Port of `com.mojang.datafixers.TypeRewriteRule`.
//!
//! Java's `TypeRewriteRule` is an interface with static factories; the port is
//! a trait plus concrete rule structs. Rules return
//! `Option<RewriteResult<Ops>>` (`Some` when a rewrite applied, `None` when it
//! did not — Java uses `Optional.empty()`).
//!
//! `checkOnce`'s onFail callback is a no-op in Java (`// TODO: toggle somehow`
//! returns `rule` directly), so it is not ported.

use crate::datafixers::functions::rule::PointFreeRule;
use crate::datafixers::rewrite_result::RewriteResult;
use crate::datafixers::types::Type;
use crate::dynamic_ops::DynamicOps;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.datafixers.TypeRewriteRule`.
pub trait TypeRewriteRule<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `TypeRewriteRule.rewrite(Type<A>)`.
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>>;

    /// Whether this rule is `nop()` (used by `seq` short-circuiting).
    fn is_nop(&self) -> bool {
        false
    }

    /// Arc-wraps a clone of this rule.
    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>>;
}

/// `TypeRewriteRule.nop()`.
pub struct Nop<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Nop<Ops> {
    pub fn new() -> Self {
        Nop {
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> Default for Nop<Ops> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Ops: DynamicOps + 'static> Debug for Nop<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Nop")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for Nop<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        Some(RewriteResult::nop(ty))
    }

    fn is_nop(&self) -> bool {
        true
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(Nop::new())
    }
}

/// `TypeRewriteRule.seq` — the `Seq` rule.
pub struct Seq<Ops: DynamicOps + 'static> {
    pub rules: Vec<Arc<dyn TypeRewriteRule<Ops>>>,
}

impl<Ops: DynamicOps + 'static> Debug for Seq<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Seq[{}]", self.rules.len())
    }
}

impl<Ops: DynamicOps + 'static> Seq<Ops> {
    pub fn new(rules: Vec<Arc<dyn TypeRewriteRule<Ops>>>) -> Self {
        Seq { rules }
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for Seq<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        let mut result = RewriteResult::nop(ty);
        for rule in &self.rules {
            // cap1: `rule.rewrite(result.view().newType()).map(s -> s.compose(result))`.
            let new_type = result.view.new_type();
            let new_result = rule.rewrite(new_type.as_ref())?;
            result = new_result.compose(&result);
        }
        Some(result)
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(Seq::new(
            self.rules.iter().map(|r| r.clone_rule()).collect(),
        ))
    }
}

/// `TypeRewriteRule.orElse`.
pub struct OrElse<Ops: DynamicOps + 'static> {
    pub first: Arc<dyn TypeRewriteRule<Ops>>,
    pub second: Arc<dyn TypeRewriteRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for OrElse<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrElse")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for OrElse<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        self.first.rewrite(ty).or_else(|| self.second.rewrite(ty))
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(OrElse {
            first: self.first.clone_rule(),
            second: self.second.clone_rule(),
        })
    }
}

/// `TypeRewriteRule.all`.
pub struct All<Ops: DynamicOps + 'static> {
    pub rule: Arc<dyn TypeRewriteRule<Ops>>,
    pub recurse: bool,
    pub check_index: bool,
}

impl<Ops: DynamicOps + 'static> Debug for All<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "All")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for All<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        Some(ty.all(self.rule.as_ref(), self.recurse, self.check_index))
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(All {
            rule: self.rule.clone_rule(),
            recurse: self.recurse,
            check_index: self.check_index,
        })
    }
}

/// `TypeRewriteRule.one`.
pub struct One<Ops: DynamicOps + 'static> {
    pub rule: Arc<dyn TypeRewriteRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for One<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "One")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for One<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        ty.one(self.rule.as_ref())
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(One {
            rule: self.rule.clone_rule(),
        })
    }
}

/// `TypeRewriteRule.once` — `orElse(rule, () -> one(once(rule)))`.
pub struct Once<Ops: DynamicOps + 'static> {
    pub rule: Arc<dyn TypeRewriteRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Once<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Once")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for Once<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        if let Some(result) = self.rule.rewrite(ty) {
            return Some(result);
        }
        let once = Arc::new(Once {
            rule: self.rule.clone_rule(),
        });
        let one = Arc::new(One { rule: once });
        ty.one(one.as_ref())
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(Once {
            rule: self.rule.clone_rule(),
        })
    }
}

/// `TypeRewriteRule.everywhere`.
pub struct Everywhere<Ops: DynamicOps + 'static> {
    pub rule: Arc<dyn TypeRewriteRule<Ops>>,
    pub optimization_rule: Arc<dyn PointFreeRule<Ops>>,
    pub recurse: bool,
    pub check_index: bool,
}

impl<Ops: DynamicOps + 'static> Debug for Everywhere<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Everywhere")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for Everywhere<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        ty.everywhere(
            self.rule.as_ref(),
            self.optimization_rule.as_ref(),
            self.recurse,
            self.check_index,
        )
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(Everywhere {
            rule: self.rule.clone_rule(),
            optimization_rule: self.optimization_rule.clone_rule(),
            recurse: self.recurse,
            check_index: self.check_index,
        })
    }
}

/// `TypeRewriteRule.ifSame`.
pub struct IfSame<Ops: DynamicOps + 'static> {
    pub target_type: Arc<dyn Type<Ops>>,
    pub value: RewriteResult<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for IfSame<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IfSame")
    }
}

impl<Ops: DynamicOps + 'static> TypeRewriteRule<Ops> for IfSame<Ops> {
    fn rewrite(&self, ty: &dyn Type<Ops>) -> Option<RewriteResult<Ops>> {
        if ty.equals_(self.target_type.as_ref(), true, true) {
            Some(self.value.clone())
        } else {
            None
        }
    }

    fn clone_rule(&self) -> Arc<dyn TypeRewriteRule<Ops>> {
        Arc::new(IfSame {
            target_type: self.target_type.clone(),
            value: self.value.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Factories
// ---------------------------------------------------------------------------

/// `TypeRewriteRule.nop()`.
pub fn nop<Ops: DynamicOps + 'static>() -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(Nop::new())
}

/// `TypeRewriteRule.seq(first, second)` with Java's nop short-circuiting.
pub fn seq2<Ops: DynamicOps + 'static>(
    first: Arc<dyn TypeRewriteRule<Ops>>,
    second: Arc<dyn TypeRewriteRule<Ops>>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    if first.is_nop() {
        return second;
    }
    if second.is_nop() {
        return first;
    }
    Arc::new(Seq::new(vec![first, second]))
}

/// `TypeRewriteRule.seq(rules)`.
pub fn seq<Ops: DynamicOps + 'static>(
    rules: Vec<Arc<dyn TypeRewriteRule<Ops>>>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    if rules.len() == 1 {
        return rules.into_iter().next().expect("one");
    }
    Arc::new(Seq::new(rules))
}

/// `TypeRewriteRule.orElse(first, second)`.
pub fn or_else<Ops: DynamicOps + 'static>(
    first: Arc<dyn TypeRewriteRule<Ops>>,
    second: Arc<dyn TypeRewriteRule<Ops>>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(OrElse { first, second })
}

/// `TypeRewriteRule.all(rule, recurse, checkIndex)`.
pub fn all<Ops: DynamicOps + 'static>(
    rule: Arc<dyn TypeRewriteRule<Ops>>,
    recurse: bool,
    check_index: bool,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(All {
        rule,
        recurse,
        check_index,
    })
}

/// `TypeRewriteRule.one(rule)`.
pub fn one<Ops: DynamicOps + 'static>(
    rule: Arc<dyn TypeRewriteRule<Ops>>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(One { rule })
}

/// `TypeRewriteRule.once(rule)`.
pub fn once<Ops: DynamicOps + 'static>(
    rule: Arc<dyn TypeRewriteRule<Ops>>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(Once { rule })
}

/// `TypeRewriteRule.checkOnce(rule, onFail)` — the Java body returns `rule`
/// directly (the `CheckOnce` variant is commented out).
pub fn check_once<Ops: DynamicOps + 'static>(
    rule: Arc<dyn TypeRewriteRule<Ops>>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    rule
}

/// `TypeRewriteRule.everywhere(rule, optimizationRule, recurse, checkIndex)`.
pub fn everywhere<Ops: DynamicOps + 'static>(
    rule: Arc<dyn TypeRewriteRule<Ops>>,
    optimization_rule: Arc<dyn PointFreeRule<Ops>>,
    recurse: bool,
    check_index: bool,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(Everywhere {
        rule,
        optimization_rule,
        recurse,
        check_index,
    })
}

/// `TypeRewriteRule.ifSame(targetType, value)`.
pub fn if_same<Ops: DynamicOps + 'static>(
    target_type: Arc<dyn Type<Ops>>,
    value: RewriteResult<Ops>,
) -> Arc<dyn TypeRewriteRule<Ops>> {
    Arc::new(IfSame { target_type, value })
}

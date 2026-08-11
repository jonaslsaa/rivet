//! Port of `com.mojang.datafixers.functions.PointFreeRule`.
//!
//! The optimizing rules (`CataFuseSame`/`CataFuseDifferent`, `LensComp`,
//! `SortProj`, `SortInj`) only fire on `Fold`/`Apply`+`ProfunctorTransformer`
//! structures that the deferred optics/recursive layer builds; their
//! `do_rewrite` bodies therefore return `None` here (no matching structure can
//! exist yet). `AppNest` (bottom-up in `OPTIMIZATION_RULE`) likewise only fires
//! on `Apply` nodes, which the optics layer builds. The structural combinators
//! (`seq`, `choice`, `all`, `one`, `once`, `many`, `everywhere`) are faithful.

use super::{Comp, PointFreeCore, PointFreeFunc};
use crate::dynamic_ops::DynamicOps;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.datafixers.functions.PointFreeRule`.
pub trait PointFreeRule<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `PointFreeRule.rewrite(PointFree<A>)`.
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>>;

    /// `PointFreeRule.rewriteOrNop(PointFree<A>)` — `orElse(rewrite(expr), expr)`.
    fn rewrite_or_nop(&self, expr: &dyn PointFreeCore<Ops>) -> Arc<dyn PointFreeCore<Ops>> {
        self.rewrite(expr).unwrap_or_else(|| expr.clone_core())
    }

    /// Arc-wraps a clone of this rule.
    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>>;
}

/// `PointFreeRule.nop()`.
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

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for Nop<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        Some(expr.clone_core())
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(Nop::new())
    }
}

/// `PointFreeRule.seq(rules)`.
pub struct Seq<Ops> {
    pub rules: Vec<Arc<dyn PointFreeRule<Ops>>>,
}

impl<Ops: DynamicOps + 'static> Debug for Seq<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Seq[{}]", self.rules.len())
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for Seq<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        let mut result = expr.clone_core();
        for rule in &self.rules {
            result = rule.rewrite_or_nop(result.as_ref());
        }
        Some(result)
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(Seq {
            rules: self.rules.iter().map(|r| r.clone_rule()).collect(),
        })
    }
}

/// `PointFreeRule.choice(rules)`.
pub struct Choice<Ops> {
    pub rules: Vec<Arc<dyn PointFreeRule<Ops>>>,
}

impl<Ops: DynamicOps + 'static> Debug for Choice<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Choice[{}]", self.rules.len())
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for Choice<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        for rule in &self.rules {
            if let Some(view) = rule.rewrite(expr) {
                return Some(view);
            }
        }
        None
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(Choice {
            rules: self.rules.iter().map(|r| r.clone_rule()).collect(),
        })
    }
}

/// `PointFreeRule.all(rule)`.
pub struct All<Ops> {
    pub rule: Arc<dyn PointFreeRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for All<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "All")
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for All<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        expr.all(self.rule.as_ref())
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(All {
            rule: self.rule.clone_rule(),
        })
    }
}

/// `PointFreeRule.one(rule)`.
pub struct One<Ops> {
    pub rule: Arc<dyn PointFreeRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for One<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "One")
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for One<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        expr.one(self.rule.as_ref())
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(One {
            rule: self.rule.clone_rule(),
        })
    }
}

/// `PointFreeRule.once(rule)`.
pub struct Once<Ops> {
    pub rule: Arc<dyn PointFreeRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Once<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Once")
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for Once<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        if let Some(view) = self.rule.rewrite(expr) {
            return Some(view);
        }
        expr.one(self)
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(Once {
            rule: self.rule.clone_rule(),
        })
    }
}

/// `PointFreeRule.many(rule)`.
pub struct Many<Ops> {
    pub rule: Arc<dyn PointFreeRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Many<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Many")
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for Many<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        let mut result = expr.clone_core();
        loop {
            match self.rule.rewrite(result.as_ref()) {
                Some(new_result) => result = new_result,
                None => return Some(result),
            }
        }
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(Many {
            rule: self.rule.clone_rule(),
        })
    }
}

/// `PointFreeRule.everywhere(topDown, bottomUp)`.
pub struct Everywhere<Ops> {
    pub top_down: Arc<dyn PointFreeRule<Ops>>,
    pub bottom_up: Arc<dyn PointFreeRule<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Everywhere<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Everywhere")
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for Everywhere<Ops> {
    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        let top_down = self.top_down.rewrite_or_nop(expr);
        let all = top_down.all(self).unwrap_or_else(|| top_down.clone());
        let bottom_up = self.bottom_up.rewrite_or_nop(all.as_ref());
        Some(bottom_up)
    }

    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(Everywhere {
            top_down: self.top_down.clone_rule(),
            bottom_up: self.bottom_up.clone_rule(),
        })
    }
}

/// The `CompRewrite` rule family: a rule over adjacent composed functions.
///
/// Java gives `CompRewrite` a default `rewrite` that traverses a `Comp`,
/// merging adjacent pairs via `doRewrite`. Rust cannot blanket-implement
/// `PointFreeRule` over `CompRewrite` without conflicting with the concrete
/// rules, so that default is factored into [`CompRewriteRule`], which wraps a
/// `CompRewrite` as a `PointFreeRule`.
pub trait CompRewrite<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `CompRewrite.doRewrite(first, second)` — merge two adjacent functions.
    fn do_rewrite(
        &self,
        first: &dyn PointFreeFunc<Ops>,
        second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>>;
}

/// The default `CompRewrite.rewrite` as a `PointFreeRule`.
pub struct CompRewriteRule<Ops> {
    pub rule: Arc<dyn CompRewrite<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for CompRewriteRule<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompRewriteRule")
    }
}

impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for CompRewriteRule<Ops> {
    fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
        Arc::new(CompRewriteRule {
            rule: self.rule.clone(),
        })
    }

    fn rewrite(&self, expr: &dyn PointFreeCore<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        let comp = expr.as_any().downcast_ref::<Comp<Ops>>()?;
        let mut result: VecDeque<Arc<dyn PointFreeFunc<Ops>>> = VecDeque::new();
        let mut rewritten = false;
        let mut queue: VecDeque<Arc<dyn PointFreeFunc<Ops>>> =
            comp.functions.iter().cloned().collect();
        while let Some(next) = queue.pop_front() {
            if let Some(last) = result.back()
                && let Some(merge) = self.rule.do_rewrite(last.as_ref(), next.as_ref())
            {
                result.pop_back();
                add_first(&mut queue, merge);
                rewritten = true;
                continue;
            }
            result.push_back(next);
        }
        if rewritten {
            if result.len() == 1 {
                return Some(result.pop_back().expect("one").clone_func());
            }
            Some(Arc::new(Comp::new(result.into_iter().collect())))
        } else {
            None
        }
    }
}

/// The `CompRewrite.addFirst` helper — splices a `Comp` back onto the queue.
fn add_first<Ops: DynamicOps + 'static>(
    queue: &mut VecDeque<Arc<dyn PointFreeFunc<Ops>>>,
    function: Arc<dyn PointFreeFunc<Ops>>,
) {
    if let Some(comp) = function.as_any().downcast_ref::<Comp<Ops>>() {
        for f in comp.functions.iter().rev() {
            queue.push_front(f.clone());
        }
    } else {
        queue.push_front(function);
    }
}

/// `CompRewrite.together(rules)` — try each rule in order.
pub struct CompRewriteImpl<Ops> {
    pub rules: Vec<Arc<dyn CompRewrite<Ops>>>,
}

impl<Ops: DynamicOps + 'static> Debug for CompRewriteImpl<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompRewriteImpl[{}]", self.rules.len())
    }
}

impl<Ops: DynamicOps + 'static> CompRewrite<Ops> for CompRewriteImpl<Ops> {
    fn do_rewrite(
        &self,
        first: &dyn PointFreeFunc<Ops>,
        second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
        for rule in &self.rules {
            if let Some(view) = rule.do_rewrite(first, second) {
                return Some(view);
            }
        }
        None
    }
}

/// `PointFreeRule.CataFuseSame` — fold fusion over the same index.
///
/// Only reachable with `Fold` nodes (the deferred recursive layer); no `Fold`
/// exists yet, so no expression can match.
#[derive(Default)]
pub struct CataFuseSame<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for CataFuseSame<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CataFuseSame")
    }
}

impl<Ops: DynamicOps + 'static> CompRewrite<Ops> for CataFuseSame<Ops> {
    fn do_rewrite(
        &self,
        _first: &dyn PointFreeFunc<Ops>,
        _second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
        None
    }
}

/// `PointFreeRule.CataFuseDifferent` — fold fusion over disjoint indices.
///
/// Deferred with the recursive layer (see `CataFuseSame`).
#[derive(Default)]
pub struct CataFuseDifferent<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for CataFuseDifferent<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CataFuseDifferent")
    }
}

impl<Ops: DynamicOps + 'static> CompRewrite<Ops> for CataFuseDifferent<Ops> {
    fn do_rewrite(
        &self,
        _first: &dyn PointFreeFunc<Ops>,
        _second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
        None
    }
}

/// `PointFreeRule.LensComp` — merge lens applications.
///
/// Deferred with the optics layer (`ProfunctorTransformer` doesn't exist yet).
#[derive(Default)]
pub struct LensComp<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for LensComp<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LensComp")
    }
}

impl<Ops: DynamicOps + 'static> CompRewrite<Ops> for LensComp<Ops> {
    fn do_rewrite(
        &self,
        _first: &dyn PointFreeFunc<Ops>,
        _second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
        None
    }
}

/// `PointFreeRule.SortProj` — reorder `proj2`-first compositions.
///
/// Deferred with the optics layer.
#[derive(Default)]
pub struct SortProj<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for SortProj<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SortProj")
    }
}

impl<Ops: DynamicOps + 'static> CompRewrite<Ops> for SortProj<Ops> {
    fn do_rewrite(
        &self,
        _first: &dyn PointFreeFunc<Ops>,
        _second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
        None
    }
}

/// `PointFreeRule.SortInj` — reorder `inj2`-first compositions.
///
/// Deferred with the optics layer.
#[derive(Default)]
pub struct SortInj<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Debug for SortInj<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SortInj")
    }
}

impl<Ops: DynamicOps + 'static> CompRewrite<Ops> for SortInj<Ops> {
    fn do_rewrite(
        &self,
        _first: &dyn PointFreeFunc<Ops>,
        _second: &dyn PointFreeFunc<Ops>,
    ) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
        None
    }
}

// ---------------------------------------------------------------------------
// Factories
// ---------------------------------------------------------------------------

/// `PointFreeRule.nop()`.
pub fn nop<Ops: DynamicOps + 'static>() -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(Nop::new())
}

/// `PointFreeRule.seq(rules)`.
pub fn seq<Ops: DynamicOps + 'static>(
    rules: Vec<Arc<dyn PointFreeRule<Ops>>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(Seq { rules })
}

/// `PointFreeRule.choice(rules)`.
pub fn choice<Ops: DynamicOps + 'static>(
    rules: Vec<Arc<dyn PointFreeRule<Ops>>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    if rules.len() == 1 {
        return rules.into_iter().next().expect("one");
    }
    Arc::new(Choice { rules })
}

/// `PointFreeRule.all(rule)`.
pub fn all<Ops: DynamicOps + 'static>(
    rule: Arc<dyn PointFreeRule<Ops>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(All { rule })
}

/// `PointFreeRule.one(rule)`.
pub fn one<Ops: DynamicOps + 'static>(
    rule: Arc<dyn PointFreeRule<Ops>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(One { rule })
}

/// `PointFreeRule.once(rule)`.
pub fn once<Ops: DynamicOps + 'static>(
    rule: Arc<dyn PointFreeRule<Ops>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(Once { rule })
}

/// `PointFreeRule.many(rule)`.
pub fn many<Ops: DynamicOps + 'static>(
    rule: Arc<dyn PointFreeRule<Ops>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(Many { rule })
}

/// `PointFreeRule.everywhere(topDown, bottomUp)`.
pub fn everywhere<Ops: DynamicOps + 'static>(
    top_down: Arc<dyn PointFreeRule<Ops>>,
    bottom_up: Arc<dyn PointFreeRule<Ops>>,
) -> Arc<dyn PointFreeRule<Ops>> {
    Arc::new(Everywhere {
        top_down,
        bottom_up,
    })
}

/// `CompRewrite.together(rules)`.
pub fn comp_rewrite_together<Ops: DynamicOps + 'static>(
    rules: Vec<Arc<dyn CompRewrite<Ops>>>,
) -> Arc<dyn CompRewrite<Ops>> {
    Arc::new(CompRewriteImpl { rules })
}

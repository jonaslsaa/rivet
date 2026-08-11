//! Port of the `com.mojang.datafixers.functions` package: the point-free
//! function calculus (`PointFree`) and its rewrite machinery (`PointFreeRule`).
//!
//! Java's `PointFree<T>` is generic over the value `T` (`Function<A, B>` for
//! function nodes); the port erases values to [`AnyValue`] (an `Arc<dyn Any>`),
//! mirroring Java's `Type<?>` wildcard erasure at the rewrite boundary. The
//! function/argument distinction is kept (`PointFreeFunc` vs `PointFreeVal`),
//! but `Apply`/`app` are deferred with the optics layer (they are only built by
//! `Functions.app`, which the deferred `opticView` uses).

pub mod rule;

use crate::datafixers::functions::rule::PointFreeRule;
use crate::datafixers::types::{AnyValue, Type};
use crate::dynamic_ops::DynamicOps;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// The evaluated form of a point-free function: given the ops and an erased
/// value, produce an erased value. This corresponds to Java's
/// `Function<DynamicOps<?>, Function<A, B>>` applied at call time.
pub type DfFn<Ops> = Arc<dyn Fn(&Ops, &AnyValue) -> AnyValue + Send + Sync>;

/// `com.mojang.datafixers.functions.PointFree<T>` — the shared node surface.
///
/// `all`/`one` return `Some(new)` when a rewrite changed the node and `None`
/// when it did not (Java returns `Optional.of(this)` for the unchanged case;
/// callers only distinguish present-with-different-node from unchanged, so
/// `None` is observationally equivalent and avoids reference-identity checks).
pub trait PointFreeCore<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `PointFree.all(rule)` — rewrite all direct children.
    fn all(&self, _rule: &dyn PointFreeRule<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        None
    }

    /// `PointFree.one(rule)` — rewrite exactly one child.
    fn one(&self, _rule: &dyn PointFreeRule<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        None
    }

    /// Arc-wraps a clone of this node (used when no rewrite applied).
    fn clone_core(&self) -> Arc<dyn PointFreeCore<Ops>>;

    /// `PointFree.toString(level)`.
    fn pf_to_string(&self, level: usize) -> String;

    /// Downcast handle for the rule bodies.
    fn as_any(&self) -> &dyn Any;
}

/// A point-free *function* node — `PointFree<Function<A, B>>`.
pub trait PointFreeFunc<Ops: DynamicOps + 'static>: PointFreeCore<Ops> {
    /// `PointFree.evalCached()` — the ops-parameterized function.
    fn eval_cached(&self) -> DfFn<Ops>;

    /// The input type of the underlying `Func<A, B>` (`Func.first()`).
    fn input_type(&self) -> Arc<dyn Type<Ops>>;

    /// The output type of the underlying `Func<A, B>` (`Func.second()`).
    fn output_type(&self) -> Arc<dyn Type<Ops>>;

    /// `Functions.isId` — `this instanceof Id`.
    fn is_id(&self) -> bool {
        false
    }

    /// Arc-wraps a clone of this node as a function node.
    fn clone_func(&self) -> Arc<dyn PointFreeFunc<Ops>>;

    /// Coerce to the core surface (`&dyn PointFreeFunc` -> `&dyn PointFreeCore`).
    fn as_core(&self) -> &dyn PointFreeCore<Ops>;
}

/// `Functions.id(type)` — the identity function node.
#[derive(Debug)]
pub struct Id<Ops: DynamicOps + 'static> {
    pub ty: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for Id<Ops> {
    fn clone(&self) -> Self {
        Id {
            ty: self.ty.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Id<Ops> {
    pub fn new(ty: Arc<dyn Type<Ops>>) -> Self {
        Id { ty }
    }
}

impl<Ops: DynamicOps + 'static> PointFreeCore<Ops> for Id<Ops> {
    fn clone_core(&self) -> Arc<dyn PointFreeCore<Ops>> {
        Arc::new(self.clone())
    }

    fn pf_to_string(&self, _level: usize) -> String {
        "id".to_string()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<Ops: DynamicOps + 'static> PointFreeFunc<Ops> for Id<Ops> {
    fn eval_cached(&self) -> DfFn<Ops> {
        Arc::new(|_ops, a| a.clone())
    }

    fn input_type(&self) -> Arc<dyn Type<Ops>> {
        self.ty.clone()
    }

    fn output_type(&self) -> Arc<dyn Type<Ops>> {
        self.ty.clone()
    }

    fn is_id(&self) -> bool {
        true
    }

    fn clone_func(&self) -> Arc<dyn PointFreeFunc<Ops>> {
        Arc::new(self.clone())
    }

    fn as_core(&self) -> &dyn PointFreeCore<Ops> {
        self
    }
}

/// `Functions.fun(name, fun, input, output)` — the named wrapper node.
pub struct FunctionWrapper<Ops: DynamicOps + 'static> {
    pub name: String,
    pub fun: DfFn<Ops>,
    pub input: Arc<dyn Type<Ops>>,
    pub output: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for FunctionWrapper<Ops> {
    fn clone(&self) -> Self {
        FunctionWrapper {
            name: self.name.clone(),
            fun: self.fun.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> FunctionWrapper<Ops> {
    pub fn new(
        name: String,
        fun: DfFn<Ops>,
        input: Arc<dyn Type<Ops>>,
        output: Arc<dyn Type<Ops>>,
    ) -> Self {
        FunctionWrapper {
            name,
            fun,
            input,
            output,
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for FunctionWrapper<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FunctionWrapper[{}]", self.name)
    }
}

impl<Ops: DynamicOps + 'static> PointFreeCore<Ops> for FunctionWrapper<Ops> {
    fn clone_core(&self) -> Arc<dyn PointFreeCore<Ops>> {
        Arc::new(self.clone())
    }

    fn pf_to_string(&self, _level: usize) -> String {
        format!("fun[{}]", self.name)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<Ops: DynamicOps + 'static> PointFreeFunc<Ops> for FunctionWrapper<Ops> {
    fn eval_cached(&self) -> DfFn<Ops> {
        self.fun.clone()
    }

    fn input_type(&self) -> Arc<dyn Type<Ops>> {
        self.input.clone()
    }

    fn output_type(&self) -> Arc<dyn Type<Ops>> {
        self.output.clone()
    }

    fn clone_func(&self) -> Arc<dyn PointFreeFunc<Ops>> {
        Arc::new(self.clone())
    }

    fn as_core(&self) -> &dyn PointFreeCore<Ops> {
        self
    }
}

/// `Functions.comp(f1, f2)` — composition node.
///
/// Java evaluates a composition in REVERSE order (`functions.length - 1`
/// down to 0): the leftmost `f1` is applied last. The `functions` array is
/// stored left-to-right (`[f1, f2, ...]`), so the last element runs first.
pub struct Comp<Ops: DynamicOps + 'static> {
    pub functions: Vec<Arc<dyn PointFreeFunc<Ops>>>,
    pub input: Arc<dyn Type<Ops>>,
    pub output: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for Comp<Ops> {
    fn clone(&self) -> Self {
        Comp {
            functions: self.functions.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Comp<Ops> {
    pub fn new(functions: Vec<Arc<dyn PointFreeFunc<Ops>>>) -> Self {
        let input = functions.last().expect("empty comp").input_type();
        let output = functions.first().expect("empty comp").output_type();
        Comp {
            functions,
            input,
            output,
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for Comp<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Comp[{}]", self.functions.len())
    }
}

impl<Ops: DynamicOps + 'static> PointFreeCore<Ops> for Comp<Ops> {
    fn all(&self, rule: &dyn PointFreeRule<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        let mut new_functions: Vec<Arc<dyn PointFreeFunc<Ops>>> = Vec::new();
        let mut rewritten = false;
        for function in &self.functions {
            // Java calls `rule.rewriteOrNop(function)` and compares reference
            // identity; the port encodes "unchanged" as `None` (rules return
            // `None` when they did not modify the node), so a present result
            // here always means a real change.
            match rule.rewrite(function.as_core()) {
                Some(rewrite) => {
                    if let Some(comp) = rewrite.as_any().downcast_ref::<Comp<Ops>>() {
                        new_functions.extend(comp.functions.clone());
                    } else if let Some(f) = clone_func_from_core(&rewrite) {
                        new_functions.push(f);
                    } else {
                        new_functions.push(function.clone());
                    }
                    rewritten = true;
                }
                None => new_functions.push(function.clone()),
            }
        }
        if rewritten {
            Some(Arc::new(Comp::new(new_functions)))
        } else {
            None
        }
    }

    fn one(&self, rule: &dyn PointFreeRule<Ops>) -> Option<Arc<dyn PointFreeCore<Ops>>> {
        for (i, function) in self.functions.iter().enumerate() {
            // Java `one` calls `rule.rewrite` directly and checks
            // `.isPresent()`; `None` here is the "no rewrite" signal.
            if let Some(rewrite) = rule.rewrite(function.as_core()) {
                let mut new_functions: Vec<Arc<dyn PointFreeFunc<Ops>>> = Vec::new();
                new_functions.extend_from_slice(&self.functions[..i]);
                if let Some(comp) = rewrite.as_any().downcast_ref::<Comp<Ops>>() {
                    new_functions.extend(comp.functions.clone());
                } else if let Some(f) = clone_func_from_core(&rewrite) {
                    new_functions.push(f);
                } else {
                    new_functions.push(function.clone());
                }
                new_functions.extend_from_slice(&self.functions[i + 1..]);
                return Some(Arc::new(Comp::new(new_functions)));
            }
        }
        None
    }

    fn clone_core(&self) -> Arc<dyn PointFreeCore<Ops>> {
        Arc::new(self.clone())
    }

    fn pf_to_string(&self, level: usize) -> String {
        let content = self
            .functions
            .iter()
            .map(|f| f.pf_to_string(level + 1))
            .collect::<Vec<_>>()
            .join(&format!(
                "\n{}\u{25E6}\n{}",
                indent(level + 1),
                indent(level + 1)
            ));
        format!("(\n{}{}\n{})", indent(level + 1), content, indent(level))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<Ops: DynamicOps + 'static> PointFreeFunc<Ops> for Comp<Ops> {
    fn eval_cached(&self) -> DfFn<Ops> {
        let functions = self.functions.clone();
        Arc::new(move |ops, input| {
            let mut value = input.clone();
            // Reverse application: `for i in (len-1)..=0`.
            for f in functions.iter().rev() {
                value = f.eval_cached()(ops, &value);
            }
            value
        })
    }

    fn input_type(&self) -> Arc<dyn Type<Ops>> {
        self.input.clone()
    }

    fn output_type(&self) -> Arc<dyn Type<Ops>> {
        self.output.clone()
    }

    fn clone_func(&self) -> Arc<dyn PointFreeFunc<Ops>> {
        Arc::new(self.clone())
    }

    fn as_core(&self) -> &dyn PointFreeCore<Ops> {
        self
    }
}

/// Downcasts an erased rewrite result back to a function node.
pub fn clone_func_from_core<Ops: DynamicOps + 'static>(
    core: &Arc<dyn PointFreeCore<Ops>>,
) -> Option<Arc<dyn PointFreeFunc<Ops>>> {
    if let Some(id) = core.as_any().downcast_ref::<Id<Ops>>() {
        return Some(id.clone_func());
    }
    if let Some(comp) = core.as_any().downcast_ref::<Comp<Ops>>() {
        return Some(comp.clone_func());
    }
    if let Some(fw) = core.as_any().downcast_ref::<FunctionWrapper<Ops>>() {
        return Some(fw.clone_func());
    }
    None
}

/// `PointFree.indent(level)`.
pub fn indent(level: usize) -> String {
    " ".repeat(level)
}

/// Port of the `Functions` static factories.
pub struct Functions<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Functions<Ops> {
    /// `Functions.id(Type<A>)` — the identity function for `A`.
    pub fn id(ty: Arc<dyn Type<Ops>>) -> Arc<dyn PointFreeFunc<Ops>> {
        Arc::new(Id::new(ty))
    }

    /// `Functions.isId(PointFree<?>)`.
    pub fn is_id(function: &dyn PointFreeFunc<Ops>) -> bool {
        function.is_id()
    }

    /// `Functions.fun(name, fun, input, output)`.
    pub fn fun(
        name: String,
        fun: DfFn<Ops>,
        input: Arc<dyn Type<Ops>>,
        output: Arc<dyn Type<Ops>>,
    ) -> Arc<dyn PointFreeFunc<Ops>> {
        Arc::new(FunctionWrapper::new(name, fun, input, output))
    }

    /// `Functions.comp(f1, f2)` — with the same id/comp flattening as Java.
    pub fn comp(
        f1: Arc<dyn PointFreeFunc<Ops>>,
        f2: Arc<dyn PointFreeFunc<Ops>>,
    ) -> Arc<dyn PointFreeFunc<Ops>> {
        if Self::is_id(f1.as_ref()) {
            return f2;
        }
        if Self::is_id(f2.as_ref()) {
            return f1;
        }
        let mut functions: Vec<Arc<dyn PointFreeFunc<Ops>>> = Vec::new();
        if let Some(comp1) = f1.as_any().downcast_ref::<Comp<Ops>>() {
            functions.extend(comp1.functions.clone());
        } else {
            functions.push(f1);
        }
        if let Some(comp2) = f2.as_any().downcast_ref::<Comp<Ops>>() {
            functions.extend(comp2.functions.clone());
        } else {
            functions.push(f2);
        }
        Arc::new(Comp::new(functions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datafixers::functions::rule::{many as pf_many, nop as pf_nop, seq as pf_seq};
    use crate::json_ops::JsonOps;

    type Json = JsonOps;

    fn typed_string() -> Arc<dyn Type<Json>> {
        Arc::new(crate::datafixers::types::templates::PrimitiveType {
            codec: crate::codec::string_codec::<Json>(),
        })
    }

    fn string_primitive() -> Arc<dyn PointFreeFunc<Json>> {
        Functions::id(typed_string())
    }

    fn comp_of_two() -> Arc<dyn PointFreeFunc<Json>> {
        Functions::comp(string_primitive(), string_primitive())
    }

    #[test]
    fn comp_all_reports_none_when_nop_rule_changes_nothing() {
        // Java: `Comp.all(nop)` compares reference identity (`rewrite != expr`);
        // nop returns the same reference, so no child changes and `all` returns
        // the original — the port encodes "unchanged" as `None`.
        let comp = comp_of_two();
        let nop = pf_nop();
        assert!(comp.as_core().all(nop.as_ref()).is_none());
    }

    #[test]
    fn comp_one_reports_none_when_no_child_changed() {
        let comp = comp_of_two();
        let nop = pf_nop();
        assert!(comp.as_core().one(nop.as_ref()).is_none());
    }

    #[test]
    fn comp_all_sees_a_seq_of_nops_as_unchanged() {
        // `Seq.rewrite` returns `None` when every rule left the node unchanged,
        // so `Comp.all` must not report a rewrite either.
        let comp = comp_of_two();
        let seq = pf_seq(vec![pf_nop(), pf_nop()]);
        assert!(comp.as_core().all(seq.as_ref()).is_none());
    }

    #[test]
    fn many_reports_none_when_its_rule_never_changes() {
        let expr = comp_of_two();
        let many = pf_many(pf_nop());
        assert!(many.rewrite(expr.as_core()).is_none());
    }

    #[test]
    fn func_all_rewrites_children_and_detects_change() {
        // A rule that (observably) replaces every child is a real change: `all`
        // must report it as such. `ToId` returns a fresh `clone_core()` Arc for
        // every child, which the port treats as a rewrite. Two `fun` children
        // keep `Functions::comp` from collapsing the composition into one node.
        let f1 = Functions::fun(
            "f1".to_string(),
            Arc::new(|_ops: &Json, value: &AnyValue| value.clone()),
            typed_string(),
            typed_string(),
        );
        let f2 = Functions::fun(
            "f2".to_string(),
            Arc::new(|_ops: &Json, value: &AnyValue| value.clone()),
            typed_string(),
            typed_string(),
        );
        let comp = Functions::comp(f1, f2);
        let id_rule = {
            struct ToId<Ops: DynamicOps + 'static>(std::marker::PhantomData<Ops>);
            impl<Ops: DynamicOps + 'static> Debug for ToId<Ops> {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "ToId")
                }
            }
            impl<Ops: DynamicOps + 'static> PointFreeRule<Ops> for ToId<Ops> {
                fn rewrite(
                    &self,
                    expr: &dyn PointFreeCore<Ops>,
                ) -> Option<Arc<dyn PointFreeCore<Ops>>> {
                    // Only rewrite FunctionWrapper nodes (not Ids or Comps).
                    if expr
                        .as_any()
                        .downcast_ref::<FunctionWrapper<Ops>>()
                        .is_some()
                    {
                        Some(expr.clone_core())
                    } else {
                        None
                    }
                }
                fn clone_rule(&self) -> Arc<dyn PointFreeRule<Ops>> {
                    Arc::new(ToId(std::marker::PhantomData))
                }
            }
            Arc::new(ToId(std::marker::PhantomData)) as Arc<dyn PointFreeRule<Json>>
        };
        assert!(comp.as_core().all(id_rule.as_ref()).is_some());
    }
}

//! Port of `com.mojang.datafixers.View`.
//!
//! Java `View<A, B>` is a record wrapping a `PointFree<Function<A, B>>`. The
//! port erases `A`/`B` and keeps the underlying function node plus its input and
//! output types.

use crate::datafixers::functions::rule::PointFreeRule;
use crate::datafixers::functions::{Functions, PointFreeCore, PointFreeFunc, clone_func_from_core};
use crate::datafixers::types::Type;
use crate::dynamic_ops::DynamicOps;
use std::fmt::Debug;
use std::sync::Arc;

/// `com.mojang.datafixers.View<A, B>` (values erased).
pub struct View<Ops: DynamicOps + 'static> {
    pub function: Arc<dyn PointFreeFunc<Ops>>,
    /// Cached `Func.first()` — the input type.
    pub input: Arc<dyn Type<Ops>>,
    /// Cached `Func.second()` — the output type.
    pub output: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for View<Ops> {
    fn clone(&self) -> Self {
        View {
            function: self.function.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for View<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "View[{}, {}]",
            self.function.pf_to_string(0),
            self.output.type_to_string()
        )
    }
}

impl<Ops: DynamicOps + 'static> View<Ops> {
    /// `View.nopView(Type<A>)` — the identity function over `type`.
    pub fn nop_view(ty: Arc<dyn Type<Ops>>) -> View<Ops> {
        let function = Functions::id(ty.clone());
        View {
            function,
            input: ty.clone(),
            output: ty,
        }
    }

    /// `View.type()` — `Func.first()`.
    pub fn ty(&self) -> Arc<dyn Type<Ops>> {
        self.input.clone()
    }

    /// `View.newType()` — `Func.second()`.
    pub fn new_type(&self) -> Arc<dyn Type<Ops>> {
        self.output.clone()
    }

    /// `View.rewrite(PointFreeRule)`.
    pub fn rewrite(&self, rule: &dyn PointFreeRule<Ops>) -> Option<View<Ops>> {
        rule.rewrite(self.function.as_core())
            .map(|function| view_from_core(function))
    }

    /// `View.rewriteOrNop(PointFreeRule)`.
    pub fn rewrite_or_nop(&self, rule: &dyn PointFreeRule<Ops>) -> View<Ops> {
        self.rewrite(rule).unwrap_or_else(|| self.clone())
    }

    /// `View.compose(View)`.
    pub fn compose(&self, that: &View<Ops>) -> View<Ops> {
        if self.is_nop() {
            return that.clone();
        }
        if that.is_nop() {
            return self.clone();
        }
        View {
            function: Functions::comp(self.function.clone(), that.function.clone()),
            input: that.input.clone(),
            output: self.output.clone(),
        }
    }

    /// `View.isNop()`.
    pub fn is_nop(&self) -> bool {
        self.function.is_id()
    }

    /// `View.create(name, type, newType, function)`.
    pub fn create(
        name: String,
        ty: Arc<dyn Type<Ops>>,
        new_ty: Arc<dyn Type<Ops>>,
        function: crate::datafixers::functions::DfFn<Ops>,
    ) -> View<Ops> {
        View {
            function: Functions::fun(name, function, ty.clone(), new_ty.clone()),
            input: ty,
            output: new_ty,
        }
    }
}

/// Builds a `View` from a rewritten function node, recovering the input/output
/// types from the node itself.
pub(crate) fn view_from_core<Ops: DynamicOps + 'static>(
    function: Arc<dyn PointFreeCore<Ops>>,
) -> View<Ops> {
    let function = clone_func_from_core(&function).expect("rewritten function is a function node");
    View {
        input: function.input_type(),
        output: function.output_type(),
        function,
    }
}

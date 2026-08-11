//! Port of the `com.mojang.datafixers.types` package: the `Type` abstraction,
//! the `TypeFamily`/`TypeTemplate` template machinery, and the concrete
//! templates/types the builder foundation needs.
//!
//! Java's `Type<A>` is generic over its value; the port erases values to
//! [`AnyValue`] (`Arc<dyn Any + Send + Sync>`), mirroring the `Type<?>`
//! wildcards used throughout `Schema.types` and the rewrite machinery. Values
//! are `Arc`-wrapped so the identity function can share them like Java's
//! reference semantics.

pub mod templates;

use crate::data_result::DataResult;
use crate::datafixers::functions::rule::PointFreeRule;
use crate::datafixers::rewrite_result::RewriteResult;
use crate::datafixers::type_rewrite_rule::TypeRewriteRule;
use crate::datafixers::view::View;
use crate::dynamic::Dynamic;
use crate::dynamic_ops::DynamicOps;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// Re-export of `Any` for the downcast helpers in the template types.
pub use std::any::Any as _Any;

/// The erased value carried through the DFU layer. Java's `Type<?>` values are
/// heterogeneous; `Arc<dyn Any>` gives Java's shared-reference semantics.
pub type AnyValue = Arc<dyn Any + Send + Sync>;

/// Wraps a typed value in the erased [`AnyValue`].
pub fn any<T: Any + Send + Sync>(value: T) -> AnyValue {
    Arc::new(value)
}

/// `com.mojang.datafixers.types.Type<A>`, with values erased to [`AnyValue`].
///
/// The rewrite cache (a startup optimization and recursion guard in Java,
/// `Type.REWRITE_CACHE`/`PENDING_REWRITE_CACHE`) is omitted: recursive-type
/// rewriting is the deferred layer, and without it the non-recursive rewrite
/// calls are finite, so the cache is unobservable.
pub trait Type<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `Type.read(Dynamic<T>)` — `codec().decode(input)`, remainder preserved.
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)>;

    /// `Type.write(DynamicOps<T>, A)` — `codec().encode(value, ops, prefix)`.
    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output>;

    /// `Type.readTyped(ops, input)` — `read` with the value wrapped in `Typed`.
    fn read_typed(
        &self,
        ops: &Ops,
        input: &Ops::Output,
    ) -> DataResult<(crate::datafixers::typed::Typed<Ops>, Ops::Output)> {
        self.read(ops, input).map_owned(|(v, rest)| {
            (
                crate::datafixers::typed::Typed::new(self.clone_ty(), v),
                rest,
            )
        })
    }

    /// `Type.writeDynamic(ops, A)` — `write` wrapped in a `Dynamic`.
    fn write_dynamic(&self, ops: &Ops, value: &AnyValue) -> DataResult<Dynamic<Ops::Output>> {
        self.write(ops, value, &ops.empty())
            .map_owned(|result| Dynamic::new(ops, result))
    }

    /// `Type.readAndWrite(ops, expectedType, rule, fRule, input)`.
    fn read_and_write(
        &self,
        ops: &Ops,
        expected_type: &dyn Type<Ops>,
        rule: &dyn TypeRewriteRule<Ops>,
        f_rule: &dyn PointFreeRule<Ops>,
        input: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let rewrite_result = self.rewrite(rule, f_rule);
        let rewrite_result = match rewrite_result {
            Some(r) => r,
            None => {
                return DataResult::error(format!(
                    "Could not build a rewrite rule: {:?} {:?}",
                    rule, f_rule
                ));
            }
        };
        if rewrite_result.view.is_nop() {
            return DataResult::success(input.clone());
        }
        self.read(ops, input).flat_map(|(value, rest)| {
            let rewrite_result = rewrite_result.clone();
            let fixed = rewrite_result.view.function.eval_cached()(ops, &value);
            cap_write::<Ops>(expected_type, &rewrite_result.view, ops, &rest, &fixed)
        })
    }

    /// `Type.rewrite(rule, fRule)` — the (cache-less) rewrite entry point.
    fn rewrite(
        &self,
        rule: &dyn TypeRewriteRule<Ops>,
        f_rule: &dyn PointFreeRule<Ops>,
    ) -> Option<RewriteResult<Ops>> {
        let ty = self.clone_ty();
        rule.rewrite(ty.as_ref()).and_then(|r| {
            r.view
                .rewrite(f_rule)
                .map(|view| RewriteResult::create(view, r.rec_data.clone()))
        })
    }

    /// `Type.rewriteOrNop(rule)`.
    fn rewrite_or_nop(&self, rule: &dyn TypeRewriteRule<Ops>) -> RewriteResult<Ops> {
        let ty = self.clone_ty();
        rule.rewrite(ty.as_ref())
            .unwrap_or_else(|| RewriteResult::nop(ty.as_ref()))
    }

    /// `Type.all(rule, recurse, checkIndex)` — rewrite all children.
    fn all(
        &self,
        _rule: &dyn TypeRewriteRule<Ops>,
        _recurse: bool,
        _check_index: bool,
    ) -> RewriteResult<Ops> {
        RewriteResult::nop(&*self.clone_ty())
    }

    /// `Type.one(rule)` — rewrite exactly one child.
    fn one(&self, _rule: &dyn TypeRewriteRule<Ops>) -> Option<RewriteResult<Ops>> {
        None
    }

    /// `Type.everywhere(rule, fRule, recurse, checkIndex)`.
    fn everywhere(
        &self,
        rule: &dyn TypeRewriteRule<Ops>,
        f_rule: &dyn PointFreeRule<Ops>,
        recurse: bool,
        check_index: bool,
    ) -> Option<RewriteResult<Ops>> {
        let rule2 = crate::datafixers::type_rewrite_rule::seq(vec![
            crate::datafixers::type_rewrite_rule::or_else(
                rule.clone_rule(),
                crate::datafixers::type_rewrite_rule::nop(),
            ),
            crate::datafixers::type_rewrite_rule::all(
                crate::datafixers::type_rewrite_rule::everywhere(
                    rule.clone_rule(),
                    f_rule.clone_rule(),
                    recurse,
                    check_index,
                ),
                recurse,
                check_index,
            ),
        ]);
        self.rewrite(rule2.as_ref(), f_rule)
    }

    /// `Type.ifSame(Type, RewriteResult)`.
    fn if_same_rewrite(
        &self,
        target_type: &dyn Type<Ops>,
        value: RewriteResult<Ops>,
    ) -> Option<RewriteResult<Ops>> {
        if self.equals_(target_type, true, true) {
            Some(value)
        } else {
            None
        }
    }

    /// `Type.equals(Object, ignoreRecursionPoints, checkIndex)`.
    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        ignore_recursion_points: bool,
        check_index: bool,
    ) -> bool;

    /// `Type.template()` — the template this type was built from.
    fn template(&self) -> Arc<dyn TypeTemplate<Ops>>;

    /// `Type.point(DynamicOps<?>)` — default value, if possible.
    fn point(&self, _ops: &Ops) -> Option<AnyValue> {
        None
    }

    /// `Type.findChoiceType(name, index)`.
    fn find_choice_type(&self, _name: &str, _index: i32) -> Option<Arc<dyn Type<Ops>>> {
        None
    }

    /// `Type.findCheckedType(index)`.
    fn find_checked_type(&self, _index: i32) -> Option<Arc<dyn Type<Ops>>> {
        None
    }

    /// `Type.findFieldTypeOpt(name)`.
    fn find_field_type_opt(&self, _name: &str) -> Option<Arc<dyn Type<Ops>>> {
        None
    }

    /// Whether this is a `RecursivePointType` (used by `RewriteResult.compose`).
    fn is_recursive_point(&self) -> bool {
        false
    }

    /// `Type.toString()`.
    fn type_to_string(&self) -> String;

    /// Arc-wraps a clone of this type.
    fn clone_ty(&self) -> Arc<dyn Type<Ops>>;

    /// Downcasts the trait object to its concrete Rust type, for the
    /// `equals_` implementations.
    fn as_any_type(&self) -> &dyn Any;
}

/// Java reference-identity comparison over templates (`TypeTemplate.equals` for
/// templates that carry no structural children, e.g. `Const`).
pub fn ptr_eq_templates<Ops: DynamicOps + 'static>(
    a: &dyn TypeTemplate<Ops>,
    b: &dyn TypeTemplate<Ops>,
) -> bool {
    std::ptr::eq(
        a as *const dyn TypeTemplate<Ops> as *const (),
        b as *const dyn TypeTemplate<Ops> as *const (),
    )
}

/// Java reference-identity comparison (`this == o`) for the identity-based
/// types (`PrimitiveType`, `EmptyPart`, `EmptyPartPassthrough`).
pub fn ptr_eq<Ops: DynamicOps + 'static>(a: &dyn Type<Ops>, b: &dyn Type<Ops>) -> bool {
    std::ptr::eq(
        a as *const dyn Type<Ops> as *const (),
        b as *const dyn Type<Ops> as *const (),
    )
}

/// The private `capWrite` step of `readAndWrite`.
fn cap_write<Ops: DynamicOps + 'static>(
    expected_type: &dyn Type<Ops>,
    f: &View<Ops>,
    ops: &Ops,
    rest: &Ops::Output,
    value: &AnyValue,
) -> DataResult<Ops::Output> {
    if !expected_type.equals_(f.output.as_ref(), true, true) {
        return DataResult::error("Rewritten type doesn't match".to_string());
    }
    let fixed = f.function.eval_cached()(ops, value);
    f.output.write(ops, &fixed, rest)
}

/// `com.mojang.datafixers.types.families.TypeFamily`.
pub trait TypeFamily<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `TypeFamily.apply(index)`.
    fn apply(&self, index: i32) -> Arc<dyn Type<Ops>>;
}

/// A closure-backed `TypeFamily` (Java anonymous `TypeFamily`).
pub struct FnFamily<Ops: DynamicOps + 'static> {
    pub f: Arc<dyn Fn(i32) -> Arc<dyn Type<Ops>> + Send + Sync>,
}

impl<Ops: DynamicOps + 'static> Debug for FnFamily<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FnFamily")
    }
}

impl<Ops: DynamicOps + 'static> TypeFamily<Ops> for FnFamily<Ops> {
    fn apply(&self, index: i32) -> Arc<dyn Type<Ops>> {
        (self.f)(index)
    }
}

/// `com.mojang.datafixers.types.templates.TypeTemplate`.
///
/// The `hmap`/`applyO`/`findFieldOrType` members are part of the deferred
/// optics/recursive-rewriting layer and are not declared here.
pub trait TypeTemplate<Ops: DynamicOps + 'static>: Debug + Send + Sync {
    /// `TypeTemplate.size()`.
    fn size(&self) -> i32;

    /// `TypeTemplate.apply(TypeFamily)`.
    fn apply(&self, family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>>;

    /// Structural equality over templates (Java `TypeTemplate.equals`).
    /// Concrete templates implement it structurally over their children.
    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool;

    /// Downcasts the template to its concrete Rust type, for the structural
    /// `template_eq` implementations.
    fn as_any_template(&self) -> &dyn Any;
}

/// A closure-backed `TypeTemplate` factory helper (Java anonymous templates).
pub fn fn_family<Ops: DynamicOps + 'static>(
    f: Arc<dyn Fn(i32) -> Arc<dyn Type<Ops>> + Send + Sync>,
) -> Arc<dyn TypeFamily<Ops>> {
    Arc::new(FnFamily { f })
}

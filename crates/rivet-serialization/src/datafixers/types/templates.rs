//! Port of the `com.mojang.datafixers.types.templates` package.
//!
//! Java's `Const.PrimitiveType<A>` wraps a `Codec<A>`; the type's *value* is
//! the decoded Java value `A`, not the ops element. The port therefore carries
//! the element type `E` and erases it to [`AnyValue`] at the `Type<Ops>`
//! boundary, round-tripping through the existing `rivet-serialization` codecs.
//!
//! The full template surface is the DSL's type-construction vocabulary; the
//! ones the builder foundation actually builds are `Const` (with
//! `PrimitiveType`), `Named`, `EmptyPart`, `EmptyPartPassthrough`, `Func`,
//! and the `Product`/`Sum` pair-or-sum types. Optics-only members
//! (`hmap`/`applyO`/`findFieldOrType`) are deferred.

use crate::codec::{self, Codec};
use crate::data_result::DataResult;
use crate::datafixers::types::{AnyValue, Type, TypeFamily, TypeTemplate, any};
use crate::dynamic::Dynamic;
use crate::dynamic_ops::DynamicOps;
use crate::pair::Pair;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `Const(type)` — a template that ignores its family and returns `type`.
pub struct Const<Ops: DynamicOps + 'static> {
    pub ty: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Const<Ops> {
    pub fn new(ty: Arc<dyn Type<Ops>>) -> Self {
        Const { ty }
    }
}

impl<Ops: DynamicOps + 'static> Debug for Const<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Const[{}]", self.ty.type_to_string())
    }
}

impl<Ops: DynamicOps + 'static> TypeTemplate<Ops> for Const<Ops> {
    fn size(&self) -> i32 {
        0
    }

    fn apply(&self, _family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>> {
        let ty = self.ty.clone();
        crate::datafixers::types::fn_family(Arc::new(move |_index| ty.clone()))
    }

    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool {
        // Java `Const` is a record over `Type<?>`; type equality for the
        // foundation's constant types is pointer identity (the `Const`
        // template is rebuilt per type and the primitive types are singletons).
        crate::datafixers::types::ptr_eq_templates(self, other)
    }

    fn as_any_template(&self) -> &dyn Any {
        self
    }
}

/// `Const.PrimitiveType<A>` — a `Type` whose value is a decoded element `E`.
///
/// `E` is the codec's element type (Java's `A`); the erased `AnyValue` held by
/// `Type<Ops>` stores an `E`.
pub struct PrimitiveType<E, Ops: DynamicOps + 'static> {
    pub codec: Arc<dyn Codec<E, Ops>>,
}

impl<E, Ops: DynamicOps + 'static> Clone for PrimitiveType<E, Ops> {
    fn clone(&self) -> Self {
        PrimitiveType {
            codec: self.codec.clone(),
        }
    }
}

impl<E, Ops: DynamicOps + 'static> Debug for PrimitiveType<E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PrimitiveType")
    }
}

impl<E: Any + Send + Sync + Clone, Ops: DynamicOps + 'static> Type<Ops> for PrimitiveType<E, Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        // Java `PrimitiveType.codec().decode` returns `Pair.of(value, ops.empty())`.
        self.codec
            .decode(ops, input)
            .map_owned(|(value, rest)| (any(value), rest))
    }

    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        // The erased value must be an `E`; a type mismatch is a programming
        // error (Java would ClassCastException).
        let value = value
            .downcast_ref::<E>()
            .expect("PrimitiveType value has the wrong element type");
        self.codec.encode(value, ops, prefix)
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        _ignore_recursion_points: bool,
        _check_index: bool,
    ) -> bool {
        // Java: `this == o` (reference identity).
        crate::datafixers::types::ptr_eq(self, other)
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        Arc::new(Const::new(Arc::new(self.clone())))
    }

    fn type_to_string(&self) -> String {
        "PrimitiveType".to_string()
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `EmptyPart` — the unit type, `Codec.EMPTY`.
#[derive(Debug, Default)]
pub struct EmptyPart<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Clone for EmptyPart<Ops> {
    fn clone(&self) -> Self {
        EmptyPart {
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> EmptyPart<Ops> {
    pub fn new() -> Self {
        EmptyPart {
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for EmptyPart<Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        // Java: `Codec.EMPTY.decode` — `Pair.of(Unit.INSTANCE, ops.empty())`.
        let _ = input;
        let empty = ops.empty();
        DataResult::success((any(crate::unit::Unit), empty))
    }

    fn write(
        &self,
        _ops: &Ops,
        _value: &AnyValue,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        DataResult::success(prefix.clone())
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        _ignore_recursion_points: bool,
        _check_index: bool,
    ) -> bool {
        crate::datafixers::types::ptr_eq(self, other)
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        Arc::new(Const::new(Arc::new(self.clone())))
    }

    fn point(&self, _ops: &Ops) -> Option<AnyValue> {
        Some(any(crate::unit::Unit))
    }

    fn type_to_string(&self) -> String {
        "EmptyPart".to_string()
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `EmptyPartPassthrough` — the remainder type, `Codec.PASSTHROUGH`.
#[derive(Debug, Default)]
pub struct EmptyPartPassthrough<Ops> {
    _m: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> Clone for EmptyPartPassthrough<Ops> {
    fn clone(&self) -> Self {
        EmptyPartPassthrough {
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> EmptyPartPassthrough<Ops> {
    pub fn new() -> Self {
        EmptyPartPassthrough {
            _m: std::marker::PhantomData,
        }
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for EmptyPartPassthrough<Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        // Java: `Codec.PASSTHROUGH.decode` — `Pair.of(new Dynamic<>(ops, input), ops.empty())`.
        let value = any(Dynamic::new(ops, input.clone()));
        let empty = ops.empty();
        DataResult::success((value, empty))
    }

    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        // Java: `Codec.PASSTHROUGH.encode` merges the dynamic into the prefix.
        let codec = codec::passthrough::<Ops>();
        match value.downcast_ref::<Dynamic<Ops::Output>>() {
            Some(d) => codec.encode(d, ops, prefix),
            None => DataResult::success(prefix.clone()),
        }
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        _ignore_recursion_points: bool,
        _check_index: bool,
    ) -> bool {
        crate::datafixers::types::ptr_eq(self, other)
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        Arc::new(Const::new(Arc::new(self.clone())))
    }

    fn point(&self, ops: &Ops) -> Option<AnyValue> {
        Some(any(Dynamic::new(ops, ops.empty())))
    }

    fn type_to_string(&self) -> String {
        "EmptyPartPassthrough".to_string()
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `Func<A, B>` — the function type. Java's `Func` throws
/// `UnsupportedOperationException` on `buildTemplate` and its codec errors on
/// both directions.
pub struct Func<Ops: DynamicOps + 'static> {
    pub first: Arc<dyn Type<Ops>>,
    pub second: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for Func<Ops> {
    fn clone(&self) -> Self {
        Func {
            first: self.first.clone(),
            second: self.second.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for Func<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({} -> {})",
            self.first.type_to_string(),
            self.second.type_to_string()
        )
    }
}

impl<Ops: DynamicOps + 'static> Func<Ops> {
    pub fn new(first: Arc<dyn Type<Ops>>, second: Arc<dyn Type<Ops>>) -> Self {
        Func { first, second }
    }

    pub fn first(&self) -> Arc<dyn Type<Ops>> {
        self.first.clone()
    }

    pub fn second(&self) -> Arc<dyn Type<Ops>> {
        self.second.clone()
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for Func<Ops> {
    fn read(&self, _ops: &Ops, _input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        DataResult::error("Cannot read a function")
    }

    fn write(
        &self,
        _ops: &Ops,
        _value: &AnyValue,
        _prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        DataResult::error("Cannot save a function")
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        ignore_recursion_points: bool,
        check_index: bool,
    ) -> bool {
        if let Some(other) = other.as_any_type().downcast_ref::<Func<Ops>>() {
            self.first
                .equals_(other.first.as_ref(), ignore_recursion_points, check_index)
                && self
                    .second
                    .equals_(other.second.as_ref(), ignore_recursion_points, check_index)
        } else {
            false
        }
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        panic!("No template for function types.")
    }

    fn type_to_string(&self) -> String {
        format!(
            "({} -> {})",
            self.first.type_to_string(),
            self.second.type_to_string()
        )
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `Named(name, element)` — a template tagging an element with a name.
pub struct Named<Ops: DynamicOps + 'static> {
    pub name: String,
    pub element: Arc<dyn TypeTemplate<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Named<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Named[{}]", self.name)
    }
}

impl<Ops: DynamicOps + 'static> TypeTemplate<Ops> for Named<Ops> {
    fn size(&self) -> i32 {
        self.element.size()
    }

    fn apply(&self, family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>> {
        let element_family = self.element.apply(family);
        let name = self.name.clone();
        crate::datafixers::types::fn_family(Arc::new(move |index| {
            let ty = element_family.apply(index);
            Arc::new(NamedType::new(name.clone(), ty)) as Arc<dyn Type<Ops>>
        }))
    }

    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool {
        other
            .as_any_template()
            .downcast_ref::<Named<Ops>>()
            .map(|o| self.name == o.name && self.element.template_eq(o.element.as_ref()))
            .unwrap_or(false)
    }

    fn as_any_template(&self) -> &dyn Any {
        self
    }
}

/// `Named.NamedType` — a `Type<Pair<String, A>>`.
pub struct NamedType<Ops: DynamicOps + 'static> {
    pub name: String,
    pub element: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for NamedType<Ops> {
    fn clone(&self) -> Self {
        NamedType {
            name: self.name.clone(),
            element: self.element.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for NamedType<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NamedType[\"{}\", {}]",
            self.name,
            self.element.type_to_string()
        )
    }
}

impl<Ops: DynamicOps + 'static> NamedType<Ops> {
    pub fn new(name: String, element: Arc<dyn Type<Ops>>) -> Self {
        NamedType { name, element }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn element(&self) -> Arc<dyn Type<Ops>> {
        self.element.clone()
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for NamedType<Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        // Java `NamedType.buildCodec`: decode the element, prepend the name.
        self.element.read(ops, input).map_owned(|(v, rest)| {
            let value = any(Pair::of(self.name.clone(), v));
            (value, rest)
        })
    }

    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        // Java `NamedType.buildCodec`: validate the name matches.
        match value.downcast_ref::<Pair<String, AnyValue>>() {
            Some(p) if p.first == self.name => self.element.write(ops, &p.second, prefix),
            Some(p) => DataResult::error(format!(
                "Named type name doesn't match: expected: {}, got: {}",
                self.name, p.first
            )),
            None => DataResult::error(format!(
                "Named type name doesn't match: expected: {}, got: ?",
                self.name
            )),
        }
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        ignore_recursion_points: bool,
        check_index: bool,
    ) -> bool {
        if let Some(other) = other.as_any_type().downcast_ref::<NamedType<Ops>>() {
            self.name == other.name
                && self.element.equals_(
                    other.element.as_ref(),
                    ignore_recursion_points,
                    check_index,
                )
        } else {
            false
        }
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        Arc::new(Named {
            name: self.name.clone(),
            element: self.element.template(),
        })
    }

    fn point(&self, ops: &Ops) -> Option<AnyValue> {
        self.element
            .point(ops)
            .map(|v| any(Pair::of(self.name.clone(), v)))
    }

    fn find_field_type_opt(&self, name: &str) -> Option<Arc<dyn Type<Ops>>> {
        self.element.find_field_type_opt(name)
    }

    fn find_choice_type(&self, name: &str, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        self.element.find_choice_type(name, index)
    }

    fn find_checked_type(&self, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        self.element.find_checked_type(index)
    }

    fn type_to_string(&self) -> String {
        format!(
            "NamedType[\"{}\", {}]",
            self.name,
            self.element.type_to_string()
        )
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `Product.ProductType<F, G>` — a `Type<Pair<F, G>>` (the `DSL.and` type).
pub struct ProductType<Ops: DynamicOps + 'static> {
    pub first: Arc<dyn Type<Ops>>,
    pub second: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for ProductType<Ops> {
    fn clone(&self) -> Self {
        ProductType {
            first: self.first.clone(),
            second: self.second.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for ProductType<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({}, {})",
            self.first.type_to_string(),
            self.second.type_to_string()
        )
    }
}

impl<Ops: DynamicOps + 'static> ProductType<Ops> {
    pub fn new(first: Arc<dyn Type<Ops>>, second: Arc<dyn Type<Ops>>) -> Self {
        ProductType { first, second }
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for ProductType<Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        self.first.read(ops, input).flat_map(|(f, rest)| {
            self.second
                .read(ops, &rest)
                .map_owned(|(s, rest2)| (any(Pair::of(f, s)), rest2))
        })
    }

    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        match value.downcast_ref::<Pair<AnyValue, AnyValue>>() {
            Some(p) => {
                // Java `ProductType.buildCodec` encodes second first, then first
                // (the pair codec's `encode` order). Match it.
                self.second
                    .write(ops, &p.second, prefix)
                    .flat_map(|rest| self.first.write(ops, &p.first, &rest))
            }
            None => DataResult::error("ProductType value is not a pair"),
        }
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        ignore_recursion_points: bool,
        check_index: bool,
    ) -> bool {
        if let Some(other) = other.as_any_type().downcast_ref::<ProductType<Ops>>() {
            self.first
                .equals_(other.first.as_ref(), ignore_recursion_points, check_index)
                && self
                    .second
                    .equals_(other.second.as_ref(), ignore_recursion_points, check_index)
        } else {
            false
        }
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        // `DSL.and(first.template(), second.template())`.
        Arc::new(Product {
            first: self.first.template(),
            second: self.second.template(),
        })
    }

    fn point(&self, ops: &Ops) -> Option<AnyValue> {
        let f = self.first.point(ops)?;
        let s = self.second.point(ops)?;
        Some(any(Pair::of(f, s)))
    }

    fn find_field_type_opt(&self, name: &str) -> Option<Arc<dyn Type<Ops>>> {
        self.first
            .find_field_type_opt(name)
            .or_else(|| self.second.find_field_type_opt(name))
    }

    fn find_choice_type(&self, name: &str, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        self.first
            .find_choice_type(name, index)
            .or_else(|| self.second.find_choice_type(name, index))
    }

    fn find_checked_type(&self, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        self.first
            .find_checked_type(index)
            .or_else(|| self.second.find_checked_type(index))
    }

    fn type_to_string(&self) -> String {
        format!(
            "({}, {})",
            self.first.type_to_string(),
            self.second.type_to_string()
        )
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `Product(f, g)` — the pair template (`DSL.and`).
pub struct Product<Ops: DynamicOps + 'static> {
    pub first: Arc<dyn TypeTemplate<Ops>>,
    pub second: Arc<dyn TypeTemplate<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Product<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Product")
    }
}

impl<Ops: DynamicOps + 'static> TypeTemplate<Ops> for Product<Ops> {
    fn size(&self) -> i32 {
        self.first.size().max(self.second.size())
    }

    fn apply(&self, family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>> {
        let first_family = self.first.apply(family);
        let second_family = self.second.apply(family);
        crate::datafixers::types::fn_family(Arc::new(move |index| {
            Arc::new(ProductType::new(
                first_family.apply(index),
                second_family.apply(index),
            )) as Arc<dyn Type<Ops>>
        }))
    }

    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool {
        other
            .as_any_template()
            .downcast_ref::<Product<Ops>>()
            .map(|o| {
                self.first.template_eq(o.first.as_ref())
                    && self.second.template_eq(o.second.as_ref())
            })
            .unwrap_or(false)
    }

    fn as_any_template(&self) -> &dyn Any {
        self
    }
}

/// `Sum.SumType<F, G>` — a `Type<Either<F, G>>` (the `DSL.or` type).
pub struct SumType<Ops: DynamicOps + 'static> {
    pub first: Arc<dyn Type<Ops>>,
    pub second: Arc<dyn Type<Ops>>,
}

impl<Ops: DynamicOps + 'static> Clone for SumType<Ops> {
    fn clone(&self) -> Self {
        SumType {
            first: self.first.clone(),
            second: self.second.clone(),
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for SumType<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({} | {})",
            self.first.type_to_string(),
            self.second.type_to_string()
        )
    }
}

impl<Ops: DynamicOps + 'static> SumType<Ops> {
    pub fn new(first: Arc<dyn Type<Ops>>, second: Arc<dyn Type<Ops>>) -> Self {
        SumType { first, second }
    }
}

impl<Ops: DynamicOps + 'static> Type<Ops> for SumType<Ops> {
    fn read(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(AnyValue, Ops::Output)> {
        // Java `SumType.buildCodec` (`Codec.either`) tries first, then second.
        let first_read: DataResult<(AnyValue, Ops::Output)> =
            self.first.read(ops, input).map_owned(|(v, rest)| {
                (
                    any(crate::either::Either::<AnyValue, AnyValue>::left(v)),
                    rest,
                )
            });
        if first_read.is_success() {
            return first_read;
        }
        self.second.read(ops, input).map_owned(|(v, rest)| {
            (
                any(crate::either::Either::<AnyValue, AnyValue>::right(v)),
                rest,
            )
        })
    }

    fn write(&self, ops: &Ops, value: &AnyValue, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        match value.downcast_ref::<crate::either::Either<AnyValue, AnyValue>>() {
            Some(crate::either::Either::Left(v)) => self.first.write(ops, v, prefix),
            Some(crate::either::Either::Right(v)) => self.second.write(ops, v, prefix),
            None => DataResult::error("SumType value is not an Either"),
        }
    }

    fn equals_(
        &self,
        other: &dyn Type<Ops>,
        ignore_recursion_points: bool,
        check_index: bool,
    ) -> bool {
        if let Some(other) = other.as_any_type().downcast_ref::<SumType<Ops>>() {
            self.first
                .equals_(other.first.as_ref(), ignore_recursion_points, check_index)
                && self
                    .second
                    .equals_(other.second.as_ref(), ignore_recursion_points, check_index)
        } else {
            false
        }
    }

    fn template(&self) -> Arc<dyn TypeTemplate<Ops>> {
        Arc::new(Sum {
            first: self.first.template(),
            second: self.second.template(),
        })
    }

    fn point(&self, ops: &Ops) -> Option<AnyValue> {
        // Java tries second first ("least-nested option first").
        if let Some(v) = self.second.point(ops) {
            return Some(any(crate::either::Either::<AnyValue, AnyValue>::right(v)));
        }
        self.first
            .point(ops)
            .map(|v| any(crate::either::Either::<AnyValue, AnyValue>::left(v)))
    }

    fn find_field_type_opt(&self, name: &str) -> Option<Arc<dyn Type<Ops>>> {
        self.first
            .find_field_type_opt(name)
            .or_else(|| self.second.find_field_type_opt(name))
    }

    fn find_choice_type(&self, name: &str, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        self.first
            .find_choice_type(name, index)
            .or_else(|| self.second.find_choice_type(name, index))
    }

    fn find_checked_type(&self, index: i32) -> Option<Arc<dyn Type<Ops>>> {
        self.first
            .find_checked_type(index)
            .or_else(|| self.second.find_checked_type(index))
    }

    fn type_to_string(&self) -> String {
        format!(
            "({} | {})",
            self.first.type_to_string(),
            self.second.type_to_string()
        )
    }

    fn clone_ty(&self) -> Arc<dyn Type<Ops>> {
        Arc::new(self.clone())
    }

    fn as_any_type(&self) -> &dyn Any {
        self
    }
}

/// `Sum(f, g)` — the sum template (`DSL.or`).
pub struct Sum<Ops: DynamicOps + 'static> {
    pub first: Arc<dyn TypeTemplate<Ops>>,
    pub second: Arc<dyn TypeTemplate<Ops>>,
}

impl<Ops: DynamicOps + 'static> Debug for Sum<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sum")
    }
}

impl<Ops: DynamicOps + 'static> TypeTemplate<Ops> for Sum<Ops> {
    fn size(&self) -> i32 {
        self.first.size().max(self.second.size())
    }

    fn apply(&self, family: &dyn TypeFamily<Ops>) -> Arc<dyn TypeFamily<Ops>> {
        let first_family = self.first.apply(family);
        let second_family = self.second.apply(family);
        crate::datafixers::types::fn_family(Arc::new(move |index| {
            Arc::new(SumType::new(
                first_family.apply(index),
                second_family.apply(index),
            )) as Arc<dyn Type<Ops>>
        }))
    }

    fn template_eq(&self, other: &dyn TypeTemplate<Ops>) -> bool {
        other
            .as_any_template()
            .downcast_ref::<Sum<Ops>>()
            .map(|o| {
                self.first.template_eq(o.first.as_ref())
                    && self.second.template_eq(o.second.as_ref())
            })
            .unwrap_or(false)
    }

    fn as_any_template(&self) -> &dyn Any {
        self
    }
}

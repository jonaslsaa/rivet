//! Port of `ca.spottedleaf.dataconverter.types.TypeUtil` — the per-backing
//! conversion interface (NBT and JSON backings implement it).
//!
//! Java's `TypeUtil<T>` is used in two shapes:
//!   - as a **concrete** `TypeUtil<D>` (e.g. `Types.JSON`) when a converter
//!     drives `convertBaseToBase`/`convertGenericToBase` and needs
//!     `genericToBase` on the destination — Rust models this with the generic
//!     [`TypeUtil<D>`] bound, never a trait object;
//!   - as a **wildcard** `TypeUtil<?>` (e.g. `root.getTypeUtil()` or the `to`
//!     of `convertTo`) where the callee only ever calls `createEmptyList`/
//!     `createEmptyMap` — Rust models this with the object-safe [`TypeUtilBase`]
//!     supertrait, which is what `MapType::get_type_util`/`ListType::get_type_util`
//!     return.
//!
//! The split is the faithful translation of the Java wildcard; every call site
//! under `ca.spottedleaf.dataconverter` that holds `TypeUtil<?>` only invokes the
//! two factory methods (verified against the pinned sources — e.g. V4068,
//! V4290, V4059).
//!
//! The abstract conversion members (`convert_to`, `base_to_generic`,
//! `generic_to_base`) are implemented by the concrete NBT/JSON backings
//! (`types.nbt`/`types.json`, later manifest units). The Java `Object`
//! parameter/return of these is the boxed [`Generic`].

use crate::server::dataconverter::types::generic::Generic;
use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;

/// The wildcard-usable surface of a `TypeUtil` — Java `TypeUtil<?>` callers
/// only invoke these two factory methods.
pub trait TypeUtilBase {
    /// `TypeUtil.createEmptyList()`.
    fn create_empty_list(&self) -> Box<dyn ListType>;
    /// `TypeUtil.createEmptyMap()`.
    fn create_empty_map(&self) -> Box<dyn MapType>;
}

/// `TypeUtil<T>` — the per-backing conversion interface.
///
/// `T` is the base value type of the backing (`Tag` for NBT,
/// `serde_json::Value` for JSON).
pub trait TypeUtil<T>: TypeUtilBase {
    /// `TypeUtil.convertTo(Object, TypeUtil<?>)` — convert a generic value to
    /// the destination backing's generic form. `None` is Java `null` (passed
    /// through unchanged, or the NBT `EndTag`/JSON `JsonNull` absence).
    ///
    /// Concrete backings return the input unchanged for `None`/`Str`/`Bool`,
    /// re-box their own `ListType`/`MapType`/`Number`/arrays into the
    /// destination, and throw `IllegalStateException` for unrecognized values.
    fn convert_to(&self, value_generic: Option<&Generic>, to: &dyn TypeUtilBase)
    -> Option<Generic>;

    /// `TypeUtil.baseToGeneric(T)` — unwrap a backing value into the generic
    /// form (`None` for the absence value).
    fn base_to_generic(&self, input: &T) -> Option<Generic>;

    /// `TypeUtil.genericToBase(Object)` — wrap a generic value back into the
    /// backing's base type (Java `null` → the backing's empty/absent value).
    fn generic_to_base(&self, input: Option<&Generic>) -> T;

    /// `TypeUtil.convertFromBaseToGeneric(T, TypeUtil<?>)`.
    fn convert_from_base_to_generic(&self, input: &T, to: &dyn TypeUtilBase) -> Option<Generic> {
        let generic = self.base_to_generic(input);
        self.convert_to(generic.as_ref(), to)
    }

    /// `TypeUtil.convertBaseToBase(T, TypeUtil<D>)`.
    fn convert_base_to_base<D, U: TypeUtil<D>>(&self, input: &T, to: &U) -> D {
        let generic = self.convert_from_base_to_generic(input, to);
        to.generic_to_base(generic.as_ref())
    }

    /// `TypeUtil.convertGenericToBase(Object, TypeUtil<D>)`.
    fn convert_generic_to_base<D, U: TypeUtil<D>>(
        &self,
        value_generic: Option<&Generic>,
        to: &U,
    ) -> D {
        let generic = self.convert_to(value_generic, to);
        to.generic_to_base(generic.as_ref())
    }
}

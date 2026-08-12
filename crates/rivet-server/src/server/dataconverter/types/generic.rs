//! The boxed generic value — the Rust model of the `Object` slot that the
//! DataConverter type layer traffics in.
//!
//! Java's `MapType.getGeneric`/`setGeneric` and `ListType.getGeneric`/
//! `setGeneric`/`addGeneric` operate on `Object`; the concrete boxed forms are
//! the boxed `Number` subtypes, `Boolean`, `String`, the four primitive arrays,
//! and the `MapType`/`ListType` containers (which alias their backing — see the
//! module docs). `Generic` is the closed enum of exactly those forms, so the
//! Java `instanceof` dispatch in the default methods becomes a `match`.

use crate::server::dataconverter::types::list_type::ListType;
use crate::server::dataconverter::types::map_type::MapType;
use rivet_serialization::number::Number;

/// A boxed generic value (`java.lang.Object` in the DataConverter layer).
///
/// Not `Debug`/`Clone`-derived: the `Map`/`List` variants hold trait objects
/// (`Box<dyn MapType>`/`Box<dyn ListType>`), which are neither.
pub enum Generic {
    /// `java.lang.Boolean` — NBT stores booleans as `ByteTag` and JSON as a
    /// boolean primitive, so a `Generic::Bool` can only ever originate from a
    /// JSON backing or an explicit `Boolean` argument.
    Bool(bool),
    /// `java.lang.Byte`.
    Byte(i8),
    /// `java.lang.Short`.
    Short(i16),
    /// `java.lang.Integer`.
    Int(i32),
    /// `java.lang.Long`.
    Long(i64),
    /// `java.lang.Float`.
    Float(f32),
    /// `java.lang.Double`.
    Double(f64),
    /// `java.lang.String`.
    Str(String),
    /// `byte[]` (copied on extraction).
    Bytes(Vec<i8>),
    /// `short[]` — NBT has no short-array tag; this form only appears via an
    /// explicit `short[]` argument or a JSON backing (which never emits raw
    /// arrays), so in practice it is inert for NBT.
    Shorts(Vec<i16>),
    /// `int[]`.
    Ints(Vec<i32>),
    /// `long[]`.
    Longs(Vec<i64>),
    /// A `MapType` — boxed so the concrete backing can alias its storage.
    Map(Box<dyn MapType>),
    /// A `ListType` — boxed so the concrete backing can alias its storage.
    List(Box<dyn ListType>),
}

impl Generic {
    /// `CopyHelper.sanitizeNumber`-style validation: `Number` variants are
    /// exactly the six boxed forms. The NBT/JSON `TypeUtil.convertTo` throw
    /// `IllegalStateException("Unknown type: ...")` for anything else; those
    /// are concrete-backing decisions, not represented here.
    pub fn as_number(&self) -> Option<Number> {
        match self {
            Generic::Byte(v) => Some(Number::Byte(*v)),
            Generic::Short(v) => Some(Number::Short(*v)),
            Generic::Int(v) => Some(Number::Int(*v)),
            Generic::Long(v) => Some(Number::Long(*v)),
            Generic::Float(v) => Some(Number::Float(*v)),
            Generic::Double(v) => Some(Number::Double(*v)),
            _ => None,
        }
    }
}

//! Port of `ca.spottedleaf.dataconverter.types` — the abstract type layer.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/dataconverter/types/`
//!   - `ObjectType.java` — the uniform-type enum and `getType(Object)` dispatch;
//!   - `ListType.java` / `MapType.java` — the container interfaces plus their
//!     generic-dispatch defaults;
//!   - `TypeUtil.java` — the per-backing conversion interface (NBT/JSON);
//!   - `Types.java` — the concrete `NBT`/`JSON`/`JSON_COMPRESSED` singletons.
//!
//! The concrete backings (`types.nbt`, `types.json`) and the `Types` handles are
//! later manifest units; this scaffold carries only the abstract contracts plus
//! the boxed [`generic::Generic`] value that models Java's runtime `Object`
//! slot (see [`crate::server::dataconverter`] for the object-model notes).

pub mod generic;
pub mod list_type;
pub mod map_type;
pub mod object_type;
pub mod type_util;

/// Test-only reference backings used by the container tests; compiled only in
/// test builds (the concrete NBT/JSON backings are later manifest units).
#[cfg(test)]
pub(crate) mod test_support;

// RivetTodo(#535): `Types.java`'s `NBT`/`JSON`/`JSON_COMPRESSED` handles are
// provided by the `ca.spottedleaf.dataconverter.types.nbt` /
// `ca.spottedleaf.dataconverter.types.json` manifest units (wave 4), which
// implement the concrete `NBTTypeUtil`/`JsonTypeUtil` behind [`type_util::TypeUtil`].

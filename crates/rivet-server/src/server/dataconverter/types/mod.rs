//! Port of `ca.spottedleaf.dataconverter.types` — the abstract and concrete
//! DataConverter type layer.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/dataconverter/types/`
//!   - `ObjectType.java` — the uniform-type enum and `getType(Object)` dispatch;
//!   - `ListType.java` / `MapType.java` — the container interfaces plus their
//!     generic-dispatch defaults;
//!   - `TypeUtil.java` — the per-backing conversion interface (NBT/JSON);
//!   - `Types.java` — the concrete `NBT`/`JSON`/`JSON_COMPRESSED` singletons;
//!   - `nbt/` — `NBTListType`, `NBTMapType`, `NBTTypeUtil`;
//!   - `json/` — `JsonListType`, `JsonMapType`, `JsonTypeUtil`.

pub mod generic;
pub mod json;
pub mod list_type;
pub mod map_type;
pub mod nbt;
pub mod object_type;
pub mod type_util;
#[allow(clippy::module_inception)]
pub mod types;

pub use types::Types;

/// Test-only reference backings used by the container tests; compiled only in
/// test builds.
#[cfg(test)]
pub(crate) mod test_support;

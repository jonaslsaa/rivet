//! Port of `ca.spottedleaf.dataconverter` — the Spottedleaf world-data
//! conversion layer used on the loaded-world path (issue #535).
//!
//! This module mirrors the Java package layout under
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/dataconverter/`.
//! The first slice ports the shared scaffold: `types` (the abstract
//! `ListType`/`MapType`/`TypeUtil`/`ObjectType` layer plus the boxed `Generic`
//! value that travels between NBT and JSON backings — `Types.java`'s concrete
//! `NBT`/`JSON` handles are wired by the later `types.nbt`/`types.json` units),
//! `converters` (`DataConverter`: version encoding + ordering), and
//! `converters::datatypes` (`DataHook` / `DataType` / `DataWalker`).
//!
//! The concrete NBT and JSON type implementations (`types.nbt`, `types.json`)
//! and the `minecraft.*` converter waves are deliberately kept out of this
//! scaffold slice.
//!
//! ## Rust object model
//!
//! Java's interface + runtime `instanceof Object` dispatch maps to a Rust trait
//! together with the boxed [`types::Generic`] enum. `ListType`/`MapType` are
//! object-safe traits implemented by the concrete NBT/JSON backings; the Java
//! `getMap`/`getList` "view" methods return a fresh
//! `Box<dyn MapType>`/`Box<dyn ListType>` wrapping the same underlying backing,
//! so a mutation through a returned view is visible in the parent — the concrete
//! backings use shared (single-threaded) interior storage to make that aliasing
//! exact.
//!
//! The value layer is single-threaded data munging, not game state: it does not
//! participate in the OWNERSHIP arena model.

pub mod converters;
pub mod types;

//! Port of `ca.spottedleaf.dataconverter.converters.datatypes` — the abstract
//! hook / type / walker contracts.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/dataconverter/converters/datatypes/`
//!   - `DataHook.java` — pre/post hooks around a conversion;
//!   - `DataType.java` — the abstract data-type with `convertOrOriginal`;
//!   - `DataWalker.java` — structure walkers with the `NO_OP` singleton.

pub mod data_hook;
pub mod data_type;
pub mod data_walker;

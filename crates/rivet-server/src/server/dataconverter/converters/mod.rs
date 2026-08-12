//! Port of `ca.spottedleaf.dataconverter.converters` — the converter dispatch
//! layer.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/ca/spottedleaf/dataconverter/converters/`
//!   - `DataConverter.java` — version encoding + ordering;
//!   - `datatypes/DataHook.java`, `DataType.java`, `DataWalker.java` — the
//!     abstract hook/type/walker contracts the minecraft datatype layer drives.
//!
//! The `minecraft.datatypes` consumers (`MCDataType`/`MCValueType`) and all
//! concrete converters/walkers are later manifest units.

pub mod data_converter;
pub mod datatypes;

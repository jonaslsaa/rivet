//! `String.valueOf(float/double)` parity — Java `Float.toString` /
//! `Double.toString`. The canonical implementation lives in
//! `rivet-serialization`'s `float_format` module (used by the JSON
//! `createFloat` path too); this module re-exports it for the NBT/SNBT
//! visitors (`StringTagVisitor.visitFloat`/`visitDouble`,
//! `TextComponentTagVisitor`).

pub use rivet_serialization::float_format::{java_double_to_string, java_float_to_string};

//! `net.minecraft.IdentifierException` — re-exported from `rivet-core`.
//!
//! The type is owned by `rivet-core` (the Rust home of the `net.minecraft`
//! root package, per the analyzer's `crate_for`). The #124 SCC units use it
//! from here so `identifier.rs` reads naturally; this module is ownership A's
//! seam and must not duplicate the definition.
//!
//! The full port (message escaping via `StringEscapeUtils.escapeJava`, the
//! `(message, cause)` variant) lives in `rivet_core::identifier_exception`;
//! the type re-exported here follows it.

pub use rivet_core::IdentifierException;

//! Port of the `com.mojang.datafixers` DataFixerUpper builder foundation.
//!
//! This is the core of issue #534: the `DataFixerBuilder` lifecycle
//! (`addSchema` parent computation, `addFixer` version gating, `build`
//! snapshotting), the `DataFixerUpper` update/getRule/getSchema surface, the
//! `Schema` type-building, and the `Type`/`TypeRewriteRule`/`View`/
//! `RewriteResult`/`PointFree` machinery they stand on.
//!
//! Values are erased to `Arc<dyn Any>` (Java's `Type<?>` wildcard) and the ops
//! are pinned as a type parameter, matching the rest of `rivet-serialization`.
//! The optics/recursive-rewrite layer is a separate, larger unit and is
//! deferred (see `schema.rs` and `functions/rule.rs` for the exact boundaries).

pub mod data_fix;
pub mod data_fix_utils;
pub mod data_fixer_builder;
pub mod data_fixer_upper;
pub mod functions;
pub mod rewrite_result;
pub mod schemas;
pub mod type_rewrite_rule;
pub mod typed;
pub mod types;
pub mod view;

pub use data_fixer_builder::DataFixerBuilder;
pub use data_fixer_upper::DataFixerUpper;
pub use schemas::Schema;
pub use typed::Typed;
pub use view::View;

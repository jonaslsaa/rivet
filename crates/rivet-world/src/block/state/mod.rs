//! `net.minecraft.world.level.block.state` — module mirror for the
//! state-predicate sub-package (issue #228).
//!
//! The value surface of the `mc.world.level.block.state` unit (`BlockState`,
//! `StateDefinition`, `Property`) lives in `rivet-registry` (issue #228), where
//! it decodes the generated tables without a world dependency; this module
//! exists only to mirror the Java package path for `state.predicate`.

pub mod predicate;

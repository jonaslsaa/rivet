//! `net.minecraft.world.level.levelgen.structure` — structure generation.
//!
//! Only the `templatesystem` slice is ported so far: the
//! `mc.world.level.levelgen.structure.templatesystem.rules` unit (issue #182) —
//! the `RuleTest`/`RuleTestType` and `PosRuleTest`/`PosRuleTestType` dispatch
//! families (see `templatesystem`). The structure/template machinery that
//! consumes these rule tests (the owning `mc.world.level.levelgen.structure`
//! units) lands with that wave.

pub mod templatesystem;

//! `net.minecraft.world.level.levelgen.structure.templatesystem` — the
//! template-system rule tests (issue #182).
//!
//! This is the `mc.world.level.levelgen.structure.templatesystem.rules` unit:
//! the eight-file `RuleTest`/`RuleTestType` family and the five-file
//! `PosRuleTest`/`PosRuleTestType` family. Java source of truth is
//! `working/Paper/.../templatesystem/` (MC 26.2).
//!
//! The two families are value predicates consulted while placing structure
//! templates: `RuleTest` tests a `BlockState` (against the random source),
//! `PosRuleTest` tests a position triple. Both are closed registries whose
//! `CODEC` is `BuiltInRegistries.*.byNameCodec().dispatch("predicate_type",
//! getType, *RuleTestType::codec)` — the same closed-registry identity split
//! the `blockpredicates`/`placement` slices use: the behavior trait is generic
//! over the random source (`RandomSource` is `Sized`, not object-safe), the
//! dispatch value is the erased carrier `Arc<dyn Erased( Pos)?RuleTest>`, and
//! the concrete `MapCodec`s are resolved by an in-module dispatch table.

pub mod always_true_test;
pub mod axis_aligned_linear_pos_test;
pub mod axis_codec;
pub mod block_match_test;
pub mod block_state_codec;
pub mod block_state_match_test;
#[cfg(test)]
pub mod codec_test_util;
pub mod linear_pos_test;
pub mod optional_field_codecs;
pub mod pos_always_true_test;
pub mod pos_rule_test;
pub mod pos_rule_test_type;
pub mod random_block_match_test;
pub mod random_block_state_match_test;
pub mod rule_test;
pub mod rule_test_type;
pub mod tag_match_test;

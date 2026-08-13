//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.PosAlwaysTrueTest`
//! (class, 26.2).
//!
//! Java: a singleton (`INSTANCE`) whose `test` is always true and whose
//! `type()` is `PosRuleTestType.ALWAYS_TRUE_TEST`. Its `CODEC` is
//! `MapCodec.unit(INSTANCE)` — encodes to `{}` and always decodes to the
//! singleton. The codec is ported here (as the ops-generic
//! `pos_always_true_test_map_codec::<Ops>()` factory) and lifted to the erased
//! carrier in `pos_rule_test`.

use crate::levelgen::structure::templatesystem::pos_rule_test::PosRuleTest;
use crate::levelgen::structure::templatesystem::pos_rule_test_type::{
    PosRuleTestTypeId, PosRuleTestTypes,
};
use rivet_registry::core::BlockPos;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.PosAlwaysTrueTest`.
///
/// `Clone` mirrors the Java singleton (`INSTANCE`) — cloning yields the same
/// always-true position rule test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosAlwaysTrueTest;

impl PosAlwaysTrueTest {
    /// `PosAlwaysTrueTest.INSTANCE`.
    pub const INSTANCE: PosAlwaysTrueTest = PosAlwaysTrueTest;
}

impl PosRuleTest for PosAlwaysTrueTest {
    /// `PosAlwaysTrueTest.test` — always true.
    fn test<R: RandomSource>(
        &self,
        _in_template_pos: &BlockPos,
        _world_pos: &BlockPos,
        _world_reference: &BlockPos,
        _random: &mut R,
    ) -> bool {
        true
    }

    fn type_id(&self) -> PosRuleTestTypeId {
        PosRuleTestTypes::ALWAYS_TRUE_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `PosAlwaysTrueTest.CODEC` — `MapCodec.unit(INSTANCE)`, as the ops-generic
/// `pos_always_true_test_map_codec::<Ops>()` factory.
pub fn pos_always_true_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<PosAlwaysTrueTest, Ops>> {
    map_codec::unit_with(Arc::new(|| PosAlwaysTrueTest::INSTANCE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_true() {
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert!(PosAlwaysTrueTest::INSTANCE.test(
            &BlockPos::ZERO,
            &BlockPos::new(0, 0, 0),
            &BlockPos::new(1, 2, 3),
            &mut random
        ));
    }

    #[test]
    fn type_identity() {
        assert_eq!(
            PosRuleTest::type_id(&PosAlwaysTrueTest::INSTANCE),
            PosRuleTestTypes::ALWAYS_TRUE_TEST
        );
    }
}

//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.AlwaysTrueTest`
//! (class, 26.2).
//!
//! Java: a singleton (`INSTANCE`) whose `test` is always true and whose
//! `type()` is `RuleTestType.ALWAYS_TRUE_TEST`. Its `CODEC` is
//! `MapCodec.unit(INSTANCE)` — encodes to `{}` and always decodes to the
//! singleton. `testAgainstWorldState` is overridden to return `true` without
//! touching the level (the only rule test that avoids the
//! capability-unavailable `getBlockState` seam). The codec is ported here (as
//! the ops-generic `always_true_test_map_codec::<Ops>()` factory) and lifted to
//! the erased carrier in `rule_test`.

use crate::level::WorldGenLevel;
use crate::levelgen::structure::templatesystem::rule_test::RuleTest;
use crate::levelgen::structure::templatesystem::rule_test_type::{RuleTestTypeId, RuleTestTypes};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.AlwaysTrueTest`.
///
/// `Clone` mirrors the Java singleton (`INSTANCE`) — cloning yields the same
/// always-true rule test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlwaysTrueTest;

impl AlwaysTrueTest {
    /// `AlwaysTrueTest.INSTANCE`.
    pub const INSTANCE: AlwaysTrueTest = AlwaysTrueTest;
}

impl RuleTest for AlwaysTrueTest {
    /// `AlwaysTrueTest.test` — always true.
    fn test<R: RandomSource>(&self, _state: &BlockState, _random: &mut R) -> bool {
        true
    }

    fn type_id(&self) -> RuleTestTypeId {
        RuleTestTypes::ALWAYS_TRUE_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// `AlwaysTrueTest.testAgainstWorldState` — overridden to `true` without
    /// touching the level (Java's `@Override`; the only rule test that avoids
    /// the world-state read).
    fn test_against_world_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        _pos: &BlockPos,
        _random: &mut R,
    ) -> bool {
        true
    }
}

/// `AlwaysTrueTest.CODEC` — `MapCodec.unit(INSTANCE)`, as the ops-generic
/// `always_true_test_map_codec::<Ops>()` factory.
pub fn always_true_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<AlwaysTrueTest, Ops>> {
    map_codec::unit_with(Arc::new(|| AlwaysTrueTest::INSTANCE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_true() {
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let air = crate::block::Block::from_name("minecraft:air")
            .unwrap()
            .default_block_state();
        assert!(AlwaysTrueTest::INSTANCE.test(&air, &mut random));
        // Java overrides `testAgainstWorldState` to return true without
        // touching the level — the port's trait override dispatches to that
        // (a read would panic on the hostile test double).
        assert!(AlwaysTrueTest::INSTANCE.test_against_world_state(
            &capability_gap_level(),
            &BlockPos::ZERO,
            &mut random
        ));
    }

    #[test]
    fn type_identity() {
        assert_eq!(
            RuleTest::type_id(&AlwaysTrueTest::INSTANCE),
            RuleTestTypes::ALWAYS_TRUE_TEST
        );
    }

    use crate::level::height_accessor::LevelHeightAccessor;

    fn capability_gap_level() -> CapabilityGapLevel {
        CapabilityGapLevel
    }

    #[derive(Clone, Copy)]
    struct CapabilityGapLevel;

    impl LevelHeightAccessor for CapabilityGapLevel {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for CapabilityGapLevel {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            panic!("getBlockState unavailable on this test double")
        }
    }
}

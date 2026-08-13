//! Port of `net.minecraft.world.level.levelgen.placement.PlacementFilter`
//! (abstract class, 26.2).
//!
//! Java: a `PlacementModifier` whose `getPositions` is `final` and delegates to
//! the abstract `shouldPlace` — pass the origin through if it passes, else
//! emit nothing. The Rust port mirrors the template method: `PlacementFilter`
//! is a standalone trait (concrete filters implement `shouldPlace` + `type_id`
//! only) and the blanket `impl PlacementModifier` provides the non-overridable
//! `get_positions` shell, matching Java's `final`.

use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId;
use crate::levelgen::placement::{PlacementContext, PlacementModifier};
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use std::fmt::Debug;

/// `net.minecraft.world.level.levelgen.placement.PlacementFilter` — a
/// modifier that either keeps its origin position or drops it.
///
/// Concrete filters (`RarityFilter`, `BiomeFilter`, …) implement
/// `should_place` + `type_id`; the `PlacementModifier` impl is provided by the
/// blanket impl below (Java's `final getPositions`).
pub trait PlacementFilter: Debug + Send + Sync + 'static {
    /// `shouldPlace(PlacementContext, RandomSource, BlockPos)` — the
    /// overridable predicate.
    fn should_place<R: RandomSource>(
        &self,
        context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool;

    /// `type()` — the registry-held `PlacementModifierType<?>` identity (same
    /// as `PlacementModifier.type()`).
    fn type_id(&self) -> PlacementModifierTypeId;
}

/// The `final getPositions` shell: `shouldPlace ? Stream.of(origin) :
/// Stream.of()`, plus the filter's own type identity. As a blanket impl on
/// every `PlacementFilter` it cannot be overridden, matching Java's `final`;
/// `type_id` delegates to the `PlacementFilter` method to avoid recursing
/// through the blanket `PlacementModifier` impl.
impl<F: PlacementFilter + ?Sized> PlacementModifier for F {
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        if self.should_place(context, random, origin) {
            Box::new(std::iter::once(*origin))
        } else {
            Box::new(std::iter::empty())
        }
    }

    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementFilter::type_id(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use rivet_util::random::LegacyRandomSource;

    /// A minimal `WorldGenLevel` double over the overworld window.
    struct TestLevel(SimpleLevelHeightAccessor);

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.0.get_height()
        }

        fn get_min_y(&self) -> i32 {
            self.0.get_min_y()
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            // RivetTodo(#399): no real world-access implementation is present —
            // the state-testing predicates surface the unavailable capability
            // explicitly (see `StateTestingPredicate::test`).
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    struct NoopGenerator;

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// A filter that keeps the origin when the random draw is even.
    #[derive(Debug)]
    struct EvenFilter;

    impl PlacementFilter for EvenFilter {
        fn should_place<R: RandomSource>(
            &self,
            _context: &PlacementContext,
            random: &mut R,
            _origin: &BlockPos,
        ) -> bool {
            random.next_int() % 2 == 0
        }

        fn type_id(&self) -> PlacementModifierTypeId {
            PlacementModifierTypeId::new(4, "minecraft:biome")
        }
    }

    /// `PlacementModifier::get_positions` on a `PlacementFilter` — the blanket
    /// impl is Java's `final getPositions`.
    fn filter_positions(filter: &EvenFilter, random: &mut LegacyRandomSource) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let context = PlacementContext::new(&mut level, &generator, None);
        let origin = BlockPos::new(1, 2, 3);
        PlacementModifier::get_positions(filter, &context, random, &origin).collect()
    }

    #[test]
    fn filter_keeps_origin_when_should_place() {
        // `shouldPlace ? Stream.of(origin) : Stream.of()` — the even-draw
        // branch keeps exactly the origin.
        let filter = EvenFilter;
        let mut random = LegacyRandomSource::new(0);
        let result = filter_positions(&filter, &mut random);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get_x(), 1);
        assert_eq!(result[0].get_y(), 2);
        assert_eq!(result[0].get_z(), 3);
    }

    #[test]
    fn filter_drops_origin_when_not_should_place() {
        let filter = EvenFilter;
        let mut random = LegacyRandomSource::new(1);
        let result = filter_positions(&filter, &mut random);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_type_identity_is_reported() {
        // The blanket `PlacementModifier::type_id` delegates to the filter's
        // own identity — no recursion through the blanket impl.
        // `PlacementModifierType.BIOME_FILTER` is insertion index 4 in
        // `PlacementModifierType.java`'s registration order.
        let filter = EvenFilter;
        let expected = PlacementModifierTypeId::new(4, "minecraft:biome");
        assert_eq!(PlacementModifier::type_id(&filter), expected);
        assert_eq!(PlacementFilter::type_id(&filter), expected);
    }
}

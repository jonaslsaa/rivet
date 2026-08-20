//! Port of `net.minecraft.world.level.levelgen.placement.RepeatingPlacement`
//! (abstract class, 26.2).
//!
//! Java is the template-method base of the count modifiers
//! (`CountPlacement`, `NoiseBasedCountPlacement`, `NoiseThresholdCountPlacement`):
//! the abstract `count(RandomSource, BlockPos)` hook plus the shared
//! `getPositions` shell — `IntStream.range(0, count(random, origin)).mapToObj(i
//! -> origin)` — which every concrete subclass inherits (none overrides it).
//!
//! Lazy-materialization parity: Java's shell returns a lazy `IntStream.range(0,
//! count)` (see the inline note at the shell). `count` is unbounded
//! (`NoiseBasedCountPlacement`'s codec accepts a plain `Codec.INT` ratio and its
//! `count()` saturates to `i32::MAX`), so Java degrades to a slow lazy pull; the
//! port's `Box<dyn Iterator>` shell (`std::iter::repeat_n`) reproduces exactly
//! that — each `next()` hands back the origin, no `count`-length allocation.
//!
//! `PlacementModifier` is a standalone trait in the port (not a superclass),
//! so the base is ported as a trait with a *provided* `get_positions` default
//! rather than `PlacementFilter`'s blanket-impl shape: `placement_filter.rs`
//! already reserves the blanket `impl<F: PlacementFilter> PlacementModifier
//! for F`, and a second blanket impl over `RepeatingPlacement` would overlap
//! with it (E0119 — a type could implement both traits). Each concrete
//! modifier therefore implements `PlacementModifier` and delegates
//! `get_positions` to this trait's provided shell, exactly mirroring Java's
//! inherited non-overridden method. `context` is ignored in the shell exactly
//! as in Java's `getPositions` body.

use crate::levelgen::placement::PlacementContext;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use std::fmt::Debug;

/// `net.minecraft.world.level.levelgen.placement.RepeatingPlacement` — the
/// abstract base of the repeating count modifiers.
///
/// Concrete modifiers implement [`count`](RepeatingPlacement::count) (and
/// their `PlacementModifier::type_id`); the inherited `get_positions` shell is
/// provided as a default method they delegate to.
pub trait RepeatingPlacement: Debug + Send + Sync + 'static {
    /// `count(RandomSource, BlockPos)` — the abstract hook each concrete
    /// modifier implements (Java's `protected abstract int count(...)`).
    fn count<R: RandomSource>(&self, random: &mut R, origin: &BlockPos) -> i32;

    /// `getPositions(PlacementContext, RandomSource, BlockPos)` — the shared
    /// shell: `IntStream.range(0, count(random, origin)).mapToObj(i -> origin)`.
    /// `context` is unused exactly as in Java; the `IntStream.range` semantics
    /// (empty for `count <= 0`, half-open `[0, count)`) are reproduced by
    /// `repeat_n(*origin, count.max(0))` (zero `count` yields an empty
    /// iterator, positive `count` yields exactly that many origin copies).
    ///
    /// The shell is **lazy** — it returns `std::iter::repeat_n(*origin,
    /// count.max(0))` without materializing `count` positions (Java's
    /// `IntStream.range` is equally lazy). `count` is unbounded —
    /// `NoiseBasedCountPlacement`'s codec accepts a plain `Codec.INT` ratio and
    /// its `count()` saturates to `i32::MAX`, so Java degrades to a slow lazy
    /// pull; the iterator reproduces exactly that (each `next()` hands back the
    /// origin, no `count`-length allocation). A caller that pushes every element
    /// into a `Vec` re-introduces the eager shape, but the placement walk
    /// consumes the iterator incrementally.
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        _context: &mut PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        let count = self.count(random, origin);
        Box::new(std::iter::repeat_n(*origin, count.max(0) as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::placement::PlacementModifier;
    use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId;
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
            // RivetTodo(#399): no real world-access implementation is present.
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

    /// A repeating modifier whose count is the fixed value, exercising the
    /// shell directly.
    #[derive(Debug)]
    struct FixedRepeat(i32);

    impl RepeatingPlacement for FixedRepeat {
        fn count<R: RandomSource>(&self, _random: &mut R, _origin: &BlockPos) -> i32 {
            self.0
        }
    }

    impl PlacementModifier for FixedRepeat {
        fn get_positions<'a, R: RandomSource>(
            &'a self,
            context: &mut PlacementContext,
            random: &mut R,
            origin: &BlockPos,
        ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
            RepeatingPlacement::get_positions(self, context, random, origin)
        }

        fn type_id(&self) -> PlacementModifierTypeId {
            // Not the real identity (a test double) — the concrete modifiers
            // own their registry identities.
            PlacementModifierTypeId::new(5, "minecraft:count")
        }
    }

    fn positions(repeat: &FixedRepeat, origin: &BlockPos) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let mut random = LegacyRandomSource::new(0);
        // UFCS: `FixedRepeat` implements both `PlacementModifier` and
        // `RepeatingPlacement`, each with a `get_positions` — Java's single
        // inherited method maps to the `PlacementModifier` trait entry.
        <FixedRepeat as PlacementModifier>::get_positions(repeat, &mut context, &mut random, origin)
            .collect()
    }

    #[test]
    fn shell_emits_count_copies_of_the_origin() {
        // `IntStream.range(0, count).mapToObj(i -> origin)` — count positions,
        // every one the origin.
        let origin = BlockPos::new(1, 2, 3);
        let result = positions(&FixedRepeat(4), &origin);
        assert_eq!(result.len(), 4);
        for pos in &result {
            assert_eq!(*pos, origin);
        }
    }

    #[test]
    fn shell_emits_nothing_for_zero_count() {
        // `IntStream.range(0, 0)` is empty.
        let origin = BlockPos::new(1, 2, 3);
        assert!(positions(&FixedRepeat(0), &origin).is_empty());
    }

    #[test]
    fn shell_emits_nothing_for_negative_count() {
        // `IntStream.range(0, -1)` is empty (half-open range with start >= end).
        let origin = BlockPos::new(1, 2, 3);
        assert!(positions(&FixedRepeat(-1), &origin).is_empty());
    }
}

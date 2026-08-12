//! Port of `net.minecraft.world.level.levelgen.placement.PlacementModifier`
//! (abstract class, 26.2).
//!
//! Java is the abstract base of every placement modifier; it carries the
//! dispatch codec `CODEC` (`BuiltInRegistries.PLACEMENT_MODIFIER_TYPE
//! .byNameCodec().dispatch(PlacementModifier::type, PlacementModifierType::codec)`)
//! and the abstract `getPositions`/`type` pair. The Rust port splits identity
//! from behavior like `Feature`: `PlacementModifier` is the generic behavior
//! contract (its `get_positions` is generic over the random source, so it is
//! *not* object-safe), `ErasedPlacementModifier` is the object-safe carrier
//! `PlacedFeature` stores its heterogeneous list as, and `placement_get_positions`
//! is the `#181` codegen dispatch that downcasts the erased carrier and calls
//! the concrete modifier's `get_positions`. The dispatch codec
//! (`PlacementModifier.CODEC`) is deferred with the by-name codec surface
//! (`#126`) and the `#181` registration table.
//!
//! Java's `getPositions` returns a lazy `Stream<BlockPos>`, but every concrete
//! modifier draws eagerly *inside* `getPositions` and returns a pure stream
//! (`RepeatingPlacement`'s `IntStream.range(0, count(...))`, `InSquarePlacement`'s
//! two `random.nextInt(16)`, `HeightRangePlacement`'s `height.sample(...)`, …).
//! The port's `get_positions` mirrors that: it draws eagerly from `random` and
//! returns an eager `Vec<BlockPos>`. The laziness that matters for parity is
//! *when* `get_positions` runs — Java's `flatMap` invokes it per upstream
//! position, interleaved with placements — and that is preserved by
//! `PlacedFeature::place_with_context`'s depth-first walk (see that method). A
//! truly lazy pull would need to hold `R`-specific state in a type-erased list,
//! which `RandomSource` (`Sized`, not object-safe) forbids.

use crate::levelgen::placement::PlacementContext;
use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use std::fmt::Debug;

/// `net.minecraft.world.level.levelgen.placement.PlacementModifier` — the
/// abstract behavior contract of every placement modifier.
///
/// Implemented by the concrete modifier structs (owned by their own manifest
/// units). `get_positions` is generic over the random source (`RandomSource`
/// is `Sized`), so concrete modifiers are dispatched monomorphically by the
/// `#181` generated match, not through a `dyn`.
pub trait PlacementModifier: Debug + Send + Sync + 'static {
    /// `getPositions(PlacementContext, RandomSource, BlockPos)` — the
    /// modifier's per-position stream, flattened by `PlacedFeature.place`.
    ///
    /// Returns a lazy `Box<dyn Iterator<Item = BlockPos> + 'a>` because Java's
    /// `getPositions` returns a lazy `Stream<BlockPos>` (`IntStream.range(...)`
    /// / `Stream.of(...)`); the per-position interleaving Java's lazy
    /// `flatMap` provides is reproduced by `PlacedFeature::place_with_context`'s
    /// depth-first walk (see module doc). Every Java modifier draws eagerly
    /// *inside* `getPositions` (before returning the stream), so the port draws
    /// eagerly too — the RNG draw order and count are unchanged from an eager
    /// port; only the *materialization* is deferred, exactly as in Java. This
    /// is what keeps `RepeatingPlacement`'s unbounded `count` from allocating a
    /// `count`-length `Vec` (Java degrades to a slow lazy pull instead of OOM).
    ///
    /// The iterator's lifetime is tied to `&'a self` alone (NOT to
    /// `context`/`random`/`origin`): `PlacedFeature::place_walk` reconstructs
    /// the `PlacementContext` per expansion (the context mutably borrows the
    /// level, and the walk must hand the level back to the recursive placement),
    /// so the returned iterator must outlive the temporary context. Every
    /// concrete modifier computes its positions eagerly from the inputs and
    /// returns an owning iterator, so this is always satisfiable.
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a>;

    /// `type()` — the registry-held `PlacementModifierType<?>` identity this
    /// modifier dispatches on (the key `PlacementModifier.CODEC` uses).
    fn type_id(&self) -> PlacementModifierTypeId;
}

/// The object-safe carrier `PlacedFeature.placement` stores each modifier as —
/// the dispatch identity plus the `dyn`-compatible surface the `#181` match
/// downcasts. Every `PlacementModifier` implements it via the blanket impl, so
/// the concrete modifier units only implement `PlacementModifier`.
pub trait ErasedPlacementModifier: Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> PlacementModifierTypeId;
}

impl<M: PlacementModifier + ?Sized> ErasedPlacementModifier for M {
    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifier::type_id(self)
    }
}

/// The `#181` hub — dispatch an erased placement modifier to its positions.
///
/// The `PlacementModifierType.register` table (x 15) is generated content —
/// emitted by `rivet-codegen` per the `#181` manifest note. The generated
/// dispatch is a monomorphized `match` over `type_id` that downcasts the
/// erased carrier to the concrete modifier and calls its `get_positions`.
/// Until the table is wired this stub panics unconditionally — it is the
/// pre-wire stand-in for the generated dispatch, whose unknown-id path will
/// throw `IllegalStateException` like Java's `Registry.getValueOrThrow` (Java
/// throws only when the key is genuinely missing).
pub fn placement_get_positions<'a, R: RandomSource>(
    modifier: &'a dyn ErasedPlacementModifier,
    _context: &PlacementContext,
    _random: &mut R,
    _origin: &BlockPos,
) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
    // STUB(mc.world.level.levelgen.placement.core) — the generated
    // `BuiltInRegistries.PLACEMENT_MODIFIER_TYPE` dispatch table (`modifier` is
    // downcast to the concrete modifier by the generated match; kept in the
    // signature so the stub shape matches).
    panic!(
        "Trying to apply placement modifier type '{}' with no registered behavior (#181 codegen)",
        modifier.type_id().location
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_registry::core::BlockPos;

    /// A concrete modifier whose only identity is the type id — the erased
    /// carrier must carry it through the blanket `ErasedPlacementModifier` impl.
    #[derive(Debug)]
    struct IdentityModifier(PlacementModifierTypeId);

    impl PlacementModifier for IdentityModifier {
        fn get_positions<'a, R: RandomSource>(
            &'a self,
            _context: &PlacementContext,
            _random: &mut R,
            _origin: &BlockPos,
        ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
            Box::new(std::iter::empty())
        }

        fn type_id(&self) -> PlacementModifierTypeId {
            self.0.clone()
        }
    }

    #[test]
    fn erased_carrier_forwards_the_type_identity() {
        // `PlacementModifierType.COUNT` is insertion index 5 in
        // `PlacementModifierType.java`'s registration order.
        let modifier = IdentityModifier(PlacementModifierTypeId::new(5, "minecraft:count"));
        let erased: &dyn ErasedPlacementModifier = &modifier;
        assert_eq!(
            erased.type_id(),
            PlacementModifierTypeId::new(5, "minecraft:count")
        );
    }

    #[test]
    #[should_panic(expected = "Trying to apply placement modifier type 'minecraft:count'")]
    fn dispatch_stub_panics_on_unknown_behavior() {
        // The `#181` dispatch table is not wired yet; dispatching panics
        // unconditionally (the pre-wire stand-in for the generated dispatch,
        // whose unknown-id path throws like Java's `Registry.getValueOrThrow`).
        let modifier = IdentityModifier(PlacementModifierTypeId::new(5, "minecraft:count"));
        let erased: &dyn ErasedPlacementModifier = &modifier;
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let context = PlacementContext::new(&mut level, &generator, None);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let _ = placement_get_positions(erased, &context, &mut random, &origin);
    }

    struct TestLevel;

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
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
}

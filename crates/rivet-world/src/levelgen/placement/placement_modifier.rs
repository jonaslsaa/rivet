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
//! returns a lazy `Box<dyn Iterator<Item = BlockPos> + 'a>` tied to `&'a self`
//! (so the iterator outlives the per-expansion `PlacementContext`). The laziness
//! that matters for parity is *when* `get_positions` runs — Java's `flatMap`
//! invokes it per upstream position, interleaved with placements — and that is
//! preserved by `PlacedFeature::place_with_context`'s depth-first walk (see that
//! method). The iterator form additionally keeps `RepeatingPlacement`'s unbounded
//! `count` from materializing a `count`-length `Vec` (Java degrades to a slow
//! lazy pull instead of OOM).

use crate::levelgen::placement::placement_modifier_type::PlacementModifierTypeId;
use crate::levelgen::placement::{
    BiomeFilter, BlockPredicateFilter, CountOnEveryLayerPlacement, CountPlacement,
    EnvironmentScanPlacement, FixedPlacement, HeightRangePlacement, HeightmapPlacement,
    InSquarePlacement, NoiseBasedCountPlacement, NoiseThresholdCountPlacement, PlacementContext,
    RandomOffsetPlacement, RarityFilter, SurfaceRelativeThresholdFilter, SurfaceWaterDepthFilter,
};
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use std::any::Any;
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
        context: &mut PlacementContext,
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

    /// The `Any` upcast the `#181` dispatch downcasts: the erased carrier is
    /// upcast to `&dyn Any` (every concrete modifier is `'static`) and
    /// downcast to the concrete modifier struct before its `get_positions`
    /// runs — the same erased-cast seam `FeatureConfiguration` and
    /// `BlockStateProvider` expose.
    fn as_any(&self) -> &dyn Any;
}

impl<M: PlacementModifier> ErasedPlacementModifier for M {
    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifier::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `#181` hub — dispatch an erased placement modifier to its positions.
///
/// This is the hand-ported stand-in for the generated
/// `PlacementModifierType.register` dispatch table: a monomorphized `match`
/// over the modifier's `PlacementModifierTypeId` (the registry element id
/// equals the insertion index in `PlacementModifierType.java`'s registration
/// order, so id dispatch is exactly Java's per-type `getPositions` call) that
/// downcasts the erased carrier to the concrete modifier struct via the `Any`
/// blanket (every concrete struct is `'static`) and calls its real
/// `get_positions`/`should_place`. `PlacementFilter` concrete structs reach
/// their `should_place` through the blanket `PlacementModifier` impl — Java's
/// `final getPositions` shell — so the filter arms call the same inherited
/// method as the leaf arms, preserving draw order and filtering semantics.
///
/// All fifteen registered modifier type ids map to exactly one of the ported
/// concrete leaves (filter/simple/repeating), so there is no silent fallback:
/// any id that does not match is genuinely unregistered in this port, and it
/// fails loudly, naming the type — mirroring `feature_place`'s unknown-id arm
/// and Java's `Registry.getValueOrThrow` (which throws only when the key is
/// genuinely missing). `minecraft:cave_surface` is NOT a modifier type — it is
/// a `CaveSurface` enum (the co-file of `CountOnEveryLayerPlacement`, whose
/// id 8 leaf is `minecraft:count_on_every_layer`) — so it has no dispatch arm.
pub fn placement_get_positions<'a, R: RandomSource>(
    modifier: &'a dyn ErasedPlacementModifier,
    context: &mut PlacementContext,
    random: &mut R,
    origin: &BlockPos,
) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
    // The erased carrier is downcast to the concrete modifier by the same
    // `Any` upcast the `FeatureConfiguration` dispatch uses: `ErasedPlacementModifier`
    // is implemented by a blanket impl over every `PlacementModifier` (all
    // concrete structs, `'static`), so `as_any()` reaches the concrete type.
    let this = modifier.as_any();
    match modifier.type_id().id {
        // `PlacementModifierType.BLOCK_PREDICATE_FILTER` — id 0.
        0 => PlacementModifier::get_positions(
            this.downcast_ref::<BlockPredicateFilter>()
                .expect("block_predicate_filter modifier must carry a BlockPredicateFilter"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.RARITY_FILTER` — id 1.
        1 => PlacementModifier::get_positions(
            this.downcast_ref::<RarityFilter>()
                .expect("rarity_filter modifier must carry a RarityFilter"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.SURFACE_RELATIVE_THRESHOLD_FILTER` — id 2.
        2 => PlacementModifier::get_positions(
            this.downcast_ref::<SurfaceRelativeThresholdFilter>().expect(
                "surface_relative_threshold_filter modifier must carry a SurfaceRelativeThresholdFilter",
            ),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.SURFACE_WATER_DEPTH_FILTER` — id 3.
        3 => PlacementModifier::get_positions(
            this.downcast_ref::<SurfaceWaterDepthFilter>()
                .expect("surface_water_depth_filter modifier must carry a SurfaceWaterDepthFilter"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.BIOME_FILTER` — id 4.
        4 => PlacementModifier::get_positions(
            this.downcast_ref::<BiomeFilter>()
                .expect("biome filter modifier must carry a BiomeFilter"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.COUNT` — id 5.
        5 => PlacementModifier::get_positions(
            this.downcast_ref::<CountPlacement>()
                .expect("count modifier must carry a CountPlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.NOISE_BASED_COUNT` — id 6.
        6 => PlacementModifier::get_positions(
            this.downcast_ref::<NoiseBasedCountPlacement>()
                .expect("noise_based_count modifier must carry a NoiseBasedCountPlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.NOISE_THRESHOLD_COUNT` — id 7.
        7 => PlacementModifier::get_positions(
            this.downcast_ref::<NoiseThresholdCountPlacement>().expect(
                "noise_threshold_count modifier must carry a NoiseThresholdCountPlacement",
            ),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.COUNT_ON_EVERY_LAYER` — id 8.
        8 => PlacementModifier::get_positions(
            this.downcast_ref::<CountOnEveryLayerPlacement>().expect(
                "count_on_every_layer modifier must carry a CountOnEveryLayerPlacement",
            ),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.ENVIRONMENT_SCAN` — id 9.
        9 => PlacementModifier::get_positions(
            this.downcast_ref::<EnvironmentScanPlacement>()
                .expect("environment_scan modifier must carry an EnvironmentScanPlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.HEIGHTMAP` — id 10.
        10 => PlacementModifier::get_positions(
            this.downcast_ref::<HeightmapPlacement>()
                .expect("heightmap modifier must carry a HeightmapPlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.HEIGHT_RANGE` — id 11.
        11 => PlacementModifier::get_positions(
            this.downcast_ref::<HeightRangePlacement>()
                .expect("height_range modifier must carry a HeightRangePlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.IN_SQUARE` — id 12.
        12 => PlacementModifier::get_positions(
            this.downcast_ref::<InSquarePlacement>()
                .expect("in_square modifier must carry an InSquarePlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.RANDOM_OFFSET` — id 13.
        13 => PlacementModifier::get_positions(
            this.downcast_ref::<RandomOffsetPlacement>()
                .expect("random_offset modifier must carry a RandomOffsetPlacement"),
            context,
            random,
            origin,
        ),
        // `PlacementModifierType.FIXED_PLACEMENT` — id 14.
        14 => PlacementModifier::get_positions(
            this.downcast_ref::<FixedPlacement>()
                .expect("fixed_placement modifier must carry a FixedPlacement"),
            context,
            random,
            origin,
        ),
        // Every registered id above has exactly one ported leaf, so this arm is
        // reached only for an id that is not a registered placement modifier
        // type at all. Failing loudly (rather than returning a fabricated
        // position stream) is the honest representation the `#181` dispatch
        // contract requires — never a silent partial fallback.
        other => panic!(
            "Trying to apply placement modifier type id '{}' with no registered behavior (#181 codegen)",
            other
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypes;
    use crate::levelgen::blockpredicates::{BlockPredicate, BlockPredicateTypeId, always_true};
    use crate::levelgen::heightmap::Types;
    use crate::levelgen::vertical_anchor::VerticalAnchor;
    use rivet_registry::core::Direction;
    use rivet_util::random::{LegacyPositionalRandomFactory, LegacyRandomSource};
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::int_provider::IntProvider;
    use std::sync::Arc;

    /// A concrete modifier whose only identity is the type id — the erased
    /// carrier must carry it through the blanket `ErasedPlacementModifier`
    /// impl. Used for the unknown-id and identity-mismatch hostile tests (its
    /// `get_positions` is never reached in those).
    #[derive(Debug)]
    struct IdentityModifier(PlacementModifierTypeId);

    impl PlacementModifier for IdentityModifier {
        fn get_positions<'a, R: RandomSource>(
            &'a self,
            _context: &mut PlacementContext,
            _random: &mut R,
            _origin: &BlockPos,
        ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
            Box::new(std::iter::empty())
        }

        fn type_id(&self) -> PlacementModifierTypeId {
            self.0.clone()
        }
    }

    /// The dispatch under test: erase a concrete modifier to its carrier and
    /// run `placement_get_positions` on it, collecting the resulting stream.
    ///
    /// The returned iterator is tied to `&modifier` alone (not to the
    /// per-call `PlacementContext`), so the context is a local.
    fn dispatch_positions<M: PlacementModifier, R: RandomSource>(
        modifier: &M,
        random: &mut R,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let erased: &dyn ErasedPlacementModifier = modifier;
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        placement_get_positions(erased, &mut context, random, origin).collect()
    }

    /// The same leaf called directly (not through the dispatch) — the parity
    /// reference a dispatched call must match draw-for-draw.
    fn direct_positions<M: PlacementModifier, R: RandomSource>(
        modifier: &M,
        random: &mut R,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        PlacementModifier::get_positions(modifier, &mut context, random, origin).collect()
    }

    /// The RNG draws the tests assert on (a subset of the `feature.selector`
    /// test-support recording, local to this module).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Call {
        IntBound(i32),
        Float,
    }

    /// A `RandomSource` wrapper that records the draws — pins the exact draw
    /// count/arguments a dispatched leaf performs, so a changed draw order
    /// fails loudly.
    struct RecordingRandom {
        inner: LegacyRandomSource,
        calls: Vec<Call>,
    }

    impl RecordingRandom {
        fn new(seed: i64) -> RecordingRandom {
            RecordingRandom {
                inner: LegacyRandomSource::new(seed),
                calls: Vec::new(),
            }
        }
    }

    impl RandomSource for RecordingRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            RecordingRandom {
                inner: self.inner.fork(),
                calls: self.calls.clone(),
            }
        }

        fn fork_positional(&mut self) -> Self::Positional {
            self.inner.fork_positional()
        }

        fn set_seed(&mut self, seed: i64) {
            self.inner.set_seed(seed);
        }

        fn next_int(&mut self) -> i32 {
            self.inner.next_int()
        }

        fn next_int_bound(&mut self, bound: i32) -> i32 {
            self.calls.push(Call::IntBound(bound));
            self.inner.next_int_bound(bound)
        }

        fn next_long(&mut self) -> i64 {
            self.inner.next_long()
        }

        fn next_boolean(&mut self) -> bool {
            self.inner.next_boolean()
        }

        fn next_float(&mut self) -> f32 {
            self.calls.push(Call::Float);
            self.inner.next_float()
        }

        fn next_double(&mut self) -> f64 {
            self.inner.next_double()
        }

        fn next_gaussian(&mut self) -> f64 {
            self.inner.next_gaussian()
        }
    }

    /// A pure (world-access-free) block predicate matching a specific absolute
    /// Y — the `EnvironmentScanPlacement` target, so the scan runs without
    /// touching the `#399`/`#232` seams.
    #[derive(Debug)]
    struct AtY(i32);

    impl BlockPredicate for AtY {
        fn test(&self, _level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
            origin.get_y() == self.0
        }

        fn type_id(&self) -> BlockPredicateTypeId {
            BlockPredicateTypes::TRUE
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// The panic payload as a `String`, for `catch_unwind` assertion. A
    /// `panic!` with a bare literal yields a `&'static str` payload; one with
    /// format arguments yields a `String` — both are recovered.
    fn panic_message<T>(result: std::thread::Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&'static str>()
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| format!("{:?}", payload)),
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
    fn dispatch_rejects_an_unregistered_type_id() {
        // id 99 is not a registered placement modifier type — the unknown-id
        // arm fails loudly and typed (naming the id), mirroring
        // `feature_place`'s unknown-id panic and Java's
        // `Registry.getValueOrThrow` (which throws only for a genuinely missing
        // key). No silent partial fallback.
        let modifier =
            IdentityModifier(PlacementModifierTypeId::new(99, "minecraft:not_a_modifier"));
        let erased: &dyn ErasedPlacementModifier = &modifier;
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = placement_get_positions(erased, &mut context, &mut random, &origin);
        }));
        assert_eq!(
            panic_message(result),
            "Trying to apply placement modifier type id '99' with no registered behavior (#181 codegen)"
        );
    }

    #[test]
    fn dispatch_rejects_a_carrier_mismatching_its_concrete_type() {
        // An `IdentityModifier` claiming COUNT (id 5) but carrying no
        // `CountPlacement`: the id-5 arm's typed downcast fails with the exact
        // per-arm message — a type-id/type mismatch is a hard typed error,
        // never a silent wrong dispatch.
        let modifier = IdentityModifier(PlacementModifierTypeId::new(5, "minecraft:count"));
        let erased: &dyn ErasedPlacementModifier = &modifier;
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = placement_get_positions(erased, &mut context, &mut random, &origin);
        }));
        assert_eq!(
            panic_message(result),
            "count modifier must carry a CountPlacement"
        );
    }

    #[test]
    fn id_0_block_predicate_filter_dispatches_the_shell() {
        // `PlacementModifierType.BLOCK_PREDICATE_FILTER` (id 0) reaches
        // `BlockPredicateFilter`'s `PlacementFilter` shell; `alwaysTrue()`
        // keeps exactly the origin (no draws).
        let modifier = BlockPredicateFilter::for_predicate(always_true());
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        assert_eq!(
            dispatch_positions(&modifier, &mut random, &origin),
            vec![origin]
        );
    }

    #[test]
    fn id_1_rarity_filter_dispatches_and_draws_once() {
        // `RarityFilter.shouldPlace` draws exactly one `nextFloat` and keeps
        // the origin on `nextFloat < 1/chance` — via the `PlacementFilter`
        // blanket shell (Java's `final getPositions`). Dispatch must forward
        // the single draw and its keep/drop verdict; a fresh equal-seed RNG
        // on the direct call yields the same verdict, pinning identical RNG
        // consumption.
        let modifier = RarityFilter::on_average_once_every(2);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = RecordingRandom::new(0);
        let positions = dispatch_positions(&modifier, &mut random, &origin);
        assert_eq!(random.calls, vec![Call::Float], "one nextFloat draw");
        let mut direct_random = LegacyRandomSource::new(0);
        let direct = direct_positions(&modifier, &mut direct_random, &origin);
        assert_eq!(positions, direct);
        assert!(positions.len() <= 1);
    }

    #[test]
    fn id_2_surface_relative_threshold_fails_at_the_height_seam() {
        // The filter's `shouldPlace` reads the heightmap through
        // `PlacementContext.getHeight` — the `#232` world-access seam — so
        // dispatch reaches the real leaf and the leaf fails explicitly rather
        // than fabricating a surface.
        let modifier = SurfaceRelativeThresholdFilter::of(Types::MotionBlocking, 0, 0);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_positions(&modifier, &mut random, &origin)
        }));
        let msg = panic_message(result);
        assert!(msg.contains("RivetTodo #232"), "got: {msg}");
    }

    #[test]
    fn id_3_surface_water_depth_fails_at_the_height_seam() {
        // Same `#232` heightmap-read seam as the sibling filter.
        let modifier = SurfaceWaterDepthFilter::for_max_depth(3);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_positions(&modifier, &mut random, &origin)
        }));
        let msg = panic_message(result);
        assert!(msg.contains("RivetTodo #232"), "got: {msg}");
    }

    #[test]
    fn id_4_biome_filter_guards_without_a_top_feature() {
        // `BiomeFilter.shouldPlace` fires Java's exact
        // `topFeature().orElseThrow(...)` guard first, before any world
        // access — so dispatch reaches the biome leaf and the guard panics
        // with the exact message (the biome-membership pass/fail is covered by
        // the leaf's own tests with an answering generator).
        let modifier = BiomeFilter::biome();
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_positions(&modifier, &mut random, &origin)
        }));
        let msg = panic_message(result);
        assert!(
            msg.contains("Tried to biome check an unregistered feature, or a feature that should not restrict the biome"),
            "got: {msg}"
        );
    }

    #[test]
    fn id_5_count_dispatches_the_repeating_shell() {
        // `PlacementModifierType.COUNT` (id 5) reaches `CountPlacement`'s
        // inherited `RepeatingPlacement` shell: `count` positions, every one
        // the origin. A constant count draws nothing.
        let modifier = CountPlacement::of_value(3);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = RecordingRandom::new(0);
        let positions = dispatch_positions(&modifier, &mut random, &origin);
        assert_eq!(positions.len(), 3);
        for pos in &positions {
            assert_eq!(*pos, origin);
        }
        assert!(
            random.calls.is_empty(),
            "constant count draws nothing, got {:?}",
            random.calls
        );
    }

    #[test]
    fn id_6_noise_based_count_matches_a_direct_call() {
        // The `BIOME_INFO_NOISE`-sampled count is deterministic per origin;
        // dispatch must forward the same count (same positions) as a direct
        // call on an equal-seed RNG.
        let modifier = NoiseBasedCountPlacement::of(10, 0.05, 0.0);
        let origin = BlockPos::new(8, 0, 16);
        let mut random = LegacyRandomSource::new(0);
        let via_dispatch = dispatch_positions(&modifier, &mut random, &origin);
        let mut direct_random = LegacyRandomSource::new(0);
        let direct = direct_positions(&modifier, &mut direct_random, &origin);
        assert_eq!(via_dispatch, direct);
    }

    #[test]
    fn id_7_noise_threshold_count_matches_a_direct_call() {
        let modifier = NoiseThresholdCountPlacement::of(0.0, 1, 5);
        let origin = BlockPos::new(8, 0, 16);
        let mut random = LegacyRandomSource::new(0);
        let via_dispatch = dispatch_positions(&modifier, &mut random, &origin);
        let mut direct_random = LegacyRandomSource::new(0);
        let direct = direct_positions(&modifier, &mut direct_random, &origin);
        assert_eq!(via_dispatch, direct);
    }

    #[test]
    fn id_8_count_on_every_layer_fails_at_the_height_seam() {
        // `getPositions` reads the MOTION_BLOCKING heightmap through the
        // `#232` seam on its first sample.
        let modifier = CountOnEveryLayerPlacement::of_int(1);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_positions(&modifier, &mut random, &origin)
        }));
        let msg = panic_message(result);
        assert!(msg.contains("RivetTodo #232"), "got: {msg}");
    }

    #[test]
    fn id_9_environment_scan_dispatches_a_pure_scan() {
        // Scanning DOWN from y=10 with the pure `AtY(9)` target: the first
        // downward step matches, yielding (x, 9, z). Both predicates are
        // world-access-free, so the scan completes without touching a seam —
        // dispatch reaches `EnvironmentScanPlacement`'s real scan.
        let modifier =
            EnvironmentScanPlacement::scanning_for_default(Direction::Down, Arc::new(AtY(9)), 5);
        let origin = BlockPos::new(1, 10, 2);
        let mut random = LegacyRandomSource::new(0);
        assert_eq!(
            dispatch_positions(&modifier, &mut random, &origin),
            vec![BlockPos::new(1, 9, 2)]
        );
    }

    #[test]
    fn id_10_heightmap_fails_at_the_height_seam() {
        let modifier = HeightmapPlacement::on_heightmap(Types::MotionBlocking);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_positions(&modifier, &mut random, &origin)
        }));
        let msg = panic_message(result);
        assert!(msg.contains("RivetTodo #232"), "got: {msg}");
    }

    #[test]
    fn id_11_height_range_dispatches_a_uniform_y() {
        // `origin.atY(height.sample(...))` — an equal-anchor uniform yields
        // the absolute Y without drawing RNG.
        let modifier = HeightRangePlacement::uniform(
            VerticalAnchor::absolute(20),
            VerticalAnchor::absolute(20),
        );
        let origin = BlockPos::new(1, 2, 3);
        let mut random = LegacyRandomSource::new(0);
        assert_eq!(
            dispatch_positions(&modifier, &mut random, &origin),
            vec![BlockPos::new(1, 20, 3)]
        );
    }

    #[test]
    fn id_12_in_square_dispatches_and_draws_two_bounds() {
        // `random.nextInt(16) + origin.getX()/getZ()` — exactly two
        // `nextInt(16)` draws, wrapped, one position; dispatch must match a
        // direct call on an equal-seed RNG.
        let modifier = InSquarePlacement::spread();
        let origin = BlockPos::new(1, 2, 3);
        let mut random = RecordingRandom::new(0);
        let positions = dispatch_positions(&modifier, &mut random, &origin);
        assert_eq!(random.calls, vec![Call::IntBound(16), Call::IntBound(16)]);
        let mut direct_random = LegacyRandomSource::new(0);
        let direct = direct_positions(&modifier, &mut direct_random, &origin);
        assert_eq!(positions, direct);
        assert_eq!(positions.len(), 1);
    }

    #[test]
    fn id_13_random_offset_dispatches_constant_spreads() {
        // Constant xz=+2, y=-1 -> (x+2, y-1, z+2); a constant spread draws
        // nothing.
        let modifier = RandomOffsetPlacement::of(
            IntProvider::Constant(ConstantInt::of(2)),
            IntProvider::Constant(ConstantInt::of(-1)),
        );
        let origin = BlockPos::new(10, 20, 30);
        let mut random = LegacyRandomSource::new(0);
        assert_eq!(
            dispatch_positions(&modifier, &mut random, &origin),
            vec![BlockPos::new(12, 19, 32)]
        );
    }

    #[test]
    fn id_14_fixed_placement_filters_to_the_origin_chunk() {
        // `FixedPlacement.getPositions` keeps only the fixed positions in the
        // origin's chunk (`SectionPos.blockToSectionCoord`); (1,0,1) is in
        // chunk (0,0) with the origin, (100,0,100) is chunk (6,6) and is
        // dropped.
        let modifier = FixedPlacement::of(&[BlockPos::new(1, 0, 1), BlockPos::new(100, 0, 100)]);
        let origin = BlockPos::new(0, 0, 0);
        let mut random = LegacyRandomSource::new(0);
        assert_eq!(
            dispatch_positions(&modifier, &mut random, &origin),
            vec![BlockPos::new(1, 0, 1)]
        );
    }

    #[test]
    fn count_placement_does_not_materialize_huge_counts() {
        // `RepeatingPlacement`'s shell is lazy: an unbounded count (`i32::MAX`,
        // the `Codec.INT` saturation Java degrades to a slow lazy pull) yields
        // a `repeat_n` iterator, not a `count`-length allocation. Pulling three
        // elements must not allocate the full range — the no-eager-collection
        // drift invariant at the leaf.
        let modifier = CountPlacement::of_value(i32::MAX);
        let origin = BlockPos::new(1, 2, 3);
        let mut level = TestLevel;
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let mut random = LegacyRandomSource::new(0);
        let mut it = placement_get_positions(&modifier, &mut context, &mut random, &origin);
        for _ in 0..3 {
            assert_eq!(it.next(), Some(origin));
        }
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

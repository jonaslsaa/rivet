//! Port of `net.minecraft.world.level.levelgen.blockpredicates.StateTestingPredicate`
//! (abstract class, 26.2).
//!
//! Java is the abstract base of the block-state-testing predicates: it holds
//! the `protected final Vec3i offset`, exposes the shared
//! `stateTestingCodec(Instance)` field (`Vec3i.offsetCodec(16)` with
//! `"offset"` optional defaulting to `Vec3i.ZERO`), and its `test` is `final`:
//! `test(level.getBlockState(origin.offset(this.offset)))`. The Rust port keeps
//! that shape: a standalone trait with an `offset()` accessor, the shared
//! ops-generic offset-field codec, and a single (non-overridable) `test` shell
//! that resolves the offset state and delegates to the abstract `test_state`.
//!
//! ## The capability-unavailable boundary
//!
//! Java's `test` reads `level.getBlockState(...)`. The real world-access
//! implementation is not ported (RivetTodo #399), so the seam must never
//! fabricate a state: `test` calls the [`WorldGenLevel::get_block_state`] seam
//! and — because no production world provides it yet — every concrete
//! state-testing predicate surfaces the unavailable capability by panicking
//! there. The `test_state` behavior itself (the pure per-state predicate) is
//! fully ported and tested; only the world-access step is deferred.
//!
//! The `SolidPredicate`/`MatchingBlocksPredicate`/`MatchingBlockTagPredicate`/
//! `MatchingFluidsPredicate`/`ReplaceablePredicate`/`WouldSurvivePredicate`/
//! `HasSturdyFacePredicate` state-testing subclasses (the `.states` unit) are
//! out of this slice's scope; the base and its offset codec are the
//! dependency-clean prerequisite.

use rivet_registry::block_state::BlockState;
use rivet_registry::core::Vec3i;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.StateTestingPredicate` —
/// the base of predicates that test a single block state at an offset.
///
/// Concrete predicates implement this and the abstract `test_state`; the
/// `test` shell (Java's `final test`) resolves the offset state and delegates.
pub trait StateTestingPredicate: Debug + Send + Sync + 'static {
    /// `this.offset` — the offset applied to the tested position.
    fn offset(&self) -> &Vec3i;

    /// `test(BlockState)` — the abstract per-state predicate.
    fn test_state(&self, state: &BlockState) -> bool;
}

/// `StateTestingPredicate.test(WorldGenLevel, BlockPos)` — the Java `final`
/// test: `this.test(level.getBlockState(origin.offset(this.offset)))`.
///
/// The offset arithmetic (`BlockPos.offset(Vec3i)`, wrapping) and the delegate
/// to `test_state` are ported exactly; the `getBlockState` call is the
/// capability-unavailable seam (RivetTodo #399) — the concrete world-access
/// implementation lands with the world unit, and until then calling through
/// it panics rather than fabricating a state.
pub fn state_testing_test<C: StateTestingPredicate>(
    predicate: &C,
    level: &dyn crate::level::WorldGenLevel,
    origin: &rivet_registry::core::BlockPos,
) -> bool {
    let pos = origin.offset_vec(predicate.offset());
    let state = level.get_block_state(&pos);
    predicate.test_state(&state)
}

/// `Vec3i.offsetCodec(int maxOffsetPerAxis)` — `Vec3i.CODEC.validate(...)` with
/// `Math.abs(x) < maxOffsetPerAxis && Math.abs(y) < ... && Math.abs(z) < ...`,
/// erroring `"Position out of range, expected at most {max}: {value}"`.
///
/// The `Vec3i.CODEC` is `Codec.INT_STREAM.comapFlatMap(Util.fixedSize(input, 3)
/// -> Vec3i, pos -> IntStream)`, so the offset codec is the ops-generic
/// `vec3i_offset_codec::<Ops>(max)` factory.
pub fn vec3i_offset_codec<Ops: DynamicOps + 'static>(
    max_offset_per_axis: i32,
) -> Arc<dyn Codec<Vec3i, Ops>> {
    let base = codec::comap_flat_map::<Vec<i32>, Vec3i, Ops>(
        codec::int_stream_codec::<Ops>(),
        Arc::new(|input: &Vec<i32>| {
            rivet_util::fixed_size_i32(input, 3).map(|ints| Vec3i::new(ints[0], ints[1], ints[2]))
        }),
        Arc::new(|v: &Vec3i| vec![v.get_x(), v.get_y(), v.get_z()]),
    );
    codec::validate(
        base,
        Arc::new(move |value: &Vec3i| {
            if value.get_x().abs() < max_offset_per_axis
                && value.get_y().abs() < max_offset_per_axis
                && value.get_z().abs() < max_offset_per_axis
            {
                rivet_serialization::DataResult::success(*value)
            } else {
                rivet_serialization::DataResult::error(format!(
                    "Position out of range, expected at most {max_offset_per_axis}: {}",
                    value
                ))
            }
        }),
    )
}

/// `Vec3i.offsetCodec(16).optionalFieldOf("offset", Vec3i.ZERO)` — the
/// `"offset"` optional field used by every offset-bearing predicate codec
/// (Java `StateTestingPredicate.stateTestingCodec`, and `InsideWorldBoundsPredicate`
/// directly with `BlockPos.ZERO`), as the ops-generic `offset_field_codec::<Ops>()`
/// factory.
///
/// `optionalFieldOf(name, defaultValue)` is the xmap: absent decodes to ZERO,
/// and the default ZERO is omitted on encode.
pub fn offset_field_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<Vec3i, Ops>> {
    let optional: Arc<dyn MapCodec<Option<Vec3i>, Ops>> =
        codec::optional_field("offset".to_string(), vec3i_offset_codec::<Ops>(16), false);
    map_codec::xmap(
        optional,
        Arc::new(|o: &Option<Vec3i>| o.unwrap_or(Vec3i::ZERO)),
        Arc::new(|v: &Vec3i| if *v == Vec3i::ZERO { None } else { Some(*v) }),
    )
}

/// Wrap the offset field as a record-builder component for a predicate —
/// `RecordCodecBuilder.of(getter, offsetFieldCodec)` (Java's
/// `...forGetter(c -> c.offset)`).
pub fn offset_field<P, Ops>(
    getter: Arc<dyn Fn(&P) -> Vec3i + Send + Sync>,
) -> RecordCodecBuilder<P, Ops, Vec3i>
where
    P: 'static,
    Ops: DynamicOps + 'static,
{
    RecordCodecBuilder::of(getter, offset_field_codec::<Ops>())
}

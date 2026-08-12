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
//! The `.states`-unit subclasses (`SolidPredicate`, `MatchingBlocksPredicate`,
//! `MatchingBlockTagPredicate`, `MatchingFluidsPredicate`,
//! `ReplaceablePredicate`, plus the `WouldSurvivePredicate`/
//! `HasSturdyFacePredicate` leaves that share the offset field) are ported
//! alongside this base; each implements `test_state` and the shared offset
//! codec, and only the world-access `test` shell resolves through the `#399`
//! seam.

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

/// `Vec3i.CODEC` — `Codec.INT_STREAM.comapFlatMap(Util.fixedSize(input, 3) ->
/// Vec3i, pos -> IntStream)`, as the ops-generic `vec3i_codec::<Ops>()` factory.
///
/// Unlike `BlockPos.CODEC` (which is `.stable()`), `Vec3i.CODEC` is NOT stable
/// — `UnobstructedPredicate.CODEC`'s `"offset"` field uses it directly.
pub fn vec3i_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Vec3i, Ops>> {
    codec::comap_flat_map::<Vec<i32>, Vec3i, Ops>(
        codec::int_stream_codec::<Ops>(),
        Arc::new(|input: &Vec<i32>| {
            rivet_util::fixed_size_i32(input, 3).map(|ints| Vec3i::new(ints[0], ints[1], ints[2]))
        }),
        Arc::new(|v: &Vec3i| vec![v.get_x(), v.get_y(), v.get_z()]),
    )
}

/// `Vec3i.CODEC.optionalFieldOf("offset", Vec3i.ZERO)` — the non-lenient
/// optional offset field (`UnobstructedPredicate.CODEC` uses this form). DFU's
/// `optionalFieldOf(name, default)` is built on the non-lenient
/// `optionalField(name, codec, false)`, so a present-but-malformed field
/// propagates its decode error (unlike `lenientOptionalFieldOf`, which falls
/// back to the default). Absent decodes to ZERO, and the default ZERO is
/// omitted on encode.
pub fn vec3i_optional_field_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<Vec3i, Ops>> {
    let optional: Arc<dyn MapCodec<Option<Vec3i>, Ops>> =
        codec::optional_field("offset".to_string(), vec3i_codec::<Ops>(), false);
    map_codec::xmap(
        optional,
        Arc::new(|o: &Option<Vec3i>| o.unwrap_or(Vec3i::ZERO)),
        Arc::new(|v: &Vec3i| if *v == Vec3i::ZERO { None } else { Some(*v) }),
    )
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
            // `wrapping_abs` mirrors Java `Math.abs`: `Math.abs(Integer.MIN_VALUE)`
            // wraps to `Integer.MIN_VALUE` (no exception), which is `< max`, so
            // the offset is accepted. Rust `.abs()` would panic on `i32::MIN` in
            // debug builds instead of reproducing that.
            if value.get_x().wrapping_abs() < max_offset_per_axis
                && value.get_y().wrapping_abs() < max_offset_per_axis
                && value.get_z().wrapping_abs() < max_offset_per_axis
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_registry::core::BlockPos;
    use rivet_registry::core::Vec3i;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;
    use std::panic;

    /// A `WorldGenLevel` double whose `get_block_state` is the unavailable
    /// capability (RivetTodo #399) — it panics, exactly like every production
    /// `WorldGenLevel` before the real world-access lands.
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
        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }

    /// A minimal `StateTestingPredicate` whose pure `test_state` is fully
    /// ported (always true) but whose `test` resolves through the
    /// unavailable `get_block_state` seam.
    #[derive(Debug)]
    struct TriviallyTrue {
        offset: Vec3i,
    }

    impl StateTestingPredicate for TriviallyTrue {
        fn offset(&self) -> &Vec3i {
            &self.offset
        }
        fn test_state(&self, _state: &BlockState) -> bool {
            true
        }
    }

    #[test]
    fn test_shell_fails_loudly_when_world_access_unavailable() {
        // The state-testing `test` resolves `level.getBlockState(origin.offset(
        // offset))` — a capability no production world provides yet — and must
        // fail loudly (never fabricate a state). The offset arithmetic runs
        // first (origin 0 + offset (1,0,0)), then the seam panics.
        let p = TriviallyTrue {
            offset: Vec3i::new(1, 0, 0),
        };
        let origin = BlockPos::new(0, 0, 0);
        let result = panic::catch_unwind(|| state_testing_test(&p, &CapabilityGapLevel, &origin));
        assert!(
            result.is_err(),
            "state-testing test must fail loudly, not fabricate a state"
        );
    }

    #[test]
    fn offset_codec_rejects_axis_at_or_past_max_and_accepts_below() {
        // `Vec3i.offsetCodec(16)` — `Math.abs(v) < 16` per axis, error
        // `"Position out of range, expected at most 16: {value}"`. The codec
        // is ops-generic; under JsonOps the int stream is a JSON array.
        let codec = vec3i_offset_codec::<JsonOps>(16);
        let ok = codec.parse(&JsonOps::INSTANCE, &json!([0, 15, -15]));
        assert!(
            ok.is_success(),
            "got: {:?}",
            ok.error_ref().map(|e| e.message().to_string())
        );
        let bad = codec.parse(&JsonOps::INSTANCE, &json!([0, 16, 0]));
        let msg = bad.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.starts_with("Position out of range, expected at most 16: "),
            "got: {msg}"
        );
    }

    #[test]
    fn offset_codec_accepts_min_int_like_java_math_abs_wrap() {
        // Java `Math.abs(Integer.MIN_VALUE)` wraps to `Integer.MIN_VALUE`
        // (two's complement, no exception), which is `< 16` — so
        // `Vec3i.offsetCodec(16)` ACCEPTS `[-2147483648, 0, 0]`. The port uses
        // `wrapping_abs` to reproduce that exactly; `i32::abs` would panic in
        // debug builds on this hostile input.
        let codec = vec3i_offset_codec::<JsonOps>(16);
        let ok = codec.parse(&JsonOps::INSTANCE, &json!([-2147483648, 0, 0]));
        assert!(
            ok.is_success(),
            "got: {:?}",
            ok.error_ref().map(|e| e.message().to_string())
        );
    }
}

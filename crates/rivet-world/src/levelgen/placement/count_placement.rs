//! Port of `net.minecraft.world.level.levelgen.placement.CountPlacement`
//! (class, 26.2).
//!
//! A `RepeatingPlacement` whose count is the sampled `IntProvider`
//! (`count(RandomSource, BlockPos) { return this.count.sample(random); }`).
//! The `CODEC` is `IntProviders.codec(0, 4096).fieldOf("count").xmap(
//! CountPlacement::new, c -> c.count)` — the required `"count"` field through
//! the validated `IntProviders.codec(0, 4096)` constant-or-dispatch codec (the
//! `#181` dispatch surface), mapped onto the wrapper.
//!
//! DFU `MapCodec<CountPlacement>` is `MapCodec<CountPlacement, Ops>` in the
//! port, so the static Java constant is exposed as the ops-generic
//! `count_placement_codec::<Ops>()` factory. Equality is value-semantic
//! (`PartialEq` on the wrapped provider, the `IntProvider` enum's documented
//! value-equality convention).

use crate::levelgen::placement::placement_modifier_type::{
    PlacementModifierTypeId, PlacementModifierTypes,
};
use crate::levelgen::placement::{PlacementContext, PlacementModifier, RepeatingPlacement};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use rivet_util::valueproviders::constant_int::ConstantInt;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.CountPlacement`.
#[derive(Debug, Clone, PartialEq)]
pub struct CountPlacement {
    /// `count` — the configured count provider.
    pub count: IntProvider,
}

impl CountPlacement {
    /// `new CountPlacement(IntProvider)` — the private constructor (the
    /// codec's `apply` function via `CountPlacement::new` in the `xmap`).
    pub fn new(count: IntProvider) -> Self {
        CountPlacement { count }
    }

    /// `CountPlacement.of(IntProvider)`.
    pub fn of(count: IntProvider) -> Self {
        CountPlacement::new(count)
    }

    /// `CountPlacement.of(int)` — wraps the constant via `ConstantInt.of`,
    /// exactly like Java's int overload.
    pub fn of_value(count: i32) -> Self {
        CountPlacement::new(IntProvider::Constant(ConstantInt::of(count)))
    }
}

impl RepeatingPlacement for CountPlacement {
    /// `count(RandomSource, BlockPos)` — `this.count.sample(random)`; the
    /// origin is unused exactly as in Java.
    fn count<R: RandomSource>(&self, random: &mut R, _origin: &BlockPos) -> i32 {
        self.count.sample(random)
    }
}

impl PlacementModifier for CountPlacement {
    /// `getPositions` — the inherited `RepeatingPlacement` shell (lazy).
    fn get_positions<'a, R: RandomSource>(
        &'a self,
        context: &PlacementContext,
        random: &mut R,
        origin: &BlockPos,
    ) -> Box<dyn Iterator<Item = BlockPos> + 'a> {
        RepeatingPlacement::get_positions(self, context, random, origin)
    }

    /// `type()` — `PlacementModifierType.COUNT` (insertion index 5 in
    /// `PlacementModifierType.java`'s registration order).
    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::COUNT
    }
}

/// `CountPlacement.CODEC` — `IntProviders.codec(0, 4096)` as the required
/// `"count"` field, mapped onto the wrapper, as the ops-generic
/// `count_placement_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// IntProviders.codec(0, 4096).fieldOf("count")
///     .xmap(CountPlacement::new, c -> c.count)
/// ```
pub fn count_placement_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<CountPlacement, Ops>>
{
    let count_field: Arc<dyn MapCodec<IntProvider, Ops>> = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(0, 4096),
        "count".to_string(),
    );
    map_codec::xmap(
        count_field,
        Arc::new(|count: &IntProvider| CountPlacement::new(count.clone())),
        // Java's `c -> c.count` — the private field read inside the lambda.
        Arc::new(|c: &CountPlacement| c.count.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    #[test]
    fn int_factory_wraps_constant_int() {
        // `of(int)` sets `count = ConstantInt.of(count)`.
        let placement = CountPlacement::of_value(7);
        assert_eq!(placement.count, IntProvider::Constant(ConstantInt::of(7)));
    }

    #[test]
    fn provider_factory_keeps_provider() {
        let provider = IntProvider::Uniform(UniformInt::of(0, 3));
        let placement = CountPlacement::of(provider.clone());
        assert_eq!(placement.count, provider);
    }

    #[test]
    fn count_samples_the_provider() {
        // `count(RandomSource, BlockPos) { return this.count.sample(random); }`
        // — a constant provider yields its value regardless of the RNG.
        let placement = CountPlacement::of_value(4);
        let origin = BlockPos::new(1, 2, 3);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(placement.count(&mut random, &origin), 4);
    }

    #[test]
    fn type_identity_is_count() {
        // `PlacementModifierType.COUNT` — insertion index 5,
        // "minecraft:count".
        let placement = CountPlacement::of_value(1);
        assert_eq!(
            placement.type_id(),
            PlacementModifierTypeId::new(5, "minecraft:count")
        );
    }

    #[test]
    fn codec_round_trip_constant_count() {
        // A constant `IntProvider` encodes through the constant-or-dispatch
        // codec as a bare int, inside the `"count"` field. The `MapCodec` is
        // lifted to a `Codec` for the round-trip harness.
        let codec = rivet_serialization::map_codec::codec_of(count_placement_codec::<JsonOps>());
        let placement = CountPlacement::of_value(5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placement)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"count": 5}));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, placement);
    }

    #[test]
    fn codec_round_trip_dispatch_provider() {
        // A non-constant provider encodes through the discriminated
        // `IntProviders` dispatch inside the `"count"` field.
        let codec = rivet_serialization::map_codec::codec_of(count_placement_codec::<JsonOps>());
        let placement = CountPlacement::of(IntProvider::Uniform(UniformInt::of(0, 3)));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placement)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"count": {"min_inclusive": 0, "max_inclusive": 3,
                              "type": "minecraft:uniform"}})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, placement);
    }

    #[test]
    fn codec_requires_count_field() {
        let codec = rivet_serialization::map_codec::codec_of(count_placement_codec::<JsonOps>());
        assert!(codec.parse(&JsonOps::INSTANCE, &json!({})).is_error());
    }

    #[test]
    fn codec_rejects_too_high_provider_on_decode() {
        // `IntProviders.codec(0, 4096)` is inclusive on both ends and
        // validates on decode.
        let codec = rivet_serialization::map_codec::codec_of(count_placement_codec::<JsonOps>());
        let input = json!({"count": {"min_inclusive": 0, "max_inclusive": 5000,
                                     "type": "minecraft:uniform"}});
        let err = codec
            .parse(&JsonOps::INSTANCE, &input)
            .error_ref()
            .map(|e| e.message().to_string());
        assert!(
            err.as_deref()
                .unwrap_or_default()
                .contains("Value provider too high: 4096"),
            "decode error should surface the bounds message, got: {err:?}"
        );
    }
}

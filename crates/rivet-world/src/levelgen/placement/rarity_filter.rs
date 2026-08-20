//! Port of `net.minecraft.world.level.levelgen.placement.RarityFilter`
//! (class, 26.2).
//!
//! Java: a `PlacementFilter` whose `shouldPlace` keeps the origin when
//! `random.nextFloat() < 1.0F / this.chance`, and whose `type()` is
//! `PlacementModifierType.RARITY_FILTER`. Its `CODEC` is the `"chance"` field
//! validated to `[1, MAX]` (`ExtraCodecs.POSITIVE_INT`) mapped onto the
//! private constructor (`RarityFilter::new`) and the `chance` getter, so the
//! decode error is exactly `"Value must be positive: {n}"`.
//!
//! `chance` is private in Java, so there is no public getter; the port mirrors
//! that (only the codec reads it, via the map codec's `from` closure).

use crate::levelgen::placement::placement_modifier_type::{
    PlacementModifierTypeId, PlacementModifierTypes,
};
use crate::levelgen::placement::{PlacementContext, PlacementFilter};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.RarityFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RarityFilter {
    /// `this.chance` — the rarity (inverse probability) of keeping the origin.
    chance: i32,
}

impl RarityFilter {
    /// `RarityFilter(int)` — the private constructor.
    fn new(chance: i32) -> Self {
        RarityFilter { chance }
    }

    /// `onAverageOnceEvery(int)` — the public factory.
    pub fn on_average_once_every(chance: i32) -> Self {
        RarityFilter::new(chance)
    }
}

impl PlacementFilter for RarityFilter {
    fn should_place<R: RandomSource>(
        &self,
        _context: &mut PlacementContext,
        random: &mut R,
        _origin: &BlockPos,
    ) -> bool {
        random.next_float() < 1.0f32 / self.chance as f32
    }

    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::RARITY_FILTER
    }
}

/// `RarityFilter.CODEC` — `ExtraCodecs.POSITIVE_INT.fieldOf("chance").xmap(
/// RarityFilter::new, c -> c.chance)`, as the ops-generic
/// `rarity_filter_map_codec::<Ops>()` factory.
pub fn rarity_filter_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<RarityFilter, Ops>>
{
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &RarityFilter| c.chance),
                "chance".to_string(),
                rivet_util::positive_int::<Ops>(),
            ))
            .apply(instance, Arc::new(|chance: i32| RarityFilter::new(chance)))
    })
}

/// `RarityFilter.CODEC` as a `Codec` (`MapCodec.codec()` — `xmap(...).codec()`),
/// the shape the `#181` generated dispatch's registration table consumes.
#[allow(dead_code)]
pub fn rarity_filter_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<RarityFilter, Ops>> {
    map_codec::codec_of(rarity_filter_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::placement::PlacementModifier;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

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

    /// `random.nextFloat() < 1.0F / chance` — the filter predicate exactly:
    /// a draw that passes chance `n` (the first `LegacyRandomSource` float
    /// after a `setSeed`).
    fn draw_passes(chance: i32, seed: i64) -> bool {
        let mut random = LegacyRandomSource::new(seed);
        random.next_float() < 1.0f32 / chance as f32
    }

    /// `PlacementModifier::get_positions` on a `RarityFilter` over the
    /// overworld window — `shouldPlace ? Stream.of(origin) : Stream.of()`.
    fn filter_positions(
        filter: &RarityFilter,
        random: &mut LegacyRandomSource,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        PlacementModifier::get_positions(filter, &mut context, random, origin).collect()
    }

    #[test]
    fn keeps_origin_once_every_chance_and_drops_otherwise() {
        // `onAverageOnceEvery` + the `PlacementFilter` blanket
        // `get_positions`: a passing draw keeps exactly the origin; a failing
        // draw drops it. `1.0F / chance` is the exact Java f32 division. The
        // seeds are chosen against the bit-exact `LegacyRandomSource`: the
        // first float after `new(1)` is ~0.7309 (fails `chance 2`), and the
        // first after `new(4096)` is ~0.0979 (passes) — the first seed whose
        // first float is below 0.5.
        let origin = BlockPos::new(1, 2, 3);
        let filter = RarityFilter::on_average_once_every(2);
        // A passing draw (chance 2 -> threshold 0.5).
        assert!(draw_passes(2, 4096), "seed 4096 must pass chance 2");
        let mut random = LegacyRandomSource::new(4096);
        let result = filter_positions(&filter, &mut random, &origin);
        assert_eq!(result, vec![origin]);
        // A failing draw.
        assert!(!draw_passes(2, 1), "seed 1 must fail chance 2");
        let mut random = LegacyRandomSource::new(1);
        let result = filter_positions(&filter, &mut random, &origin);
        assert!(result.is_empty());
    }

    #[test]
    fn rarity_type_identity_is_reported() {
        // `PlacementModifierType.RARITY_FILTER` is insertion index 1 in
        // `PlacementModifierType.java`'s registration order.
        let filter = RarityFilter::on_average_once_every(3);
        assert_eq!(
            PlacementFilter::type_id(&filter),
            PlacementModifierTypes::RARITY_FILTER
        );
        assert_eq!(
            PlacementModifier::type_id(&filter),
            PlacementModifierTypes::RARITY_FILTER
        );
    }

    #[test]
    fn codec_round_trips_and_validates_chance() {
        // `ExtraCodecs.POSITIVE_INT.fieldOf("chance")`: the `"chance"` field
        // is required, positive-only, and encodes back.
        let ops = JsonOps::INSTANCE;
        let codec = rarity_filter_codec::<JsonOps>();
        let filter = RarityFilter::on_average_once_every(7);
        let encoded = codec
            .encode_start(&ops, &filter)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"chance": 7}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .copied()
            .expect("decode should succeed");
        assert_eq!(decoded, filter);
    }

    #[test]
    fn codec_rejects_zero_chance_with_java_message() {
        // `ExtraCodecs.POSITIVE_INT` -> `"Value must be positive: 0"`.
        let ops = JsonOps::INSTANCE;
        let codec = rarity_filter_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({"chance": 0}));
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value must be positive: 0");
    }

    #[test]
    fn codec_missing_chance_field_errors() {
        let ops = JsonOps::INSTANCE;
        let codec = rarity_filter_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key chance"), "got: {msg}");
    }
}

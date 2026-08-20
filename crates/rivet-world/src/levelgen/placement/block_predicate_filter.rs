//! Port of `net.minecraft.world.level.levelgen.placement.BlockPredicateFilter`
//! (class, 26.2).
//!
//! Java: a `PlacementFilter` whose `shouldPlace` keeps the origin when
//! `this.predicate.test(context.getLevel(), origin)` — the predicate reads the
//! world through the block-state `#399` world-access seams — and whose
//! `type()` is `PlacementModifierType.BLOCK_PREDICATE_FILTER`. Its `CODEC` is
//! `BlockPredicate.CODEC.fieldOf("predicate")`, so decoding the filter is the
//! recursive block-predicate dispatch (`block_predicate_codec`), whose
//! `matching_blocks`/`matching_biomes`/`matching_fluids` fields raise the ops
//! bound to `RegistryOpsLookup`.
//!
//! The predicate read (`predicate.test`) goes through the `#399` seams: the
//! state/biome/collision reads are unavailable until the world unit lands, so
//! a `BlockPredicateFilter` whose predicate actually reads the world fails
//! explicitly rather than fabricating a result. The dispatch codec value is
//! fully ported (see `blockpredicates::block_predicate`).

use crate::levelgen::blockpredicates::BlockPredicate;
use crate::levelgen::placement::placement_modifier_type::{
    PlacementModifierTypeId, PlacementModifierTypes,
};
use crate::levelgen::placement::{PlacementContext, PlacementFilter};
use rivet_registry::core::BlockPos;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.BlockPredicateFilter`.
///
/// The erased predicate is `Arc<dyn BlockPredicate>` (the recursive dispatch
/// codec's value type); Java's predicate field is `final`, so the port has no
/// public getter — only the codec reads it, via the map codec's `from` closure.
#[derive(Debug, Clone)]
pub struct BlockPredicateFilter {
    /// `this.predicate` — the block predicate whose test gates placement.
    predicate: Arc<dyn BlockPredicate>,
}

impl BlockPredicateFilter {
    /// `forPredicate(BlockPredicate)` — the public factory.
    pub fn for_predicate(predicate: Arc<dyn BlockPredicate>) -> Self {
        BlockPredicateFilter { predicate }
    }
}

impl PlacementFilter for BlockPredicateFilter {
    fn should_place<R: RandomSource>(
        &self,
        context: &mut PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        // `this.predicate.test(context.getLevel(), origin)` — the predicate's
        // world reads (state/biome/collision) are the `#399` seams.
        self.predicate.test(context.get_level(), origin)
    }

    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::BLOCK_PREDICATE_FILTER
    }
}

/// `BlockPredicateFilter.CODEC` — `BlockPredicate.CODEC.fieldOf("predicate")`,
/// as the ops-generic `block_predicate_filter_map_codec::<Ops>()` factory.
///
/// `Ops` must also implement [`RegistryOpsLookup`]: the block-predicate
/// dispatch's `matching_blocks`/`matching_fluids`/`matching_biomes`
/// `"blocks"`/`"fluids"`/`"biomes"` fields are
/// `RegistryCodecs.homogeneousList(...)` (same raise as
/// `block_predicate::block_predicate_codec`).
pub fn block_predicate_filter_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<BlockPredicateFilter, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &BlockPredicateFilter| c.predicate.clone()),
                "predicate".to_string(),
                crate::levelgen::blockpredicates::block_predicate_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|predicate: Arc<dyn BlockPredicate>| {
                    BlockPredicateFilter::for_predicate(predicate)
                }),
            )
    })
}

/// `BlockPredicateFilter.CODEC` as a `Codec` (`MapCodec.codec()` —
/// `record.codec()`), the shape the `#181` generated dispatch's registration
/// table consumes.
#[allow(dead_code)]
pub fn block_predicate_filter_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<BlockPredicateFilter, Ops>> {
    map_codec::codec_of(block_predicate_filter_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypes;
    use crate::levelgen::blockpredicates::{BlockPredicate, always_true};
    use crate::levelgen::placement::PlacementModifier;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON — the only ops implementing
    /// `RegistryOpsLookup` (the block-predicate dispatch's holder-set fields
    /// require it). The always-true predicate needs no registry, so an empty
    /// access is enough.
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

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

    #[test]
    fn predicate_filter_delegates_to_the_predicate() {
        // `this.predicate.test(context.getLevel(), origin)` — an always-true
        // predicate keeps the origin (the blanket `PlacementFilter`
        // `get_positions` shell), and the filter reports the
        // `BLOCK_PREDICATE_FILTER` identity.
        let origin = BlockPos::new(1, 2, 3);
        let filter = BlockPredicateFilter::for_predicate(always_true());
        assert_eq!(
            PlacementFilter::type_id(&filter),
            PlacementModifierTypes::BLOCK_PREDICATE_FILTER
        );
        let mut level = TestLevel(create(-64, 384));
        let generator = NoopGenerator;
        let mut context = PlacementContext::new(&mut level, &generator, None);
        let mut random = LegacyRandomSource::new(0);
        let result: Vec<_> =
            PlacementModifier::get_positions(&filter, &mut context, &mut random, &origin).collect();
        assert_eq!(result, vec![origin]);
    }

    #[test]
    fn codec_round_trips_the_predicate_dispatch() {
        // `BlockPredicate.CODEC.fieldOf("predicate")` — an always-true
        // predicate encodes to the `{}` unit map under the dispatch key
        // `minecraft:true`, and decodes back to an equivalent predicate.
        let codec = block_predicate_filter_codec::<TestOps>();
        let ops = ops();
        let filter = BlockPredicateFilter::for_predicate(always_true());
        let encoded = codec
            .encode_start(&ops, &filter)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"predicate": {"type": "minecraft:true"}}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded.predicate),
            BlockPredicateTypes::TRUE
        );
    }

    #[test]
    fn codec_missing_predicate_field_errors() {
        let codec = block_predicate_filter_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(&ops, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key predicate"), "got: {msg}");
    }

    #[test]
    fn codec_unknown_predicate_type_errors() {
        // The block-predicate by-name dispatch rejects an unknown `"type"`.
        let codec = block_predicate_filter_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(&ops, &json!({"predicate": {"type": "minecraft:nope"}}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:block_predicate_type]: minecraft:nope"),
            "got: {msg}"
        );
    }
}

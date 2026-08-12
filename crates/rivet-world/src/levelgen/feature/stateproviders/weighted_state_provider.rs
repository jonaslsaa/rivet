//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! WeightedStateProvider` (class, 26.2).
//!
//! Java: a provider that holds a `WeightedList<BlockState>` and samples by
//! selecting a weighted element (`getRandomOrThrow`). `type()` is
//! `BlockStateProviderType.WEIGHTED_STATE_PROVIDER`. The constructor throws
//! `IllegalArgumentException("Weighted list must have at least one entry")`
//! for an empty list (mirrored by the `Vec`-length `panic!`, the Rust analog
//! of Java's unchecked exception).
//!
//! `CODEC` is a record codec over the `"entries"` field,
//! `WeightedList.nonEmptyCodec(BlockState.CODEC)` — the element codec is
//! `BlockState.CODEC`, *not* the recursive `BlockStateProvider.CODEC`, so the
//! `RecursiveSelf` the dispatch graph passes to this provider's map codec is
//! unused (see `block_state_provider`). No `toString` in Java (identity-based
//! `Object.toString`), so none is ported.

use rivet_registry::block_state::BlockState;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::WeightedList;
use std::sync::Arc;

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    BlockStateProvider, ErasedBlockStateProvider,
};
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use rivet_registry::core::BlockPos;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.WeightedStateProvider`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedStateProvider {
    /// `this.weightedList`.
    weighted_list: WeightedList<BlockState>,
}

impl WeightedStateProvider {
    /// `new WeightedStateProvider(WeightedList<BlockState>)` — the public
    /// constructor. Java throws `IllegalArgumentException` for an empty list;
    /// the Rust analog panics with Paper's exact message.
    pub fn new(weighted_list: WeightedList<BlockState>) -> WeightedStateProvider {
        if weighted_list.is_empty() {
            panic!("Weighted list must have at least one entry");
        }
        WeightedStateProvider { weighted_list }
    }

    /// `new WeightedStateProvider(WeightedList.Builder<BlockState>)` — the
    /// builder form, `this(weightedList.build())`.
    pub fn from_builder(
        builder: rivet_util::WeightedListBuilder<BlockState>,
    ) -> WeightedStateProvider {
        WeightedStateProvider::new(builder.build())
    }

    /// `this.weightedList`.
    pub fn weighted_list(&self) -> &WeightedList<BlockState> {
        &self.weighted_list
    }
}

impl BlockStateProvider for WeightedStateProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        random: &mut R,
        _pos: &BlockPos,
    ) -> BlockState {
        self.weighted_list.get_random_or_throw(random)
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `WeightedStateProvider.CODEC` — a record codec over the `"entries"` field
/// (`WeightedList.nonEmptyCodec(BlockState.CODEC)`), as the ops-generic
/// `weighted_state_provider_map_codec::<Ops>(top)` factory. `top` is the
/// `BlockStateProvider.CODEC` `RecursiveSelf` from the dispatch graph, but it
/// is ignored (`_top`): the entries hold `BlockState`s, not providers, so this
/// provider does not recurse into `BlockStateProvider.CODEC`.
pub fn weighted_state_provider_map_codec<Ops: DynamicOps + 'static>(
    _top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> Arc<dyn MapCodec<WeightedStateProvider, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|w: &WeightedStateProvider| w.weighted_list.clone()),
                // `WeightedList.nonEmptyCodec(BlockState.CODEC).fieldOf("entries")`.
                codec::field_of::<WeightedList<BlockState>, Ops>(
                    rivet_util::weighted::weighted_list_non_empty_codec::<BlockState, Ops>(
                        rivet_registry::block_state_codec::block_state_codec::<Ops>(),
                    ),
                    "entries".to_string(),
                ),
            ))
            .apply(instance, Arc::new(WeightedStateProvider::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::Weighted;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    fn states(ids: &[u16]) -> WeightedList<BlockState> {
        WeightedList::new(
            &ids.iter()
                .map(|&id| Weighted::new(BlockState::of(BlockId::from_id(id)), 1))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn get_state_samples_a_weighted_element() {
        // A single weight-1 element always returns that state.
        let single =
            WeightedStateProvider::new(WeightedList::of_value(BlockState::of(BlockId::from_id(0))));
        let mut random = LegacyRandomSource::new(12345);
        let state = single.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert_eq!(state, BlockState::of(BlockId::from_id(0)));

        // Two weight-1 elements: every sample is one of the two.
        let pair = WeightedStateProvider::new(states(&[0, 1]));
        let samples: Vec<BlockState> = (0..6)
            .map(|_| pair.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0)))
            .collect();
        for s in &samples {
            assert!(
                s == &BlockState::of(BlockId::from_id(0))
                    || s == &BlockState::of(BlockId::from_id(1))
            );
        }
    }

    #[test]
    #[should_panic(expected = "Weighted list must have at least one entry")]
    fn empty_weighted_list_panics_on_construction() {
        let _ = WeightedStateProvider::new(WeightedList::of());
    }

    #[test]
    fn codec_round_trips_through_the_entries_field() {
        let codec =
            rivet_serialization::map_codec::codec_of(weighted_state_provider_map_codec::<JsonOps>(
                // The recursive self is unused by `BlockState` (not recursive),
                // so a placeholder provider Arc suffices for the direct
                // map-codec test.
                rivet_serialization::map_codec::unit_codec(Arc::new(
                    crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(
                        BlockState::of(BlockId::from_id(0)),
                    ),
                ) as Arc<dyn ErasedBlockStateProvider>),
            ));
        let input = json!({
            "entries": [
                {"data": {"Name": "minecraft:air"}, "weight": 1},
                {"data": {"Name": "minecraft:stone"}, "weight": 3}
            ]
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            BlockStateProvider::type_id(decoded),
            BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER
        );
        assert_eq!(decoded.weighted_list().unwrap().len(), 2);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn non_empty_codec_rejects_empty_entries() {
        let codec =
            rivet_serialization::map_codec::codec_of(weighted_state_provider_map_codec::<JsonOps>(
                rivet_serialization::map_codec::unit_codec(Arc::new(
                    crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(
                        BlockState::of(BlockId::from_id(0)),
                    ),
                ) as Arc<dyn ErasedBlockStateProvider>),
            ));
        let input = json!({"entries": []});
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(
            msg,
            "Weighted list must contain at least one entry with non-zero weight"
        );
    }

    struct TestLevel;

    impl crate::level::height_accessor::LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl crate::level::WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            // RivetTodo(#399): never read here.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }
}

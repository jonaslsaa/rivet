//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! RotatedBlockProvider` (class, 26.2).
//!
//! Java: a provider that stores a `Block` and yields its default state with a
//! random pillar axis: `Direction.Axis.getRandom(random)` (=
//! `Util.getRandom(Direction.Axis.VALUES, random)`), then
//! `block.defaultBlockState().trySetValue(RotatedPillarBlock.AXIS, axis)`.
//! `type()` is `BlockStateProviderType.ROTATED_BLOCK_PROVIDER`.
//!
//! `CODEC` is `BlockState.CODEC.fieldOf("state").xmap(BlockStateBase::getBlock,
//! Block::defaultBlockState)` — the state serializes as a plain block (its
//! default state), so the codec round-trips through `BlockState`. No `Block`
//! value type is ported in this unit: the field is held as a `BlockId`
//! (the registry-held block identity), and `Block::defaultBlockState` is
//! `BlockState::of(block)`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::BlockStateProvider;
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::core::Axis;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.RotatedBlockProvider`.
#[derive(Debug, Clone, PartialEq)]
pub struct RotatedBlockProvider {
    /// `this.block` — the registry-held `Block` identity.
    block: BlockId,
}

impl RotatedBlockProvider {
    /// `new RotatedBlockProvider(Block)`.
    pub fn new(block: BlockId) -> RotatedBlockProvider {
        RotatedBlockProvider { block }
    }

    /// `this.block`.
    pub fn block(&self) -> BlockId {
        self.block
    }
}

impl BlockStateProvider for RotatedBlockProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        random: &mut R,
        _pos: &BlockPos,
    ) -> BlockState {
        // `Direction.Axis.getRandom(random)` = `Util.getRandom(Axis.VALUES,
        // random)`.
        let random_axis = rivet_util::util::get_random(&Axis::VALUES, random);
        // `this.block.defaultBlockState().trySetValue(RotatedPillarBlock.AXIS,
        // randomAxis)` — `Block::defaultBlockState` is `BlockState::of(block)`;
        // `trySetValue` returns the state unchanged when the block has no axis
        // property (`trySetValue` with `RotatedPillarBlock.AXIS`, an
        // `EnumProperty<Direction.Axis>`; the `From<Axis>` value conversion).
        BlockState::of(self.block)
            .try_set_value(BlockStateProperties::AXIS, random_axis)
            .expect("RotatedBlockProvider set a valid axis")
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `RotatedBlockProvider.CODEC` — `BlockState.CODEC.fieldOf("state")
/// .xmap(BlockStateBase::getBlock, Block::defaultBlockState)` (the
/// `BlockStateProvider`-erased form of the codec is lifted by the dispatch
/// graph), as the ops-generic `rotated_block_provider_map_codec::<Ops>()`
/// factory.
pub fn rotated_block_provider_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<RotatedBlockProvider, Ops>> {
    let state_codec = codec::field_of(
        rivet_registry::block_state_codec::block_state_codec::<Ops>(),
        "state".to_string(),
    );
    // `.xmap(BlockStateBase::getBlock, Block::defaultBlockState)` — the field
    // encodes a `BlockId` as its default `BlockState` and decodes back via
    // `BlockState::block()`.
    map_codec::xmap(
        state_codec,
        Arc::new(|state: &BlockState| RotatedBlockProvider::new(state.block())),
        Arc::new(|p: &RotatedBlockProvider| BlockState::of(p.block)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trips_through_the_state_field() {
        let codec = map_codec::codec_of(rotated_block_provider_map_codec::<JsonOps>());
        // `minecraft:oak_log` — a rotated-pillar block whose default state has
        // the axis property. Its state id is looked up by name.
        let log = BlockId::from_name("minecraft:oak_log").expect("oak_log block exists");
        let input = json!({"state": {"Name": "minecraft:oak_log"}});
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            decoded.type_id(),
            BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER
        );
        assert_eq!(decoded.block(), log);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        // `oak_log` is non-singleton: encode writes its full default-state
        // `Properties` compound (`Optional::of` always emits it), then the
        // `"Name"` type key.
        assert_eq!(
            encoded,
            json!({"state": {"Properties": {"axis": "y"}, "Name": "minecraft:oak_log"}})
        );
    }

    #[test]
    fn get_state_sets_a_random_pillar_axis() {
        // Oak log's default state has the `axis` property, so `trySetValue`
        // sets one of X/Y/Z; the state remains on the same block.
        let log = BlockId::from_name("minecraft:oak_log").expect("oak_log block exists");
        let p = RotatedBlockProvider::new(log);
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        for _ in 0..8 {
            let state = p.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
            assert_eq!(state.block(), log);
            let axis = state
                .get_value(BlockStateProperties::AXIS)
                .expect("axis property set on a rotated block");
            assert!(
                matches!(
                    axis,
                    rivet_registry::block_state_property::PropertyValue::Enum("x")
                        | rivet_registry::block_state_property::PropertyValue::Enum("y")
                        | rivet_registry::block_state_property::PropertyValue::Enum("z")
                ),
                "axis was {axis:?}"
            );
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

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            // RivetTodo(#399): never read here.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }
}

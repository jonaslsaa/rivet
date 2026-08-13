//! Port of `net.minecraft.world.level.levelgen.feature.configurations.BlockColumnConfiguration`
//! (record, 26.2).
//!
//! Java: a four-field record `record BlockColumnConfiguration(List<Layer>
//! layers, Direction direction, BlockPredicate allowedPlacement, boolean
//! prioritizeTip)` whose `CODEC` is a `RecordCodecBuilder` over the required
//! `"layers"` field (`Layer.CODEC.listOf()`), the required `"direction"` field
//! (`Direction.CODEC`), the required `"allowed_placement"` field
//! (`BlockPredicate.CODEC` — the `"type"` by-name dispatch) and the required
//! `"prioritize_tip"` field (`Codec.BOOL`). The nested `Layer` record
//! (`IntProvider height, BlockStateProvider state`) has its own `CODEC` over
//! the required `"height"` field (`IntProviders.NON_NEGATIVE_CODEC` — the
//! integer provider dispatch validated to `[0, MAX_VALUE]`) and the required
//! `"provider"` field (`BlockStateProvider.CODEC` — the `"type"` by-name
//! dispatch). The static helpers `layer(height, state)` and `simple(height,
//! state)` (which wraps one layer in `Direction.UP` with
//! `BlockPredicate.ONLY_IN_AIR_PREDICATE` and `prioritizeTip = false`) are
//! mirrored. DFU `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java
//! constants are exposed as the ops-generic
//! `block_column_configuration_codec::<Ops>()` / `layer_codec::<Ops>()`
//! factories.
//!
//! The `allowedPlacement` half is held as the erased `Arc<dyn BlockPredicate>`
//! carrier and the `Layer.state` half as the erased
//! `Arc<dyn ErasedBlockStateProvider>` carrier; neither trait extends
//! `PartialEq` (predicates/providers are behavior, not values), so the
//! configuration derives no `PartialEq` — the same shape
//! `DiskConfiguration`/`SimpleBlockConfiguration` take.
//!
//! `simple()` builds the `allowedPlacement` as Paper's `ONLY_IN_AIR_PREDICATE`
//! — `matchesTag(BlockTags.AIR)`, a [`MatchingBlockTagPredicate`] over the
//! `minecraft:air` tag (members `minecraft:air`/`minecraft:void_air`/
//! `minecraft:cave_air`). The tag query itself is served by the generated
//! behavior-table tag surface (`BlockState::is_in_tag`); only the world-access
//! `test` path defers through the `#399` seam like every other predicate.

use crate::levelgen::blockpredicates::{
    BlockPredicate, MatchingBlockTagPredicate, block_predicate_codec,
};
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use rivet_registry::Identifier;
use rivet_registry::TagKey;
use rivet_registry::core::Direction;
use rivet_registry::core::Vec3i;
use rivet_registry::core::direction_codec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::int_provider::{IntProvider, non_negative_int_provider_codec};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.BlockColumnConfiguration`.
///
/// The `allowedPlacement` half is held as the erased `Arc<dyn BlockPredicate>`
/// carrier; the trait does not extend `PartialEq`, so the configuration is
/// `Clone`+`Debug` only (the same shape `DiskConfiguration` takes).
#[derive(Debug, Clone)]
pub struct BlockColumnConfiguration {
    /// `layers` — the column's layers, bottom to top.
    pub layers: Vec<Layer>,
    /// `direction` — the direction the column grows.
    pub direction: Direction,
    /// `allowedPlacement` — the predicate for blocks the column may replace.
    pub allowed_placement: Arc<dyn BlockPredicate>,
    /// `prioritizeTip` — whether the column tips are prioritized.
    pub prioritize_tip: bool,
}

/// `BlockColumnConfiguration.Layer` — one column layer: a height provider and
/// the block state provider filling that height.
///
/// The `state` half is held as the erased `Arc<dyn ErasedBlockStateProvider>`
/// carrier, so `Layer` is `Clone`+`Debug` only (no `PartialEq`).
#[derive(Debug, Clone)]
pub struct Layer {
    /// `Layer.height` — an `IntProvider` validated to `[0, MAX_VALUE]`.
    pub height: IntProvider,
    /// `Layer.state` — the block state provider for the layer's blocks.
    pub state: Arc<dyn ErasedBlockStateProvider>,
}

impl BlockColumnConfiguration {
    /// `new BlockColumnConfiguration(List<Layer>, Direction, BlockPredicate,
    /// boolean)` — the record constructor (the codec's `apply` function).
    pub fn new(
        layers: Vec<Layer>,
        direction: Direction,
        allowed_placement: Arc<dyn BlockPredicate>,
        prioritize_tip: bool,
    ) -> Self {
        BlockColumnConfiguration {
            layers,
            direction,
            allowed_placement,
            prioritize_tip,
        }
    }

    /// `BlockColumnConfiguration.layers()`.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// `BlockColumnConfiguration.direction()`.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// `BlockColumnConfiguration.allowedPlacement()`.
    pub fn allowed_placement(&self) -> &Arc<dyn BlockPredicate> {
        &self.allowed_placement
    }

    /// `BlockColumnConfiguration.prioritizeTip()`.
    pub fn prioritize_tip(&self) -> bool {
        self.prioritize_tip
    }

    /// `BlockColumnConfiguration.layer(IntProvider, BlockStateProvider)`.
    pub fn layer(height: IntProvider, state: Arc<dyn ErasedBlockStateProvider>) -> Layer {
        Layer::new(height, state)
    }

    /// `BlockColumnConfiguration.simple(IntProvider, BlockStateProvider)` — a
    /// single-layer `Direction.UP` column with
    /// `BlockPredicate.ONLY_IN_AIR_PREDICATE` and `prioritizeTip = false`.
    pub fn simple(height: IntProvider, state: Arc<dyn ErasedBlockStateProvider>) -> Self {
        BlockColumnConfiguration::new(
            vec![BlockColumnConfiguration::layer(height, state)],
            Direction::Up,
            only_in_air_predicate(),
            false,
        )
    }
}

/// `BlockPredicate.ONLY_IN_AIR_PREDICATE` — `matchesTag(BlockTags.AIR)`.
///
/// Java: `matchesTag(Vec3i.ZERO, BlockTags.AIR)` — a `MatchingBlockTagPredicate`
/// matching the `minecraft:air` block tag (`air`, `void_air`, `cave_air`). The
/// predicate's `test` defers through the block-predicate `#399` world-access
/// seam (like every other predicate); the value surface is faithful.
pub fn only_in_air_predicate() -> Arc<dyn BlockPredicate> {
    Arc::new(MatchingBlockTagPredicate::new(
        Vec3i::ZERO,
        TagKey::create(
            &*rivet_registry::registries::BLOCK,
            Identifier::parse("minecraft:air"),
        ),
    ))
}

impl Layer {
    /// `new Layer(IntProvider, BlockStateProvider)` — the nested record
    /// constructor (the codec's `apply` function).
    pub fn new(height: IntProvider, state: Arc<dyn ErasedBlockStateProvider>) -> Self {
        Layer { height, state }
    }

    /// `Layer.height()`.
    pub fn height(&self) -> &IntProvider {
        &self.height
    }

    /// `Layer.state()`.
    pub fn state(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.state
    }
}

/// `Layer.CODEC` — a record codec over the required `"height"` and `"provider"`
/// fields, as the ops-generic `layer_codec::<Ops>()` factory.
pub fn layer_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>() -> Arc<dyn Codec<Layer, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|l: &Layer| l.height.clone()),
                "height".to_string(),
                non_negative_int_provider_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|l: &Layer| l.state.clone()),
                codec::field_of(block_state_provider_codec::<Ops>(), "provider".to_string()),
            ))
            .apply(
                instance,
                Arc::new(
                    |height: IntProvider, state: Arc<dyn ErasedBlockStateProvider>| {
                        Layer::new(height, state)
                    },
                ),
            )
    })
}

/// `BlockColumnConfiguration.CODEC` — a record codec over the required
/// `"layers"`, `"direction"`, `"allowed_placement"` and `"prioritize_tip"`
/// fields, as the ops-generic `block_column_configuration_codec::<Ops>()`
/// factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     Layer.CODEC.listOf().fieldOf("layers"),
///     Direction.CODEC.fieldOf("direction"),
///     BlockPredicate.CODEC.fieldOf("allowed_placement"),
///     Codec.BOOL.fieldOf("prioritize_tip"))
///     .apply(i, BlockColumnConfiguration::new))
/// ```
pub fn block_column_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<BlockColumnConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &BlockColumnConfiguration| c.layers.clone()),
                codec::field_of(codec::list(layer_codec::<Ops>()), "layers".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &BlockColumnConfiguration| c.direction),
                codec::field_of(direction_codec::<Ops>(), "direction".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &BlockColumnConfiguration| c.allowed_placement.clone()),
                codec::field_of(
                    block_predicate_codec::<Ops>(),
                    "allowed_placement".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|c: &BlockColumnConfiguration| c.prioritize_tip),
                "prioritize_tip".to_string(),
                codec::bool_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(
                    |layers: Vec<Layer>,
                     direction: Direction,
                     allowed_placement: Arc<dyn BlockPredicate>,
                     prioritize_tip: bool| {
                        BlockColumnConfiguration::new(
                            layers,
                            direction,
                            allowed_placement,
                            prioritize_tip,
                        )
                    },
                ),
            )
    })
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for BlockColumnConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::blockpredicates::StateTestingPredicate;
    use crate::levelgen::blockpredicates::block_predicate_type::BlockPredicateTypes;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::block_state_codec::block_state_codec;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// `block_state_provider_codec`/`block_predicate_codec` dispatch over the
    /// registry-backed matching predicates, so the codec requires `RegistryOps`
    /// (the `RegistryOpsLookup` ops). The `simple` provider and `always_true`
    /// predicate dispatch through the by-name type registry, so an empty access
    /// is enough.
    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn air_state() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:air").unwrap())
    }

    #[test]
    fn layer_codec_round_trip() {
        // Paper's NetherFeatures.BASALT_COLUMN layers are
        // `new Layer(UniformInt.of(1, 4), new SimpleStateProvider(...))`.
        let codec = layer_codec::<TestOps>();
        let layer = Layer::new(
            IntProvider::Constant(ConstantInt::of(2)),
            Arc::new(simple(air_state())),
        );
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &layer)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "height": 2,
                "provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.height, layer.height);
    }

    #[test]
    fn codec_round_trip() {
        // Paper's NetherFeatures.BASALT_COLUMN:
        // `new BlockColumnConfiguration(List.of(layer(UniformInt.of(1, 4),
        // new SimpleStateProvider(BASALT)), layer(ConstantInt.of(1),
        // new SimpleStateProvider(BASALT))), Direction.UP,
        // BlockPredicate.ONLY_IN_AIR_PREDICATE, false)` — here with the
        // `ONLY_IN_AIR_PREDICATE` tag predicate and a two-layer stack.
        let layer = |height: i32| -> Layer {
            Layer::new(
                IntProvider::Constant(ConstantInt::of(height)),
                Arc::new(simple(air_state())),
            )
        };
        let config = BlockColumnConfiguration::new(
            vec![layer(1), layer(2)],
            Direction::Up,
            only_in_air_predicate(),
            false,
        );
        let codec = block_column_configuration_codec::<TestOps>();
        let ops = ops();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "layers": [
                    {"height": 1, "provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}}},
                    {"height": 2, "provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}}},
                ],
                "direction": "up",
                "allowed_placement": {"type": "minecraft:matching_block_tag", "tag": "minecraft:air"},
                "prioritize_tip": false,
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.layers.len(), 2);
        assert_eq!(decoded.direction, Direction::Up);
        assert!(!decoded.prioritize_tip);
        assert_eq!(
            BlockPredicate::type_id(&*decoded.allowed_placement),
            BlockPredicateTypes::MATCHING_BLOCK_TAG
        );
    }

    #[test]
    fn simple_builds_the_up_column_shape() {
        // `new BlockColumnConfiguration(List.of(layer(height, state)),
        // Direction.UP, BlockPredicate.ONLY_IN_AIR_PREDICATE, false)` — the
        // single-layer `Direction.UP` column with `prioritizeTip = false`.
        let config = BlockColumnConfiguration::simple(
            IntProvider::Constant(ConstantInt::of(3)),
            Arc::new(simple(air_state())),
        );
        assert_eq!(config.layers.len(), 1);
        assert_eq!(config.direction, Direction::Up);
        assert!(!config.prioritize_tip);
        assert_eq!(
            BlockPredicate::type_id(&*config.allowed_placement),
            BlockPredicateTypes::MATCHING_BLOCK_TAG
        );
        let as_tag = config
            .allowed_placement
            .as_any()
            .downcast_ref::<MatchingBlockTagPredicate>()
            .expect("only-in-air is a MatchingBlockTagPredicate");
        assert_eq!(as_tag.offset(), &Vec3i::ZERO);
        assert_eq!(
            as_tag.tag().location().to_string(),
            "minecraft:air".to_string()
        );
        // The tag value is behavior-table backed: air/cave_air/void_air match,
        // stone does not (`state.is(minecraft:air)`).
        assert!(as_tag.test_state(&air_state()));
        assert!(as_tag.test_state(&BlockState::of(
            BlockId::from_name("minecraft:cave_air").unwrap()
        )));
        assert!(!as_tag.test_state(&BlockState::of(
            BlockId::from_name("minecraft:stone").unwrap()
        )));
    }

    #[test]
    fn codec_requires_all_fields() {
        let codec = block_column_configuration_codec::<TestOps>();
        let ops = ops();
        // The `"prioritize_tip"` field is required (`Codec.BOOL.fieldOf`).
        let missing = json!({
            "layers": [],
            "direction": "up",
            "allowed_placement": {"type": "minecraft:true"},
        });
        assert!(codec.parse(&ops, &missing).is_error());
        // The `"direction"` field is required and resolves by serialized name.
        let bad_direction = json!({
            "layers": [],
            "direction": "sideways",
            "allowed_placement": {"type": "minecraft:true"},
            "prioritize_tip": false,
        });
        assert!(codec.parse(&ops, &bad_direction).is_error());
    }

    #[test]
    fn unknown_block_state_provider_type_errors() {
        // `BlockStateProvider.CODEC` — an unknown `"type"` key errors with the
        // by-name registry message (exercised through the nested layer codec).
        let codec = layer_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(
            &ops,
            &json!({"height": 1, "provider": {"type": "minecraft:not_a_provider"}}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("Unknown registry key"), "got: {msg}");
    }

    #[test]
    fn layer_bounds_are_non_negative() {
        // `IntProviders.NON_NEGATIVE_CODEC` — a negative height provider fails
        // decode with Paper's `"Value provider too low"` message.
        let codec = layer_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(
            &ops,
            &json!({"height": -1, "provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}}}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("Value provider too low"), "got: {msg}");
    }

    /// The `"layers"` field codec is `Layer.CODEC.listOf()` — its element
    /// codec needs `RegistryOps` too (the nested provider dispatch), so
    /// `block_state_codec` (the plain `JsonOps` element codec) is not used
    /// here; this just pins that the `simple_state_provider` type name is the
    /// registered serialized name.
    #[test]
    fn simple_state_provider_encodes_through_block_state_codec_shape() {
        // The `simple` provider writes `{"type": "minecraft:simple_state_provider",
        // "state": <BlockState.CODEC>}`; the state itself is the `"Name"`
        // dispatch (exercised end-to-end in `layer_codec_round_trip`). Here the
        // plain state codec confirms the `minecraft:air` singleton shape.
        let state = air_state();
        let encoded = block_state_codec::<JsonOps>()
            .encode_start(&JsonOps::INSTANCE, &state)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"Name": "minecraft:air"}));
    }
}

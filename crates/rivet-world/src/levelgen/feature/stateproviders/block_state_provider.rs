//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! BlockStateProvider` (abstract class, 26.2) — the dispatch root of the
//! block-state-provider framework.
//!
//! Java is the abstract base of the eight concrete providers, with the
//! dispatch codec `CODEC`:
//!
//! ```text
//! CODEC = BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.byNameCodec()
//!     .dispatch(BlockStateProvider::type, BlockStateProviderType::codec);
//! ```
//!
//! The port splits identity from behavior the same way `BlockPredicate` does:
//! [`BlockStateProvider`] is the generic behavior contract whose `get_state` is
//! generic over the random source (`RandomSource` is `Sized`, not object-safe),
//! and [`ErasedBlockStateProvider`] is the object-safe carrier the providers'
//! codec graph stores each provider as (`Arc<dyn ErasedBlockStateProvider>`).
//! Every concrete provider implements `BlockStateProvider`, so the erased
//! carrier is blanket-derived; `as_any` is the explicit downcast seam (Java's
//! erased `BlockStateProvider` cast) the dispatch codec uses on encode, exactly
//! like `BlockPredicate::as_any`.
//!
//! The `CODEC` is the ops-generic [`block_state_provider_codec::<Ops>()`]
//! factory: a `codec::recursive` graph whose single `RecursiveSelf` threads
//! into the recursive fields (`RandomizedIntStateProvider`'s source,
//! `RuleBasedStateProvider`'s fallback/rules) so arbitrary nesting round-trips
//! — the same pattern `BlockPredicate.CODEC` / `HeightProvider.CODEC` use.
//! (`WeightedStateProvider` is *not* recursive: its `entries` hold
//! `BlockState`s, so its map codec ignores the `RecursiveSelf`.) `Ops` must
//! implement [`RegistryOpsLookup`]: `RuleBasedStateProvider.Rule` embeds
//! `BlockPredicate.CODEC`, whose `matching_blocks`/`matching_fluids`/
//! `matching_biomes` fields resolve the registry through the ops.
//!
//! `BlockStateProvider::getState`'s dispatch over the erased carrier is
//! [`block_state_provider_get_state`] below: it downcasts the erased carrier to
//! the concrete provider and calls its `get_state` (Java's virtual `getState`).
//! All eight providers are owned by this unit, so the dispatch is a closed
//! match (the analogue of the `BlockStateProviderType.register` table; the
//! `#181` codegen tables cover the *other* frameworks — features, placement
//! modifiers). The recursive providers (`RandomizedIntStateProvider`'s source,
//! `RuleBasedStateProvider`'s fallback/rules) reach their stored erased
//! providers through this function.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes, block_state_provider_type_by_name,
};
use crate::levelgen::feature::stateproviders::dual_noise_provider::DualNoiseProvider;
use crate::levelgen::feature::stateproviders::noise_provider::NoiseProvider;
use crate::levelgen::feature::stateproviders::noise_threshold_provider::NoiseThresholdProvider;
use crate::levelgen::feature::stateproviders::randomized_int_state_provider::RandomizedIntStateProvider;
use crate::levelgen::feature::stateproviders::rotated_block_provider::RotatedBlockProvider;
use crate::levelgen::feature::stateproviders::rule_based_state_provider::RuleBasedStateProvider;
use crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider;
use crate::levelgen::feature::stateproviders::weighted_state_provider::WeightedStateProvider;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider`
/// — the abstract behavior contract of every state provider.
///
/// Implemented by the concrete provider structs (owned by this unit).
/// `get_state` is generic over the random source (`RandomSource` is `Sized`),
/// so the concrete providers are dispatched monomorphically (by the `#181`
/// generated match when wired), not through a `dyn`.
pub trait BlockStateProvider: Any + Debug + Send + Sync + 'static {
    /// `getState(WorldGenLevel, RandomSource, BlockPos)` — the provider's
    /// per-position block state.
    fn get_state<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
    ) -> BlockState;

    /// `getOptionalState(WorldGenLevel, RandomSource, BlockPos)` — the default
    /// implementation delegates to `getState`; overridden by
    /// `RuleBasedStateProvider` (which returns `None` when no rule matches and
    /// no fallback is present).
    fn get_optional_state<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
    ) -> Option<BlockState> {
        Some(self.get_state(level, random, pos))
    }

    /// `type()` — the registry-held `BlockStateProviderType<?>` identity this
    /// provider dispatches on (the key `BlockStateProvider.CODEC` uses).
    fn type_id(&self) -> BlockStateProviderTypeId;

    /// `as_any` — the downcast seam (Java's erased `BlockStateProvider` cast)
    /// the dispatch codec uses on encode to recover the concrete provider type.
    fn as_any(&self) -> &dyn Any;
}

/// The object-safe carrier the codec graph stores each provider as — the
/// dispatch identity plus the `dyn`-compatible surface. Every
/// `BlockStateProvider` implements it via the blanket impl, so the concrete
/// provider modules only implement `BlockStateProvider`.
pub trait ErasedBlockStateProvider: Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> BlockStateProviderTypeId;

    /// `as_any` — the downcast seam over the erased carrier.
    fn as_any(&self) -> &dyn Any;
}

impl<P: BlockStateProvider + ?Sized> ErasedBlockStateProvider for P {
    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProvider::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        BlockStateProvider::as_any(self)
    }
}

/// `getState(WorldGenLevel, RandomSource, BlockPos)` over the erased carrier —
/// dispatch an erased provider to its state.
///
/// This is the closed analogue of the `BlockStateProviderType.register` table:
/// all eight concrete providers are owned by this unit, so the dispatch is a
/// complete match that downcasts the erased carrier and calls the concrete
/// `get_state` (Java's virtual `getState`). The recursive providers
/// (`RandomizedIntStateProvider` calling its `source`, `RuleBasedStateProvider`
/// calling each `rule.then()` / `fallback`) reach the stored erased provider
/// through this function. `get_state` is generic over the random source
/// (`RandomSource` is `Sized`, not object-safe), so the dispatch monomorphizes
/// per `R` — the same split as `feature_place`. The unknown-id arm is
/// unreachable for the eight registered types (Java's
/// `Registry.getValueOrThrow` throws only for a genuinely missing key).
pub fn block_state_provider_get_state<R: RandomSource>(
    provider: &dyn ErasedBlockStateProvider,
    level: &dyn WorldGenLevel,
    random: &mut R,
    pos: &BlockPos,
) -> BlockState {
    let any = provider.as_any();
    if let Some(s) = any.downcast_ref::<SimpleStateProvider>() {
        s.get_state(level, random, pos)
    } else if let Some(w) = any.downcast_ref::<WeightedStateProvider>() {
        w.get_state(level, random, pos)
    } else if let Some(n) = any.downcast_ref::<NoiseThresholdProvider>() {
        n.get_state(level, random, pos)
    } else if let Some(n) = any.downcast_ref::<NoiseProvider>() {
        n.get_state(level, random, pos)
    } else if let Some(d) = any.downcast_ref::<DualNoiseProvider>() {
        d.get_state(level, random, pos)
    } else if let Some(r) = any.downcast_ref::<RotatedBlockProvider>() {
        r.get_state(level, random, pos)
    } else if let Some(r) = any.downcast_ref::<RandomizedIntStateProvider>() {
        r.get_state(level, random, pos)
    } else if let Some(r) = any.downcast_ref::<RuleBasedStateProvider>() {
        r.get_state(level, random, pos)
    } else {
        panic!(
            "Trying to apply block state provider type '{}' with no registered behavior (#181 codegen)",
            provider.type_id().location
        );
    }
}

/// `getOptionalState(...)` over the erased carrier — dispatch an erased
/// provider to its optional state.
///
/// `TrunkPlacer.placeBelowTrunkBlock` calls
/// `config.belowTrunkProvider.getOptionalState(...)`, so this closed match
/// mirrors [`block_state_provider_get_state`] and forwards to each concrete
/// provider's `get_optional_state` (the default `Some(get_state)`, overridden
/// by `RuleBasedStateProvider`). The unknown-id arm is unreachable for the
/// eight registered types, same as the `get_state` dispatch.
pub fn block_state_provider_get_optional_state<R: RandomSource>(
    provider: &dyn ErasedBlockStateProvider,
    level: &dyn WorldGenLevel,
    random: &mut R,
    pos: &BlockPos,
) -> Option<BlockState> {
    let any = provider.as_any();
    if let Some(s) = any.downcast_ref::<SimpleStateProvider>() {
        s.get_optional_state(level, random, pos)
    } else if let Some(w) = any.downcast_ref::<WeightedStateProvider>() {
        w.get_optional_state(level, random, pos)
    } else if let Some(n) = any.downcast_ref::<NoiseThresholdProvider>() {
        n.get_optional_state(level, random, pos)
    } else if let Some(n) = any.downcast_ref::<NoiseProvider>() {
        n.get_optional_state(level, random, pos)
    } else if let Some(d) = any.downcast_ref::<DualNoiseProvider>() {
        d.get_optional_state(level, random, pos)
    } else if let Some(r) = any.downcast_ref::<RotatedBlockProvider>() {
        r.get_optional_state(level, random, pos)
    } else if let Some(r) = any.downcast_ref::<RandomizedIntStateProvider>() {
        r.get_optional_state(level, random, pos)
    } else if let Some(r) = any.downcast_ref::<RuleBasedStateProvider>() {
        r.get_optional_state(level, random, pos)
    } else {
        panic!(
            "Trying to apply block state provider type '{}' with no registered behavior (#181 codegen)",
            provider.type_id().location
        );
    }
}

/// `BlockStateProvider.simple(BlockState)` — a `SimpleStateProvider` over a
/// fixed state.
pub fn simple(state: BlockState) -> SimpleStateProvider {
    SimpleStateProvider::new(state)
}

/// `BlockStateProvider.simple(Block)` — a `SimpleStateProvider` over a block's
/// default state (`Block.defaultBlockState()` is `BlockState.of(block)`).
pub fn simple_block(block: BlockId) -> SimpleStateProvider {
    SimpleStateProvider::new(BlockState::of(block))
}

/// `BlockStateProvider.CODEC` — the recursive by-name dispatch codec, as the
/// ops-generic `block_state_provider_codec::<Ops>()` factory.
pub fn block_state_provider_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>> {
    codec::recursive("BlockStateProvider".to_string(), Arc::new(create_dispatch))
}

/// The non-recursive dispatch body given the `RecursiveSelf` (`top`): the
/// `"type"` by-name dispatch. Every provider that recurses into
/// `BlockStateProvider.CODEC` (`RuleBasedStateProvider`,
/// `RandomizedIntStateProvider`) receives `top` as the child-element codec so
/// the whole nested graph shares this single recursive codec.
/// `WeightedStateProvider` also receives `top` (its map codec takes the
/// `RecursiveSelf` by name) but ignores it — its `entries` hold `BlockState`s.
fn create_dispatch<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>> {
    // `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.byNameCodec().dispatch(...)`.
    map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        BlockStateProviderTypeId,
        Arc<dyn ErasedBlockStateProvider>,
        Ops,
    >(
        "type",
        block_state_provider_type_by_name_codec::<Ops>(),
        Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
            DataResult::success(ErasedBlockStateProvider::type_id(&**p))
        }),
        codec_for_type(top),
    ))
}

/// `BlockStateProviderType::codec` — resolve a `BlockStateProviderTypeId` to
/// its `MapCodec<Arc<dyn ErasedBlockStateProvider>>` (the dispatch's `codec`
/// function).
fn codec_for_type<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> key_dispatch_codec::CodecFn<BlockStateProviderTypeId, Arc<dyn ErasedBlockStateProvider>, Ops> {
    Arc::new(move |k: &BlockStateProviderTypeId| {
        if *k == BlockStateProviderTypes::SIMPLE_STATE_PROVIDER {
            DataResult::success(erase_map_codec::<SimpleStateProvider, Ops>(
                crate::levelgen::feature::stateproviders::simple_state_provider::simple_state_provider_map_codec::<
                    Ops,
                >(),
                Arc::new(|s: &SimpleStateProvider| {
                    Arc::new(*s) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    *p.as_any()
                        .downcast_ref::<SimpleStateProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-simple value"))
                }),
            ))
        } else if *k == BlockStateProviderTypes::WEIGHTED_STATE_PROVIDER {
            DataResult::success(erase_map_codec::<WeightedStateProvider, Ops>(
                crate::levelgen::feature::stateproviders::weighted_state_provider::weighted_state_provider_map_codec::<
                    Ops,
                >(top.clone()),
                Arc::new(|w: &WeightedStateProvider| {
                    Arc::new(w.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<WeightedStateProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-weighted value"))
                        .clone()
                }),
            ))
        } else if *k == BlockStateProviderTypes::NOISE_THRESHOLD_PROVIDER {
            DataResult::success(erase_map_codec::<NoiseThresholdProvider, Ops>(
                crate::levelgen::feature::stateproviders::noise_threshold_provider::noise_threshold_provider_map_codec::<
                    Ops,
                >(),
                Arc::new(|n: &NoiseThresholdProvider| {
                    Arc::new(n.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<NoiseThresholdProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-noise-threshold value"))
                        .clone()
                }),
            ))
        } else if *k == BlockStateProviderTypes::NOISE_PROVIDER {
            DataResult::success(erase_map_codec::<NoiseProvider, Ops>(
                crate::levelgen::feature::stateproviders::noise_provider::noise_provider_map_codec::<
                    Ops,
                >(),
                Arc::new(|n: &NoiseProvider| {
                    Arc::new(n.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<NoiseProvider>()
                        .unwrap_or_else(|| {
                            panic!("block-state-provider dispatch produced a non-noise value")
                        })
                        .clone()
                }),
            ))
        } else if *k == BlockStateProviderTypes::DUAL_NOISE_PROVIDER {
            DataResult::success(erase_map_codec::<DualNoiseProvider, Ops>(
                crate::levelgen::feature::stateproviders::dual_noise_provider::dual_noise_provider_map_codec::<
                    Ops,
                >(),
                Arc::new(|d: &DualNoiseProvider| {
                    Arc::new(d.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<DualNoiseProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-dual-noise value"))
                        .clone()
                }),
            ))
        } else if *k == BlockStateProviderTypes::ROTATED_BLOCK_PROVIDER {
            DataResult::success(erase_map_codec::<RotatedBlockProvider, Ops>(
                crate::levelgen::feature::stateproviders::rotated_block_provider::rotated_block_provider_map_codec::<
                    Ops,
                >(),
                Arc::new(|r: &RotatedBlockProvider| {
                    Arc::new(r.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<RotatedBlockProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-rotated value"))
                        .clone()
                }),
            ))
        } else if *k == BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER {
            DataResult::success(erase_map_codec::<RandomizedIntStateProvider, Ops>(
                crate::levelgen::feature::stateproviders::randomized_int_state_provider::randomized_int_state_provider_map_codec::<
                    Ops,
                >(top.clone()),
                Arc::new(|r: &RandomizedIntStateProvider| {
                    Arc::new(r.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<RandomizedIntStateProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-randomized-int value"))
                        .clone()
                }),
            ))
        } else if *k == BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER {
            DataResult::success(erase_map_codec::<RuleBasedStateProvider, Ops>(
                crate::levelgen::feature::stateproviders::rule_based_state_provider::rule_based_state_provider_map_codec::<
                    Ops,
                >(top.clone()),
                Arc::new(|r: &RuleBasedStateProvider| {
                    Arc::new(r.clone()) as Arc<dyn ErasedBlockStateProvider>
                }),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    p.as_any()
                        .downcast_ref::<RuleBasedStateProvider>()
                        .unwrap_or_else(|| panic!("block-state-provider dispatch produced a non-rule-based value"))
                        .clone()
                }),
            ))
        } else {
            DataResult::error(format!(
                "Block state provider type '{}' is not ported",
                k.location
            ))
        }
    })
}

/// Lift a concrete provider's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedBlockStateProvider>>` — Java's
/// `MapCodec<? extends BlockStateProvider>` variance, via xmap (the same lift
/// `BlockPredicate`'s `erase_map_codec` performs). The wrap side boxes a clone
/// of the concrete provider; the unwrap side downcasts the erased provider
/// through `as_any`.
#[allow(clippy::type_complexity)]
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> Arc<dyn ErasedBlockStateProvider> + Send + Sync>,
    unwrap: Arc<dyn Fn(&Arc<dyn ErasedBlockStateProvider>) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedBlockStateProvider>, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.byNameCodec()` over the type id
/// — `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key()
/// .identifier())`.
///
/// The unknown-key error reproduces Paper's exactly: `"Unknown registry key in "
/// + this.key() + ": " + name` where `this.key()` is
/// `Registries.BLOCK_STATE_PROVIDER_TYPE` (`createRegistryKey(
/// "worldgen/block_state_provider_type")` = `ResourceKey.createRegistryKey(
/// Identifier.withDefaultNamespace(...))`, toString `"ResourceKey[
/// minecraft:root / minecraft:worldgen/block_state_provider_type]"`).
#[allow(clippy::doc_lazy_continuation)]
pub fn block_state_provider_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<BlockStateProviderTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, BlockStateProviderTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match block_state_provider_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/block_state_provider_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &BlockStateProviderTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;

    /// The test ops: a `RegistryOps` over JSON — the only ops that implement
    /// `RegistryOpsLookup` (the `RuleBasedStateProvider.Rule`'s embedded
    /// `BlockPredicate.CODEC` requires it).
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn empty_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    /// A concrete provider whose only identity is the type id — the erased
    /// carrier must carry it through the blanket `ErasedBlockStateProvider` impl.
    #[derive(Debug, Clone)]
    struct IdentityProvider(BlockStateProviderTypeId);

    impl BlockStateProvider for IdentityProvider {
        fn get_state<R: RandomSource>(
            &self,
            _level: &dyn WorldGenLevel,
            _random: &mut R,
            _pos: &BlockPos,
        ) -> BlockState {
            BlockState::of(BlockId::from_id(0))
        }

        fn type_id(&self) -> BlockStateProviderTypeId {
            self.0.clone()
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn erased_carrier_forwards_the_type_identity() {
        // `BlockStateProviderType.RULE_BASED_STATE_PROVIDER` is insertion index
        // 7 in `BlockStateProviderType.java`'s registration order.
        let provider = IdentityProvider(BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER);
        let erased: &dyn ErasedBlockStateProvider = &provider;
        assert_eq!(
            erased.type_id(),
            BlockStateProviderTypes::RULE_BASED_STATE_PROVIDER
        );
    }

    #[test]
    fn by_name_codec_resolves_known_and_unknown() {
        let codec = block_state_provider_type_by_name_codec::<JsonOps>();
        let input = JsonOps::INSTANCE.create_string("minecraft:noise_provider".to_string());
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, BlockStateProviderTypes::NOISE_PROVIDER);
        let unknown = JsonOps::INSTANCE.create_string("minecraft:not_a_type".to_string());
        let result = codec.parse(&JsonOps::INSTANCE, &unknown);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/block_state_provider_type]: minecraft:not_a_type"
            ),
            "got: {msg}"
        );
    }

    #[test]
    fn dispatch_unknown_type_errors_like_by_name_codec() {
        let codec = block_state_provider_codec::<TestOps>();
        let input = serde_json::json!({"type": "minecraft:not_a_type", "state": "minecraft:air"});
        let result = codec.parse(&empty_ops(), &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains(
                "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/block_state_provider_type]: minecraft:not_a_type"
            ),
            "got: {msg}"
        );
    }

    #[test]
    fn dispatch_round_trips_each_simple_type() {
        // The simple/weighted/rotated dispatch branches round-trip through the
        // top-level `BlockStateProvider.CODEC`'s `"type"`-key dispatch.
        let codec = block_state_provider_codec::<TestOps>();
        let input = serde_json::json!({
            "type": "minecraft:simple_state_provider",
            "state": {"Name": "minecraft:air"}
        });
        let decoded_result = codec.parse(&empty_ops(), &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            ErasedBlockStateProvider::type_id(decoded.as_ref()),
            BlockStateProviderTypes::SIMPLE_STATE_PROVIDER
        );
        let encoded = codec
            .encode_start(&empty_ops(), decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            serde_json::json!({
                "state": {"Name": "minecraft:air"},
                "type": "minecraft:simple_state_provider"
            })
        );
        let re_result = codec.parse(&empty_ops(), &encoded);
        let re_decoded = re_result.result().expect("re-decode should succeed");
        let a = decoded
            .as_any()
            .downcast_ref::<SimpleStateProvider>()
            .unwrap();
        let b = re_decoded
            .as_any()
            .downcast_ref::<SimpleStateProvider>()
            .unwrap();
        assert_eq!(a.state(), b.state());
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

    #[test]
    fn simple_provider_get_state_is_constant() {
        let provider = simple_block(rivet_registry::generated::blocks::BlockId::from_id(0));
        let mut random = LegacyRandomSource::new(1);
        let level = TestLevel;
        let pos = BlockPos::new(1, 2, 3);
        let state = provider.get_state(&level, &mut random, &pos);
        assert_eq!(
            state,
            BlockState::of(rivet_registry::generated::blocks::BlockId::from_id(0))
        );
    }

    #[test]
    fn dispatch_applies_a_simple_state_provider() {
        // The closed dispatch downcasts the erased carrier to the concrete
        // provider and calls its `get_state` (Java's virtual `getState`).
        let provider = simple_block(rivet_registry::generated::blocks::BlockId::from_id(0));
        let erased: &dyn ErasedBlockStateProvider = &provider;
        let level = TestLevel;
        let mut random = LegacyRandomSource::new(1);
        let pos = BlockPos::new(1, 2, 3);
        let state = block_state_provider_get_state(erased, &level, &mut random, &pos);
        assert_eq!(
            state,
            BlockState::of(rivet_registry::generated::blocks::BlockId::from_id(0))
        );
    }

    #[test]
    #[should_panic(
        expected = "Trying to apply block state provider type 'minecraft:simple_state_provider'"
    )]
    fn dispatch_panics_for_an_unknown_provider_type() {
        // An erased carrier that is not one of the eight concrete providers
        // (the `IdentityProvider` test double) falls through the closed match
        // to the panic arm — the analogue of Java's `Registry.getValueOrThrow`.
        let provider = IdentityProvider(BlockStateProviderTypes::SIMPLE_STATE_PROVIDER);
        let erased: &dyn ErasedBlockStateProvider = &provider;
        let level = TestLevel;
        let mut random = LegacyRandomSource::new(1);
        let pos = BlockPos::new(0, 0, 0);
        let _ = block_state_provider_get_state(erased, &level, &mut random, &pos);
    }
}

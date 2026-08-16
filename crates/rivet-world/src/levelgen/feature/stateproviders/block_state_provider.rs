//! STUB(mc.world.level.levelgen.feature.stateproviders) — cross-unit stub for
//! `net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider`
//! (abstract class, 26.2).
//!
//! The full port lives on `origin/main` (PR #559, commit `ba4096f8`):
//! `BlockStateProvider` is the generic behavior contract (`get_state` is
//! generic over the random source — `RandomSource` is `Sized`, not
//! object-safe), [`ErasedBlockStateProvider`] is the object-safe carrier the
//! codec graph stores each provider as, and the `CODEC` is the recursive
//! `"type"` by-name dispatch over `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE`.
//! This stub mirrors that exact surface — module path, names, codec signature,
//! `simple(BlockState)` factory — restricted to the single provider type the
//! `DiskConfiguration` unit consumes ([`SimpleStateProvider`]).
//!
//! Merge-state note: this worktree's last `origin/main` merge (`827eaaa1`,
//! second parent `5214c6ce` = PR #555) predates PR #559, so the full port is
//! *not* yet in this tree. The next `origin/main` merge is therefore not a
//! clean first-time application of the full port: it must resolve the
//! `stateproviders` overlap by keeping `origin/main`'s files (the
//! `BlockStateProviderTypeId`/`BlockStateProviderTypes` split into
//! `block_state_provider_type.rs`, `SimpleStateProvider` in
//! `simple_state_provider.rs`, plus `codec_helpers.rs` and the seven other
//! provider modules) and deleting this stub. The `DiskConfiguration` unit
//! consumes only `block_state_provider_codec`, `ErasedBlockStateProvider`, and
//! `simple`/`SimpleStateProvider`, which the full port provides with identical
//! signatures, so the disk unit needs no edits at that merge.
//!
//! The stub's dispatch reproduces Java's by-name lookup for its one entry:
//! `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.byNameCodec()` resolves
//! `minecraft:simple_state_provider` to `BlockStateProviderType.SIMPLE_STATE_PROVIDER`,
//! whose map codec is `BlockState.CODEC.fieldOf("state").xmap(...)`. An
//! unknown `"type"` key errors with the merged port's exact message (`Unknown
//! registry key in ResourceKey[minecraft:root /
//! minecraft:worldgen/block_state_provider_type]: {name}`).

use crate::level::WorldGenLevel;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::core::BlockPos;
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
/// STUB(mc.world.level.levelgen.feature.stateproviders): only the surface the
/// `DiskConfiguration` unit consumes is here — [`simple`] and the
/// [`ErasedBlockStateProvider`] carrier. The `get_state` behavior is not
/// implemented (it defers with the full port); every call through it panics
/// rather than fabricating a state (the established worldgen-seam pattern, the
/// same as the `#181`/`#180` dispatch stubs).
pub trait BlockStateProvider: Any + Debug + Send + Sync + 'static {
    /// `getState(WorldGenLevel, RandomSource, BlockPos)` — the provider's
    /// per-position block state.
    ///
    /// STUB: the behavior defers with the full port; this stub panics.
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

/// `net.minecraft.core.Registry` element identity for
/// `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE` — the per-instance `u32` id
/// (element id == holder id == insertion index, OWNERSHIP.md §Registries) plus
/// the registry-key location of the type's registration (`register(
/// Registries.BLOCK_STATE_PROVIDER_TYPE, "simple_state_provider", ...)` in
/// declaration order).
///
/// Identity-semantic (not `Copy`) — the same split as `BlockPredicateTypeId`
/// and the height-provider type ids; the merged port (PR #559) carries this
/// deliberately so the identity is only ever held by reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockStateProviderTypeId {
    /// The per-instance `u32` identity (insertion index in the provider-type
    /// registry).
    pub id: u32,
    /// The registry-key location of the type's registration.
    pub location: &'static str,
}

impl BlockStateProviderTypeId {
    /// `new BlockStateProviderTypeId(u32, ResourceLocation)`.
    pub const fn new(id: u32, location: &'static str) -> BlockStateProviderTypeId {
        BlockStateProviderTypeId { id, location }
    }
}

/// `net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProviderType.SIMPLE_STATE_PROVIDER`
/// — the first declaration-order entry, the `minecraft:simple_state_provider`
/// registration (id 0).
///
/// STUB: the other seven entries defer with the owning unit.
pub const SIMPLE_STATE_PROVIDER: BlockStateProviderTypeId =
    BlockStateProviderTypeId::new(0, "minecraft:simple_state_provider");

/// `BlockStateProvider.simple(BlockState)` — a `SimpleStateProvider` over a
/// fixed state.
pub fn simple(state: BlockState) -> SimpleStateProvider {
    SimpleStateProvider::new(state)
}

/// `net.minecraft.world.level.levelgen.feature.stateproviders.SimpleStateProvider`
/// — a provider that always returns a fixed `BlockState`.
///
/// STUB: only the codec surface the `DiskConfiguration` unit consumes; the
/// `get_state` behavior defers with the owning unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleStateProvider {
    /// `this.state`.
    state: BlockState,
}

impl SimpleStateProvider {
    /// `SimpleStateProvider(BlockState)` — the protected constructor, exposed
    /// for `BlockStateProvider.simple(BlockState)`.
    pub(crate) fn new(state: BlockState) -> SimpleStateProvider {
        SimpleStateProvider { state }
    }

    /// `this.state`.
    pub fn state(&self) -> BlockState {
        self.state
    }
}

impl BlockStateProvider for SimpleStateProvider {
    fn get_state<R: RandomSource>(
        &self,
        _level: &dyn WorldGenLevel,
        _random: &mut R,
        _pos: &BlockPos,
    ) -> BlockState {
        // STUB(mc.world.level.levelgen.feature.stateproviders): the real
        // provider simply returns `self.state`; the behavior defers with the
        // full port.
        panic!("BlockStateProvider.get_state not ported (#181)");
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        SIMPLE_STATE_PROVIDER
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `BlockStateProvider.CODEC` — the recursive by-name dispatch codec, as the
/// ops-generic `block_state_provider_codec::<Ops>()` factory.
///
/// STUB: this is the dispatch over the single stub-registered provider type,
/// mirroring the merged port's shape (which dispatches all eight types through
/// the same `"type"`-key dispatch and the same `codec::recursive` graph).
pub fn block_state_provider_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>> {
    codec::recursive("BlockStateProvider".to_string(), Arc::new(create_dispatch))
}

/// The non-recursive dispatch body given the `RecursiveSelf` (`top`): the
/// `"type"` by-name dispatch. The stub has one registered type; the full port
/// threads `top` into the recursive providers (`RuleBasedStateProvider`,
/// `RandomizedIntStateProvider`), which defer with the owning unit.
fn create_dispatch<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    _top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>> {
    let dispatch = key_dispatch_codec::dispatch_map::<
        BlockStateProviderTypeId,
        Arc<dyn ErasedBlockStateProvider>,
        Ops,
    >(
        "type",
        block_state_provider_type_by_name_codec::<Ops>(),
        Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
            DataResult::success(ErasedBlockStateProvider::type_id(&**p))
        }),
        codec_for_type(),
    );
    map_codec::codec_of(dispatch)
}

/// `BlockStateProviderType::codec` — resolve a `BlockStateProviderTypeId` to
/// its `MapCodec<Arc<dyn ErasedBlockStateProvider>>` (the dispatch's `codec`
/// function).
///
/// STUB: the stub resolves its one registered type; the other seven defer with
/// the owning unit.
fn codec_for_type<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> key_dispatch_codec::CodecFn<BlockStateProviderTypeId, Arc<dyn ErasedBlockStateProvider>, Ops> {
    Arc::new(move |k: &BlockStateProviderTypeId| {
        if *k == SIMPLE_STATE_PROVIDER {
            DataResult::success(erase_map_codec::<SimpleStateProvider, Ops>(
                simple_state_provider_map_codec::<Ops>(),
                Arc::new(
                    |s: &SimpleStateProvider| -> Arc<dyn ErasedBlockStateProvider> { Arc::new(*s) },
                ),
                Arc::new(|p: &Arc<dyn ErasedBlockStateProvider>| {
                    let any = ErasedBlockStateProvider::as_any(&**p);
                    match any.downcast_ref::<SimpleStateProvider>() {
                        Some(s) => *s,
                        None => {
                            panic!(
                                "Trying to encode block state provider type '{}' as simple_state_provider (#181)",
                                ErasedBlockStateProvider::type_id(&**p).location
                            );
                        }
                    }
                }),
            ))
        } else {
            // STUB(mc.world.level.levelgen.feature.stateproviders): Java's
            // `Registry.getValueOrThrow` (behind the dispatch's encode lookup)
            // throws a hard exception for a key the registry does not hold.
            // This branch is unreachable in the stub's lifetime — the by-name
            // codec only ever resolves `minecraft:simple_state_provider`, so a
            // non-SIMPLE id can neither be decoded (the by-name lookup errors
            // first with the exact unknown-key message) nor encoded (the only
            // constructible provider reports `SIMPLE_STATE_PROVIDER`). It
            // panics rather than fabricating a codec DataResult error, mirroring
            // Java's exception the same way the `get_state` seam does; the full
            // port (PR #559) replaces the whole dispatch. The panic text is
            // inert — it cannot be observed in the stub's lifetime — so it is
            // not live parity behavior; it just mirrors the merged port's
            // message shape.
            panic!(
                "Block state provider type '{}' is not ported (#181)",
                k.location
            );
        }
    })
}

/// `SimpleStateProvider.CODEC` — `BlockState.CODEC.fieldOf("state").xmap(...)`,
/// as the ops-generic `simple_state_provider_map_codec::<Ops>()` factory.
pub fn simple_state_provider_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<SimpleStateProvider, Ops>> {
    map_codec::xmap(
        codec::field_of(block_state_codec::<Ops>(), "state".to_string()),
        Arc::new(|s: &BlockState| SimpleStateProvider::new(*s)),
        Arc::new(|p: &SimpleStateProvider| p.state),
    )
}

/// Erase a `MapCodec<A>` to `MapCodec<Arc<dyn ErasedBlockStateProvider>>` via
/// the downcast-encode/erase-decode wrappers.
#[allow(clippy::type_complexity)]
fn erase_map_codec<A: 'static, Ops: DynamicOps + 'static>(
    codec: Arc<dyn MapCodec<A, Ops>>,
    to_erased: Arc<dyn Fn(&A) -> Arc<dyn ErasedBlockStateProvider> + Send + Sync>,
    from_erased: Arc<dyn Fn(&Arc<dyn ErasedBlockStateProvider>) -> A + Send + Sync>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedBlockStateProvider>, Ops>> {
    map_codec::xmap(codec, to_erased, from_erased)
}

/// `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.byNameCodec()` — the by-name
/// codec over the registry-key location.
///
/// STUB: the stub resolves its one registered key (`minecraft:simple_state_provider`);
/// the merged port's `BlockStateProviderType` `by_name` lookup (all eight
/// entries) replaces this when the owning unit lands. The unknown-key error
/// reproduces Paper's exactly: `"Unknown registry key in " + this.key() + ": " +
/// name` where `this.key()` is `Registries.BLOCK_STATE_PROVIDER_TYPE`
/// (`createRegistryKey("worldgen/block_state_provider_type")` =
/// `ResourceKey.createRegistryKey(...)`, toString `"ResourceKey[
/// minecraft:root / minecraft:worldgen/block_state_provider_type]"`).
fn block_state_provider_type_by_name_codec<Ops: DynamicOps + 'static>()
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

/// `BlockStateProviderType.byName(String)` — the registry-key location to its
/// type id.
///
/// STUB: the stub resolves its one registered key (`minecraft:simple_state_provider`);
/// the merged port's `BlockStateProviderType` `by_name` lookup (all eight
/// entries) replaces this when the owning unit lands.
pub fn block_state_provider_type_by_name(name: &str) -> Option<BlockStateProviderTypeId> {
    if name == SIMPLE_STATE_PROVIDER.location {
        Some(SIMPLE_STATE_PROVIDER)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    #[test]
    fn codec_round_trips_simple_provider() {
        let codec = block_state_provider_codec::<TestOps>();
        let ops = ops();
        let provider: Arc<dyn ErasedBlockStateProvider> = Arc::new(simple(BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap(),
        )));
        let encoded = codec
            .encode_start(&ops, &provider)
            .result()
            .expect("encode should succeed")
            .clone();
        // `key_dispatch_codec`'s MapEncoder reproduces Java's KeyDispatchCodec
        // 'Encode key AFTER value' ordering, so the state fields emit before the
        // `"type"` key — the order Paper produces (JsonOps LinkedTreeMap).
        assert_eq!(
            encoded,
            json!({"state": {"Name": "minecraft:stone"}, "type": "minecraft:simple_state_provider"})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            ErasedBlockStateProvider::type_id(&*decoded),
            SIMPLE_STATE_PROVIDER
        );
        assert_eq!(
            decoded
                .as_any()
                .downcast_ref::<SimpleStateProvider>()
                .unwrap()
                .state(),
            BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone").unwrap()
            )
        );
    }

    #[test]
    fn codec_rejects_unknown_type() {
        let codec = block_state_provider_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:no_such_provider", "state": {"Name": "minecraft:stone"}}),
        );
        assert!(result.is_error());
    }

    #[test]
    fn codec_requires_the_type_key() {
        let codec = block_state_provider_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(&ops, &json!({"state": {"Name": "minecraft:stone"}}));
        assert!(result.is_error());
    }
}

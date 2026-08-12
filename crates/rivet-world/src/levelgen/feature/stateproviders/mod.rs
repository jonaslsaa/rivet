//! `net.minecraft.world.level.levelgen.feature.stateproviders` (12 files,
//! 26.2) — the block-state-provider framework.
//!
//! Java is an abstract `BlockStateProvider` base with the dispatch codec
//! `CODEC = BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE.byNameCodec().dispatch(
//! BlockStateProvider::type, BlockStateProviderType::codec)` and eight concrete
//! providers registered into `BuiltInRegistries.BLOCKSTATE_PROVIDER_TYPE` in
//! declaration order. The Rust port follows the `BlockPredicate`/`HeightProvider`
//! split: [`block_state_provider::BlockStateProvider`] is the generic behavior
//! contract (`get_state` is generic over the random source — `RandomSource` is
//! `Sized`, not object-safe), [`block_state_provider::ErasedBlockStateProvider`]
//! is the object-safe carrier the codec graph stores each provider as, and
//! [`block_state_provider::block_state_provider_get_state`] is the closed
//! dispatch that downcasts the erased carrier and calls the concrete
//! `get_state` (all eight providers are owned here, so the dispatch is a
//! complete match — the analogue of the `#181` codegen tables for the *other*
//! frameworks).
//!
//! ## Codec shape
//!
//! `block_state_provider_codec::<Ops>()` is a `codec::recursive` graph whose
//! single `RecursiveSelf` threads into the recursive fields
//! (`RandomizedIntStateProvider`'s source, `RuleBasedStateProvider`'s
//! fallback/rules) so arbitrary nesting round-trips. (`WeightedStateProvider`
//! is *not* recursive: its `entries` hold `BlockState`s, so its map codec
//! ignores the `RecursiveSelf`.)
//! `Ops` must implement `RegistryOpsLookup`: `RuleBasedStateProvider.Rule`
//! embeds `BlockPredicate.CODEC`, whose `matching_blocks`/`matching_fluids`/
//! `matching_biomes` fields resolve the registry through the ops.
//!
//! The Java-exact `POSITIVE_FLOAT` / `Codec.floatRange` validation codecs
//! (`positive_float` / `float_range` in [`noise_based_state_provider`]) are
//! kept local to this unit: rivet-serialization's `float_range` diverges from
//! Java's message, and the unit brief forbids editing rivet-serialization. The
//! `InclusiveRange` value used by `DualNoiseProvider`'s `variety` field is a
//! cross-unit stub in `rivet-util` (owned by `mc.util`), ported only to the
//! surface this unit consumes.
//!
//! The `#399` world-access seam: `RuleBasedStateProvider.getState` falls back
//! to `level.get_block_state(pos)` when no rule matches and no fallback is
//! present. That read is not implemented yet (RivetTodo #399) and fails
//! explicitly — the provider never fabricates a state.

pub mod block_state_provider;
pub mod block_state_provider_type;
pub mod codec_helpers;
pub mod dual_noise_provider;
pub mod noise_based_state_provider;
pub mod noise_provider;
pub mod noise_threshold_provider;
pub mod randomized_int_state_provider;
pub mod rotated_block_provider;
pub mod rule_based_state_provider;
pub mod simple_state_provider;
pub mod weighted_state_provider;

pub use block_state_provider::{
    BlockStateProvider, ErasedBlockStateProvider, block_state_provider_codec,
    block_state_provider_get_state, simple, simple_block,
};
pub use block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes, block_state_provider_type_by_name,
};
pub use dual_noise_provider::DualNoiseProvider;
pub use noise_provider::NoiseProvider;
pub use noise_threshold_provider::NoiseThresholdProvider;
pub use randomized_int_state_provider::RandomizedIntStateProvider;
pub use rotated_block_provider::RotatedBlockProvider;
pub use rule_based_state_provider::RuleBasedStateProvider;
pub use simple_state_provider::SimpleStateProvider;
pub use weighted_state_provider::WeightedStateProvider;

//! `net.minecraft.world.level.levelgen.feature` — feature worldgen.
//!
//! Owned by the `mc.world.level.levelgen.feature.core` manifest unit (26.2):
//! `Feature.java`, `ConfiguredFeature.java`, `FeaturePlaceContext.java`,
//! `FeatureCountTracker.java`, `package-info.java`. (`WeightedPlacedFeature.java`
//! is owned by the `.feature.selector` manifest unit — see MANIFEST.tsv — and
//! will be ported there when that unit lands.)
//! The `configurations` package slice (the `FeatureConfiguration` trait and its
//! first three value types) is owned by the `.configurations.core` unit and
//! lives in the `configurations` submodule.
//!
//! ## Feature identity and the erased wildcard
//!
//! Java `Feature<FC>` is an object with identity: it is registered by-reference
//! into `BuiltInRegistries.FEATURE`, and `ConfiguredFeature`'s dispatch codecs
//! dispatch on that identity. The Rust port keeps the same value/id model:
//! `ConfiguredFeature<FC>` pairs a `FeatureId` (the registry-held identity
//! handle) with its config, and the erased wildcard `ConfiguredFeature<?, ?>`
//! — used by `getSubFeatures`, `PlacedFeature`'s holder, and the codecs — is
//! `ConfiguredFeatureErased` (the feature id plus the config as a
//! `dyn FeatureConfiguration`).
//!
//! The concrete feature structs (`TreeFeature`, `NoOpFeature`, …) are owned by
//! their own manifest units and implement the generic `FeatureBehavior<FC>`
//! contract. Because `RandomSource` is `Sized` (not object-safe), a feature's
//! `place` is generic over the random source, so the feature registry cannot
//! hold `dyn FeatureBehavior` — the `#181` hub (its registration table is
//! emitted by `rivet-codegen` per the manifest note) is a monomorphized dispatch
//! that downcasts the erased
//! config and calls the concrete feature's `place_with_config` (the faithful
//! mapping of `Feature.place(FC, WorldGenLevel, ChunkGenerator, RandomSource,
//! BlockPos)`), preserving the `ensureCanWrite` gate that Java's
//! `ConfiguredFeature.place` path applies. This core unit declares
//! that dispatch as the `feature_place` stub and does NOT hand-port
//! `Feature.java`'s registration table. `Feature.register`'s ~60 entries are
//! generated content.
//!
//! The configured/registry codecs (`DIRECT_CODEC` by-name dispatch,
//! `RegistryFileCodec`/`RegistryCodecs.homogeneousList`, the per-feature
//! `configuredCodec`) are deferred to issue #126 with the by-name codec
//! surface.
//!
//! RivetTodo(#228): `Feature.java`'s protected/static helper surface is not
//! ported — `setBlock(LevelWriter, BlockPos, BlockState)` +
//! `safeSetBlock(WorldGenLevel, BlockPos, BlockState, Predicate)` (write through
//! `LevelWriter` with `Block.UPDATE_ALL`/`UPDATE_CLIENTS`; the
//! `BlockState`/`Block` flag constants belong to the block-state worldgen
//! slice), `isReplaceable(TagKey)`, `checkNeighbors(Function<BlockPos,BlockState>,
//! BlockPos, Predicate)`, `isAdjacentToAir(Function<BlockPos,BlockState>, BlockPos)`,
//! `markAboveForPostProcessing(WorldGenLevel, BlockPos)`, and `configuredCodec()`
//! (deferred with the `#126` codec surface). Concrete feature units will reach
//! for these at `context.level().setBlock(...)` / `checkNeighbors(...)`.

pub mod configurations;

mod feature_count_tracker;
mod feature_place_context;

// The `net.minecraft.world.level.levelgen.feature.stateproviders` value layer
// — STUB(mc.world.level.levelgen.feature.stateproviders): the `BlockStateProvider`
// dispatch surface the `.configurations.disk` unit consumes. The full port
// lives on `origin/main` (PR #559) and replaces this stub when merged.
pub mod stateproviders;

// The `net.minecraft.world.level.levelgen.feature.featuresize` value layer
// (issue #391) — the `FeatureSize` hierarchy and its declaration-order codec
// dispatch. Owned by the `mc.world.level.levelgen.feature.featuresize` unit.
pub mod featuresize;

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::Holder;
use rivet_registry::core::BlockPos;
use rivet_util::RandomSource;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

pub use feature_count_tracker::FeatureCountTracker;
pub use feature_place_context::FeaturePlaceContext;

/// `net.minecraft.world.level.levelgen.feature.ConfiguredFeature<FC, F>`
/// (record, 26.2) — a feature paired with its configuration.
/// The feature generic `F` is erased to its registry-held `FeatureId` (see the
/// module doc for the identity split).
///
/// Generic in Java over the configuration (`FC`) and the feature (`F`); the
/// Rust port keeps the configuration generic and stores the feature's
/// registry-held identity (`FeatureId`). Placement dispatches through
/// `feature_place`, the `#181` codegen hub.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredFeature<FC: FeatureConfiguration> {
    /// `ConfiguredFeature.feature` — the feature's registry-held identity.
    pub feature: FeatureId,
    /// `ConfiguredFeature.config` — the feature configuration.
    pub config: FC,
}

impl<FC: FeatureConfiguration> ConfiguredFeature<FC> {
    /// `new ConfiguredFeature(F feature, FC config)` — the record constructor.
    pub fn new(feature: FeatureId, config: FC) -> Self {
        ConfiguredFeature { feature, config }
    }

    /// `ConfiguredFeature.place(WorldGenLevel, ChunkGenerator, RandomSource,
    /// BlockPos)` — `this.feature.place(this.config, ...)`, dispatched through
    /// the `#181` feature-registry hub.
    pub fn place<R: RandomSource>(
        &self,
        level: &mut dyn WorldGenLevel,
        chunk_generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        feature_place(
            self.feature.clone(),
            &self.config,
            level,
            chunk_generator,
            random,
            origin,
        )
    }

    /// `ConfiguredFeature.getSubFeatures()` — `config.getSubFeatures()`, the
    /// lazy sub-feature iterator (Java's lazy `Stream`).
    pub fn get_sub_features(
        &self,
    ) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        self.config.get_sub_features()
    }

    /// Erase to the wildcard `ConfiguredFeature<?, ?>` — the form stored in
    /// `PlacedFeature` holders and the `LIST_CODEC` holder sets.
    pub fn into_erased(self) -> ConfiguredFeatureErased {
        ConfiguredFeatureErased {
            feature: self.feature,
            config: Arc::new(self.config),
        }
    }
}

/// `toString()` — `"Configured: " + this.feature + ": " + this.config`.
///
/// Java renders the feature's and config's `toString` (for the erased config
/// the Rust port uses the `Debug` form, the closest value-string stand-in).
impl<FC: FeatureConfiguration> fmt::Display for ConfiguredFeature<FC> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Configured: {:?}: {:?}", self.feature, self.config)
    }
}

/// Java's `ConfiguredFeature<?, ?>` wildcard, erased. Java erases both the
/// config and the feature to their bounds; the Rust port erases the feature to
/// its `FeatureId` identity and the config to a `dyn FeatureConfiguration`.
/// The concrete config type is recovered by the `#181` dispatch, which
/// downcasts before calling the concrete feature's `place_with_config`.
#[derive(Debug, Clone)]
pub struct ConfiguredFeatureErased {
    /// The feature's registry-held identity.
    pub feature: FeatureId,
    /// The configuration, erased to the `FeatureConfiguration` surface.
    pub config: Arc<dyn FeatureConfiguration>,
}

impl ConfiguredFeatureErased {
    /// `ConfiguredFeature<?, ?>.place(...)` — dispatched through the `#181`
    /// feature-registry hub (the config is downcast there).
    pub fn place<R: RandomSource>(
        &self,
        level: &mut dyn WorldGenLevel,
        chunk_generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        feature_place(
            self.feature.clone(),
            self.config.as_ref(),
            level,
            chunk_generator,
            random,
            origin,
        )
    }

    /// `ConfiguredFeature.getSubFeatures()` — `config.getSubFeatures()`, the
    /// lazy sub-feature iterator (Java's lazy `Stream`).
    pub fn get_sub_features(
        &self,
    ) -> Box<dyn Iterator<Item = Holder<ConfiguredFeatureErased>> + '_> {
        self.config.get_sub_features()
    }
}

/// `ConfiguredFeature<?, ?>.toString()` — the same record `toString` as the
/// generic form (`"Configured: feature: config"`).
impl fmt::Display for ConfiguredFeatureErased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Configured: {:?}: {:?}", self.feature, self.config)
    }
}

/// `net.minecraft.world.level.levelgen.feature.Feature<FC extends
/// FeatureConfiguration>` — the abstract feature base's behavior contract.
///
/// Java `Feature<FC>` is an object whose identity is registered in
/// `BuiltInRegistries.FEATURE`; the Rust port splits it into:
/// - `FeatureId` — the identity handle (Java's object identity, a
///   `BuiltInRegistries.FEATURE` holder), stored by `ConfiguredFeature`.
/// - `FeatureBehavior<FC>` — the overridable behavior (Java's virtual
///   methods), the contract concrete feature structs implement.
///
/// The trait is generic over the config type and its `place` is generic over
/// the random source (`RandomSource` is `Sized`, not object-safe), so it is
/// *not* object-safe: the `#181` feature registry dispatches through the
/// monomorphized `feature_place` match, which downcasts the erased config and
/// calls the concrete feature's `place_with_config`.
pub trait FeatureBehavior<FC: FeatureConfiguration>: Debug + Send + Sync + 'static {
    /// `Feature.place(FeaturePlaceContext<FC>)` — the abstract behavior; every
    /// concrete feature implements it.
    fn place<R: RandomSource>(&self, context: &mut FeaturePlaceContext<'_, FC, R>) -> bool;

    /// `Feature.place(FC, WorldGenLevel, ChunkGenerator, RandomSource, BlockPos)`
    /// — `level.ensureCanWrite(origin) && this.place(new FeaturePlaceContext(
    /// Optional.empty(), ...))`. The top-feature slot is `None` at this entry
    /// point.
    fn place_with_config<R: RandomSource>(
        &self,
        config: &FC,
        level: &mut dyn WorldGenLevel,
        chunk_generator: &dyn ChunkGenerator,
        random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        level.ensure_can_write(origin)
            && self.place(&mut FeaturePlaceContext::new(
                None,
                level,
                chunk_generator,
                random,
                origin,
                config,
            ))
    }
}

// ---------------------------------------------------------------------------
// Feature identity + the #181 dispatch hub
// ---------------------------------------------------------------------------

/// `net.minecraft.core.Registry` element identity for `BuiltInRegistries.FEATURE`
/// — the per-feature-instance `u32` id (element id == holder id == network id
/// == insertion index, OWNERSHIP.md §Registries). `ConfiguredFeature` holds
/// this handle; the `#181`/codegen registration table assigns these ids, and
/// until then the leaf units' hand-assigned `FeatureId`s are the values those
/// generated registrations will use.
///
/// A `FeatureId` is identity-semantic (Java `Feature` object identity), so it
/// is intentionally *not* `Copy`: the id is opaque and can be cloned through
/// the registry. The value types that embed it (`ConfiguredFeature`) derive
/// `Clone`+`PartialEq`.
///
/// The registry-key location of a feature's registration is resolved through
/// the generated `#181` registration table (Java's `Feature` has no
/// id/location accessor), not stored on the id — the configured codecs recover
/// the dispatch key from the table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureId {
    /// The per-instance `u32` identity (insertion index in the feature registry).
    pub id: u32,
}

impl FeatureId {
    /// `new FeatureId(u32)` — a feature's registry identity.
    pub const fn new(id: u32) -> FeatureId {
        FeatureId { id }
    }
}

/// The `#181` hub — dispatch a `FeatureId` + erased config to a placement.
///
/// `Feature.java`'s registration table (the `register(...)` calls that bind
/// each concrete feature to `BuiltInRegistries.FEATURE`) is generated content —
/// emitted by `rivet-codegen` per the `#181` manifest note. The generated
/// dispatch is a monomorphized `match` that downcasts the erased config to the
/// feature's `Config` and calls the concrete feature's `place_with_config`
/// (the faithful mapping of `Feature.place(FC, WorldGenLevel, ChunkGenerator,
/// RandomSource, BlockPos)`, which applies the `ensureCanWrite` gate). Until
/// the table is wired this stub panics unconditionally — it is the pre-wire
/// stand-in for the generated dispatch, whose unknown-id path will throw
/// `IllegalStateException` like Java's `Registry.getValueOrThrow` (Java throws
/// only when the key is genuinely missing).
pub fn feature_place<R: RandomSource>(
    feature: FeatureId,
    _config: &dyn FeatureConfiguration,
    _level: &mut dyn WorldGenLevel,
    _chunk_generator: &dyn ChunkGenerator,
    _random: &mut R,
    _origin: &BlockPos,
) -> bool {
    // STUB(mc.world.level.levelgen.feature.core) — the generated
    // `BuiltInRegistries.FEATURE` dispatch table (`Feature.register`, ~60
    // entries). The dispatch key (the feature's registry location) is recovered
    // from that table, not stored on the `FeatureId`.
    panic!(
        "Trying to place feature '{}' with no registered behavior (#181 codegen)",
        feature.id
    );
    // The remaining parameters (`_config`, `_level`, `_chunk_generator`,
    // `_random`, `_origin`) are unused only because this stub panics before the
    // generated match would downcast the config and call the concrete feature's
    // `place_with_config`; the `_` prefixes keep them in the signature (so the
    // stub shape matches the generated dispatch exactly) without tripping
    // `-Dwarnings`.
}

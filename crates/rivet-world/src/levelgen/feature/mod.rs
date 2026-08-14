//! `net.minecraft.world.level.levelgen.feature` — feature worldgen.
//!
//! Owned by the `mc.world.level.levelgen.feature.core` manifest unit (26.2):
//! `Feature.java`, `ConfiguredFeature.java`, `FeaturePlaceContext.java`,
//! `FeatureCountTracker.java`, `package-info.java`. `WeightedPlacedFeature.java`
//! is owned by the `.feature.selector` manifest unit (see MANIFEST.tsv) and is
//! ported in the vegetation-family wave (issue #600) as
//! [`weighted_placed_feature`] (the `.selector` unit's selector-feature/config
//! port lands on top of it).
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
//! `ConfiguredFeature.place` path applies. This core unit ports that dispatch as
//! `feature_place` and covers the leaf it can faithfully reach — `minecraft:no_op`
//! (see `no_op_feature`, id 0, config `NoneFeatureConfiguration`), plus this wave
//! (issue #600) the five `.feature.selector` leaves: `random_selector` (id 52),
//! `weighted_random_selector` (id 53), `simple_random_selector` (id 54),
//! `random_boolean_selector` (id 55), and `sequence` (id 56).
//! The remaining registrations are generated content: `Feature.register`'s
//! entries bind each leaf to `BuiltInRegistries.FEATURE`, and the generated
//! dispatch folds them in. Until they are emitted, dispatching to an unavailable
//! leaf fails explicitly (an honest panic naming the id), never fabricating
//! success — the same capability-unavailable seam as `#399` world access.
//!
//! The configured/registry codecs (`DIRECT_CODEC` by-name dispatch,
//! `RegistryFileCodec`/`RegistryCodecs.homogeneousList`, the per-feature
//! `configuredCodec`) are deferred to issue #126 with the by-name codec
//! surface.
//!
//! `Feature.java`'s protected/static helper surface is ported where the seams
//! exist: `isReplaceable`, `checkNeighbors`, `isAdjacentToAir` (pure functions
//! over `BlockState`/`BlockPos`), and `place_with_config` (the `FeatureBehavior`
//! default; see below). The write seam `WorldGenLevel::set_block` (the target
//! of `LevelWriter.setBlock` with the `Block.UPDATE_ALL`/`UPDATE_CLIENTS` flag
//! constants) is declared on `WorldGenLevel` and reduces `Feature.setBlock`/
//! `safeSetBlock` to it — its default fails explicitly until the owning
//! `world.level` unit lands (RivetTodo #232). The `Feature.setBlock`/
//! `safeSetBlock` helper methods themselves and
//! `ChunkAccess.markPosForPostProcessing` are NOT ported yet, so declaring
//! them would fabricate a write. `configuredCodec()` stays deferred with the
//! `#126` by-name codec surface. Concrete feature units will reach for the
//! ported helpers at `check_neighbors(context.level(), ...)` /
//! `is_replaceable(...)`.

pub mod configurations;

mod feature_count_tracker;
mod feature_place_context;
mod no_op_feature;
pub mod ore_feature;

// The vegetation-family wave (issue #600) — the nine aquatic/vegetation
// feature structs that live in this module. Each is owned by its own leaf row
// (`.feature.bamboo`/`basaltpillar`/`blockblob`/`blockpile`/`blueice`/`kelp`/
// `nether_forest_vegetation`/`sea_pickle`/`seagrass`, the last through the
// `.feature.vegetation` cluster) and wired into the `#181` dispatch hub at its
// registration id; each config is owned by its own `configurations.*` row.
pub mod bamboo_feature;
pub mod basalt_pillar_feature;
pub mod block_blob_feature;
pub mod block_pile_feature;
pub mod blue_ice_feature;
pub mod kelp_feature;
pub mod nether_forest_vegetation_feature;
pub mod sea_pickle_feature;
pub mod seagrass_feature;

// The `net.minecraft.world.level.levelgen.feature.stateproviders` value layer
// (this unit) — the `BlockStateProvider` hierarchy and its declaration-order
// codec dispatch (see the submodule doc).
pub mod stateproviders;

// The `net.minecraft.world.level.levelgen.feature.featuresize` value layer
// (issue #391) — the `FeatureSize` hierarchy and its declaration-order codec
// dispatch. Owned by the `mc.world.level.levelgen.feature.featuresize` unit.
pub mod featuresize;

// The vegetation-family wave (issue #600) — the `.feature.selector` manifest
// unit's slice that lives in this module: the `WeightedPlacedFeature` record
// (owned by `.feature.selector`), the registry-key seam the selector features
// resolve their holders through, the `getSubFeatures` flattening helper the
// selector/composite configurations
// share, and the five concrete selector features (`RandomSelectorFeature`,
// `RandomBooleanSelectorFeature`, `SimpleRandomSelectorFeature`,
// `WeightedRandomSelectorFeature`, `SequenceFeature`). The selector
// configurations are owned by `.configurations.selector` and live in the
// `configurations` submodule.
pub mod random_boolean_selector_feature;
pub mod random_selector_feature;
pub mod registry_keys;
pub mod sequence_feature;
pub mod simple_random_selector_feature;
pub mod sub_features;
pub mod weighted_placed_feature;
pub mod weighted_random_selector_feature;

// The shared test double for the `.feature.selector` placement tests — the
// two-registry access the selector features resolve their holders through, a
// `WorldGenLevel`/`ChunkGenerator` double over it, and the RNG-call-recording
// `RandomSource` that pins the exact Java draw order.
#[cfg(test)]
mod test_support;

// The `net.minecraft.world.level.levelgen.feature.foliageplacers` framework
// (this unit) — the `FoliagePlacer` hierarchy, its `FoliagePlacerType` ids, and
// the eleven concrete placers (see the submodule doc).
pub mod foliageplacers;

// STUB(mc.world.level.levelgen.feature.tree): `TreeFeature.validTreePos` —
// the cross-unit helper the foliage/trunk/root placer leaves consume before
// every placement. Owned by the pending `feature.tree` manifest unit (row 569);
// see `tree_feature.rs`.
mod tree_feature;

use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::BlockBlobConfiguration;
use crate::levelgen::feature::configurations::BlockPileConfiguration;
use crate::levelgen::feature::configurations::CompositeFeatureConfiguration;
use crate::levelgen::feature::configurations::CountConfiguration;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use crate::levelgen::feature::configurations::NetherForestVegetationConfig;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::configurations::ProbabilityFeatureConfiguration;
use crate::levelgen::feature::configurations::RandomBooleanFeatureConfiguration;
use crate::levelgen::feature::configurations::RandomFeatureConfiguration;
use crate::levelgen::feature::configurations::WeightedRandomFeatureConfiguration;
use crate::levelgen::feature::no_op_feature::NO_OP;
use rivet_registry::Holder;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_util::RandomSource;
use std::any::Any;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

pub use bamboo_feature::{BAMBOO, BambooFeature};
pub use basalt_pillar_feature::{BASALT_PILLAR, BasaltPillarFeature};
pub use block_blob_feature::{BLOCK_BLOB, BlockBlobFeature};
pub use block_pile_feature::{BLOCK_PILE, BlockPileFeature};
pub use blue_ice_feature::{BLUE_ICE, BlueIceFeature};
pub use feature_count_tracker::FeatureCountTracker;
pub use feature_place_context::FeaturePlaceContext;
pub use kelp_feature::{KELP, KelpFeature};
pub use nether_forest_vegetation_feature::{
    NETHER_FOREST_VEGETATION, NetherForestVegetationFeature,
};
pub use no_op_feature::NoOpFeature;
pub use sea_pickle_feature::{SEA_PICKLE, SeaPickleFeature};
pub use seagrass_feature::{SEAGRASS, SeagrassFeature};

// The vegetation-family wave (issue #600) — the `.feature.selector` unit's
// slice that lives in this module: `WeightedPlacedFeature` (owned by
// `.feature.selector`), the registry-key seam, the `getSubFeatures` flattening
// helper, and the five concrete selector features.
pub use random_boolean_selector_feature::{RANDOM_BOOLEAN_SELECTOR, RandomBooleanSelectorFeature};
pub use random_selector_feature::{RANDOM_SELECTOR, RandomSelectorFeature};
pub use registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
pub use sequence_feature::{SEQUENCE, SequenceFeature};
pub use simple_random_selector_feature::{SIMPLE_RANDOM_SELECTOR, SimpleRandomSelectorFeature};
pub use sub_features::placed_sub_features;
pub use weighted_placed_feature::WeightedPlacedFeature;
pub use weighted_random_selector_feature::{
    WEIGHTED_RANDOM_SELECTOR, WeightedRandomSelectorFeature,
};

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
/// RandomSource, BlockPos)`, which applies the `ensureCanWrite` gate).
///
/// This unit ports the dispatch and covers the leaves it can faithfully
/// reach: `minecraft:no_op` (id 0, `NoOpFeature` over `NoneFeatureConfiguration`)
/// and, this wave (issue #600), the five `.feature.selector` leaves —
/// `random_selector` (id 52, `RandomSelectorFeature` over
/// `RandomFeatureConfiguration`), `weighted_random_selector` (id 53,
/// `WeightedRandomSelectorFeature` over `WeightedRandomFeatureConfiguration`),
/// `simple_random_selector` (id 54, `SimpleRandomSelectorFeature` over
/// `CompositeFeatureConfiguration`), `random_boolean_selector` (id 55,
/// `RandomBooleanSelectorFeature` over `RandomBooleanFeatureConfiguration`),
/// and `sequence` (id 56, `SequenceFeature` over
/// `CompositeFeatureConfiguration`) — whose ids are the feature registry's
/// insertion indices (protocol ids in `registries.json`; the registration
/// table's 63 `register(...)` calls are counted directly from `Feature.java`).
/// Every other registered feature is an unavailable leaf (owned by its own
/// manifest unit) — dispatching to one fails explicitly with an honest panic
/// naming the feature id, never fabricating success. `Registry.getValueOrThrow`
/// is the by-name CODEC path (`#126`), not placement dispatch — `ConfiguredFeature.place`
/// calls the `Feature` object directly, so Java cannot fail placement on an
/// unregistered key at all. Here the id is *registered* but its behavior has
/// not been emitted, so the failure is the same capability-unavailable seam as
/// `#399` world access and `block_state_provider_get_state`'s unknown-type
/// arm. The `minecraft:no_op` id is the feature registry's insertion index 0
/// (protocol id 0 in `registries.json`).
pub fn feature_place<R: RandomSource>(
    feature: FeatureId,
    config: &dyn FeatureConfiguration,
    level: &mut dyn WorldGenLevel,
    chunk_generator: &dyn ChunkGenerator,
    random: &mut R,
    origin: &BlockPos,
) -> bool {
    match feature.id {
        // `Feature.NO_OP` — the no-op feature returns `true` unconditionally.
        0 => {
            let config = (config as &dyn Any)
                .downcast_ref::<NoneFeatureConfiguration>()
                .expect("no_op feature must carry a NoneFeatureConfiguration");
            NO_OP.place_with_config(config, level, chunk_generator, random, origin)
        }
        // The five `.feature.selector` leaves (this wave, issue #600) — each
        // downcasts to its own config and delegates to `place_with_config` (the
        // `ensureCanWrite` gate applied here, as Java's `Feature.place(FC, …)`
        // does). The selector features resolve their placed/configured-feature
        // holders through `WorldGenLevel::registry_access`.
        // `Feature.RANDOM_SELECTOR`.
        52 => {
            let config = (config as &dyn Any)
                .downcast_ref::<RandomFeatureConfiguration>()
                .expect("random_selector feature must carry a RandomFeatureConfiguration");
            RANDOM_SELECTOR.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.WEIGHTED_RANDOM_SELECTOR`.
        53 => {
            let config = (config as &dyn Any)
                .downcast_ref::<WeightedRandomFeatureConfiguration>()
                .expect(
                    "weighted_random_selector feature must carry a WeightedRandomFeatureConfiguration",
                );
            WEIGHTED_RANDOM_SELECTOR.place_with_config(
                config,
                level,
                chunk_generator,
                random,
                origin,
            )
        }
        // `Feature.SIMPLE_RANDOM_SELECTOR`.
        54 => {
            let config = (config as &dyn Any)
                .downcast_ref::<CompositeFeatureConfiguration>()
                .expect(
                    "simple_random_selector feature must carry a CompositeFeatureConfiguration",
                );
            SIMPLE_RANDOM_SELECTOR.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.RANDOM_BOOLEAN_SELECTOR`.
        55 => {
            let config = (config as &dyn Any)
                .downcast_ref::<RandomBooleanFeatureConfiguration>()
                .expect(
                    "random_boolean_selector feature must carry a RandomBooleanFeatureConfiguration",
                );
            RANDOM_BOOLEAN_SELECTOR.place_with_config(
                config,
                level,
                chunk_generator,
                random,
                origin,
            )
        }
        // `Feature.SEQUENCE`.
        56 => {
            let config = (config as &dyn Any)
                .downcast_ref::<CompositeFeatureConfiguration>()
                .expect("sequence feature must carry a CompositeFeatureConfiguration");
            SEQUENCE.place_with_config(config, level, chunk_generator, random, origin)
        }
        // The nine vegetation-family leaves (this wave, issue #600) — the
        // aquatic/vegetation feature structs (each owned by its own
        // `.feature.*` MANIFEST row). Each downcasts to its own config and
        // delegates to `place_with_config` (the `ensureCanWrite` gate applied
        // here, as Java's `Feature.place(FC, …)` does). `Feature.BLOCK_PILE`.
        3 => {
            let config = (config as &dyn Any)
                .downcast_ref::<BlockPileConfiguration>()
                .expect("block_pile feature must carry a BlockPileConfiguration");
            BLOCK_PILE.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.BLUE_ICE`.
        23 => {
            let config = (config as &dyn Any)
                .downcast_ref::<NoneFeatureConfiguration>()
                .expect("blue_ice feature must carry a NoneFeatureConfiguration");
            BLUE_ICE.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.BLOCK_BLOB`.
        25 => {
            let config = (config as &dyn Any)
                .downcast_ref::<BlockBlobConfiguration>()
                .expect("block_blob feature must carry a BlockBlobConfiguration");
            BLOCK_BLOB.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.SEAGRASS`.
        33 => {
            let config = (config as &dyn Any)
                .downcast_ref::<ProbabilityFeatureConfiguration>()
                .expect("seagrass feature must carry a ProbabilityFeatureConfiguration");
            SEAGRASS.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.KELP`.
        34 => {
            let config = (config as &dyn Any)
                .downcast_ref::<NoneFeatureConfiguration>()
                .expect("kelp feature must carry a NoneFeatureConfiguration");
            KELP.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.SEA_PICKLE`.
        38 => {
            let config = (config as &dyn Any)
                .downcast_ref::<CountConfiguration>()
                .expect("sea_pickle feature must carry a CountConfiguration");
            SEA_PICKLE.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.BAMBOO`.
        40 => {
            let config = (config as &dyn Any)
                .downcast_ref::<ProbabilityFeatureConfiguration>()
                .expect("bamboo feature must carry a ProbabilityFeatureConfiguration");
            BAMBOO.place_with_config(config, level, chunk_generator, random, origin)
        }
        // `Feature.NETHER_FOREST_VEGETATION`.
        42 => {
            let config = (config as &dyn Any)
                .downcast_ref::<NetherForestVegetationConfig>()
                .expect(
                    "nether_forest_vegetation feature must carry a NetherForestVegetationConfig",
                );
            NETHER_FOREST_VEGETATION.place_with_config(
                config,
                level,
                chunk_generator,
                random,
                origin,
            )
        }
        // `Feature.BASALT_PILLAR`.
        50 => {
            let config = (config as &dyn Any)
                .downcast_ref::<NoneFeatureConfiguration>()
                .expect("basalt_pillar feature must carry a NoneFeatureConfiguration");
            BASALT_PILLAR.place_with_config(config, level, chunk_generator, random, origin)
        }
        // The remaining registered features are generated content emitted
        // by `rivet-codegen`; until then they are unavailable leaves. Failing
        // explicitly (rather than returning a fabricated boolean) is the honest
        // representation the `#181` dispatch contract requires.
        other => panic!(
            "Trying to place feature id '{}' whose behavior is not ported yet (#181 codegen)",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// `Feature.java`'s protected/static helper surface
// ---------------------------------------------------------------------------
//
// The write seam `WorldGenLevel::set_block` (the target of
// `LevelWriter.setBlock` with the `Block.UPDATE_ALL`/`UPDATE_CLIENTS` flags)
// is declared on `WorldGenLevel` and reduces `Feature.setBlock`/`safeSetBlock`
// to it — its default fails explicitly until the owning `world.level` unit
// lands (RivetTodo #232). The `Feature.setBlock` / `safeSetBlock` helper
// methods themselves and `ChunkAccess.markPosForPostProcessing` are NOT ported
// yet, so declaring them would be fabrication. The three pure helpers below
// are reachable with existing seams.

/// `Feature.isReplaceable(TagKey<Block>)` — `s -> !s.is(cannotReplaceTag)`
/// over the block-state form; the port's `BlockState::is_in_tag(&str)` reads
/// the block-tag tables (unknown tags read as `false`, matching Paper's
/// `is(TagKey)` on a registry that has not bound the tag).
pub fn is_replaceable(cannot_replace_tag: &str) -> impl Fn(BlockState) -> bool + '_ {
    move |state: BlockState| !state.is_in_tag(cannot_replace_tag)
}

/// `Feature.checkNeighbors(Function<BlockPos, BlockState>, BlockPos,
/// Predicate<BlockState>)` — `true` if `predicate` holds at any of the six
/// axis-neighbor positions (`Direction.values()` order).
pub fn check_neighbors(
    block_getter: impl Fn(&BlockPos) -> BlockState,
    pos: &BlockPos,
    predicate: impl Fn(&BlockState) -> bool,
) -> bool {
    Direction::VALUES
        .iter()
        .any(|direction| predicate(&block_getter(&pos.relative(direction))))
}

/// `Feature.isAdjacentToAir(Function<BlockPos, BlockState>, BlockPos)` —
/// `checkNeighbors(blockGetter, pos, BlockStateBase::isAir)`.
pub fn is_adjacent_to_air(block_getter: impl Fn(&BlockPos) -> BlockState, pos: &BlockPos) -> bool {
    check_neighbors(block_getter, pos, |state| state.is_air())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::LegacyRandomSource;

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

    struct TestGenerator;

    impl ChunkGenerator for TestGenerator {
        fn get_min_y(&self) -> i32 {
            -64
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// `NoOpFeature.place` returns `true` unconditionally (Java: `return true`)
    /// — the placement that always "succeeds" and writes nothing. The dispatch
    /// applies the `ensureCanWrite` gate (`true` by default) then the concrete
    /// behavior, so `feature_place` for `minecraft:no_op` is `true`.
    #[test]
    fn no_op_place_returns_true() {
        let mut level = TestLevel;
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        let origin = BlockPos::new(0, 0, 0);
        let placed = feature_place(
            FeatureId::new(0),
            &NoneFeatureConfiguration::INSTANCE,
            &mut level,
            &generator,
            &mut random,
            &origin,
        );
        assert!(placed);
    }

    /// `ConfiguredFeature.place` routes through `feature_place` with the
    /// feature's config; a configured `minecraft:no_op` places `true`.
    #[test]
    fn configured_no_op_feature_place_returns_true() {
        let configured =
            ConfiguredFeature::new(FeatureId::new(0), NoneFeatureConfiguration::INSTANCE);
        let mut level = TestLevel;
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        let origin = BlockPos::new(0, 0, 0);
        assert!(configured.place(&mut level, &generator, &mut random, &origin));
    }

    /// The erased wildcard `ConfiguredFeature<?, ?>` recovers the config type
    /// through the dispatch (the `as_any` downcast) and places the same way.
    #[test]
    fn configured_no_op_feature_erased_place_returns_true() {
        let configured =
            ConfiguredFeature::new(FeatureId::new(0), NoneFeatureConfiguration::INSTANCE)
                .into_erased();
        let mut level = TestLevel;
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        let origin = BlockPos::new(0, 0, 0);
        assert!(configured.place(&mut level, &generator, &mut random, &origin));
    }

    /// Hostile: dispatching to a registered feature whose behavior has not been
    /// emitted (id 1, a generated-content leaf) fails explicitly with a panic
    /// naming the id — never fabricating a verdict (the `#399`-style
    /// capability-unavailable seam).
    #[test]
    #[should_panic(
        expected = "Trying to place feature id '1' whose behavior is not ported yet (#181 codegen)"
    )]
    fn dispatch_panics_for_an_unported_leaf() {
        let mut level = TestLevel;
        let generator = TestGenerator;
        let mut random = LegacyRandomSource::new(1);
        let origin = BlockPos::new(0, 0, 0);
        let _ = feature_place(
            FeatureId::new(1),
            &NoneFeatureConfiguration::INSTANCE,
            &mut level,
            &generator,
            &mut random,
            &origin,
        );
    }

    /// The five `.feature.selector` dispatch arms (ids 52-56) each downcast the
    /// erased config before delegating, so handing each arm the *wrong* config
    /// panics with that arm's "must carry a ..." message. This pins the
    /// registration id → feature mapping (the feature registry's insertion
    /// order in `Feature.java`) independently of `place_with_config`, which the
    /// bare `TestLevel` cannot reach (it has no `registry_access`).
    #[test]
    fn selector_dispatch_arms_pin_registration_ids() {
        let generator = TestGenerator;
        let origin = BlockPos::new(0, 0, 0);
        // (feature id, expected downcast panic message).
        let arms = [
            (
                52,
                "random_selector feature must carry a RandomFeatureConfiguration",
            ),
            (
                53,
                "weighted_random_selector feature must carry a WeightedRandomFeatureConfiguration",
            ),
            (
                54,
                "simple_random_selector feature must carry a CompositeFeatureConfiguration",
            ),
            (
                55,
                "random_boolean_selector feature must carry a RandomBooleanFeatureConfiguration",
            ),
            (
                56,
                "sequence feature must carry a CompositeFeatureConfiguration",
            ),
        ];
        for (id, message) in arms {
            let mut level = TestLevel;
            let mut random = LegacyRandomSource::new(1);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                feature_place(
                    FeatureId::new(id),
                    &NoneFeatureConfiguration::INSTANCE,
                    &mut level,
                    &generator,
                    &mut random,
                    &origin,
                )
            }));
            match result {
                Err(payload) => {
                    let text = payload
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                        .unwrap_or("<non-string panic>");
                    assert!(
                        text.contains(message),
                        "id {id}: expected panic containing {message:?}, got {text:?}"
                    );
                }
                Ok(placed) => panic!("id {id}: expected a wrong-config panic, but placed {placed}"),
            }
        }
    }

    /// `Feature.isReplaceable(cannotReplaceTag)` — `!state.is(tag)`. Grounded in
    /// the real `minecraft:replaceable` tag: air is a member (not replaceable),
    /// stone is not (replaceable).
    #[test]
    fn is_replaceable_reads_the_block_tag_table() {
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let replaceable = is_replaceable("minecraft:replaceable");
        assert!(!replaceable(air), "air is a `replaceable` member");
        assert!(replaceable(stone), "stone is not a `replaceable` member");
    }

    /// Unknown tags read as empty (Paper's `is(TagKey)` on an unbound registry),
    /// so every state is replaceable.
    #[test]
    fn is_replaceable_unknown_tag_reads_empty() {
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let replaceable = is_replaceable("minecraft:no_such_tag");
        assert!(replaceable(air));
    }

    /// `Feature.checkNeighbors` — `true` if the predicate holds at any of the
    /// six axis neighbors. A stone neighbor at +Y is detected.
    #[test]
    fn check_neighbors_matches_any_axis_neighbor() {
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let getter = |pos: &BlockPos| {
            if pos.get_y() == 1 { stone } else { air }
        };
        let found = check_neighbors(getter, &BlockPos::new(0, 0, 0), |state| !state.is_air());
        assert!(found);
    }

    #[test]
    fn check_neighbors_is_false_when_no_neighbor_matches() {
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let found = check_neighbors(|_| air, &BlockPos::new(0, 0, 0), |state| !state.is_air());
        assert!(!found);
    }

    /// `Feature.isAdjacentToAir` — `checkNeighbors(..., BlockStateBase::isAir)`.
    #[test]
    fn is_adjacent_to_air_detects_an_air_neighbor() {
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let getter = |pos: &BlockPos| {
            if pos.get_z() == -1 { air } else { stone }
        };
        assert!(is_adjacent_to_air(getter, &BlockPos::new(0, 0, 0)));
    }

    #[test]
    fn is_adjacent_to_air_is_false_without_an_air_neighbor() {
        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        assert!(!is_adjacent_to_air(|_| stone, &BlockPos::new(0, 0, 0)));
    }
}

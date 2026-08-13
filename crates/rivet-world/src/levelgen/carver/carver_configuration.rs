//! Port of `net.minecraft.world.level.levelgen.carver.CarverConfiguration`
//! (class, 26.2) — the configuration base every carver and configured carver
//! is generic over.
//!
//! Java: `CarverConfiguration extends ProbabilityFeatureConfiguration` and adds
//! five fields (`y` HeightProvider, `yScale` FloatProvider, `lavaLevel`
//! VerticalAnchor, `debugSettings` CarverDebugSettings, `replaceable`
//! HolderSet<Block>) plus the `CODEC` record codec over them:
//!
//! ```java
//! RecordCodecBuilder.mapCodec(i -> i.group(
//!     Codec.floatRange(0.0F, 1.0F).fieldOf("probability").forGetter(c -> c.probability),
//!     HeightProvider.CODEC.fieldOf("y").forGetter(c -> c.y),
//!     FloatProviders.CODEC.fieldOf("yScale").forGetter(c -> c.yScale),
//!     VerticalAnchor.CODEC.fieldOf("lava_level").forGetter(c -> c.lavaLevel),
//!     CarverDebugSettings.CODEC.optionalFieldOf("debug_settings", CarverDebugSettings.DEFAULT).forGetter(c -> c.debugSettings),
//!     RegistryCodecs.homogeneousList(Registries.BLOCK).fieldOf("replaceable").forGetter(c -> c.replaceable)
//! ).apply(i, CarverConfiguration::new))
//! ```
//!
//! The shell (issue #306) kept `CarverConfiguration` a *marker trait* so the
//! erased `Arc<dyn CarverConfiguration>` could hold it. The `#180` algorithm
//! needs the real field surface, so the trait now exposes the six accessors
//! (Java's `public final` fields) plus `as_any` for the dispatch downcast, and
//! the base *value* — Java's `CarverConfiguration` class data — is the
//! [`CarverConfigurationBase`] struct (OWNERSHIP.md — no inheritance: the
//! concrete sub-configurations embed the base and implement the trait by
//! delegation). The `extends ProbabilityFeatureConfiguration` inheritance is
//! also composition: the base carries the `probability` field directly and
//! implements the `FeatureConfiguration` marker.
//!
//! The `CODEC` is the ops-generic [`carver_configuration_base_map_codec`]
//! factory (a `MapCodec` — `CarverConfiguration.CODEC` is a `MapCodec`, the
//! flattenable super-group both `CaveCarverConfiguration.CODEC` and
//! `CanyonCarverConfiguration.CODEC` embed via `forGetter(c -> c)`).

use crate::levelgen::carver::carver_debug_settings::CarverDebugSettings;
use crate::levelgen::carver::carver_debug_settings::carver_debug_settings_codec;
use crate::levelgen::heightproviders::height_provider::HeightProvider;
use crate::levelgen::vertical_anchor::VerticalAnchor;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::float_provider::{FloatProvider, float_provider_codec};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.carver.CarverConfiguration` — the
/// configuration type bound of `ConfiguredWorldCarver` and `WorldCarverBehavior`
/// (Java's `C extends CarverConfiguration`). Every carver configuration value
/// implements it; the base field surface is delegated to the embedded
/// [`CarverConfigurationBase`].
pub trait CarverConfiguration: Debug + Send + Sync + 'static {
    /// `probability` (inherited from `ProbabilityFeatureConfiguration`).
    fn probability(&self) -> f32;
    /// `y` — the `HeightProvider` the carvers sample for the tunnel's start height.
    fn y(&self) -> &HeightProvider;
    /// `yScale` — the `FloatProvider` cave tunnels scale their vertical radius by.
    fn y_scale(&self) -> &FloatProvider;
    /// `lavaLevel` — the `VerticalAnchor` below which carved blocks become lava.
    fn lava_level(&self) -> &VerticalAnchor;
    /// `debugSettings` — the carver debug block-state overrides.
    fn debug_settings(&self) -> &CarverDebugSettings;
    /// `replaceable` — the `HolderSet<Block>` `WorldCarver.canReplaceBlock` tests against.
    fn replaceable(&self) -> &HolderSet<BlockType>;
    /// `type_id`-style identity for the erased dispatch downcast.
    fn as_any(&self) -> &dyn Any;
}

/// Java's `CarverConfiguration` base class value — the six fields every carver
/// configuration shares (see the module doc for the trait/struct split).
#[derive(Debug, Clone)]
pub struct CarverConfigurationBase {
    /// `probability` — inherited from `ProbabilityFeatureConfiguration`
    /// (`Codec.floatRange(0.0F, 1.0F)`).
    pub probability: f32,
    /// `y` — the `HeightProvider` the carvers sample for the tunnel's start height.
    pub y: HeightProvider,
    /// `yScale` — the `FloatProvider` cave tunnels scale their vertical radius by.
    pub y_scale: FloatProvider,
    /// `lavaLevel` — the `VerticalAnchor` below which carved blocks become lava.
    pub lava_level: VerticalAnchor,
    /// `debugSettings` — the carver debug block-state overrides.
    pub debug_settings: CarverDebugSettings,
    /// `replaceable` — the `HolderSet<Block>` `WorldCarver.canReplaceBlock` tests against.
    pub replaceable: HolderSet<BlockType>,
}

impl CarverConfigurationBase {
    /// `new CarverConfiguration(float, HeightProvider, FloatProvider,
    /// VerticalAnchor, CarverDebugSettings, HolderSet<Block>)` — the
    /// constructor (the codec's `apply` function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        probability: f32,
        y: HeightProvider,
        y_scale: FloatProvider,
        lava_level: VerticalAnchor,
        debug_settings: CarverDebugSettings,
        replaceable: HolderSet<BlockType>,
    ) -> Self {
        CarverConfigurationBase {
            probability,
            y,
            y_scale,
            lava_level,
            debug_settings,
            replaceable,
        }
    }
}

impl CarverConfiguration for CarverConfigurationBase {
    fn probability(&self) -> f32 {
        self.probability
    }
    fn y(&self) -> &HeightProvider {
        &self.y
    }
    fn y_scale(&self) -> &FloatProvider {
        &self.y_scale
    }
    fn lava_level(&self) -> &VerticalAnchor {
        &self.lava_level
    }
    fn debug_settings(&self) -> &CarverDebugSettings {
        &self.debug_settings
    }
    fn replaceable(&self) -> &HolderSet<BlockType> {
        &self.replaceable
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl PartialEq for CarverConfigurationBase {
    fn eq(&self, other: &Self) -> bool {
        // `Objects.equals` on the record's Float fields compares via
        // `Float.equals` (`Float.compare` semantics).
        self.probability == other.probability
            && self.y == other.y
            && self.y_scale == other.y_scale
            && self.lava_level == other.lava_level
            && self.debug_settings == other.debug_settings
            && self.replaceable == other.replaceable
    }
}

impl Eq for CarverConfigurationBase {}

impl crate::levelgen::feature::configurations::FeatureConfiguration for CarverConfigurationBase {}

/// `CarverConfiguration.CODEC` — the ops-generic
/// `carver_configuration_base_map_codec::<Ops>()` factory (a `MapCodec` record
/// codec over the six fields, `RecordCodecBuilder.mapCodec`). The
/// sub-configuration codecs (`CaveCarverConfiguration.CODEC`/
/// `CanyonCarverConfiguration.CODEC`) embed it via `forGetter(c -> c)` — the
/// flattened super-group.
pub fn carver_configuration_base_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<CarverConfigurationBase, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CarverConfigurationBase| c.probability),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "probability".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverConfigurationBase| c.y.clone()),
                codec::field_of(
                    crate::levelgen::heightproviders::height_provider::height_provider_codec::<Ops>(
                    ),
                    "y".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverConfigurationBase| c.y_scale.clone()),
                codec::field_of(float_provider_codec::<Ops>(), "yScale".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverConfigurationBase| c.lava_level),
                codec::field_of(
                    crate::levelgen::vertical_anchor::vertical_anchor_codec::<Ops>(),
                    "lava_level".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverConfigurationBase| c.debug_settings),
                codec::optional_field_of::<CarverDebugSettings, Ops>(
                    "debug_settings",
                    carver_debug_settings_codec::<Ops>(),
                    CarverDebugSettings::default(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CarverConfigurationBase| c.replaceable.clone()),
                replaceable_blocks_field_codec::<Ops>(),
            ))
            .apply(instance, Arc::new(CarverConfigurationBase::new))
    })
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the `"replaceable"`
/// field codec (the same helper `SpeleothemClusterConfiguration` uses).
fn replaceable_blocks_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<rivet_registry::Holder<BlockType>, Ops>> = Arc::new(
        RegistryFixedCodec::create(&rivet_registry::registries::BLOCK),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> = Arc::new(HolderSetCodec::create(
        &rivet_registry::registries::BLOCK,
        element,
        false,
    ));
    codec::field_of(holder_set, "replaceable".to_string())
}

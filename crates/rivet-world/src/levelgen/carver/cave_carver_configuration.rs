//! Port of `net.minecraft.world.level.levelgen.carver.CaveCarverConfiguration`
//! (class, 26.2) — the `CaveWorldCarver`/`NetherWorldCarver` configuration:
//! the `CarverConfiguration` base plus the three cave-specific `FloatProvider`s.
//!
//! Java: `CaveCarverConfiguration extends CarverConfiguration` and its `CODEC`
//! (a `Codec`, `RecordCodecBuilder.create`) embeds `CarverConfiguration.CODEC`
//! as the flattened first group member (`forGetter(c -> c)`), then the three
//! cave fields:
//!
//! ```java
//! RecordCodecBuilder.create(i -> i.group(
//!     CarverConfiguration.CODEC.forGetter(c -> c),
//!     FloatProviders.CODEC.fieldOf("horizontal_radius_multiplier").forGetter(c -> c.horizontalRadiusMultiplier),
//!     FloatProviders.CODEC.fieldOf("vertical_radius_multiplier").forGetter(c -> c.verticalRadiusMultiplier),
//!     FloatProviders.codec(-1.0F, 1.0F).fieldOf("floor_level").forGetter(c -> c.floorLevel)
//! ).apply(i, CaveCarverConfiguration::new))
//! ```
//!
//! OWNERSHIP.md — no inheritance: the Rust struct *embeds* the
//! [`CarverConfigurationBase`] and implements `CarverConfiguration` by
//! delegation. The codec's flattened base group member is
//! `RecordCodecBuilder::of(getter, carver_configuration_base_map_codec())`,
//! whose `MapCodec` decoder reads the same accumulated map (the DFU flatten).

use crate::levelgen::carver::carver_configuration::{
    CarverConfiguration, CarverConfigurationBase, carver_configuration_base_map_codec,
};
use crate::levelgen::heightproviders::height_provider::HeightProvider;
use crate::levelgen::vertical_anchor::VerticalAnchor;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::float_provider::{
    FloatProvider, float_provider_codec, float_provider_codec_with_bounds,
};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `CaveCarverConfiguration` — the `CarverConfiguration` base embedded (the
/// `extends` inheritance) plus `horizontalRadiusMultiplier`/
/// `verticalRadiusMultiplier`/`floorLevel`.
#[derive(Debug, Clone)]
pub struct CaveCarverConfiguration {
    /// The flattened `CarverConfiguration` base (the `super` fields).
    pub base: CarverConfigurationBase,
    /// `horizontalRadiusMultiplier` — the `FloatProvider` cave tunnels scale
    /// their horizontal radius by.
    pub horizontal_radius_multiplier: FloatProvider,
    /// `verticalRadiusMultiplier` — the `FloatProvider` cave tunnels scale
    /// their vertical radius by.
    pub vertical_radius_multiplier: FloatProvider,
    /// `floorLevel` — the `FloatProvider` `CaveWorldCarver.shouldSkip` tests
    /// `yd <= floorLevel` against (bounded `[-1.0, 1.0]`).
    pub floor_level: FloatProvider,
}

impl CaveCarverConfiguration {
    /// `new(float, HeightProvider, FloatProvider, VerticalAnchor,
    /// CarverDebugSettings, HolderSet<Block>, FloatProvider, FloatProvider,
    /// FloatProvider)` — the 9-arg constructor (the codec's `apply`
    /// function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        probability: f32,
        y: HeightProvider,
        y_scale: FloatProvider,
        lava_level: VerticalAnchor,
        debug_settings: crate::levelgen::carver::carver_debug_settings::CarverDebugSettings,
        replaceable: HolderSet<BlockType>,
        horizontal_radius_multiplier: FloatProvider,
        vertical_radius_multiplier: FloatProvider,
        floor_level: FloatProvider,
    ) -> Self {
        CaveCarverConfiguration {
            base: CarverConfigurationBase::new(
                probability,
                y,
                y_scale,
                lava_level,
                debug_settings,
                replaceable,
            ),
            horizontal_radius_multiplier,
            vertical_radius_multiplier,
            floor_level,
        }
    }

    /// The 8-arg constructor with `CarverDebugSettings.DEFAULT` — the Java
    /// 8-arg overload (used by callers that don't override the debug settings).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_default_debug(
        probability: f32,
        y: HeightProvider,
        y_scale: FloatProvider,
        lava_level: VerticalAnchor,
        replaceable: HolderSet<BlockType>,
        horizontal_radius_multiplier: FloatProvider,
        vertical_radius_multiplier: FloatProvider,
        floor_level: FloatProvider,
    ) -> Self {
        CaveCarverConfiguration::new(
            probability,
            y,
            y_scale,
            lava_level,
            crate::levelgen::carver::carver_debug_settings::CarverDebugSettings::default(),
            replaceable,
            horizontal_radius_multiplier,
            vertical_radius_multiplier,
            floor_level,
        )
    }

    /// `new(CarverConfiguration carver, FloatProvider, FloatProvider,
    /// FloatProvider)` — the from-base constructor (`new CarverConfiguration(
    /// carver, ...)`), building the base from the given configuration's fields.
    pub fn new_from_base(
        carver: &CarverConfigurationBase,
        horizontal_radius_multiplier: FloatProvider,
        vertical_radius_multiplier: FloatProvider,
        floor_level: FloatProvider,
    ) -> Self {
        CaveCarverConfiguration {
            base: carver.clone(),
            horizontal_radius_multiplier,
            vertical_radius_multiplier,
            floor_level,
        }
    }
}

impl CarverConfiguration for CaveCarverConfiguration {
    fn probability(&self) -> f32 {
        self.base.probability()
    }
    fn y(&self) -> &HeightProvider {
        self.base.y()
    }
    fn y_scale(&self) -> &FloatProvider {
        self.base.y_scale()
    }
    fn lava_level(&self) -> &VerticalAnchor {
        self.base.lava_level()
    }
    fn debug_settings(
        &self,
    ) -> &crate::levelgen::carver::carver_debug_settings::CarverDebugSettings {
        self.base.debug_settings()
    }
    fn replaceable(&self) -> &HolderSet<BlockType> {
        self.base.replaceable()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl PartialEq for CaveCarverConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // Java `CaveCarverConfiguration` has no `equals` override — the
        // `CarverConfiguration`/`Object` identity semantics apply. The
        // `PartialEq` here is the value comparison the codec round-trip tests
        // need; it is NOT Java-observable.
        self.base == other.base
            && self.horizontal_radius_multiplier == other.horizontal_radius_multiplier
            && self.vertical_radius_multiplier == other.vertical_radius_multiplier
            && self.floor_level == other.floor_level
    }
}

impl Eq for CaveCarverConfiguration {}

impl crate::levelgen::feature::configurations::FeatureConfiguration for CaveCarverConfiguration {}

/// `CaveCarverConfiguration.CODEC` — the ops-generic
/// `cave_carver_configuration_codec::<Ops>()` factory (a record `Codec` over
/// the flattened base + the three cave fields,
/// `RecordCodecBuilder.create`).
pub fn cave_carver_configuration_codec<
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
>() -> Arc<dyn Codec<CaveCarverConfiguration, Ops>> {
    record_builder::create::<CaveCarverConfiguration, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CaveCarverConfiguration| c.base.clone()),
                carver_configuration_base_map_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CaveCarverConfiguration| c.horizontal_radius_multiplier.clone()),
                codec::field_of(
                    float_provider_codec::<Ops>(),
                    "horizontal_radius_multiplier".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CaveCarverConfiguration| c.vertical_radius_multiplier.clone()),
                codec::field_of(
                    float_provider_codec::<Ops>(),
                    "vertical_radius_multiplier".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CaveCarverConfiguration| c.floor_level.clone()),
                codec::field_of(
                    float_provider_codec_with_bounds::<Ops>(-1.0, 1.0),
                    "floor_level".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |base: CarverConfigurationBase,
                     horizontal_radius_multiplier: FloatProvider,
                     vertical_radius_multiplier: FloatProvider,
                     floor_level: FloatProvider| {
                        CaveCarverConfiguration::new_from_base(
                            &base,
                            horizontal_radius_multiplier,
                            vertical_radius_multiplier,
                            floor_level,
                        )
                    },
                ),
            )
    })
}

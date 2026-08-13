//! Port of `net.minecraft.world.level.levelgen.carver.CanyonCarverConfiguration`
//! (class, 26.2) — the `CanyonWorldCarver` configuration: the
//! `CarverConfiguration` base plus `verticalRotation` and the nested
//! `CanyonShapeConfiguration` shape.
//!
//! Java: `CanyonCarverConfiguration extends CarverConfiguration` and its `CODEC`
//! (a `Codec`, `RecordCodecBuilder.create`) embeds `CarverConfiguration.CODEC`
//! as the flattened first group member (`forGetter(c -> c)`), then the canyon
//! fields:
//!
//! ```java
//! RecordCodecBuilder.create(i -> i.group(
//!     CarverConfiguration.CODEC.forGetter(c -> c),
//!     FloatProviders.CODEC.fieldOf("vertical_rotation").forGetter(c -> c.verticalRotation),
//!     CanyonShapeConfiguration.CODEC.fieldOf("shape").forGetter(c -> c.shape)
//! ).apply(i, CanyonCarverConfiguration::new))
//! ```
//!
//! The nested `CanyonShapeConfiguration` is a top-level type here (no nested
//! classes in Rust) with its own `canyon_shape_configuration_codec::<Ops>()`.
//!
//! OWNERSHIP.md — no inheritance: the Rust struct *embeds* the
//! [`CarverConfigurationBase`] and implements `CarverConfiguration` by
//! delegation, exactly like `CaveCarverConfiguration`.

use crate::levelgen::carver::carver_configuration::{
    CarverConfiguration, CarverConfigurationBase, carver_configuration_base_map_codec,
};
use crate::levelgen::carver::carver_debug_settings::CarverDebugSettings;
use crate::levelgen::heightproviders::height_provider::HeightProvider;
use crate::levelgen::vertical_anchor::VerticalAnchor;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::float_provider::{FloatProvider, float_provider_codec};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `CanyonCarverConfiguration` — the `CarverConfiguration` base embedded (the
/// `extends` inheritance) plus `verticalRotation` and the `shape`.
#[derive(Debug, Clone)]
pub struct CanyonCarverConfiguration {
    /// The flattened `CarverConfiguration` base (the `super` fields).
    pub base: CarverConfigurationBase,
    /// `verticalRotation` — the `FloatProvider` `CanyonWorldCarver.updateY`
    /// rotates the canyon's forward vector by (a signed angle, in degrees? —
    /// `updateY` adds `Mth.wrapDegrees((double)(currentStep + 1) *
    /// this.getY(sample, step)`-style rotation, see the carver).
    pub vertical_rotation: FloatProvider,
    /// `shape` — the nested `CanyonShapeConfiguration` (the canyon's width/
    /// thickness profile fields).
    pub shape: CanyonShapeConfiguration,
}

impl CanyonCarverConfiguration {
    /// `new(float, HeightProvider, FloatProvider, VerticalAnchor,
    /// CarverDebugSettings, HolderSet<Block>, FloatProvider,
    /// CanyonShapeConfiguration)` — the 8-arg constructor (the codec's `apply`
    /// function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        probability: f32,
        y: HeightProvider,
        y_scale: FloatProvider,
        lava_level: VerticalAnchor,
        debug_settings: CarverDebugSettings,
        replaceable: HolderSet<BlockType>,
        vertical_rotation: FloatProvider,
        shape: CanyonShapeConfiguration,
    ) -> Self {
        CanyonCarverConfiguration {
            base: CarverConfigurationBase::new(
                probability,
                y,
                y_scale,
                lava_level,
                debug_settings,
                replaceable,
            ),
            vertical_rotation,
            shape,
        }
    }

    /// `new(CarverConfiguration carver, FloatProvider, CanyonShapeConfiguration)`
    /// — the from-base constructor. Java names the third parameter
    /// `distanceFactor` (a Paper-side oddity) but passes it straight to
    /// `verticalRotation`; the Rust name keeps the field's real meaning.
    pub fn new_from_base(
        carver: &CarverConfigurationBase,
        vertical_rotation: FloatProvider,
        shape: CanyonShapeConfiguration,
    ) -> Self {
        CanyonCarverConfiguration {
            base: carver.clone(),
            vertical_rotation,
            shape,
        }
    }
}

impl CarverConfiguration for CanyonCarverConfiguration {
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
    fn debug_settings(&self) -> &CarverDebugSettings {
        self.base.debug_settings()
    }
    fn replaceable(&self) -> &HolderSet<BlockType> {
        self.base.replaceable()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl PartialEq for CanyonCarverConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // Java `CanyonCarverConfiguration` has no `equals` override — the
        // `CarverConfiguration`/`Object` identity semantics apply. The
        // `PartialEq` here is the value comparison the codec round-trip tests
        // need; it is NOT Java-observable.
        self.base == other.base
            && self.vertical_rotation == other.vertical_rotation
            && self.shape == other.shape
    }
}

impl Eq for CanyonCarverConfiguration {}

impl crate::levelgen::feature::configurations::FeatureConfiguration for CanyonCarverConfiguration {}

/// `CanyonCarverConfiguration.CanyonShapeConfiguration` — the nested shape
/// value class (the canyon's width/thickness profile). The codec is a record
/// `Codec` over its six fields (`RecordCodecBuilder.create`).
#[derive(Debug, Clone)]
pub struct CanyonShapeConfiguration {
    /// `distanceFactor` — the `FloatProvider` the canyon's longitudinal
    /// distance step is scaled by (`CanyonWorldCarver.calculateWidth` etc.).
    pub distance_factor: FloatProvider,
    /// `thickness` — the `FloatProvider` canyon wall thickness.
    pub thickness: FloatProvider,
    /// `widthSmoothness` — `ExtraCodecs.POSITIVE_INT` (`[1, i32::MAX]`); the
    /// chance denominator `CanyonWorldCarver.doCarve` rolls `nextInt(widthSmoothness)`
    /// against when (re)initializing the width factor.
    pub width_smoothness: i32,
    /// `horizontalRadiusFactor` — the `FloatProvider` scaling the canyon's
    /// horizontal radius.
    pub horizontal_radius_factor: FloatProvider,
    /// `verticalRadiusDefaultFactor` — `Codec.FLOAT`; the y-index-independent
    /// term of the vertical-radius multiplier (`verticalMultiplier` weights the
    /// center factor).
    pub vertical_radius_default_factor: f32,
    /// `verticalRadiusCenterFactor` — `Codec.FLOAT`; the term scaled by the
    /// `verticalMultiplier` (peak at the canyon's vertical center).
    pub vertical_radius_center_factor: f32,
}

impl CanyonShapeConfiguration {
    /// `new(FloatProvider, FloatProvider, int, FloatProvider, float, float)` —
    /// the constructor (the shape codec's `apply` function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        distance_factor: FloatProvider,
        thickness: FloatProvider,
        width_smoothness: i32,
        horizontal_radius_factor: FloatProvider,
        vertical_radius_default_factor: f32,
        vertical_radius_center_factor: f32,
    ) -> Self {
        CanyonShapeConfiguration {
            distance_factor,
            thickness,
            width_smoothness,
            horizontal_radius_factor,
            vertical_radius_default_factor,
            vertical_radius_center_factor,
        }
    }
}

impl PartialEq for CanyonShapeConfiguration {
    fn eq(&self, other: &Self) -> bool {
        self.distance_factor == other.distance_factor
            && self.thickness == other.thickness
            && self.width_smoothness == other.width_smoothness
            && self.horizontal_radius_factor == other.horizontal_radius_factor
            && self.vertical_radius_default_factor == other.vertical_radius_default_factor
            && self.vertical_radius_center_factor == other.vertical_radius_center_factor
    }
}

impl Eq for CanyonShapeConfiguration {}

/// `CanyonCarverConfiguration.CODEC` — the ops-generic
/// `canyon_carver_configuration_codec::<Ops>()` factory (a record `Codec` over
/// the flattened base + `vertical_rotation` + `shape`,
/// `RecordCodecBuilder.create`).
pub fn canyon_carver_configuration_codec<
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
>() -> Arc<dyn Codec<CanyonCarverConfiguration, Ops>> {
    record_builder::create::<CanyonCarverConfiguration, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonCarverConfiguration| c.base.clone()),
                carver_configuration_base_map_codec::<Ops>(),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonCarverConfiguration| c.vertical_rotation.clone()),
                codec::field_of(
                    float_provider_codec::<Ops>(),
                    "vertical_rotation".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonCarverConfiguration| c.shape.clone()),
                codec::field_of(
                    canyon_shape_configuration_codec::<Ops>(),
                    "shape".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |base: CarverConfigurationBase,
                     vertical_rotation: FloatProvider,
                     shape: CanyonShapeConfiguration| {
                        CanyonCarverConfiguration::new_from_base(&base, vertical_rotation, shape)
                    },
                ),
            )
    })
}

/// `CanyonShapeConfiguration.CODEC` — the ops-generic
/// `canyon_shape_configuration_codec::<Ops>()` factory (a record `Codec` over
/// the six shape fields, `RecordCodecBuilder.create`). `width_smoothness` is
/// `ExtraCodecs.POSITIVE_INT` (`codec::int_range(1, i32::MAX)`); the two float
/// factors are `Codec.FLOAT` (`codec::float_codec`, NOT bounded like the
/// `floor_level` field of `CaveCarverConfiguration`).
pub fn canyon_shape_configuration_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<CanyonShapeConfiguration, Ops>> {
    record_builder::create::<CanyonShapeConfiguration, Ops>(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonShapeConfiguration| c.distance_factor.clone()),
                codec::field_of(float_provider_codec::<Ops>(), "distance_factor".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonShapeConfiguration| c.thickness.clone()),
                codec::field_of(float_provider_codec::<Ops>(), "thickness".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonShapeConfiguration| c.width_smoothness),
                codec::field_of(
                    codec::int_range::<Ops>(1, i32::MAX),
                    "width_smoothness".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonShapeConfiguration| c.horizontal_radius_factor.clone()),
                codec::field_of(
                    float_provider_codec::<Ops>(),
                    "horizontal_radius_factor".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonShapeConfiguration| c.vertical_radius_default_factor),
                codec::field_of(
                    codec::float_codec::<Ops>(),
                    "vertical_radius_default_factor".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CanyonShapeConfiguration| c.vertical_radius_center_factor),
                codec::field_of(
                    codec::float_codec::<Ops>(),
                    "vertical_radius_center_factor".to_string(),
                ),
            ))
            .apply(instance, Arc::new(CanyonShapeConfiguration::new))
    })
}

//! Port of `net.minecraft.world.level.levelgen.feature.featuresize.TwoLayersFeatureSize`
//! (class, 26.2).
//!
//! Java is the two-layer tree-canopy size: `limit`, `lowerSize`, `upperSize`
//! plus the inherited `OptionalInt minClippedHeight`. Its `CODEC` is a
//! `RecordCodecBuilder.mapCodec` over the three ranged optional fields (defaults
//! `limit=1`, `lower_size=0`, `upper_size=1`) and the `min_clipped_height`
//! optional (absent → empty), so the port exposes it as the ops-generic
//! `map_codec::<Ops>()` factory. The concrete 3-arg constructor passes
//! `OptionalInt.empty()` for `minClippedHeight`.

use crate::levelgen::feature::featuresize::FeatureSize;
use rivet_registry::feature_size_type::FeatureSizeTypeId;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::any::Any;
use std::sync::Arc;

/// `Codec.intRange(...).optionalFieldOf(name, default)` — the lenient
/// optional-with-default field (absent → default on decode; omitted on encode
/// when equal to the default).
fn defaulted_field<Ops: DynamicOps + 'static>(
    name: &str,
    range: (i32, i32),
    default: i32,
) -> Arc<dyn MapCodec<i32, Ops>> {
    codec::lenient_optional_field_of(name, codec::int_range::<Ops>(range.0, range.1), default)
}

/// `net.minecraft.world.level.levelgen.feature.featuresize.TwoLayersFeatureSize`.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoLayersFeatureSize {
    /// `limit` — the relative height below which `lowerSize` applies.
    pub limit: i32,
    /// `lowerSize` — the canopy size for `yo < limit`.
    pub lower_size: i32,
    /// `upperSize` — the canopy size for `yo >= limit`.
    pub upper_size: i32,
    /// `minClippedHeight` — `Some` height clips the canopy above it.
    pub min_clipped_height: Option<i32>,
}

impl TwoLayersFeatureSize {
    /// `TwoLayersFeatureSize(int limit, int lowerSize, int upperSize)` — the
    /// 3-arg constructor delegates with `minClippedHeight = OptionalInt.empty()`.
    pub fn new(limit: i32, lower_size: i32, upper_size: i32) -> Self {
        TwoLayersFeatureSize {
            limit,
            lower_size,
            upper_size,
            min_clipped_height: None,
        }
    }

    /// `TwoLayersFeatureSize(int, int, int, OptionalInt)`.
    pub fn new_with_min_clipped_height(
        limit: i32,
        lower_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    ) -> Self {
        TwoLayersFeatureSize {
            limit,
            lower_size,
            upper_size,
            min_clipped_height,
        }
    }
}

impl FeatureSize for TwoLayersFeatureSize {
    fn type_id(&self) -> Arc<FeatureSizeTypeId> {
        FeatureSizeTypeId::from_name("minecraft:two_layers_feature_size")
            .expect("two_layers_feature_size is a generated built-in")
    }

    fn get_size_at_height(&self, _tree_height: i32, yo: i32) -> i32 {
        // `yo < this.limit ? this.lowerSize : this.upperSize`.
        if yo < self.limit {
            self.lower_size
        } else {
            self.upper_size
        }
    }

    fn min_clipped_height(&self) -> Option<i32> {
        self.min_clipped_height
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `TwoLayersFeatureSize.CODEC`, as the ops-generic `map_codec::<Ops>()`
/// factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.mapCodec(i -> i.group(
///     Codec.intRange(0, 81).optionalFieldOf("limit", 1),
///     Codec.intRange(0, 16).optionalFieldOf("lower_size", 0),
///     Codec.intRange(0, 16).optionalFieldOf("upper_size", 1),
///     minClippedHeightCodec())
///     .apply(i, TwoLayersFeatureSize::new))
/// ```
/// The `optionalFieldOf(name, default)` forms are lenient with a decode
/// default; `min_clipped_height` is the non-lenient optional `OptionalInt`
/// field (absent → empty).
pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<TwoLayersFeatureSize, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &TwoLayersFeatureSize| s.limit),
                defaulted_field::<Ops>("limit", (0, 81), 1),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &TwoLayersFeatureSize| s.lower_size),
                defaulted_field::<Ops>("lower_size", (0, 16), 0),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &TwoLayersFeatureSize| s.upper_size),
                defaulted_field::<Ops>("upper_size", (0, 16), 1),
            ))
            .and(super::feature_size::min_clipped_height_codec())
            .apply(
                instance,
                Arc::new(
                    |limit: i32,
                     lower_size: i32,
                     upper_size: i32,
                     min_clipped_height: Option<i32>| {
                        TwoLayersFeatureSize {
                            limit,
                            lower_size,
                            upper_size,
                            min_clipped_height,
                        }
                    },
                ),
            )
    })
}

//! Port of `net.minecraft.world.level.levelgen.feature.featuresize.ThreeLayersFeatureSize`
//! (class, 26.2).
//!
//! Java is the three-layer tree-canopy size: `limit`, `upperLimit`,
//! `lowerSize`, `middleSize`, `upperSize` plus the inherited
//! `OptionalInt minClippedHeight`. Its `CODEC` is a
//! `RecordCodecBuilder.mapCodec` over the five ranged optional fields
//! (defaults `limit=1`, `upper_limit=1`, `lower_size=0`, `middle_size=1`,
//! `upper_size=1`) and the `min_clipped_height` optional (absent → empty), so
//! the port exposes it as the ops-generic `map_codec::<Ops>()` factory. The
//! six-field group exercises the `RecordCodecBuilder` `Group6`/`ap6` surface.

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

/// `net.minecraft.world.level.levelgen.feature.featuresize.ThreeLayersFeatureSize`.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreeLayersFeatureSize {
    /// `limit` — the relative height below which `lowerSize` applies.
    pub limit: i32,
    /// `upperLimit` — `upperSize` applies at `yo >= treeHeight - upperLimit`.
    pub upper_limit: i32,
    /// `lowerSize` — the canopy size below `limit`.
    pub lower_size: i32,
    /// `middleSize` — the canopy size between the lower and upper layers.
    pub middle_size: i32,
    /// `upperSize` — the canopy size at the top layer.
    pub upper_size: i32,
    /// `minClippedHeight` — `Some` height clips the canopy above it.
    pub min_clipped_height: Option<i32>,
}

impl ThreeLayersFeatureSize {
    /// `ThreeLayersFeatureSize(int, int, int, int, int, OptionalInt)`.
    pub fn new(
        limit: i32,
        upper_limit: i32,
        lower_size: i32,
        middle_size: i32,
        upper_size: i32,
        min_clipped_height: Option<i32>,
    ) -> Self {
        ThreeLayersFeatureSize {
            limit,
            upper_limit,
            lower_size,
            middle_size,
            upper_size,
            min_clipped_height,
        }
    }
}

impl FeatureSize for ThreeLayersFeatureSize {
    fn type_id(&self) -> Arc<FeatureSizeTypeId> {
        FeatureSizeTypeId::from_name("minecraft:three_layers_feature_size")
            .expect("three_layers_feature_size is a generated built-in")
    }

    fn get_size_at_height(&self, tree_height: i32, yo: i32) -> i32 {
        // `if (yo < this.limit) return this.lowerSize; return yo >= treeHeight
        // - this.upperLimit ? this.upperSize : this.middleSize;`
        if yo < self.limit {
            self.lower_size
        } else if yo >= tree_height - self.upper_limit {
            self.upper_size
        } else {
            self.middle_size
        }
    }

    fn min_clipped_height(&self) -> Option<i32> {
        self.min_clipped_height
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `ThreeLayersFeatureSize.CODEC`, as the ops-generic `map_codec::<Ops>()`
/// factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.mapCodec(i -> i.group(
///     Codec.intRange(0, 80).optionalFieldOf("limit", 1),
///     Codec.intRange(0, 80).optionalFieldOf("upper_limit", 1),
///     Codec.intRange(0, 16).optionalFieldOf("lower_size", 0),
///     Codec.intRange(0, 16).optionalFieldOf("middle_size", 1),
///     Codec.intRange(0, 16).optionalFieldOf("upper_size", 1),
///     minClippedHeightCodec())
///     .apply(i, ThreeLayersFeatureSize::new))
/// ```
pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ThreeLayersFeatureSize, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &ThreeLayersFeatureSize| s.limit),
                defaulted_field::<Ops>("limit", (0, 80), 1),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &ThreeLayersFeatureSize| s.upper_limit),
                defaulted_field::<Ops>("upper_limit", (0, 80), 1),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &ThreeLayersFeatureSize| s.lower_size),
                defaulted_field::<Ops>("lower_size", (0, 16), 0),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &ThreeLayersFeatureSize| s.middle_size),
                defaulted_field::<Ops>("middle_size", (0, 16), 1),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|s: &ThreeLayersFeatureSize| s.upper_size),
                defaulted_field::<Ops>("upper_size", (0, 16), 1),
            ))
            .and(super::feature_size::min_clipped_height_codec())
            .apply(
                instance,
                Arc::new(
                    |limit: i32,
                     upper_limit: i32,
                     lower_size: i32,
                     middle_size: i32,
                     upper_size: i32,
                     min_clipped_height: Option<i32>| {
                        ThreeLayersFeatureSize {
                            limit,
                            upper_limit,
                            lower_size,
                            middle_size,
                            upper_size,
                            min_clipped_height,
                        }
                    },
                ),
            )
    })
}

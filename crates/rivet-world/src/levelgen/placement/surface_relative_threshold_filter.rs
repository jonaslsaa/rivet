//! Port of `net.minecraft.world.level.levelgen.placement.SurfaceRelativeThresholdFilter`
//! (class, 26.2).
//!
//! Java: a `PlacementFilter` whose `shouldPlace` keeps the origin when its Y
//! falls inside `[surface + minInclusive, surface + maxInclusive]`, where
//! `surface` is the column's height under the configured `Heightmap.Types`
//! (`context.getHeight` — `PlacementContext.getHeight` delegates to
//! `this.level.getHeight(type, x, z)`), and whose `type()` is
//! `PlacementModifierType.SURFACE_RELATIVE_THRESHOLD_FILTER`. Its `CODEC` is
//! the required `"heightmap"` field (`Heightmap.Types.CODEC`) plus the
//! optional `"min_inclusive"` (default `Integer.MIN_VALUE`) and
//! `"max_inclusive"` (default `Integer.MAX_VALUE`) fields.
//!
//! The Java `long surfaceY`/`minY`/`maxY` widen the int height and thresholds
//! to `long` before summing, so the comparisons never overflow; the port
//! mirrors that with `i64` (the exact widening Java's assignment to `long`
//! performs — the `min_inclusive`/`max_inclusive` ints sign-extend).
//!
//! The `getHeight` read is `PlacementContext.getHeight`, which delegates to the
//! `WorldGenLevel::get_height_at` trait-default seam (the `#228`-deferred
//! `LevelReader.getHeight` heightmap read — see `placement_context.rs`), so the
//! filter body stays executable against a level that answers heights.

use crate::levelgen::heightmap::Types;
use crate::levelgen::placement::placement_modifier_type::{
    PlacementModifierTypeId, PlacementModifierTypes,
};
use crate::levelgen::placement::{PlacementContext, PlacementFilter};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.placement.SurfaceRelativeThresholdFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceRelativeThresholdFilter {
    /// `this.heightmap` — the heightmap the surface is read from.
    heightmap: Types,
    /// `this.minInclusive` — the inclusive lower bound, relative to the surface.
    min_inclusive: i32,
    /// `this.maxInclusive` — the inclusive upper bound, relative to the surface.
    max_inclusive: i32,
}

impl SurfaceRelativeThresholdFilter {
    /// `of(Heightmap.Types, int, int)`.
    pub fn of(heightmap: Types, min_inclusive: i32, max_inclusive: i32) -> Self {
        SurfaceRelativeThresholdFilter {
            heightmap,
            min_inclusive,
            max_inclusive,
        }
    }
}

impl PlacementFilter for SurfaceRelativeThresholdFilter {
    fn should_place<R: RandomSource>(
        &self,
        context: &PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        // `PlacementContext.getHeight(this.heightmap, origin.getX(), origin.getZ())`
        // — the `LevelReader.getHeight` read; the `#228`-deferred heightmap
        // read fails explicitly rather than fabricating a surface.
        let surface_y = context.get_height(self.heightmap, origin.get_x(), origin.get_z()) as i64;
        let min_y = surface_y + self.min_inclusive as i64;
        let max_y = surface_y + self.max_inclusive as i64;
        min_y <= origin.get_y() as i64 && origin.get_y() as i64 <= max_y
    }

    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::SURFACE_RELATIVE_THRESHOLD_FILTER
    }
}

/// `SurfaceRelativeThresholdFilter.CODEC` — `Heightmap.Types.CODEC` as the
/// required `"heightmap"` field plus the optional `"min_inclusive"` /
/// `"max_inclusive"` int fields (defaults `Integer.MIN_VALUE` /
/// `Integer.MAX_VALUE`, omitted on encode when equal to the default), as the
/// ops-generic `surface_relative_threshold_filter_map_codec::<Ops>()` factory.
pub fn surface_relative_threshold_filter_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<SurfaceRelativeThresholdFilter, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &SurfaceRelativeThresholdFilter| c.heightmap),
                "heightmap".to_string(),
                Arc::new(crate::levelgen::heightmap::types_codec::<Ops>()),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|c: &SurfaceRelativeThresholdFilter| c.min_inclusive),
                codec::optional_field_of("min_inclusive", codec::int_codec::<Ops>(), i32::MIN),
            ))
            .and(record_builder::RecordCodecBuilder::of(
                Arc::new(|c: &SurfaceRelativeThresholdFilter| c.max_inclusive),
                codec::optional_field_of("max_inclusive", codec::int_codec::<Ops>(), i32::MAX),
            ))
            .apply(
                instance,
                Arc::new(|heightmap: Types, min_inclusive: i32, max_inclusive: i32| {
                    SurfaceRelativeThresholdFilter::of(heightmap, min_inclusive, max_inclusive)
                }),
            )
    })
}

/// `SurfaceRelativeThresholdFilter.CODEC` as a `Codec`
/// (`MapCodec.codec()` — `record.codec()`), the shape the `#181` generated
/// dispatch's registration table consumes.
#[allow(dead_code)]
pub fn surface_relative_threshold_filter_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SurfaceRelativeThresholdFilter, Ops>> {
    map_codec::codec_of(surface_relative_threshold_filter_map_codec::<Ops>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::WorldGenLevel;
    use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor, create};
    use crate::levelgen::placement::PlacementModifier;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::json;

    /// A minimal `WorldGenLevel` double over the overworld window. The
    /// column-height read answers a fixed surface, so the filter's band math
    /// is exercised against a real height (the `#228` default seam is
    /// deferral-only).
    struct TestLevel {
        accessor: SimpleLevelHeightAccessor,
        surface: i32,
    }

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            self.accessor.get_height()
        }

        fn get_min_y(&self) -> i32 {
            self.accessor.get_min_y()
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }

        fn get_height_at(&self, _ty: Types, _x: i32, _z: i32) -> i32 {
            self.surface
        }
    }

    struct NoopGenerator;

    impl crate::chunk::ChunkGenerator for NoopGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// `PlacementModifier::get_positions` on a `SurfaceRelativeThresholdFilter`
    /// over the overworld window — the blanket `PlacementFilter` shell.
    fn filter_positions(
        filter: &SurfaceRelativeThresholdFilter,
        level: &mut TestLevel,
        random: &mut LegacyRandomSource,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let generator = NoopGenerator;
        let context = PlacementContext::new(level, &generator, None);
        PlacementModifier::get_positions(filter, &context, random, origin).collect()
    }

    #[test]
    fn keeps_origin_when_y_is_within_the_relative_band() {
        // `minY <= origin.getY() && origin.getY() <= maxY` with `minY =
        // surface + minInclusive`, `maxY = surface + maxInclusive` (all in
        // long): surface 70, band [-3, 3] -> [67, 73], and origin.y = 70 is
        // inside.
        let origin = BlockPos::new(1, 70, 3);
        let filter = SurfaceRelativeThresholdFilter::of(Types::MotionBlocking, -3, 3);
        let mut level = TestLevel {
            accessor: create(-64, 384),
            surface: 70,
        };
        let mut random = LegacyRandomSource::new(0);
        let result = filter_positions(&filter, &mut level, &mut random, &origin);
        assert_eq!(result, vec![origin]);
    }

    #[test]
    fn drops_origin_when_y_is_outside_the_relative_band() {
        // Surface 70, band [-3, 3] -> [67, 73]; origin.y = 74 is one above the
        // max and drops.
        let origin = BlockPos::new(1, 74, 3);
        let filter = SurfaceRelativeThresholdFilter::of(Types::MotionBlocking, -3, 3);
        let mut level = TestLevel {
            accessor: create(-64, 384),
            surface: 70,
        };
        let mut random = LegacyRandomSource::new(0);
        let result = filter_positions(&filter, &mut level, &mut random, &origin);
        assert!(result.is_empty());
    }

    #[test]
    fn band_boundaries_are_inclusive() {
        // Band [67, 73] is inclusive on both ends.
        let filter = SurfaceRelativeThresholdFilter::of(Types::MotionBlocking, -3, 3);
        for y in [67, 73] {
            let origin = BlockPos::new(1, y, 3);
            let mut level = TestLevel {
                accessor: create(-64, 384),
                surface: 70,
            };
            let mut random = LegacyRandomSource::new(0);
            let result = filter_positions(&filter, &mut level, &mut random, &origin);
            assert_eq!(result, vec![origin], "y={y} is inside the band");
        }
    }

    #[test]
    fn surface_relative_type_identity_is_reported() {
        // `PlacementModifierType.SURFACE_RELATIVE_THRESHOLD_FILTER` is
        // insertion index 2 in `PlacementModifierType.java`'s registration
        // order.
        let filter = SurfaceRelativeThresholdFilter::of(Types::WorldSurface, 0, 1);
        assert_eq!(
            PlacementFilter::type_id(&filter),
            PlacementModifierTypes::SURFACE_RELATIVE_THRESHOLD_FILTER
        );
    }

    #[test]
    fn codec_round_trips_fields_and_defaults() {
        // The `"heightmap"` field is required (encoded via the
        // `Heightmap.Types` enum codec); `"min_inclusive"`/`"max_inclusive"`
        // default to `Integer.MIN_VALUE`/`Integer.MAX_VALUE` and are omitted
        // on encode when equal to those defaults.
        let ops = JsonOps::INSTANCE;
        let codec = surface_relative_threshold_filter_codec::<JsonOps>();
        let filter = SurfaceRelativeThresholdFilter::of(Types::OceanFloor, -3, 3);
        let encoded = codec
            .encode_start(&ops, &filter)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "heightmap": "OCEAN_FLOOR",
                "min_inclusive": -3,
                "max_inclusive": 3
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .copied()
            .expect("decode should succeed");
        assert_eq!(decoded, filter);
        // Defaults: a filter with the default bounds encodes to just the
        // heightmap field, and decodes back to the default bounds.
        let defaulted = SurfaceRelativeThresholdFilter::of(Types::WorldSurface, i32::MIN, i32::MAX);
        let encoded_default = codec
            .encode_start(&ops, &defaulted)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded_default, json!({"heightmap": "WORLD_SURFACE"}));
        let decoded_default = codec
            .parse(&ops, &json!({"heightmap": "WORLD_SURFACE"}))
            .result()
            .copied()
            .expect("decode should succeed");
        assert_eq!(decoded_default, defaulted);
    }

    #[test]
    fn codec_unknown_heightmap_errors() {
        // `Heightmap.Types.CODEC` rejects an unknown serialized name.
        let ops = JsonOps::INSTANCE;
        let codec = surface_relative_threshold_filter_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({"heightmap": "NOT_A_HEIGHTMAP"}));
        assert!(result.is_error());
    }

    #[test]
    fn codec_missing_heightmap_field_errors() {
        let ops = JsonOps::INSTANCE;
        let codec = surface_relative_threshold_filter_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key heightmap"), "got: {msg}");
    }
}

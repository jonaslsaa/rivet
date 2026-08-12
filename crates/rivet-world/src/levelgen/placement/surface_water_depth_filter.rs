//! Port of `net.minecraft.world.level.levelgen.placement.SurfaceWaterDepthFilter`
//! (class, 26.2).
//!
//! Java: a `PlacementFilter` whose `shouldPlace` keeps the origin when the
//! column's water depth — `WORLD_SURFACE` height minus `OCEAN_FLOOR` height —
//! is at most `this.maxWaterDepth`, and whose `type()` is
//! `PlacementModifierType.SURFACE_WATER_DEPTH_FILTER`. Its `CODEC` is the
//! required `"max_water_depth"` int field mapped onto the private constructor
//! (`SurfaceWaterDepthFilter::new`) and the `maxWaterDepth` getter.
//!
//! `maxWaterDepth` is private in Java, so there is no public getter; the port
//! mirrors that (only the codec reads it, via the map codec's `from` closure).
//!
//! The two height reads are `PlacementContext.getHeight`, which delegates to
//! the `WorldGenLevel::get_height_at` trait-default seam (the `#228`-deferred
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

/// `net.minecraft.world.level.levelgen.placement.SurfaceWaterDepthFilter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceWaterDepthFilter {
    /// `this.maxWaterDepth` — the inclusive maximum column water depth.
    max_water_depth: i32,
}

impl SurfaceWaterDepthFilter {
    /// `SurfaceWaterDepthFilter(int)` — the private constructor.
    fn new(max_water_depth: i32) -> Self {
        SurfaceWaterDepthFilter { max_water_depth }
    }

    /// `forMaxDepth(int)` — the public factory.
    pub fn for_max_depth(max_water_depth: i32) -> Self {
        SurfaceWaterDepthFilter::new(max_water_depth)
    }
}

impl PlacementFilter for SurfaceWaterDepthFilter {
    fn should_place<R: RandomSource>(
        &self,
        context: &PlacementContext,
        _random: &mut R,
        origin: &BlockPos,
    ) -> bool {
        // `PlacementContext.getHeight(Heightmap.Types.OCEAN_FLOOR, ...)` /
        // `getHeight(Heightmap.Types.WORLD_SURFACE, ...)` — the
        // `#228`-deferred `LevelReader.getHeight` heightmap reads.
        let y_ocean_floor = context.get_height(Types::OceanFloor, origin.get_x(), origin.get_z());
        let y_surface_floor =
            context.get_height(Types::WorldSurface, origin.get_x(), origin.get_z());
        y_surface_floor.wrapping_sub(y_ocean_floor) <= self.max_water_depth
    }

    fn type_id(&self) -> PlacementModifierTypeId {
        PlacementModifierTypes::SURFACE_WATER_DEPTH_FILTER
    }
}

/// `SurfaceWaterDepthFilter.CODEC` — `Codec.INT.fieldOf("max_water_depth")`
/// mapped onto the private constructor and the `maxWaterDepth` getter
/// (`xmap(SurfaceWaterDepthFilter::new, c -> c.maxWaterDepth)`), as the
/// ops-generic `surface_water_depth_filter_map_codec::<Ops>()` factory.
pub fn surface_water_depth_filter_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<SurfaceWaterDepthFilter, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of_named(
                Arc::new(|c: &SurfaceWaterDepthFilter| c.max_water_depth),
                "max_water_depth".to_string(),
                codec::int_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|max_water_depth: i32| SurfaceWaterDepthFilter::new(max_water_depth)),
            )
    })
}

/// `SurfaceWaterDepthFilter.CODEC` as a `Codec`
/// (`MapCodec.codec()` — `record.codec()`), the shape the `#181` generated
/// dispatch's registration table consumes.
#[allow(dead_code)]
pub fn surface_water_depth_filter_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SurfaceWaterDepthFilter, Ops>> {
    map_codec::codec_of(surface_water_depth_filter_map_codec::<Ops>())
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
    /// column-height reads answer through a per-column `(ocean, surface)` pair,
    /// so the filter's band math is exercised against real heights (the `#228`
    /// default seam is deferral-only).
    struct TestLevel {
        accessor: SimpleLevelHeightAccessor,
        ocean_floor: i32,
        world_surface: i32,
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

        fn get_height_at(&self, ty: Types, _x: i32, _z: i32) -> i32 {
            match ty {
                Types::OceanFloor => self.ocean_floor,
                Types::WorldSurface => self.world_surface,
                other => panic!("unexpected heightmap type in water-depth test: {other:?}"),
            }
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

    /// `PlacementModifier::get_positions` on a `SurfaceWaterDepthFilter` over
    /// the overworld window — the blanket `PlacementFilter` shell.
    fn filter_positions(
        filter: &SurfaceWaterDepthFilter,
        level: &mut TestLevel,
        random: &mut LegacyRandomSource,
        origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let generator = NoopGenerator;
        let context = PlacementContext::new(level, &generator, None);
        PlacementModifier::get_positions(filter, &context, random, origin).collect()
    }

    #[test]
    fn water_depth_filter_keeps_origin_when_depth_is_at_most_max() {
        // `ySurfaceFloor - yOceanFloor <= maxWaterDepth` — a column with
        // water depth 4 (surface 63, ocean floor 59) passes `max 5` and keeps
        // the origin.
        let origin = BlockPos::new(1, 63, 3);
        let filter = SurfaceWaterDepthFilter::for_max_depth(5);
        let mut level = TestLevel {
            accessor: create(-64, 384),
            ocean_floor: 59,
            world_surface: 63,
        };
        let mut random = LegacyRandomSource::new(0);
        let result = filter_positions(&filter, &mut level, &mut random, &origin);
        assert_eq!(result, vec![origin]);
    }

    #[test]
    fn water_depth_filter_drops_origin_when_depth_exceeds_max() {
        // Depth 6 (surface 63, ocean floor 57) fails `max 5` and drops the
        // origin.
        let origin = BlockPos::new(1, 63, 3);
        let filter = SurfaceWaterDepthFilter::for_max_depth(5);
        let mut level = TestLevel {
            accessor: create(-64, 384),
            ocean_floor: 57,
            world_surface: 63,
        };
        let mut random = LegacyRandomSource::new(0);
        let result = filter_positions(&filter, &mut level, &mut random, &origin);
        assert!(result.is_empty());
    }

    #[test]
    fn water_depth_filter_boundary_is_inclusive() {
        // Depth exactly `max 5` passes (`<=`).
        let origin = BlockPos::new(1, 63, 3);
        let filter = SurfaceWaterDepthFilter::for_max_depth(5);
        let mut level = TestLevel {
            accessor: create(-64, 384),
            ocean_floor: 58,
            world_surface: 63,
        };
        let mut random = LegacyRandomSource::new(0);
        let result = filter_positions(&filter, &mut level, &mut random, &origin);
        assert_eq!(result, vec![origin]);
    }

    #[test]
    fn surface_water_depth_type_identity_is_reported() {
        // `PlacementModifierType.SURFACE_WATER_DEPTH_FILTER` is insertion
        // index 3 in `PlacementModifierType.java`'s registration order.
        let filter = SurfaceWaterDepthFilter::for_max_depth(7);
        assert_eq!(
            PlacementFilter::type_id(&filter),
            PlacementModifierTypes::SURFACE_WATER_DEPTH_FILTER
        );
    }

    #[test]
    fn codec_round_trips_max_water_depth() {
        // `Codec.INT.fieldOf("max_water_depth")`: the field is required and
        // encodes back.
        let ops = JsonOps::INSTANCE;
        let codec = surface_water_depth_filter_codec::<JsonOps>();
        let filter = SurfaceWaterDepthFilter::for_max_depth(4);
        let encoded = codec
            .encode_start(&ops, &filter)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"max_water_depth": 4}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .copied()
            .expect("decode should succeed");
        assert_eq!(decoded, filter);
    }

    #[test]
    fn codec_missing_max_water_depth_field_errors() {
        let ops = JsonOps::INSTANCE;
        let codec = surface_water_depth_filter_codec::<JsonOps>();
        let result = codec.parse(&ops, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key max_water_depth"), "got: {msg}");
    }
}

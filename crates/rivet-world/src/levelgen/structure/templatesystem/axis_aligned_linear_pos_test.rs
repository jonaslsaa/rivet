//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.AxisAlignedLinearPosTest`
//! (class, 26.2).
//!
//! Java: `LinearPosTest` constrained to a single axis — the distance is the
//! axis-projected Manhattan distance (the perpendicular components are zeroed by
//! the direction step), and the chance ramps linearly with it, clamped into
//! `[minChance, maxChance]`. Its `CODEC` is a `RecordCodecBuilder` over the four
//! `LinearPosTest` fields plus `Direction.Axis.CODEC.optionalFieldOf("axis",
//! Direction.Axis.Y)`, and the constructor throws
//! `IllegalArgumentException("Invalid range: [min,max]")` when
//! `minDist >= maxDist`. The `test`:
//!
//! ```text
//! Direction direction = Direction.get(Direction.AxisDirection.POSITIVE, this.axis);
//! float xd = Math.abs((worldPos.getX() - worldReference.getX()) * direction.getStepX());
//! float yd = Math.abs((worldPos.getY() - worldReference.getY()) * direction.getStepY());
//! float zd = Math.abs((worldPos.getZ() - worldReference.getZ()) * direction.getStepZ());
//! int dist = (int)(xd + yd + zd);
//! float rnd = random.nextFloat();
//! return rnd <= Mth.clampedLerp(Mth.inverseLerp(dist, minDist, maxDist), minChance, maxChance);
//! ```
//!
//! The codec is ported here (as the ops-generic
//! `axis_aligned_linear_pos_test_map_codec::<Ops>()` factory) and lifted to the
//! erased carrier in `pos_rule_test`.

use crate::levelgen::structure::templatesystem::pos_rule_test::PosRuleTest;
use crate::levelgen::structure::templatesystem::pos_rule_test_type::{
    PosRuleTestTypeId, PosRuleTestTypes,
};
use rivet_registry::core::BlockPos;
use rivet_registry::core::{Axis, AxisDirection, Direction};
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.AxisAlignedLinearPosTest`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisAlignedLinearPosTest {
    /// `minChance` — the chance at `minDist`.
    pub min_chance: f32,
    /// `maxChance` — the chance at `maxDist`.
    pub max_chance: f32,
    /// `minDist` — the distance of the `minChance` ramp endpoint.
    pub min_dist: i32,
    /// `maxDist` — the distance of the `maxChance` ramp endpoint.
    pub max_dist: i32,
    /// `axis` — the axis the projected distance is measured along.
    pub axis: Axis,
}

impl AxisAlignedLinearPosTest {
    /// `new AxisAlignedLinearPosTest(float, float, int, int, Direction.Axis)` —
    /// throws `IllegalArgumentException("Invalid range: [minDist,maxDist]")` when
    /// `minDist >= maxDist` (ported as a panic with the exact message).
    pub fn new(min_chance: f32, max_chance: f32, min_dist: i32, max_dist: i32, axis: Axis) -> Self {
        if min_dist >= max_dist {
            panic!("Invalid range: [{min_dist},{max_dist}]");
        }
        AxisAlignedLinearPosTest {
            min_chance,
            max_chance,
            min_dist,
            max_dist,
            axis,
        }
    }
}

impl PosRuleTest for AxisAlignedLinearPosTest {
    /// `AxisAlignedLinearPosTest.test` — the axis-projected Manhattan distance
    /// (`Direction.get(Direction.AxisDirection.POSITIVE, axis)` zeroes the two
    /// perpendicular components via `getStepX/Y/Z`), then the same
    /// `clampedLerp(inverseLerp(dist, minDist, maxDist), minChance, maxChance)`
    /// ramp as `LinearPosTest`.
    fn test<R: RandomSource>(
        &self,
        _in_template_pos: &BlockPos,
        world_pos: &BlockPos,
        world_reference: &BlockPos,
        random: &mut R,
    ) -> bool {
        let direction = Direction::from_axis_and_direction(self.axis, AxisDirection::Positive);
        let xd = ((world_pos.get_x().wrapping_sub(world_reference.get_x()))
            .wrapping_mul(direction.step_x()))
        .wrapping_abs() as f32;
        let yd = ((world_pos.get_y().wrapping_sub(world_reference.get_y()))
            .wrapping_mul(direction.step_y()))
        .wrapping_abs() as f32;
        let zd = ((world_pos.get_z().wrapping_sub(world_reference.get_z()))
            .wrapping_mul(direction.step_z()))
        .wrapping_abs() as f32;
        let dist = (xd + yd + zd) as i32;
        let rnd = random.next_float();
        rnd <= mth::clamped_lerp_f32(
            mth::inverse_lerp_f32(dist as f32, self.min_dist as f32, self.max_dist as f32),
            self.min_chance,
            self.max_chance,
        )
    }

    fn type_id(&self) -> PosRuleTestTypeId {
        PosRuleTestTypes::AXIS_ALIGNED_LINEAR_POS_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `AxisAlignedLinearPosTest.CODEC` — the record codec over the four defaulted
/// optional fields plus `axis` (default `Axis::Y`), as the ops-generic
/// `axis_aligned_linear_pos_test_map_codec::<Ops>()` factory.
pub fn axis_aligned_linear_pos_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<AxisAlignedLinearPosTest, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &AxisAlignedLinearPosTest| t.min_chance),
                codec::optional_field_of::<f32, Ops>(
                    "min_chance",
                    codec::float_codec::<Ops>(),
                    0.0,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &AxisAlignedLinearPosTest| t.max_chance),
                codec::optional_field_of::<f32, Ops>(
                    "max_chance",
                    codec::float_codec::<Ops>(),
                    0.0,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &AxisAlignedLinearPosTest| t.min_dist),
                codec::optional_field_of::<i32, Ops>("min_dist", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &AxisAlignedLinearPosTest| t.max_dist),
                codec::optional_field_of::<i32, Ops>("max_dist", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &AxisAlignedLinearPosTest| t.axis),
                codec::optional_field_of::<Axis, Ops>(
                    "axis",
                    crate::levelgen::structure::templatesystem::axis_codec::axis_codec::<Ops>(),
                    Axis::Y,
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |min_chance: f32, max_chance: f32, min_dist: i32, max_dist: i32, axis: Axis| {
                        AxisAlignedLinearPosTest::new(
                            min_chance, max_chance, min_dist, max_dist, axis,
                        )
                    },
                ),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::structure::templatesystem::codec_test_util;
    use serde_json::json;
    use std::panic;

    #[test]
    fn invalid_range_panics_like_java() {
        let result = panic::catch_unwind(|| AxisAlignedLinearPosTest::new(0.0, 0.0, 3, 3, Axis::Y));
        assert!(result.is_err());
        let msg = result
            .err()
            .and_then(|e| e.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert_eq!(msg, "Invalid range: [3,3]");
        let result = panic::catch_unwind(|| AxisAlignedLinearPosTest::new(0.0, 0.0, 4, 3, Axis::Y));
        assert!(result.is_err());
    }

    #[test]
    fn boundary_ok() {
        let t = AxisAlignedLinearPosTest::new(0.0, 1.0, 0, 10, Axis::Z);
        assert_eq!(t.min_chance, 0.0);
        assert_eq!(t.max_chance, 1.0);
        assert_eq!(t.min_dist, 0);
        assert_eq!(t.max_dist, 10);
        assert_eq!(t.axis, Axis::Z);
    }

    /// `test` with a fixed seed — Java stream parity (the draw is the same
    /// `nextFloat` of `LegacyRandomSource(0)` at `dist 0`).
    fn test_at_min_dist(min_chance: f32, max_chance: f32) -> bool {
        let t = AxisAlignedLinearPosTest::new(min_chance, max_chance, 0, 10, Axis::Y);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        t.test(
            &BlockPos::ZERO,
            &BlockPos::ZERO,
            &BlockPos::ZERO,
            &mut random,
        )
    }

    #[test]
    fn test_at_min_dist_uses_min_chance() {
        // dist == minDist (0) → inverseLerp 0 → clampedLerp(minChance). The
        // first `LegacyRandomSource(0)` draw is 0.73096776, so the min-chance
        // endpoint decides: 1.0 always passes, 0.5 fails.
        assert!(test_at_min_dist(1.0, 1.0));
        assert!(!test_at_min_dist(0.5, 1.0));
    }

    #[test]
    fn test_at_or_beyond_max_dist_uses_max_chance() {
        // dist >= maxDist → clampedLerp caps at maxChance (1.0) → always true.
        let t = AxisAlignedLinearPosTest::new(0.0, 1.0, 0, 10, Axis::X);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let far = BlockPos::new(100, 0, 0);
        assert!(t.test(&BlockPos::ZERO, &far, &BlockPos::ZERO, &mut random));
    }

    #[test]
    fn test_axis_projects_the_distance() {
        // An X-axis test is insensitive to Y/Z separation: moving 5 blocks on Z
        // with an X axis leaves the distance at 0, so the truth value equals the
        // at-min-dist value (same seed draw).
        let t = AxisAlignedLinearPosTest::new(0.0, 1.0, 0, 10, Axis::X);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let z_only = BlockPos::new(0, 0, 5);
        let same_draw = t.test(&BlockPos::ZERO, &z_only, &BlockPos::ZERO, &mut random);
        assert_eq!(same_draw, test_at_min_dist(0.0, 1.0));
    }

    #[test]
    fn codec_round_trips_with_axis() {
        let codec = codec_test_util::codec(axis_aligned_linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = AxisAlignedLinearPosTest::new(0.2, 0.8, 1, 5, Axis::X);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(
            encoded,
            json!({"min_chance": 0.2, "max_chance": 0.8, "min_dist": 1, "max_dist": 5, "axis": "x"})
        );
        assert_eq!(codec_test_util::decode(&codec, &encoded), t);
    }

    #[test]
    fn codec_defaults_axis_to_y() {
        // The `axis` field is `optionalFieldOf("axis", Axis.Y)`: absent → Y,
        // and Y is omitted on encode. The other default-valued fields
        // (`min_chance` 0.0, `min_dist` 0) are omitted the same way.
        let codec = codec_test_util::codec(axis_aligned_linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = AxisAlignedLinearPosTest::new(0.0, 1.0, 0, 10, Axis::Y);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(encoded, json!({"max_chance": 1.0, "max_dist": 10}));
        let decoded = codec_test_util::decode(&codec, &encoded);
        assert_eq!(decoded.axis, Axis::Y);
    }

    #[test]
    fn codec_rejects_invalid_range() {
        // The constructor panic propagates out of decode (Java throws the
        // `IllegalArgumentException` from the apply function), so a decode that
        // yields an invalid range must panic, not return a DataResult error.
        let codec = codec_test_util::codec(axis_aligned_linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        codec_test_util::decode_unwind(
            codec,
            json!({"min_chance": 0.0, "max_chance": 1.0, "min_dist": 5, "max_dist": 5}),
        );
    }
}

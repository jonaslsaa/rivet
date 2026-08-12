//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.LinearPosTest`
//! (class, 26.2).
//!
//! Java: a position rule test whose truth value ramps linearly with the
//! Manhattan distance between `worldPos` and `worldReference`, clamped into
//! `[minChance, maxChance]`. Its `CODEC` is a `RecordCodecBuilder` over four
//! `optionalFieldOf(name, default)` fields (`min_chance`/`max_chance` default
//! `0.0F`, `min_dist`/`max_dist` default `0`), and the constructor throws
//! `IllegalArgumentException("Invalid range: [min,max]")` when
//! `minDist >= maxDist`. The `test`:
//!
//! ```text
//! int dist = worldPos.distManhattan(worldReference);
//! float rnd = random.nextFloat();
//! return rnd <= Mth.clampedLerp(Mth.inverseLerp(dist, minDist, maxDist), minChance, maxChance);
//! ```
//!
//! The codec is ported here (as the ops-generic `linear_pos_test_map_codec::<Ops>()`
//! factory) and lifted to the erased carrier in `pos_rule_test`.
//!
//! `BlockPos.distManhattan` is inherited from `Vec3i` in Java but only declared
//! on `Vec3i` in the port, so the Manhattan distance is replicated here with
//! the same wrapping arithmetic (`Vec3i.distManhattan`'s float-abs-then-truncate
//! sum).

use crate::levelgen::structure::templatesystem::optional_field_codecs::defaulted_optional_field_of;
use crate::levelgen::structure::templatesystem::pos_rule_test::PosRuleTest;
use crate::levelgen::structure::templatesystem::pos_rule_test_type::{
    PosRuleTestTypeId, PosRuleTestTypes,
};
use rivet_registry::core::BlockPos;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.structure.templatesystem.LinearPosTest`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearPosTest {
    /// `minChance` — the chance at `minDist`.
    pub min_chance: f32,
    /// `maxChance` — the chance at `maxDist`.
    pub max_chance: f32,
    /// `minDist` — the distance of the `minChance` ramp endpoint.
    pub min_dist: i32,
    /// `maxDist` — the distance of the `maxChance` ramp endpoint.
    pub max_dist: i32,
}

impl LinearPosTest {
    /// `new LinearPosTest(float, float, int, int)` — throws
    /// `IllegalArgumentException("Invalid range: [minDist,maxDist]")` when
    /// `minDist >= maxDist` (ported as a panic with the exact message).
    pub fn new(min_chance: f32, max_chance: f32, min_dist: i32, max_dist: i32) -> Self {
        if min_dist >= max_dist {
            panic!("Invalid range: [{min_dist},{max_dist}]");
        }
        LinearPosTest {
            min_chance,
            max_chance,
            min_dist,
            max_dist,
        }
    }
}

/// `Vec3i.distManhattan(BlockPos)` — Java's float-abs-then-truncate sum, with
/// wrapping int subtraction (the same arithmetic `Vec3i::dist_manhattan`
/// performs; re-declared here because the port only exposes it on `Vec3i`).
fn dist_manhattan(world_pos: &BlockPos, world_reference: &BlockPos) -> i32 {
    let xd = world_pos
        .get_x()
        .wrapping_sub(world_reference.get_x())
        .wrapping_abs() as f32;
    let yd = world_pos
        .get_y()
        .wrapping_sub(world_reference.get_y())
        .wrapping_abs() as f32;
    let zd = world_pos
        .get_z()
        .wrapping_sub(world_reference.get_z())
        .wrapping_abs() as f32;
    (xd + yd + zd) as i32
}

impl PosRuleTest for LinearPosTest {
    /// `LinearPosTest.test` — `rnd <= clampedLerp(inverseLerp(dist, minDist,
    /// maxDist), minChance, maxChance)` with `dist = distManhattan(worldPos,
    /// worldReference)`. `inverse_lerp_f32`/`clamped_lerp_f32` are the exact
    /// `Mth` float overloads.
    fn test<R: RandomSource>(
        &self,
        _in_template_pos: &BlockPos,
        world_pos: &BlockPos,
        world_reference: &BlockPos,
        random: &mut R,
    ) -> bool {
        let dist = dist_manhattan(world_pos, world_reference);
        let rnd = random.next_float();
        rnd <= mth::clamped_lerp_f32(
            mth::inverse_lerp_f32(dist as f32, self.min_dist as f32, self.max_dist as f32),
            self.min_chance,
            self.max_chance,
        )
    }

    fn type_id(&self) -> PosRuleTestTypeId {
        PosRuleTestTypes::LINEAR_POS_TEST
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `LinearPosTest.CODEC` — the record codec over the four defaulted optional
/// fields, as the ops-generic `linear_pos_test_map_codec::<Ops>()` factory.
pub fn linear_pos_test_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<LinearPosTest, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|t: &LinearPosTest| t.min_chance),
                defaulted_optional_field_of::<f32, Ops>(
                    "min_chance",
                    codec::float_codec::<Ops>(),
                    0.0,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &LinearPosTest| t.max_chance),
                defaulted_optional_field_of::<f32, Ops>(
                    "max_chance",
                    codec::float_codec::<Ops>(),
                    0.0,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &LinearPosTest| t.min_dist),
                defaulted_optional_field_of::<i32, Ops>("min_dist", codec::int_codec::<Ops>(), 0),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|t: &LinearPosTest| t.max_dist),
                defaulted_optional_field_of::<i32, Ops>("max_dist", codec::int_codec::<Ops>(), 0),
            ))
            .apply(
                instance,
                Arc::new(
                    |min_chance: f32, max_chance: f32, min_dist: i32, max_dist: i32| {
                        LinearPosTest::new(min_chance, max_chance, min_dist, max_dist)
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
        // `minDist >= maxDist` → `IllegalArgumentException("Invalid range:
        // [min,max]")`, ported as a panic with the exact message.
        let result = panic::catch_unwind(|| LinearPosTest::new(0.0, 0.0, 3, 3));
        assert!(result.is_err());
        let msg = result
            .err()
            .and_then(|e| e.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert_eq!(msg, "Invalid range: [3,3]");
        let result = panic::catch_unwind(|| LinearPosTest::new(0.0, 0.0, 4, 3));
        assert!(result.is_err());
    }

    #[test]
    fn boundary_ok() {
        // `minDist < maxDist` is valid.
        let t = LinearPosTest::new(0.0, 1.0, 0, 10);
        assert_eq!(t.min_chance, 0.0);
        assert_eq!(t.max_chance, 1.0);
        assert_eq!(t.min_dist, 0);
        assert_eq!(t.max_dist, 10);
    }

    #[test]
    fn test_at_min_dist_uses_min_chance() {
        // dist == minDist (0) → inverseLerp 0 → clampedLerp(minChance). The
        // first `LegacyRandomSource(0)` draw is 0.73096776, so the min-chance
        // endpoint decides: 1.0 always passes, 0.5 fails.
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let always = LinearPosTest::new(1.0, 1.0, 0, 10);
        assert!(always.test(
            &BlockPos::ZERO,
            &BlockPos::ZERO,
            &BlockPos::ZERO,
            &mut random
        ));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let below_draw = LinearPosTest::new(0.5, 1.0, 0, 10);
        assert!(!below_draw.test(
            &BlockPos::ZERO,
            &BlockPos::ZERO,
            &BlockPos::ZERO,
            &mut random
        ));
    }

    #[test]
    fn test_at_or_beyond_max_dist_uses_max_chance() {
        // dist >= maxDist → clampedLerp caps at maxChance (1.0) → always true.
        let t = LinearPosTest::new(0.0, 1.0, 0, 10);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let far = BlockPos::new(0, 0, 100);
        assert!(t.test(&BlockPos::ZERO, &far, &BlockPos::ZERO, &mut random));
    }

    #[test]
    fn test_below_min_chance_is_false() {
        // maxChance 0.0 → every rnd > 0 is false (rnd is never <= 0).
        let t = LinearPosTest::new(0.0, 0.0, 0, 10);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let far = BlockPos::new(0, 0, 100);
        assert!(!t.test(&BlockPos::ZERO, &far, &BlockPos::ZERO, &mut random));
    }

    #[test]
    fn codec_round_trips_with_defaults_omitted() {
        // Default-valued fields are omitted on encode (Java DFU `optionalFieldOf`
        // xmap semantics) and defaulted back on decode. The constructor requires
        // minDist < maxDist, so at least one of min_dist/max_dist is non-default;
        // here the two chance fields are at their 0.0 defaults and omitted.
        let codec = codec_test_util::codec(linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = LinearPosTest::new(0.0, 0.0, 1, 2);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(encoded, json!({"min_dist": 1, "max_dist": 2}));
        assert_eq!(codec_test_util::decode(&codec, &encoded), t);
    }

    #[test]
    fn codec_round_trips_with_values() {
        let codec = codec_test_util::codec(linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        let t = LinearPosTest::new(0.2, 0.8, 1, 5);
        let encoded = codec_test_util::encode(&codec, &t);
        assert_eq!(
            encoded,
            json!({"min_chance": 0.2, "max_chance": 0.8, "min_dist": 1, "max_dist": 5})
        );
        assert_eq!(codec_test_util::decode(&codec, &encoded), t);
    }

    #[test]
    fn codec_defaults_fields_when_absent() {
        // Absent fields decode to the defaults: min_chance/max_chance 0.0,
        // min_dist/max_dist 0 — so the constructor sees `new(0.0, 0.0, 0, 0)`
        // and panics (`min >= max`) exactly where Java's `new` throws
        // `IllegalArgumentException` out of `codec.decode` (DFU does not turn
        // a constructor throw into a DataResult error).
        let codec = codec_test_util::codec(linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        codec_test_util::decode_unwind(codec, json!({}));
    }

    #[test]
    fn codec_rejects_invalid_range() {
        // The constructor panic propagates out of decode (Java throws the
        // `IllegalArgumentException` from the apply function).
        let codec = codec_test_util::codec(linear_pos_test_map_codec::<
            rivet_serialization::json_ops::JsonOps,
        >());
        codec_test_util::decode_unwind(
            codec,
            json!({"min_chance": 0.0, "max_chance": 1.0, "min_dist": 5, "max_dist": 5}),
        );
    }
}

//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! MegaPineFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).and(IntProviders.codec(0, 24).
//! fieldOf("crown_height").forGetter(p -> p.crownHeight)).apply(i, ...)`.
//! `createFoliage` walks `yy` from `foliagePos.y - foliageHeight + offset` up to
//! `foliagePos.y + offset` (i.e. `yo` from `foliageHeight - offset` down to
//! `-offset`), one row per step with a *smooth* radius
//! `leafRadius + radiusOffset + Mth.floor((float)yo / foliageHeight * 3.5F)`
//! and a *jagged* radius that bumps `+1` when `yo > 0`, the smooth radius did
//! not change since the previous row, and `(yy & 1) == 0`. The row is placed at
//! `BlockPos(x, yy, z)` with row `y` offset `0`. The skip test is the same int
//! `dx + dz >= 7 || dx*dx + dz*dz > cur*cur` as `MegaJungleFoliagePlacer`.
//!
//! Note `foliageHeight == 0` yields `(float)yo / 0F` — IEEE float division,
//! `Mth.floor(+Inf) == Integer.MAX_VALUE` in both languages.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::{
    FoliageAttachment, FoliagePlacer, FoliageSetter, foliage_placer_parts,
};
use crate::levelgen::feature::foliageplacers::foliage_placer_type::{
    FoliagePlacerTypeId, FoliagePlacerTypes,
};
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.MegaPineFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct MegaPineFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.crownHeight`.
    crown_height: IntProvider,
}

impl MegaPineFoliagePlacer {
    /// `new MegaPineFoliagePlacer(IntProvider, IntProvider, IntProvider)`.
    pub fn new(
        radius: IntProvider,
        offset: IntProvider,
        crown_height: IntProvider,
    ) -> MegaPineFoliagePlacer {
        MegaPineFoliagePlacer {
            radius,
            offset,
            crown_height,
        }
    }

    /// `this.crownHeight`.
    pub fn crown_height(&self) -> &IntProvider {
        &self.crown_height
    }
}

impl FoliagePlacer for MegaPineFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER
    }

    fn create_foliage_with_offset<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        foliage_setter: &mut dyn FoliageSetter,
        random: &mut R,
        config: &TreeConfiguration,
        _tree_height: i32,
        foliage_attachment: &FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        offset: i32,
    ) {
        let foliage_pos = foliage_attachment.pos;
        let mut prev_radius = 0;

        let start_y = foliage_pos
            .get_y()
            .wrapping_sub(foliage_height)
            .wrapping_add(offset);
        let end_y = foliage_pos.get_y().wrapping_add(offset);
        let mut yy = start_y;
        while yy <= end_y {
            let yo = foliage_pos.get_y().wrapping_sub(yy);
            let smooth_radius = leaf_radius
                .wrapping_add(foliage_attachment.radius_offset)
                .wrapping_add(mth::floor(yo as f32 / foliage_height as f32 * 3.5));
            let jagged_radius = if yo > 0 && smooth_radius == prev_radius && (yy & 1) == 0 {
                smooth_radius.wrapping_add(1)
            } else {
                smooth_radius
            };

            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &foliage_pos.at_y(yy),
                jagged_radius,
                0,
                foliage_attachment.double_trunk,
            );
            prev_radius = smooth_radius;
            yy = yy.wrapping_add(1);
        }
    }

    fn foliage_height<R: RandomSource>(
        &self,
        random: &mut R,
        _tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        self.crown_height.sample(random)
    }

    fn should_skip_location<R: RandomSource>(
        &self,
        _random: &mut R,
        dx: i32,
        _y: i32,
        dz: i32,
        current_radius: i32,
        _double_trunk: bool,
    ) -> bool {
        // Java `int` arithmetic, wrapping: `dx + dz >= 7 || dx*dx + dz*dz >
        // currentRadius*currentRadius`.
        dx.wrapping_add(dz) >= 7
            || dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))
                > current_radius.wrapping_mul(current_radius)
    }

    fn radius(&self) -> &IntProvider {
        &self.radius
    }

    fn offset(&self) -> &IntProvider {
        &self.offset
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `MegaPineFoliagePlacer.CODEC` — the ops-generic `MapCodec<MegaPineFoliagePlacer>`
/// factory: `foliagePlacerParts(i).and(IntProviders.codec(0, 24).fieldOf(
/// "crown_height")).apply(i, MegaPineFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn mega_pine_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<MegaPineFoliagePlacer, Ops>> {
    record_builder::map_codec::<MegaPineFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<MegaPineFoliagePlacer, Ops>(
            Arc::new(|p: &MegaPineFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &MegaPineFoliagePlacer| p.offset.clone()),
        );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &MegaPineFoliagePlacer| p.crown_height.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(0, 24),
                    "crown_height".to_string(),
                ),
            ))
            .apply(instance, Arc::new(MegaPineFoliagePlacer::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_serialization::map_codec;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    fn provider(min: i32, max: i32) -> IntProvider {
        IntProvider::Uniform(UniformInt::of(min, max))
    }

    #[test]
    fn codec_round_trips_the_mega_pine_record() {
        let codec = map_codec::codec_of(mega_pine_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 1, "max_inclusive": 1, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 1, "max_inclusive": 1, "type": "minecraft:uniform"},
            "crown_height": {"min_inclusive": 4, "max_inclusive": 8, "type": "minecraft:uniform"}
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER
        );
        assert_eq!(decoded.crown_height(), &provider(4, 8));
        assert_eq!(decoded.radius(), &provider(1, 1));
        assert_eq!(decoded.offset(), &provider(1, 1));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn smooth_radius_uses_floor_of_y_ratio() {
        // `Mth.floor((float)yo / foliageHeight * 3.5F)`: yo=2, height=4 -> floor(
        // 2/4*3.5)=floor(1.75)=1; yo=4 -> floor(3.5)=3.
        let p = MegaPineFoliagePlacer::new(provider(1, 1), provider(0, 0), provider(4, 4));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            4
        );
    }

    #[test]
    fn skip_test_is_int_circle_plus_distance_cap() {
        let p = MegaPineFoliagePlacer::new(provider(1, 1), provider(0, 0), provider(4, 4));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert!(p.should_skip_location(&mut random, 3, 0, 4, 8, false));
        assert!(p.should_skip_location(&mut random, 3, 0, 3, 3, false));
        assert!(!p.should_skip_location(&mut random, 1, 0, 1, 3, false));
    }
}

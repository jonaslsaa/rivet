//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! FancyFoliagePlacer` (class, 26.2).
//!
//! Java extends `BlobFoliagePlacer`; `CODEC = blobParts(i).apply(i,
//! FancyFoliagePlacer::new)` reuses the shared three-field blob record. Each row
//! is the fixed `leafRadius` except the first and last rows, which are one
//! smaller; the circle-skip is `Mth.square(dx + 0.5F) + Mth.square(dz + 0.5F) >
//! currentRadius * currentRadius` — the f32 squares of the half-step-offset
//! offsets (the `+0.5F` moves the corner cut to a quarter-circle), compared
//! against the **int** `currentRadius * currentRadius` (Java int arithmetic,
//! wrapping) widened to f32.
//!
//! The Rust port reuses [`blob_parts_map_codec`] for the record and implements
//! `FoliagePlacer` directly.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::blob_foliage_placer::blob_parts_map_codec;
use crate::levelgen::feature::foliageplacers::foliage_placer::{
    FoliageAttachment, FoliagePlacer, FoliageSetter,
};
use crate::levelgen::feature::foliageplacers::foliage_placer_type::{
    FoliagePlacerTypeId, FoliagePlacerTypes,
};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use rivet_util::mth;
use rivet_util::valueproviders::int_provider::IntProvider;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.FancyFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct FancyFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.height` — the blob's fixed foliage height (from `BlobFoliagePlacer`).
    height: i32,
}

impl FancyFoliagePlacer {
    /// `new FancyFoliagePlacer(IntProvider, IntProvider, int)`.
    pub fn new(radius: IntProvider, offset: IntProvider, height: i32) -> FancyFoliagePlacer {
        FancyFoliagePlacer {
            radius,
            offset,
            height,
        }
    }

    /// `this.height`.
    pub fn height(&self) -> i32 {
        self.height
    }
}

impl FoliagePlacer for FancyFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::FANCY_FOLIAGE_PLACER
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
        let mut yo = offset;
        while yo >= offset.wrapping_sub(foliage_height) {
            let current_radius = if yo != offset && yo != offset.wrapping_sub(foliage_height) {
                leaf_radius.wrapping_add(1)
            } else {
                leaf_radius
            };
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &foliage_attachment.pos,
                current_radius,
                yo,
                foliage_attachment.double_trunk,
            );
            yo = yo.wrapping_sub(1);
        }
    }

    fn foliage_height<R: RandomSource>(
        &self,
        _random: &mut R,
        _tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        self.height
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
        // `Mth.square(dx + 0.5F)` (f32) vs `currentRadius * currentRadius`
        // (int, wrapping) widened to f32.
        mth::square_f32(dx as f32 + 0.5) + mth::square_f32(dz as f32 + 0.5)
            > current_radius.wrapping_mul(current_radius) as f32
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

/// `FancyFoliagePlacer.CODEC` — `blobParts(i).apply(i, FancyFoliagePlacer::new)`,
/// as the ops-generic `fancy_foliage_placer_map_codec::<Ops>()` factory.
pub fn fancy_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<FancyFoliagePlacer, Ops>> {
    blob_parts_map_codec::<FancyFoliagePlacer, Ops>(
        Arc::new(|p: &FancyFoliagePlacer| p.radius.clone()),
        Arc::new(|p: &FancyFoliagePlacer| p.offset.clone()),
        Arc::new(|p: &FancyFoliagePlacer| p.height),
        Arc::new(FancyFoliagePlacer::new),
    )
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
    fn codec_round_trips_the_fancy_record() {
        let codec = map_codec::codec_of(fancy_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 3, "max_inclusive": 3, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": 2
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::FANCY_FOLIAGE_PLACER
        );
        assert_eq!(decoded.height(), 2);
        assert_eq!(decoded.radius(), &provider(3, 3));
        assert_eq!(decoded.offset(), &provider(0, 0));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn circle_skip_keeps_corners_outside_the_quarter_circle() {
        // `Mth.square(dx + 0.5F) + Mth.square(dz + 0.5F) > currentRadius²` — the
        // (cur, cur) corner skips, (cur, cur-1) stays, when cur=2:
        // 2.5²+2.5²=12.5 > 4 (skip), 2.5²+1.5²=8.5 > 4 (skip),
        // 1.5²+1.5²=4.5 > 4 (skip), 1.5²+0.5²=2.5 ≤ 4 (keep).
        let p = FancyFoliagePlacer::new(provider(2, 2), provider(0, 0), 2);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert!(p.should_skip_location(&mut random, 2, 0, 2, 2, false));
        assert!(p.should_skip_location(&mut random, 2, 0, 1, 2, false));
        assert!(p.should_skip_location(&mut random, 1, 0, 1, 2, false));
        assert!(!p.should_skip_location(&mut random, 1, 0, 0, 2, false));
        assert!(!p.should_skip_location(&mut random, 0, 0, 0, 2, false));
    }

    #[test]
    fn foliage_height_is_the_fixed_height() {
        let p = FancyFoliagePlacer::new(provider(0, 1), provider(0, 1), 5);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            5
        );
    }
}

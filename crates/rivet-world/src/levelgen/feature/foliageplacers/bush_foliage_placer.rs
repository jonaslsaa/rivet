//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! BushFoliagePlacer` (class, 26.2).
//!
//! Java extends `BlobFoliagePlacer`; `CODEC = blobParts(i).apply(i,
//! BushFoliagePlacer::new)` reuses the shared three-field blob record
//! (`foliagePlacerParts` + the `"height"` int field). Unlike the tapered blob,
//! the bush's `currentRadius` drops by `yo` each row (no `/2` taper): `leafRadius
//! + radiusOffset - 1 - yo`, and the corner-skip is a pure `random.nextInt(2)`
//! coin flip at the radius corners (no `y == 0` exception).
//!
//! The Rust port reuses [`blob_parts_map_codec`] for the record and implements
//! `FoliagePlacer` directly (Java's inherited `radius`/`offset`/`height` fields).

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
use rivet_util::valueproviders::int_provider::IntProvider;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.BushFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct BushFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.height` — the blob's fixed foliage height (from `BlobFoliagePlacer`).
    height: i32,
}

impl BushFoliagePlacer {
    /// `new BushFoliagePlacer(IntProvider, IntProvider, int)`.
    pub fn new(radius: IntProvider, offset: IntProvider, height: i32) -> BushFoliagePlacer {
        BushFoliagePlacer {
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

impl FoliagePlacer for BushFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::BUSH_FOLIAGE_PLACER
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
            let current_radius = leaf_radius
                .wrapping_add(foliage_attachment.radius_offset)
                .wrapping_sub(1)
                .wrapping_sub(yo);
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
        random: &mut R,
        dx: i32,
        _y: i32,
        dz: i32,
        current_radius: i32,
        _double_trunk: bool,
    ) -> bool {
        dx == current_radius && dz == current_radius && random.next_int_bound(2) == 0
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

/// `BushFoliagePlacer.CODEC` — `blobParts(i).apply(i, BushFoliagePlacer::new)`,
/// as the ops-generic `bush_foliage_placer_map_codec::<Ops>()` factory.
pub fn bush_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BushFoliagePlacer, Ops>> {
    blob_parts_map_codec::<BushFoliagePlacer, Ops>(
        Arc::new(|p: &BushFoliagePlacer| p.radius.clone()),
        Arc::new(|p: &BushFoliagePlacer| p.offset.clone()),
        Arc::new(|p: &BushFoliagePlacer| p.height),
        Arc::new(BushFoliagePlacer::new),
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
    fn codec_round_trips_the_bush_record() {
        let codec = map_codec::codec_of(bush_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": 1
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::BUSH_FOLIAGE_PLACER
        );
        assert_eq!(decoded.height(), 1);
        assert_eq!(decoded.radius(), &provider(2, 2));
        assert_eq!(decoded.offset(), &provider(0, 0));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn foliage_height_is_the_fixed_height() {
        let p = BushFoliagePlacer::new(provider(0, 1), provider(0, 1), 5);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            5
        );
    }
}

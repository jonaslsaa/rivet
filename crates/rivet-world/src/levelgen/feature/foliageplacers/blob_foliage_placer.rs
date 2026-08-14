//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! BlobFoliagePlacer` (class, 26.2).
//!
//! Java: a foliage placer that tapers a blob of leaves around the trunk —
//! each row from `offset` down to `offset - foliageHeight` has radius
//! `max(leafRadius + radiusOffset - 1 - yo/2, 0)` (the integer division is
//! Java's truncating `/`), with the `shouldSkipLocation` corner-skip
//! (`dx == dz == currentRadius && (random.nextInt(2) == 0 || y == 0)`)
//! reproducing the classic rounded blob.
//!
//! `CODEC` is the shared three-field blob record — `blobParts(i).apply(i,
//! BlobFoliagePlacer::new)` — where `blobParts` = `foliagePlacerParts(i)
//! .and(Codec.intRange(0, 16).fieldOf("height"))`. The Rust port keeps the
//! shared group as the [`blob_parts_map_codec`] helper (`BlobFoliagePlacer`,
//! `BushFoliagePlacer`, and `FancyFoliagePlacer` all build their `CODEC` from
//! it, exactly like Java's `blobParts`).
//!
//! `BlobFoliagePlacer` is the superclass of `BushFoliagePlacer` and
//! `FancyFoliagePlacer`; since the Rust port models the placer base as the
//! [`FoliagePlacer`] trait, each subclass implements the trait directly with
//! its own fields (Java's inherited `radius`/`offset`/`height`).

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
use rivet_util::valueproviders::int_provider::IntProvider;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.BlobFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct BlobFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.height` — the blob's fixed foliage height.
    height: i32,
}

impl BlobFoliagePlacer {
    /// `new BlobFoliagePlacer(IntProvider, IntProvider, int)`.
    pub fn new(radius: IntProvider, offset: IntProvider, height: i32) -> BlobFoliagePlacer {
        BlobFoliagePlacer {
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

impl FoliagePlacer for BlobFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::BLOB_FOLIAGE_PLACER
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
            let current_radius = std::cmp::max(
                leaf_radius
                    .wrapping_add(foliage_attachment.radius_offset)
                    .wrapping_sub(1)
                    .wrapping_sub(yo / 2),
                0,
            );
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
        y: i32,
        dz: i32,
        current_radius: i32,
        _double_trunk: bool,
    ) -> bool {
        dx == current_radius && dz == current_radius && (random.next_int_bound(2) == 0 || y == 0)
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

/// `BlobFoliagePlacer.blobParts(Instance)` — the shared three-field record
/// group (`foliagePlacerParts` + the `"height"` int field), composed into the
/// `MapCodec<C>` both `BlobFoliagePlacer`, `BushFoliagePlacer`, and
/// `FancyFoliagePlacer` build their `CODEC` from (`blobParts(i).apply(i,
/// Xxx::new)`).
#[allow(clippy::type_complexity)]
pub(crate) fn blob_parts_map_codec<C, Ops>(
    get_radius: Arc<dyn Fn(&C) -> IntProvider + Send + Sync>,
    get_offset: Arc<dyn Fn(&C) -> IntProvider + Send + Sync>,
    get_height: Arc<dyn Fn(&C) -> i32 + Send + Sync>,
    new: Arc<dyn Fn(IntProvider, IntProvider, i32) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<C, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    record_builder::map_codec::<C, Ops>(move |instance| {
        let (radius_builder, offset_builder) =
            foliage_placer_parts::<C, Ops>(get_radius, get_offset);
        instance
            .group(radius_builder)
            .and(offset_builder)
            .and(RecordCodecBuilder::of(
                get_height,
                codec::field_of(codec::int_range::<Ops>(0, 16), "height".to_string()),
            ))
            .apply(instance, new)
    })
}

/// `BlobFoliagePlacer.CODEC` — `RecordCodecBuilder.mapCodec(i ->
/// blobParts(i).apply(i, BlobFoliagePlacer::new))`, as the ops-generic
/// `blob_foliage_placer_map_codec::<Ops>()` factory.
pub fn blob_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BlobFoliagePlacer, Ops>> {
    blob_parts_map_codec::<BlobFoliagePlacer, Ops>(
        Arc::new(|p: &BlobFoliagePlacer| p.radius.clone()),
        Arc::new(|p: &BlobFoliagePlacer| p.offset.clone()),
        Arc::new(|p: &BlobFoliagePlacer| p.height),
        Arc::new(BlobFoliagePlacer::new),
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
    fn codec_round_trips_the_blob_record() {
        let codec = map_codec::codec_of(blob_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 3, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": 3
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::BLOB_FOLIAGE_PLACER
        );
        assert_eq!(decoded.height(), 3);
        assert_eq!(decoded.radius(), &provider(2, 3));
        assert_eq!(decoded.offset(), &provider(0, 0));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_height_out_of_range() {
        // `Codec.intRange(0, 16)` rejects on decode.
        let codec = map_codec::codec_of(blob_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": 2, "offset": 0, "height": 20
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Value 20 outside of range [0:16]"),
            "got: {msg}"
        );
    }

    #[test]
    fn foliage_height_is_the_fixed_height() {
        let p = BlobFoliagePlacer::new(provider(0, 1), provider(0, 1), 5);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            5
        );
    }
}

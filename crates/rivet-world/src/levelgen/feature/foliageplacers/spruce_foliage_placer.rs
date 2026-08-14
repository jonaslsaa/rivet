//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! SpruceFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).and(IntProviders.codec(0, 24).
//! fieldOf("trunk_height").forGetter(p -> p.trunkHeight)).apply(i, ...)` — a
//! two-level taper: the radius cycles through `[minRadius, maxRadius)` over the
//! rows, resuming the climb at `minRadius` whenever it reaches `maxRadius` and
//! capping `maxRadius` at `leafRadius + radiusOffset`. `foliageHeight` is
//! `Math.max(4, treeHeight - trunkHeight.sample(random))` — Java int
//! subtraction wraps, so the port uses `wrapping_sub` on the difference while
//! `Math.max` (a plain comparison, `std::cmp::max`) does not wrap.
//!
//! The Rust port models the `foliagePlacerParts` prefix as the two
//! `RecordCodecBuilder`s returned by the shared [`foliage_placer_parts`] helper
//! (`int_provider_codec_with_bounds(0, 16)` for both `"radius"` and `"offset"`),
//! followed by the `"trunk_height"` field with its `codec(0, 24)` bounds —
//! the faithful `P3` shape of the Java record.

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
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.SpruceFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct SpruceFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.trunkHeight`.
    trunk_height: IntProvider,
}

impl SpruceFoliagePlacer {
    /// `new SpruceFoliagePlacer(IntProvider, IntProvider, IntProvider)`.
    pub fn new(
        radius: IntProvider,
        offset: IntProvider,
        trunk_height: IntProvider,
    ) -> SpruceFoliagePlacer {
        SpruceFoliagePlacer {
            radius,
            offset,
            trunk_height,
        }
    }

    /// `this.trunkHeight`.
    pub fn trunk_height(&self) -> &IntProvider {
        &self.trunk_height
    }
}

impl FoliagePlacer for SpruceFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER
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
        let mut current_radius = random.next_int_bound(2);
        let mut max_radius = 1;
        let mut min_radius = 0;

        let mut yo = offset;
        while yo >= -foliage_height {
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &foliage_pos,
                current_radius,
                yo,
                foliage_attachment.double_trunk,
            );
            if current_radius >= max_radius {
                current_radius = min_radius;
                min_radius = 1;
                max_radius = std::cmp::min(
                    max_radius.wrapping_add(1),
                    leaf_radius.wrapping_add(foliage_attachment.radius_offset),
                );
            } else {
                current_radius = current_radius.wrapping_add(1);
            }
            yo = yo.wrapping_sub(1);
        }
    }

    fn foliage_height<R: RandomSource>(
        &self,
        random: &mut R,
        tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        std::cmp::max(4, tree_height.wrapping_sub(self.trunk_height.sample(random)))
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
        dx == current_radius && dz == current_radius && current_radius > 0
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

/// `SpruceFoliagePlacer.CODEC` — the ops-generic `MapCodec<SpruceFoliagePlacer>`
/// factory: `foliagePlacerParts(i).and(IntProviders.codec(0, 24).fieldOf(
/// "trunk_height").forGetter(...)).apply(i, SpruceFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn spruce_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<SpruceFoliagePlacer, Ops>> {
    record_builder::map_codec::<SpruceFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<SpruceFoliagePlacer, Ops>(
            Arc::new(|p: &SpruceFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &SpruceFoliagePlacer| p.offset.clone()),
        );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &SpruceFoliagePlacer| p.trunk_height.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(0, 24),
                    "trunk_height".to_string(),
                ),
            ))
            .apply(instance, Arc::new(SpruceFoliagePlacer::new))
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
    fn codec_round_trips_the_spruce_record() {
        let codec = map_codec::codec_of(spruce_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "trunk_height": {"min_inclusive": 4, "max_inclusive": 6, "type": "minecraft:uniform"}
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER
        );
        assert_eq!(decoded.trunk_height(), &provider(4, 6));
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
    fn codec_rejects_trunk_height_out_of_range() {
        // `IntProviders.codec(0, 24)` rejects providers sampling outside bounds.
        let codec = map_codec::codec_of(spruce_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": 2,
            "offset": 0,
            "trunk_height": {"min_inclusive": 0, "max_inclusive": 40, "type": "minecraft:uniform"}
        });
        let result = codec.parse(&JsonOps::INSTANCE, &input);
        assert!(result.is_error());
    }

    #[test]
    fn foliage_height_is_max_of_four_and_remainder() {
        // `Math.max(4, treeHeight - trunkHeight.sample(random))`.
        let p = SpruceFoliagePlacer::new(provider(2, 2), provider(0, 0), provider(2, 2));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            8
        );
        assert_eq!(
            p.foliage_height(&mut random, 3, &TreeConfiguration::stub()),
            4
        );
    }
}

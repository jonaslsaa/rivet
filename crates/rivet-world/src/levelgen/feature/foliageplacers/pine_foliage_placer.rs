//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! PineFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).and(IntProviders.codec(0, 24).
//! fieldOf("height").forGetter(p -> p.height)).apply(i, PineFoliagePlacer::new)`.
//! `createFoliage` grows `currentRadius` up to `leafRadius + radiusOffset` each
//! row, stepping it back once at `yo == offset - foliageHeight + 1`; the crown
//! therefore expands toward the base. `foliageRadius` adds
//! `random.nextInt(max(trunkHeight + 1, 1))` on top of the sampled radius.
//!
//! The Rust port keeps the `foliagePlacerParts` prefix (two
//! `RecordCodecBuilder`s over `int_provider_codec_with_bounds(0, 16)`) plus the
//! `"height"` field with `codec(0, 24)` bounds.

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

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.PineFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct PineFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.height`.
    height: IntProvider,
}

impl PineFoliagePlacer {
    /// `new PineFoliagePlacer(IntProvider, IntProvider, IntProvider)`.
    pub fn new(radius: IntProvider, offset: IntProvider, height: IntProvider) -> PineFoliagePlacer {
        PineFoliagePlacer {
            radius,
            offset,
            height,
        }
    }

    /// `this.height`.
    pub fn height(&self) -> &IntProvider {
        &self.height
    }
}

impl FoliagePlacer for PineFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::PINE_FOLIAGE_PLACER
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
        let mut current_radius = 0;

        let mut yo = offset;
        while yo >= offset.wrapping_sub(foliage_height) {
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
            if current_radius >= 1 && yo == offset.wrapping_sub(foliage_height).wrapping_add(1) {
                current_radius = current_radius.wrapping_sub(1);
            } else if current_radius < leaf_radius.wrapping_add(foliage_attachment.radius_offset) {
                current_radius = current_radius.wrapping_add(1);
            }
            yo = yo.wrapping_sub(1);
        }
    }

    fn foliage_radius<R: RandomSource>(&self, random: &mut R, trunk_height: i32) -> i32 {
        // `super.foliageRadius(random, trunkHeight) + nextInt(...)`. Rust cannot
        // call a defaulted trait body once an impl overrides it (even via UFCS),
        // so the base body — `this.radius.sample(random)` — is inlined here.
        self.radius().sample(random).wrapping_add(random.next_int_bound(std::cmp::max(
            trunk_height.wrapping_add(1),
            1,
        )))
    }

    fn foliage_height<R: RandomSource>(
        &self,
        random: &mut R,
        _tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        self.height.sample(random)
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

/// `PineFoliagePlacer.CODEC` — the ops-generic `MapCodec<PineFoliagePlacer>`
/// factory: `foliagePlacerParts(i).and(IntProviders.codec(0, 24).fieldOf(
/// "height").forGetter(...)).apply(i, PineFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn pine_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<PineFoliagePlacer, Ops>> {
    record_builder::map_codec::<PineFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<PineFoliagePlacer, Ops>(
            Arc::new(|p: &PineFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &PineFoliagePlacer| p.offset.clone()),
        );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &PineFoliagePlacer| p.height.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(0, 24),
                    "height".to_string(),
                ),
            ))
            .apply(instance, Arc::new(PineFoliagePlacer::new))
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
    fn codec_round_trips_the_pine_record() {
        let codec = map_codec::codec_of(pine_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": {"min_inclusive": 5, "max_inclusive": 7, "type": "minecraft:uniform"}
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::PINE_FOLIAGE_PLACER
        );
        assert_eq!(decoded.height(), &provider(5, 7));
        assert_eq!(decoded.radius(), &provider(2, 2));
        assert_eq!(decoded.offset(), &provider(0, 0));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn foliage_radius_adds_a_draw_over_max_one() {
        // `super.foliageRadius(random, trunkHeight) + random.nextInt(
        // max(trunkHeight + 1, 1))`.
        let p = PineFoliagePlacer::new(provider(1, 1), provider(0, 0), provider(5, 5));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        let r = p.foliage_radius(&mut random, 0);
        assert!(r >= 1 && r <= 2);
    }

    #[test]
    fn foliage_height_is_the_height_provider() {
        let p = PineFoliagePlacer::new(provider(1, 1), provider(0, 0), provider(5, 5));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            5
        );
    }
}

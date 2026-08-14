//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! MegaJungleFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).and(Codec.intRange(0, 16).
//! fieldOf("height").forGetter(p -> p.height)).apply(i, MegaJungleFoliagePlacer::new)`.
//! `createFoliage` sets `leafHeight` to `foliageHeight` for a double trunk,
//! otherwise `1 + random.nextInt(2)`, then lays one row per `yo` with
//! `currentRadius = leafRadius + radiusOffset + 1 - yo` (the crown widens going
//! down). The skip test is `dx + dz >= 7 || dx*dx + dz*dz > cur*cur` — **int**
//! arithmetic throughout (Java `int * int`, wrapping), not f32.
//!
//! The Rust port models the `foliagePlacerParts` prefix (two `RecordCodecBuilder`s
//! over `int_provider_codec_with_bounds(0, 16)`) plus the `"height"` field via
//! `codec::int_range(0, 16)`.

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

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.MegaJungleFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct MegaJungleFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.height`.
    height: i32,
}

impl MegaJungleFoliagePlacer {
    /// `new MegaJungleFoliagePlacer(IntProvider, IntProvider, int)`.
    pub fn new(radius: IntProvider, offset: IntProvider, height: i32) -> MegaJungleFoliagePlacer {
        MegaJungleFoliagePlacer {
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

impl FoliagePlacer for MegaJungleFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER
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
        let leaf_height = if foliage_attachment.double_trunk {
            foliage_height
        } else {
            1i32.wrapping_add(random.next_int_bound(2))
        };

        let mut yo = offset;
        while yo >= offset.wrapping_sub(leaf_height) {
            let current_radius = leaf_radius
                .wrapping_add(foliage_attachment.radius_offset)
                .wrapping_add(1)
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

/// `MegaJungleFoliagePlacer.CODEC` — the ops-generic `MapCodec<MegaJungleFoliagePlacer>`
/// factory: `foliagePlacerParts(i).and(Codec.intRange(0, 16).fieldOf("height")).
/// apply(i, MegaJungleFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn mega_jungle_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<MegaJungleFoliagePlacer, Ops>> {
    record_builder::map_codec::<MegaJungleFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) =
            foliage_placer_parts::<MegaJungleFoliagePlacer, Ops>(
                Arc::new(|p: &MegaJungleFoliagePlacer| p.radius.clone()),
                Arc::new(|p: &MegaJungleFoliagePlacer| p.offset.clone()),
            );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &MegaJungleFoliagePlacer| p.height),
                codec::field_of(codec::int_range::<Ops>(0, 16), "height".to_string()),
            ))
            .apply(instance, Arc::new(MegaJungleFoliagePlacer::new))
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
    fn codec_round_trips_the_mega_jungle_record() {
        let codec = map_codec::codec_of(mega_jungle_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 3, "max_inclusive": 3, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": 3
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER
        );
        assert_eq!(decoded.height(), 3);
        assert_eq!(decoded.radius(), &provider(3, 3));
        assert_eq!(decoded.offset(), &provider(0, 0));
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn codec_rejects_height_out_of_range() {
        // `Codec.intRange(0, 16)` rejects ints outside [0, 16].
        let codec = map_codec::codec_of(mega_jungle_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": 3,
            "offset": 0,
            "height": 20
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &input).is_error());
    }

    #[test]
    fn skip_test_is_int_circle_plus_distance_cap() {
        // `dx + dz >= 7 || dx*dx + dz*dz > currentRadius*currentRadius` (int).
        let p = MegaJungleFoliagePlacer::new(provider(3, 3), provider(0, 0), 3);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // dx + dz = 7 -> skip regardless of radius.
        assert!(p.should_skip_location(&mut random, 3, 0, 4, 8, false));
        // dx²+dz² > 8² inside radius 8 keeps the disc.
        assert!(!p.should_skip_location(&mut random, 3, 0, 3, 8, false));
        // dx²+dz² > cur² skips the corner at radius 3: 9+9=18 > 9.
        assert!(p.should_skip_location(&mut random, 3, 0, 3, 3, false));
        assert!(!p.should_skip_location(&mut random, 1, 0, 1, 3, false));
    }

    #[test]
    fn foliage_height_is_the_fixed_height() {
        let p = MegaJungleFoliagePlacer::new(provider(0, 1), provider(0, 1), 5);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            5
        );
    }
}

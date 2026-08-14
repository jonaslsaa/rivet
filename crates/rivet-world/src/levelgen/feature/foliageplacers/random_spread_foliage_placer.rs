//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! RandomSpreadFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).and(i.group(
//! IntProviders.codec(1, 512).fieldOf("foliage_height")...,
//! Codec.intRange(0, 256).fieldOf("leaf_placement_attempts")...)).
//! apply(i, RandomSpreadFoliagePlacer::new)` — the shared two-field prefix
//! followed by a **nested** two-field `i.group(...)`. The Rust port reproduces
//! the nesting with `instance.group(fh).and(attempts).apply(instance, |a, b|
//! (a, b))` — a `RecordCodecBuilder<O, Ops, (IntProvider, i32)>` whose decoder
//! (like every other field's) runs against the same input map on decode — then
//! chains that inner group as the third field of the outer record.
//!
//! `createFoliage` makes `leafPlacementAttempts` tries, each `setWithOffset`
//! placing the leaf at a uniformly random offset inside the box `[-leafRadius,
//! leafRadius) × [-foliageHeight, foliageHeight) × [-leafRadius, leafRadius)`
//! (x, y, z; six `nextInt` draws per try: x, x, y, y, z, z), then
//! `tryPlaceLeaf`. `shouldSkipLocation` is a constant `false` — this placer
//! does not skip corners.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::{
    FoliageAttachment, FoliagePlacer, FoliageSetter, foliage_placer_parts, try_place_leaf,
};
use crate::levelgen::feature::foliageplacers::foliage_placer_type::{
    FoliagePlacerTypeId, FoliagePlacerTypes,
};
use rivet_registry::core::Vec3i;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.RandomSpreadFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct RandomSpreadFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.foliageHeight`.
    foliage_height: IntProvider,
    /// `this.leafPlacementAttempts`.
    leaf_placement_attempts: i32,
}

impl RandomSpreadFoliagePlacer {
    /// `new RandomSpreadFoliagePlacer(IntProvider, IntProvider, IntProvider, int)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        radius: IntProvider,
        offset: IntProvider,
        foliage_height: IntProvider,
        leaf_placement_attempts: i32,
    ) -> RandomSpreadFoliagePlacer {
        RandomSpreadFoliagePlacer {
            radius,
            offset,
            foliage_height,
            leaf_placement_attempts,
        }
    }

    /// `this.foliageHeight`.
    pub fn foliage_height(&self) -> &IntProvider {
        &self.foliage_height
    }

    /// `this.leafPlacementAttempts`.
    pub fn leaf_placement_attempts(&self) -> i32 {
        self.leaf_placement_attempts
    }
}

impl FoliagePlacer for RandomSpreadFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER
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
        _offset: i32,
    ) {
        let origin = foliage_attachment.pos;
        let origin_vec = Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z());
        let mut pos = origin.mutable();

        for _i in 0..self.leaf_placement_attempts {
            // `random.nextInt(leafRadius) - random.nextInt(leafRadius)` per
            // axis — Java int draw order x, x, y, y, z, z.
            pos.set_with_offset_xyz(
                &origin_vec,
                random
                    .next_int_bound(leaf_radius)
                    .wrapping_sub(random.next_int_bound(leaf_radius)),
                random
                    .next_int_bound(foliage_height)
                    .wrapping_sub(random.next_int_bound(foliage_height)),
                random
                    .next_int_bound(leaf_radius)
                    .wrapping_sub(random.next_int_bound(leaf_radius)),
            );
            try_place_leaf(level, foliage_setter, random, config, &pos.immutable());
        }
    }

    fn foliage_height<R: RandomSource>(
        &self,
        random: &mut R,
        _tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        self.foliage_height.sample(random)
    }

    fn should_skip_location<R: RandomSource>(
        &self,
        _random: &mut R,
        _dx: i32,
        _y: i32,
        _dz: i32,
        _current_radius: i32,
        _double_trunk: bool,
    ) -> bool {
        false
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

/// `RandomSpreadFoliagePlacer.CODEC` — the ops-generic `MapCodec<RandomSpreadFoliagePlacer>`
/// factory: `foliagePlacerParts(i).and(i.group(foliage_height,
/// leaf_placement_attempts)).apply(i, RandomSpreadFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn random_spread_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<RandomSpreadFoliagePlacer, Ops>> {
    record_builder::map_codec::<RandomSpreadFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<RandomSpreadFoliagePlacer, Ops>(
            Arc::new(|p: &RandomSpreadFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &RandomSpreadFoliagePlacer| p.offset.clone()),
        );
        // `i.group(IntProviders.codec(1, 512).fieldOf("foliage_height"),
        // Codec.intRange(0, 256).fieldOf("leaf_placement_attempts"))` — the
        // nested two-field group, materialized as the `(IntProvider, i32)`
        // value the outer record's third field carries.
        let inner = instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &RandomSpreadFoliagePlacer| p.foliage_height.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(1, 512),
                    "foliage_height".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &RandomSpreadFoliagePlacer| p.leaf_placement_attempts),
                codec::field_of(
                    codec::int_range::<Ops>(0, 256),
                    "leaf_placement_attempts".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |foliage_height: IntProvider, leaf_placement_attempts: i32| {
                        (foliage_height, leaf_placement_attempts)
                    },
                ),
            );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .and(inner)
            .apply(
                instance,
                Arc::new(
                    |radius: IntProvider, offset: IntProvider, group: (IntProvider, i32)| {
                        RandomSpreadFoliagePlacer::new(radius, offset, group.0, group.1)
                    },
                ),
            )
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
    fn codec_round_trips_the_random_spread_record() {
        let codec = map_codec::codec_of(random_spread_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "foliage_height": {"min_inclusive": 1, "max_inclusive": 3, "type": "minecraft:uniform"},
            "leaf_placement_attempts": 64
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER
        );
        assert_eq!(decoded.foliage_height(), &provider(1, 3));
        assert_eq!(decoded.leaf_placement_attempts(), 64);
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
    fn codec_rejects_out_of_range_fields() {
        let codec = map_codec::codec_of(random_spread_foliage_placer_map_codec::<JsonOps>());
        // `IntProviders.codec(1, 512)` rejects max outside [1, 512].
        let bad_height = json!({
            "radius": 2,
            "offset": 0,
            "foliage_height": {"min_inclusive": 1, "max_inclusive": 600, "type": "minecraft:uniform"},
            "leaf_placement_attempts": 64
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &bad_height).is_error());
        // `Codec.intRange(0, 256)` rejects attempts outside [0, 256].
        let bad_attempts = json!({
            "radius": 2,
            "offset": 0,
            "foliage_height": 2,
            "leaf_placement_attempts": 300
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &bad_attempts).is_error());
    }

    #[test]
    fn foliage_height_is_the_provider() {
        let p = RandomSpreadFoliagePlacer::new(provider(2, 2), provider(0, 0), provider(3, 3), 64);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // The inherent `foliage_height(&self)` accessor shadows the trait
        // method, so reach the behavior (sampling the provider) explicitly.
        assert_eq!(
            FoliagePlacer::foliage_height(&p, &mut random, 10, &TreeConfiguration::stub()),
            3
        );
    }

    #[test]
    fn should_skip_location_is_always_false() {
        let p = RandomSpreadFoliagePlacer::new(provider(2, 2), provider(0, 0), provider(3, 3), 64);
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert!(!p.should_skip_location(&mut random, 5, 0, -5, 2, true));
    }
}

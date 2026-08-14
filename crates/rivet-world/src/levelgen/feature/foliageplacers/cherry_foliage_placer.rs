//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! CherryFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).and(i.group(
//! IntProviders.codec(4, 16).fieldOf("height")...,
//! Codec.floatRange(0, 1).fieldOf("wide_bottom_layer_hole_chance")...,
//! Codec.floatRange(0, 1).fieldOf("corner_hole_chance").forGetter(p ->
//! p.wideBottomLayerHoleChance),
//! Codec.floatRange(0, 1).fieldOf("hanging_leaves_chance")...,
//! Codec.floatRange(0, 1).fieldOf("hanging_leaves_extension_chance")...)).
//! apply(i, CherryFoliagePlacer::new)` — the shared two-field prefix followed by
//! a **nested** five-field `i.group(...)`.
//!
//! The `corner_hole_chance` field has a **quirk** that must be preserved
//! exactly: its `forGetter` reads `wideBottomLayerHoleChance`, not
//! `cornerHoleChance`. So on encode the placer's *wide-layer* chance is written
//! into the `corner_hole_chance` key (the actual `cornerHoleChance` field is
//! never serialized); on decode the `corner_hole_chance` key populates the
//! `cornerHoleChance` field. A hand-written placer whose two chances differ
//! therefore does not round-trip — encode rewrites `corner_hole_chance`.
//!
//! The Rust port reproduces the nesting with a `RecordCodecBuilder<O, Ops,
//! (IntProvider, f32, f32, f32, f32)>` (decoded against the same input map like
//! every other field) chained as the outer record's third field.
//!
//! `createFoliage` shifts the foliage origin up by `offset`, then lays the two
//! tapering top rows (`currentRadius - 2` at `foliageHeight - 3`,
//! `currentRadius - 1` at `foliageHeight - 4`), the full disc rows from
//! `foliageHeight - 5` down to `0`, and the two hanging rows
//! (`placeLeavesRowWithHangingLeavesBelow` at `y` -1 and -2). The skip test
//! uses `wideBottomLayerHoleChance` for the `y == -1` edge holes and
//! `cornerHoleChance` for the disc corners (`dx + dz > currentRadius * 2 - 2`
//! is Java int, wrapping, on the wide layer).

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

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.CherryFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct CherryFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
    /// `this.height`.
    height: IntProvider,
    /// `this.wideBottomLayerHoleChance`.
    wide_bottom_layer_hole_chance: f32,
    /// `this.cornerHoleChance`.
    corner_hole_chance: f32,
    /// `this.hangingLeavesChance`.
    hanging_leaves_chance: f32,
    /// `this.hangingLeavesExtensionChance`.
    hanging_leaves_extension_chance: f32,
}

impl CherryFoliagePlacer {
    /// `new CherryFoliagePlacer(IntProvider, IntProvider, IntProvider, float,
    /// float, float, float)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        radius: IntProvider,
        offset: IntProvider,
        height: IntProvider,
        wide_bottom_layer_hole_chance: f32,
        corner_hole_chance: f32,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
    ) -> CherryFoliagePlacer {
        CherryFoliagePlacer {
            radius,
            offset,
            height,
            wide_bottom_layer_hole_chance,
            corner_hole_chance,
            hanging_leaves_chance,
            hanging_leaves_extension_chance,
        }
    }

    /// `this.height`.
    pub fn height(&self) -> &IntProvider {
        &self.height
    }

    /// `this.wideBottomLayerHoleChance`.
    pub fn wide_bottom_layer_hole_chance(&self) -> f32 {
        self.wide_bottom_layer_hole_chance
    }

    /// `this.cornerHoleChance`.
    pub fn corner_hole_chance(&self) -> f32 {
        self.corner_hole_chance
    }

    /// `this.hangingLeavesChance`.
    pub fn hanging_leaves_chance(&self) -> f32 {
        self.hanging_leaves_chance
    }

    /// `this.hangingLeavesExtensionChance`.
    pub fn hanging_leaves_extension_chance(&self) -> f32 {
        self.hanging_leaves_extension_chance
    }
}

impl FoliagePlacer for CherryFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER
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
        let double_trunk = foliage_attachment.double_trunk;
        let foliage_pos = foliage_attachment.pos.above_steps(offset);
        let current_radius = leaf_radius
            .wrapping_add(foliage_attachment.radius_offset)
            .wrapping_sub(1);
        self.place_leaves_row(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            current_radius.wrapping_sub(2),
            foliage_height.wrapping_sub(3),
            double_trunk,
        );
        self.place_leaves_row(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            current_radius.wrapping_sub(1),
            foliage_height.wrapping_sub(4),
            double_trunk,
        );

        let mut y = foliage_height.wrapping_sub(5);
        while y >= 0 {
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &foliage_pos,
                current_radius,
                y,
                double_trunk,
            );
            y = y.wrapping_sub(1);
        }

        self.place_leaves_row_with_hanging_leaves_below(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            current_radius,
            -1,
            double_trunk,
            self.hanging_leaves_chance,
            self.hanging_leaves_extension_chance,
        );
        self.place_leaves_row_with_hanging_leaves_below(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            current_radius.wrapping_sub(1),
            -2,
            double_trunk,
            self.hanging_leaves_chance,
            self.hanging_leaves_extension_chance,
        );
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
        random: &mut R,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        _double_trunk: bool,
    ) -> bool {
        // `y == -1 && (dx == cur || dz == cur) && nextFloat() <
        // wideBottomLayerHoleChance`.
        if y == -1
            && (dx == current_radius || dz == current_radius)
            && random.next_float() < self.wide_bottom_layer_hole_chance
        {
            return true;
        }

        let corner = dx == current_radius && dz == current_radius;
        let wide_layer = current_radius > 2;
        if wide_layer {
            // `corner || dx + dz > currentRadius * 2 - 2 && nextFloat() <
            // cornerHoleChance` — Java int `cur * 2 - 2` wraps.
            corner
                || (dx.wrapping_add(dz) > current_radius.wrapping_mul(2).wrapping_sub(2)
                    && random.next_float() < self.corner_hole_chance)
        } else {
            corner && random.next_float() < self.corner_hole_chance
        }
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

/// `CherryFoliagePlacer.CODEC` — the ops-generic `MapCodec<CherryFoliagePlacer>`
/// factory: `foliagePlacerParts(i).and(i.group(height,
/// wide_bottom_layer_hole_chance, corner_hole_chance, hanging_leaves_chance,
/// hanging_leaves_extension_chance)).apply(i, CherryFoliagePlacer::new)`. The
/// `corner_hole_chance` getter reads `wide_bottom_layer_hole_chance`, exactly as
/// Java's `forGetter(p -> p.wideBottomLayerHoleChance)` does.
#[allow(clippy::type_complexity)]
pub fn cherry_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<CherryFoliagePlacer, Ops>> {
    record_builder::map_codec::<CherryFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<CherryFoliagePlacer, Ops>(
            Arc::new(|p: &CherryFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &CherryFoliagePlacer| p.offset.clone()),
        );
        // `i.group(...)` — the nested five-field group, materialized as the
        // `(IntProvider, f32, f32, f32, f32)` value the outer record's third
        // field carries.
        let inner = instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &CherryFoliagePlacer| p.height.clone()),
                codec::field_of(
                    int_provider_codec_with_bounds::<Ops>(4, 16),
                    "height".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &CherryFoliagePlacer| p.wide_bottom_layer_hole_chance),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "wide_bottom_layer_hole_chance".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                // Java `forGetter(p -> p.wideBottomLayerHoleChance)` — the
                // quirk: the `corner_hole_chance` key is encoded from (and thus
                // populated with) the wide-layer chance.
                Arc::new(|p: &CherryFoliagePlacer| p.wide_bottom_layer_hole_chance),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "corner_hole_chance".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &CherryFoliagePlacer| p.hanging_leaves_chance),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "hanging_leaves_chance".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &CherryFoliagePlacer| p.hanging_leaves_extension_chance),
                codec::field_of(
                    codec::float_range::<Ops>(0.0, 1.0),
                    "hanging_leaves_extension_chance".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |height: IntProvider,
                     wide_bottom_layer_hole_chance: f32,
                     corner_hole_chance: f32,
                     hanging_leaves_chance: f32,
                     hanging_leaves_extension_chance: f32| {
                        (
                            height,
                            wide_bottom_layer_hole_chance,
                            corner_hole_chance,
                            hanging_leaves_chance,
                            hanging_leaves_extension_chance,
                        )
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
                    |radius: IntProvider,
                     offset: IntProvider,
                     group: (IntProvider, f32, f32, f32, f32)| {
                        CherryFoliagePlacer::new(
                            radius, offset, group.0, group.1, group.2, group.3, group.4,
                        )
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
    fn codec_round_trips_the_cherry_record() {
        let codec = map_codec::codec_of(cherry_foliage_placer_map_codec::<JsonOps>());
        // The `corner_hole_chance` key round-trips only when it equals the
        // wide-layer chance (encode writes the wide-layer chance into it).
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": {"min_inclusive": 6, "max_inclusive": 8, "type": "minecraft:uniform"},
            "wide_bottom_layer_hole_chance": 0.7,
            "corner_hole_chance": 0.7,
            "hanging_leaves_chance": 0.6,
            "hanging_leaves_extension_chance": 0.1
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER
        );
        assert_eq!(decoded.height(), &provider(6, 8));
        assert_eq!(decoded.wide_bottom_layer_hole_chance(), 0.7);
        assert_eq!(decoded.corner_hole_chance(), 0.7);
        assert_eq!(decoded.hanging_leaves_chance(), 0.6);
        assert_eq!(decoded.hanging_leaves_extension_chance(), 0.1);
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
    fn corner_hole_chance_key_encodes_the_wide_layer_chance() {
        // The `forGetter(p -> p.wideBottomLayerHoleChance)` quirk: a placer with
        // different chances encodes `corner_hole_chance` from the wide-layer
        // value, so the key does not preserve the corner field.
        let codec = map_codec::codec_of(cherry_foliage_placer_map_codec::<JsonOps>());
        let placer = CherryFoliagePlacer::new(
            provider(2, 2),
            provider(0, 0),
            provider(6, 6),
            0.9,
            0.1,
            0.5,
            0.2,
        );
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &placer)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded["wide_bottom_layer_hole_chance"], json!(0.9));
        // The key is written from the wide-layer chance, not the corner field.
        assert_eq!(encoded["corner_hole_chance"], json!(0.9));
        assert_eq!(encoded["hanging_leaves_chance"], json!(0.5));
        assert_eq!(encoded["hanging_leaves_extension_chance"], json!(0.2));
        // And the corner field is not otherwise serialized.
        assert_eq!(encoded.as_object().unwrap().len(), 7);
    }

    #[test]
    fn codec_rejects_out_of_range_fields() {
        let codec = map_codec::codec_of(cherry_foliage_placer_map_codec::<JsonOps>());
        // `IntProviders.codec(4, 16)` rejects height outside [4, 16].
        let bad_height = json!({
            "radius": 2,
            "offset": 0,
            "height": 3,
            "wide_bottom_layer_hole_chance": 0.5,
            "corner_hole_chance": 0.5,
            "hanging_leaves_chance": 0.5,
            "hanging_leaves_extension_chance": 0.1
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &bad_height).is_error());
        // `Codec.floatRange(0, 1)` rejects chances outside [0, 1].
        let bad_chance = json!({
            "radius": 2,
            "offset": 0,
            "height": 6,
            "wide_bottom_layer_hole_chance": 1.5,
            "corner_hole_chance": 0.5,
            "hanging_leaves_chance": 0.5,
            "hanging_leaves_extension_chance": 0.1
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &bad_chance).is_error());
    }

    #[test]
    fn foliage_height_is_the_provider() {
        let p = CherryFoliagePlacer::new(
            provider(2, 2),
            provider(0, 0),
            provider(7, 7),
            0.5,
            0.5,
            0.5,
            0.1,
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            7
        );
    }

    #[test]
    fn wide_layer_skip_uses_the_two_chances() {
        let p = CherryFoliagePlacer::new(
            provider(2, 2),
            provider(0, 0),
            provider(7, 7),
            0.0,
            1.0,
            0.5,
            0.1,
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // `wideBottomLayerHoleChance == 0`: the y == -1 edge never skips.
        assert!(!p.should_skip_location(&mut random, 2, -1, 0, 2, false));
        // `cornerHoleChance == 1`: the (cur, cur) corner always skips; wide
        // layer (cur > 2) also skips the `dx + dz > cur*2 - 2` edges.
        let p2 = CherryFoliagePlacer::new(
            provider(2, 2),
            provider(0, 0),
            provider(7, 7),
            0.0,
            1.0,
            0.5,
            0.1,
        );
        let mut random2 = rivet_util::random::LegacyRandomSource::new(0);
        assert!(p2.should_skip_location(&mut random2, 3, 0, 3, 3, false));
        // Wide layer: `dx + dz > cur*2 - 2` — (3, 0) gives 3 > 4 false, so
        // only the corner skip applies; (2, 1) gives 3 > 4 false too.
        assert!(!p2.should_skip_location(&mut random2, 3, 0, 0, 3, false));
        assert!(!p2.should_skip_location(&mut random2, 2, 0, 1, 3, false));
        assert!(!p2.should_skip_location(&mut random2, 1, 0, 1, 3, false));
        // cur <= 2 (not wide): only the corner skips.
        let p3 = CherryFoliagePlacer::new(
            provider(2, 2),
            provider(0, 0),
            provider(7, 7),
            0.0,
            1.0,
            0.5,
            0.1,
        );
        let mut random3 = rivet_util::random::LegacyRandomSource::new(0);
        assert!(p3.should_skip_location(&mut random3, 2, 0, 2, 2, false));
        assert!(!p3.should_skip_location(&mut random3, 2, 0, 0, 2, false));
    }
}

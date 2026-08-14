//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! AcaciaFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).apply(i, AcaciaFoliagePlacer::new)` —
//! the plain two-field record (radius + offset), the same `P2` the base
//! `foliagePlacerParts` returns. `createFoliage` shifts the foliage origin up by
//! `offset`, then places three flat rows (`y` at `-1 - foliageHeight`,
//! `-foliageHeight`, and `0`) with the crown radii (`leafRadius + radiusOffset`,
//! `leafRadius - 1`, `leafRadius + radiusOffset - 1`). `foliageHeight` is
//! constant `0` (the acacia canopy is a flat disc of three rings).
//!
//! The Rust port models the `foliagePlacerParts` prefix as the two
//! `RecordCodecBuilder`s returned by the shared [`foliage_placer_parts`] helper
//! (`int_provider_codec_with_bounds(0, 16)` for both fields).

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::{
    FoliageAttachment, FoliagePlacer, FoliageSetter, foliage_placer_parts,
};
use crate::levelgen::feature::foliageplacers::foliage_placer_type::{
    FoliagePlacerTypeId, FoliagePlacerTypes,
};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::IntProvider;
use std::any::Any;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.AcaciaFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct AcaciaFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
}

impl AcaciaFoliagePlacer {
    /// `new AcaciaFoliagePlacer(IntProvider, IntProvider)`.
    pub fn new(radius: IntProvider, offset: IntProvider) -> AcaciaFoliagePlacer {
        AcaciaFoliagePlacer { radius, offset }
    }
}

impl FoliagePlacer for AcaciaFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER
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
        self.place_leaves_row(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            leaf_radius.wrapping_add(foliage_attachment.radius_offset),
            -1i32.wrapping_sub(foliage_height),
            double_trunk,
        );
        self.place_leaves_row(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            leaf_radius.wrapping_sub(1),
            -foliage_height,
            double_trunk,
        );
        self.place_leaves_row(
            level,
            foliage_setter,
            random,
            config,
            &foliage_pos,
            leaf_radius
                .wrapping_add(foliage_attachment.radius_offset)
                .wrapping_sub(1),
            0,
            double_trunk,
        );
    }

    fn foliage_height<R: RandomSource>(
        &self,
        _random: &mut R,
        _tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        0
    }

    fn should_skip_location<R: RandomSource>(
        &self,
        _random: &mut R,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        _double_trunk: bool,
    ) -> bool {
        if y == 0 {
            (dx > 1 || dz > 1) && dx != 0 && dz != 0
        } else {
            dx == current_radius && dz == current_radius && current_radius > 0
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

/// `AcaciaFoliagePlacer.CODEC` — the ops-generic `MapCodec<AcaciaFoliagePlacer>`
/// factory: `foliagePlacerParts(i).apply(i, AcaciaFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn acacia_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<AcaciaFoliagePlacer, Ops>> {
    record_builder::map_codec::<AcaciaFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<AcaciaFoliagePlacer, Ops>(
            Arc::new(|p: &AcaciaFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &AcaciaFoliagePlacer| p.offset.clone()),
        );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .apply(instance, Arc::new(AcaciaFoliagePlacer::new))
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
    fn codec_round_trips_the_acacia_record() {
        let codec = map_codec::codec_of(acacia_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"}
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER
        );
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
    fn foliage_height_is_zero() {
        let p = AcaciaFoliagePlacer::new(provider(2, 2), provider(0, 0));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            0
        );
    }
}

//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! DarkOakFoliagePlacer` (class, 26.2).
//!
//! Java: `CODEC = foliagePlacerParts(i).apply(i, DarkOakFoliagePlacer::new)` —
//! the plain two-field record (radius + offset). `createFoliage` shifts the
//! foliage origin up by `offset` and places either a double-trunk crown (rows at
//! `y` -1/0/1 of `leafRadius + 2/+3/+2`, plus a `random.nextBoolean()` row at
//! `y` 2 of `leafRadius`) or a single-trunk crown (rows at `y` -1 and 0 of
//! `leafRadius + 2` and `leafRadius + 1`).
//!
//! Both skip predicates are overridden. `shouldSkipLocation` is
//! `y == -1 && !doubleTrunk ? dx == cur && dz == cur : y == 1 && dx + dz >
//! cur * 2 - 2` — **int** arithmetic (the `cur * 2 - 2` is Java int, wrapping).
//! `shouldSkipLocationSigned` adds the double-trunk corner cap at `y == 0`
//! (`(dx == -cur || dx >= cur) && (dz == -cur || dz >= cur)`), otherwise
//! delegates to the base's signed wrapper (the `min(|dx|, |dx-1|)` fold).
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

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.DarkOakFoliagePlacer`.
#[derive(Debug, Clone)]
pub struct DarkOakFoliagePlacer {
    /// `this.radius` — the protected radius provider (from `FoliagePlacer`).
    radius: IntProvider,
    /// `this.offset` — the protected offset provider (from `FoliagePlacer`).
    offset: IntProvider,
}

impl DarkOakFoliagePlacer {
    /// `new DarkOakFoliagePlacer(IntProvider, IntProvider)`.
    pub fn new(radius: IntProvider, offset: IntProvider) -> DarkOakFoliagePlacer {
        DarkOakFoliagePlacer { radius, offset }
    }
}

impl FoliagePlacer for DarkOakFoliagePlacer {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER
    }

    fn create_foliage_with_offset<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        foliage_setter: &mut dyn FoliageSetter,
        random: &mut R,
        config: &TreeConfiguration,
        _tree_height: i32,
        foliage_attachment: &FoliageAttachment,
        _foliage_height: i32,
        leaf_radius: i32,
        offset: i32,
    ) {
        let pos = foliage_attachment.pos.above_steps(offset);
        let double_trunk = foliage_attachment.double_trunk;
        if double_trunk {
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &pos,
                leaf_radius.wrapping_add(2),
                -1,
                double_trunk,
            );
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &pos,
                leaf_radius.wrapping_add(3),
                0,
                double_trunk,
            );
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &pos,
                leaf_radius.wrapping_add(2),
                1,
                double_trunk,
            );
            if random.next_boolean() {
                self.place_leaves_row(
                    level,
                    foliage_setter,
                    random,
                    config,
                    &pos,
                    leaf_radius,
                    2,
                    double_trunk,
                );
            }
        } else {
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &pos,
                leaf_radius.wrapping_add(2),
                -1,
                double_trunk,
            );
            self.place_leaves_row(
                level,
                foliage_setter,
                random,
                config,
                &pos,
                leaf_radius.wrapping_add(1),
                0,
                double_trunk,
            );
        }
    }

    fn foliage_height<R: RandomSource>(
        &self,
        _random: &mut R,
        _tree_height: i32,
        _config: &TreeConfiguration,
    ) -> i32 {
        4
    }

    fn should_skip_location<R: RandomSource>(
        &self,
        _random: &mut R,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        // `y == -1 && !doubleTrunk ? dx == cur && dz == cur : y == 1 && dx + dz
        // > cur * 2 - 2` — Java int `cur * 2 - 2` wraps.
        if y == -1 && !double_trunk {
            dx == current_radius && dz == current_radius
        } else {
            y == 1 && dx.wrapping_add(dz) > current_radius.wrapping_mul(2).wrapping_sub(2)
        }
    }

    fn should_skip_location_signed<R: RandomSource>(
        &self,
        random: &mut R,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        // `y == 0 && doubleTrunk && (dx == -cur || dx >= cur) && (dz == -cur ||
        // dz >= cur) || super.shouldSkipLocationSigned(...)`. Rust cannot call a
        // defaulted trait body once an impl overrides it (even via UFCS), so the
        // base body — resolve the `min(|d|, |d-1|)` double-trunk minimums, then
        // `this.shouldSkipLocation(...)` (virtual) — is inlined here.
        (y == 0
            && double_trunk
            && (dx == -current_radius || dx >= current_radius)
            && (dz == -current_radius || dz >= current_radius))
            || {
                let (min_dx, min_dz) = if double_trunk {
                    (
                        std::cmp::min(dx.wrapping_abs(), dx.wrapping_sub(1).wrapping_abs()),
                        std::cmp::min(dz.wrapping_abs(), dz.wrapping_sub(1).wrapping_abs()),
                    )
                } else {
                    (dx.wrapping_abs(), dz.wrapping_abs())
                };
                self.should_skip_location(random, min_dx, y, min_dz, current_radius, double_trunk)
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

/// `DarkOakFoliagePlacer.CODEC` — the ops-generic `MapCodec<DarkOakFoliagePlacer>`
/// factory: `foliagePlacerParts(i).apply(i, DarkOakFoliagePlacer::new)`.
#[allow(clippy::type_complexity)]
pub fn dark_oak_foliage_placer_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<DarkOakFoliagePlacer, Ops>> {
    record_builder::map_codec::<DarkOakFoliagePlacer, Ops>(move |instance| {
        let (radius_builder, offset_builder) = foliage_placer_parts::<DarkOakFoliagePlacer, Ops>(
            Arc::new(|p: &DarkOakFoliagePlacer| p.radius.clone()),
            Arc::new(|p: &DarkOakFoliagePlacer| p.offset.clone()),
        );
        instance
            .group(radius_builder)
            .and(offset_builder)
            .apply(instance, Arc::new(DarkOakFoliagePlacer::new))
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
    fn codec_round_trips_the_dark_oak_record() {
        let codec = map_codec::codec_of(dark_oak_foliage_placer_map_codec::<JsonOps>());
        let input = json!({
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"}
        });
        let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            FoliagePlacer::type_id(decoded),
            FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER
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
    fn should_skip_location_is_row_specific() {
        let p = DarkOakFoliagePlacer::new(provider(2, 2), provider(0, 0));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // `y == -1 && !doubleTrunk`: skip the (cur, cur) corner only.
        assert!(p.should_skip_location(&mut random, 3, -1, 3, 3, false));
        assert!(!p.should_skip_location(&mut random, 3, -1, 2, 3, false));
        // `y == 1`: skip when `dx + dz > cur * 2 - 2`.
        assert!(p.should_skip_location(&mut random, 3, 1, 3, 3, false));
        assert!(!p.should_skip_location(&mut random, 2, 1, 2, 3, false));
    }

    #[test]
    fn should_skip_location_signed_caps_double_trunk_top_corners() {
        let p = DarkOakFoliagePlacer::new(provider(2, 2), provider(0, 0));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // `y == 0 && doubleTrunk && (dx == -cur || dx >= cur) && (dz == -cur ||
        // dz >= cur)`.
        assert!(p.should_skip_location_signed(&mut random, 3, 0, -3, 3, true));
        assert!(p.should_skip_location_signed(&mut random, -3, 0, 3, 3, true));
        assert!(!p.should_skip_location_signed(&mut random, 2, 0, 2, 3, true));
        // Non-zero rows fall through to the base signed wrapper.
        assert!(!p.should_skip_location_signed(&mut random, 1, 1, 1, 3, false));
    }

    #[test]
    fn foliage_height_is_four() {
        let p = DarkOakFoliagePlacer::new(provider(2, 2), provider(0, 0));
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        assert_eq!(
            p.foliage_height(&mut random, 10, &TreeConfiguration::stub()),
            4
        );
    }
}

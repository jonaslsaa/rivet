//! Monomorphized runtime dispatch for the existing generic placer traits.
use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::{
    FoliageAttachment, FoliagePlacer, FoliageSetter,
};
use crate::levelgen::feature::trunkplacers::trunk_placer::TrunkPlacer;
use rivet_registry::{block_state::BlockState, core::BlockPos};
use rivet_util::RandomSource;

macro_rules! with_trunk {
    ($erased:expr, $p:ident, $body:expr) => {{
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::straight_trunk_placer::StraightTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::forking_trunk_placer::ForkingTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::giant_trunk_placer::GiantTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::mega_jungle_trunk_placer::MegaJungleTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::dark_oak_trunk_placer::DarkOakTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::fancy_trunk_placer::FancyTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::bending_trunk_placer::BendingTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::upwards_branching_trunk_placer::UpwardsBranchingTrunkPlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::trunkplacers::cherry_trunk_placer::CherryTrunkPlacer>() { $body } else
        { panic!("unavailable trunk placer {:?}", $erased.type_id()) }
    }};
}
macro_rules! with_foliage {
    ($erased:expr, $p:ident, $body:expr) => {{
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::blob_foliage_placer::BlobFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::bush_foliage_placer::BushFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::fancy_foliage_placer::FancyFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::spruce_foliage_placer::SpruceFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::pine_foliage_placer::PineFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::acacia_foliage_placer::AcaciaFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::mega_jungle_foliage_placer::MegaJungleFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::mega_pine_foliage_placer::MegaPineFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::dark_oak_foliage_placer::DarkOakFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::random_spread_foliage_placer::RandomSpreadFoliagePlacer>() { $body } else
        if let Some($p) = $erased.as_any().downcast_ref::<crate::levelgen::feature::foliageplacers::cherry_foliage_placer::CherryFoliagePlacer>() { $body } else
        { panic!("unavailable foliage placer {:?}", $erased.type_id()) }
    }};
}

pub(super) fn tree_height<R: RandomSource>(config: &TreeConfiguration, random: &mut R) -> i32 {
    with_trunk!(config.trunk_placer, p, p.get_tree_height(random))
}
pub(super) fn is_free(
    config: &TreeConfiguration,
    level: &dyn WorldGenLevel,
    pos: &BlockPos,
) -> bool {
    with_trunk!(config.trunk_placer, p, p.is_free(level, pos))
}
pub(super) fn place_trunk<R: RandomSource>(
    config: &TreeConfiguration,
    level: &dyn WorldGenLevel,
    setter: &mut dyn FnMut(&BlockPos, BlockState),
    random: &mut R,
    height: i32,
    origin: &BlockPos,
) -> Vec<FoliageAttachment> {
    with_trunk!(
        config.trunk_placer,
        p,
        p.place_trunk(level, setter, random, height, origin, config)
    )
}
pub(super) fn foliage_height<R: RandomSource>(
    config: &TreeConfiguration,
    random: &mut R,
    height: i32,
) -> i32 {
    with_foliage!(
        config.foliage_placer,
        p,
        FoliagePlacer::foliage_height(p, random, height, config)
    )
}
pub(super) fn foliage_radius<R: RandomSource>(
    config: &TreeConfiguration,
    random: &mut R,
    height: i32,
) -> i32 {
    with_foliage!(config.foliage_placer, p, p.foliage_radius(random, height))
}
#[allow(clippy::too_many_arguments)]
pub(super) fn create_foliage<R: RandomSource>(
    config: &TreeConfiguration,
    level: &dyn WorldGenLevel,
    setter: &mut dyn FoliageSetter,
    random: &mut R,
    height: i32,
    attachment: &FoliageAttachment,
    foliage_height: i32,
    radius: i32,
) {
    with_foliage!(
        config.foliage_placer,
        p,
        p.create_foliage(
            level,
            setter,
            random,
            config,
            height,
            attachment,
            foliage_height,
            radius
        )
    )
}

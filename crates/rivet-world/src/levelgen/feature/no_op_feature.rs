//! Port of `net.minecraft.world.level.levelgen.feature.NoOpFeature`
//! (class, 26.2).
//!
//! Java: a 15-line leaf — `Feature<NoneFeatureConfiguration>` whose `place`
//! returns `true` unconditionally (a placement that always "succeeds" and
//! writes nothing). Owned by the `mc.world.level.levelgen.feature.noop`
//! manifest unit in the work queue; it is ported *here* (the `feature.core`
//! unit) because it is the one concrete feature the `#181` dispatch hub can
//! faithfully reach in this unit — the dispatch needs a concrete
//! `FeatureBehavior` case for `minecraft:no_op` (id 0), whose config
//! (`NoneFeatureConfiguration`) is already ported. The remaining ~62 feature
//! leaves are unavailable; dispatching to one fails explicitly (see
//! `feature_place`).

use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_util::RandomSource;

/// `net.minecraft.world.level.levelgen.feature.NoOpFeature`.
#[derive(Debug)]
pub struct NoOpFeature;

/// `Feature.NO_OP` — the registered `minecraft:no_op` singleton.
pub const NO_OP: NoOpFeature = NoOpFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for NoOpFeature {
    /// `NoOpFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)` —
    /// `return true`.
    fn place<R: RandomSource>(
        &self,
        _context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        true
    }
}

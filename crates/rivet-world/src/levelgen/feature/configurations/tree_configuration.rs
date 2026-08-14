//! Port of `net.minecraft.world.level.levelgen.feature.configurations.
//! TreeConfiguration` (class, 26.2).
//!
//! STUB(mc.world.level.levelgen.feature.configurations.tree): this unit is
//! owned by the pending `configurations.tree` manifest unit (issue #575's
//! tree-family wave — see task #1322 "Port configurations (tree, root, fallen)
//! + type registries"). The full record (trunk/foliage providers, trunk placer,
//! foliage placer, root placer, minimum size, decorators, `ignoreVines`,
//! `belowTrunkProvider`, and the 9-field `CODEC`) is ported there.
//!
//! The foliage/trunk placer units that land first only consume the three
//! `BlockStateProvider` fields (`trunkProvider`, `foliageProvider`,
//! `belowTrunkProvider` — see `FoliagePlacer.tryPlaceLeaf` /
//! `TrunkPlacer.placeBelowTrunkBlock`), so this stub carries exactly those as
//! `Arc<dyn ErasedBlockStateProvider>`, plus a `stub()` constructor for the
//! codec/geometry tests. When the owning unit lands it replaces this file.
//!
//! (There is no `FeatureConfiguration` impl here — that trait and the dispatch
//! live in `configurations.core`, already merged.)

use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.TreeConfiguration`
/// — the foliage/trunk-consumed slice of the tree config record.
pub struct TreeConfiguration {
    /// `this.trunkProvider` — the trunk block state provider.
    pub trunk_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `this.foliageProvider` — the foliage (leaves) block state provider.
    pub foliage_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `this.belowTrunkProvider` — the dirt below the trunk.
    pub below_trunk_provider: Arc<dyn ErasedBlockStateProvider>,
}

impl TreeConfiguration {
    /// A test-only config whose three providers are a single
    /// `SimpleStateProvider` of the default state (used by the foliage placer
    /// codec/geometry tests; production construction lands with the owning
    /// unit).
    pub fn stub() -> TreeConfiguration {
        let state = rivet_registry::block_state::BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_id(1),
        );
        let provider = crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(state);
        let erased: Arc<dyn ErasedBlockStateProvider> = Arc::new(provider);
        TreeConfiguration {
            trunk_provider: erased.clone(),
            foliage_provider: erased.clone(),
            below_trunk_provider: erased,
        }
    }
}

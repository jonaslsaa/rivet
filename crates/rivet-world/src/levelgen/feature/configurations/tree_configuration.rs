//! Port of `net.minecraft.world.level.levelgen.feature.configurations.
//! TreeConfiguration` (class, 26.2) — the foliage-consumed slice.
//!
//! STUB(mc.world.level.levelgen.feature.configurations.tree): the full record
//! (trunk/foliage providers, trunk placer, foliage placer, root placer, minimum
//! size, decorators, `ignoreVines`, `belowTrunkProvider`, and the 9-field
//! `CODEC`) is owned by the pending `configurations.tree` manifest unit (issue
//! #575's tree-family wave — see task #1322 "Port configurations (tree, root,
//! fallen) + type registries"). The trunk/root/decorator slices that would
//! consume the rest of the record live on the preserved
//! `feature/worldgen-tree-scaffolding` branch.
//!
//! The foliage-placer slice consumes `foliageProvider` (`FoliagePlacer.
//! tryPlaceLeaf` → `block_state_provider_get_state(config.foliage_provider,
//! ...)`) and the trunk-placer slice (the merged `trunkplacers` manifest unit)
//! consumes `trunkProvider` / `belowTrunkProvider` (`TrunkPlacer.
//! placeBelowTrunkBlock`), so this stub carries those three fields as
//! `Arc<dyn ErasedBlockStateProvider>`, plus a `stub()` constructor for the
//! codec/geometry tests. When the owning unit lands it replaces this file.
//!
//! (There is no `FeatureConfiguration` impl here — that trait and the dispatch
//! live in `configurations.core`, already merged.)

use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.TreeConfiguration`
/// — the foliage-consumed slice of the tree config record.
pub struct TreeConfiguration {
    /// `this.foliageProvider` — the foliage (leaves) block state provider.
    pub foliage_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `this.trunkProvider` — the trunk (log) block state provider.
    pub trunk_provider: Arc<dyn ErasedBlockStateProvider>,
    /// `this.belowTrunkProvider` — the optional below-trunk block state
    /// provider (`getOptionalState`); `TrunkPlacer.placeBelowTrunkBlock`
    /// places its state only when the provider yields one.
    pub below_trunk_provider: Arc<dyn ErasedBlockStateProvider>,
}

impl TreeConfiguration {
    /// A test-only config whose providers are single `SimpleStateProvider`s of
    /// the default state (used by the foliage/trunk placer codec/geometry
    /// tests; production construction lands with the owning unit).
    pub fn stub() -> TreeConfiguration {
        let state = rivet_registry::block_state::BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_id(1),
        );
        let provider = crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(state);
        let erased: Arc<dyn ErasedBlockStateProvider> = Arc::new(provider);
        TreeConfiguration {
            foliage_provider: erased.clone(),
            trunk_provider: erased.clone(),
            below_trunk_provider: erased,
        }
    }
}

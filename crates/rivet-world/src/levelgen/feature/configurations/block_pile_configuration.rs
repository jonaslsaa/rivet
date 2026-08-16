//! STUB(mc.world.level.levelgen.feature.configurations.blockpile) — cross-unit
//! stub for `net.minecraft.world.level.levelgen.feature.configurations.BlockPileConfiguration`
//! (class, 26.2).
//!
//! Java: a single-field class wrapping a `BlockStateProvider` with a
//! `public final BlockStateProvider stateProvider` field and a `CODEC` of
//! `BlockStateProvider.CODEC.fieldOf("state_provider").xmap(BlockPileConfiguration::new,
//! c -> c.stateProvider).codec()`. The `NetherForestVegetationConfig` unit
//! (owned by `mc.world.level.levelgen.feature.configurations.netherforestvegetation`)
//! consumes this type as its superclass (`extends BlockPileConfiguration`),
//! so the Rust port needs the struct to embed — the superclass fields become
//! an embedded field named after the parent (PORTING.md). This stub is only
//! that surface: the `state_provider` field and the `new` constructor. The
//! class's own `CODEC` (the `.xmap` wrapper) and any full port belong to the
//! owning `blockpile` unit and replace this stub wholesale when it lands.

use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.BlockPileConfiguration`.
///
/// STUB(mc.world.level.levelgen.feature.configurations.blockpile): the
/// `state_provider` field (a `public final BlockStateProvider`, held as the
/// erased `Arc<dyn ErasedBlockStateProvider>` carrier) and the constructor are
/// the surface the `NetherForestVegetationConfig` superclass call consumes.
/// Java does not override `equals` on this class (identity semantics), and the
/// provider field is behavior, not a value, so no `PartialEq` is derived —
/// `Clone`+`Debug` only (the same shape the `DiskConfiguration` unit takes for
/// its erased provider field).
#[derive(Debug, Clone)]
pub struct BlockPileConfiguration {
    /// `stateProvider` — the block state provider for the pile.
    pub state_provider: Arc<dyn ErasedBlockStateProvider>,
}

impl BlockPileConfiguration {
    /// `new BlockPileConfiguration(BlockStateProvider)`.
    pub fn new(state_provider: Arc<dyn ErasedBlockStateProvider>) -> BlockPileConfiguration {
        BlockPileConfiguration { state_provider }
    }
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for BlockPileConfiguration {}

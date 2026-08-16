//! `net.minecraft.world.level.levelgen.feature.stateproviders` — the
//! block-state-provider framework.
//!
//! STUB(mc.world.level.levelgen.feature.stateproviders) — this module is a
//! cross-unit stub for the `DiskConfiguration` record (owned by the
//! `mc.world.level.levelgen.feature.configurations.disk` unit), which consumes
//! `BlockStateProvider.CODEC`. The full port (the `BlockStateProvider`/
//! `BlockStateProviderType` split, the `ErasedBlockStateProvider` carrier, the
//! eight concrete providers, and the recursive `"type"`-key dispatch codec)
//! lives on `origin/main` (PR #559, commit `ba4096f8`). This stub is only the
//! surface the disk unit consumes, mirroring the merged port's module path,
//! names, and codec shape.
//!
//! Merge-state note: this worktree's last `origin/main` merge (`827eaaa1`,
//! second parent `5214c6ce` = PR #555) predates PR #559, so the full port is
//! *not* yet in this tree. The next `origin/main` merge is therefore not a
//! clean first-time application of the full port: it must resolve the
//! `stateproviders` overlap by keeping `origin/main`'s files (the
//! `BlockStateProviderTypeId`/`BlockStateProviderTypes` split into
//! `block_state_provider_type.rs`, `SimpleStateProvider` in
//! `simple_state_provider.rs`, plus `codec_helpers.rs` and the seven other
//! provider modules) and deleting this stub. The `DiskConfiguration` unit
//! consumes only `block_state_provider_codec`, `ErasedBlockStateProvider`, and
//! `simple`/`SimpleStateProvider`, which the full port provides with identical
//! signatures, so the disk unit needs no edits at that merge.
//!
//! The stub carries a single registered provider type — `SimpleStateProvider`
//! (the `minecraft:simple_state_provider` entry, `BlockStateProviderType`
//! declaration order index 0) — enough to decode/encode `DiskConfiguration`
//! fixtures with a constant state, exercising the dispatch and the
//! `state_provider` field. The other seven providers (`weighted_state_provider`,
//! `noise_threshold_provider`, `noise_provider`, `dual_noise_provider`,
//! `rotated_block_provider`, `randomized_int_state_provider`,
//! `rule_based_state_provider`) and the world-access `get_state` behavior defer
//! with the owning unit (see the STUB marker in the module doc).

pub mod block_state_provider;

pub use block_state_provider::{
    BlockStateProvider, ErasedBlockStateProvider, SimpleStateProvider, block_state_provider_codec,
    simple,
};

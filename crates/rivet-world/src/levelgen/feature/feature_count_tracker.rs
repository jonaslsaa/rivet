//! STUB(mc.world.level.levelgen.feature.core) — `FeatureCountTracker` (26.2),
//! BLOCKED.
//!
//! Static debug-only bookkeeping (`SharedConstants.DEBUG_FEATURE_COUNT`, off in
//! production), keyed by `ServerLevel`. Both call sites — `ChunkGenerator`'s
//! `chunkDecorated` and `PlacedFeature`'s `featurePlaced` — gate on the same
//! flag. Not ported: `ServerLevel` and the logger / `Registry<PlacedFeature>`
//! are out of this crate. Placeholder keeps the `feature.core` surface and the
//! `PlacedFeature`→tracker edge until the `server.level` unit and a
//! debug-count flag exist.

/// `net.minecraft.world.level.levelgen.feature.FeatureCountTracker`.
///
/// Placeholder for the static debug-only bookkeeping (see module doc). No
/// behavior yet: the `ServerLevel`-keyed static cache and the logger are out of
/// this unit's crate.
pub struct FeatureCountTracker;

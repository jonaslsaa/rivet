//! STUB(mc.server.level) — `FeatureCountTracker` (26.2), BLOCKED on the
//! `server.level` unit.
//!
//! Static debug-only bookkeeping (`SharedConstants.DEBUG_FEATURE_COUNT`, off in
//! production), keyed by `ServerLevel`. Both call sites — `ChunkGenerator`'s
//! `chunkDecorated` and `PlacedFeature`'s `featurePlaced` — gate on the same
//! flag. Not ported: `ServerLevel` and the logger / `Registry<PlacedFeature>`
//! are out of this crate. Placeholder keeps the `feature.core` surface and the
//! `PlacedFeature`→tracker edge until the `server.level` unit and a
//! debug-count flag exist. The tracker's real port belongs to
//! `mc.server.level` (pending), not `feature.core`, so the marker points there.

/// `net.minecraft.world.level.levelgen.feature.FeatureCountTracker`.
///
/// Placeholder for the static debug-only bookkeeping (see module doc). No
/// behavior yet: the `ServerLevel`-keyed static cache and the logger are out of
/// this unit's crate.
pub struct FeatureCountTracker;

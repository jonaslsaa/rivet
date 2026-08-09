//! Port of `net.minecraft.world.level.levelgen.carver.CarverConfiguration`
//! (class, 26.2) — the configuration bound every carver and configured carver
//! is generic over.
//!
//! Java: `CarverConfiguration extends ProbabilityFeatureConfiguration` and adds
//! five fields (`y` HeightProvider, `yScale` FloatProvider, `lavaLevel`
//! VerticalAnchor, `debugSettings` CarverDebugSettings, `replaceable`
//! HolderSet<Block>) plus the `CODEC` record codec over them. None of the
//! added field types are ported yet (the value-provider/height-provider units,
//! the block slice `#228`, and the `#126` codec surface), so the Rust shell
//! keeps `CarverConfiguration` a *marker trait*: it captures the `WC extends
//! CarverConfiguration` bound that `ConfiguredWorldCarver` and
//! `WorldCarverBehavior` require without fabricating the deferred field
//! surface. The inherited `probability` field (on the already-ported
//! `ProbabilityFeatureConfiguration`) is recovered by the concrete
//! configurations when they land with `#180`.

use std::fmt::Debug;

/// `net.minecraft.world.level.levelgen.carver.CarverConfiguration` — the
/// configuration type bound of `ConfiguredWorldCarver` and
/// `WorldCarverBehavior`.
///
/// Implemented by every carver configuration value type. Java's class is
/// generic in nothing and adds fields; the Rust trait is a marker because the
/// field types (`HeightProvider`, `FloatProvider`, `VerticalAnchor`,
/// `CarverDebugSettings`, `HolderSet<Block>`) are not ported yet.
pub trait CarverConfiguration: Debug + Send + Sync + 'static {}

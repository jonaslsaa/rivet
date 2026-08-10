//! `net.minecraft.world.flag.FeatureFlag` — an opaque flag handle.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/
//! FeatureFlag.java`. A 12-line class holding `(FeatureFlagUniverse universe,
//! long mask)` with **no identity semantics beyond its fields** — Java relies
//! on object identity only via the shared `FeatureFlagUniverse` reference, and
//! `mask` is the `1L << bit` bit position assigned by the builder.
//!
//! The Rust port is a `Clone` value pair (`mask()` is the bit that `1L << bit`
//! produces for a bit index up to 63; `1L << bit` in Java is signed-left-shift
//! of a `long`, which Rust's `1u64 << bit` reproduces exactly for `bit in
//! 0..64` — the builder guards `bit >= 64`). `universe()` returns the universe
//! this flag belongs to (the universe is a value type, see
//! `feature_flag_universe`).

use super::feature_flag_universe::FeatureFlagUniverse;

/// `FeatureFlag` — the `(universe, mask)` opaque handle.
///
/// Not `Copy`: the universe owns a `String`, so the handle is `Clone`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureFlag {
    universe: FeatureFlagUniverse,
    mask: u64,
}

impl FeatureFlag {
    /// `new FeatureFlag(FeatureFlagUniverse, int bit)` — the package-private
    /// constructor only the registry `Builder` calls.
    pub(crate) fn new(universe: FeatureFlagUniverse, bit: u32) -> Self {
        FeatureFlag {
            universe,
            mask: 1u64 << bit,
        }
    }

    /// `flag.universe` — the owning universe.
    pub fn universe(&self) -> &FeatureFlagUniverse {
        &self.universe
    }

    /// `flag.mask` — the `1L << bit` bit mask.
    pub fn mask(&self) -> u64 {
        self.mask
    }
}

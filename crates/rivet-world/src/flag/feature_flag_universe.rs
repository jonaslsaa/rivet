//! `net.minecraft.world.flag.FeatureFlagUniverse` — the flag-universe id.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/
//! FeatureFlagUniverse.java`. A tiny class holding a `String id` whose
//! `toString` returns the id. Rust has no Java reference identity, so the
//! universe is a **value** type here: `PartialEq`/`Eq` compare the id string.
//!
//! `FeatureFlagSet`/`FeatureFlagRegistry` rely on Java's *reference* identity
//! of a `FeatureFlagUniverse` (`this.universe == other.universe` in
//! `FeatureFlagSet`); within the registry that is exactly value equality on the
//! universe id (a `Builder` hands the same instance to every flag it creates
//! and to the built registry). Cross-registry comparisons in Java (a
//! `FeatureFlag` from a different `Builder` with a same-named universe are NOT
//! equal even though `universe.id` matches) are NOT reproduced — the value port
//! treats same-id universes as identical, which the level.dat codec never
//! distinguishes (only the `"main"` registry exists).

/// `FeatureFlagUniverse` — the `(String id)` value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureFlagUniverse {
    id: String,
}

impl FeatureFlagUniverse {
    /// `new FeatureFlagUniverse(String id)`.
    pub fn new(id: String) -> Self {
        FeatureFlagUniverse { id }
    }
}

impl std::fmt::Display for FeatureFlagUniverse {
    /// `FeatureFlagUniverse.toString()` — the id.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

//! `net.minecraft.world.flag.FeatureFlagUniverse` — the flag-universe id.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/
//! FeatureFlagUniverse.java`. A tiny class holding a `String id` whose
//! `toString` returns the id. Java's `FeatureFlagSet`/`FeatureFlagRegistry`
//! compare universes by **reference** identity (`this.universe ==
//! other.universe`), so a `FeatureFlag` created by one `Builder` is never equal
//! to one created by another `Builder`, even when both use the same id string.
//!
//! The port preserves that identity by backing the id with an `Arc<String>`:
//! `PartialEq`/`Eq`/`Hash` are `Arc::ptr_eq` (two universes are equal iff they
//! share one allocation — i.e. one `Builder`), `Clone` shares the allocation,
//! and every `new` creates a fresh allocation. The id remains readable via
//! `id()` for `Display` (Java's `toString`). `Arc`'s *derived* `PartialEq`
//! compares the pointed-to strings, which would reintroduce value identity, so
//! it is overridden here.

use std::sync::Arc;

/// `FeatureFlagUniverse` — a reference-identity universe over a `String id`.
#[derive(Debug, Clone)]
pub struct FeatureFlagUniverse {
    id: Arc<String>,
}

/// Java's `==` on a `FeatureFlagUniverse` is reference identity.
impl PartialEq for FeatureFlagUniverse {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.id, &other.id)
    }
}

impl Eq for FeatureFlagUniverse {}

impl std::hash::Hash for FeatureFlagUniverse {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Identity hash — Java hashes the object, not the id string.
        Arc::as_ptr(&self.id).hash(state);
    }
}

impl FeatureFlagUniverse {
    /// `new FeatureFlagUniverse(String id)` — a fresh identity allocation.
    pub fn new(id: String) -> Self {
        FeatureFlagUniverse { id: Arc::new(id) }
    }

    /// The universe id (Java's private `String id`), for `Display` and tests.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl std::fmt::Display for FeatureFlagUniverse {
    /// `FeatureFlagUniverse.toString()` — the id.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_per_allocation_not_per_id() {
        // Two independent universes with the same id are NOT equal (Java
        // reference identity); a clone shares the allocation and IS equal.
        let a = FeatureFlagUniverse::new("main".to_string());
        let b = FeatureFlagUniverse::new("main".to_string());
        assert_ne!(a, b);
        let a_clone = a.clone();
        assert_eq!(a, a_clone);
        // Display is the id for both.
        assert_eq!(a.to_string(), "main");
        assert_eq!(b.to_string(), "main");
    }

    #[test]
    fn hash_matches_identity() {
        let a = FeatureFlagUniverse::new("main".to_string());
        let a_clone = a.clone();
        let b = FeatureFlagUniverse::new("main".to_string());
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        seen.insert(a.clone());
        seen.insert(a_clone);
        seen.insert(b);
        // a and its clone collide on the same identity slot; b is distinct.
        assert_eq!(seen.len(), 2);
    }
}

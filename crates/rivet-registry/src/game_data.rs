//! `GameData` — the frozen, shared game registry data.
//!
//! PROVENANCE: Paper's `GameData` (registered content + provider); the
//! OWNERSHIP.md §Registries model: "Loaded/generated at startup, frozen,
//! `Arc<GameData>` shared everywhere including worker pools. Interior mutability
//! forbidden after freeze." and "`GameData` owns the provider; `Level` may hold
//! a per-dimension provider (layer order STATIC → WORLDGEN → DIMENSIONS →
//! RELOADABLE is observable — keep an explicit ordered vec)."
//!
//! Reload contract (issue #107): **reload = rebuild + swap.** A datapack reload
//! builds a fresh `GameData` (fresh layers, fresh registries) and atomically
//! replaces the old one behind an `Arc` — there is no in-place reload and no
//! `Arc::get_mut`. `GameData` has no `&mut self` methods: after construction
//! the provider is immutable, so code holding an old `GameData` keeps seeing
//! the old tables across a swap.

use crate::access::{LayeredRegistryAccess, RegistryAccess, RegistryLayer};

/// `GameData` — the frozen registry provider.
///
/// Clone shares the underlying erased entries (cheap `Arc` bumps), so the
/// server's `Arc<GameData>` can be handed to worker pools freely. `new()` is
/// the only construction path; a reload builds a new `GameData` and swaps it
/// in.
#[derive(Debug, Clone)]
pub struct GameData {
    /// The layered provider (explicit order STATIC → WORLDGEN → DIMENSIONS →
    /// RELOADABLE, never a HashMap).
    pub access: LayeredRegistryAccess<RegistryLayer>,
}

impl GameData {
    /// Build a fresh `GameData` from the STATIC layer (startup).
    pub fn new() -> Self {
        GameData {
            access: LayeredRegistryAccess::create_registry_access(),
        }
    }

    /// `GameData` owns the provider — the composite access across all layers.
    pub fn provider(&self) -> RegistryAccess {
        self.access.composite_access()
    }
}

impl Default for GameData {
    fn default() -> Self {
        GameData::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::RegistryLayer;

    #[test]
    fn game_data_new_builds_the_four_layers_in_order() {
        let data = GameData::new();
        // STATIC → WORLDGEN → DIMENSIONS → RELOADABLE is the observable layer
        // order; all empty until built-in content is ported.
        for layer in RegistryLayer::VALUES {
            assert!(data.access.get_layer(layer).list_registry_keys().is_empty());
        }
        assert!(data.provider().list_registry_keys().is_empty());
    }

    #[test]
    fn game_data_clone_keeps_the_provider_intact() {
        let data = GameData::new();
        let copy = data.clone();
        assert_eq!(
            copy.provider().list_registry_keys(),
            data.provider().list_registry_keys()
        );
    }
}

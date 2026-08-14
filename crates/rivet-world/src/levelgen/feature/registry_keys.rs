//! The `Registries.CONFIGURED_FEATURE` / `Registries.PLACED_FEATURE` registry
//! keys the selector and vegetation-patch features resolve their holders
//! through.
//!
//! Java features never touch a `RegistryAccess` — `Holder.value()` resolves
//! through the value stored in the holder. The Rust port's `Holder` is a pure
//! `(RegistryId, id)` pair (OWNERSHIP §Registries, the back-reference rule), so
//! resolving a `Holder<PlacedFeature>` or a `Holder<ConfiguredFeatureErased>`
//! needs the owning registry. `PlacedFeature::place` takes the configured-feature
//! `HolderLookup` as its first parameter, and the selector/composite features
//! reach both lookups through `WorldGenLevel::registry_access` — the deferred
//! seam in `crate::level::world_gen_level` (a `STUB(mc.world.level)` default
//! that panics with `RivetTodo #232` until a production `WorldGenLevel`
//! provides it).
//!
//! Both registry keys are defined in `biome_generation_settings.rs`
//! (`mc.world.level.biome.core`, merged): `Registries.CONFIGURED_FEATURE` as a
//! `LazyLock` (the `levelgen/noise/registry_keys.rs` and
//! `biome_generation_settings.rs` convention: `Identifier` owns its `String`
//! fields, so no `Identifier` value can be a `const`) and
//! `Registries.PLACED_FEATURE` as the identical `worldgen/placed_feature`
//! static. This module re-exports both, so the feature/ and configuration/
//! modules resolve the registries through one seam while the definitions live
//! in a single place and can never drift.

pub use crate::biome::biome_generation_settings::{CONFIGURED_FEATURE, PLACED_FEATURE};

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;

    #[test]
    fn keys_match_java_registry_identifiers() {
        // Java: `Registries.CONFIGURED_FEATURE = createRegistryKey("worldgen/configured_feature")`,
        // `PLACED_FEATURE = createRegistryKey("worldgen/placed_feature")`.
        assert_eq!(
            CONFIGURED_FEATURE.identifier().to_string(),
            "minecraft:worldgen/configured_feature"
        );
        assert_eq!(
            PLACED_FEATURE.identifier().to_string(),
            "minecraft:worldgen/placed_feature"
        );
        // `createRegistryKey` wires both to `Registries.ROOT_REGISTRY_NAME`.
        assert_eq!(
            CONFIGURED_FEATURE.registry(),
            &Identifier::with_default_namespace("root")
        );
        assert_eq!(
            PLACED_FEATURE.registry(),
            &Identifier::with_default_namespace("root")
        );
    }
}

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
//! seam in `crate::level::world_gen_level`, marked there as
//! `STUB(mc.world.level.levelgen.feature.selector)`.
//!
//! `Registries.CONFIGURED_FEATURE` has no typed-key definition elsewhere in the
//! crate, so it lives here as a `LazyLock` (the `levelgen/noise/registry_keys.rs`
//! and `biome_generation_settings.rs` convention: `Identifier` owns its `String`
//! fields, so no `Identifier` value can be a `const`). `Registries.PLACED_FEATURE`
//! is owned by `biome_generation_settings.rs` (`mc.world.level.biome.core`,
//! merged), which defines the identical `worldgen/placed_feature` key — this
//! module re-exports that static, so the two modules share one definition and
//! can never drift. `vegetation_patch_configuration.rs` (PR #616, owned
//! elsewhere) and the concrete features in this module resolve the registries
//! through these statics.

use crate::levelgen::feature::ConfiguredFeatureErased;
use rivet_registry::resource_key::ResourceKey;
use rivet_registry::{Identifier, Registry};
use std::sync::LazyLock;

/// `Registries.CONFIGURED_FEATURE` — `"worldgen/configured_feature"`, the
/// registry key `ConfiguredFeature.CODEC` is a `RegistryFileCodec` over.
pub static CONFIGURED_FEATURE: LazyLock<ResourceKey<Registry<ConfiguredFeatureErased>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/configured_feature",
        ))
    });

/// `Registries.PLACED_FEATURE` — `"worldgen/placed_feature"`, the registry key
/// `PlacedFeature.CODEC` is a `RegistryFileCodec` over. Re-exported from
/// [`crate::biome::biome_generation_settings`] so the key has a single
/// definition (no `biome_generation_settings::PLACED_FEATURE` / this-module
/// drift).
pub use crate::biome::biome_generation_settings::PLACED_FEATURE;

#[cfg(test)]
mod tests {
    use super::*;

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

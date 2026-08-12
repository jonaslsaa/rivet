//! The typed registry keys of the `#177` noise-value-layer unit.
//!
//! Java's `Registries.NOISE`/`DENSITY_FUNCTION`/`DENSITY_FUNCTION_TYPE` are
//! declared in `net.minecraft.core.registries.Registries` (`rivet-registry`),
//! but their element types cannot be declared there: `NOISE`'s element is the
//! worldgen synth unit's `NoiseParameters`, `DENSITY_FUNCTION`'s is the erased
//! `Arc<dyn DensityFunction>` carrier, and `DENSITY_FUNCTION_TYPE`'s is this
//! slice's `DensityFunctionTypeId` — all `rivet-world` types. Declaring the
//! keys with real element types in `rivet-registry` would create a Cargo cycle,
//! so the typed keys live here (the `mc.core` unit's full ~140-key set remains
//! the eventual owner of the *wire* keys; the placeholder `STUB`s in
//! `rivet-registry::registries` were removed when this unit landed).
//!
//! The `LazyLock` convention matches `rivet-registry::registries`:
//! `Identifier` owns its `String` fields, so no `Identifier` value can be a
//! `const`.

use crate::levelgen::noise::density_function::DensityFunction;
use crate::levelgen::noise::density_function_type::DensityFunctionTypeId;
use crate::levelgen::synth::normal_noise::NoiseParameters;
use rivet_registry::Identifier;
use rivet_registry::{Registry, ResourceKey};
use std::sync::Arc;
use std::sync::LazyLock;

/// `Registries.NOISE` — `createRegistryKey("worldgen/noise")`, the
/// `NormalNoise.NoiseParameters` registry key.
pub static NOISE: LazyLock<ResourceKey<Registry<NoiseParameters>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/noise"))
});

/// `Registries.DENSITY_FUNCTION` — `createRegistryKey("worldgen/density_function")`,
/// the erased density-function registry key. `DensityFunction.CODEC` is the
/// `RegistryFileCodec` over this key.
pub static DENSITY_FUNCTION: LazyLock<ResourceKey<Registry<Arc<dyn DensityFunction>>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/density_function",
        ))
    });

/// `Registries.DENSITY_FUNCTION_TYPE` — `createRegistryKey("worldgen/density_function_type")`,
/// the `MapCodec<? extends DensityFunction>` registry key.
/// `DensityFunctions.CODEC` dispatches through it (by name); the per-type
/// identity is this slice's `DensityFunctionTypeId`.
pub static DENSITY_FUNCTION_TYPE: LazyLock<ResourceKey<Registry<DensityFunctionTypeId>>> =
    LazyLock::new(|| {
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/density_function_type",
        ))
    });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_java_registry_identifiers() {
        // Java: `Registries.NOISE = createRegistryKey("worldgen/noise")`,
        // `DENSITY_FUNCTION = createRegistryKey("worldgen/density_function")`,
        // `DENSITY_FUNCTION_TYPE = createRegistryKey("worldgen/density_function_type")`.
        assert_eq!(NOISE.identifier().to_string(), "minecraft:worldgen/noise");
        assert_eq!(
            DENSITY_FUNCTION.identifier().to_string(),
            "minecraft:worldgen/density_function"
        );
        assert_eq!(
            DENSITY_FUNCTION_TYPE.identifier().to_string(),
            "minecraft:worldgen/density_function_type"
        );
        assert_eq!(
            NOISE.registry(),
            &Identifier::with_default_namespace("root")
        );
        assert_eq!(
            DENSITY_FUNCTION.registry(),
            &Identifier::with_default_namespace("root")
        );
        assert_eq!(
            DENSITY_FUNCTION_TYPE.registry(),
            &Identifier::with_default_namespace("root")
        );
    }
}

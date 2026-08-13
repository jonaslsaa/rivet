//! Production bootstrap helper for the worldgen registries.
//!
//! Drives the three `net.minecraft.data.worldgen` bootstraps — [`noise_data`]
//! (NOISE), [`noise_router_data`] (DENSITY_FUNCTION), and
//! [`noise_generator_settings`] (NOISE_SETTINGS) — through the test seam
//! `RecordingContext` into frozen `rivet-registry` registries, and returns the
//! `RegistryAccess` with all three populated. This is the single reusable entry
//! point for code that needs the *real* overworld noise composition (probes,
//! offline samplers, future worldgen wiring) instead of re-deriving the
//! `RecordingContext` → `RegistryBuilder` → `freeze` sequence per call site.
//!
//! Build order is Java's dependency order: NOISE first (the density-function
//! bootstrap resolves `Holder<NoiseParameters>` through it), then
//! DENSITY_FUNCTION, then NOISE_SETTINGS (which resolves the `overworld` router
//! functions through the density-function registry).
//!
//! `RegistryAccess::from_pairs` consumes each frozen `Registry<T>` and
//! `Registry<T>` is not `Clone`, so each build function freezes its own
//! prerequisites: NOISE is frozen four times (once in
//! [`build_density_function_registry`], then again in
//! [`build_noise_settings_registry`] — once directly and once through its
//! `build_density_function_registry` call — and once more in
//! [`build_worldgen_registries`]) and DENSITY_FUNCTION twice. All
//! instances of a given registry have identical declaration-ordered contents,
//! so the `Holder::Reference`s the density-function bootstrap produces resolve
//! through whichever NOISE / DENSITY_FUNCTION instance `RandomState` is handed
//! — the same pattern the noisegen unit tests use. `Registry::value_of` resolves
//! a `Reference` by element id (the registration insertion index), which
//! matches the declaration order on every instance; the holder's `registry`
//! owner field is only consulted by `can_serialize_in` (a codec-path owner
//! check, not exercised by this helper).
//!
//! The `RecordingContext` owner `RegistryId`s are hardcoded (0/1/2) — a
//! test-seam simplification inherited from the noisegen unit tests. Each frozen
//! registry actually carries a distinct per-instance `RegistryId` from the
//! builder; the ids do not affect holder resolution here (see above), but a
//! future `can_serialize_in`/codec consumer of the returned holders must
//! re-key them through the real registry identity (`#126`).
//!
//! The `RegistrySetBuilder` production bootstrap (Java's `BuildState`) is a
//! separate, later unit (`#126`); until it lands, `RecordingContext` is the
//! documented seam, and this helper is its production consumer.

use crate::data::worldgen::bootstrap_context::RecordingContext;
use crate::data::worldgen::noise_data;
use crate::levelgen::noise::registry_keys;
use crate::levelgen::noisegen::noise_generator_settings;
use crate::levelgen::noisegen::noise_generator_settings::NoiseGeneratorSettings;
use crate::levelgen::noisegen::noise_router_data::{self, DensityFunctionValue};
use crate::levelgen::synth::normal_noise::NoiseParameters;
use rivet_registry::holder::RegistryId;
use rivet_registry::registry::{Registry, RegistryKey};
use rivet_registry::root::AnyBox;
use rivet_registry::{RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey};
use std::sync::Arc;

/// A `RegistryKey<()>` — the erased access key for a typed registry.
type ErasedKey = RegistryKey<()>;

/// `Registries.NOISE` key erased to the access's stored key type.
fn noise_key() -> ErasedKey {
    ResourceKey::create_registry_key(registry_keys::NOISE.identifier().clone())
}

/// `Registries.DENSITY_FUNCTION` key erased to the access's stored key type.
fn density_function_key() -> ErasedKey {
    ResourceKey::create_registry_key(registry_keys::DENSITY_FUNCTION.identifier().clone())
}

/// `Registries.NOISE_SETTINGS` key erased to the access's stored key type.
fn noise_settings_key() -> ErasedKey {
    ResourceKey::create_registry_key(registry_keys::NOISE_SETTINGS.identifier().clone())
}

/// A frozen NOISE registry (via [`noise_data::bootstrap`]).
fn build_noise_registry() -> Registry<NoiseParameters> {
    let mut builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(&registry_keys::NOISE);
    let mut context = RecordingContext::<NoiseParameters>::new(
        RegistryId(0),
        (*registry_keys::NOISE).clone(),
        RegistryAccess::empty(),
    );
    noise_data::bootstrap(&mut context);
    for registration in context.registrations() {
        builder.register(
            &registration.key,
            Arc::new(registration.value.clone()),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// A frozen DENSITY_FUNCTION registry (via [`noise_router_data::bootstrap`]).
///
/// Freezes a fresh NOISE registry into the bootstrap access — the bootstrap's
/// `lookup(&NOISE)` calls resolve `Holder<NoiseParameters>` through it, and the
/// two instances are identical (see the module doc).
fn build_density_function_registry() -> Registry<DensityFunctionValue> {
    let noise = build_noise_registry();
    let with_noise = RegistryAccess::from_pairs(vec![(noise_key(), Box::new(noise) as AnyBox)]);
    let mut builder: RegistryBuilder<DensityFunctionValue> =
        RegistryBuilder::new(&registry_keys::DENSITY_FUNCTION);
    let mut context = RecordingContext::<DensityFunctionValue>::new(
        RegistryId(1),
        (*registry_keys::DENSITY_FUNCTION).clone(),
        with_noise,
    );
    noise_router_data::bootstrap(&mut context);
    for registration in context.registrations() {
        builder.register(
            &registration.key,
            Arc::new(registration.value.clone()),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// `Registries.BIOME` key erased to the access's stored key type.
fn biome_key() -> ErasedKey {
    ResourceKey::create_registry_key(rivet_registry::registries::BIOME.identifier().clone())
}

/// A frozen biome registry carrying the 33 `SurfaceRuleData`-referenced keys
/// (the single source of truth in `biome::biomes::SURFACE_RULE_BIOMES`) as
/// `BiomeId` handles (the surface trees only need the holder identity).
fn build_biome_registry() -> Registry<rivet_registry::biome_id::BiomeId> {
    use crate::biome::biomes::SURFACE_RULE_BIOMES;
    use rivet_registry::biome_id::BiomeId;
    let biome_key = &*rivet_registry::registries::BIOME;
    let mut builder: RegistryBuilder<BiomeId> = RegistryBuilder::new(biome_key);
    for (i, name) in SURFACE_RULE_BIOMES.iter().enumerate() {
        builder.register(
            &ResourceKey::create(
                biome_key,
                rivet_registry::Identifier::with_default_namespace(name),
            ),
            Arc::new(BiomeId::from_id(i as u16)),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// A frozen NOISE_SETTINGS registry (via [`noise_generator_settings::bootstrap`]).
///
/// Freezes fresh NOISE + DENSITY_FUNCTION + BIOME registries into the bootstrap
/// access (the module doc explains the rebuilds; the biome registry is required
/// by the `SurfaceRuleData` builders the settings bootstrap resolves).
fn build_noise_settings_registry() -> Registry<NoiseGeneratorSettings> {
    let noise = build_noise_registry();
    let functions = build_density_function_registry();
    let biomes = build_biome_registry();
    let with_worldgen = RegistryAccess::from_pairs(vec![
        (noise_key(), Box::new(noise) as AnyBox),
        (density_function_key(), Box::new(functions) as AnyBox),
        (biome_key(), Box::new(biomes) as AnyBox),
    ]);
    let mut builder: RegistryBuilder<NoiseGeneratorSettings> =
        RegistryBuilder::new(&registry_keys::NOISE_SETTINGS);
    let mut context = RecordingContext::<NoiseGeneratorSettings>::new(
        RegistryId(2),
        (*registry_keys::NOISE_SETTINGS).clone(),
        with_worldgen,
    );
    noise_generator_settings::bootstrap(&mut context);
    for registration in context.registrations() {
        builder.register(
            &registration.key,
            Arc::new(registration.value.clone()),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// The worldgen registries — NOISE, DENSITY_FUNCTION, BIOME, NOISE_SETTINGS —
/// frozen and bundled in a `RegistryAccess`. Build once per world/seed; the
/// access is cheap to clone (shares the frozen registries). The BIOME registry
/// rides along because the NOISE_SETTINGS bootstrap's `SurfaceRuleData` builders
/// resolve their biome holders through it.
pub fn build_worldgen_registries() -> RegistryAccess {
    let noise = build_noise_registry();
    let functions = build_density_function_registry();
    let biomes = build_biome_registry();
    let settings = build_noise_settings_registry();
    RegistryAccess::from_pairs(vec![
        (noise_key(), Box::new(noise) as AnyBox),
        (density_function_key(), Box::new(functions) as AnyBox),
        (biome_key(), Box::new(biomes) as AnyBox),
        (noise_settings_key(), Box::new(settings) as AnyBox),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS;
    use crate::levelgen::noisegen::noise_generator_settings::{OVERWORLD, dummy};
    use rivet_registry::HolderGetter;

    #[test]
    fn builds_all_four_registries() {
        let access = build_worldgen_registries();
        let noise = access
            .lookup(&registry_keys::NOISE)
            .expect("NOISE registry");
        let functions = access
            .lookup(&registry_keys::DENSITY_FUNCTION)
            .expect("DENSITY_FUNCTION registry");
        let settings = access
            .lookup(&registry_keys::NOISE_SETTINGS)
            .expect("NOISE_SETTINGS registry");
        // The biome registry rides along for the SurfaceRuleData builders the
        // settings bootstrap resolves.
        let biomes = access
            .lookup(&*rivet_registry::registries::BIOME)
            .expect("BIOME registry");
        // The registries are the bootstrapped sets, not empty stubs.
        assert!(noise.key_set().len() >= 60, "noise registry populated");
        assert!(
            functions.key_set().len() >= 30,
            "density-function registry populated"
        );
        assert_eq!(
            settings.key_set().len(),
            7,
            "the seven NOISE_SETTINGS presets"
        );
        assert_eq!(
            biomes.key_set().len(),
            33,
            "the SurfaceRuleData-referenced biomes"
        );
        // The overworld preset resolves to the real settings (never dummy()).
        let holder = settings.get_or_throw(&OVERWORLD);
        let value = holder.value(settings);
        assert_eq!(value.noise_settings, OVERWORLD_NOISE_SETTINGS);
        assert_eq!(value.sea_level, 63);
        assert!(value.aquifers_enabled);
        assert!(value.ore_veins_enabled);
        assert!(!value.disable_mob_generation);
        assert!(!value.use_legacy_random_source);
    }

    #[test]
    fn holder_references_resolve_through_the_built_registry() {
        // A `Holder::Reference` produced by the density-function bootstrap (the
        // `HolderHolder` seam) resolves through the frozen DENSITY_FUNCTION
        // registry in the returned access — the `RandomState` wiring path.
        let access = build_worldgen_registries();
        let settings = access
            .lookup(&registry_keys::NOISE_SETTINGS)
            .expect("NOISE_SETTINGS registry");
        let holder = settings.get_or_throw(&OVERWORLD);
        assert!(matches!(holder, rivet_registry::Holder::Reference { .. }));
        // Value resolves without panicking — and is NOT the dummy() preset
        // (the overworld preset enables aquifers/ore-veins and mob generation;
        // `dummy()` disables them).
        let value = holder.value(settings);
        assert!(value.aquifers_enabled);
        assert!(value.ore_veins_enabled);
        assert!(!value.disable_mob_generation);
        assert!(!dummy().aquifers_enabled);
        assert!(dummy().disable_mob_generation);
    }

    #[test]
    fn bootstrapped_noise_registry_instantiates_real_noise() {
        use crate::levelgen::noisegen::random_state::RandomState;
        let access = build_worldgen_registries();
        let noise = access
            .lookup(&registry_keys::NOISE)
            .expect("NOISE registry");
        let functions = access
            .lookup(&registry_keys::DENSITY_FUNCTION)
            .expect("DENSITY_FUNCTION registry");
        let settings = dummy();
        let state = RandomState::create(&settings, noise, functions, 42);
        // A real instantiated `NormalNoise` from the bootstrapped NOISE
        // registry — finite bounds, never a panic from an unbound holder.
        let instantiated =
            state.get_or_create_noise(&crate::levelgen::noise::noises::CONTINENTALNESS);
        assert!(instantiated.max_value().is_finite());
        // The wired router's final density resolves (the `HolderHolder` seam).
        let _ = state.router().final_density();
    }
}

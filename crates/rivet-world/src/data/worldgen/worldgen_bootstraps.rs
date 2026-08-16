//! Production bootstrap helper for the worldgen registries.
//!
//! Drives the three `net.minecraft.data.worldgen` bootstraps — [`noise_data`]
//! (NOISE), [`noise_router_data`] (DENSITY_FUNCTION), and
//! [`noise_generator_settings`] (NOISE_SETTINGS) — through the test seam
//! `RecordingContext` into frozen `rivet-registry` registries, and returns the
//! `RegistryAccess` with all six populated (NOISE, DENSITY_FUNCTION, BIOME,
//! NOISE_SETTINGS, CONFIGURED_FEATURE, PLACED_FEATURE). The feature registries
//! are currently empty typed registries: they make the worldgen back-reference
//! boundary real without fabricating feature values, while the feature decoder
//! remains an explicit downstream stage. The BIOME registry rides along because the NOISE_SETTINGS
//! bootstrap's `SurfaceRuleData` builders resolve their biome holders through
//! it. This is the single reusable entry point for code that needs the *real*
//! overworld noise composition (probes, offline samplers, future worldgen
//! wiring) instead of re-deriving the `RecordingContext` → `RegistryBuilder` →
//! `freeze` sequence per call site.
//!
//! Build order is Java's dependency order: NOISE first (the density-function
//! bootstrap resolves `Holder<NoiseParameters>` through it), then
//! DENSITY_FUNCTION, then BIOME, then NOISE_SETTINGS (which resolves the
//! `overworld` router functions through the density-function registry and the
//! `SurfaceRuleData` biome holders through the biome registry).
//!
//! `RegistryAccess::from_pairs` consumes each frozen `Registry<T>` and
//! `Registry<T>` is not `Clone`, so registries are shared by cloning the
//! access's erased (key, value) entries — never the `Registry<T>` value.
//! `build_worldgen_registries` freezes NOISE, DENSITY_FUNCTION, and BIOME once
//! each into a base access, runs the settings bootstrap against a clone of that
//! access, and composes the returned access from the same base plus the frozen
//! NOISE_SETTINGS registry via the layered composite. The two accesses therefore
//! carry the *same* biome registry instance, and the `Holder::Reference`s the
//! `SurfaceRuleData` builders produce — whose `registry` field is that biome
//! registry's `RegistryId` — pass the codec-path `can_serialize_in` owner check
//! against the returned access.
//!
//! `build_density_function_registry` still freezes its own NOISE for the
//! density-function bootstrap, so NOISE has two instances and DENSITY_FUNCTION
//! one. All instances of a given registry have identical declaration-ordered
//! contents, so the `Holder::Reference`s the density-function bootstrap produces
//! resolve through whichever NOISE / DENSITY_FUNCTION instance `RandomState` is
//! handed — the same pattern the noisegen unit tests use. `Registry::value_of`
//! resolves a `Reference` by element id (the registration insertion index),
//! which matches the declaration order on every instance.
//!
//! The `RecordingContext` owner `RegistryId`s are hardcoded (0/1/2) — a
//! test-seam simplification inherited from the noisegen unit tests. Each frozen
//! registry carries a distinct per-instance `RegistryId` from the builder; the
//! ids do not affect holder resolution here, and the shared biome registry keeps
//! the surface-rule holders' owner field aligned with the returned access, but
//! a future `can_serialize_in`/codec consumer of the other holders' owner fields
//! must re-key them through the real registry identity (`#126`).
//!
//! The `RegistrySetBuilder` production bootstrap (Java's `BuildState`) is a
//! separate, later unit (`#126`); until it lands, `RecordingContext` is the
//! documented seam, and this helper is its production consumer.

use crate::data::worldgen::bootstrap_context::RecordingContext;
use crate::data::worldgen::noise_data;
use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use crate::levelgen::noise::registry_keys;
use crate::levelgen::noisegen::noise_generator_settings;
use crate::levelgen::noisegen::noise_generator_settings::NoiseGeneratorSettings;
use crate::levelgen::noisegen::noise_router_data::{self, DensityFunctionValue};
use crate::levelgen::placement::PlacedFeature;
use crate::levelgen::synth::normal_noise::NoiseParameters;
use rivet_registry::access::{LayeredRegistryAccess, RegistryLayer};
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

/// `Registries.CONFIGURED_FEATURE` key erased to the access's stored key type.
fn configured_feature_key() -> ErasedKey {
    ResourceKey::create_registry_key(CONFIGURED_FEATURE.identifier().clone())
}

/// `Registries.PLACED_FEATURE` key erased to the access's stored key type.
fn placed_feature_key() -> ErasedKey {
    ResourceKey::create_registry_key(PLACED_FEATURE.identifier().clone())
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
    for name in SURFACE_RULE_BIOMES {
        // The value is the biome's real generated id (`BiomeId::from_name`), not
        // a fabricated positional index: a `SurfaceRuleData` `is_biome` holder
        // resolved through this registry reads this value back via
        // `Holder::value`, so it must equal the id the generated biome table
        // assigns the same key. (`dense_biome_id` on a `Reference` reads the
        // positional `id` field, not this value — the `#179` apply-path gap the
        // surface_rules module doc documents.)
        let value = BiomeId::from_name(&format!("minecraft:{name}"))
            .expect("SURFACE_RULE_BIOMES entries are generated biome keys");
        builder.register(
            &ResourceKey::create(
                biome_key,
                rivet_registry::Identifier::with_default_namespace(name),
            ),
            Arc::new(value),
            RegistrationInfo::BUILT_IN,
        );
    }
    builder.freeze()
}

/// A frozen NOISE_SETTINGS registry (via [`noise_generator_settings::bootstrap`]).
///
/// Runs the settings bootstrap against `with_worldgen` — the caller's base
/// access (NOISE + DENSITY_FUNCTION + BIOME, see [`build_worldgen_registries`]).
/// The `SurfaceRuleData` builders the settings bootstrap resolves read their
/// biome holders through that access's BIOME registry.
fn build_noise_settings_registry(
    with_worldgen: RegistryAccess,
) -> Registry<NoiseGeneratorSettings> {
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

/// The worldgen registries — NOISE, DENSITY_FUNCTION, BIOME, NOISE_SETTINGS,
/// CONFIGURED_FEATURE, and PLACED_FEATURE — frozen and bundled in a
/// `RegistryAccess`. Build once per world/seed; the access is cheap to clone
/// (shares the frozen registries).
///
/// The NOISE, DENSITY_FUNCTION, BIOME, and typed feature registries are frozen
/// once each into a base access, the settings bootstrap runs against a clone of
/// that access (sharing the same entries), and the returned access composes the
/// base with the frozen NOISE_SETTINGS registry via the layered composite. Both
/// accesses therefore carry the *same* biome registry instance, so the
/// `SurfaceRuleData` biome holders the settings bootstrap produces pass the
/// codec-path `can_serialize_in` owner check against the returned access. The
/// feature registries are intentionally empty until the configured/placed
/// feature decoder lands; their presence makes holder ownership explicit rather
/// than failing because the registry itself is absent.
pub fn build_worldgen_registries() -> RegistryAccess {
    let base = RegistryAccess::from_pairs(vec![
        (noise_key(), Box::new(build_noise_registry()) as AnyBox),
        (
            density_function_key(),
            Box::new(build_density_function_registry()) as AnyBox,
        ),
        (biome_key(), Box::new(build_biome_registry()) as AnyBox),
        (
            configured_feature_key(),
            Box::new(RegistryBuilder::<ConfiguredFeatureErased>::new(&*CONFIGURED_FEATURE).freeze())
                as AnyBox,
        ),
        (
            placed_feature_key(),
            Box::new(RegistryBuilder::<PlacedFeature>::new(&*PLACED_FEATURE).freeze()) as AnyBox,
        ),
    ]);
    let settings = build_noise_settings_registry(base.clone());
    let settings_access =
        RegistryAccess::from_pairs(vec![(noise_settings_key(), Box::new(settings) as AnyBox)]);
    // The layers are a vehicle for entry-sharing (the composite merges the
    // erased entries, cloning the pairs), not a semantic layer map.
    LayeredRegistryAccess::new(vec![RegistryLayer::Static, RegistryLayer::Worldgen])
        .replace_from(RegistryLayer::Static, &[base])
        .replace_from(RegistryLayer::Worldgen, &[settings_access])
        .composite_access()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS;
    use crate::levelgen::noisegen::noise_generator_settings::{OVERWORLD, dummy};
    use crate::levelgen::surface_rules::rule_source_codec;
    use rivet_registry::HolderGetter;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    #[test]
    fn builds_all_six_registries() {
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
        assert!(
            access.lookup(&*CONFIGURED_FEATURE).is_some(),
            "configured-feature registry key is present"
        );
        assert!(
            access.lookup(&*PLACED_FEATURE).is_some(),
            "placed-feature registry key is present"
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

    /// The `SurfaceRuleData` biome holders the settings bootstrap produced must
    /// pass the codec-path `can_serialize_in` owner check against the returned
    /// access. Encoding the OVERWORLD `surface_rule` through a `RegistryOps`
    /// over that access resolves every `Holder<BiomeId>`; a holder whose
    /// `registry` field pointed at a *different* biome-registry instance would
    /// error `"Element ... is not valid in current registry set"`. This pins the
    /// one-shared-biome-instance composition (a regression for the duplicate
    /// `build_biome_registry()` composition, where the settings bootstrap and the
    /// returned access carried distinct instances).
    #[test]
    fn overworld_surface_rule_biome_holders_serialize_against_the_returned_access() {
        let access = build_worldgen_registries();
        let settings = access
            .lookup(&registry_keys::NOISE_SETTINGS)
            .expect("NOISE_SETTINGS registry");
        let holder = settings.get_or_throw(&OVERWORLD);
        let value = holder.value(settings);
        let codec = rule_source_codec::<TestOps>();
        // The ops owns an access; a clone shares the same registry entries
        // (a cheap Arc bump), so the settings borrow above stays valid.
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access.clone());
        let encoded = codec
            .encode_start(&ops, &value.surface_rule)
            .get_or_throw("encode OVERWORLD surface rule")
            .clone();
        // The biome rules encode by identifier (e.g. `minecraft:badlands`), the
        // `can_serialize_in` gate's observable output.
        let text = serde_json::to_string(&encoded).expect("encoded surface rule serializes");
        assert!(
            text.contains("minecraft:badlands") && text.contains("minecraft:stony_peaks"),
            "the surface rule must encode its referenced biome identifiers, got {text}"
        );
    }

    /// The `#179` biome-registry identity: the bootstrapped biome registry
    /// registers each `SURFACE_RULE_BIOMES` key under its real generated id, so
    /// a `SurfaceRuleData` condition holder resolved through it (`is_biome` →
    /// `getOrThrow`) reads back that real id via `Holder::value`. The reference
    /// is `key`-round-trippable, and the fabricated-index regression (the old
    /// `from_id(i)` values — basalt_deltas registered as id 1 instead of the
    /// real 2) is pinned by the value/id split.
    #[test]
    fn surface_rule_biome_registry_resolves_real_generated_ids() {
        use crate::biome::biomes::SURFACE_RULE_BIOMES;
        use crate::biome::dense_biome_id;
        use rivet_registry::biome_id::BiomeId;

        let access = build_worldgen_registries();
        let biomes = access
            .lookup(&*rivet_registry::registries::BIOME)
            .expect("BIOME registry");
        let biome_key = &*rivet_registry::registries::BIOME;

        for name in SURFACE_RULE_BIOMES {
            let key = ResourceKey::create(
                biome_key,
                rivet_registry::Identifier::with_default_namespace(name),
            );
            let holder = biomes.get_or_throw(&key);
            // A positional `Reference` into the frozen subset registry
            // (identity-based `get_id`), which resolves back to its key.
            assert!(
                matches!(&holder, rivet_registry::Holder::Reference { .. }),
                "{name} holder must be a reference"
            );
            assert_eq!(
                holder.key(biomes),
                key,
                "{name} reference round-trips its key"
            );
            // The registered VALUE is the biome's real generated id (never the
            // fabricated subset index the builder used before #179).
            let real = BiomeId::from_name(&format!("minecraft:{name}")).expect("generated key");
            assert_eq!(
                holder.value(biomes).id(),
                real.id(),
                "{name} value must be its real generated id"
            );
        }

        // The residual `#179` apply-path gap, documented honestly: `dense_biome_id`
        // on the positional `Reference` reads the subset index, not the real id.
        // The biome-core wiring that makes the runtime biome source produce
        // `Reference`s from this registry must also resolve them to their values.
        let basalt_deltas = ResourceKey::create(
            biome_key,
            rivet_registry::Identifier::with_default_namespace("basalt_deltas"),
        );
        let holder = biomes.get_or_throw(&basalt_deltas);
        let basalt_real = BiomeId::from_name("minecraft:basalt_deltas").unwrap();
        assert_eq!(
            holder.value(biomes).id(),
            basalt_real.id(),
            "value is real id 2"
        );
        assert_eq!(
            dense_biome_id(&holder),
            1,
            "reference reads the subset index"
        );
        assert_ne!(dense_biome_id(&holder), basalt_real.id());
    }
}

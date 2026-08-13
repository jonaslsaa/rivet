//! Port of `net.minecraft.world.level.levelgen.RandomState` (26.2).
//!
//! The per-world random/noise wiring: the base `PositionalRandomFactory`, the
//! aquifer/ore random factories, the two visitors that wire the router
//! (`NoiseWiringHelper` re-seeds `BlendedNoise` and re-instantiates
//! `NormalNoise`; the noise-flattener resolves `HolderHolder`/`Marker` values
//! for the `Climate.Sampler`), and the noise/random caches.
//!
//! Translation notes:
//! - Java stores a `HolderGetter<NoiseParameters>`; the Rust holder model needs
//!   a `HolderLookup` to resolve `Holder::value`/`unwrapKey`, so `create` takes
//!   `&dyn HolderLookup` for both the noise registry and the density-function
//!   registry (the flattener resolves `HolderHolder` values through the
//!   latter — a documented seam deviation; the deferred `RegistrySetBuilder`
//!   holder keeps the `getOrThrow` contract).
//! - The visitors' `wrapped` caches are Java `HashMap<DensityFunction,
//!   DensityFunction>` keyed by object identity; the Rust port keys them by
//!   [`IdentityKey`] — an `Arc` address hash that holds its key strongly, so a
//!   freed intermediate's recycled address can never spuriously alias a live
//!   cache entry (Java's map keeps its keys reachable).
//! - `noiseInstances`/`positionalRandoms` are Java `ConcurrentHashMap`s; the
//!   sync-tick model uses `Mutex<HashMap>` (`OWNERSHIP.md` — no shared game
//!   state; the mutex is the visitor `&self` seam, uncontended).
//! - `SurfaceSystem` is the `levelgen::surface_rules` STUB (the owning surface
//!   unit ports the constructor; `RandomState` carries the type identity).

use crate::biome::Sampler;
use crate::levelgen::noise::density_function::{
    DensityFunction, IdentityKey, NoiseHolder, Visitor, map_all,
};
use crate::levelgen::noise::density_functions::{HolderHolder, Marker};
use crate::levelgen::noise::noise_router::NoiseRouter;
use crate::levelgen::noise::noises;
use crate::levelgen::noisegen::noise_generator_settings::NoiseGeneratorSettings;
use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
use crate::levelgen::random::PositionalRandomFactoryOverloads;
use crate::levelgen::surface_rules::SurfaceSystem;
use crate::levelgen::synth::blended_noise::BlendedNoise;
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::Holder;
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_registry::access::RegistryAccess;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::registry::Registry;
use rivet_util::random::{LegacyRandomSource, RandomSource};
use rivet_util::worldgen_random::{AlgorithmPositionalRandomFactory, AlgorithmRandomSource};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// `net.minecraft.world.level.levelgen.RandomState` — the per-world random
/// wiring. Holds `&Registry` views (concrete registries are `Send + Sync`, the
/// `Visitor` bound the `#177` value model needs — see the module doc for the
/// seam deviation from Java's `HolderGetter.Provider`).
pub struct RandomState<'a> {
    /// `random` — `settings.getRandomSource().newInstance(seed).forkPositional()`.
    random: AlgorithmPositionalRandomFactory,
    /// `noises` — the `Registries.NOISE` registry.
    noises: &'a Registry<NoiseParameters>,
    /// `functions` — the `Registries.DENSITY_FUNCTION` registry. Carried so the
    /// `NoiseChunk` wrap can resolve `HolderHolder::Reference` values (Java's
    /// `BuildState`-bound `holder.value()` during the chunk construction wrap).
    functions: &'a Registry<DensityFunctionValue>,
    /// `router` — the noise-wired router.
    router: NoiseRouter,
    /// `sampler` — the climate sampler over the flattened router.
    sampler: Sampler,
    /// `surfaceSystem` — the `levelgen::surface_rules` STUB identity.
    surface_system: SurfaceSystem,
    /// `aquiferRandom`.
    aquifer_random: AlgorithmPositionalRandomFactory,
    /// `oreRandom`.
    ore_random: AlgorithmPositionalRandomFactory,
    /// `noiseIntances` — the `ConcurrentHashMap`-equivalent noise cache.
    noise_instances: Mutex<HashMap<ResourceKey<NoiseParameters>, NormalNoise>>,
    /// `positionalRandoms` — the `ConcurrentHashMap`-equivalent factory cache.
    positional_randoms: Mutex<HashMap<Identifier, AlgorithmPositionalRandomFactory>>,
}

impl<'a> RandomState<'a> {
    /// `RandomState.create(HolderLookup.Provider holders, ResourceKey<NoiseGeneratorSettings>,
    /// long seed)` — the provider-based overload. Resolves the settings holder
    /// through the `NOISE_SETTINGS` registry, then delegates. The concrete
    /// `&RegistryAccess` replaces Java's `HolderLookup.Provider` (`lookup` is
    /// generic, so the provider is not dyn-compatible; `RegistryAccess` is the
    /// only implementor).
    pub fn create_from_provider(
        holders: &'a RegistryAccess,
        noise_settings: &ResourceKey<NoiseGeneratorSettings>,
        seed: i64,
    ) -> RandomState<'a> {
        let settings_registry =
            holders.lookup_or_throw(&crate::levelgen::noise::registry_keys::NOISE_SETTINGS);
        let settings_holder = settings_registry.get_or_throw(noise_settings);
        let settings = settings_holder.value(settings_registry);
        let noises = holders.lookup_or_throw(&crate::levelgen::noise::registry_keys::NOISE);
        let functions =
            holders.lookup_or_throw(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION);
        Self::create(settings, noises, functions, seed)
    }

    /// `RandomState.create(NoiseGeneratorSettings settings, HolderGetter<NoiseParameters>
    /// noises, long seed)` — the direct overload. `functions` is the
    /// density-function registry the flattener needs (see the module doc).
    pub fn create(
        settings: &NoiseGeneratorSettings,
        noises: &'a Registry<NoiseParameters>,
        functions: &'a Registry<DensityFunctionValue>,
        seed: i64,
    ) -> RandomState<'a> {
        let random = settings
            .get_random_source()
            .new_instance(seed)
            .fork_positional();
        let aquifer_random = random
            .from_hash_of_identifier(&Identifier::with_default_namespace("aquifer"))
            .fork_positional();
        let ore_random = random
            .from_hash_of_identifier(&Identifier::with_default_namespace("ore"))
            .fork_positional();
        let noise_instances: Mutex<HashMap<ResourceKey<NoiseParameters>, NormalNoise>> =
            Mutex::new(HashMap::new());
        let positional_randoms: Mutex<HashMap<Identifier, AlgorithmPositionalRandomFactory>> =
            Mutex::new(HashMap::new());

        let use_legacy_init = settings.use_legacy_random_source;
        let router = {
            let helper = NoiseWiringHelper {
                random: &random,
                noises,
                functions,
                seed,
                use_legacy_init,
                noise_instances: &noise_instances,
                wrapped: Mutex::new(HashMap::new()),
            };
            settings.noise_router.map_all(&helper)
        };

        let sampler = {
            let flattener = NoiseFlattener {
                functions,
                wrapped: Mutex::new(HashMap::new()),
            };
            Sampler {
                temperature: map_all(router.temperature(), &flattener),
                humidity: map_all(router.vegetation(), &flattener),
                continentalness: map_all(router.continents(), &flattener),
                erosion: map_all(router.erosion(), &flattener),
                depth: map_all(router.depth(), &flattener),
                weirdness: map_all(router.ridges(), &flattener),
                spawn_target: settings.spawn_target.clone(),
            }
        };

        RandomState {
            random,
            noises,
            functions,
            router,
            sampler,
            surface_system: SurfaceSystem,
            aquifer_random,
            ore_random,
            noise_instances,
            positional_randoms,
        }
    }

    /// `getOrCreateNoise(ResourceKey<NoiseParameters>)` — the
    /// `ConcurrentHashMap.computeIfAbsent` noise cache.
    pub fn get_or_create_noise(&self, name: &ResourceKey<NoiseParameters>) -> NormalNoise {
        let mut map = self.noise_instances.lock().unwrap();
        map.entry(name.clone())
            .or_insert_with(|| noises::instantiate(self.noises, &self.random, name))
            .clone()
    }

    /// `getOrCreateRandomFactory(Identifier)` — the
    /// `ConcurrentHashMap.computeIfAbsent` factory cache.
    pub fn get_or_create_random_factory(
        &self,
        name: &Identifier,
    ) -> AlgorithmPositionalRandomFactory {
        let mut map = self.positional_randoms.lock().unwrap();
        *map.entry(name.clone())
            .or_insert_with(|| self.random.from_hash_of_identifier(name).fork_positional())
    }

    /// `router()`.
    pub fn router(&self) -> &NoiseRouter {
        &self.router
    }

    /// `functions()` — the density-function registry (resolves
    /// `HolderHolder::Reference` values during the `NoiseChunk` wrap).
    pub fn functions(&self) -> &Registry<DensityFunctionValue> {
        self.functions
    }

    /// `sampler()`.
    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }

    /// `surfaceSystem()`.
    pub fn surface_system(&self) -> SurfaceSystem {
        self.surface_system
    }

    /// `aquiferRandom()`.
    pub fn aquifer_random(&self) -> AlgorithmPositionalRandomFactory {
        self.aquifer_random
    }

    /// `oreRandom()`.
    pub fn ore_random(&self) -> AlgorithmPositionalRandomFactory {
        self.ore_random
    }
}

/// `RandomState.NoiseWiringHelper` — the router-wiring visitor. Re-seeds every
/// `BlendedNoise` (`withNewRandom`), re-instantiates the legacy nether biome
/// noises, and binds every other noise through `getOrCreateNoise`.
struct NoiseWiringHelper<'a> {
    random: &'a AlgorithmPositionalRandomFactory,
    noises: &'a Registry<NoiseParameters>,
    /// The density-function registry — resolves `HolderHolder::Reference` values
    /// the router carries from a bootstrap (Java's `BuildState` binds every
    /// reference before the router reaches the wiring visitor).
    functions: &'a Registry<DensityFunctionValue>,
    seed: i64,
    use_legacy_init: bool,
    noise_instances: &'a Mutex<HashMap<ResourceKey<NoiseParameters>, NormalNoise>>,
    wrapped: Mutex<HashMap<IdentityKey, Arc<dyn DensityFunction>>>,
}

impl NoiseWiringHelper<'_> {
    /// `newLegacyInstance(long seedOffset)` — `new LegacyRandomSource(seed + seedOffset)`.
    fn new_legacy_instance(&self, seed_offset: i64) -> AlgorithmRandomSource {
        AlgorithmRandomSource::Legacy(LegacyRandomSource::new(self.seed.wrapping_add(seed_offset)))
    }

    /// `visitNoise(NoiseHolder)` — the per-noise re-instantiation.
    fn visit_noise_impl(&self, noise: &NoiseHolder) -> NoiseHolder {
        let noise_data = noise.noise_data();
        if noise_data.is_key(self.noises, &noises::TEMPERATURE_NETHER) {
            let mut random = self.new_legacy_instance(0);
            let new_noise = NormalNoise::create_legacy_nether_biome(
                &mut random,
                noise_data.value(self.noises).clone(),
            );
            NoiseHolder::new_with_noise(noise_data.clone(), Some(new_noise))
        } else if noise_data.is_key(self.noises, &noises::VEGETATION_NETHER) {
            let mut random = self.new_legacy_instance(1);
            let new_noise = NormalNoise::create_legacy_nether_biome(
                &mut random,
                noise_data.value(self.noises).clone(),
            );
            NoiseHolder::new_with_noise(noise_data.clone(), Some(new_noise))
        } else {
            let name = noise_data
                .unwrap_key(self.noises)
                .expect("registered noise holder has a key");
            let instantiate = {
                let mut map = self.noise_instances.lock().unwrap();
                map.entry(name.clone())
                    .or_insert_with(|| noises::instantiate(self.noises, self.random, &name))
                    .clone()
            };
            NoiseHolder::new_with_noise(noise_data.clone(), Some(instantiate))
        }
    }

    /// `wrapNew(DensityFunction)` — the `BlendedNoise`/`EndIslandDensityFunction`
    /// re-seed logic.
    fn wrap_new(&self, function: &dyn DensityFunction) -> Arc<dyn DensityFunction> {
        if let Some(noise) = function.as_any().downcast_ref::<BlendedNoise>() {
            let mut terrain_random: AlgorithmRandomSource = if self.use_legacy_init {
                self.new_legacy_instance(0)
            } else {
                self.random
                    .from_hash_of_identifier(&Identifier::with_default_namespace("terrain"))
            };
            Arc::new(noise.with_new_random(&mut terrain_random)) as Arc<dyn DensityFunction>
        } else if function
            .as_any()
            .downcast_ref::<crate::levelgen::noise::density_functions::EndIslandDensityFunction>()
            .is_some()
        {
            Arc::new(
                crate::levelgen::noise::density_functions::EndIslandDensityFunction::new(self.seed),
            ) as Arc<dyn DensityFunction>
        } else {
            function.clone_arc()
        }
    }
}

impl Visitor for NoiseWiringHelper<'_> {
    fn apply(&self, input: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        let key = IdentityKey::new(input.clone());
        {
            let wrapped = self.wrapped.lock().unwrap();
            if let Some(value) = wrapped.get(&key) {
                return value.clone();
            }
        }
        let value = self.wrap_new(input.as_ref());
        self.wrapped.lock().unwrap().insert(key, value.clone());
        value
    }

    fn visit_noise(&self, noise: &NoiseHolder) -> NoiseHolder {
        self.visit_noise_impl(noise)
    }

    fn resolve_holder(
        &self,
        holder: &Holder<Arc<dyn DensityFunction>>,
    ) -> Option<Arc<dyn DensityFunction>> {
        Some(holder.value(&*self.functions).clone())
    }
}

/// `RandomState`'s anonymous noise-flattener visitor — resolves every
/// `HolderHolder` to its value and unwraps every `Marker` to its wrapped
/// function (the `Climate.Sampler` inputs).
struct NoiseFlattener<'a> {
    functions: &'a Registry<DensityFunctionValue>,
    wrapped: Mutex<HashMap<IdentityKey, Arc<dyn DensityFunction>>>,
}

impl NoiseFlattener<'_> {
    fn wrap_new(&self, function: &dyn DensityFunction) -> Arc<dyn DensityFunction> {
        if let Some(holder) = function.as_any().downcast_ref::<HolderHolder>() {
            holder.function().value(self.functions).clone()
        } else if let Some(marker) = function.as_any().downcast_ref::<Marker>() {
            marker.wrapped().clone()
        } else {
            function.clone_arc()
        }
    }
}

impl Visitor for NoiseFlattener<'_> {
    fn apply(&self, input: &Arc<dyn DensityFunction>) -> Arc<dyn DensityFunction> {
        let key = IdentityKey::new(input.clone());
        {
            let wrapped = self.wrapped.lock().unwrap();
            if let Some(value) = wrapped.get(&key) {
                return value.clone();
            }
        }
        let value = self.wrap_new(input.as_ref());
        self.wrapped.lock().unwrap().insert(key, value.clone());
        value
    }

    fn resolve_holder(
        &self,
        holder: &Holder<Arc<dyn DensityFunction>>,
    ) -> Option<Arc<dyn DensityFunction>> {
        Some(holder.value(&*self.functions).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::worldgen::bootstrap_context::RecordingContext;
    use crate::data::worldgen::noise_data;
    use crate::levelgen::noisegen::noise_generator_settings::dummy;
    use crate::levelgen::noisegen::noise_router_data::bootstrap as density_function_bootstrap;
    use rivet_registry::RegistrationInfo;
    use rivet_registry::RegistryAccess;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::registry::Registry;
    use rivet_registry::root::AnyBox;
    use rivet_util::random::PositionalRandomFactory;

    /// A freshly-frozen noise registry (via `NoiseData.bootstrap`). Built
    /// per-call: the `RegistryAccess` value model shares registries by moving
    /// the unique `Box<dyn AnyRegistry>` (OWNERSHIP forbids `Arc<dyn
    /// AnyRegistry>`), so a test needing the noise registry in two accesses
    /// freezes two identical instances (same `RegistryId`, same elements).
    fn build_noise_registry() -> Registry<NoiseParameters> {
        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let mut noise_builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(noise_key);
        let mut noise_ctx = RecordingContext::<NoiseParameters>::new(
            RegistryId(0),
            (*crate::levelgen::noise::registry_keys::NOISE).clone(),
            RegistryAccess::empty(),
        );
        noise_data::bootstrap(&mut noise_ctx);
        for reg in noise_ctx.registrations() {
            noise_builder.register(
                &reg.key,
                Arc::new(reg.value.clone()),
                RegistrationInfo::BUILT_IN,
            );
        }
        noise_builder.freeze()
    }

    /// A `RegistryAccess` with the noise + density-function registries
    /// populated, plus the same again for the noise-flattener.
    fn make_access() -> RegistryAccess {
        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let df_key = &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION;
        let df_access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(noise_key.identifier().clone()),
            Box::new(build_noise_registry()) as AnyBox,
        )]);
        let mut df_builder: RegistryBuilder<DensityFunctionValue> = RegistryBuilder::new(df_key);
        let mut df_ctx = RecordingContext::<DensityFunctionValue>::new(
            RegistryId(1),
            (*crate::levelgen::noise::registry_keys::DENSITY_FUNCTION).clone(),
            df_access,
        );
        density_function_bootstrap(&mut df_ctx);
        for reg in df_ctx.registrations() {
            df_builder.register(
                &reg.key,
                Arc::new(reg.value.clone()),
                RegistrationInfo::BUILT_IN,
            );
        }
        let df_registry = df_builder.freeze();

        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(noise_key.identifier().clone()),
                Box::new(build_noise_registry()) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(df_key.identifier().clone()),
                Box::new(df_registry) as AnyBox,
            ),
        ])
    }

    #[test]
    fn create_wires_router_and_sampler() {
        let access = make_access();
        let noises = access
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry");
        let functions = access
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("df registry");
        let settings = dummy();
        let state = RandomState::create(&settings, noises, functions, 1234);
        // The router's final density is the wired (flattened) value — non-null.
        let _ = state.router().final_density();
        let _ = state.sampler().temperature;
        // The aquifer/ore random factories derive from the base random.
        let mut sb = String::new();
        state.ore_random().parity_config_string(&mut sb);
        assert!(!sb.is_empty());
    }

    #[test]
    fn get_or_create_noise_instantiates_and_caches() {
        let access = make_access();
        let noises = access
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry");
        let functions = access
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("df registry");
        let settings = dummy();
        let state = RandomState::create(&settings, noises, functions, 99);
        let a = state.get_or_create_noise(&noises::RIDGE);
        let b = state.get_or_create_noise(&noises::RIDGE);
        // Same key returns the same instantiated noise (the computeIfAbsent cache).
        assert_eq!(a.max_value(), b.max_value());
    }
}

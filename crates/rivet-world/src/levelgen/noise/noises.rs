//! Port of `net.minecraft.world.level.levelgen.Noises` (class, 26.2).
//!
//! The ~70 `ResourceKey<NoiseParameters>` constants (Paper's exact
//! declaration order) plus `instantiate` — the holder-resolving
//! `NormalNoise.create(context.fromHashOf(name), holder.value())`.
//!
//! The constants are `LazyLock` statics, not `const`: `Identifier` owns its
//! `String` fields (value type, faithful to Java), and `String::from` is not a
//! `const fn`, so no `Identifier` value can be a `const` — the same convention
//! as `rivet-registry::registries` (the `NOISE` key itself).

use crate::levelgen::noise::density_function::DensityFunction;
use crate::levelgen::random::PositionalRandomFactoryOverloads;
use crate::levelgen::synth::normal_noise::{NoiseParameters, NormalNoise};
use rivet_registry::Identifier;
use rivet_registry::ResourceKey;
use rivet_util::random::PositionalRandomFactory;
use std::sync::{Arc, LazyLock};

/// `net.minecraft.world.level.levelgen.Noises`.
pub struct Noises;

/// `Noises.createKey(String name)` — `ResourceKey.create(Registries.NOISE,
/// Identifier.withDefaultNamespace(name))`.
fn create_key(name: &str) -> ResourceKey<NoiseParameters> {
    ResourceKey::create(
        &crate::levelgen::noise::registry_keys::NOISE,
        Identifier::with_default_namespace(name),
    )
}

/// `Noises.TEMPERATURE` — `createKey("temperature")`.
pub static TEMPERATURE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("temperature"));
/// `Noises.VEGETATION` — `createKey("vegetation")`.
pub static VEGETATION: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("vegetation"));
/// `Noises.CONTINENTALNESS` — `createKey("continentalness")`.
pub static CONTINENTALNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("continentalness"));
/// `Noises.EROSION` — `createKey("erosion")`.
pub static EROSION: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("erosion"));
/// `Noises.TEMPERATURE_LARGE` — `createKey("temperature_large")`.
pub static TEMPERATURE_LARGE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("temperature_large"));
/// `Noises.VEGETATION_LARGE` — `createKey("vegetation_large")`.
pub static VEGETATION_LARGE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("vegetation_large"));
/// `Noises.CONTINENTALNESS_LARGE` — `createKey("continentalness_large")`.
pub static CONTINENTALNESS_LARGE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("continentalness_large"));
/// `Noises.EROSION_LARGE` — `createKey("erosion_large")`.
pub static EROSION_LARGE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("erosion_large"));
/// `Noises.RIDGE` — `createKey("ridge")`.
pub static RIDGE: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("ridge"));
/// `Noises.SHIFT` — `createKey("offset")`.
pub static SHIFT: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("offset"));
/// `Noises.TEMPERATURE_NETHER` — `createKey("nether/temperature")`.
pub static TEMPERATURE_NETHER: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("nether/temperature"));
/// `Noises.VEGETATION_NETHER` — `createKey("nether/vegetation")`.
pub static VEGETATION_NETHER: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("nether/vegetation"));
/// `Noises.AQUIFER_BARRIER` — `createKey("aquifer_barrier")`.
pub static AQUIFER_BARRIER: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("aquifer_barrier"));
/// `Noises.AQUIFER_FLUID_LEVEL_FLOODEDNESS` —
/// `createKey("aquifer_fluid_level_floodedness")`.
pub static AQUIFER_FLUID_LEVEL_FLOODEDNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("aquifer_fluid_level_floodedness"));
/// `Noises.AQUIFER_LAVA` — `createKey("aquifer_lava")`.
pub static AQUIFER_LAVA: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("aquifer_lava"));
/// `Noises.AQUIFER_FLUID_LEVEL_SPREAD` — `createKey("aquifer_fluid_level_spread")`.
pub static AQUIFER_FLUID_LEVEL_SPREAD: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("aquifer_fluid_level_spread"));
/// `Noises.PILLAR` — `createKey("pillar")`.
pub static PILLAR: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("pillar"));
/// `Noises.PILLAR_RARENESS` — `createKey("pillar_rareness")`.
pub static PILLAR_RARENESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("pillar_rareness"));
/// `Noises.PILLAR_THICKNESS` — `createKey("pillar_thickness")`.
pub static PILLAR_THICKNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("pillar_thickness"));
/// `Noises.SPAGHETTI_2D` — `createKey("spaghetti_2d")`.
pub static SPAGHETTI_2D: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_2d"));
/// `Noises.SPAGHETTI_2D_ELEVATION` — `createKey("spaghetti_2d_elevation")`.
pub static SPAGHETTI_2D_ELEVATION: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_2d_elevation"));
/// `Noises.SPAGHETTI_2D_MODULATOR` — `createKey("spaghetti_2d_modulator")`.
pub static SPAGHETTI_2D_MODULATOR: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_2d_modulator"));
/// `Noises.SPAGHETTI_2D_THICKNESS` — `createKey("spaghetti_2d_thickness")`.
pub static SPAGHETTI_2D_THICKNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_2d_thickness"));
/// `Noises.SPAGHETTI_3D_1` — `createKey("spaghetti_3d_1")`.
pub static SPAGHETTI_3D_1: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_3d_1"));
/// `Noises.SPAGHETTI_3D_2` — `createKey("spaghetti_3d_2")`.
pub static SPAGHETTI_3D_2: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_3d_2"));
/// `Noises.SPAGHETTI_3D_RARITY` — `createKey("spaghetti_3d_rarity")`.
pub static SPAGHETTI_3D_RARITY: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_3d_rarity"));
/// `Noises.SPAGHETTI_3D_THICKNESS` — `createKey("spaghetti_3d_thickness")`.
pub static SPAGHETTI_3D_THICKNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_3d_thickness"));
/// `Noises.SPAGHETTI_ROUGHNESS` — `createKey("spaghetti_roughness")`.
pub static SPAGHETTI_ROUGHNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_roughness"));
/// `Noises.SPAGHETTI_ROUGHNESS_MODULATOR` — `createKey("spaghetti_roughness_modulator")`.
pub static SPAGHETTI_ROUGHNESS_MODULATOR: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("spaghetti_roughness_modulator"));
/// `Noises.CAVE_ENTRANCE` — `createKey("cave_entrance")`.
pub static CAVE_ENTRANCE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("cave_entrance"));
/// `Noises.CAVE_LAYER` — `createKey("cave_layer")`.
pub static CAVE_LAYER: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("cave_layer"));
/// `Noises.CAVE_CHEESE` — `createKey("cave_cheese")`.
pub static CAVE_CHEESE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("cave_cheese"));
/// `Noises.ORE_VEININESS` — `createKey("ore_veininess")`.
pub static ORE_VEININESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("ore_veininess"));
/// `Noises.ORE_VEIN_A` — `createKey("ore_vein_a")`.
pub static ORE_VEIN_A: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("ore_vein_a"));
/// `Noises.ORE_VEIN_B` — `createKey("ore_vein_b")`.
pub static ORE_VEIN_B: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("ore_vein_b"));
/// `Noises.ORE_GAP` — `createKey("ore_gap")`.
pub static ORE_GAP: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("ore_gap"));
/// `Noises.NOODLE` — `createKey("noodle")`.
pub static NOODLE: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("noodle"));
/// `Noises.NOODLE_THICKNESS` — `createKey("noodle_thickness")`.
pub static NOODLE_THICKNESS: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("noodle_thickness"));
/// `Noises.NOODLE_RIDGE_A` — `createKey("noodle_ridge_a")`.
pub static NOODLE_RIDGE_A: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("noodle_ridge_a"));
/// `Noises.NOODLE_RIDGE_B` — `createKey("noodle_ridge_b")`.
pub static NOODLE_RIDGE_B: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("noodle_ridge_b"));
/// `Noises.JAGGED` — `createKey("jagged")`.
pub static JAGGED: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("jagged"));
/// `Noises.SURFACE` — `createKey("surface")`.
pub static SURFACE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("surface"));
/// `Noises.SURFACE_SECONDARY` — `createKey("surface_secondary")`.
pub static SURFACE_SECONDARY: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("surface_secondary"));
/// `Noises.CLAY_BANDS_OFFSET` — `createKey("clay_bands_offset")`.
pub static CLAY_BANDS_OFFSET: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("clay_bands_offset"));
/// `Noises.BADLANDS_PILLAR` — `createKey("badlands_pillar")`.
pub static BADLANDS_PILLAR: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("badlands_pillar"));
/// `Noises.BADLANDS_PILLAR_ROOF` — `createKey("badlands_pillar_roof")`.
pub static BADLANDS_PILLAR_ROOF: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("badlands_pillar_roof"));
/// `Noises.BADLANDS_SURFACE` — `createKey("badlands_surface")`.
pub static BADLANDS_SURFACE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("badlands_surface"));
/// `Noises.ICEBERG_PILLAR` — `createKey("iceberg_pillar")`.
pub static ICEBERG_PILLAR: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("iceberg_pillar"));
/// `Noises.ICEBERG_PILLAR_ROOF` — `createKey("iceberg_pillar_roof")`.
pub static ICEBERG_PILLAR_ROOF: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("iceberg_pillar_roof"));
/// `Noises.ICEBERG_SURFACE` — `createKey("iceberg_surface")`.
pub static ICEBERG_SURFACE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("iceberg_surface"));
/// `Noises.SULFUR_CAVE_GRADIENT` — `createKey("sulfur_cave_gradient")`.
pub static SULFUR_CAVE_GRADIENT: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("sulfur_cave_gradient"));
/// `Noises.SWAMP` — `createKey("surface_swamp")`.
pub static SWAMP: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("surface_swamp"));
/// `Noises.CALCITE` — `createKey("calcite")`.
pub static CALCITE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("calcite"));
/// `Noises.GRAVEL` — `createKey("gravel")`.
pub static GRAVEL: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("gravel"));
/// `Noises.POWDER_SNOW` — `createKey("powder_snow")`.
pub static POWDER_SNOW: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("powder_snow"));
/// `Noises.PACKED_ICE` — `createKey("packed_ice")`.
pub static PACKED_ICE: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("packed_ice"));
/// `Noises.ICE` — `createKey("ice")`.
pub static ICE: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("ice"));
/// `Noises.SOUL_SAND_LAYER` — `createKey("soul_sand_layer")`.
pub static SOUL_SAND_LAYER: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("soul_sand_layer"));
/// `Noises.GRAVEL_LAYER` — `createKey("gravel_layer")`.
pub static GRAVEL_LAYER: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("gravel_layer"));
/// `Noises.PATCH` — `createKey("patch")`.
pub static PATCH: LazyLock<ResourceKey<NoiseParameters>> = LazyLock::new(|| create_key("patch"));
/// `Noises.NETHERRACK` — `createKey("netherrack")`.
pub static NETHERRACK: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("netherrack"));
/// `Noises.NETHER_WART` — `createKey("nether_wart")`.
pub static NETHER_WART: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("nether_wart"));
/// `Noises.NETHER_STATE_SELECTOR` — `createKey("nether_state_selector")`.
pub static NETHER_STATE_SELECTOR: LazyLock<ResourceKey<NoiseParameters>> =
    LazyLock::new(|| create_key("nether_state_selector"));
/// `Registries.DENSITY_FUNCTION` — the erased density-function registry key,
/// re-exported here for `DensityFunction.CODEC`'s `RegistryFileCodec` (the
/// `#177` key lives in `registry_keys`; `density_function.rs` references it
/// through this alias to keep the registry-key module single-owner).
pub fn noise_registry_key_for_density_function()
-> &'static rivet_registry::ResourceKey<rivet_registry::Registry<Arc<dyn DensityFunction>>> {
    &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION
}

/// `Noises.instantiate(HolderGetter<NoiseParameters>, PositionalRandomFactory,
/// ResourceKey<NoiseParameters>)` — `NormalNoise.create(context.fromHashOf(
/// name.identifier()), noises.getOrThrow(name).value())`.
///
/// The Rust holder is a pure `(RegistryId, id)` reference (OWNERSHIP's
/// back-reference rule), so the value is resolved through a
/// `HolderLookup<NoiseParameters>` instead of Java's `HolderGetter` — every
/// `Holder::value()` in this codebase takes `&dyn HolderLookup` (the
/// documented `holder_lookup` binding-model deviation).
pub fn instantiate(
    noises: &dyn rivet_registry::holder_lookup::HolderLookup<NoiseParameters>,
    context: &impl PositionalRandomFactory,
    name: &ResourceKey<NoiseParameters>,
) -> NormalNoise {
    let holder = noises.get_or_throw(name);
    let mut random = context.from_hash_of_identifier(name.identifier());
    let parameters = holder.value(noises).clone();
    NormalNoise::create(&mut random, parameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_have_java_identifiers() {
        assert_eq!(
            TEMPERATURE.identifier().to_string(),
            "minecraft:temperature"
        );
        assert_eq!(
            CONTINENTALNESS.identifier().to_string(),
            "minecraft:continentalness"
        );
        assert_eq!(
            TEMPERATURE_NETHER.identifier().to_string(),
            "minecraft:nether/temperature"
        );
        assert_eq!(SHIFT.identifier().to_string(), "minecraft:offset");
        assert_eq!(
            SULFUR_CAVE_GRADIENT.identifier().to_string(),
            "minecraft:sulfur_cave_gradient"
        );
        assert_eq!(
            NETHER_STATE_SELECTOR.identifier().to_string(),
            "minecraft:nether_state_selector"
        );
    }

    #[test]
    fn keys_live_in_the_noise_registry() {
        // Java: `ResourceKey.create(Registries.NOISE, ...)` — the registry-name
        // identifier is `minecraft:worldgen/noise` for every key.
        for key in [
            &*TEMPERATURE,
            &*RIDGE,
            &*AQUIFER_LAVA,
            &*NETHER_STATE_SELECTOR,
        ] {
            assert_eq!(key.registry().to_string(), "minecraft:worldgen/noise");
        }
    }
}

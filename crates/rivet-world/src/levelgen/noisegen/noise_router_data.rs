//! Port of `net.minecraft.world.level.levelgen.NoiseRouterData` (26.2).
//!
//! The shared noise/function registry keys and the `overworld`/`nether`/`end`/
//! `caves`/`floatingIslands` router builders. Every density function is built
//! through the `DensityFunctions` combinators and resolved through
//! `HolderGetter`s passed by the caller (the `BootstrapContext.lookup` views).
//!
//! Translation notes:
//! - The `DensityFunction` element is the erased `Arc<dyn DensityFunction>`
//!   carrier (the `#177` registry). `HolderHolder::new(Holder<Arc<dyn
//!   DensityFunction>>)` is Java's `new DensityFunctions.HolderHolder(holder)`.
//! - `registerAndWrap` returns the holder-wrapped function; `getFunction`
//!   resolves a key through the `HolderGetter`.
//! - `-0.50375F` / `-0.08F` are `float` literals meeting `double` parameters —
//!   widened via `-0.50375f32 as f64` (PORTING.md float-promotion rule).
//! - The `mappedNoise` overloads: the 3-arg `(noise, minTarget, maxTarget)` is
//!   `(1.0, 1.0, minTarget, maxTarget)`; the 4-arg `(noise, yScale, minTarget,
//!   maxTarget)` is `(1.0, yScale, minTarget, maxTarget)` (Paper's deprecated
//!   xzScale overload). `noise`'s 1-arg `(noise)` is `(1.0, 1.0)`; the 2-arg
//!   `(noise, yScale)` is `(1.0, yScale)`.
//! - `Mth.lerp(DensityFunction, double, DensityFunction)` maps to
//!   `density_functions::lerp_double`.
//! - `OreVeinifier.VeinType` min/max are folded like Java's
//!   `Stream.of(values()).mapToInt(...).min()/max()`.

use crate::data::worldgen::bootstrap_context::BootstrapContext;
use crate::data::worldgen::terrain_provider;
use crate::level::dimension::dimension_type::{MAX_Y as DIM_MAX_Y, MIN_Y as DIM_MIN_Y};
use crate::levelgen::noise::density_function::DensityFunction;
use crate::levelgen::noise::density_functions::{self as fns, HolderHolder, SplineCoordinate};
use crate::levelgen::noise::noise_router::NoiseRouter;
use crate::levelgen::noise::noises;
use crate::levelgen::noisegen::ore_veinifier::VeinType;
use crate::levelgen::synth::blended_noise::BlendedNoise;
use crate::levelgen::synth::normal_noise::NoiseParameters;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::{Identifier, ResourceKey};
use rivet_util::mth;
use std::sync::{Arc, LazyLock};

/// The erased density-function element (Java's `DensityFunction`).
pub type DensityFunctionValue = Arc<dyn DensityFunction>;
/// `ResourceKey<DensityFunction>`.
pub type DensityFunctionKey = ResourceKey<DensityFunctionValue>;

/// `NoiseRouterData.GLOBAL_OFFSET` — `-0.50375F`.
pub const GLOBAL_OFFSET: f32 = -0.50375;
/// `ORE_THICKNESS` — `0.08F`.
const ORE_THICKNESS: f32 = 0.08;
/// `VEININESS_FREQUENCY` — `1.5`.
const VEININESS_FREQUENCY: f64 = 1.5;
/// `NOODLE_SPACING_AND_STRAIGHTNESS` — `1.5`. Java declares this constant but
/// never reads it (a leftover); the port keeps it for fidelity.
#[allow(dead_code)]
const NOODLE_SPACING_AND_STRAIGHTNESS: f64 = 1.5;
/// `SURFACE_DENSITY_THRESHOLD` — `1.5625`.
const SURFACE_DENSITY_THRESHOLD: f64 = 1.5625;
/// `CHEESE_NOISE_TARGET` — `-0.703125`.
const CHEESE_NOISE_TARGET: f64 = -0.703125;
/// `NoiseRouterData.NOISE_ZERO` — `0.390625`.
pub const NOISE_ZERO: f64 = 0.390625;
/// `NoiseRouterData.ISLAND_CHUNK_DISTANCE` — `64`.
pub const ISLAND_CHUNK_DISTANCE: i32 = 64;
/// `NoiseRouterData.ISLAND_CHUNK_DISTANCE_SQR` — `4096L`.
pub const ISLAND_CHUNK_DISTANCE_SQR: i64 = 4096;
/// `DENSITY_Y_ANCHOR_BOTTOM` — `-64`.
const DENSITY_Y_ANCHOR_BOTTOM: i32 = -64;
/// `DENSITY_Y_ANCHOR_TOP` — `320`.
const DENSITY_Y_ANCHOR_TOP: i32 = 320;
/// `DENSITY_Y_BOTTOM` — `1.5`.
const DENSITY_Y_BOTTOM: f64 = 1.5;
/// `DENSITY_Y_TOP` — `-1.5`.
const DENSITY_Y_TOP: f64 = -1.5;
/// `OVERWORLD_BOTTOM_SLIDE_HEIGHT` — `24`. Java declares it (NoiseRouterData.java:31)
/// but the overworld bootstrap passes the literal; kept for fidelity.
#[allow(dead_code)]
const OVERWORLD_BOTTOM_SLIDE_HEIGHT: i32 = 24;
/// `BASE_DENSITY_MULTIPLIER` — `4.0`.
const BASE_DENSITY_MULTIPLIER: f64 = 4.0;
/// `BLENDING_FACTOR` — `constant(10.0)`.
fn blending_factor() -> DensityFunctionValue {
    fns::constant(10.0)
}
/// `BLENDING_JAGGEDNESS` — `zero()`.
fn blending_jaggedness() -> DensityFunctionValue {
    fns::zero()
}

/// `createKey(String)` — `ResourceKey.create(Registries.DENSITY_FUNCTION, Identifier.withDefaultNamespace(name))`.
fn create_key(name: &str) -> DensityFunctionKey {
    ResourceKey::create(
        &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION,
        Identifier::with_default_namespace(name),
    )
}

macro_rules! key {
    ($name:literal) => {
        LazyLock::new(|| create_key($name))
    };
}

/// `ZERO` — `"zero"`.
static ZERO: LazyLock<DensityFunctionKey> = key!("zero");
/// `Y` — `"y"`.
static Y: LazyLock<DensityFunctionKey> = key!("y");
/// `SHIFT_X` — `"shift_x"`.
static SHIFT_X: LazyLock<DensityFunctionKey> = key!("shift_x");
/// `SHIFT_Z` — `"shift_z"`.
static SHIFT_Z: LazyLock<DensityFunctionKey> = key!("shift_z");
/// `BASE_3D_NOISE_OVERWORLD` — `"overworld/base_3d_noise"`.
static BASE_3D_NOISE_OVERWORLD: LazyLock<DensityFunctionKey> = key!("overworld/base_3d_noise");
/// `BASE_3D_NOISE_NETHER` — `"nether/base_3d_noise"`.
static BASE_3D_NOISE_NETHER: LazyLock<DensityFunctionKey> = key!("nether/base_3d_noise");
/// `BASE_3D_NOISE_END` — `"end/base_3d_noise"`.
static BASE_3D_NOISE_END: LazyLock<DensityFunctionKey> = key!("end/base_3d_noise");
/// `CONTINENTS` — `"overworld/continents"`.
pub static CONTINENTS: LazyLock<DensityFunctionKey> = key!("overworld/continents");
/// `EROSION` — `"overworld/erosion"`.
pub static EROSION: LazyLock<DensityFunctionKey> = key!("overworld/erosion");
/// `RIDGES` — `"overworld/ridges"`.
pub static RIDGES: LazyLock<DensityFunctionKey> = key!("overworld/ridges");
/// `RIDGES_FOLDED` — `"overworld/ridges_folded"`.
pub static RIDGES_FOLDED: LazyLock<DensityFunctionKey> = key!("overworld/ridges_folded");
/// `OFFSET` — `"overworld/offset"`.
pub static OFFSET: LazyLock<DensityFunctionKey> = key!("overworld/offset");
/// `FACTOR` — `"overworld/factor"`.
pub static FACTOR: LazyLock<DensityFunctionKey> = key!("overworld/factor");
/// `JAGGEDNESS` — `"overworld/jaggedness"`.
pub static JAGGEDNESS: LazyLock<DensityFunctionKey> = key!("overworld/jaggedness");
/// `DEPTH` — `"overworld/depth"`.
pub static DEPTH: LazyLock<DensityFunctionKey> = key!("overworld/depth");
/// `SLOPED_CHEESE` — `"overworld/sloped_cheese"`.
static SLOPED_CHEESE: LazyLock<DensityFunctionKey> = key!("overworld/sloped_cheese");
/// `CONTINENTS_LARGE` — `"overworld_large_biomes/continents"`.
pub static CONTINENTS_LARGE: LazyLock<DensityFunctionKey> =
    key!("overworld_large_biomes/continents");
/// `EROSION_LARGE` — `"overworld_large_biomes/erosion"`.
pub static EROSION_LARGE: LazyLock<DensityFunctionKey> = key!("overworld_large_biomes/erosion");
/// `OFFSET_LARGE` — `"overworld_large_biomes/offset"`.
static OFFSET_LARGE: LazyLock<DensityFunctionKey> = key!("overworld_large_biomes/offset");
/// `FACTOR_LARGE` — `"overworld_large_biomes/factor"`.
static FACTOR_LARGE: LazyLock<DensityFunctionKey> = key!("overworld_large_biomes/factor");
/// `JAGGEDNESS_LARGE` — `"overworld_large_biomes/jaggedness"`.
static JAGGEDNESS_LARGE: LazyLock<DensityFunctionKey> = key!("overworld_large_biomes/jaggedness");
/// `DEPTH_LARGE` — `"overworld_large_biomes/depth"`.
static DEPTH_LARGE: LazyLock<DensityFunctionKey> = key!("overworld_large_biomes/depth");
/// `SLOPED_CHEESE_LARGE` — `"overworld_large_biomes/sloped_cheese"`.
static SLOPED_CHEESE_LARGE: LazyLock<DensityFunctionKey> =
    key!("overworld_large_biomes/sloped_cheese");
/// `OFFSET_AMPLIFIED` — `"overworld_amplified/offset"`.
static OFFSET_AMPLIFIED: LazyLock<DensityFunctionKey> = key!("overworld_amplified/offset");
/// `FACTOR_AMPLIFIED` — `"overworld_amplified/factor"`.
static FACTOR_AMPLIFIED: LazyLock<DensityFunctionKey> = key!("overworld_amplified/factor");
/// `JAGGEDNESS_AMPLIFIED` — `"overworld_amplified/jaggedness"`.
static JAGGEDNESS_AMPLIFIED: LazyLock<DensityFunctionKey> = key!("overworld_amplified/jaggedness");
/// `DEPTH_AMPLIFIED` — `"overworld_amplified/depth"`.
static DEPTH_AMPLIFIED: LazyLock<DensityFunctionKey> = key!("overworld_amplified/depth");
/// `SLOPED_CHEESE_AMPLIFIED` — `"overworld_amplified/sloped_cheese"`.
static SLOPED_CHEESE_AMPLIFIED: LazyLock<DensityFunctionKey> =
    key!("overworld_amplified/sloped_cheese");
/// `SLOPED_CHEESE_END` — `"end/sloped_cheese"`.
static SLOPED_CHEESE_END: LazyLock<DensityFunctionKey> = key!("end/sloped_cheese");
/// `SPAGHETTI_ROUGHNESS_FUNCTION` — `"overworld/caves/spaghetti_roughness_function"`.
static SPAGHETTI_ROUGHNESS_FUNCTION: LazyLock<DensityFunctionKey> =
    key!("overworld/caves/spaghetti_roughness_function");
/// `ENTRANCES` — `"overworld/caves/entrances"`.
static ENTRANCES: LazyLock<DensityFunctionKey> = key!("overworld/caves/entrances");
/// `NOODLE` — `"overworld/caves/noodle"`.
static NOODLE: LazyLock<DensityFunctionKey> = key!("overworld/caves/noodle");
/// `PILLARS` — `"overworld/caves/pillars"`.
static PILLARS: LazyLock<DensityFunctionKey> = key!("overworld/caves/pillars");
/// `SPAGHETTI_2D_THICKNESS_MODULATOR` —
/// `"overworld/caves/spaghetti_2d_thickness_modulator"`.
static SPAGHETTI_2D_THICKNESS_MODULATOR: LazyLock<DensityFunctionKey> =
    key!("overworld/caves/spaghetti_2d_thickness_modulator");
/// `SPAGHETTI_2D` — `"overworld/caves/spaghetti_2d"`.
static SPAGHETTI_2D: LazyLock<DensityFunctionKey> = key!("overworld/caves/spaghetti_2d");

/// `NoiseRouterData.bootstrap(BootstrapContext<DensityFunction>)` — registers
/// every shared density function in declaration order and returns the `PILLARS`
/// holder (Java's `Holder<? extends DensityFunction>`).
pub fn bootstrap(
    context: &mut impl BootstrapContext<DensityFunctionValue>,
) -> Holder<DensityFunctionValue> {
    context.register_default(&ZERO, fns::zero());
    let below_bottom = DIM_MIN_Y * 2;
    let above_top = DIM_MAX_Y * 2;
    context.register_default(
        &Y,
        fns::y_clamped_gradient(
            below_bottom,
            above_top,
            below_bottom as f64,
            above_top as f64,
        ),
    );
    // Each `noises`/`functions` lookup resolves through the `&mut context`
    // (Java's `BootstrapContext` re-resolves the getters per call), so every
    // owned value is computed inside a block that releases the borrow before
    // the `&mut` register call — the `NoiseGeneratorSettings.bootstrap` idiom.
    let shift_x = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::cache2d(fns::shift_a(
            noises.get_or_throw(&noises::SHIFT),
        )))
    };
    let shift_x = register_and_wrap(context, &SHIFT_X, shift_x);
    let shift_z = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::cache2d(fns::shift_b(
            noises.get_or_throw(&noises::SHIFT),
        )))
    };
    let shift_z = register_and_wrap(context, &SHIFT_Z, shift_z);
    context.register_default(
        &BASE_3D_NOISE_OVERWORLD,
        Arc::new(BlendedNoise::create_unseeded(0.25, 0.125, 80.0, 160.0, 8.0))
            as DensityFunctionValue,
    );
    context.register_default(
        &BASE_3D_NOISE_NETHER,
        Arc::new(BlendedNoise::create_unseeded(0.25, 0.375, 80.0, 60.0, 8.0))
            as DensityFunctionValue,
    );
    context.register_default(
        &BASE_3D_NOISE_END,
        Arc::new(BlendedNoise::create_unseeded(0.25, 0.25, 80.0, 160.0, 4.0))
            as DensityFunctionValue,
    );
    let continents = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::shifted_noise2d(
            shift_x.clone(),
            shift_z.clone(),
            0.25,
            noises.get_or_throw(&noises::CONTINENTALNESS),
        ))
    };
    let continents = register_and_wrap(context, &CONTINENTS, continents);
    let erosion = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::shifted_noise2d(
            shift_x.clone(),
            shift_z.clone(),
            0.25,
            noises.get_or_throw(&noises::EROSION),
        ))
    };
    let erosion = register_and_wrap(context, &EROSION, erosion);
    let ridge = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::shifted_noise2d(
            shift_x.clone(),
            shift_z.clone(),
            0.25,
            noises.get_or_throw(&noises::RIDGE),
        ))
    };
    let ridge = register_and_wrap(context, &RIDGES, ridge);
    context.register_default(&RIDGES_FOLDED, peaks_and_valleys(ridge));
    let jagged_noise = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::noise(noises.get_or_throw(&noises::JAGGED), 1500.0, 0.0)
    };
    register_terrain_noises(
        context,
        jagged_noise.clone(),
        continents.clone(),
        erosion.clone(),
        &OFFSET,
        &FACTOR,
        &JAGGEDNESS,
        &DEPTH,
        &SLOPED_CHEESE,
        false,
    );
    let continents_large = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::shifted_noise2d(
            shift_x.clone(),
            shift_z.clone(),
            0.25,
            noises.get_or_throw(&noises::CONTINENTALNESS_LARGE),
        ))
    };
    let continents_large = register_and_wrap(context, &CONTINENTS_LARGE, continents_large);
    let erosion_large = {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::flat_cache(fns::shifted_noise2d(
            shift_x.clone(),
            shift_z.clone(),
            0.25,
            noises.get_or_throw(&noises::EROSION_LARGE),
        ))
    };
    let erosion_large = register_and_wrap(context, &EROSION_LARGE, erosion_large);
    register_terrain_noises(
        context,
        jagged_noise.clone(),
        continents_large,
        erosion_large,
        &OFFSET_LARGE,
        &FACTOR_LARGE,
        &JAGGEDNESS_LARGE,
        &DEPTH_LARGE,
        &SLOPED_CHEESE_LARGE,
        false,
    );
    register_terrain_noises(
        context,
        jagged_noise,
        continents,
        erosion,
        &OFFSET_AMPLIFIED,
        &FACTOR_AMPLIFIED,
        &JAGGEDNESS_AMPLIFIED,
        &DEPTH_AMPLIFIED,
        &SLOPED_CHEESE_AMPLIFIED,
        true,
    );
    context.register_default(&SLOPED_CHEESE_END, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        fns::add(
            fns::end_islands(0),
            get_function(functions, &BASE_3D_NOISE_END),
        )
    });
    context.register_default(&SPAGHETTI_ROUGHNESS_FUNCTION, {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        spaghetti_roughness_function(noises)
    });
    context.register_default(&SPAGHETTI_2D_THICKNESS_MODULATOR, {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        fns::cache_once(fns::mapped_noise(
            noises.get_or_throw(&noises::SPAGHETTI_2D_THICKNESS),
            2.0,
            1.0,
            -0.6,
            -1.3,
        ))
    });
    context.register_default(&SPAGHETTI_2D, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        spaghetti_2d(functions, noises)
    });
    context.register_default(&ENTRANCES, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        entrances(functions, noises)
    });
    context.register_default(&NOODLE, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        noodle(functions, noises)
    });
    context.register_default(&PILLARS, {
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in bootstrap");
        pillars(noises)
    })
}

/// `registerTerrainNoises(context, functions, jaggedNoise, continentsFunction,
/// erosionFunction, offsetName, factorName, jaggednessName, depthName,
/// slopedCheeseName, amplified)`. The Rust port drops the `functions`
/// parameter: it re-resolves the `DENSITY_FUNCTION` getter from `context` per
/// use (see the `bootstrap` borrow note), mirroring `NoiseGeneratorSettings`.
#[allow(clippy::too_many_arguments)]
fn register_terrain_noises(
    context: &mut impl BootstrapContext<DensityFunctionValue>,
    jagged_noise: DensityFunctionValue,
    continents_function: DensityFunctionValue,
    erosion_function: DensityFunctionValue,
    offset_name: &DensityFunctionKey,
    factor_name: &DensityFunctionKey,
    jaggedness_name: &DensityFunctionKey,
    depth_name: &DensityFunctionKey,
    sloped_cheese_name: &DensityFunctionKey,
    amplified: bool,
) {
    let continents = SplineCoordinate::new(continents_function);
    let erosion = SplineCoordinate::new(erosion_function);
    let weirdness = SplineCoordinate::new({
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        get_function(functions, &RIDGES)
    });
    let ridges = SplineCoordinate::new({
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        get_function(functions, &RIDGES_FOLDED)
    });
    let offset = register_and_wrap(
        context,
        offset_name,
        spline_with_blending(
            fns::add(
                fns::constant(GLOBAL_OFFSET as f64),
                fns::spline(terrain_provider::overworld_offset(
                    continents.clone(),
                    erosion.clone(),
                    ridges.clone(),
                    amplified,
                )),
            ),
            fns::blend_offset(),
        ),
    );
    let factor = register_and_wrap(
        context,
        factor_name,
        spline_with_blending(
            fns::spline(terrain_provider::overworld_factor(
                continents.clone(),
                erosion.clone(),
                weirdness.clone(),
                ridges.clone(),
                amplified,
            )),
            blending_factor(),
        ),
    );
    let depth = register_and_wrap(context, depth_name, offset_to_depth(offset));
    let unscaled_jaggedness = register_and_wrap(
        context,
        jaggedness_name,
        spline_with_blending(
            fns::spline(terrain_provider::overworld_jaggedness(
                continents, erosion, weirdness, ridges, amplified,
            )),
            blending_jaggedness(),
        ),
    );
    let jaggedness = fns::flat_cache(fns::mul(unscaled_jaggedness, jagged_noise.half_negative()));
    let initial_density = noise_gradient_density(factor, fns::add(depth, jaggedness));
    context.register_default(sloped_cheese_name, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in bootstrap");
        fns::add(
            initial_density,
            get_function(functions, &BASE_3D_NOISE_OVERWORLD),
        )
    });
}

/// `offsetToDepth(offset)` — `add(yClampedGradient(-64, 320, 1.5, -1.5), offset)`.
fn offset_to_depth(offset: DensityFunctionValue) -> DensityFunctionValue {
    fns::add(
        fns::y_clamped_gradient(
            DENSITY_Y_ANCHOR_BOTTOM,
            DENSITY_Y_ANCHOR_TOP,
            DENSITY_Y_BOTTOM,
            DENSITY_Y_TOP,
        ),
        offset,
    )
}

/// `registerAndWrap(context, name, value)` — `new
/// DensityFunctions.HolderHolder(context.register(name, value))`.
fn register_and_wrap(
    context: &mut impl BootstrapContext<DensityFunctionValue>,
    name: &DensityFunctionKey,
    value: DensityFunctionValue,
) -> DensityFunctionValue {
    let holder = context.register_default(name, value);
    Arc::new(HolderHolder::new(holder))
}

/// `getFunction(functions, name)` — `new
/// DensityFunctions.HolderHolder(functions.getOrThrow(name))`.
fn get_function(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    name: &DensityFunctionKey,
) -> DensityFunctionValue {
    Arc::new(HolderHolder::new(functions.get_or_throw(name)))
}

/// `peaksAndValleys(DensityFunction weirdness)` — the folded-ridge transform.
fn peaks_and_valleys(weirdness: DensityFunctionValue) -> DensityFunctionValue {
    fns::mul(
        fns::add(
            fns::add(weirdness.abs(), fns::constant(-0.6666666666666666)).abs(),
            fns::constant(-0.3333333333333333),
        ),
        fns::constant(-3.0),
    )
}

/// `NoiseRouterData.peaksAndValleys(float weirdness)` — the float overload
/// delegating to `TerrainProvider.peaksAndValleys`.
pub fn peaks_and_valleys_f32(weirdness: f32) -> f32 {
    terrain_provider::peaks_and_valleys(weirdness)
}

/// `spaghettiRoughnessFunction(noises)`.
fn spaghetti_roughness_function(
    noises: &dyn HolderGetter<NoiseParameters>,
) -> DensityFunctionValue {
    // Java's 1-arg `noise(noiseData)` defaults to `(1.0, 1.0)`.
    let spaghetti_roughness_noise =
        fns::noise(noises.get_or_throw(&noises::SPAGHETTI_ROUGHNESS), 1.0, 1.0);
    let spaghetti_roughness_modulator = fns::mapped_noise(
        noises.get_or_throw(&noises::SPAGHETTI_ROUGHNESS_MODULATOR),
        1.0,
        1.0,
        0.0,
        -0.1,
    );
    fns::cache_once(fns::mul(
        spaghetti_roughness_modulator,
        fns::add(spaghetti_roughness_noise.abs(), fns::constant(-0.4)),
    ))
}

/// `entrances(functions, noises)`.
fn entrances(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
) -> DensityFunctionValue {
    let spaghetti_3d_rarity_modulator = fns::cache_once(fns::noise(
        noises.get_or_throw(&noises::SPAGHETTI_3D_RARITY),
        2.0,
        1.0,
    ));
    let spaghetti_3d_thickness_modulator = fns::mapped_noise(
        noises.get_or_throw(&noises::SPAGHETTI_3D_THICKNESS),
        1.0,
        1.0,
        -0.065,
        -0.088,
    );
    let spaghetti_3d_cave_1 = QuantizedSpaghettiRarity::wrap_rarity_3d(
        spaghetti_3d_rarity_modulator.clone(),
        noises.get_or_throw(&noises::SPAGHETTI_3D_1),
    );
    let spaghetti_3d_cave_2 = QuantizedSpaghettiRarity::wrap_rarity_3d(
        spaghetti_3d_rarity_modulator,
        noises.get_or_throw(&noises::SPAGHETTI_3D_2),
    );
    let spaghetti_3d_function = fns::add(
        fns::max(spaghetti_3d_cave_1, spaghetti_3d_cave_2),
        spaghetti_3d_thickness_modulator,
    )
    .clamp(-1.0, 1.0);
    let spaghetti_roughness_function = get_function(functions, &SPAGHETTI_ROUGHNESS_FUNCTION);
    let big_entrance_noise_source =
        fns::noise(noises.get_or_throw(&noises::CAVE_ENTRANCE), 0.75, 0.5);
    let big_entrances_function = fns::add(
        fns::add(big_entrance_noise_source, fns::constant(0.37)),
        fns::y_clamped_gradient(-10, 30, 0.3, 0.0),
    );
    fns::cache_once(fns::min(
        big_entrances_function,
        fns::add(spaghetti_roughness_function, spaghetti_3d_function),
    ))
}

/// `noodle(functions, noises)`.
fn noodle(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
) -> DensityFunctionValue {
    let y = get_function(functions, &Y);
    let noodle_min_y = -60;
    let noodle_max_y = 320;
    let noodle_toggle = y_limited_interpolatable(
        y.clone(),
        fns::noise(noises.get_or_throw(&noises::NOODLE), 1.0, 1.0),
        noodle_min_y,
        noodle_max_y,
        -1,
    );
    let noodle_thickness = y_limited_interpolatable(
        y.clone(),
        fns::mapped_noise(
            noises.get_or_throw(&noises::NOODLE_THICKNESS),
            1.0,
            1.0,
            -0.05,
            -0.1,
        ),
        noodle_min_y,
        noodle_max_y,
        0,
    );
    let noodle_ridge_frequency = 2.6666666666666665;
    let noodle_ridge_a = y_limited_interpolatable(
        y.clone(),
        fns::noise(
            noises.get_or_throw(&noises::NOODLE_RIDGE_A),
            noodle_ridge_frequency,
            noodle_ridge_frequency,
        ),
        noodle_min_y,
        noodle_max_y,
        0,
    );
    let noodle_ridge_b = y_limited_interpolatable(
        y,
        fns::noise(
            noises.get_or_throw(&noises::NOODLE_RIDGE_B),
            noodle_ridge_frequency,
            noodle_ridge_frequency,
        ),
        noodle_min_y,
        noodle_max_y,
        0,
    );
    let noodle_ridged = fns::mul(
        fns::constant(1.5),
        fns::max(noodle_ridge_a.abs(), noodle_ridge_b.abs()),
    );
    fns::range_choice(
        noodle_toggle,
        -1000000.0,
        0.0,
        fns::constant(64.0),
        fns::add(noodle_thickness, noodle_ridged),
    )
}

/// `pillars(noises)`.
fn pillars(noises: &dyn HolderGetter<NoiseParameters>) -> DensityFunctionValue {
    let xz_frequency = 25.0;
    let y_frequency = 0.3;
    let pillar_noise_source = fns::noise(
        noises.get_or_throw(&noises::PILLAR),
        xz_frequency,
        y_frequency,
    );
    let pillar_rareness_modulator = fns::mapped_noise(
        noises.get_or_throw(&noises::PILLAR_RARENESS),
        1.0,
        1.0,
        0.0,
        -2.0,
    );
    let pillar_thickness_modulator = fns::mapped_noise(
        noises.get_or_throw(&noises::PILLAR_THICKNESS),
        1.0,
        1.0,
        0.0,
        1.1,
    );
    let pillars_with_rareness = fns::add(
        fns::mul(pillar_noise_source, fns::constant(2.0)),
        pillar_rareness_modulator,
    );
    fns::cache_once(fns::mul(
        pillars_with_rareness,
        pillar_thickness_modulator.cube(),
    ))
}

/// `spaghetti2D(functions, noises)`.
fn spaghetti_2d(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
) -> DensityFunctionValue {
    let spaghetti_2d_rarity_modulator = fns::noise(
        noises.get_or_throw(&noises::SPAGHETTI_2D_MODULATOR),
        2.0,
        1.0,
    );
    let spaghetti_2d_cave = QuantizedSpaghettiRarity::wrap_rarity_2d(
        spaghetti_2d_rarity_modulator,
        noises.get_or_throw(&noises::SPAGHETTI_2D),
    );
    let spaghetti_2d_elevation_modulator = fns::mapped_noise(
        noises.get_or_throw(&noises::SPAGHETTI_2D_ELEVATION),
        1.0,
        0.0,
        mth::floor_div(-64, 8) as f64,
        8.0,
    );
    let spaghetti_2d_thickness_modulator =
        get_function(functions, &SPAGHETTI_2D_THICKNESS_MODULATOR);
    let sloped_spaghetti = fns::add(
        fns::flat_cache(spaghetti_2d_elevation_modulator),
        fns::y_clamped_gradient(-64, 320, 8.0, -40.0),
    )
    .abs();
    let layer_ridged = fns::add(sloped_spaghetti, spaghetti_2d_thickness_modulator.clone()).cube();
    let ridge_offset = 0.083;
    let cave_noise = fns::add(
        spaghetti_2d_cave,
        fns::mul(
            fns::constant(ridge_offset),
            spaghetti_2d_thickness_modulator,
        ),
    );
    fns::max(cave_noise, layer_ridged).clamp(-1.0, 1.0)
}

/// `underground(functions, noises, slopedCheese)`.
fn underground(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
    sloped_cheese: DensityFunctionValue,
) -> DensityFunctionValue {
    let spaghetti_2d_function = get_function(functions, &SPAGHETTI_2D);
    let spaghetti_roughness_function = get_function(functions, &SPAGHETTI_ROUGHNESS_FUNCTION);
    let layer_noise_source = fns::noise(noises.get_or_throw(&noises::CAVE_LAYER), 1.0, 8.0);
    let layerized_caverns_function = fns::mul(fns::constant(4.0), layer_noise_source.square());
    let cheese = fns::noise(
        noises.get_or_throw(&noises::CAVE_CHEESE),
        1.0,
        0.6666666666666666,
    );
    let solidified_cheese_with_top_slide = fns::add(
        fns::add(fns::constant(0.27), cheese).clamp(-1.0, 1.0),
        fns::add(
            fns::constant(1.5),
            fns::mul(fns::constant(-0.64), sloped_cheese),
        )
        .clamp(0.0, 0.5),
    );
    let base_cave_density = fns::add(layerized_caverns_function, solidified_cheese_with_top_slide);
    let underground_subtractions = fns::min(
        fns::min(base_cave_density, get_function(functions, &ENTRANCES)),
        fns::add(spaghetti_2d_function, spaghetti_roughness_function),
    );
    let pillars_without_cutoff = get_function(functions, &PILLARS);
    let pillars = fns::range_choice(
        pillars_without_cutoff.clone(),
        -1000000.0,
        0.03,
        fns::constant(-1000000.0),
        pillars_without_cutoff,
    );
    fns::max(underground_subtractions, pillars)
}

/// `postProcess(slide)` — `interpolated(mul(blendDensity(slide), constant(0.64))).squeeze()`.
fn post_process(slide: DensityFunctionValue) -> DensityFunctionValue {
    let blended = fns::blend_density(slide);
    fns::interpolated(fns::mul(blended, fns::constant(0.64))).squeeze()
}

/// `remap(input, fromMin, fromMax, toMin, toMax)`.
fn remap(
    input: DensityFunctionValue,
    from_min: f64,
    from_max: f64,
    to_min: f64,
    to_max: f64,
) -> DensityFunctionValue {
    let factor = (to_max - to_min) / (from_max - from_min);
    let offset = to_min - from_min * factor;
    fns::add(
        fns::mul(input, fns::constant(factor)),
        fns::constant(offset),
    )
}

/// `overworld(functions, noises, largeBiomes, amplified)` — the full overworld
/// `NoiseRouter`.
#[allow(clippy::too_many_lines)]
pub fn overworld(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
    large_biomes: bool,
    amplified: bool,
) -> NoiseRouter {
    let barrier_noise = fns::noise(noises.get_or_throw(&noises::AQUIFER_BARRIER), 1.0, 0.5);
    let fluid_level_floodedness_noise = fns::noise(
        noises.get_or_throw(&noises::AQUIFER_FLUID_LEVEL_FLOODEDNESS),
        1.0,
        0.67,
    );
    let fluid_level_spread_noise = fns::noise(
        noises.get_or_throw(&noises::AQUIFER_FLUID_LEVEL_SPREAD),
        1.0,
        0.7142857142857143,
    );
    let lava_noise = fns::noise(noises.get_or_throw(&noises::AQUIFER_LAVA), 1.0, 1.0);
    let shift_x = get_function(functions, &SHIFT_X);
    let shift_z = get_function(functions, &SHIFT_Z);
    let temperature = fns::shifted_noise2d(
        shift_x.clone(),
        shift_z.clone(),
        0.25,
        noises.get_or_throw(if large_biomes {
            &noises::TEMPERATURE_LARGE
        } else {
            &noises::TEMPERATURE
        }),
    );
    let vegetation = fns::shifted_noise2d(
        shift_x,
        shift_z,
        0.25,
        noises.get_or_throw(if large_biomes {
            &noises::VEGETATION_LARGE
        } else {
            &noises::VEGETATION
        }),
    );
    let offset = get_function(
        functions,
        if large_biomes {
            &OFFSET_LARGE
        } else if amplified {
            &OFFSET_AMPLIFIED
        } else {
            &OFFSET
        },
    );
    let factor = get_function(
        functions,
        if large_biomes {
            &FACTOR_LARGE
        } else if amplified {
            &FACTOR_AMPLIFIED
        } else {
            &FACTOR
        },
    );
    let depth = get_function(
        functions,
        if large_biomes {
            &DEPTH_LARGE
        } else if amplified {
            &DEPTH_AMPLIFIED
        } else {
            &DEPTH
        },
    );
    let preliminary_surface_level = preliminary_surface_level(offset, factor, amplified);
    let sloped_cheese = fns::cache_once(get_function(
        functions,
        if large_biomes {
            &SLOPED_CHEESE_LARGE
        } else if amplified {
            &SLOPED_CHEESE_AMPLIFIED
        } else {
            &SLOPED_CHEESE
        },
    ));
    let surface_with_entrances = fns::min(
        sloped_cheese.clone(),
        fns::mul(fns::constant(5.0), get_function(functions, &ENTRANCES)),
    );
    let caves = fns::range_choice(
        sloped_cheese.clone(),
        -1000000.0,
        SURFACE_DENSITY_THRESHOLD,
        surface_with_entrances,
        underground(functions, noises, sloped_cheese),
    );
    let full_noise = fns::min(
        post_process(slide_overworld(amplified, caves)),
        get_function(functions, &NOODLE),
    );
    let y = get_function(functions, &Y);
    let vein_min_y = [VeinType::Copper, VeinType::Iron]
        .into_iter()
        .map(VeinType::min_y)
        .min()
        .unwrap_or(-DIM_MIN_Y * 2);
    let vein_max_y = [VeinType::Copper, VeinType::Iron]
        .into_iter()
        .map(VeinType::max_y)
        .max()
        .unwrap_or(-DIM_MIN_Y * 2);
    let vein_toggle = y_limited_interpolatable(
        y.clone(),
        fns::noise(
            noises.get_or_throw(&noises::ORE_VEININESS),
            VEININESS_FREQUENCY,
            VEININESS_FREQUENCY,
        ),
        vein_min_y,
        vein_max_y,
        0,
    );
    let ore_ridge_frequency = 4.0;
    let vein_a = y_limited_interpolatable(
        y.clone(),
        fns::noise(
            noises.get_or_throw(&noises::ORE_VEIN_A),
            ore_ridge_frequency,
            ore_ridge_frequency,
        ),
        vein_min_y,
        vein_max_y,
        0,
    )
    .abs();
    let vein_b = y_limited_interpolatable(
        y,
        fns::noise(
            noises.get_or_throw(&noises::ORE_VEIN_B),
            ore_ridge_frequency,
            ore_ridge_frequency,
        ),
        vein_min_y,
        vein_max_y,
        0,
    )
    .abs();
    let vein_ridged = fns::add(
        fns::constant((-ORE_THICKNESS) as f64),
        fns::max(vein_a, vein_b),
    );
    let vein_gap = fns::noise(noises.get_or_throw(&noises::ORE_GAP), 1.0, 1.0);
    NoiseRouter::new(
        barrier_noise,
        fluid_level_floodedness_noise,
        fluid_level_spread_noise,
        lava_noise,
        temperature,
        vegetation,
        get_function(
            functions,
            if large_biomes {
                &CONTINENTS_LARGE
            } else {
                &CONTINENTS
            },
        ),
        get_function(
            functions,
            if large_biomes {
                &EROSION_LARGE
            } else {
                &EROSION
            },
        ),
        depth,
        get_function(functions, &RIDGES),
        preliminary_surface_level,
        full_noise,
        vein_toggle,
        vein_ridged,
        vein_gap,
    )
}

/// `slideOverworld(isAmplified, caves)`.
fn slide_overworld(is_amplified: bool, caves: DensityFunctionValue) -> DensityFunctionValue {
    slide(
        caves,
        -64,
        384,
        if is_amplified { 16 } else { 80 },
        if is_amplified { 0 } else { 64 },
        -0.078125,
        0,
        24,
        if is_amplified { 0.4 } else { 0.1171875 },
    )
}

/// `slideNetherLike(functions, minY, height)`.
fn slide_nether_like(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    min_y: i32,
    height: i32,
) -> DensityFunctionValue {
    slide(
        get_function(functions, &BASE_3D_NOISE_NETHER),
        min_y,
        height,
        24,
        0,
        0.9375,
        -8,
        24,
        2.5,
    )
}

/// `slideEndLike(caves, minY, height)`.
fn slide_end_like(caves: DensityFunctionValue, min_y: i32, height: i32) -> DensityFunctionValue {
    slide(caves, min_y, height, 72, -184, -23.4375, 4, 32, -0.234375)
}

/// `nether(functions, noises)` — the full nether `NoiseRouter`.
pub fn nether(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
) -> NoiseRouter {
    let temperature = fns::shifted_noise2d(
        fns::zero(),
        fns::zero(),
        0.25,
        noises.get_or_throw(&noises::TEMPERATURE_NETHER),
    );
    let vegetation = fns::shifted_noise2d(
        fns::zero(),
        fns::zero(),
        0.25,
        noises.get_or_throw(&noises::VEGETATION_NETHER),
    );
    let slide = slide_nether_like(functions, 0, 128);
    let full_noise = post_process(slide);
    NoiseRouter::new(
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        temperature,
        vegetation,
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        full_noise,
        fns::zero(),
        fns::zero(),
        fns::zero(),
    )
}

/// `caves(functions)` — the `caves` `NoiseRouter`.
pub fn caves(functions: &dyn HolderGetter<DensityFunctionValue>) -> NoiseRouter {
    let slide = slide_nether_like(functions, -64, 192);
    simple_router(post_process(slide))
}

/// `floatingIslands(functions, noises)` — the `floating_islands` `NoiseRouter`.
/// Java's `noises` parameter is likewise unused here (signature fidelity).
pub fn floating_islands(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    _noises: &dyn HolderGetter<NoiseParameters>,
) -> NoiseRouter {
    let slide = slide_end_like(get_function(functions, &BASE_3D_NOISE_END), 0, 256);
    simple_router(post_process(slide))
}

/// `slideEnd(caves)`.
fn slide_end(caves: DensityFunctionValue) -> DensityFunctionValue {
    slide_end_like(caves, 0, 128)
}

/// `end(functions)` — the `end` `NoiseRouter`.
pub fn end(functions: &dyn HolderGetter<DensityFunctionValue>) -> NoiseRouter {
    let islands = fns::cache2d(fns::end_islands(0));
    let full_noise = post_process(slide_end(get_function(functions, &SLOPED_CHEESE_END)));
    NoiseRouter::new(
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        islands,
        fns::zero(),
        fns::zero(),
        fns::zero(),
        full_noise,
        fns::zero(),
        fns::zero(),
        fns::zero(),
    )
}

/// `simpleRouter(fullNoise)` — a router with every field `zero()` except the
/// final density.
fn simple_router(full_noise: DensityFunctionValue) -> NoiseRouter {
    NoiseRouter::new(
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        fns::zero(),
        full_noise,
        fns::zero(),
        fns::zero(),
        fns::zero(),
    )
}

/// `none()` — `simpleRouter(zero())`.
pub fn none() -> NoiseRouter {
    simple_router(fns::zero())
}

/// `splineWithBlending(spline, blendingTarget)`.
fn spline_with_blending(
    spline: DensityFunctionValue,
    blending_target: DensityFunctionValue,
) -> DensityFunctionValue {
    let blended_spline = fns::lerp(fns::blend_alpha(), blending_target, spline);
    fns::flat_cache(fns::cache2d(blended_spline))
}

/// `noiseGradientDensity(factor, depthWithJaggedness)`.
fn noise_gradient_density(
    factor: DensityFunctionValue,
    depth_with_jaggedness: DensityFunctionValue,
) -> DensityFunctionValue {
    let gradient_unscaled = fns::mul(depth_with_jaggedness, factor);
    fns::mul(
        fns::constant(BASE_DENSITY_MULTIPLIER),
        gradient_unscaled.quarter_negative(),
    )
}

/// `preliminarySurfaceLevel(offset, factor, amplified)`.
fn preliminary_surface_level(
    offset: DensityFunctionValue,
    factor: DensityFunctionValue,
    amplified: bool,
) -> DensityFunctionValue {
    let cached_factor = fns::cache2d(factor);
    let cached_offset = fns::cache2d(offset);
    let upper_bound = remap(
        fns::add(
            fns::mul(fns::constant(0.2734375), cached_factor.invert()),
            fns::mul(fns::constant(-1.0), cached_offset.clone()),
        ),
        1.5,
        -1.5,
        -64.0,
        320.0,
    )
    .clamp(-40.0, 320.0);
    let density = fns::add(
        slide_overworld(
            amplified,
            fns::add(
                noise_gradient_density(cached_factor, offset_to_depth(cached_offset)),
                fns::constant(CHEESE_NOISE_TARGET),
            )
            .clamp(-64.0, 64.0),
        ),
        fns::constant(-NOISE_ZERO),
    );
    fns::find_top_surface(
        density,
        upper_bound,
        -64,
        crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS.get_cell_height(),
    )
}

/// `yLimitedInterpolatable(y, whenInRange, minYInclusive, maxYInclusive,
/// whenOutOfRange)`.
fn y_limited_interpolatable(
    y: DensityFunctionValue,
    when_in_range: DensityFunctionValue,
    min_y_inclusive: i32,
    max_y_inclusive: i32,
    when_out_of_range: i32,
) -> DensityFunctionValue {
    fns::interpolated(fns::range_choice(
        y,
        min_y_inclusive as f64,
        (max_y_inclusive + 1) as f64,
        when_in_range,
        fns::constant(when_out_of_range as f64),
    ))
}

/// `slide(caves, minY, height, topStartY, topEndY, topTarget, bottomStartY,
/// bottomEndY, bottomTarget)`.
#[allow(clippy::too_many_arguments)]
fn slide(
    caves: DensityFunctionValue,
    min_y: i32,
    height: i32,
    top_start_y: i32,
    top_end_y: i32,
    top_target: f64,
    bottom_start_y: i32,
    bottom_end_y: i32,
    bottom_target: f64,
) -> DensityFunctionValue {
    let noise_value = caves;
    let top_factor = fns::y_clamped_gradient(
        min_y + height - top_start_y,
        min_y + height - top_end_y,
        1.0,
        0.0,
    );
    let noise_value = fns::lerp_double(top_factor, top_target, noise_value);
    let bottom_factor =
        fns::y_clamped_gradient(min_y + bottom_start_y, min_y + bottom_end_y, 0.0, 1.0);
    fns::lerp_double(bottom_factor, bottom_target, noise_value)
}

/// `NoiseRouterData.QuantizedSpaghettiRarity` — the interval-select spaghetti
/// rarity helper.
pub struct QuantizedSpaghettiRarity;

impl QuantizedSpaghettiRarity {
    /// `wrapRarity2d(input, noise)`.
    pub fn wrap_rarity_2d(
        input: DensityFunctionValue,
        noise: Holder<NoiseParameters>,
    ) -> DensityFunctionValue {
        fns::interval_select(
            input,
            vec![-0.75, -0.5, 0.5, 0.75],
            vec![
                Self::noise_function_for_rarity(noise.clone(), 0.5),
                Self::noise_function_for_rarity(noise.clone(), 0.75),
                Self::noise_function_for_rarity(noise.clone(), 1.0),
                Self::noise_function_for_rarity(noise.clone(), 2.0),
                Self::noise_function_for_rarity(noise, 3.0),
            ],
        )
        .abs()
    }

    /// `wrapRarity3d(input, noise)`.
    pub fn wrap_rarity_3d(
        input: DensityFunctionValue,
        noise: Holder<NoiseParameters>,
    ) -> DensityFunctionValue {
        fns::interval_select(
            input,
            vec![-0.5, 0.0, 0.5],
            vec![
                Self::noise_function_for_rarity(noise.clone(), 0.75),
                Self::noise_function_for_rarity(noise.clone(), 1.0),
                Self::noise_function_for_rarity(noise.clone(), 1.5),
                Self::noise_function_for_rarity(noise, 2.0),
            ],
        )
        .abs()
    }

    /// `noiseFunctionForRarity(noise, rarity)`.
    fn noise_function_for_rarity(
        noise: Holder<NoiseParameters>,
        rarity: f64,
    ) -> DensityFunctionValue {
        fns::mul(
            fns::constant(rarity),
            fns::noise(noise, 1.0 / rarity, 1.0 / rarity),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::worldgen::bootstrap_context::RecordingContext;
    use crate::data::worldgen::noise_data;
    use rivet_registry::RegistrationInfo;
    use rivet_registry::RegistryAccess;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::registry::Registry;
    use rivet_registry::root::AnyBox;

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

    /// Build a `RegistryAccess` containing the noise registry (via
    /// `NoiseData.bootstrap`) and the density-function registry (via
    /// `NoiseRouterData.bootstrap`), in that order.
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
        bootstrap(&mut df_ctx);
        for reg in df_ctx.registrations() {
            // The registry element is the erased `Arc<dyn DensityFunction>`
            // carrier, so `register` takes `Arc<Arc<dyn DensityFunction>>`.
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
    fn keys_have_java_identifiers() {
        assert_eq!(
            CONTINENTS.identifier().to_string(),
            "minecraft:overworld/continents"
        );
        assert_eq!(
            EROSION.identifier().to_string(),
            "minecraft:overworld/erosion"
        );
        assert_eq!(
            RIDGES.identifier().to_string(),
            "minecraft:overworld/ridges"
        );
        assert_eq!(
            RIDGES_FOLDED.identifier().to_string(),
            "minecraft:overworld/ridges_folded"
        );
        assert_eq!(
            OFFSET.identifier().to_string(),
            "minecraft:overworld/offset"
        );
        assert_eq!(
            FACTOR.identifier().to_string(),
            "minecraft:overworld/factor"
        );
        assert_eq!(DEPTH.identifier().to_string(), "minecraft:overworld/depth");
        assert_eq!(
            SPAGHETTI_2D.identifier().to_string(),
            "minecraft:overworld/caves/spaghetti_2d"
        );
    }

    #[test]
    fn bootstrap_registers_all_shared_functions() {
        let access = make_access();
        let mut ctx = RecordingContext::<DensityFunctionValue>::new(
            RegistryId(2),
            (*crate::levelgen::noise::registry_keys::DENSITY_FUNCTION).clone(),
            access,
        );
        let pillars_holder = bootstrap(&mut ctx);
        // PILLARS is the returned holder.
        let _ = pillars_holder;
        let regs: Vec<_> = ctx.registrations().iter().cloned().collect();
        let ids: Vec<String> = regs
            .iter()
            .map(|r| r.key.identifier().to_string())
            .collect();
        assert_eq!(ids[0], "minecraft:zero");
        assert_eq!(ids[1], "minecraft:y");
        assert_eq!(ids[2], "minecraft:shift_x");
        assert_eq!(ids[3], "minecraft:shift_z");
        assert_eq!(ids[4], "minecraft:overworld/base_3d_noise");
        assert_eq!(ids[5], "minecraft:nether/base_3d_noise");
        assert_eq!(ids[6], "minecraft:end/base_3d_noise");
        assert_eq!(ids[7], "minecraft:overworld/continents");
        assert_eq!(ids[8], "minecraft:overworld/erosion");
        assert_eq!(ids[9], "minecraft:overworld/ridges");
        assert_eq!(ids[10], "minecraft:overworld/ridges_folded");
        assert!(ids.contains(&"minecraft:overworld/sloped_cheese".to_string()));
        assert!(ids.contains(&"minecraft:overworld/caves/pillars".to_string()));
        assert_eq!(ids.last().unwrap(), "minecraft:overworld/caves/pillars");
    }
}

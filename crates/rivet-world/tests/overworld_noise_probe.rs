//! Real OVERWORLD noise probe — issue #919's immediately-ready slice.
//!
//! Bootstraps the real NOISE / DENSITY_FUNCTION / NOISE_SETTINGS registries
//! (via [`rivet_world::data::worldgen::worldgen_bootstraps`]) and drives the
//! real OVERWORLD `NoiseGeneratorSettings` (never `dummy()`):
//!
//!   1. **Composed-noise golden** — the committed Paper 26.2 fixture
//!      (`tools/rivet-oracle/fixtures/composed-noise/composed-noise.json`,
//!      captured by `ComposedNoiseProbe` at seed 42) pins the router climate
//!      fields, the float-cast weirdness + `peaksAndValleys`, the interpolated
//!      final density, the raw `finalDensity`, and `preliminarySurfaceLevel`
//!      as raw IEEE-754 bit patterns. These tests compare Rivet's computed
//!      values **bit-exactly** (`f64::to_bits` / `f32::to_bits`) against that
//!      fixture.
//!   2. **Raw noise-fill** — `NoiseBasedChunkGenerator::fill_from_noise` fills
//!      a real overworld `ProtoChunk`. These tests prove determinism (same
//!      seed → identical blocks), seed sensitivity (different seed → a
//!      different chunk), non-air content (real terrain, not an empty void),
//!      and the WORLD_SURFACE_WG / OCEAN_FLOOR_WG worldgen heightmaps.
//!
//! This is deliberately **outside** authoritative ChunkMap/client serving: no
//! `ChunkStatus::Noise` or `BIOMES` claim is made here — the fixture
//! comparison asserts value leaves, not chunk-status reachability.

use rivet_registry::Holder;
use rivet_registry::HolderGetter;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::ChunkPos;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId, current_version_container_factory,
};
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries;
use rivet_world::level::height_accessor::create as create_accessor;
use rivet_world::levelgen::blending::blender::Blender;
use rivet_world::levelgen::heightmap::Types;
use rivet_world::levelgen::noise::density_function::SinglePointContext;
use rivet_world::levelgen::noise::registry_keys;
use rivet_world::levelgen::noisegen::noise_based_chunk_generator::NoiseBasedChunkGenerator;
use rivet_world::levelgen::noisegen::noise_generator_settings::OVERWORLD;
use rivet_world::levelgen::noisegen::noise_router_data::peaks_and_valleys_f32;
use rivet_world::levelgen::noisegen::random_state::RandomState;
use serde_json::Value;

const FIXTURE: &str =
    include_str!("../../../tools/rivet-oracle/fixtures/composed-noise/composed-noise.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("composed-noise fixture JSON parses")
}

/// The bootstrapped worldgen `RegistryAccess` (NOISE + DENSITY_FUNCTION +
/// NOISE_SETTINGS).
fn access() -> rivet_registry::RegistryAccess {
    build_worldgen_registries()
}

/// A `RandomState` over the real overworld settings, resolved through the
/// bootstrapped access (`RandomState.create(registryAccess, OVERWORLD, seed)`).
fn random_state<'a>(access: &'a rivet_registry::RegistryAccess, seed: i64) -> RandomState<'a> {
    RandomState::create_from_provider(access, &OVERWORLD, seed)
}

/// The overworld `NoiseBasedChunkGenerator` (a `Direct` settings holder — the
/// noisegen construction form) whose settings value is the real overworld
/// preset from the bootstrapped NOISE_SETTINGS registry.
fn generator(access: &rivet_registry::RegistryAccess) -> NoiseBasedChunkGenerator {
    let settings_registry = access.lookup_or_throw(&registry_keys::NOISE_SETTINGS);
    let settings = settings_registry
        .get_or_throw(&OVERWORLD)
        .value(settings_registry)
        .clone();
    NoiseBasedChunkGenerator::new(Holder::Direct(settings))
}

/// `Float.floatToIntBits` — the fixture stores the f32 bit pattern as Java's
/// signed `int` (widened to i64 in JSON), so the u32 bit pattern round-trips
/// through `i32` before widening.
fn f32_bits(v: f32) -> i64 {
    (v.to_bits() as i32) as i64
}

fn f64_bits(v: f64) -> i64 {
    v.to_bits() as i64
}

// ---------------------------------------------------------------------------
// Composed-noise golden: bit-exact comparison against the committed Paper 26.2
// fixture (seed 42, overworld normal).
// ---------------------------------------------------------------------------

#[test]
fn composed_noise_climate_matches_paper_bit_exactly() {
    let root = fixture();
    assert_eq!(root["seed"].as_i64(), Some(42));
    assert_eq!(root["noise-settings"].as_str(), Some("minecraft:overworld"));
    let access = access();
    let state = random_state(&access, 42);
    let router = state.router();
    let climate = root["climate"].as_array().expect("climate array");
    assert_eq!(climate.len(), 8);
    for entry in climate {
        let x = entry["x"].as_i64().unwrap() as i32;
        let z = entry["z"].as_i64().unwrap() as i32;
        let ctx = SinglePointContext::new(x, 0, z);
        // The six router climate fields, float-cast (`(float) compute`).
        for (name, value) in [
            ("temperature", router.temperature().compute(&ctx) as f32),
            ("vegetation", router.vegetation().compute(&ctx) as f32),
            ("continents", router.continents().compute(&ctx) as f32),
            ("erosion", router.erosion().compute(&ctx) as f32),
            ("depth", router.depth().compute(&ctx) as f32),
            ("ridges", router.ridges().compute(&ctx) as f32),
        ] {
            assert_eq!(
                f32_bits(value),
                entry[name]["bits"].as_i64().unwrap(),
                "climate {name} at ({x},0,{z})"
            );
        }
        // weirdness is the float-cast ridges value (the same cast `weirdness`
        // feeds); peaksAndValleys folds that float.
        let weirdness = router.ridges().compute(&ctx) as f32;
        assert_eq!(
            f32_bits(weirdness),
            entry["weirdness"]["bits"].as_i64().unwrap(),
            "weirdness at ({x},0,{z})"
        );
        let pv = peaks_and_valleys_f32(weirdness);
        assert_eq!(
            f32_bits(pv),
            entry["peaksAndValleys"]["bits"].as_i64().unwrap(),
            "peaksAndValleys at ({x},0,{z})"
        );
    }
}

#[test]
fn composed_noise_density_matches_paper_bit_exactly() {
    let root = fixture();
    let access = access();
    let state = random_state(&access, 42);
    let generator = generator(&access);
    let router = state.router();
    let density = root["density"].as_array().expect("density array");
    assert_eq!(density.len(), 80);
    for entry in density {
        let x = entry["x"].as_i64().unwrap() as i32;
        let y = entry["y"].as_i64().unwrap() as i32;
        let z = entry["z"].as_i64().unwrap() as i32;
        let ctx = SinglePointContext::new(x, y, z);
        let interpolated = generator.get_interpolated_noise_value(&state, &ctx);
        assert!(
            interpolated.is_finite(),
            "interpolated density must be finite at ({x},{y},{z})"
        );
        assert_eq!(
            f64_bits(interpolated),
            entry["density"]["bits"].as_i64().unwrap(),
            "density at ({x},{y},{z})"
        );
        let final_density = router.final_density().compute(&ctx);
        assert_eq!(
            f64_bits(final_density),
            entry["finalDensity"]["bits"].as_i64().unwrap(),
            "finalDensity at ({x},{y},{z})"
        );
        let surface = router.preliminary_surface_level().compute(&ctx);
        assert_eq!(
            f64_bits(surface),
            entry["preliminarySurfaceLevel"]["bits"].as_i64().unwrap(),
            "preliminarySurfaceLevel at ({x},{y},{z})"
        );
    }
}

// ---------------------------------------------------------------------------
// Raw noise-fill: determinism, seed sensitivity, non-air content, and the
// WORLD_SURFACE_WG / OCEAN_FLOOR_WG worldgen heightmaps.
// ---------------------------------------------------------------------------

/// A 24-section all-air overworld `ProtoChunk` (`-64..=319`, the real
/// worldgen chunk shape `fillFromNoise` needs). `sections: None` lets
/// `ChunkAccess::new` build the all-air sections from the accessor's section
/// count (24).
fn worldgen_proto(pos: ChunkPos) -> ProtoChunk<BlockState, BiomeId, &'static str> {
    let factory = current_version_container_factory();
    let air = Blocks::AIR.default_block_state();
    ProtoChunk::new(
        pos,
        UpgradeData::empty(24),
        create_accessor(-64, 384),
        &factory,
        None,
        air,
        air,
        &resolve_state_flags,
    )
}

/// Every in-build-height block in a canonical (y, x, z) order — the fill's
/// full observable block output.
fn fill_output(proto: &ProtoChunk<BlockState, BiomeId, &'static str>) -> Vec<BlockState> {
    let mut out = Vec::with_capacity(16 * 16 * 384);
    for y in -64..=319 {
        for x in 0..16 {
            for z in 0..16 {
                out.push(proto.get_block_state(x, y, z));
            }
        }
    }
    out
}

/// `get_or_create_heightmap_unprimed(...).get_height_at(x, z, -64)`.
fn height_at(
    proto: &mut ProtoChunk<BlockState, BiomeId, &'static str>,
    ty: Types,
    x: i32,
    z: i32,
) -> i32 {
    proto
        .get_or_create_heightmap_unprimed(ty)
        .get_height_at(x, z, -64)
}

#[test]
fn overworld_fill_is_deterministic() {
    let access = access();
    let state = random_state(&access, 42);
    let generator = generator(&access);
    let mut a = worldgen_proto(ChunkPos::ZERO);
    let mut b = worldgen_proto(ChunkPos::ZERO);
    generator.fill_from_noise(Blender::empty(), &state, &mut a);
    generator.fill_from_noise(Blender::empty(), &state, &mut b);
    assert_eq!(
        fill_output(&a),
        fill_output(&b),
        "the same seed must fill the chunk identically"
    );
}

#[test]
fn overworld_fill_is_seed_sensitive() {
    let access = access();
    let generator = generator(&access);
    let state_42 = random_state(&access, 42);
    let state_43 = random_state(&access, 43);
    let mut a = worldgen_proto(ChunkPos::ZERO);
    let mut b = worldgen_proto(ChunkPos::ZERO);
    generator.fill_from_noise(Blender::empty(), &state_42, &mut a);
    generator.fill_from_noise(Blender::empty(), &state_43, &mut b);
    assert_ne!(
        fill_output(&a),
        fill_output(&b),
        "different seeds must fill the chunk differently"
    );
}

#[test]
fn overworld_fill_produces_non_air_terrain() {
    let access = access();
    let state = random_state(&access, 42);
    let generator = generator(&access);
    let mut proto = worldgen_proto(ChunkPos::ZERO);
    generator.fill_from_noise(Blender::empty(), &state, &mut proto);
    let air = Blocks::AIR.default_block_state();
    let non_air = fill_output(&proto)
        .into_iter()
        .filter(|state| *state != air)
        .count();
    assert!(
        non_air > 0,
        "the overworld fill must place non-air blocks (real terrain)"
    );
    // The default block (stone) appears somewhere — the fill is not pure void.
    let stone = Blocks::STONE.default_block_state();
    assert!(
        fill_output(&proto).into_iter().any(|state| state == stone),
        "the fill must place the settings' default block (stone)"
    );
}

#[test]
fn overworld_fill_produces_worldgen_heightmaps() {
    let access = access();
    let state = random_state(&access, 42);
    let generator = generator(&access);
    let mut proto = worldgen_proto(ChunkPos::ZERO);
    generator.fill_from_noise(Blender::empty(), &state, &mut proto);

    // WORLD_SURFACE_WG / OCEAN_FLOOR_WG exist (the doFill prologue creates
    // them) and every column's height is inside the build height window.
    let mut surface_heights = Vec::new();
    let mut floor_heights = Vec::new();
    for x in 0..16 {
        for z in 0..16 {
            let surface = height_at(&mut proto, Types::WorldSurfaceWg, x, z);
            let floor = height_at(&mut proto, Types::OceanFloorWg, x, z);
            assert!(
                (-64..=319).contains(&surface),
                "WORLD_SURFACE_WG at ({x},{z}) = {surface} out of build height"
            );
            assert!(
                (-64..=319).contains(&floor),
                "OCEAN_FLOOR_WG at ({x},{z}) = {floor} out of build height"
            );
            assert!(
                surface >= floor,
                "WORLD_SURFACE_WG {surface} < OCEAN_FLOOR_WG {floor} at ({x},{z})"
            );
            surface_heights.push(surface);
            floor_heights.push(floor);
        }
    }
    // The fill created real terrain: the surface is above the deep void floor
    // and the heightmaps vary across the chunk (not a degenerate flat fill).
    let max_surface = *surface_heights.iter().max().unwrap();
    let min_surface = *surface_heights.iter().min().unwrap();
    let max_floor = *floor_heights.iter().max().unwrap();
    let min_floor = *floor_heights.iter().min().unwrap();
    assert!(
        max_surface > -64 + 100,
        "max WORLD_SURFACE_WG {max_surface} is implausibly low for real terrain"
    );
    // The seed-42 (0,0) chunk is uniformly ocean: the WORLD_SURFACE_WG
    // (NOT_AIR) heightmap tracks the water surface — one block below sea
    // level 63 — not the varying seafloor. This pins the heightmap's behavior
    // (topmost non-air, not topmost motion-blocking) for an ocean column.
    assert_eq!(
        min_surface, 62,
        "WORLD_SURFACE_WG must be the uniform water surface at sea level 63, got {min_surface}"
    );
    assert_eq!(
        max_surface, 62,
        "WORLD_SURFACE_WG must be the uniform water surface at sea level 63, got {max_surface}"
    );
    // The seafloor (OCEAN_FLOOR_WG, blocksMotion) reflects the real noise
    // terrain and must vary across the chunk.
    assert!(
        min_floor < max_floor,
        "OCEAN_FLOOR_WG must vary across the chunk (got constant {min_floor})"
    );
    assert!(
        max_floor <= min_surface,
        "OCEAN_FLOOR_WG {max_floor} must sit at or below the water surface {min_surface}"
    );
    // An ocean floor exists somewhere below the surface (the overworld has
    // oceans/water bodies; the floor heightmap is meaningfully populated).
    assert!(
        floor_heights.iter().any(|h| *h <= 63),
        "some column has an ocean/floor height at or below sea level 63"
    );
}

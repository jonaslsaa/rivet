//! Integration tests for issue #287 Part A — the Paper-faithful heightmap
//! compute slice — driven through the public `ChunkAccess` surface with real
//! populated sections.
//!
//! The `levelgen::heightmap` unit tests pin the per-block `update`/`isOpaque`
//! math against the pinned Paper 26.2 `Heightmap.java`. This file exercises
//! the same code through the chunk's on-demand priming (`get_height_at`) and
//! the `update_heightmaps_after` walk, proving the section reads resolve the
//! four predicates (air / blocks-motion / fluid / leaves) and that a real
//! section below a removed block is found by the downward re-scan.
//!
//! `ProtoChunk`'s status dispatch (`Empty` → `WORLDGEN_HEIGHTMAPS`,
//! `Full` → `FINAL_HEIGHTMAPS`) is covered by a unit test in `proto_chunk.rs`,
//! because the worldgen chunk's `base` (and thus its heightmap storage) is not
//! public — integration tests cannot observe it.

use rivet_registry::core::ChunkPos;
use rivet_world::chunk::chunk_access::ChunkAccess;
use rivet_world::chunk::level_chunk_section::LevelChunkSection;
use rivet_world::chunk::palette::GlobalIdMap;
use rivet_world::chunk::paletted_container::PalettedContainer;
use rivet_world::chunk::paletted_container_factory::PalettedContainerFactory;
use rivet_world::chunk::strategy::Strategy;
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::level::height_accessor::SimpleLevelHeightAccessor;
use rivet_world::level::height_accessor::create as create_accessor;
use rivet_world::levelgen::heightmap::{StateFlags, Types};

/// A value-map where the global id is the value (`u8`).
#[derive(Clone, Copy)]
struct TestGlobalMap;
impl GlobalIdMap<u8> for TestGlobalMap {
    fn get_id(&self, value: &u8) -> i32 {
        *value as i32
    }
    fn by_id_or_throw(&self, id: i32) -> u8 {
        id as u8
    }
    fn size(&self) -> i32 {
        256
    }
    fn by_id(&self, id: i32) -> Option<u8> {
        Some(id as u8)
    }
    fn clone_box(&self) -> Box<dyn GlobalIdMap<u8>> {
        Box::new(*self)
    }
}

fn block_strategy() -> Strategy<u8> {
    Strategy::create_for_block_states(Box::new(TestGlobalMap))
}
fn biome_strategy() -> Strategy<u8> {
    Strategy::create_for_biomes(Box::new(TestGlobalMap))
}
fn accessor() -> SimpleLevelHeightAccessor {
    create_accessor(-64, 384)
}
fn factory() -> PalettedContainerFactory<u8, u8> {
    PalettedContainerFactory::new(block_strategy(), 0, biome_strategy(), 0)
}

/// The four per-state flag shapes the heightmap predicates discriminate: id 0
/// is air, 1 stone (blocks motion), 2 water (fluid, no motion), 3 leaves.
fn test_flags(s: &u8) -> StateFlags {
    match *s {
        0 => StateFlags {
            is_air: true,
            blocks_motion: false,
            has_fluid: false,
            is_leaves: false,
        },
        1 => StateFlags {
            is_air: false,
            blocks_motion: true,
            has_fluid: false,
            is_leaves: false,
        },
        2 => StateFlags {
            is_air: false,
            blocks_motion: false,
            has_fluid: true,
            is_leaves: false,
        },
        // The tests only use ids 0..=3; id 3 stands in for a leaves block.
        _ => StateFlags {
            is_air: false,
            blocks_motion: true,
            has_fluid: false,
            is_leaves: true,
        },
    }
}

/// A 24-section chunk whose section 0 carries the given `(x, sectionY, z, id)`
/// entries (section-relative Y, so sectionY 0 is absolute y -64); every other
/// section is all-air. `air`/`void_air` are 0/255, and the heightmap resolver
/// is [`test_flags`]. The `ChunkAccess` constructor primes no heightmap entry
/// (exactly like the `ProtoChunk` constructor), so every read or update walks
/// the sections on demand.
fn base_with_section_entries(entries: &[(i32, i32, i32, u8)]) -> ChunkAccess<u8, u8, &'static str> {
    let mut states = PalettedContainer::new(0u8, block_strategy());
    for &(x, y, z, id) in entries {
        states.set(x, y, z, id);
    }
    let mut sections = Vec::with_capacity(24);
    sections.push(LevelChunkSection::new(
        states,
        PalettedContainer::new(0u8, biome_strategy()),
        |s: &u8| *s == 0,
        |_| false,
        |s: &u8| !test_flags(s).has_fluid,
        |_| false,
        |_| false,
    ));
    for _ in 1..24 {
        sections.push(LevelChunkSection::new(
            PalettedContainer::new(0u8, block_strategy()),
            PalettedContainer::new(0u8, biome_strategy()),
            |s: &u8| *s == 0,
            |_| false,
            |s: &u8| !test_flags(s).has_fluid,
            |_| false,
            |_| false,
        ));
    }
    ChunkAccess::new(
        ChunkPos::ZERO,
        UpgradeData::empty(24),
        accessor(),
        &factory(),
        0,
        Some(sections),
        &test_flags,
    )
}

#[test]
fn get_height_at_primes_a_missing_entry_over_a_stone_section() {
    // Stone at the bottom of section 0 (absolute y -64). The first read has
    // no heightmap entry, so it primes on demand: it walks the column down
    // from the highest filled section and records the stone's Y.
    let mut base = base_with_section_entries(&[(0, 0, 0, 1)]);
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -64);
    // Priming creates the entry (Java `computeIfAbsent`).
    assert!(base.has_primed_heightmap(Types::WorldSurface));
    // An all-air column decodes as `minY - 1` (the never-set entry is 0).
    assert_eq!(base.get_height_at(Types::WorldSurface, 5, 5), -65);
}

#[test]
fn the_four_predicates_discriminate_through_prime_heightmaps() {
    // Column x=0 has water at -62 over stone at -64; column x=1 has leaves at
    // -62 over stone at -64. The `prime_heightmaps` walk classifies the
    // topmost opaque block per type:
    // - `WorldSurface` (NOT_AIR) tops at the water / the leaves (-62);
    // - `OceanFloor` (blocksMotion) skips the water, so column 0 tops at the
    //   stone (-64);
    // - `MotionBlocking` (blocksMotion || hasFluid) counts the water (-62);
    // - `MotionBlockingNoLeaves` excludes the leaves, so column 1 tops at the
    //   stone (-64).
    let mut base = base_with_section_entries(&[
        (0, 0, 0, 1), // stone at -64, column 0
        (0, 2, 0, 2), // water at -62, column 0
        (1, 0, 0, 1), // stone at -64, column 1
        (1, 2, 0, 3), // leaves at -62, column 1
    ]);
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -62);
    assert_eq!(base.get_height_at(Types::OceanFloor, 0, 0), -64);
    assert_eq!(base.get_height_at(Types::MotionBlocking, 0, 0), -62);
    assert_eq!(base.get_height_at(Types::MotionBlockingNoLeaves, 0, 0), -62);
    assert_eq!(base.get_height_at(Types::WorldSurface, 1, 0), -62);
    assert_eq!(base.get_height_at(Types::OceanFloor, 1, 0), -62);
    assert_eq!(base.get_height_at(Types::MotionBlocking, 1, 0), -62);
    assert_eq!(base.get_height_at(Types::MotionBlockingNoLeaves, 1, 0), -64);
}

#[test]
fn update_heightmaps_after_raises_lowers_and_noops_like_java() {
    // Stone at -64; the constructor primed no heightmap, so the first update
    // primes the missing entries (all-air scan -> empty columns) and then
    // raises: stone placed at -64 sets height -63, so getHeight reads the
    // topmost opaque Y, -64.
    let mut base = base_with_section_entries(&[(0, 0, 0, 1)]);
    let types = [Types::WorldSurface, Types::MotionBlocking];
    base.update_heightmaps_after(&types, 0, -64, 0, test_flags(&1));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -64);
    assert_eq!(base.get_height_at(Types::MotionBlocking, 0, 0), -64);
    // Raising: stone placed one block higher moves the top to -63.
    base.update_heightmaps_after(&types, 0, -63, 0, test_flags(&1));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -63);
    // Lowering: air at the top triggers the downward re-scan, which reads the
    // real section through `flags_at`; the stone at -64 is the next opaque
    // block, so the height falls back to -64.
    base.update_heightmaps_after(&types, 0, -63, 0, test_flags(&0));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -64);
    // A non-top air placement is a no-op: -63 is not `firstAvailable - 1`
    // (that slot is -64), so Java falls through to `return false`.
    base.update_heightmaps_after(&types, 0, -63, 0, test_flags(&0));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -64);
    // A placement far below the surface hits the early `localY <= firstAvailable
    // - 2` return: air at -65 leaves the column untouched.
    base.update_heightmaps_after(&types, 0, -65, 0, test_flags(&0));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -64);
}

#[test]
fn removing_the_only_surface_block_empties_the_column() {
    // An all-air chunk. The placed state simulates the #216 section write:
    // stone at -64 raises the column to -64, then air at -64 removes it.
    let mut base = base_with_section_entries(&[]);
    base.update_heightmaps_after(&[Types::WorldSurface], 0, -64, 0, test_flags(&1));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -64);
    // The downward re-scan finds no opaque block below (the real section is
    // all-air), so Java `setHeight(minY)` stores 0 and getHeight falls to
    // `minY - 1`, exactly like a column that never had a surface block.
    base.update_heightmaps_after(&[Types::WorldSurface], 0, -64, 0, test_flags(&0));
    assert_eq!(base.get_height_at(Types::WorldSurface, 0, 0), -65);
}

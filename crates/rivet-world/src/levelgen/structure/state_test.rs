use super::*;
use crate::biome::{climate::Climate, fixed_biome_source::FixedBiomeSource};
use rivet_registry::{ResourceKey, holder_lookup::HolderGetter, registries::BIOME};

fn key(path: &str) -> Identifier {
    Identifier::with_default_namespace(path)
}

fn access() -> RegistryAccess {
    static ACCESS: std::sync::LazyLock<RegistryAccess> = std::sync::LazyLock::new(|| {
        super::super::structure_registry::build_structure_access(
            &crate::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
        )
        .unwrap()
    });
    ACCESS.clone()
}

fn source(access: &RegistryAccess, biome: &str) -> Arc<dyn BiomeSource> {
    let biome = access
        .lookup(&BIOME)
        .unwrap()
        .get_or_throw(&ResourceKey::create(&BIOME, key(biome)));
    Arc::new(FixedBiomeSource::new(biome))
}

fn state(biome: &str, seeds: PaperStructureSeeds) -> ChunkGeneratorStructureState {
    let access = access();
    ChunkGeneratorStructureState::create_for_normal(
        access.clone(),
        42,
        source(&access, biome),
        Climate::empty(),
        seeds,
    )
    .unwrap()
}

#[test]
fn actual_registry_biomes_filter_sets_and_each_individual_structure() {
    let mut plains = state("plains", PaperStructureSeeds::default());
    let names: Vec<_> = plains
        .possible_structure_sets()
        .iter()
        .map(|set| set.key.path())
        .collect();
    assert!(names.contains(&"villages"));
    assert!(names.contains(&"strongholds"));
    assert!(!names.contains(&"buried_treasures"));
    assert!(!names.contains(&"nether_complexes"));
    assert_eq!(
        plains
            .placements_for_structure(&key("village_plains"))
            .unwrap()
            .len(),
        1
    );
    assert!(
        plains
            .placements_for_structure(&key("village_desert"))
            .unwrap()
            .is_empty()
    );
    let beach = state("beach", PaperStructureSeeds::default());
    assert!(
        beach
            .possible_structure_sets()
            .iter()
            .any(|set| set.key == key("buried_treasures"))
    );
}

#[test]
fn keyed_injection_never_replaces_original_exclusion_target() {
    let seeds = PaperStructureSeeds {
        village: 123,
        ..Default::default()
    };
    let mut state = state("plains", seeds);
    let villages = key("villages");
    let injected = state
        .possible_structure_sets()
        .iter()
        .find(|set| set.key == villages)
        .unwrap()
        .placement
        .clone();
    let original = state.original_placements[&villages].clone();
    assert_eq!(injected.settings().salt, 123);
    assert_eq!(original.settings().salt, 10_387_312);
    assert!(!Arc::ptr_eq(&injected, &original));
    let StructurePlacement::RandomSpread(original_spread) = &*original else {
        panic!("real village spread")
    };
    let StructurePlacement::RandomSpread(injected_spread) = &*injected else {
        panic!("injected village spread")
    };
    let chunk = (-4..=4)
        .flat_map(|x| (-4..=4).map(move |z| (x, z)))
        .map(|(x, z)| original_spread.potential_structure_chunk(42, x * 34, z * 34))
        .find(|pos| injected_spread.potential_structure_chunk(42, pos.x(), pos.z()) != *pos)
        .expect("different salts produce a distinguishing village candidate");
    assert!(
        PlacementAuthority::is_structure_chunk(&mut state, &villages, chunk.x(), chunk.z())
            .unwrap()
    );
    assert!(
        !state
            .is_structure_chunk(&villages, chunk.x(), chunk.z())
            .unwrap()
    );
}

#[test]
fn lazy_rings_keep_identity_and_flat_concentric_seed_zero() {
    let access = access();
    let source = source(&access, "plains");
    let mut flat = ChunkGeneratorStructureState::create_for_flat(
        access.clone(),
        42,
        source,
        Climate::empty(),
        PaperStructureSeeds::default(),
        &[key("strongholds")],
    )
    .unwrap();
    assert!(!flat.positions_generated);
    let placement = flat.possible_structure_sets()[0].placement.clone();
    let original = flat.original_placements[&key("strongholds")].clone();
    assert!(Arc::ptr_eq(&placement, &original));
    let StructurePlacement::ConcentricRings(rings) = &*placement else {
        panic!("real stronghold rings")
    };
    let expected =
        generate_ring_positions_with_biome_source(rings, 0, None, &*flat.source, &flat.sampler)
            .unwrap();
    assert_eq!(
        flat.ring_positions_for_set(&key("strongholds"))
            .unwrap()
            .unwrap(),
        expected
    );
    assert!(flat.positions_generated);
    assert_eq!(
        PlacementAuthority::ring_positions(&mut flat, rings)
            .unwrap()
            .unwrap(),
        expected
    );
    assert!(
        PlacementAuthority::ring_positions(&mut flat, &rings.clone())
            .unwrap()
            .is_none(),
        "equal settings are not the same Java placement object"
    );
    let mut normal = state("plains", PaperStructureSeeds::default());
    assert_ne!(
        normal
            .ring_positions_for_set(&key("strongholds"))
            .unwrap()
            .unwrap(),
        expected
    );
    let mut overridden = state(
        "plains",
        PaperStructureSeeds {
            stronghold: Some(0),
            ..Default::default()
        },
    );
    assert_eq!(
        overridden
            .ring_positions_for_set(&key("strongholds"))
            .unwrap()
            .unwrap(),
        expected
    );
}

//! Real height interpolation and real biome-holder identity against Paper's
//! Structure.generate. The adapter only forwards existing native services.
use super::generate_start;
use crate::{
    biome::{biome_source::BiomeSource, fixed_biome_source::FixedBiomeSource},
    chunk::chunk_generator::ChunkGenerator,
    data::worldgen::worldgen_bootstraps::build_worldgen_registries,
    level::height_accessor::{LevelHeightAccessor, create},
    levelgen::{
        heightmap::Types,
        noise::registry_keys::NOISE_SETTINGS,
        noisegen::{
            noise_based_chunk_generator::NoiseBasedChunkGenerator,
            noise_generator_settings::OVERWORLD, random_state::RandomState,
        },
        structure::{registry_keys::STRUCTURE, structure_registry::build_structure_access},
    },
};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::{
    Identifier, ResourceKey, core::ChunkPos, holder::Holder, holder_lookup::HolderGetter,
    registries::BIOME,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[test]
fn lazy_structure_points_preserve_actual_paper_rejection_and_eligible_body_refusal() {
    use super::find_lazy_generation_point;
    use crate::biome::generated_biome_source::overworld_biome_source_from_getter;
    use rivet_util::{
        RandomSource, random_source::LegacyRandomSource, worldgen_random::WorldgenRandom,
    };
    let access = build_structure_access(&build_worldgen_registries()).unwrap();
    let structures = access.lookup(&STRUCTURE).unwrap();
    let biomes = access.lookup(&BIOME).unwrap();
    let overworld: Arc<dyn BiomeSource> = Arc::new(overworld_biome_source_from_getter(biomes));
    let settings = access
        .lookup(&NOISE_SETTINGS)
        .unwrap()
        .get_optional_by_key(&OVERWORLD)
        .unwrap()
        .clone();
    let noise = NoiseBasedChunkGenerator::new(Holder::Direct(settings));
    let height = create(-64, 384);
    let mut random_states = HashMap::new();
    let mut rejections = 0;
    let mut eligible = 0;
    let mut missing_points = 0;
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/lazy-structure-point.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let seed = row["seed"].as_i64().unwrap();
        let random_state = random_states
            .entry(seed)
            .or_insert_with(|| RandomState::create_from_provider(&access, &OVERWORLD, seed));
        let key = Identifier::with_default_namespace(row["structure"].as_str().unwrap());
        let structure = structures.get_optional(&key).unwrap();
        let mode = row["generator_biome"].as_str().unwrap();
        let source: Arc<dyn BiomeSource> = if mode == "normal" {
            Arc::clone(&overworld)
        } else {
            Arc::new(FixedBiomeSource::new(biomes.get_or_throw(
                &ResourceKey::create(&BIOME, Identifier::with_default_namespace(mode)),
            )))
        };
        let generator = Generator {
            noise: noise.clone(),
            source,
        };
        let chunk = ChunkPos::new(
            row["x"].as_i64().unwrap() as i32,
            row["z"].as_i64().unwrap() as i32,
        );
        let context = format!("seed{seed} {key} {chunk:?} {mode}");
        let point =
            find_lazy_generation_point(&structure.body, &generator, random_state, chunk, &height)
                .unwrap();
        assert_eq!(
            point.is_some(),
            row["point_present"].as_bool().unwrap(),
            "{context}"
        );
        if let Some(point) = point {
            assert_eq!(
                serde_json::json!([point.get_x(), point.get_y(), point.get_z()]),
                row["point"],
                "actual noise-derived point {context}"
            );
            let biome = generator.get_biome_source().get_noise_biome(
                point.get_x() >> 2,
                point.get_y() >> 2,
                point.get_z() >> 2,
                random_state.sampler(),
            );
            assert_eq!(
                biome.key(biomes).identifier().to_string(),
                row["biome"].as_str().unwrap(),
                "actual MultiNoise/fixed biome {context}"
            );
        } else {
            missing_points += 1;
        }
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(seed, chunk.x(), chunk.z());
        let next = random.next_long();
        assert_eq!(
            next,
            row["rng_next"].as_i64().unwrap(),
            "source lazy point consumes no RNG {context}"
        );
        assert_eq!(
            next,
            row["valid_rng_next"].as_i64().unwrap(),
            "source validation consumes no RNG {context}"
        );
        let result = generate_start(
            structure,
            &key,
            &generator,
            random_state,
            chunk,
            5,
            &height,
            seed,
        );
        if row["valid"].as_bool().unwrap() {
            eligible += 1;
            assert!(
                matches!(
                    result,
                    Err(crate::levelgen::structure::StructureError::UnsupportedStructure(_))
                ),
                "eligible point must not fabricate body success {context}"
            );
        } else {
            rejections += 1;
            let start = result.unwrap_or_else(|error| {
                panic!(
                    "genuine source rejection must not require unexecuted body: {context}: {error}"
                )
            });
            assert!(!start.is_valid(), "{context}");
            assert_eq!(
                start.create_tag(chunk),
                parse_compound_fully("{id:\"INVALID\"}").unwrap(),
                "{context}"
            );
        }
        count += 1;
    }
    assert_eq!(count, 288);
    assert!(eligible > 0 && rejections > 0 && missing_points > 0);
}

#[test]
fn mineshaft_graphs_height_moves_biome_eligibility_and_rng_match_actual_paper() {
    use crate::levelgen::structure::mineshaft::generate_pieces;
    use rivet_util::{
        RandomSource, random_source::LegacyRandomSource, worldgen_random::WorldgenRandom,
    };
    let access = build_structure_access(&build_worldgen_registries()).unwrap();
    let structures = access.lookup(&STRUCTURE).unwrap();
    let biomes = access.lookup(&BIOME).unwrap();
    let settings = access
        .lookup(&NOISE_SETTINGS)
        .unwrap()
        .get_optional_by_key(&OVERWORLD)
        .unwrap()
        .clone();
    let noise = NoiseBasedChunkGenerator::new(Holder::Direct(settings));
    let height = create(-64, 384);
    let mut random_states = HashMap::new();
    let mut count = 0;
    let mut accepted = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/mineshaft-generation.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let seed = row["seed"].as_i64().unwrap();
        let random_state = random_states
            .entry(seed)
            .or_insert_with(|| RandomState::create_from_provider(&access, &OVERWORLD, seed));
        let key = Identifier::with_default_namespace(row["structure"].as_str().unwrap());
        let structure = structures.get_optional(&key).unwrap();
        let crate::levelgen::structure::structure_body::StructureBody::Mineshaft { kind } =
            structure.body
        else {
            panic!("real mineshaft definition")
        };
        let source = FixedBiomeSource::new(biomes.get_or_throw(&ResourceKey::create(
            &BIOME,
            Identifier::parse(row["biome"].as_str().unwrap()),
        )));
        let generator = Generator {
            noise: noise.clone(),
            source: Arc::new(source),
        };
        let chunk = ChunkPos::new(
            row["x"].as_i64().unwrap() as i32,
            row["z"].as_i64().unwrap() as i32,
        );
        let label = format!("seed{seed} {} chunk{chunk:?} biome{}", key, row["biome"]);
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(seed, chunk.x(), chunk.z());
        let (pieces, dy) = generate_pieces(
            kind,
            chunk,
            &mut random,
            generator.get_sea_level(),
            generator.get_min_y(),
            |center| {
                generator.get_base_height(
                    center.get_x(),
                    center.get_z(),
                    Types::WorldSurfaceWg,
                    &height,
                    random_state,
                )
            },
        );
        assert_eq!(
            random.next_long(),
            row["rng_next"].as_i64().unwrap(),
            "full graph RNG continuation {label}"
        );
        let start = generate_start(
            structure,
            &key,
            &generator,
            random_state,
            chunk,
            5,
            &height,
            seed,
        )
        .unwrap();
        let expected = parse_compound_fully(row["saved"].as_str().unwrap()).unwrap();
        assert_eq!(
            start.is_valid(),
            row["valid"].as_bool().unwrap(),
            "biome eligibility {label}"
        );
        assert_eq!(
            start.create_tag(chunk),
            expected,
            "full ordered piece graph/NBT {label}"
        );
        assert_eq!(
            start.pieces().pieces().len() as u64,
            row["piece_count"].as_u64().unwrap(),
            "{label}"
        );
        if start.is_valid() {
            accepted += 1;
            assert_eq!(
                start.pieces(),
                &pieces,
                "independent graph and Structure.generate agree {label}"
            );
            assert_eq!(
                serde_json::json!([
                    chunk.get_middle_block_x(),
                    50i32.wrapping_add(dy),
                    chunk.get_min_block_z()
                ]),
                row["generation_point"],
                "adjusted generation point {label}"
            );
            let loaded = crate::levelgen::structure::StructureStart::load(&expected, |_| {
                Some(structure.native_definition(&key).unwrap())
            })
            .unwrap();
            assert_eq!(
                loaded.create_tag(chunk),
                expected,
                "native load/save {label}"
            );
        }
        count += 1;
    }
    assert_eq!((count, accepted), (36, 18));
    assert_eq!(random_states.len(), 3);
}

struct Generator {
    noise: NoiseBasedChunkGenerator,
    source: Arc<dyn BiomeSource>,
}
impl ChunkGenerator for Generator {
    fn get_min_y(&self) -> i32 {
        self.noise.get_min_y()
    }
    fn get_gen_depth(&self) -> i32 {
        self.noise.get_gen_depth()
    }
    fn get_sea_level(&self) -> i32 {
        self.noise.get_sea_level()
    }
    fn get_biome_source(&self) -> &dyn BiomeSource {
        &*self.source
    }
    fn get_base_height(
        &self,
        x: i32,
        z: i32,
        kind: Types,
        height: &dyn LevelHeightAccessor,
        random: &RandomState,
    ) -> i32 {
        self.noise.get_base_height(x, z, kind, height, random)
    }
}

#[test]
fn buried_treasure_generation_matches_actual_noise_heights_biomes_and_saved_starts() {
    let access = build_structure_access(&build_worldgen_registries())
        .expect("complete real structure registries");
    let structure = access
        .lookup(&STRUCTURE)
        .unwrap()
        .get_optional(&Identifier::with_default_namespace("buried_treasure"))
        .unwrap();
    let settings = access
        .lookup(&NOISE_SETTINGS)
        .unwrap()
        .get_optional_by_key(&OVERWORLD)
        .unwrap()
        .clone();
    let noise = NoiseBasedChunkGenerator::new(Holder::Direct(settings));
    let biomes = access.lookup(&BIOME).unwrap();
    let height = create(-64, 384);
    let mut random_states = HashMap::new();
    let mut rows = 0;
    let mut valid = 0;
    let mut invalid = 0;
    let mut actual_heights = HashSet::new();
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/buried-treasure-generation.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let seed = row["seed"].as_i64().unwrap();
        let random = random_states
            .entry(seed)
            .or_insert_with(|| RandomState::create_from_provider(&access, &OVERWORLD, seed));
        let biome = Identifier::parse(row["biome"].as_str().unwrap());
        let source =
            FixedBiomeSource::new(biomes.get_or_throw(&ResourceKey::create(&BIOME, biome)));
        let generator = Generator {
            noise: noise.clone(),
            source: Arc::new(source),
        };
        let chunk = ChunkPos::new(
            row["x"].as_i64().unwrap() as i32,
            row["z"].as_i64().unwrap() as i32,
        );
        let context = format!("seed {seed} biome {} chunk {chunk:?}", row["biome"]);
        let y = generator.get_first_occupied_height(
            chunk.get_middle_block_x(),
            chunk.get_middle_block_z(),
            Types::OceanFloorWg,
            &height,
            random,
        );
        assert_eq!(
            i64::from(y),
            row["height"].as_i64().unwrap(),
            "actual noise height {context}"
        );
        actual_heights.insert(y);
        let start = generate_start(
            structure,
            &Identifier::with_default_namespace("buried_treasure"),
            &generator,
            random,
            chunk,
            5,
            &height,
            seed,
        )
        .unwrap();
        assert_eq!(
            start.is_valid(),
            row["valid"].as_bool().unwrap(),
            "generation biome eligibility {context}"
        );
        assert_eq!(
            i64::from(start.references()),
            row["references"].as_i64().unwrap(),
            "source INVALID reference reset {context}"
        );
        assert_eq!(
            start.create_tag(chunk),
            parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
            "full saved start {context}"
        );
        if start.is_valid() {
            valid += 1;
            assert_eq!(start.pieces().pieces().len(), 1);
            assert_eq!(start.chunk_pos(), chunk);
        } else {
            invalid += 1;
            assert!(start.pieces().is_empty());
        }
        rows += 1;
    }
    assert_eq!((rows, valid, invalid), (30, 15, 15));
    assert_eq!(random_states.len(), 3);
    assert!(
        actual_heights.len() > 1,
        "test must execute nonconstant actual noise heights"
    );
}

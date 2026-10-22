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

struct Generator {
    noise: NoiseBasedChunkGenerator,
    source: FixedBiomeSource,
}
impl ChunkGenerator for Generator {
    fn get_min_y(&self) -> i32 {
        self.noise.get_min_y()
    }
    fn get_gen_depth(&self) -> i32 {
        self.noise.get_gen_depth()
    }
    fn get_biome_source(&self) -> &dyn BiomeSource {
        &self.source
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
            source,
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

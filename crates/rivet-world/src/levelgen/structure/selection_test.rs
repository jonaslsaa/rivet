use super::*;
use crate::biome::{
    biome_source::BiomeSource, climate::Climate, fixed_biome_source::FixedBiomeSource,
};
use crate::levelgen::structure::{
    placement::{
        PaperStructureSeeds, RandomSpreadPlacement, RandomSpreadType, StructurePlacement,
        StructurePlacementSettings,
    },
    registry_keys::STRUCTURE,
    registry_keys::STRUCTURE_SET,
    structure_registry::StructureSet,
    structure_registry::build_structure_access,
};
use rivet_registry::{
    Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey,
    holder_lookup::HolderGetter, registries::BIOME,
};
use std::sync::Arc;

fn entries(weights: &[i32]) -> Vec<StructureSelectionEntry> {
    let access = build_structure_access(
        &crate::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
    )
    .unwrap();
    let registry = access.lookup(&STRUCTURE).unwrap();
    let keys = registry.key_set();
    weights
        .iter()
        .zip(keys)
        .map(|(&weight, key)| StructureSelectionEntry {
            structure: registry.get_or_throw(&ResourceKey::create(&STRUCTURE, key)),
            weight,
        })
        .collect()
}

#[test]
fn complete_selection_order_matches_genuine_paper_create_structures() {
    let access = build_structure_access(
        &crate::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
    )
    .unwrap();
    let registry = access.lookup(&STRUCTURE).unwrap();
    let biome = access
        .lookup(&BIOME)
        .unwrap()
        .get_or_throw(&ResourceKey::create(
            &BIOME,
            Identifier::with_default_namespace("plains"),
        ));
    let source: Arc<dyn BiomeSource> = Arc::new(FixedBiomeSource::new(biome.clone()));
    let prototypes: Vec<_> = registry
        .entry_set_ref()
        .into_iter()
        .take(4)
        .map(|(_, value)| {
            let mut value = value.clone();
            value.settings.biomes =
                rivet_registry::holder_set::HolderSet::direct(vec![biome.clone()]);
            Holder::direct(value)
        })
        .collect();
    struct Context<'a> {
        candidates: &'a [Holder<Structure>],
        existing: bool,
        stop: i64,
        attempts: Vec<usize>,
    }
    impl StructureSelectionContext for Context<'_> {
        type Error = std::convert::Infallible;
        fn has_valid_start(&mut self, structure: &Holder<Structure>) -> Result<bool, Self::Error> {
            Ok(self.existing && *structure == self.candidates[0])
        }
        fn try_generate(&mut self, entry: &StructureSelectionEntry) -> Result<bool, Self::Error> {
            let index = self
                .candidates
                .iter()
                .position(|candidate| *candidate == entry.structure)
                .unwrap();
            self.attempts.push(index);
            Ok(index as i64 == self.stop)
        }
    }
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-selection.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let weights = row["weights"].as_array().unwrap();
        let entries: Vec<_> = weights
            .iter()
            .enumerate()
            .map(|(index, weight)| StructureSelectionEntry {
                structure: prototypes[index].clone(),
                weight: weight.as_i64().unwrap() as i32,
            })
            .collect();
        let mut sets = RegistryBuilder::new(&STRUCTURE_SET);
        for index in 0..row["set_count"].as_u64().unwrap() {
            sets.register(
                &ResourceKey::create(
                    &STRUCTURE_SET,
                    Identifier::parse(&format!("probe:set_{index}")),
                ),
                Arc::new(StructureSet {
                    structures: entries.clone(),
                    placement: StructurePlacement::RandomSpread(RandomSpreadPlacement {
                        settings: StructurePlacementSettings::default(),
                        spacing: 1,
                        separation: 0,
                        spread_type: RandomSpreadType::Linear,
                    }),
                }),
                RegistrationInfo::BUILT_IN,
            );
        }
        let structures = RegistryBuilder::<Structure>::new(&STRUCTURE).freeze();
        let access = RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(STRUCTURE.identifier().clone()),
                Box::new(structures),
            ),
            (
                ResourceKey::create_registry_key(STRUCTURE_SET.identifier().clone()),
                Box::new(sets.freeze()),
            ),
        ]);
        let mut state = ChunkGeneratorStructureState::create_for_normal(
            access,
            row["seed"].as_i64().unwrap(),
            source.clone(),
            Climate::empty(),
            PaperStructureSeeds::default(),
        )
        .unwrap();
        let mut context = Context {
            candidates: &prototypes,
            existing: row["existing"].as_bool().unwrap(),
            stop: row["stop_index"].as_i64().unwrap(),
            attempts: Vec::new(),
        };
        let result = create_structures(
            &mut state,
            ChunkPos::new(
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            ),
            &mut context,
        );
        assert_eq!(
            matches!(result, Err(SelectionError::InvalidWeightTotal(_))),
            row["result"] == "invalid_bound",
            "{row}"
        );
        if row["result"] != "invalid_bound" {
            result.unwrap();
        }
        assert_eq!(
            serde_json::json!(context.attempts),
            row["attempts"],
            "{row}"
        );
    }
}

#[test]
fn unsupported_generation_is_not_retried_and_cancelled_valid_stops() {
    let entries = entries(&[1, 2, 3]);
    let mut attempts = 0;
    let result = try_generate_entries(&entries, 42, ChunkPos::new(-4, 7), |_| {
        attempts += 1;
        Err::<bool, _>("unsupported body")
    });
    assert!(matches!(
        result,
        Err(SelectionError::Generation("unsupported body"))
    ));
    assert_eq!(attempts, 1);
    attempts = 0;
    try_generate_entries(&entries, 42, ChunkPos::new(-4, 7), |_| {
        attempts += 1;
        Ok::<_, std::convert::Infallible>(true)
    })
    .unwrap();
    assert_eq!(
        attempts, 1,
        "valid but cancelled attempts count as source true"
    );
    let mut attempted = Vec::new();
    try_generate_entries(&entries, 42, ChunkPos::new(-4, 7), |entry| {
        attempted.push(entry.structure.clone());
        Ok::<_, std::convert::Infallible>(false)
    })
    .unwrap();
    assert_eq!(attempted.len(), entries.len());
    for entry in entries {
        assert_eq!(
            attempted.iter().filter(|h| **h == entry.structure).count(),
            1
        );
    }
}

#[test]
fn wrapped_invalid_weight_total_is_an_explicit_failure() {
    let entries = entries(&[i32::MAX, 1]);
    let result = try_generate_entries(
        &entries,
        0,
        ChunkPos::new(0, 0),
        |_| -> Result<bool, std::convert::Infallible> {
            panic!("nextInt rejects the wrapped bound before attempting generation")
        },
    );
    assert!(matches!(
        result,
        Err(SelectionError::InvalidWeightTotal(i32::MIN))
    ));
}

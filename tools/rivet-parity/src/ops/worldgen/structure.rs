//! `structure.start`: the structure starts of one structure set over a chunk grid.
//!
//! The contract (identical to `RivetReferenceOracle.structureStart`): for one
//! structure set and world seed, scan the chunk grid [-48, 47]^2 of the set's
//! dimension (`overworld`, `nether`, `end`: the real biome source and noise
//! settings of that dimension) the way `ChunkGenerator.createStructures` does,
//! without a chunk: every chunk where the set's (Paper-injected) placement
//! holds is a `candidate`; its start is the weighted selection of
//! `setLargeFeatureSeed(seed, x, z)` over the set's structures, each tried
//! through `Structure.generate` until one is valid. Pieces are the
//! `StructureStart` pieces in order (`type`, `box` [minX, minY, minZ, maxX,
//! maxY, maxZ], optional `rotation` / `mirror`, `depth`); no block is placed.
//! A generation that throws is the verdict `threw` on that chunk.
//!
//! Rivet generates starts only for the structures whose piece builders are
//! ported (the mineshafts and the buried treasure) in the overworld: a set
//! that contains any other structure answers `Unported`, naming it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rivet_registry::core::{ChunkPos, Direction};
use rivet_registry::{Holder, Identifier};
use rivet_server::server::level::generated_world::OverworldGenerator;
use rivet_world::level::height_accessor::create as create_height_accessor;
use rivet_world::levelgen::structure::generation::generate_start;
use rivet_world::levelgen::structure::placement::PaperStructureSeeds;
use rivet_world::levelgen::structure::registry_keys::STRUCTURE;
use rivet_world::levelgen::structure::selection::{
    SelectionError, StructureSelectionContext, create_structures,
};
use rivet_world::levelgen::structure::state::ChunkGeneratorStructureState;
use rivet_world::levelgen::structure::structure_registry::{Structure, StructureSelectionEntry};
use rivet_world::levelgen::structure::{StructureError, StructureStart};
use serde_json::{Map, Value, json};

use super::feature::{HEIGHT, MIN_Y};
use super::{panic_message, request_i64, request_str};
use crate::ops::compare::Outcome;
use crate::ops::packet;

/// Half-width of the scanned chunk grid (`RivetReferenceOracle.STRUCTURE_GRID`).
const GRID: i32 = 48;

/// The dimension a set generates in.
pub fn dimension_of(key: &str) -> &'static str {
    match key {
        "minecraft:end_cities" => "end",
        "minecraft:nether_complexes" | "minecraft:nether_fossils" => "nether",
        _ => "overworld",
    }
}

thread_local! {
    static GENERATORS: RefCell<HashMap<i64, Arc<OverworldGenerator>>> = RefCell::new(HashMap::new());
}

fn generator(seed: i64) -> Arc<OverworldGenerator> {
    GENERATORS.with(|generators| {
        Arc::clone(
            generators
                .borrow_mut()
                .entry(seed)
                .or_insert_with(|| Arc::new(OverworldGenerator::new(seed))),
        )
    })
}

/// What the selection callback saw at one chunk.
struct Recorder<'a> {
    generator: &'a OverworldGenerator,
    target: &'a HashSet<Identifier>,
    pos: ChunkPos,
    found: Option<(Identifier, StructureStart)>,
}

impl StructureSelectionContext for Recorder<'_> {
    type Error = StructureError;

    fn has_valid_start(&mut self, _structure: &Holder<Structure>) -> Result<bool, Self::Error> {
        // Nothing is generated beforehand: every chunk starts empty.
        Ok(false)
    }

    fn try_generate(&mut self, entry: &StructureSelectionEntry) -> Result<bool, Self::Error> {
        let access = self.generator.registry_access();
        let registry = access
            .lookup(&STRUCTURE)
            .ok_or(StructureError::RegistryUnavailable)?;
        let key = entry
            .structure
            .unwrap_key(registry)
            .ok_or(StructureError::RegistryUnavailable)?
            .identifier()
            .clone();
        if !self.target.contains(&key) {
            // Another set's structure at this chunk: not part of this op's answer.
            return Ok(false);
        }
        let height = create_height_accessor(MIN_Y, HEIGHT);
        let start = generate_start(
            entry.structure.value(registry),
            &key,
            self.generator,
            self.generator.random_state(),
            self.pos,
            0,
            &height,
            self.generator.seed(),
        )?;
        let valid = start.is_valid();
        if valid {
            self.found = Some((key, start));
        }
        Ok(valid)
    }
}

/// `structure.start`: `{structure_set, seed, dimension}` -> `{candidates, starts}`.
pub fn structure_start(request: &Map<String, Value>) -> Outcome {
    let key = match request_str(request, "structure_set") {
        Ok(key) => key,
        Err(outcome) => return outcome,
    };
    let seed = match request_i64(request, "seed") {
        Ok(seed) => seed,
        Err(outcome) => return outcome,
    };
    let dimension = match request_str(request, "dimension") {
        Ok(dimension) => dimension,
        Err(outcome) => return outcome,
    };
    let set_key = Identifier::parse(key);
    let generator = generator(seed);
    let access = generator.registry_access();
    let Some(structures) = access
        .lookup(&rivet_world::levelgen::structure::registry_keys::STRUCTURE_SET)
        .and_then(|sets| sets.get_optional(&set_key))
    else {
        return Outcome::Unavailable(format!("structure set {key} is not in Rivet's registry"));
    };
    let Some(structure_registry) = access.lookup(&STRUCTURE) else {
        return Outcome::Unavailable("the structure registry is not built".into());
    };
    let mut target = HashSet::new();
    for entry in &structures.structures {
        let name = entry
            .structure
            .unwrap_key(structure_registry)
            .expect("registry-held structure")
            .identifier()
            .clone();
        if let Err(error) = entry
            .structure
            .value(structure_registry)
            .native_definition(&name)
        {
            return Outcome::Unported(format!("structure piece builder not ported: {error}"));
        }
        target.insert(name);
    }
    if dimension != "overworld" {
        return Outcome::Unported(format!(
            "Rivet generates structure starts only in the overworld, not {dimension}"
        ));
    }
    match packet::guarded(|| scan(&generator, &set_key, &target, seed)) {
        Ok(Ok(result)) => Outcome::Accepted(result),
        Ok(Err(reason)) => Outcome::Unavailable(reason),
        Err(payload) => {
            Outcome::Unavailable(format!("Rust panic: {}", panic_message(payload.as_ref())))
        }
    }
}

fn scan(
    generator: &OverworldGenerator,
    set_key: &Identifier,
    target: &HashSet<Identifier>,
    seed: i64,
) -> Result<Map<String, Value>, String> {
    let mut state = ChunkGeneratorStructureState::create_for_normal(
        generator.registry_access().clone(),
        seed,
        generator.biome_source().shared_source(),
        generator.random_state().sampler().clone(),
        PaperStructureSeeds::default(),
    )
    .map_err(|e| format!("structure state: {e}"))?;
    let mut candidates = Vec::new();
    let mut starts = Vec::new();
    for x in -GRID..GRID {
        for z in -GRID..GRID {
            if !state
                .is_structure_chunk(set_key, x, z)
                .map_err(|e| format!("placement at {x},{z}: {e}"))?
            {
                continue;
            }
            candidates.push(json!([x, z]));
            let mut recorder = Recorder {
                generator,
                target,
                pos: ChunkPos::new(x, z),
                found: None,
            };
            match create_structures(&mut state, ChunkPos::new(x, z), &mut recorder) {
                Ok(()) => {}
                Err(SelectionError::Generation(error)) => {
                    return Err(format!("generation at {x},{z}: {error}"));
                }
                Err(other) => return Err(format!("selection at {x},{z}: {other}")),
            }
            if let Some((structure, start)) = recorder.found {
                let mut pieces = Vec::new();
                for piece in start.pieces().pieces() {
                    let data = piece.data();
                    let tag = piece.create_tag();
                    let mut entry = Map::new();
                    entry.insert("type".into(), json!(tag.get_string_or("id", "")));
                    let b = data.bounding_box;
                    entry.insert(
                        "box".into(),
                        json!([b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z]),
                    );
                    // `StructurePiece.setOrientation` derives the piece's rotation and mirror
                    // from its facing; a piece that never had one has neither.
                    if let Some(direction) = data.orientation {
                        let (rotation, mirror) = match direction {
                            Direction::South => ("NONE", "LEFT_RIGHT"),
                            Direction::West => ("CLOCKWISE_90", "LEFT_RIGHT"),
                            Direction::East => ("CLOCKWISE_90", "NONE"),
                            _ => ("NONE", "NONE"),
                        };
                        entry.insert("rotation".into(), json!(rotation));
                        entry.insert("mirror".into(), json!(mirror));
                    }
                    entry.insert("depth".into(), json!(data.gen_depth));
                    pieces.push(Value::Object(entry));
                }
                starts.push(json!({
                    "chunk": [x, z],
                    "structure": structure.to_string(),
                    "pieces": pieces,
                }));
            }
        }
    }
    let mut result = Map::new();
    result.insert("candidates".into(), Value::Array(candidates));
    result.insert("starts".into(), Value::Array(starts));
    Ok(result)
}

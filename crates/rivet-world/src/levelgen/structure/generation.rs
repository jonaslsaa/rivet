//! Structure generation-point validation, separate from set selection and the
//! server's cancellable structure-spawn event. Unsupported bodies cannot be
//! interpreted as an invalid biome or an empty successful start.

use super::{StructureDefinition, StructureError, StructureStart};
use super::{structure_body::StructureBody, structure_registry::Structure};
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::heightmap::Types;
use crate::levelgen::noisegen::random_state::RandomState;
use rivet_registry::{
    Identifier,
    core::{BlockPos, ChunkPos},
};
use rivet_util::{random_source::LegacyRandomSource, worldgen_random::WorldgenRandom};

#[cfg(test)]
#[path = "generation_fixture.rs"]
mod tests;

/// The supported `Structure.generate` body. The generator supplies both the
/// actual base-height query and the canonical biome source used by Paper's
/// `isValidBiome`, including quart conversion of the generation-point height.
#[allow(clippy::too_many_arguments)] // Structure.generate's independent context inputs.
pub fn generate_start(
    structure: &Structure,
    key: &Identifier,
    generator: &dyn ChunkGenerator,
    random_state: &RandomState,
    chunk: ChunkPos,
    references: i32,
    height: &dyn LevelHeightAccessor,
    seed: i64,
) -> Result<StructureStart, StructureError> {
    // These source families return lazy piece-builder consumers. Validation
    // precedes body execution: genuine rejection is supported even while the
    // eligible piece builder remains explicitly unavailable.
    if matches!(
        structure.body,
        StructureBody::Shipwreck { .. } | StructureBody::Igloo | StructureBody::DesertPyramid
    ) {
        let Some(point) =
            find_lazy_generation_point(&structure.body, generator, random_state, chunk, height)?
        else {
            return Ok(StructureStart::invalid());
        };
        let biome = generator.get_biome_source().get_noise_biome(
            point.get_x() >> 2,
            point.get_y() >> 2,
            point.get_z() >> 2,
            random_state.sampler(),
        );
        if !structure.settings.biomes.contains(&biome) {
            return Ok(StructureStart::invalid());
        }
        return Err(StructureError::UnsupportedStructure(format!(
            "{key} ({}) piece builder",
            structure.body.type_id()
        )));
    }
    let definition = structure.native_definition(key)?;
    match &definition {
        StructureDefinition::Mineshaft { kind, .. } => {
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
            random.set_large_feature_seed(seed, chunk.x(), chunk.z());
            let (pieces, dy) = super::mineshaft::generate_pieces(
                *kind,
                chunk,
                &mut random,
                generator.get_sea_level(),
                generator.get_min_y(),
                |center| {
                    generator.get_base_height(
                        center.get_x(),
                        center.get_z(),
                        Types::WorldSurfaceWg,
                        height,
                        random_state,
                    )
                },
            );
            let biome = generator.get_biome_source().get_noise_biome(
                chunk.get_middle_block_x() >> 2,
                50i32.wrapping_add(dy) >> 2,
                chunk.get_min_block_z() >> 2,
                random_state.sampler(),
            );
            if !structure.settings.biomes.contains(&biome) {
                return Ok(StructureStart::invalid());
            }
            Ok(StructureStart::new(definition, chunk, references, pieces))
        }
        StructureDefinition::BuriedTreasure { .. } => {
            let x = chunk.get_min_block_x().wrapping_add(8);
            let z = chunk.get_min_block_z().wrapping_add(8);
            let y = generator.get_first_occupied_height(
                x,
                z,
                Types::OceanFloorWg,
                height,
                random_state,
            );
            let biome = generator.get_biome_source().get_noise_biome(
                x >> 2,
                y >> 2,
                z >> 2,
                random_state.sampler(),
            );
            if !structure.settings.biomes.contains(&biome) {
                return Ok(StructureStart::invalid());
            }
            Ok(StructureStart::new(
                definition,
                chunk,
                references,
                StructureDefinition::buried_treasure_pieces(chunk),
            ))
        }
    }
}

/// The exact lazy stub positions for Shipwreck, Igloo and SinglePieceStructure
/// (DesertPyramid width/depth 21). No builder callback or RNG draw occurs here.
pub fn find_lazy_generation_point(
    body: &StructureBody,
    generator: &dyn ChunkGenerator,
    random_state: &RandomState,
    chunk: ChunkPos,
    height: &dyn LevelHeightAccessor,
) -> Result<Option<BlockPos>, StructureError> {
    let kind = match body {
        StructureBody::Shipwreck { is_beached } => {
            if *is_beached {
                Types::WorldSurfaceWg
            } else {
                Types::OceanFloorWg
            }
        }
        StructureBody::Igloo => Types::WorldSurfaceWg,
        StructureBody::DesertPyramid => {
            let x = chunk.get_min_block_x();
            let z = chunk.get_min_block_z();
            let mut lowest = i32::MAX;
            for (cx, cz) in [
                (x, z),
                (x, z.wrapping_add(21)),
                (x.wrapping_add(21), z),
                (x.wrapping_add(21), z.wrapping_add(21)),
            ] {
                lowest = lowest.min(generator.get_first_occupied_height(
                    cx,
                    cz,
                    Types::WorldSurfaceWg,
                    height,
                    random_state,
                ));
            }
            if lowest < generator.get_sea_level() {
                return Ok(None);
            }
            Types::WorldSurfaceWg
        }
        _ => {
            return Err(StructureError::UnsupportedStructure(format!(
                "{} lazy generation point",
                body.type_id()
            )));
        }
    };
    let x = chunk.get_middle_block_x();
    let z = chunk.get_middle_block_z();
    Ok(Some(BlockPos::new(
        x,
        generator.get_first_occupied_height(x, z, kind, height, random_state),
        z,
    )))
}

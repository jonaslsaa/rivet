//! Structure generation-point validation, separate from set selection and the
//! server's cancellable structure-spawn event. Unsupported bodies cannot be
//! interpreted as an invalid biome or an empty successful start.

use super::structure_registry::Structure;
use super::{StructureDefinition, StructureError, StructureStart};
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::heightmap::Types;
use crate::levelgen::noisegen::random_state::RandomState;
use rivet_registry::{Identifier, core::ChunkPos};

#[cfg(test)]
#[path = "generation_fixture.rs"]
mod tests;

/// The supported `Structure.generate` body. The generator supplies both the
/// actual base-height query and the canonical biome source used by Paper's
/// `isValidBiome`, including quart conversion of the generation-point height.
pub fn generate_start(
    structure: &Structure,
    key: &Identifier,
    generator: &dyn ChunkGenerator,
    random_state: &RandomState,
    chunk: ChunkPos,
    references: i32,
    height: &dyn LevelHeightAccessor,
) -> Result<StructureStart, StructureError> {
    let definition = structure.native_definition(key)?;
    match &definition {
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

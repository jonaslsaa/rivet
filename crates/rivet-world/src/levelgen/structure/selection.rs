//! `ChunkGenerator.createStructures`: existing-start short circuit, placement,
//! then weighted attempts without replacement. Callers retain generation/event
//! authority: a cancelled valid start must return `true`, not trigger a retry.
use super::{
    placement::PlacementError,
    state::ChunkGeneratorStructureState,
    structure_registry::{Structure, StructureSelectionEntry},
};
use rivet_registry::{Holder, core::ChunkPos};
use rivet_util::{
    RandomSource, random_source::LegacyRandomSource, worldgen_random::WorldgenRandom,
};

#[derive(Debug, thiserror::Error)]
pub enum SelectionError<E> {
    #[error(transparent)]
    Placement(#[from] PlacementError),
    #[error("structure generation callback failed: {0}")]
    Generation(E),
    #[error("Java weighted structure choice requires a positive wrapped total, got {0}")]
    InvalidWeightTotal(i32),
}

pub trait StructureSelectionContext {
    type Error;

    fn has_valid_start(&mut self, structure: &Holder<Structure>) -> Result<bool, Self::Error>;

    /// Mirrors Paper's `tryGenerateStructure` result: invalid generation is
    /// false; stored OR event-cancelled valid generation is true. Unsupported
    /// capability is an error, never a failed attempt eligible for retry.
    fn try_generate(&mut self, entry: &StructureSelectionEntry) -> Result<bool, Self::Error>;
}

pub fn create_structures<C: StructureSelectionContext>(
    state: &mut ChunkGeneratorStructureState,
    chunk: ChunkPos,
    context: &mut C,
) -> Result<(), SelectionError<C::Error>> {
    // Clones retain placement Arc and holder identity while permitting mutable
    // lazy ring-cache queries; iteration remains the source registry order.
    for set in state.possible_structure_sets().to_vec() {
        let mut existing = false;
        for entry in &set.structures {
            if context
                .has_valid_start(&entry.structure)
                .map_err(SelectionError::Generation)?
            {
                existing = true;
                break;
            }
        }
        if existing || !state.is_structure_chunk(&set.key, chunk.x(), chunk.z())? {
            continue;
        }
        try_generate_entries(&set.structures, state.level_seed(), chunk, |entry| {
            context.try_generate(entry)
        })?;
    }
    Ok(())
}

fn try_generate_entries<E>(
    entries: &[StructureSelectionEntry],
    seed: i64,
    chunk: ChunkPos,
    mut generate: impl FnMut(&StructureSelectionEntry) -> Result<bool, E>,
) -> Result<(), SelectionError<E>> {
    if entries.len() == 1 {
        generate(&entries[0]).map_err(SelectionError::Generation)?;
        return Ok(());
    }
    let mut options: Vec<_> = entries.iter().collect();
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(seed, chunk.x(), chunk.z());
    let mut total = options
        .iter()
        .fold(0i32, |sum, entry| sum.wrapping_add(entry.weight));
    while !options.is_empty() {
        if total <= 0 {
            return Err(SelectionError::InvalidWeightTotal(total));
        }
        let mut choice = random.next_int_bound(total);
        let mut index = 0;
        for entry in &options {
            choice = choice.wrapping_sub(entry.weight);
            if choice < 0 {
                break;
            }
            index += 1;
        }
        let selected = options[index];
        if generate(selected).map_err(SelectionError::Generation)? {
            return Ok(());
        }
        options.remove(index);
        total = total.wrapping_sub(selected.weight);
    }
    Ok(())
}

#[cfg(test)]
#[path = "selection_test.rs"]
mod tests;

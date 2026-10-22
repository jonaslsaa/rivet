//! The structure half of addVanillaDecorations. Registry order selects the
//! per-step seed index; each structure's canonical fastutil reference set
//! selects start order. No outer identity-map ordering is invented here.
use super::structure_manager::StructureManagerError;
use rivet_registry::{Identifier, access::RegistryAccess, core::ChunkPos};
use rivet_util::{RandomSource, worldgen_random::WorldgenRandom};
use rivet_world::{
    chunk::structure_access::StructureStartEntry,
    level::height_accessor::LevelHeightAccessor,
    levelgen::{
        generation_step::Decoration,
        structure::{
            BoundingBox, StructureError, StructurePlacementAuthority, registry_keys::STRUCTURE,
        },
    },
};

/// WorldGenLevel.setBlock has no Result channel. The real adapter carries a
/// typed failure through that signature; the start scope restores its canonical
/// loan and converts this payload back to a normal placement error.
pub(super) struct StructurePlacementFailure(pub StructureError);

#[derive(Debug, thiserror::Error)]
pub enum StructurePlacementError {
    #[error("native FEATURES workspace duplicates the canonical center chunk {0:?}")]
    DuplicateCanonicalChunk(ChunkPos),
    #[error("STRUCTURE registry authority is unavailable for decoration")]
    RegistryUnavailable,
    #[error(transparent)]
    References(#[from] StructureManagerError),
    #[error("{0}")]
    RegionUnavailable(super::world_gen_region::UnavailableChunkDiagnostic),
    #[error("structure {structure} from {source_chunk:?}: {reason}")]
    Start {
        source_chunk: ChunkPos,
        structure: Identifier,
        reason: StructureError,
    },
    #[error("canonical structure {structure} in {source_chunk:?} disappeared while being placed")]
    MissingLoan {
        source_chunk: ChunkPos,
        structure: Identifier,
    },
}

/// Implemented by the real transformer/region adapter, not by a detached
/// copied-start view. Reads enforce the region's actual status/radius contract.
/// A taken entry has one owner; returning it must be non-panicking and cannot
/// overwrite another entry. World callbacks cannot mutate this loan map.
pub trait StructureDecorationAccess: StructurePlacementAuthority {
    fn referenced_chunks(
        &self,
        center: ChunkPos,
        structure: &Identifier,
    ) -> Result<Vec<ChunkPos>, StructurePlacementError>;
    fn start_entry(
        &self,
        source: ChunkPos,
        structure: &Identifier,
    ) -> Result<Option<&StructureStartEntry>, StructurePlacementError>;
    fn take_start_entry(
        &mut self,
        source: ChunkPos,
        structure: &Identifier,
    ) -> Result<Option<StructureStartEntry>, StructurePlacementError>;
    fn return_start_entry(
        &mut self,
        source: ChunkPos,
        structure: Identifier,
        entry: StructureStartEntry,
    );
}

pub struct StructureDecorationPlan {
    by_step: Vec<Vec<Identifier>>,
}

impl StructureDecorationPlan {
    pub fn new(access: &RegistryAccess) -> Result<Self, StructurePlacementError> {
        let registry = access
            .lookup(&STRUCTURE)
            .ok_or(StructurePlacementError::RegistryUnavailable)?;
        let mut by_step = vec![Vec::new(); Decoration::VALUES.len()];
        // entry_set_ref follows the actual dense registry stream. Filtering
        // that stream by step preserves groupingBy's per-list encounter order.
        for (key, structure) in registry.entry_set_ref() {
            let step = Decoration::VALUES
                .iter()
                .position(|step| *step == structure.settings.step)
                .expect("registered decoration enum");
            by_step[step].push(key.identifier().clone());
        }
        Ok(Self { by_step })
    }

    /// Execute immediately BEFORE placed features at this step, sharing their
    /// WorldgenRandom. Empty reference lists still consume the registered
    /// structure's index and perform setFeatureSeed, as Paper does.
    pub fn place_step<R: RandomSource + 'static>(
        &self,
        access: &mut impl StructureDecorationAccess,
        random: &mut WorldgenRandom<R>,
        decoration_seed: i64,
        step: usize,
        center: ChunkPos,
        writable: BoundingBox,
    ) -> Result<(), StructurePlacementError> {
        let Some(structures) = self.by_step.get(step) else {
            return Ok(());
        };
        for (index, structure) in structures.iter().enumerate() {
            random.set_feature_seed(decoration_seed, index as i32, step as i32);
            // startsForStructure eagerly builds the entire valid-start list
            // before placing its first element. A deferred later reference
            // must therefore fail before an earlier start mutates terrain.
            let mut starts = Vec::new();
            for source in access.referenced_chunks(center, structure)? {
                if let Some(entry) = access.start_entry(source, structure)? {
                    let start =
                        entry
                            .native()
                            .map_err(|reason| StructurePlacementError::Start {
                                source_chunk: source,
                                structure: structure.clone(),
                                reason,
                            })?;
                    if start.is_valid() {
                        starts.push(source);
                    }
                }
            }
            for source in starts {
                let mut entry = access.take_start_entry(source, structure)?.ok_or_else(|| {
                    StructurePlacementError::MissingLoan {
                        source_chunk: source,
                        structure: structure.clone(),
                    }
                })?;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    entry
                        .native_mut()
                        .map_err(|reason| StructurePlacementError::Start {
                            source_chunk: source,
                            structure: structure.clone(),
                            reason,
                        })?
                        .place_in_chunk(access, random, writable, center)
                        .map_err(|reason| StructurePlacementError::Start {
                            source_chunk: source,
                            structure: structure.clone(),
                            reason,
                        })
                }));
                // Source piece bounds/state mutations survive both errors and
                // panics. Never reconstruct a start from a copied NBT payload.
                access.return_start_entry(source, structure.clone(), entry);
                match result {
                    Ok(result) => result?,
                    Err(payload) => match payload.downcast::<StructurePlacementFailure>() {
                        Ok(failure) => {
                            return Err(StructurePlacementError::Start {
                                source_chunk: source,
                                structure: structure.clone(),
                                reason: failure.0,
                            });
                        }
                        Err(payload) => std::panic::resume_unwind(payload),
                    },
                }
            }
        }
        Ok(())
    }
}

/// ChunkGenerator.getWritableArea: generation minY+1, inclusive maxY, XZ
/// restricted to the center chunk (not the region's wider block-write radius).
pub fn writable_area(center: ChunkPos, height: &dyn LevelHeightAccessor) -> BoundingBox {
    let x = center.get_min_block_x();
    let z = center.get_min_block_z();
    BoundingBox::new([
        x,
        height.get_min_y().wrapping_add(1),
        z,
        x.wrapping_add(15),
        height.get_max_y(),
        z.wrapping_add(15),
    ])
}

//! The real createStructures task over canonical owner storage. Selection,
//! generation, event cancellation and starts-available publication are separate
//! operations: an unavailable operation never becomes an invalid start.
use super::{
    generated_world::OverworldGenerator,
    structure_manager::{StructureChunkStorage, StructureManagerError},
    structure_spawn_events::{
        SpawnDecision, StructureSpawnAuthority, StructureSpawnDispatchError, StructureSpawnPayload,
        StructureWorldIdentity,
    },
};
use rivet_registry::{Holder, Identifier, core::ChunkPos};
use rivet_world::{
    chunk::status::ChunkStatus,
    level::height_accessor::LevelHeightAccessor,
    levelgen::structure::{
        StructureError, StructureStart,
        generation::generate_start,
        placement::{PaperStructureSeeds, PlacementError},
        registry_keys::STRUCTURE,
        selection::{SelectionError, StructureSelectionContext, create_structures},
        state::ChunkGeneratorStructureState,
        structure_registry::{Structure, StructureSelectionEntry},
    },
};
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
#[path = "structure_generation_fixture.rs"]
mod paper_fixture;

#[derive(Debug, thiserror::Error)]
pub enum StructureGenerationError {
    #[error(transparent)]
    Storage(#[from] StructureManagerError),
    #[error(transparent)]
    Body(#[from] StructureError),
    #[error(transparent)]
    Event(#[from] StructureSpawnDispatchError),
    #[error(transparent)]
    Placement(#[from] PlacementError),
    #[error("structure selection: {0}")]
    Selection(String),
    #[error("structure generation runtime belongs to another world")]
    DifferentWorld,
}

/// Paper's per-world generator-state and StructureCheck full-results cache.
/// The event authority must be explicitly provided by the boot owner; there is
/// deliberately no default accepted/empty listener implementation here.
pub struct StructureGenerationRuntime {
    generator: Arc<OverworldGenerator>,
    state: ChunkGeneratorStructureState,
    world: StructureWorldIdentity,
    events: StructureSpawnAuthority,
    loaded_starts: HashMap<ChunkPos, HashMap<Identifier, i32>>,
}

impl StructureGenerationRuntime {
    pub fn new(
        generator: Arc<OverworldGenerator>,
        world: StructureWorldIdentity,
        seeds: PaperStructureSeeds,
        events: StructureSpawnAuthority,
    ) -> Result<Self, StructureGenerationError> {
        if world.seed() != generator.seed() {
            return Err(StructureGenerationError::DifferentWorld);
        }
        let state = ChunkGeneratorStructureState::create_for_normal(
            generator.registry_access().clone(),
            generator.seed(),
            generator.biome_source().shared_source(),
            generator.random_state().sampler().clone(),
            seeds,
        )?;
        Ok(Self {
            generator,
            state,
            world,
            events,
            loaded_starts: HashMap::new(),
        })
    }

    pub fn belongs_to(&self, generator: &Arc<OverworldGenerator>) -> bool {
        Arc::ptr_eq(&self.generator, generator)
    }

    /// None means not published, while Some(empty) is an authoritative loaded
    /// chunk with no valid starts. Invalid starts never enter this cache.
    pub fn loaded_starts(&self, pos: ChunkPos) -> Option<&HashMap<Identifier, i32>> {
        self.loaded_starts.get(&pos)
    }

    pub fn generate_starts(
        &mut self,
        storage: &mut impl StructureChunkStorage,
        pos: ChunkPos,
        height: &dyn LevelHeightAccessor,
    ) -> Result<(), StructureGenerationError> {
        require_center(storage, pos)?;
        let mut context = StartContext {
            storage,
            pos,
            height,
            generator: &self.generator,
            world: &self.world,
            events: &self.events,
        };
        create_structures(&mut self.state, pos, &mut context).map_err(|error| match error {
            SelectionError::Generation(error) => error,
            SelectionError::Placement(error) => error.into(),
            other => StructureGenerationError::Selection(other.to_string()),
        })?;
        self.on_starts_available(storage, pos)
    }

    /// Source onStructureStartsAvailable/onStructureLoad: publish full valid
    /// start/reference results only after the whole task succeeds. The owner
    /// invokes this on its tick thread, where Paper queues the notification.
    pub fn on_starts_available(
        &mut self,
        storage: &impl StructureChunkStorage,
        pos: ChunkPos,
    ) -> Result<(), StructureGenerationError> {
        require_center(storage, pos)?;
        let mut results = HashMap::new();
        for key in storage.start_keys(pos) {
            let start = read_start(storage, pos, &key)?.expect("canonical start key");
            if start.is_valid() {
                results.insert(key, start.references());
            }
        }
        self.loaded_starts.insert(pos, results);
        Ok(())
    }
}

fn require_center(
    storage: &impl StructureChunkStorage,
    pos: ChunkPos,
) -> Result<(), StructureManagerError> {
    if storage.status(pos).is_none() {
        return Err(StructureManagerError::MissingChunk {
            pos,
            required: ChunkStatus::Empty,
        });
    }
    if storage.has_unkeyed_starts(pos) {
        return Err(StructureManagerError::UnresolvedUnkeyedStarts { pos });
    }
    Ok(())
}

fn read_start<'a>(
    storage: &'a impl StructureChunkStorage,
    pos: ChunkPos,
    key: &Identifier,
) -> Result<Option<&'a StructureStart>, StructureManagerError> {
    require_center(storage, pos)?;
    storage
        .start(pos, key)
        .map(|entry| {
            entry
                .native()
                .map_err(|reason| StructureManagerError::StartUnavailable {
                    pos,
                    key: key.clone(),
                    reason,
                })
        })
        .transpose()
}

struct StartContext<'a, S> {
    storage: &'a mut S,
    pos: ChunkPos,
    height: &'a dyn LevelHeightAccessor,
    generator: &'a OverworldGenerator,
    world: &'a StructureWorldIdentity,
    events: &'a StructureSpawnAuthority,
}

impl<S: StructureChunkStorage> StructureSelectionContext for StartContext<'_, S> {
    type Error = StructureGenerationError;

    fn has_valid_start(&mut self, holder: &Holder<Structure>) -> Result<bool, Self::Error> {
        let registry = self
            .generator
            .registry_access()
            .lookup(&STRUCTURE)
            .ok_or(StructureError::RegistryUnavailable)?;
        let key = holder
            .unwrap_key(registry)
            .ok_or(StructureError::RegistryUnavailable)?;
        Ok(read_start(self.storage, self.pos, key.identifier())?
            .is_some_and(StructureStart::is_valid))
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
        let references =
            read_start(self.storage, self.pos, &key)?.map_or(0, StructureStart::references);
        let start = generate_start(
            entry.structure.value(registry),
            &key,
            self.generator,
            self.generator.random_state(),
            self.pos,
            references,
            self.height,
            self.generator.seed(),
        )?;
        if !start.is_valid() {
            return Ok(false);
        }
        let event = StructureSpawnPayload::new(
            self.world.clone(),
            access,
            key.clone(),
            start.bounding_box()?,
            self.pos,
        )?;
        if self.events.dispatch(event)? == SpawnDecision::Accepted {
            self.storage.set_start(self.pos, key, start.into());
        }
        // Cancellation consumes this successful weighted attempt too.
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::generated_chunk_owner::{GeneratedChunkOwner, GenerationReadiness};
    use super::*;
    use rivet_world::levelgen::structure::{StructureDefinition, TerrainAdjustment};

    static GENERATOR: std::sync::LazyLock<Arc<OverworldGenerator>> =
        std::sync::LazyLock::new(|| Arc::new(OverworldGenerator::new(42)));

    fn runtime(generator: &Arc<OverworldGenerator>) -> StructureGenerationRuntime {
        StructureGenerationRuntime::new(
            Arc::clone(generator),
            StructureWorldIdentity::new(generator.seed(), Identifier::parse("minecraft:overworld")),
            PaperStructureSeeds::default(),
            StructureSpawnAuthority::unavailable("fixture deliberately supplies no dispatcher"),
        )
        .unwrap()
    }

    #[test]
    fn starts_available_publishes_valid_reference_counts_and_preserves_prior_cache_on_error() {
        let generator = &*GENERATOR;
        let mut runtime = runtime(generator);
        let pos = ChunkPos::ZERO;
        let key = Identifier::parse("minecraft:buried_treasure");
        let mut storage = HashMap::from([(pos, generator.create_holder(pos).into_state())]);
        storage.set_start(
            pos,
            key.clone(),
            StructureStart::new(
                StructureDefinition::BuriedTreasure {
                    registry_id: key.to_string(),
                    terrain_adjustment: TerrainAdjustment::None,
                },
                pos,
                5,
                StructureDefinition::buried_treasure_pieces(pos),
            )
            .into(),
        );
        storage.set_start(
            pos,
            Identifier::parse("minecraft:mineshaft"),
            StructureStart::invalid().into(),
        );
        runtime.on_starts_available(&storage, pos).unwrap();
        assert_eq!(
            runtime.loaded_starts(pos),
            Some(&HashMap::from([(key.clone(), 5)]))
        );
        assert_eq!(
            storage[&pos].status(),
            ChunkStatus::Empty,
            "notification never certifies a generation rung"
        );
        storage.set_start(
            pos,
            key.clone(),
            rivet_world::chunk::structure_access::StructureStartEntry::deferred(
                rivet_nbt::tag::Tag::Int(rivet_nbt::int_tag::IntTag::value_of(7)),
            ),
        );
        assert!(matches!(
            runtime.on_starts_available(&storage, pos),
            Err(StructureGenerationError::Storage(
                StructureManagerError::StartUnavailable { .. }
            ))
        ));
        assert_eq!(runtime.loaded_starts(pos).unwrap()[&key], 5);
    }

    #[test]
    fn owner_executes_real_empty_selection_without_requiring_an_unneeded_event() {
        let generator = &*GENERATOR;
        let mut runtime = runtime(generator);
        let keys: Vec<_> = runtime
            .state
            .possible_structure_sets()
            .iter()
            .map(|set| set.key.clone())
            .collect();
        let mut empty = None;
        'search: for x in 0..16 {
            for z in 0..16 {
                let mut candidate = false;
                for key in &keys {
                    candidate |= runtime.state.is_structure_chunk(key, x, z).unwrap();
                }
                if !candidate {
                    empty = Some(ChunkPos::new(x, z));
                    break 'search;
                }
            }
        }
        let pos = empty.expect("real placement predicates have a chunk with no attempted starts");
        let mut owner = GeneratedChunkOwner::new(Arc::clone(generator));
        assert!(owner.install_structure_runtime(runtime).is_ok());
        let consumer = owner.new_consumer();
        owner.replace_requests(consumer, [(pos, ChunkStatus::StructureStarts)]);
        let report = owner.run_jobs(2);
        assert_eq!(report.failed, 0);
        assert!(matches!(
            owner.readiness(pos, ChunkStatus::StructureStarts),
            GenerationReadiness::Ready
        ));
        assert_eq!(
            owner.state(pos).unwrap().status(),
            ChunkStatus::StructureStarts
        );
        assert!(
            owner
                .structure_runtime()
                .unwrap()
                .loaded_starts(pos)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn runtime_admission_preserves_the_rejected_world_authority() {
        let original = &*GENERATOR;
        let runtime = runtime(original);
        let mut other_owner = GeneratedChunkOwner::new(Arc::new(OverworldGenerator::new(42)));
        let rejected = match other_owner.install_structure_runtime(runtime) {
            Err(runtime) => runtime,
            Ok(()) => panic!("same seed is not the same world"),
        };
        assert!(rejected.belongs_to(original));
        assert!(other_owner.structure_runtime().is_none());
    }
}

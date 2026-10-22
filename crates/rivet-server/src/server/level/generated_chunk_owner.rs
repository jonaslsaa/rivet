//! Canonical tick-thread generation ownership. Dependencies come exclusively
//! from the actual status pyramid; unavailable bodies are never pass-throughs.
use super::generated_world::{
    FeatureWorkspace, FeatureWritebacks, GeneratedChunkError, GenerationChunkHolder,
    GenerationChunkLoan, GenerationChunkState, OverworldGenerator, SpawnRegionProtos,
};
use crate::server::lighting::{GeneratedLightBridge, GeneratedLightWorkspace};
use rivet_registry::core::ChunkPos;
use rivet_world::chunk::status::{ChunkStatus, chunk_pyramid::GENERATION_PYRAMID};
use rivet_world::level::WorldBorderSettings;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GenerationConsumerId(u64);

#[derive(Debug)]
pub enum GenerationOwnerError {
    StructureGenerationUnavailable { status: ChunkStatus },
    Generation(GeneratedChunkError),
    StructureTerrain(rivet_world::levelgen::noise::beardifier::BeardifierError),
    StructureGeneration(super::structure_generation::StructureGenerationError),
    RuntimeUnavailable { status: ChunkStatus },
    LightProjection(crate::server::lighting::GeneratedLightBridgeError),
    LightWorkspace(crate::server::lighting::GeneratedLightWorkspaceError),
}
impl std::fmt::Display for GenerationOwnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StructureGenerationUnavailable { status } => write!(
                f,
                "actual structure generation is unavailable for {status:?}"
            ),
            Self::Generation(error) => write!(f, "{error}"),
            Self::StructureTerrain(error) => write!(f, "{error}"),
            Self::StructureGeneration(error) => write!(f, "{error}"),
            Self::RuntimeUnavailable { status } => {
                write!(f, "world runtime settings unavailable for {status:?}")
            }
            Self::LightProjection(error) => write!(f, "generated lighting: {error}"),
            Self::LightWorkspace(error) => write!(f, "generated lighting workspace: {error:?}"),
        }
    }
}

#[derive(Debug)]
pub enum GenerationReadiness<'a> {
    Ready,
    Pending,
    Failed {
        pos: ChunkPos,
        status: ChunkStatus,
        error: &'a GenerationOwnerError,
    },
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct GenerationJobReport {
    pub attempted: usize,
    pub advanced: usize,
    pub failed: usize,
}

pub struct GenerationAdmissionError {
    pub state: GenerationChunkState,
}

struct Entry {
    state: GenerationChunkState,
    failure: Option<(ChunkStatus, GenerationOwnerError)>,
}

/// The owner retains values after cancellation. Eviction/save acknowledgement
/// is intentionally not conflated with removing a consumer's demand.
pub struct GeneratedChunkOwner {
    generator: Arc<OverworldGenerator>,
    next_consumer: u64,
    consumers: HashMap<GenerationConsumerId, HashMap<ChunkPos, ChunkStatus>>,
    required: HashMap<ChunkPos, ChunkStatus>,
    chunks: HashMap<ChunkPos, Entry>,
    structure_runtime: Option<super::structure_generation::StructureGenerationRuntime>,
    structure_placement: Option<Arc<super::structure_placement_adapter::StructurePlacementRuntime>>,
    light_channels: Option<(bool, bool)>,
    spawn_settings: Option<(WorldBorderSettings, bool)>,
}

impl GeneratedChunkOwner {
    pub fn new(generator: Arc<OverworldGenerator>) -> Self {
        Self {
            generator,
            next_consumer: 0,
            consumers: HashMap::new(),
            required: HashMap::new(),
            chunks: HashMap::new(),
            structure_runtime: None,
            structure_placement: None,
            light_channels: None,
            spawn_settings: None,
        }
    }
    pub fn generator(&self) -> &Arc<OverworldGenerator> {
        &self.generator
    }
    /// Supply the actual dimension's enabled light channels. Neither channel
    /// is inferred from the generator or from the presence of a workspace.
    pub fn set_light_channels(&mut self, has_sky_light: bool, has_block_light: bool) {
        let was_missing = self.light_channels.is_none();
        self.light_channels = Some((has_sky_light, has_block_light));
        if was_missing {
            self.retry_runtime_settings();
        }
    }

    /// Refresh from the world's current border and SPAWN_MOBS gamerule before
    /// running jobs. No default border or disabled-mob fallback is installed.
    pub fn set_spawn_settings(&mut self, border: WorldBorderSettings, spawn_mobs: bool) {
        let was_missing = self.spawn_settings.is_none();
        self.spawn_settings = Some((border.current_bounds_snapshot(), spawn_mobs));
        if was_missing {
            self.retry_runtime_settings();
        }
    }

    fn retry_runtime_settings(&mut self) {
        for entry in self.chunks.values_mut() {
            if matches!(
                entry.failure,
                Some((_, GenerationOwnerError::RuntimeUnavailable { .. }))
            ) {
                entry.failure = None;
            }
        }
    }
    pub fn install_structure_placement(
        &mut self,
        runtime: Arc<super::structure_placement_adapter::StructurePlacementRuntime>,
    ) -> Result<(), Arc<super::structure_placement_adapter::StructurePlacementRuntime>> {
        if !runtime.belongs_to(&self.generator) || self.structure_placement.is_some() {
            return Err(runtime);
        }
        self.structure_placement = Some(runtime);
        for entry in self.chunks.values_mut() {
            if matches!(entry.failure, Some((_, GenerationOwnerError::Generation(GeneratedChunkError::Generation(
                rivet_world::chunk::status::world_gen_context::GenError::StructureDecorationIndexUnavailable { .. }
            ))))) {
                entry.failure = None;
            }
        }
        Ok(())
    }
    /// Installation is explicit and cannot transfer authority across worlds.
    /// On rejection the caller retains the complete runtime and its caches.
    pub fn install_structure_runtime(
        &mut self,
        runtime: super::structure_generation::StructureGenerationRuntime,
    ) -> Result<(), Box<super::structure_generation::StructureGenerationRuntime>> {
        if !runtime.belongs_to(&self.generator) || self.structure_runtime.is_some() {
            return Err(Box::new(runtime));
        }
        self.structure_runtime = Some(runtime);
        // Previously unavailable tasks can now be attempted with real authority.
        for entry in self.chunks.values_mut() {
            if matches!(
                entry.failure,
                Some((
                    _,
                    GenerationOwnerError::StructureGenerationUnavailable { .. }
                ))
            ) {
                entry.failure = None;
            }
        }
        Ok(())
    }
    pub fn structure_runtime(
        &self,
    ) -> Option<&super::structure_generation::StructureGenerationRuntime> {
        self.structure_runtime.as_ref()
    }
    pub fn new_consumer(&mut self) -> GenerationConsumerId {
        let id = GenerationConsumerId(self.next_consumer);
        self.next_consumer = self
            .next_consumer
            .checked_add(1)
            .expect("generation consumer ID exhaustion");
        self.consumers.insert(id, HashMap::new());
        id
    }
    pub fn replace_requests(
        &mut self,
        id: GenerationConsumerId,
        requests: impl IntoIterator<Item = (ChunkPos, ChunkStatus)>,
    ) -> bool {
        let Some(consumer) = self.consumers.get_mut(&id) else {
            return false;
        };
        consumer.clear();
        for (pos, target) in requests {
            consumer
                .entry(pos)
                .and_modify(|old| *old = ChunkStatus::max(*old, target))
                .or_insert(target);
        }
        self.rebuild_requirements();
        true
    }
    pub fn release_consumer(&mut self, id: GenerationConsumerId) -> bool {
        let removed = self.consumers.remove(&id).is_some();
        if removed {
            self.rebuild_requirements();
        }
        removed
    }
    fn rebuild_requirements(&mut self) {
        self.required.clear();
        for consumer in self.consumers.values() {
            for (&pos, &target) in consumer {
                for (dependency, status) in dependencies(pos, target, true) {
                    self.required
                        .entry(dependency)
                        .and_modify(|old| *old = ChunkStatus::max(*old, status))
                        .or_insert(status);
                }
                self.required
                    .entry(pos)
                    .and_modify(|old| *old = ChunkStatus::max(*old, target))
                    .or_insert(target);
            }
        }
    }
    pub fn requested_status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
        self.required.get(&pos).copied()
    }
    pub fn state(&self, pos: ChunkPos) -> Option<&GenerationChunkState> {
        self.chunks.get(&pos).map(|entry| &entry.state)
    }
    pub(super) fn state_mut(&mut self, pos: ChunkPos) -> Option<&mut GenerationChunkState> {
        self.chunks.get_mut(&pos).map(|entry| &mut entry.state)
    }
    pub fn len(&self) -> usize {
        self.chunks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
    fn status(&self, pos: ChunkPos) -> ChunkStatus {
        self.state(pos)
            .map_or(ChunkStatus::Empty, GenerationChunkState::status)
    }

    /// Adopt an ungenerated same-world holder state. Older standalone holders
    /// can carry provisional structure statuses, so advanced-state admission
    /// stays closed until a genuine completed-structure producer is attached.
    pub fn admit(
        &mut self,
        state: GenerationChunkState,
    ) -> Result<(), Box<GenerationAdmissionError>> {
        if !state.belongs_to(&self.generator)
            || self.chunks.contains_key(&state.pos())
            || state.has_retained_dependencies()
            || state.has_retained_light_storage()
            || state.has_structure_capability()
            || state.status() != ChunkStatus::Empty
        {
            return Err(Box::new(GenerationAdmissionError { state }));
        }
        self.chunks.insert(
            state.pos(),
            Entry {
                state,
                failure: None,
            },
        );
        Ok(())
    }
    pub fn readiness(&self, pos: ChunkPos, target: ChunkStatus) -> GenerationReadiness<'_> {
        if self
            .state(pos)
            .is_some_and(|state| state.status().is_or_after(target))
        {
            return GenerationReadiness::Ready;
        }
        let mut required = dependencies(pos, target, true);
        required.push((pos, target));
        for (dependency, status) in required {
            if let Some(entry) = self.chunks.get(&dependency)
                && let Some((failed_status, error)) = &entry.failure
                && status.is_or_after(*failed_status)
            {
                return GenerationReadiness::Failed {
                    pos: dependency,
                    status: *failed_status,
                    error,
                };
            }
        }
        GenerationReadiness::Pending
    }
    /// Explicit retry after an external capability change; canonical data and
    /// the holder's own terminal FEATURES failure cache remain intact.
    pub fn retry(&mut self, pos: ChunkPos) -> bool {
        self.chunks
            .get_mut(&pos)
            .is_some_and(|entry| entry.failure.take().is_some())
    }
    pub fn run_jobs(&mut self, max_jobs: usize) -> GenerationJobReport {
        let mut report = GenerationJobReport::default();
        while report.attempted < max_jobs {
            let mut ready: Vec<_> = self
                .required
                .iter()
                .filter_map(|(&pos, &target)| {
                    let current = self.status(pos);
                    if (current.is_or_after(target)
                        && (self.chunks.contains_key(&pos) || target != ChunkStatus::Empty))
                        || self
                            .chunks
                            .get(&pos)
                            .is_some_and(|entry| entry.failure.is_some())
                    {
                        return None;
                    }
                    let next = if target == ChunkStatus::Empty {
                        ChunkStatus::Empty
                    } else {
                        ChunkStatus::ALL[current.index() + 1]
                    };
                    dependencies(pos, next, false)
                        .iter()
                        .all(|&(neighbor, status)| self.status(neighbor).is_or_after(status))
                        .then_some((next, pos))
                })
                .collect();
            ready.sort_by_key(|(status, pos)| (status.index(), pos.x(), pos.z()));
            let Some((next, pos)) = ready.first().copied() else {
                break;
            };
            report.attempted += 1;
            let entry = self.chunks.entry(pos).or_insert_with(|| Entry {
                state: self.generator.create_holder(pos).into_state(),
                failure: None,
            });
            // Existing executor structure stubs are not generation capability.
            if matches!(
                next,
                ChunkStatus::StructureStarts | ChunkStatus::StructureReferences
            ) {
                let Some(mut runtime) = self.structure_runtime.take() else {
                    entry.failure = Some((
                        next,
                        GenerationOwnerError::StructureGenerationUnavailable { status: next },
                    ));
                    report.failed += 1;
                    continue;
                };
                let height = entry.state.chunk().height_accessor();
                // Retain per-world caches/authority even on an unexpected
                // implementation panic, just as canonical chunks are retained.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if next == ChunkStatus::StructureStarts {
                        runtime.generate_starts(self, pos, &height)
                    } else {
                        super::structure_manager::StructureManager::new(
                            self,
                            &super::structure_manager::UnambiguousStructureOrder,
                        )
                        .create_references_commutative(pos)
                        .map_err(super::structure_generation::StructureGenerationError::from)
                    }
                }));
                self.structure_runtime = Some(runtime);
                match result {
                    Ok(Ok(())) => {
                        self.chunks
                            .get_mut(&pos)
                            .expect("canonical task center")
                            .state
                            .chunk_mut()
                            .set_persisted_status(next);
                        report.advanced += 1;
                    }
                    Ok(Err(error)) => {
                        self.chunks
                            .get_mut(&pos)
                            .expect("canonical task center")
                            .failure =
                            Some((next, GenerationOwnerError::StructureGeneration(error)));
                        report.failed += 1;
                    }
                    Err(payload) => std::panic::resume_unwind(payload),
                }
                continue;
            }
            let terrain = if matches!(
                next,
                ChunkStatus::Biomes
                    | ChunkStatus::Noise
                    | ChunkStatus::Surface
                    | ChunkStatus::Carvers
            ) {
                if let Some(terrain) = self
                    .state(pos)
                    .and_then(GenerationChunkState::cached_structure_terrain)
                {
                    Some(Arc::clone(terrain))
                } else {
                    use super::structure_manager::{
                        StructureBeardifierSource, StructureManager, UnambiguousStructureOrder,
                    };
                    use rivet_world::levelgen::noise::beardifier::Beardifier;
                    let access = self.generator.registry_access();
                    let manager = StructureManager::new(self, &UnambiguousStructureOrder);
                    match Beardifier::for_structures_in_chunk(
                        &StructureBeardifierSource {
                            manager: &manager,
                            access,
                        },
                        pos,
                    ) {
                        Ok(terrain) => Some(Arc::new(terrain)),
                        Err(error) => {
                            self.chunks
                                .get_mut(&pos)
                                .expect("selected canonical chunk")
                                .failure =
                                Some((next, GenerationOwnerError::StructureTerrain(error)));
                            report.failed += 1;
                            continue;
                        }
                    }
                }
            } else {
                None
            };
            if (matches!(next, ChunkStatus::InitializeLight | ChunkStatus::Light)
                && self.light_channels.is_none())
                || (next == ChunkStatus::Spawn && self.spawn_settings.is_none())
            {
                self.chunks.get_mut(&pos).expect("canonical center").failure = Some((
                    next,
                    GenerationOwnerError::RuntimeUnavailable { status: next },
                ));
                report.failed += 1;
                continue;
            }
            // These are disposable value projections, not additional chunk
            // owners. Only light buffers may be returned to the canonical
            // protos; sections, status, entities and failures never come back
            // from a provider projection.
            let mut light_projections = HashMap::new();
            if matches!(next, ChunkStatus::InitializeLight | ChunkStatus::Light) {
                let projection_result = self.chunks.iter().try_for_each(|(&neighbor, entry)| {
                    if neighbor != pos
                        && pos.get_chessboard_distance(&neighbor) <= 2
                        && entry.state.status().is_or_after(ChunkStatus::Light)
                        && entry.state.chunk().base().is_light_correct()
                    {
                        let runtime =
                            GeneratedLightBridge::runtime_from_generated(entry.state.chunk())
                                .map_err(GenerationOwnerError::LightProjection)?;
                        let (base, _) = runtime.into_base_and_entities();
                        light_projections.insert((neighbor.x(), neighbor.z()), base);
                    }
                    Ok::<(), GenerationOwnerError>(())
                });
                if let Err(error) = projection_result {
                    self.chunks.get_mut(&pos).expect("canonical center").failure =
                        Some((next, error));
                    report.failed += 1;
                    continue;
                }
            }
            let light_workspace =
                if matches!(next, ChunkStatus::InitializeLight | ChunkStatus::Light) {
                    let (sky, block) = self.light_channels.expect("validated dimension channels");
                    match GeneratedLightWorkspace::new(
                        self.chunks[&pos].state.chunk().height_accessor(),
                        sky,
                        block,
                        pos,
                        &mut light_projections,
                    ) {
                        Ok(workspace) => Some(workspace),
                        Err(error) => {
                            self.chunks.get_mut(&pos).expect("canonical center").failure =
                                Some((next, GenerationOwnerError::LightWorkspace(error)));
                            report.failed += 1;
                            continue;
                        }
                    }
                } else {
                    None
                };
            let entry = self.chunks.remove(&pos).expect("selected canonical chunk");
            let workspace = FeatureWorkspace::new();
            let mut loans = HashMap::new();
            if next == ChunkStatus::Features {
                for (neighbor, _) in dependencies(pos, next, true) {
                    let entry = self
                        .chunks
                        .remove(&neighbor)
                        .expect("ready canonical FEATURES dependency");
                    let (chunk, metadata) = entry.state.into_loan();
                    workspace.insert(chunk);
                    loans.insert(neighbor, (metadata, entry.failure));
                }
            }
            let mut spawn_workspace = if next == ChunkStatus::Spawn {
                let mut neighbors = Vec::with_capacity(8);
                // The scheduler has already proved the exact radius-one
                // BIOMES prerequisite. Preserve the full proto, not its base.
                for (neighbor, _) in dependencies(pos, next, false) {
                    let entry = self
                        .chunks
                        .remove(&neighbor)
                        .expect("ready SPAWN dependency");
                    let (chunk, metadata) = entry.state.into_loan();
                    neighbors.push(chunk);
                    loans.insert(neighbor, (metadata, entry.failure));
                }
                Some(
                    SpawnRegionProtos::new_with_world_border_settings(
                        pos,
                        neighbors,
                        self.spawn_settings.expect("validated SPAWN settings").0,
                    )
                    .expect("canonical exact radius-one neighbors"),
                )
            } else {
                None
            };
            let (mut holder, light) = match GenerationChunkHolder::from_state(
                entry.state,
                Arc::clone(&self.generator),
                workspace,
                None,
            ) {
                Ok(result) => result,
                Err(error) => {
                    if let Some(workspace) = spawn_workspace.take() {
                        let chunks = workspace
                            .into_neighbours()
                            .into_iter()
                            .map(|chunk| (chunk.get_pos(), chunk))
                            .collect();
                        self.return_loans(chunks, loans);
                    } else {
                        self.return_loans(error.workspace.take_all(), loans);
                    }
                    self.chunks.insert(
                        pos,
                        Entry {
                            state: error.state,
                            failure: entry.failure,
                        },
                    );
                    panic!(
                        "canonical generation owner produced inconsistent resume state: {:?}",
                        error.reason
                    );
                }
            };
            if let Some(runtime) = &self.structure_placement {
                assert!(
                    holder
                        .install_structure_placement(Arc::clone(runtime))
                        .is_ok(),
                    "canonical owner retains the world's placement runtime"
                );
            }
            assert!(
                light.is_none(),
                "canonical owner never retains parallel light storage"
            );
            if let Some(workspace) = light_workspace {
                assert!(holder.attach_generated_light_workspace(workspace).is_none());
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(workspace) = spawn_workspace.as_mut() {
                    holder.generate_spawn_with_region_rule(
                        workspace,
                        self.spawn_settings.expect("validated SPAWN settings").1,
                    )
                } else if let Some(terrain) = terrain {
                    holder.generate_through_with_structure_terrain(next, terrain)
                } else {
                    holder.generate_through(next)
                }
            }));
            if let Some(storage) = holder.take_generated_light_storage() {
                for ((x, z), projection) in storage {
                    let canonical = self
                        .chunks
                        .get_mut(&ChunkPos::new(x, z))
                        .expect("lighting projection still has its canonical owner")
                        .state
                        .chunk_mut()
                        .base_mut();
                    canonical.set_block_nibbles(projection.block_nibbles().to_vec());
                    canonical.set_sky_nibbles(projection.sky_nibbles().to_vec());
                    canonical.set_block_emptiness_map(
                        projection.block_emptiness_map().map(<[bool]>::to_vec),
                    );
                    canonical.set_sky_emptiness_map(
                        projection.sky_emptiness_map().map(<[bool]>::to_vec),
                    );
                }
            }
            let mut state = holder.into_state();
            if let Some(workspace) = spawn_workspace {
                let chunks = workspace
                    .into_neighbours()
                    .into_iter()
                    .map(|chunk| (chunk.get_pos(), chunk))
                    .collect();
                self.return_loans(chunks, loans);
            } else {
                self.return_loans(state.take_feature_dependencies(), loans);
            }
            match result {
                Ok(Ok(())) => {
                    self.chunks.insert(
                        pos,
                        Entry {
                            state,
                            failure: None,
                        },
                    );
                    report.advanced += 1;
                }
                Ok(Err(error)) => {
                    self.chunks.insert(
                        pos,
                        Entry {
                            state,
                            failure: Some((next, GenerationOwnerError::Generation(error))),
                        },
                    );
                    report.failed += 1;
                }
                Err(payload) => {
                    self.chunks.insert(
                        pos,
                        Entry {
                            state,
                            failure: entry.failure,
                        },
                    );
                    std::panic::resume_unwind(payload);
                }
            }
        }
        report
    }

    fn return_loans(
        &mut self,
        chunks: FeatureWritebacks,
        mut loans: HashMap<
            ChunkPos,
            (
                GenerationChunkLoan,
                Option<(ChunkStatus, GenerationOwnerError)>,
            ),
        >,
    ) {
        for (pos, chunk) in chunks {
            let (metadata, failure) = loans
                .remove(&pos)
                .expect("returned dependency belongs to canonical loan");
            let state = match metadata.rejoin(chunk) {
                Ok(state) => state,
                Err(_) => unreachable!("region cannot change a chunk coordinate"),
            };
            assert!(
                self.chunks.insert(pos, Entry { state, failure }).is_none(),
                "loaned chunk cannot have another canonical owner"
            );
        }
        assert!(
            loans.is_empty(),
            "region must return every canonical dependency"
        );
    }
}

fn dependencies(
    pos: ChunkPos,
    target: ChunkStatus,
    accumulated: bool,
) -> Vec<(ChunkPos, ChunkStatus)> {
    let step = GENERATION_PYRAMID.get_step_to(target);
    let deps = if accumulated {
        step.accumulated_dependencies()
    } else {
        step.direct_dependencies()
    };
    let radius = deps.size() as i32 - 1;
    let mut result = Vec::new();
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx == 0 && dz == 0 {
                continue;
            }
            result.push((
                ChunkPos::new(pos.x().wrapping_add(dx), pos.z().wrapping_add(dz)),
                deps.get(dx.abs().max(dz.abs()) as usize),
            ));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn generator() -> Arc<OverworldGenerator> {
        static GENERATOR: std::sync::LazyLock<Arc<OverworldGenerator>> =
            std::sync::LazyLock::new(|| Arc::new(OverworldGenerator::new(42)));
        Arc::clone(&GENERATOR)
    }
    #[test]
    fn empty_job_is_real_owned_progress_and_structure_refusal_keeps_actual_status() {
        fn assert_send<T: Send>() {}
        assert_send::<GeneratedChunkOwner>();
        let mut owner = GeneratedChunkOwner::new(generator());
        let consumer = owner.new_consumer();
        owner.replace_requests(consumer, [(ChunkPos::ZERO, ChunkStatus::Empty)]);
        assert!(matches!(
            owner.readiness(ChunkPos::ZERO, ChunkStatus::Empty),
            GenerationReadiness::Pending
        ));
        assert_eq!(owner.run_jobs(0).attempted, 0);
        assert!(owner.is_empty());
        assert_eq!(
            owner.run_jobs(1),
            GenerationJobReport {
                attempted: 1,
                advanced: 1,
                failed: 0
            }
        );
        assert!(matches!(
            owner.readiness(ChunkPos::ZERO, ChunkStatus::Empty),
            GenerationReadiness::Ready
        ));
        owner.replace_requests(consumer, [(ChunkPos::ZERO, ChunkStatus::StructureStarts)]);
        assert_eq!(
            owner.run_jobs(1),
            GenerationJobReport {
                attempted: 1,
                advanced: 0,
                failed: 1
            }
        );
        assert_eq!(
            owner.state(ChunkPos::ZERO).unwrap().status(),
            ChunkStatus::Empty
        );
        assert!(matches!(
            owner.readiness(ChunkPos::ZERO, ChunkStatus::StructureStarts),
            GenerationReadiness::Failed {
                status: ChunkStatus::StructureStarts,
                error: GenerationOwnerError::StructureGenerationUnavailable { .. },
                ..
            }
        ));
        assert_eq!(owner.run_jobs(5).attempted, 0, "failure must not hot-loop");
        assert!(owner.retry(ChunkPos::ZERO));
        assert_eq!(
            owner.run_jobs(1).failed,
            1,
            "retry cannot fabricate new capability"
        );
    }
    #[test]
    fn overlapping_consumers_merge_exact_pyramid_requirements_and_cancel_independently() {
        let mut owner = GeneratedChunkOwner::new(generator());
        let first = owner.new_consumer();
        let second = owner.new_consumer();
        owner.replace_requests(first, [(ChunkPos::ZERO, ChunkStatus::Features)]);
        owner.replace_requests(second, [(ChunkPos::ZERO, ChunkStatus::Carvers)]);
        assert_eq!(
            owner.requested_status(ChunkPos::ZERO),
            Some(ChunkStatus::Features)
        );
        assert_eq!(
            owner.requested_status(ChunkPos::new(1, 0)),
            Some(ChunkStatus::Carvers)
        );
        assert_eq!(
            owner.requested_status(ChunkPos::new(8, 0)),
            Some(ChunkStatus::StructureStarts)
        );
        let report = owner.run_jobs(2);
        assert_eq!(report.attempted, 2);
        assert_eq!(report.failed, 2);
        let retained = owner.len();
        assert!(owner.release_consumer(first));
        assert_eq!(
            owner.requested_status(ChunkPos::ZERO),
            Some(ChunkStatus::Carvers)
        );
        assert!(owner.release_consumer(second));
        assert_eq!(owner.requested_status(ChunkPos::ZERO), None);
        assert_eq!(owner.run_jobs(10).attempted, 0);
        assert_eq!(
            owner.len(),
            retained,
            "cancellation is not unsaved eviction"
        );
        assert!(!owner.replace_requests(first, [(ChunkPos::ZERO, ChunkStatus::Full)]));
    }
    #[test]
    fn admission_refuses_provisional_advanced_status_without_losing_state() {
        let generator = generator();
        let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
        // A hostile caller cannot turn a status label into verified structure
        // generation authority through the owner admission API.
        state.chunk_mut().set_persisted_status(ChunkStatus::Carvers);
        let mut owner = GeneratedChunkOwner::new(generator);
        let error = owner.admit(state).expect_err("unverified advanced state");
        assert_eq!(error.state.status(), ChunkStatus::Carvers);
        assert!(owner.is_empty());
    }

    #[test]
    fn admission_refuses_structure_capability_and_returns_original_payload() {
        use rivet_world::block::blocks::Blocks;
        let generator = generator();
        let mut state = generator
            .create_holder_with_structure_feature_count(ChunkPos::ZERO, Some(0))
            .into_state();
        state
            .chunk_mut()
            .set_block_state(0, 64, 0, Blocks::STONE.default_block_state());
        let neighbor = generator.create_holder(ChunkPos::new(1, 0)).into_state();
        let runtime =
            crate::server::lighting::GeneratedLightBridge::runtime_from_generated(neighbor.chunk())
                .expect("real neighbour value conversion");
        let (runtime, _) = runtime.into_base_and_entities();
        state.restore_generated_light_storage(HashMap::from([((1, 0), runtime)]));
        let mut owner = GeneratedChunkOwner::new(Arc::clone(&generator));
        let error = owner
            .admit(state)
            .expect_err("owner cannot resume caller-supplied structure capability");
        assert!(owner.is_empty());
        assert_eq!(
            error.state.chunk().get_block_state(0, 64, 0),
            Blocks::STONE.default_block_state()
        );
        let (holder, light) = GenerationChunkHolder::from_state(
            error.state,
            generator,
            FeatureWorkspace::new(),
            Some(0),
        )
        .expect("original capability remains recoverable");
        assert_eq!(holder.into_state().status(), ChunkStatus::Empty);
        let light = light.expect("admission failure must retain light ownership");
        assert_eq!(light.len(), 1);
        assert_eq!(light[&(1, 0)].get_pos(), ChunkPos::new(1, 0));
    }

    #[test]
    fn admission_returns_foreign_light_storage_without_creating_parallel_owners() {
        let generator = generator();
        let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
        // A detached task may legitimately own an empty fresh-light window.
        state.restore_generated_light_storage(HashMap::new());
        let mut owner = GeneratedChunkOwner::new(generator);
        let mut error = owner
            .admit(state)
            .expect_err("foreign provider must not shadow canonical neighbors");
        assert!(owner.is_empty());
        assert!(error.state.take_generated_light_storage().is_some());
    }

    fn insert_task_fixture(owner: &mut GeneratedChunkOwner, pos: ChunkPos, status: ChunkStatus) {
        // Isolate the downstream task under test. These completed prerequisite
        // labels are never admitted through the production owner API.
        let mut state = owner.generator.create_holder(pos).into_state();
        state.chunk_mut().set_persisted_status(status);
        owner.chunks.insert(
            pos,
            Entry {
                state,
                failure: None,
            },
        );
    }

    #[test]
    fn lighting_jobs_require_dimension_authority_then_compute_real_fresh_light() {
        let mut owner = GeneratedChunkOwner::new(generator());
        let center = ChunkPos::ZERO;
        insert_task_fixture(&mut owner, center, ChunkStatus::Features);
        owner.required.insert(center, ChunkStatus::Light);
        assert_eq!(owner.run_jobs(1).failed, 1);
        assert_eq!(owner.status(center), ChunkStatus::Features);
        assert!(!owner.state(center).unwrap().chunk().has_light_engine());

        owner.set_light_channels(true, true);
        assert_eq!(owner.run_jobs(1).advanced, 1);
        let center_chunk = owner.state(center).unwrap().chunk();
        assert!(center_chunk.has_light_engine());
        assert!(
            !center_chunk.base().is_light_correct(),
            "INITIALIZE_LIGHT does not fabricate computed light"
        );
        // The actual LIGHT rung requires eight initialized neighbors, not a
        // fabricated pre-lit ring. They remain unchanged when ineligible for
        // Starlight's relaxed neighbor cache.
        for (pos, status) in dependencies(center, ChunkStatus::Light, false) {
            insert_task_fixture(&mut owner, pos, status);
        }
        let neighbor = ChunkPos::new(2, 0);
        insert_task_fixture(&mut owner, neighbor, ChunkStatus::Light);
        let neighbor_chunk = owner.chunks.get_mut(&neighbor).unwrap().state.chunk_mut();
        neighbor_chunk.base_mut().set_light_correct(true);
        neighbor_chunk.add_entity(rivet_nbt::compound_tag::CompoundTag::new());
        neighbor_chunk.set_upgrading(ChunkStatus::Spawn);
        assert_eq!(owner.run_jobs(1).advanced, 1);
        let chunk = owner.state(center).unwrap().chunk();
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.base().is_light_correct());
        assert_eq!(chunk.base().get_sky_light_at(0, 64, 0), Some(15));
        let neighbor_chunk = owner.state(neighbor).unwrap().chunk();
        assert_eq!(neighbor_chunk.get_entities().len(), 1);
        assert!(neighbor_chunk.is_upgrading());
        assert_eq!(neighbor_chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(!owner.state(center).unwrap().has_retained_light_storage());
        assert_eq!(
            owner.status(ChunkPos::new(1, 0)),
            ChunkStatus::InitializeLight
        );
    }

    #[test]
    fn spawn_job_requires_actual_settings_and_returns_neighbor_owners() {
        let mut owner = GeneratedChunkOwner::new(generator());
        let center = ChunkPos::ZERO;
        insert_task_fixture(&mut owner, center, ChunkStatus::Light);
        for (pos, status) in dependencies(center, ChunkStatus::Spawn, false) {
            insert_task_fixture(&mut owner, pos, status);
            owner
                .chunks
                .get_mut(&pos)
                .unwrap()
                .state
                .chunk_mut()
                .add_entity(rivet_nbt::compound_tag::CompoundTag::new());
        }
        owner.required.insert(center, ChunkStatus::Spawn);
        assert_eq!(owner.run_jobs(1).failed, 1);
        assert_eq!(owner.status(center), ChunkStatus::Light);
        assert_eq!(owner.len(), 9);
        // This explicitly tests a world whose real gamerule is false; there
        // is no production default or successful-mob-construction claim.
        owner.set_spawn_settings(WorldBorderSettings::default_settings(), false);
        assert_eq!(owner.run_jobs(1).advanced, 1);
        assert_eq!(owner.status(center), ChunkStatus::Spawn);
        assert_eq!(owner.len(), 9);
        for (pos, status) in dependencies(center, ChunkStatus::Spawn, false) {
            let state = owner.state(pos).unwrap();
            assert_eq!(state.status(), status);
            assert_eq!(state.chunk().get_entities().len(), 1);
        }
        assert!(
            owner
                .state(center)
                .unwrap()
                .chunk()
                .get_entities()
                .is_empty()
        );
        assert_eq!(owner.run_jobs(1).attempted, 0, "SPAWN is not FULL");
    }

    #[test]
    fn spawn_true_preserves_all_owners_at_real_creature_construction_boundary() {
        use rivet_world::block::blocks::Blocks;
        use rivet_world::chunk::storage::section_reconstruction::BiomeId;
        use rivet_world::levelgen::heightmap::FINAL_HEIGHTMAPS;
        let generator = generator();
        let mut owner = GeneratedChunkOwner::new(Arc::clone(&generator));
        let center = ChunkPos::new(-8, -4);
        let mut fixtures = dependencies(center, ChunkStatus::Spawn, false);
        fixtures.push((center, ChunkStatus::Light));
        for (pos, status) in fixtures {
            insert_task_fixture(&mut owner, pos, status);
            let chunk = owner.chunks.get_mut(&pos).unwrap().state.chunk_mut();
            let source = generator.biome_source();
            // Same actual beach biome / seed-42 turtle case exercised by the
            // standalone SPAWN region test, now through canonical owner loans.
            chunk.fill_biomes_from_noise(source, source.sampler(), &|_| BiomeId(3));
            if pos == center {
                chunk.prime_heightmaps(&FINAL_HEIGHTMAPS);
                for x in 0..16 {
                    for z in 0..16 {
                        chunk.set_block_state(
                            pos.get_min_block_x() + x,
                            0,
                            pos.get_min_block_z() + z,
                            Blocks::SAND.default_block_state(),
                        );
                    }
                }
                let mut nibbles = chunk.base().sky_nibbles().to_vec();
                for nibble in &mut nibbles {
                    nibble.set_full();
                    nibble.update_visible();
                }
                chunk.base_mut().set_sky_nibbles(nibbles);
                chunk.base_mut().set_light_correct(true);
            } else {
                chunk.add_entity(rivet_nbt::compound_tag::CompoundTag::new());
            }
        }
        owner.set_spawn_settings(WorldBorderSettings::default_settings(), true);
        owner.required.insert(center, ChunkStatus::Spawn);
        assert_eq!(owner.run_jobs(1).failed, 1);
        assert!(matches!(
            owner.chunks[&center].failure,
            Some((
                ChunkStatus::Spawn,
                GenerationOwnerError::Generation(GeneratedChunkError::SpawnRegion(
                    super::super::generated_world::SpawnRegionError::UnsupportedEntity {
                        entity_type: "minecraft:turtle",
                        ..
                    }
                ))
            ))
        ));
        assert_eq!(owner.status(center), ChunkStatus::Light);
        assert_eq!(owner.len(), 9);
        for (pos, status) in dependencies(center, ChunkStatus::Spawn, false) {
            let state = owner.state(pos).unwrap();
            assert_eq!(state.status(), status);
            assert_eq!(state.chunk().get_entities().len(), 1);
        }
        assert!(
            owner
                .state(center)
                .unwrap()
                .chunk()
                .get_entities()
                .is_empty()
        );
    }

    #[test]
    fn first_noise_context_authority_refusal_preserves_canonical_chunk_before_biomes() {
        use rivet_nbt::compound_tag::CompoundTag;
        use rivet_registry::Identifier;
        use rivet_world::{block::blocks::Blocks, chunk::structure_access::StructureStartEntry};
        let generator = generator();
        let mut owner = GeneratedChunkOwner::new(Arc::clone(&generator));
        let center = ChunkPos::ZERO;
        // Explicit test setup supplies already-completed dependency statuses;
        // this is not a production admission or structure-generation path.
        for (pos, status) in dependencies(center, ChunkStatus::Biomes, false) {
            let mut state = generator.create_holder(pos).into_state();
            state.chunk_mut().set_persisted_status(status);
            owner.chunks.insert(
                pos,
                Entry {
                    state,
                    failure: None,
                },
            );
        }
        let mut state = generator.create_holder(center).into_state();
        state
            .chunk_mut()
            .set_persisted_status(ChunkStatus::StructureReferences);
        state
            .chunk_mut()
            .set_block_state(3, 200, 4, Blocks::STONE.default_block_state());
        let key = Identifier::parse("minecraft:village_plains");
        let mut tag = CompoundTag::new();
        tag.put_string("id", "minecraft:village_plains");
        state.chunk_mut().set_start_for_structure(
            key.clone(),
            StructureStartEntry::deferred(rivet_nbt::tag::Tag::Compound(tag.clone())),
        );
        state
            .chunk_mut()
            .add_reference_for_structure(key.clone(), center.pack() as u64);
        state.restore_generated_light_storage(HashMap::new());
        owner.chunks.insert(
            center,
            Entry {
                state,
                failure: None,
            },
        );
        owner.required.insert(center, ChunkStatus::Biomes);
        let snapshot = |state: &GenerationChunkState| {
            let sections = state
                .chunk()
                .get_sections()
                .iter()
                .map(|section| {
                    let mut blocks = Vec::with_capacity(4096);
                    for x in 0..16 {
                        for y in 0..16 {
                            for z in 0..16 {
                                blocks.push(section.get_block_state(x, y, z));
                            }
                        }
                    }
                    let mut biomes = Vec::with_capacity(64);
                    for x in 0..4 {
                        for y in 0..4 {
                            for z in 0..4 {
                                biomes.push(section.get_noise_biome(x, y, z));
                            }
                        }
                    }
                    (blocks, biomes)
                })
                .collect::<Vec<_>>();
            let heightmaps = state
                .chunk()
                .heightmaps()
                .iter()
                .map(|heightmap| heightmap.as_ref().map(|map| map.get_raw_data().to_vec()))
                .collect::<Vec<_>>();
            (sections, heightmaps)
        };
        let before = snapshot(owner.state(center).unwrap());
        let report = owner.run_jobs(1);
        assert_eq!(
            report,
            GenerationJobReport {
                attempted: 1,
                advanced: 0,
                failed: 1
            }
        );
        let retained = owner.state(center).unwrap();
        assert_eq!(retained.status(), ChunkStatus::StructureReferences);
        assert!(retained.cached_structure_terrain().is_none());
        assert_eq!(
            retained.chunk().get_block_state(3, 200, 4),
            Blocks::STONE.default_block_state()
        );
        assert_eq!(snapshot(retained), before);
        assert_eq!(
            retained
                .chunk()
                .get_start_for_structure(&key)
                .unwrap()
                .create_tag(center),
            rivet_nbt::tag::Tag::Compound(tag)
        );
        assert!(matches!(
            &owner.chunks[&center].failure,
            Some((
                ChunkStatus::Biomes,
                GenerationOwnerError::StructureTerrain(_)
            ))
        ));
        assert!(
            owner
                .chunks
                .get_mut(&center)
                .unwrap()
                .state
                .take_generated_light_storage()
                .is_some()
        );
    }
}

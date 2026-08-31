//! The bounded generated FULL serving workspace (Paper 26.2).
//!
//! The scheduler owns exactly the radius-four send view (117 positions) plus
//! the non-target radius-one support set (48 positions), for 165 holders
//! attached to the FULL dependency graph. Paper's farther accumulated radius-11
//! inputs remain an upstream ephemeral-generation responsibility.  Generation is deliberately synchronous at the
//! owner boundary: status waves are deterministic X-major/Z-minor walks and
//! only the final consuming SPAWN-to-FULL conversion publishes to `ChunkMap`.
//! No lower-status proto, fallback chunk, or packet is observable while a wave
//! or conversion is incomplete.

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use rivet_registry::core::ChunkPos;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, GENERATION_PYRAMID};

use crate::server::level::level_chunk::StructureKey;
use rivet_registry::block_state::BlockState;
use rivet_world::chunk::storage::section_reconstruction::BiomeId as WorldgenBiomeId;
use rivet_world::level::height_accessor::create as create_height_accessor;

use super::chunk_map::ChunkMap;
use super::chunk_tracking_view::ChunkTrackingView;
use super::generated_world::{
    FeatureWorkspace, GeneratedChunkError, GenerationChunkHolder, OverworldGenerator,
    SpawnRegionProtos,
};
use crate::server::lighting::{
    GENERATED_LIGHT_REQUIRED_RADIUS, GeneratedLightBridge, GeneratedLightStorage,
    GeneratedLightWorkspace, GeneratedLightWorkspaceError,
};

/// The radius of the exact player send view for this slice.
pub const GENERATED_VIEW_DISTANCE: i32 = 4;
/// The exact number of radius-four target chunks (`11 * 11 - 4`).
pub const GENERATED_TARGET_COUNT: usize = 117;
/// The accumulated FULL dependency radius in Paper's generation pyramid.
pub const FULL_SUPPORT_RADIUS: i32 = 11;
/// The exact number of non-target radius-one support holders.
pub const GENERATED_SUPPORT_COUNT: usize = 48;

/// One deterministic status wave.  The positions are sorted by x first, then
/// z, independently of holder-map insertion or any worker scheduling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedStatusWave {
    status: ChunkStatus,
    positions: Vec<ChunkPos>,
}

impl GeneratedStatusWave {
    /// The status this wave reaches.
    pub fn status(&self) -> ChunkStatus {
        self.status
    }

    /// The wave's canonical X-major/Z-minor positions.
    pub fn positions(&self) -> &[ChunkPos] {
        &self.positions
    }
}

/// Typed refusal from the bounded serving workspace.  A refusal never writes
/// to the caller's `ChunkMap`.
#[derive(Debug, thiserror::Error)]
pub enum GeneratedWorkspaceError {
    /// The view shape was not the exact Paper radius-four target set.  This is
    /// checked before constructing holders, so wrapped extreme coordinates fail
    /// closed rather than becoming an empty successful world.
    #[error(
        "generated workspace view centered at {center} with distance {view_distance} enumerated {actual_count} targets; expected {expected_count}"
    )]
    InvalidTargetView {
        center: ChunkPos,
        view_distance: i32,
        actual_count: usize,
        expected_count: usize,
    },
    /// A generated-world config does not describe this fixed normal-overworld
    /// slice.
    #[error("generated world config field {field} is incompatible; expected {expected}")]
    InvalidConfiguration {
        field: &'static str,
        expected: &'static str,
    },
    /// A required support holder was removed before its dependent operation.
    #[error("generated workspace target {target} is missing support holder {support}")]
    MissingSupport { target: ChunkPos, support: ChunkPos },
    /// A status task refused at a typed Paper boundary.
    #[error("generated workspace holder {position} refused status {target:?}: {source}")]
    Generation {
        position: ChunkPos,
        target: ChunkStatus,
        source: GeneratedChunkError,
    },
    /// A target failed the consuming SPAWN-to-FULL bridge.
    #[error("generated workspace target {position} refused consuming FULL conversion: {source}")]
    Conversion {
        position: ChunkPos,
        source: GeneratedChunkError,
    },
    /// The generated LIGHT task could not take ownership of its bounded
    /// runtime neighbour workspace. No holder is attached when this fails.
    #[error("generated workspace target {position} refused LIGHT workspace: {source}")]
    LightWorkspace {
        position: ChunkPos,
        #[source]
        source: GeneratedLightWorkspaceError,
    },
    /// A generated worldgen proto could not cross the temporary runtime LIGHT
    /// value boundary while recovering provider-owned neighbour writes.
    #[error("generated workspace LIGHT write-back at {position} refused: {message}")]
    LightWriteback { position: ChunkPos, message: String },
    /// The caller attempted to serve a generated view before all target chunks
    /// had been atomically installed.
    #[error("generated chunk {position} is not ready for packet serving (status: {status:?})")]
    PacketBeforeReady {
        position: ChunkPos,
        status: Option<ChunkStatus>,
    },
}

/// A finite generated serving graph.  Every mutable value is owned by the sync
/// tick thread; the immutable generator and FEATURES workspace are shared by
/// value through `Arc`/`Rc` internals, never through a game-state lock.
pub struct GeneratedWorkspace {
    seed: i64,
    view: ChunkTrackingView,
    generator: Arc<OverworldGenerator>,
    targets: Vec<ChunkPos>,
    support_positions: Vec<ChunkPos>,
    required_status: HashMap<ChunkPos, ChunkStatus>,
    waves: Vec<GeneratedStatusWave>,
    holders: HashMap<ChunkPos, GenerationChunkHolder>,
    feature_workspace: FeatureWorkspace,
    feature_workspace_seeded: bool,
}

impl GeneratedWorkspace {
    /// Construct the exact radius-four target set and its FULL dependency
    /// closure around `center`.
    pub fn new(seed: i64, center: ChunkPos) -> Result<Self, GeneratedWorkspaceError> {
        let view = ChunkTrackingView::of(center, GENERATED_VIEW_DISTANCE);
        let mut targets = Vec::with_capacity(view.chunk_count());
        view.for_each(|pos| targets.push(pos));
        if targets.len() != GENERATED_TARGET_COUNT {
            return Err(GeneratedWorkspaceError::InvalidTargetView {
                center,
                view_distance: GENERATED_VIEW_DISTANCE,
                actual_count: targets.len(),
                expected_count: GENERATED_TARGET_COUNT,
            });
        }

        let required_status = accumulate_full_support(&targets);
        let mut support_positions: Vec<_> = required_status.keys().copied().collect();
        sort_positions(&mut support_positions);
        let generator = Arc::new(OverworldGenerator::new(seed));
        let feature_workspace = FeatureWorkspace::new();
        let holders = support_positions
            .iter()
            .copied()
            .map(|pos| {
                (
                    pos,
                    generator.create_holder_with_workspace(pos, feature_workspace.clone()),
                )
            })
            .collect();
        let waves = build_status_waves(&support_positions, &required_status);

        Ok(Self {
            seed,
            view,
            generator,
            targets,
            support_positions,
            required_status,
            waves,
            holders,
            feature_workspace,
            feature_workspace_seeded: false,
        })
    }

    /// The seed captured by this workspace.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// The realized normal-overworld generator geometry.
    pub(crate) fn generator_geometry(&self) -> (i32, i32, i32) {
        (
            self.generator.get_min_y(),
            self.generator.get_gen_depth(),
            self.generator.get_sea_level(),
        )
    }

    /// The exact radius-four target view.
    pub fn view(&self) -> &ChunkTrackingView {
        &self.view
    }

    /// The 117 positions that may be installed and served.
    pub fn target_positions(&self) -> &[ChunkPos] {
        &self.targets
    }

    /// The exact 165 holders attached to the bounded FULL graph: 117 targets
    /// plus the 48 non-target radius-one supports, in canonical order.
    pub fn support_positions(&self) -> &[ChunkPos] {
        &self.support_positions
    }

    /// The number of attached FULL holders, always 165 for this slice.
    pub fn holder_count(&self) -> usize {
        self.holders.len()
    }

    /// The minimum status required at a support position by the FULL step.
    pub fn required_status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
        self.required_status.get(&pos).copied()
    }

    /// The deterministic executable waves. FULL is absent because FULL is the
    /// consuming holder-to-LevelChunk publication boundary.
    pub fn status_waves(&self) -> &[GeneratedStatusWave] {
        &self.waves
    }

    /// Whether every target is exactly at the SPAWN parent required by the
    /// consuming FULL bridge. This is the readiness predicate used before
    /// packet serving; support statuses are checked against their wave bounds.
    pub fn is_ready(&self) -> bool {
        self.targets.iter().all(|position| {
            self.holders
                .get(position)
                .is_some_and(|holder| holder.status() == ChunkStatus::Spawn)
        }) && self.required_status.iter().all(|(position, required)| {
            self.holders
                .get(position)
                .is_some_and(|holder| holder.status().is_or_after(*required))
        })
    }

    /// Inspect readiness without exposing lower-status chunks as serveable.
    pub fn require_ready(&self) -> Result<(), GeneratedWorkspaceError> {
        if let Some(position) = self.targets.iter().copied().find(|position| {
            self.holders
                .get(position)
                .is_none_or(|holder| holder.status() != ChunkStatus::Spawn)
        }) {
            return Err(GeneratedWorkspaceError::PacketBeforeReady {
                position,
                status: self
                    .holders
                    .get(&position)
                    .map(GenerationChunkHolder::status),
            });
        }
        if let Some(position) = self.support_positions.iter().copied().find(|position| {
            self.required_status.get(position).is_some_and(|required| {
                self.holders
                    .get(position)
                    .is_none_or(|holder| !holder.status().is_or_after(*required))
            })
        }) {
            return Err(GeneratedWorkspaceError::PacketBeforeReady {
                position,
                status: self
                    .holders
                    .get(&position)
                    .map(GenerationChunkHolder::status),
            });
        }
        Ok(())
    }

    /// Drain every required status in deterministic order.  This method only
    /// mutates tick-thread-owned holders; `ChunkMap` publication is separate.
    pub fn generate(&mut self) -> Result<(), GeneratedWorkspaceError> {
        for wave_index in 0..self.waves.len() {
            let status = self.waves[wave_index].status;
            let positions = self.waves[wave_index].positions.clone();
            for position in positions {
                let Some(holder) = self.holders.get(&position) else {
                    return Err(GeneratedWorkspaceError::MissingSupport {
                        target: position,
                        support: position,
                    });
                };
                if holder.status().is_or_after(status) {
                    continue;
                }

                match status {
                    ChunkStatus::Features => self.generate_features(position)?,
                    ChunkStatus::InitializeLight => self.generate_initialize_light(position)?,
                    ChunkStatus::Light => self.generate_light(position)?,
                    ChunkStatus::Spawn => self.generate_spawn(position)?,
                    _ => self
                        .holders
                        .get_mut(&position)
                        .expect("holder presence checked above")
                        .generate_through(status)
                        .map_err(|source| GeneratedWorkspaceError::Generation {
                            position,
                            target: status,
                            source,
                        })?,
                }
            }
        }
        Ok(())
    }

    /// Generate, consume-convert every target, then install all 117 chunks.
    /// Conversion is completed into a temporary vector first, so no map entry
    /// changes when a later target refuses.
    pub fn install_into(mut self, chunk_map: &mut ChunkMap) -> Result<(), GeneratedWorkspaceError> {
        self.generate()?;

        let mut converted = Vec::with_capacity(self.targets.len());
        for position in self.targets.iter().copied() {
            let holder =
                self.holders
                    .remove(&position)
                    .ok_or(GeneratedWorkspaceError::MissingSupport {
                        target: position,
                        support: position,
                    })?;
            let (chunk, generated_light_storage) = holder
                .into_level_chunk()
                .map_err(|source| GeneratedWorkspaceError::Conversion { position, source })?;
            // This workspace never attaches a second LIGHT owner: production
            // LIGHT remains the upstream bridge and its runtime storage is
            // returned by the holder boundary.  A non-empty value here would
            // need the downstream live ChunkMap/light-owner handoff (#231), so
            // refuse to publish rather than silently treating it as a chunk.
            if generated_light_storage.is_some() {
                return Err(GeneratedWorkspaceError::Conversion {
                    position,
                    source: GeneratedChunkError::UnsupportedStatus(ChunkStatus::Full),
                });
            }
            converted.push((position, chunk));
        }

        for (position, chunk) in converted {
            chunk_map.install(position, chunk);
        }
        Ok(())
    }

    /// Seed the shared FEATURES region cache from the authoritative holder
    /// values exactly once. Later runs retain cross-chunk writes through the
    /// `FeatureWorkspace` and publish those values back to holders.
    fn seed_feature_workspace(&mut self) {
        if self.feature_workspace_seeded {
            return;
        }
        for position in &self.support_positions {
            let snapshot = self
                .holders
                .get(position)
                .expect("support position has a holder")
                .snapshot_proto();
            self.feature_workspace.insert(snapshot);
        }
        self.feature_workspace_seeded = true;
    }

    /// Copy successful FEATURES writes from the shared region cache back into
    /// every non-center holder.  The center's own proto is authoritative for
    /// its status and writes; it is refreshed into the cache for the next
    /// target after that holder returns.
    fn publish_feature_workspace(&mut self, center: ChunkPos) {
        for proto in self.feature_workspace.snapshot_chunks() {
            let position = proto.get_pos();
            if position != center
                && let Some(holder) = self.holders.get_mut(&position)
            {
                *holder.proto_mut() = proto;
            }
        }
        let center_snapshot = self
            .holders
            .get(&center)
            .expect("FEATURES center holder remains owned")
            .snapshot_proto();
        self.feature_workspace.insert(center_snapshot);
    }

    /// Attach an engine-capable generated LIGHT task and run Paper's
    /// `INITIALIZE_LIGHT` rung. The rung computes nothing, so its provider owns
    /// an intentionally empty finite store until the target's later LIGHT
    /// promotion replaces it with the bounded runtime-neighbour window.
    fn generate_initialize_light(
        &mut self,
        position: ChunkPos,
    ) -> Result<(), GeneratedWorkspaceError> {
        let height_accessor =
            create_height_accessor(self.generator.get_min_y(), self.generator.get_gen_depth());
        let light_workspace =
            GeneratedLightWorkspace::new_for_initialize(height_accessor, true, false, position)
                .map_err(|source| GeneratedWorkspaceError::LightWorkspace { position, source })?;
        let detached = {
            let holder = self
                .holders
                .get_mut(&position)
                .expect("INITIALIZE_LIGHT position has a holder");
            let detached = holder.take_generated_light_storage();
            holder.attach_generated_light_workspace(light_workspace);
            detached
        };
        if let Some(storage) = detached
            && !storage.is_empty()
        {
            self.publish_light_storage(position, storage)?;
        }
        let attempt = {
            let holder = self
                .holders
                .get_mut(&position)
                .expect("INITIALIZE_LIGHT position has a holder");
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                holder.generate_through(ChunkStatus::InitializeLight)
            }))
        };
        let storage = self
            .holders
            .get_mut(&position)
            .expect("INITIALIZE_LIGHT position has a holder")
            .take_generated_light_storage();
        match attempt {
            Ok(Ok(())) => {
                if let Some(storage) = storage
                    && !storage.is_empty()
                {
                    self.publish_light_storage(position, storage)?;
                }
                Ok(())
            }
            Ok(Err(source)) => Err(GeneratedWorkspaceError::Generation {
                position,
                target: ChunkStatus::InitializeLight,
                source,
            }),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Build the bounded required-neighbour LIGHT window from current holder
    /// snapshots, replace the initialization-only task, run the consuming
    /// generated LIGHT write-back, and return every provider-owned neighbour to
    /// its holder.
    fn generate_light(&mut self, position: ChunkPos) -> Result<(), GeneratedWorkspaceError> {
        let mut runtime_chunks = HashMap::new();
        for (&candidate, holder) in &self.holders {
            if candidate != position
                && position.get_chessboard_distance(&candidate) <= GENERATED_LIGHT_REQUIRED_RADIUS
            {
                let generated = holder.snapshot_proto();
                let runtime =
                    GeneratedLightBridge::runtime_from_generated(&generated).map_err(|error| {
                        GeneratedWorkspaceError::LightWriteback {
                            position,
                            message: error.to_string(),
                        }
                    })?;
                let (runtime, _) = runtime.into_base_and_entities();
                runtime_chunks.insert((candidate.x(), candidate.z()), runtime);
            }
        }

        let height_accessor =
            create_height_accessor(self.generator.get_min_y(), self.generator.get_gen_depth());
        let light_workspace = GeneratedLightWorkspace::new_for_generated(
            height_accessor,
            true,
            false,
            position,
            &mut runtime_chunks,
        )
        .map_err(|source| GeneratedWorkspaceError::LightWorkspace { position, source })?;

        let detached = {
            let holder = self
                .holders
                .get_mut(&position)
                .expect("LIGHT position has a holder");
            let detached = holder.take_generated_light_storage();
            holder.attach_generated_light_workspace(light_workspace);
            detached
        };
        if let Some(storage) = detached
            && !storage.is_empty()
        {
            self.publish_light_storage(position, storage)?;
        }

        let attempt = {
            let holder = self
                .holders
                .get_mut(&position)
                .expect("LIGHT position has a holder");
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                holder.generate_through(ChunkStatus::Light)
            }))
        };
        let storage = self
            .holders
            .get_mut(&position)
            .expect("LIGHT position has a holder")
            .take_generated_light_storage();
        match attempt {
            Ok(Ok(())) => {
                if let Some(storage) = storage {
                    self.publish_light_storage(position, storage)?;
                }
                Ok(())
            }
            Ok(Err(source)) => Err(GeneratedWorkspaceError::Generation {
                position,
                target: ChunkStatus::Light,
                source,
            }),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Publish the runtime provider's bounded neighbour light state only after
    /// every returned value has been validated. The generated proto remains the
    /// authority for worldgen metadata; LIGHT changes only the Starlight fields
    /// that the runtime provider owns.
    fn publish_light_storage(
        &mut self,
        center: ChunkPos,
        storage: GeneratedLightStorage,
    ) -> Result<(), GeneratedWorkspaceError> {
        for ((key_x, key_z), runtime) in &storage {
            let key_position = ChunkPos::new(*key_x, *key_z);
            let position = runtime.get_pos();
            if position != key_position {
                return Err(GeneratedWorkspaceError::LightWriteback {
                    position: center,
                    message: format!(
                        "runtime storage key {key_position:?} contains chunk at {position:?}"
                    ),
                });
            }
            if position == center {
                return Err(GeneratedWorkspaceError::LightWriteback {
                    position: center,
                    message: "runtime LIGHT storage returned its center chunk".to_string(),
                });
            }
            if !self.holders.contains_key(&position) {
                return Err(GeneratedWorkspaceError::LightWriteback {
                    position: center,
                    message: format!("runtime storage returned unowned chunk {position:?}"),
                });
            }
        }

        for (_, runtime) in storage {
            let position = runtime.get_pos();
            let proto = self
                .holders
                .get_mut(&position)
                .expect("owner checked during LIGHT writeback")
                .proto_mut();
            proto.set_block_nibbles(runtime.block_nibbles().to_vec());
            proto.set_sky_nibbles(runtime.sky_nibbles().to_vec());
            proto.set_sky_emptiness_map(runtime.sky_emptiness_map().map(<[bool]>::to_vec));
            proto.set_light_correct(runtime.is_light_correct());
        }
        Ok(())
    }

    /// Run one production FEATURES status task against the common dependency
    /// workspace. Unlike the obsolete detached region helper, this invokes the
    /// same `GenerationChunkHolder`/Paper FEATURES body and retains writes via
    /// the shared workspace.
    fn generate_features(&mut self, position: ChunkPos) -> Result<(), GeneratedWorkspaceError> {
        self.seed_feature_workspace();
        self.holders
            .get_mut(&position)
            .expect("FEATURES position has a holder")
            .generate_through(ChunkStatus::Features)
            .map_err(|source| GeneratedWorkspaceError::Generation {
                position,
                target: ChunkStatus::Features,
                source,
            })?;
        self.publish_feature_workspace(position);
        Ok(())
    }

    /// Run the production shared radius-one SPAWN region. Neighbours are moved
    /// out of the holder arena for the duration and rebuilt afterwards. Both
    /// typed failures and panics restore snapshots, so no partial graph state
    /// survives a failed SPAWN body.
    fn generate_spawn(&mut self, position: ChunkPos) -> Result<(), GeneratedWorkspaceError> {
        let neighbour_positions = spawn_neighbour_positions(position);
        for neighbour in neighbour_positions.iter().copied() {
            if !self.holders.contains_key(&neighbour) {
                return Err(GeneratedWorkspaceError::MissingSupport {
                    target: position,
                    support: neighbour,
                });
            }
        }

        let mut center =
            self.holders
                .remove(&position)
                .ok_or(GeneratedWorkspaceError::MissingSupport {
                    target: position,
                    support: position,
                })?;
        let center_snapshot = center.snapshot_proto();
        let mut neighbour_snapshots = Vec::with_capacity(neighbour_positions.len());
        let mut neighbours = Vec::with_capacity(neighbour_positions.len());
        for neighbour in neighbour_positions.iter().copied() {
            let holder = self
                .holders
                .remove(&neighbour)
                .expect("neighbour presence checked above");
            neighbour_snapshots.push((neighbour, holder.snapshot_proto()));
            neighbours.push(holder.into_proto());
        }

        // The positions above are a mathematically complete radius-one set;
        // constructor validation is retained as a defense against future
        // scheduler changes, but cannot reject this canonical list.
        let mut region = SpawnRegionProtos::new(position, neighbours)
            .expect("canonical radius-one SPAWN neighbours must be complete");
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            center.generate_spawn_with_region(&mut region)
        }));
        let returned_neighbours = region.into_neighbours();

        match result {
            Ok(Ok(())) => {
                for proto in returned_neighbours {
                    let neighbour = proto.get_pos();
                    self.holders.insert(
                        neighbour,
                        GenerationChunkHolder::from_proto_with_workspace(
                            proto,
                            self.generator.clone(),
                            self.feature_workspace.clone(),
                        ),
                    );
                }
                self.holders.insert(position, center);
                Ok(())
            }
            Ok(Err(source)) => {
                self.restore_spawn_failure(position, center, center_snapshot, neighbour_snapshots);
                Err(GeneratedWorkspaceError::Generation {
                    position,
                    target: ChunkStatus::Spawn,
                    source,
                })
            }
            Err(payload) => {
                self.restore_spawn_failure(position, center, center_snapshot, neighbour_snapshots);
                std::panic::resume_unwind(payload)
            }
        }
    }

    fn restore_spawn_failure(
        &mut self,
        position: ChunkPos,
        mut center: GenerationChunkHolder,
        center_snapshot: ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
        neighbour_snapshots: Vec<(
            ChunkPos,
            ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
        )>,
    ) {
        *center.proto_mut() = center_snapshot;
        self.holders.insert(position, center);
        for (neighbour, proto) in neighbour_snapshots {
            self.holders.insert(
                neighbour,
                GenerationChunkHolder::from_proto_with_workspace(
                    proto,
                    self.generator.clone(),
                    self.feature_workspace.clone(),
                ),
            );
        }
    }
}

/// Materialize the holders attached to the bounded FULL view. Paper's FULL
/// dependency pyramid has an accumulated radius of 11, but only the 117 target
/// tickets and their radius-one neighbours are attached to this slice. The
/// farther dependency window is an ephemeral generation input owned by the
/// upstream scheduler, not an additional served/attached holder graph.
fn accumulate_full_support(targets: &[ChunkPos]) -> HashMap<ChunkPos, ChunkStatus> {
    let full = GENERATION_PYRAMID.get_step_to(ChunkStatus::Full);
    let mut required = HashMap::with_capacity(GENERATED_TARGET_COUNT + GENERATED_SUPPORT_COUNT);
    for target in targets {
        for dx in -1..=1 {
            for dz in -1..=1 {
                let position =
                    ChunkPos::new(target.x().wrapping_add(dx), target.z().wrapping_add(dz));
                let distance = dx.abs().max(dz.abs()) as usize;
                let status = full.required_status_at_radius(distance);
                required
                    .entry(position)
                    .and_modify(|current: &mut ChunkStatus| {
                        if status.index() > current.index() {
                            *current = status;
                        }
                    })
                    .or_insert(status);
            }
        }
    }
    required
}

fn build_status_waves(
    support_positions: &[ChunkPos],
    required_status: &HashMap<ChunkPos, ChunkStatus>,
) -> Vec<GeneratedStatusWave> {
    ChunkStatus::ALL
        .into_iter()
        .skip(1)
        .filter(|status| *status != ChunkStatus::Full)
        .map(|status| GeneratedStatusWave {
            status,
            positions: support_positions
                .iter()
                .copied()
                .filter(|position| {
                    required_status
                        .get(position)
                        .is_some_and(|required| required.index() >= status.index())
                })
                .collect(),
        })
        .collect()
}

fn sort_positions(positions: &mut [ChunkPos]) {
    positions.sort_by_key(|position| (position.x(), position.z()));
}

fn spawn_neighbour_positions(center: ChunkPos) -> Vec<ChunkPos> {
    let mut positions = Vec::with_capacity(8);
    for dx in -1..=1 {
        for dz in -1..=1 {
            if dx != 0 || dz != 0 {
                positions.push(ChunkPos::new(
                    center.x().wrapping_add(dx),
                    center.z().wrapping_add(dz),
                ));
            }
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn seed_42_view_is_exactly_117_targets_in_raster_order() {
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        assert_eq!(workspace.target_positions().len(), GENERATED_TARGET_COUNT);
        assert_eq!(workspace.target_positions()[0], ChunkPos::new(-5, -4));
        assert_eq!(workspace.target_positions()[116], ChunkPos::new(5, 4));
        for pair in workspace.target_positions().windows(2) {
            assert!((pair[0].x(), pair[0].z()) < (pair[1].x(), pair[1].z()));
        }
    }

    #[test]
    fn attached_full_graph_is_165_with_48_non_target_radius_one_supports() {
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        assert_eq!(workspace.support_positions().len(), 165);
        assert_eq!(workspace.holder_count(), 165);
        let targets: HashSet<_> = workspace.target_positions().iter().copied().collect();
        let radius_one_supports = workspace
            .support_positions()
            .iter()
            .filter(|position| {
                !targets.contains(position)
                    && targets.iter().any(|target| {
                        position.x().abs_diff(target.x()) <= 1
                            && position.z().abs_diff(target.z()) <= 1
                    })
            })
            .count();
        assert_eq!(radius_one_supports, GENERATED_SUPPORT_COUNT);
        assert_eq!(
            workspace.required_status(ChunkPos::ZERO),
            Some(ChunkStatus::Spawn)
        );
        assert_eq!(
            workspace.required_status(ChunkPos::new(6, 0)),
            Some(ChunkStatus::InitializeLight)
        );
        assert_eq!(workspace.required_status(ChunkPos::new(7, 0)), None);
    }

    #[test]
    fn status_waves_are_deterministic_and_features_attach_165_holders() {
        let a = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        let b = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        assert_eq!(a.status_waves(), b.status_waves());
        assert!(
            a.status_waves()
                .iter()
                .all(|wave| wave.status() != ChunkStatus::Full)
        );
        let features = a
            .status_waves()
            .iter()
            .find(|wave| wave.status() == ChunkStatus::Features)
            .expect("FEATURES wave");
        assert_eq!(features.positions().len(), 165);
        assert_eq!(
            features
                .positions()
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len(),
            165
        );
    }

    #[test]
    fn missing_spawn_support_is_typed_and_does_not_publish() {
        let mut workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        let missing = ChunkPos::new(1, 0);
        workspace.holders.remove(&missing);
        let error = workspace
            .generate_spawn(ChunkPos::ZERO)
            .expect_err("missing radius-one support");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::MissingSupport { target, support }
                if target == ChunkPos::ZERO && support == missing
        ));
        assert!(!workspace.is_ready());
    }

    #[test]
    fn status_failure_leaves_chunk_map_empty() {
        let mut workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        // Limit this counterfactual to the first unavailable status task. The
        // real full drain is intentionally owned by the production pipeline;
        // this test only proves its transactional map boundary.
        workspace.waves = vec![GeneratedStatusWave {
            status: ChunkStatus::InitializeLight,
            positions: vec![ChunkPos::ZERO],
        }];
        let mut chunk_map = ChunkMap::empty(GENERATED_VIEW_DISTANCE);
        let error = workspace
            .install_into(&mut chunk_map)
            .expect_err("the unavailable status is typed");
        assert!(matches!(error, GeneratedWorkspaceError::Generation { .. }));
        assert_eq!(chunk_map.len(), 0);
    }

    #[test]
    fn packet_before_ready_is_refused_without_lower_status_substitution() {
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        let error = workspace
            .require_ready()
            .expect_err("fresh holders are not ready");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::PacketBeforeReady {
                position: ChunkPos { .. },
                status: Some(ChunkStatus::Empty)
            }
        ));
    }

    #[test]
    fn consuming_promotion_requires_exact_spawn_parent() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let mut holder = generator.create_holder(ChunkPos::ZERO);
        holder.proto_mut().set_persisted_status(ChunkStatus::Spawn);
        let (chunk, storage) = holder.into_level_chunk().expect("SPAWN promotes");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert!(storage.is_none());

        let mut refused = generator.create_holder(ChunkPos::new(1, 0));
        refused.proto_mut().set_persisted_status(ChunkStatus::Light);
        assert!(matches!(
            refused.into_level_chunk(),
            Err(GeneratedChunkError::Convert { .. })
        ));
    }

    #[test]
    fn deterministic_install_order_is_target_raster_order() {
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        assert_eq!(workspace.target_positions()[0], ChunkPos::new(-5, -4));
        assert_eq!(workspace.target_positions()[116], ChunkPos::new(5, 4));
        for pair in workspace.target_positions().windows(2) {
            assert!((pair[0].x(), pair[0].z()) < (pair[1].x(), pair[1].z()));
        }
    }

    #[test]
    fn ready_install_publishes_exactly_the_117_targets() {
        let mut workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        for holder in workspace.holders.values_mut() {
            holder.proto_mut().set_persisted_status(ChunkStatus::Spawn);
        }
        assert!(workspace.is_ready());
        let targets = workspace.target_positions().to_vec();
        let mut chunk_map = ChunkMap::empty(GENERATED_VIEW_DISTANCE);
        workspace
            .install_into(&mut chunk_map)
            .expect("all synthetic SPAWN parents convert");
        assert_eq!(chunk_map.len(), GENERATED_TARGET_COUNT);
        assert!(
            targets
                .iter()
                .all(|position| chunk_map.get_chunk(*position).is_some())
        );
    }

    #[test]
    fn rollback_conversion_failure_installs_no_partial_graph() {
        let mut workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        for holder in workspace.holders.values_mut() {
            holder.proto_mut().set_persisted_status(ChunkStatus::Spawn);
        }
        workspace
            .holders
            .get_mut(&ChunkPos::ZERO)
            .expect("center target")
            .proto_mut()
            .set_persisted_status(ChunkStatus::Full);
        let mut chunk_map = ChunkMap::empty(GENERATED_VIEW_DISTANCE);
        let error = workspace
            .install_into(&mut chunk_map)
            .expect_err("one target refuses conversion");
        assert!(matches!(error, GeneratedWorkspaceError::Conversion { .. }));
        assert_eq!(chunk_map.len(), 0);
    }

    #[test]
    fn wrapped_extreme_center_fails_closed() {
        for center in [
            ChunkPos::new(i32::MAX, i32::MIN),
            ChunkPos::new(i32::MAX, 0),
            ChunkPos::new(i32::MIN, 0),
        ] {
            assert!(matches!(
                GeneratedWorkspace::new(42, center),
                Err(GeneratedWorkspaceError::InvalidTargetView {
                    actual_count: 0,
                    expected_count: GENERATED_TARGET_COUNT,
                    ..
                })
            ));
        }
    }

    #[test]
    fn generated_light_wave_uses_production_provider_and_persists_results() {
        let mut workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        // FEATURES and SPAWN are intentionally outside this focused end-to-end
        // slice: FEATURES retains its typed decoration boundary. Seed the
        // holders at FEATURES and drive one target through both production
        // light rungs, avoiding minutes of unrelated worldgen while still
        // exercising the real initialization and bounded required-neighbour
        // LIGHT attachment. The FEATURES status is a precondition fixture only; the
        // decoration body is covered by its own typed-boundary tests.
        for holder in workspace.holders.values_mut() {
            holder
                .proto_mut()
                .set_persisted_status(ChunkStatus::Features);
        }
        workspace.waves = vec![
            GeneratedStatusWave {
                status: ChunkStatus::InitializeLight,
                positions: vec![ChunkPos::ZERO],
            },
            GeneratedStatusWave {
                status: ChunkStatus::Light,
                positions: vec![ChunkPos::ZERO],
            },
        ];

        workspace.generate().expect("generated LIGHT wave");

        let center = workspace
            .holders
            .get(&ChunkPos::ZERO)
            .expect("target holder remains attached");
        assert_eq!(center.status(), ChunkStatus::Light);
        let center_proto = center.snapshot_proto();
        assert!(
            center_proto.is_light_correct(),
            "production LIGHT must persist the center result"
        );
        assert!(
            center_proto.sky_emptiness_map().is_some(),
            "production LIGHT must persist sky emptiness"
        );
        assert!(
            center_proto.has_light_engine(),
            "INITIALIZE_LIGHT must associate the production light engine"
        );
        let untouched_target = workspace
            .targets
            .iter()
            .copied()
            .find(|position| *position != ChunkPos::ZERO)
            .expect("a second target exists");
        assert_eq!(
            workspace
                .holders
                .get(&untouched_target)
                .expect("untouched target remains attached")
                .status(),
            ChunkStatus::Features
        );
        assert!(
            workspace
                .holders
                .values_mut()
                .all(|holder| holder.take_generated_light_storage().is_none()),
            "bounded provider storage must be detached after LIGHT"
        );

        // The non-target radius-one support ring remains at FEATURES and is
        // never promoted or served. Its runtime light values were returned to
        // the generated holder arena after the target's bounded task.
        let target_set: HashSet<_> = workspace.targets.iter().copied().collect();
        for position in &workspace.support_positions {
            if !target_set.contains(position) {
                assert_eq!(
                    workspace
                        .holders
                        .get(position)
                        .expect("support holder remains attached")
                        .status(),
                    ChunkStatus::Features
                );
            }
        }
    }

    #[test]
    fn generated_light_wave_rejects_missing_required_neighbor_without_attachment() {
        let mut workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("target view");
        for holder in workspace.holders.values_mut() {
            holder
                .proto_mut()
                .set_persisted_status(ChunkStatus::Features);
        }
        workspace.waves = vec![
            GeneratedStatusWave {
                status: ChunkStatus::InitializeLight,
                positions: vec![ChunkPos::ZERO],
            },
            GeneratedStatusWave {
                status: ChunkStatus::Light,
                positions: vec![ChunkPos::ZERO],
            },
        ];
        let missing = ChunkPos::new(1, 0);
        workspace.holders.remove(&missing);

        let error = workspace
            .generate()
            .expect_err("LIGHT must reject a missing inner-ring holder");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::LightWorkspace { position, source }
                if position == ChunkPos::ZERO
                    && matches!(
                        source,
                        GeneratedLightWorkspaceError::MissingNeighbour { center, pos }
                            if center == ChunkPos::ZERO && pos == missing
                    )
        ));
        assert_eq!(
            workspace
                .holders
                .get(&ChunkPos::ZERO)
                .expect("center remains attached")
                .status(),
            ChunkStatus::InitializeLight
        );
        assert!(
            workspace
                .holders
                .values_mut()
                .all(|holder| holder.take_generated_light_storage().is_none()),
            "a rejected bounded workspace must not leave a provider attached"
        );
    }
}

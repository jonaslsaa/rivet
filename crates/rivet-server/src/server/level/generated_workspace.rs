//! A finite, tick-thread-owned generated workspace for the seed-42 join view.
//!
//! The workspace is deliberately separate from the live server boot until the
//! generated FEATURES breadth can complete. It owns the target holders and the
//! accumulated FULL support closure by value, drives deterministic X-major /
//! Z-minor status waves, and exposes one transactional install seam. A typed
//! generation or conversion refusal leaves the caller's `ChunkMap` untouched;
//! this module never fabricates a FULL chunk to get a server boot green.

use std::collections::HashMap;
use std::sync::Arc;

use crate::server::level::level_chunk::StructureKey;
use rivet_registry::core::{ChunkPos, SectionPos};
use rivet_util::StaticCache2D;
use rivet_world::chunk::chunk_generator::ChunkGenerator;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, GENERATION_PYRAMID, GenError};
use rivet_world::level::height_accessor::LevelHeightAccessor;

use super::chunk_map::ChunkMap;
use super::chunk_tracking_view::ChunkTrackingView;
use super::generated_world::{
    GeneratedChunkError, GenerationChunkHolder, OverworldGenerator, run_biome_decoration_in_region,
};
use super::world_gen_region::{
    CenterHolder, GenerationChunkHolderView, OwnedProtoHolder, WorldGenRegion,
};
use rivet_registry::block_state::BlockState;
use rivet_world::chunk::storage::section_reconstruction::BiomeId as WorldgenBiomeId;

/// The finite generated-serving view: the exact 117-position view emitted by
/// `ChunkTrackingView::for_each` at radius four.
pub const GENERATED_VIEW_DISTANCE: i32 = 4;
/// The exact number of target chunks in the radius-four send view.
pub const GENERATED_TARGET_COUNT: usize = 117;
/// The accumulated FULL dependency radius from `ChunkPyramid`.
pub const FULL_SUPPORT_RADIUS: i32 = 11;

/// One deterministic status wave. Positions are always X-major / Z-minor,
/// independent of hash-map insertion order or any future worker scheduling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedStatusWave {
    status: ChunkStatus,
    positions: Vec<ChunkPos>,
}

impl GeneratedStatusWave {
    /// The wave's target status.
    pub fn status(&self) -> ChunkStatus {
        self.status
    }

    /// The wave's deterministic positions.
    pub fn positions(&self) -> &[ChunkPos] {
        &self.positions
    }
}

/// Typed refusal from the finite generated workspace. The error carries the
/// exact holder/status boundary instead of replacing an incomplete generated
/// chunk with superflat content.
#[derive(Debug, thiserror::Error)]
pub enum GeneratedWorkspaceError {
    /// The wrapped target view did not produce the exact supported 117-position
    /// set. Paper's int arithmetic can wrap the view bounds at extreme centers;
    /// this workspace refuses that pathological shape instead of treating an
    /// empty enumeration as a successful generated world.
    #[error(
        "generated workspace view centered at {center} with distance {view_distance} enumerated {actual_count} targets; expected {expected_count}"
    )]
    InvalidTargetView {
        center: ChunkPos,
        view_distance: i32,
        actual_count: usize,
        expected_count: usize,
    },
    /// A generated-serving constructor received a config outside the fixed
    /// normal-overworld contract.
    #[error("generated world config field {field} is incompatible; expected {expected}")]
    InvalidConfiguration {
        field: &'static str,
        expected: &'static str,
    },
    /// A support holder required by a shared FEATURES region was absent.
    #[error("generated workspace target {target} is missing FULL support holder {support}")]
    MissingSupport { target: ChunkPos, support: ChunkPos },
    /// The workspace does not yet own Paper's SPAWN WorldGenRegion/cache
    /// seam. Refuse before calling the detached holder-only spawn closure.
    #[error(
        "generated workspace cannot run SPAWN for {position}: Paper's shared WorldGenRegion seam is not wired"
    )]
    SpawnRegionUnavailable { position: ChunkPos },
    /// A status wave refused before its holder could advance.
    #[error("generated workspace holder {position} refused status {target:?}: {source}")]
    Generation {
        position: ChunkPos,
        target: ChunkStatus,
        source: GeneratedChunkError,
    },
    /// A target holder refused the consuming FULL conversion.
    #[error("generated workspace target {position} refused consuming FULL conversion: {source}")]
    Conversion {
        position: ChunkPos,
        source: GeneratedChunkError,
    },
}

/// A finite generated world slice. All mutable holders and generated protos
/// remain on the owner thread; the only shared value is the immutable
/// per-world generator configuration already used by `GenerationChunkHolder`.
pub struct GeneratedWorkspace {
    seed: i64,
    view: ChunkTrackingView,
    generator: Arc<OverworldGenerator>,
    targets: Vec<ChunkPos>,
    support_positions: Vec<ChunkPos>,
    required_status: HashMap<ChunkPos, ChunkStatus>,
    waves: Vec<GeneratedStatusWave>,
    holders: HashMap<ChunkPos, GenerationChunkHolder>,
    feature_failures: HashMap<ChunkPos, GenError>,
}

impl GeneratedWorkspace {
    /// Build the finite workspace around `center` for a world seed.
    ///
    /// The target list is not hand-written: it is derived solely from
    /// `ChunkTrackingView::for_each`, so the install set stays exactly aligned
    /// with the join/send view. The support map is the union of the FULL step's
    /// accumulated dependency window around those targets, through radius 11.
    /// A wrapped view at an extreme center is refused before any generator or
    /// holder is constructed; an empty target map is never a successful world.
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
        let holders = support_positions
            .iter()
            .copied()
            .map(|pos| (pos, generator.create_holder(pos)))
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
            feature_failures: HashMap::new(),
        })
    }

    /// The seed captured by this workspace.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// The realized generator geometry used by every holder in this workspace.
    /// This internal seam keeps generated world metadata tied to the actual
    /// normal-overworld generator rather than flat-world constants.
    pub(crate) fn generator_geometry(&self) -> (i32, i32, i32) {
        (
            self.generator.get_min_y(),
            self.generator.get_gen_depth(),
            self.generator.get_sea_level(),
        )
    }

    /// The exact target view used by the workspace.
    pub fn view(&self) -> &ChunkTrackingView {
        &self.view
    }

    /// The exact 117 target install positions in X-major / Z-minor order.
    pub fn target_positions(&self) -> &[ChunkPos] {
        &self.targets
    }

    /// The accumulated FULL support closure in deterministic order.
    pub fn support_positions(&self) -> &[ChunkPos] {
        &self.support_positions
    }

    /// The generated minimum status required at a support position by the FULL
    /// dependency closure. This is a value-only inspection seam for scheduling
    /// and focused tests.
    pub fn required_status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
        self.required_status.get(&pos).copied()
    }

    /// The deterministic executable status waves. FULL is intentionally absent:
    /// FULL is the consuming holder-to-LevelChunk boundary, not an executor
    /// task, and install performs that conversion only after all waves finish.
    pub fn status_waves(&self) -> &[GeneratedStatusWave] {
        &self.waves
    }

    /// Generate the workspace in dependency order without touching a
    /// `ChunkMap`. A typed refusal leaves all already-produced holder state in
    /// this tick-thread workspace for diagnostics, but no server authority is
    /// changed.
    pub fn generate(&mut self) -> Result<(), GeneratedWorkspaceError> {
        for wave_index in 0..self.waves.len() {
            let status = self.waves[wave_index].status;
            let positions = self.waves[wave_index].positions.clone();
            for position in positions {
                if self
                    .holders
                    .get(&position)
                    .is_some_and(|holder| holder.status().is_or_after(status))
                {
                    // A retry walks the immutable ladder from its first wave,
                    // but a holder may already be beyond this rung. Never ask
                    // the executor to demote it; completed FEATURES holders in
                    // particular must not be re-decorated.
                    continue;
                }
                if status == ChunkStatus::Spawn {
                    // `GenerationChunkHolder::run_spawn` is intentionally a
                    // detached holder-only seam. Paper SPAWN reads through a
                    // shared WorldGenRegion/cache, so do not invoke it until
                    // this workspace composes that exact region.
                    return Err(GeneratedWorkspaceError::SpawnRegionUnavailable { position });
                }
                if status == ChunkStatus::Features {
                    // Shared FEATURES is not an executor idempotent path: it
                    // temporarily moves the complete dependency window and
                    // performs decoration itself. The completed-status guard
                    // above skips it on retries.
                    self.generate_features_with_shared_region(position)?;
                    continue;
                }
                let holder = self.holders.get_mut(&position).ok_or(
                    GeneratedWorkspaceError::MissingSupport {
                        target: position,
                        support: position,
                    },
                )?;
                holder.generate_through(status).map_err(|source| {
                    GeneratedWorkspaceError::Generation {
                        position,
                        target: status,
                        source,
                    }
                })?;
            }
        }
        Ok(())
    }

    /// Generate, consume-convert every target holder, and only then install the
    /// 117 resulting `LevelChunk`s. No `ChunkMap::install` call occurs until
    /// every conversion succeeds, so any typed refusal installs none.
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
            let chunk = holder
                .into_level_chunk()
                .map_err(|source| GeneratedWorkspaceError::Conversion { position, source })?;
            converted.push((position, chunk));
        }

        for (position, chunk) in converted {
            chunk_map.install(position, chunk);
        }
        Ok(())
    }

    /// Run one FEATURES target against the workspace's moved support protos.
    /// `WorldGenRegion` owns the ring adapters only for the duration of the
    /// call; they are extracted and rebuilt as holders afterwards, preserving
    /// every cross-chunk block/entity/post-processing write.
    fn generate_features_with_shared_region(
        &mut self,
        target: ChunkPos,
    ) -> Result<(), GeneratedWorkspaceError> {
        if let Some(source) = self.feature_failures.get(&target).copied() {
            return Err(GeneratedWorkspaceError::Generation {
                position: target,
                target: ChunkStatus::Features,
                source: GeneratedChunkError::Generation(source),
            });
        }

        let ring_positions = feature_dependency_positions(target);
        for support in ring_positions.iter().copied() {
            if !self.holders.contains_key(&support) {
                return Err(GeneratedWorkspaceError::MissingSupport { target, support });
            }
        }

        let mut center =
            self.holders
                .remove(&target)
                .ok_or(GeneratedWorkspaceError::MissingSupport {
                    target,
                    support: target,
                })?;
        if !center.status().is_or_after(ChunkStatus::Carvers) {
            let source = GeneratedChunkError::Generation(GenError::FeaturesNotGenerated);
            self.holders.insert(target, center);
            return Err(GeneratedWorkspaceError::Generation {
                position: target,
                target: ChunkStatus::Features,
                source,
            });
        }

        let mut ring = Vec::with_capacity(ring_positions.len());
        for support in ring_positions {
            let holder = self
                .holders
                .remove(&support)
                .expect("shared FEATURES ring presence was checked above");
            ring.push(holder.into_proto());
        }

        let result = {
            let center_proto = center.proto_mut();
            center_proto.prime_heightmaps(&rivet_world::levelgen::heightmap::FINAL_HEIGHTMAPS);
            let center_pos = center_proto.get_pos();
            let origin = SectionPos::of_chunk_pos(
                &center_pos,
                center_proto.height_accessor().get_min_section_y(),
            )
            .origin();
            let mut region = compose_shared_feature_region(center_proto, &self.generator, ring);
            let result = run_biome_decoration_in_region(&mut region, &self.generator, &origin);
            let returned = region.into_owned_proto_chunks();
            (result, returned)
        };

        for proto in result.1 {
            let position = proto.get_pos();
            self.holders.insert(
                position,
                GenerationChunkHolder::from_proto(proto, self.generator.clone()),
            );
        }
        self.holders.insert(target, center);

        if let Err(source) = result.0 {
            // The region has already been returned, so its cross-chunk writes
            // are retained. Do not run a failed decoration body again: the
            // same typed failure is deterministic, while re-decoration could
            // duplicate ticks, block-entity writes, or post-processing marks.
            self.feature_failures.insert(target, source);
            return Err(GeneratedWorkspaceError::Generation {
                position: target,
                target: ChunkStatus::Features,
                source: GeneratedChunkError::Generation(source),
            });
        }
        // This is the executor's post-task publication, performed only after
        // the exact shared-region body returned `Ok`; no FEATURES status is
        // stamped on a typed decoration refusal.
        self.holders
            .get_mut(&target)
            .expect("the center holder was reinserted above")
            .proto_mut()
            .set_persisted_status(ChunkStatus::Features);
        Ok(())
    }
}

/// Build the union of the FULL step's accumulated dependency windows around
/// the exact target view. Each support position stores the strongest required
/// status among all target windows, which is what deterministic status waves
/// consume.
fn accumulate_full_support(targets: &[ChunkPos]) -> HashMap<ChunkPos, ChunkStatus> {
    let full = GENERATION_PYRAMID.get_step_to(ChunkStatus::Full);
    let mut required = HashMap::new();
    for target in targets {
        for dx in -FULL_SUPPORT_RADIUS..=FULL_SUPPORT_RADIUS {
            for dz in -FULL_SUPPORT_RADIUS..=FULL_SUPPORT_RADIUS {
                let distance = dx.abs().max(dz.abs()) as usize;
                if distance > FULL_SUPPORT_RADIUS as usize {
                    continue;
                }
                let pos = ChunkPos::new(target.x().wrapping_add(dx), target.z().wrapping_add(dz));
                let status = full.required_status_at_radius(distance);
                required
                    .entry(pos)
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
        .map(|status| {
            let positions = support_positions
                .iter()
                .copied()
                .filter(|pos| {
                    required_status
                        .get(pos)
                        .is_some_and(|required| required.index() >= status.index())
                })
                .collect();
            GeneratedStatusWave { status, positions }
        })
        .collect()
}

fn sort_positions(positions: &mut [ChunkPos]) {
    positions.sort_by_key(|pos| (pos.x(), pos.z()));
}

fn feature_dependency_positions(center: ChunkPos) -> Vec<ChunkPos> {
    let mut positions = Vec::with_capacity(17 * 17 - 1);
    for dx in -8..=8 {
        for dz in -8..=8 {
            if dx == 0 && dz == 0 {
                continue;
            }
            positions.push(ChunkPos::new(
                center.x().wrapping_add(dx),
                center.z().wrapping_add(dz),
            ));
        }
    }
    positions
}

fn compose_shared_feature_region<'a>(
    center: &'a mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
    ring: Vec<ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>>,
) -> WorldGenRegion<'a, BlockState, WorldgenBiomeId, StructureKey> {
    let center_pos = center.get_pos();
    let center_status = center.get_persisted_status();
    let mut by_pos: HashMap<ChunkPos, ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>> = ring
        .into_iter()
        .map(|chunk| (chunk.get_pos(), chunk))
        .collect();
    let mut holders: Vec<
        Box<dyn GenerationChunkHolderView<BlockState, WorldgenBiomeId, StructureKey> + 'a>,
    > = Vec::with_capacity(17 * 17 - 1);
    for dx in -8..=8 {
        for dz in -8..=8 {
            let pos = ChunkPos::new(
                center_pos.x().wrapping_add(dx),
                center_pos.z().wrapping_add(dz),
            );
            if pos == center_pos {
                continue;
            }
            let chunk = by_pos
                .remove(&pos)
                .expect("the shared FEATURES dependency window must be complete");
            holders.push(Box::new(OwnedProtoHolder::new(chunk)));
        }
    }
    // Insert the borrowed center at its X-major/Z-minor slot after all ring
    // holders have been built. This keeps one mutable borrow of `center` and
    // preserves the cache's exact coordinate order.
    let center_index = 8 * 17 + 8;
    holders.insert(
        center_index,
        Box::new(CenterHolder::new(center.base_mut(), center_status)),
    );

    let cache = StaticCache2D::from_entries(
        center_pos.x().wrapping_sub(8),
        center_pos.z().wrapping_sub(8),
        17,
        17,
        holders,
    );
    let step = GENERATION_PYRAMID
        .get_step_to(ChunkStatus::Features)
        .clone();
    WorldGenRegion::new(
        cache,
        center_pos,
        step,
        generator.seed(),
        generator.get_min_y(),
        generator.get_gen_depth(),
        generator.get_sea_level(),
        Arc::new(generator.biome_source().clone()),
        generator.feature_access().clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::super::generated_world::fresh_worldgen_chunk;
    use super::*;

    #[test]
    fn seed_42_view_derives_exactly_117_targets_in_raster_order() {
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        assert_eq!(workspace.seed(), 42);
        assert_eq!(workspace.target_positions().len(), 117);
        assert_eq!(workspace.target_positions()[0], ChunkPos::new(-5, -4));
        assert_eq!(workspace.target_positions()[116], ChunkPos::new(5, 4));
        for pair in workspace.target_positions().windows(2) {
            assert!(
                (pair[0].x(), pair[0].z()) < (pair[1].x(), pair[1].z()),
                "targets must be strictly X-major/Z-minor: {:?} -> {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn support_closure_is_full_radius_11_and_has_status_boundaries() {
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        assert_eq!(workspace.support_positions().len(), 1085);
        assert!(
            workspace
                .support_positions()
                .iter()
                .any(|pos| { pos.x().abs().max(pos.z().abs()) == FULL_SUPPORT_RADIUS })
        );
        assert_eq!(
            workspace.required_status(ChunkPos::ZERO),
            Some(ChunkStatus::Spawn)
        );
        assert_eq!(
            workspace.required_status(ChunkPos::new(7, 0)),
            Some(ChunkStatus::Carvers)
        );
        assert_eq!(
            workspace.required_status(ChunkPos::new(8, 0)),
            Some(ChunkStatus::Biomes)
        );
        assert_eq!(
            workspace.required_status(ChunkPos::new(9, 0)),
            Some(ChunkStatus::StructureStarts)
        );
    }

    #[test]
    fn shared_features_move_the_complete_17_by_17_dependency_window() {
        assert_eq!(
            feature_dependency_positions(ChunkPos::ZERO).len(),
            17 * 17 - 1
        );
        assert!(!feature_dependency_positions(ChunkPos::ZERO).contains(&ChunkPos::ZERO));
    }

    #[test]
    fn shared_features_region_returns_owned_ring_writes() {
        let generator = Arc::new(OverworldGenerator::new(42));
        let mut center = generator.create_holder(ChunkPos::ZERO);
        center
            .generate_through(ChunkStatus::Carvers)
            .expect("center CARVERS");
        let ring = feature_dependency_positions(ChunkPos::ZERO)
            .into_iter()
            .map(|pos| {
                let mut chunk = fresh_worldgen_chunk(pos, &generator);
                chunk.prime_heightmaps(&rivet_world::levelgen::heightmap::FINAL_HEIGHTMAPS);
                let distance = pos.x().abs().max(pos.z().abs());
                chunk.set_persisted_status(if distance <= 1 {
                    ChunkStatus::Carvers
                } else {
                    ChunkStatus::StructureStarts
                });
                chunk
            })
            .collect();
        let write_pos = ChunkPos::new(1, 0).get_block_at(0, 64, 0);
        let state = rivet_world::block::blocks::Blocks::STONE.default_block_state();
        let returned = {
            let mut region = compose_shared_feature_region(center.proto_mut(), &generator, ring);
            assert!(region.set_block(&write_pos, state, 2, 512));
            region.into_owned_proto_chunks()
        };
        let persisted = returned
            .into_iter()
            .find(|chunk| chunk.get_pos() == ChunkPos::new(1, 0))
            .expect("the written ring proto must be returned");
        assert_eq!(
            persisted.get_block_state(write_pos.get_x(), write_pos.get_y(), write_pos.get_z()),
            state
        );
    }

    #[test]
    fn status_waves_are_deterministic_and_have_no_executor_full_wave() {
        let a = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        let b = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        assert_eq!(a.status_waves(), b.status_waves());
        assert_eq!(a.status_waves().len(), ChunkStatus::ALL.len() - 2);
        assert!(
            a.status_waves()
                .iter()
                .all(|wave| wave.status() != ChunkStatus::Full)
        );
        assert_eq!(
            a.status_waves().last().map(GeneratedStatusWave::status),
            Some(ChunkStatus::Spawn)
        );

        // Every holder runs each intermediate rung up to its terminal status.
        // FULL's dependency closure makes the FEATURES wave the 117 d0 targets
        // plus the 48 d1 support holders; d2 CARVERS holders do not enter it.
        let features = a
            .status_waves()
            .iter()
            .find(|wave| wave.status() == ChunkStatus::Features)
            .expect("FEATURES wave");
        assert_eq!(features.positions().len(), 165);
        assert!(features.positions().contains(&ChunkPos::ZERO));
        assert!(features.positions().contains(&ChunkPos::new(1, 0)));
        assert!(!features.positions().contains(&ChunkPos::new(7, 0)));
        let target_set: std::collections::HashSet<_> =
            a.target_positions().iter().copied().collect();
        let feature_set: std::collections::HashSet<_> =
            features.positions().iter().copied().collect();
        assert_eq!(target_set.len(), 117);
        assert!(
            target_set.is_subset(&feature_set),
            "every d0 target must enter FEATURES"
        );
        assert_eq!(
            feature_set.len() - target_set.len(),
            48,
            "FEATURES has 48 d1 supports"
        );
        assert!(
            feature_set.iter().all(|position| {
                target_set.iter().any(|target| {
                    position.x().wrapping_sub(target.x()).abs() <= 1
                        && position.z().wrapping_sub(target.z()).abs() <= 1
                })
            }),
            "d2 support must stay out of FEATURES"
        );
        assert_eq!(a.required_status(ChunkPos::ZERO), Some(ChunkStatus::Spawn));
        assert_eq!(
            a.required_status(ChunkPos::new(6, 0)),
            Some(ChunkStatus::InitializeLight)
        );
        assert_eq!(
            a.required_status(ChunkPos::new(7, 0)),
            Some(ChunkStatus::Carvers)
        );
    }

    #[test]
    fn retry_skips_completed_features_before_later_typed_failure() {
        let mut workspace =
            GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        // Mark every support holder as already completed through FEATURES. This
        // isolates the retry boundary: no lower-rung worldgen work is needed,
        // and the next wave reaches the honest light-engine refusal directly.
        for holder in workspace.holders.values_mut() {
            holder
                .proto_mut()
                .set_persisted_status(ChunkStatus::Features);
        }

        // Treat FEATURES as already completed, then retry into the honest
        // light-engine boundary. A retry must skip decoration rather than
        // re-running it against already-written holders.
        let error = workspace.generate().expect_err("light engine is not wired");
        println!("retry error: {error:?}");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::Generation {
                target: ChunkStatus::InitializeLight,
                source: GeneratedChunkError::Generation(GenError::LightEngineMissing {
                    status: ChunkStatus::InitializeLight,
                }),
                ..
            }
        ));
        assert!(
            workspace
                .holders
                .values()
                .filter(|holder| holder.status() == ChunkStatus::Features)
                .count()
                >= 165
        );
    }

    #[test]
    fn failed_features_are_not_redecorated_after_failure() {
        let mut workspace =
            GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        let source = GenError::FeaturePlacementDecode {
            chunk_pos: ChunkPos::ZERO,
            step_index: 9,
            global_feature_index: 17,
            feature_key: "minecraft:dark_forest_vegetation",
        };
        workspace.feature_failures.insert(ChunkPos::ZERO, source);

        let error = workspace
            .generate_features_with_shared_region(ChunkPos::ZERO)
            .expect_err("cached FEATURES failure");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::Generation {
                position: ChunkPos::ZERO,
                target: ChunkStatus::Features,
                source: GeneratedChunkError::Generation(GenError::FeaturePlacementDecode {
                    step_index: 9,
                    global_feature_index: 17,
                    ..
                }),
            }
        ));
    }

    #[test]
    fn spawn_refuses_before_detached_holder_only_execution() {
        let mut workspace =
            GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        for holder in workspace.holders.values_mut() {
            holder.proto_mut().set_persisted_status(ChunkStatus::Light);
        }

        let error = workspace
            .generate()
            .expect_err("SPAWN needs the shared WorldGenRegion seam");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::SpawnRegionUnavailable { .. }
        ));
    }

    #[test]
    fn install_converts_consumingly_before_any_map_install() {
        let mut workspace =
            GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        // Synthetic FULL statuses isolate the consuming conversion seam without
        // invoking the real light/FEATURES boundaries. Leave one target at SPAWN
        // so conversion fails after earlier targets have already converted.
        for holder in workspace.holders.values_mut() {
            holder.proto_mut().set_persisted_status(ChunkStatus::Full);
        }
        let failing = ChunkPos::ZERO;
        workspace
            .holders
            .get_mut(&failing)
            .expect("center target")
            .proto_mut()
            .set_persisted_status(ChunkStatus::Spawn);

        let mut chunk_map = ChunkMap::empty(GENERATED_VIEW_DISTANCE);
        let error = workspace
            .install_into(&mut chunk_map)
            .expect_err("the pre-FULL target must refuse conversion");
        println!("conversion error: {error:?}");
        assert!(matches!(
            error,
            GeneratedWorkspaceError::Conversion {
                position,
                source: GeneratedChunkError::Convert(
                    crate::server::level::level_chunk::LevelChunkBridgeError::NotFull(
                        ChunkStatus::Spawn
                    )
                ),
            } if position == failing
        ));
        assert_eq!(
            chunk_map.len(),
            0,
            "conversion failure must install nothing"
        );
    }

    #[test]
    fn wrapped_extreme_center_is_typed_refusal_not_empty_success() {
        for center in [
            ChunkPos::new(i32::MAX, i32::MIN),
            ChunkPos::new(i32::MAX, 0),
            ChunkPos::new(i32::MIN, 0),
            ChunkPos::new(0, i32::MAX),
            ChunkPos::new(0, i32::MIN),
        ] {
            let error = match GeneratedWorkspace::new(42, center) {
                Err(error) => error,
                Ok(_) => panic!("pathological wrapped view must be refused at {center}"),
            };
            assert!(matches!(
                error,
                GeneratedWorkspaceError::InvalidTargetView {
                    actual_count: 0,
                    expected_count: 117,
                    ..
                }
            ));
        }
    }

    #[test]
    fn workspace_does_not_claim_boot_support_without_completed_features() {
        // The constructor is intentionally value-only and does not install a
        // placeholder. The live seed-42 FEATURES breadth remains a typed
        // boundary, so boot integration must not silently turn this into FULL.
        let workspace = GeneratedWorkspace::new(42, ChunkPos::ZERO).expect("normal target view");
        assert_eq!(workspace.target_positions().len(), 117);
        assert_eq!(
            workspace.support_positions().len(),
            workspace.required_status.len()
        );
    }
}

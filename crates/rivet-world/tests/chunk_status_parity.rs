//! Exact parity tests for the `chunk::status` value-layer pyramid (issue #185,
//! A1 slice) against the Paper 26.2 builder calls — spec §3.2 (accumulated
//! dependency tables) and §3.5 (access radii) of `docs/chunk-pipeline-spec.md`.
//!
//! Unlike the unit tests inside the `status` module (which can see private
//! internals), this integration target locks the *public* `ChunkPyramid` /
//! `ChunkStep` / `ChunkStatus` surface against the exact values the Java
//! `ChunkPyramid` static builder and the `ChunkTaskScheduler.getAccessRadius0`
//! recursion produce. Any drift in the builder calls, the max-merge, the
//! accumulated fold, or the byRadius walk fails here.
//!
//! Ground truth (Paper 26.2, verified against the Java builder):
//! - FULL accumulated = [SPAWN, INIT_LIGHT, CARVERS, BIOMES, SS x8] (radius 11)
//! - NOISE accumulated = [BIOMES, BIOMES, SS x8] (radius 9)
//! - access radii = [0,0,8,8,9,9,9,10,10,11,11,11]

use rivet_world::chunk::status::{
    ChunkPyramid, ChunkStatus, ChunkStatusTask, GENERATION_PYRAMID, LOADING_PYRAMID,
};

/// Spec §3.5 access radii, in status order: EMPTY 0, SS 0, SR 8, BIOMES 8,
/// NOISE 9, SURFACE 9, CARVERS 9, FEATURES 10, INIT_LIGHT 10, LIGHT 11,
/// SPAWN 11, FULL 11.
const EXPECTED_ACCESS_RADII: [i32; 12] = [0, 0, 8, 8, 9, 9, 9, 10, 10, 11, 11, 11];

#[test]
fn access_radius_table_matches_paper() {
    for (i, status) in ChunkStatus::ALL.iter().enumerate() {
        assert_eq!(
            ChunkPyramid::access_radius(*status),
            EXPECTED_ACCESS_RADII[i],
            "access radius of {status:?}"
        );
    }
    assert_eq!(ChunkPyramid::max_access_radius(), 11);
}

#[test]
fn full_step_accumulated_dependencies_match_paper() {
    let full = GENERATION_PYRAMID.get_step_to(ChunkStatus::Full);
    assert_eq!(full.get_accumulated_radius_of(ChunkStatus::Empty), 11);
    assert_eq!(
        full.accumulated_dependencies().as_list(),
        &[
            ChunkStatus::Spawn,
            ChunkStatus::InitializeLight,
            ChunkStatus::Carvers,
            ChunkStatus::Biomes,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
        ]
    );
    // byRadius[0] is the parent (SPAWN), never the target itself.
    assert_eq!(full.required_status_at_radius(0), ChunkStatus::Spawn);
    assert!(
        !full
            .accumulated_dependencies()
            .as_list()
            .contains(&ChunkStatus::Full)
    );
}

#[test]
fn noise_step_accumulated_and_by_radius_match_paper() {
    let noise = GENERATION_PYRAMID.get_step_to(ChunkStatus::Noise);
    assert_eq!(noise.get_accumulated_radius_of(ChunkStatus::Empty), 9);
    assert_eq!(
        noise.accumulated_dependencies().as_list(),
        &[
            ChunkStatus::Biomes,
            ChunkStatus::Biomes,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
        ]
    );
    // BIOMES is required at 0..=1; STRUCTURE_STARTS fills 2..=8.
    assert_eq!(noise.required_status_at_radius(0), ChunkStatus::Biomes);
    assert_eq!(noise.required_status_at_radius(1), ChunkStatus::Biomes);
    assert_eq!(
        noise.required_status_at_radius(2),
        ChunkStatus::StructureStarts
    );
    assert_eq!(
        noise.required_status_at_radius(9),
        ChunkStatus::StructureStarts
    );
}

#[test]
fn direct_dependencies_and_write_radii_match_paper() {
    let noise = GENERATION_PYRAMID.get_step_to(ChunkStatus::Noise);
    // Builder: with_parent(BIOMES) then addRequirement(SS, 8) + addRequirement(BIOMES, 1).
    assert_eq!(
        noise.direct_dependencies().as_list(),
        &[
            ChunkStatus::Biomes,
            ChunkStatus::Biomes,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
            ChunkStatus::StructureStarts,
        ]
    );
    // blockStateWriteRadius: NOISE/SURFACE/CARVERS = 0, FEATURES = 1, else -1.
    for status in ChunkStatus::ALL {
        let want = match status {
            ChunkStatus::Noise | ChunkStatus::Surface | ChunkStatus::Carvers => 0,
            ChunkStatus::Features => 1,
            _ => -1,
        };
        assert_eq!(
            GENERATION_PYRAMID
                .get_step_to(status)
                .block_state_write_radius(),
            want,
            "write radius of {status:?}"
        );
    }
}

#[test]
fn generation_task_identities_match_paper() {
    let expected = [
        ChunkStatusTask::PassThrough,
        ChunkStatusTask::GenerateStructureStarts,
        ChunkStatusTask::GenerateStructureReferences,
        ChunkStatusTask::GenerateBiomes,
        ChunkStatusTask::GenerateNoise,
        ChunkStatusTask::GenerateSurface,
        ChunkStatusTask::GenerateCarvers,
        ChunkStatusTask::GenerateFeatures,
        ChunkStatusTask::InitializeLight,
        ChunkStatusTask::Light,
        ChunkStatusTask::GenerateSpawn,
        ChunkStatusTask::Full,
    ];
    for (i, status) in ChunkStatus::ALL.iter().enumerate() {
        assert_eq!(
            GENERATION_PYRAMID.get_step_to(*status).task(),
            expected[i],
            "task of {status:?}"
        );
    }
}

#[test]
fn loading_pyramid_is_all_zero_radius() {
    for step in LOADING_PYRAMID.steps() {
        assert_eq!(step.get_accumulated_radius_of(ChunkStatus::Empty), 0);
        assert_eq!(
            step.required_status_at_radius(0),
            step.target_status().parent()
        );
    }
}

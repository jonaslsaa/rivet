//! Port of `net.minecraft.server.level.ChunkLevel` (MC 26.2, Paper) — the
//! level↔status value layer.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/ChunkLevel.java`.
//!
//! Owned by the `mc.server.level.pipeline.level` manifest unit (#185): the
//! value layer (constants + the level→status / status→level mappings) every
//! pipeline class reads.
//!
//! `ChunkLevel` is where the level numbers meet the chunk-status ladder. The
//! only external dependency is `ChunkStatus` (`rivet-world::chunk::status`,
//! the 12-rung 26.2 ladder ported ahead of `mc.world.level.chunk.status`).

use rivet_world::chunk::status::ChunkStatus;

use super::FullChunkStatus;

/// `ChunkLevel.FULL_CHUNK_LEVEL` — the level at which a chunk is fully loaded.
pub const FULL_CHUNK_LEVEL: i32 = 33;
/// `ChunkLevel.BLOCK_TICKING_LEVEL` — the level at which a chunk block-ticks.
pub const BLOCK_TICKING_LEVEL: i32 = 32;
/// `ChunkLevel.ENTITY_TICKING_LEVEL` — the level at which a chunk entity-ticks.
pub const ENTITY_TICKING_LEVEL: i32 = 31;
/// `ChunkLevel.MAX_LEVEL` — the highest level a chunk can be loaded at
/// (`FULL_CHUNK_LEVEL + RADIUS_AROUND_FULL_CHUNK`).
pub const MAX_LEVEL: i32 = FULL_CHUNK_LEVEL + RADIUS_AROUND_FULL_CHUNK;

/// RivetTodo(#185): `RADIUS_AROUND_FULL_CHUNK` and the two tables below are the
/// generation pyramid's FULL step, owned by `mc.world.level.chunk.status`
/// (ChunkPyramid/ChunkStep/ChunkDependencies — the parallel chunk-pyramid
/// track). Java computes them as
/// `ChunkPyramid.GENERATION_PYRAMID.getStepTo(ChunkStatus.FULL)
/// .accumulatedDependencies()`. Rather than implement the pyramid here, this
/// seam encodes the derived values for the pinned 26.2 pyramid and verifies
/// them against the Paper golden fixture
/// (`tools/rivet-oracle/fixtures/chunk-level/`, ChunkLevelProbe); replace with
/// the real `ChunkPyramid` when the chunk-pyramid unit lands.
///
/// `RADIUS_AROUND_FULL_CHUNK` — the FULL step's accumulated-dependency radius
/// (the number of levels a chunk can be below FULL and still be in generation
/// range).
pub const RADIUS_AROUND_FULL_CHUNK: i32 = 11;

/// The FULL step's accumulated dependencies by distance: `[distance]` →
/// `ChunkStatus`, for `distance` in `0..=RADIUS_AROUND_FULL_CHUNK`. Java:
/// `FULL_CHUNK_STEP.accumulatedDependencies().get(distance)`.
static FULL_STEP_ACCUMULATED_DEPENDENCIES: [ChunkStatus; 12] = [
    ChunkStatus::Spawn,           // 0
    ChunkStatus::InitializeLight, // 1
    ChunkStatus::Carvers,         // 2
    ChunkStatus::Biomes,          // 3
    ChunkStatus::StructureStarts, // 4
    ChunkStatus::StructureStarts, // 5
    ChunkStatus::StructureStarts, // 6
    ChunkStatus::StructureStarts, // 7
    ChunkStatus::StructureStarts, // 8
    ChunkStatus::StructureStarts, // 9
    ChunkStatus::StructureStarts, // 10
    ChunkStatus::StructureStarts, // 11
];

/// `ChunkStep.getAccumulatedRadiusOf(status)` for the FULL step — the radius
/// at which each status first appears in the FULL step's accumulated
/// dependencies (0 for the statuses whose requirement radius is the FULL chunk
/// itself, up to 11 for EMPTY/STRUCTURE_STARTS at the pyramid's outer edge).
const fn full_step_accumulated_radius_of(status: ChunkStatus) -> i32 {
    match status {
        ChunkStatus::Empty | ChunkStatus::StructureStarts => 11,
        ChunkStatus::StructureReferences | ChunkStatus::Biomes => 3,
        ChunkStatus::Noise | ChunkStatus::Surface | ChunkStatus::Carvers => 2,
        ChunkStatus::Features | ChunkStatus::InitializeLight => 1,
        ChunkStatus::Light | ChunkStatus::Spawn | ChunkStatus::Full => 0,
    }
}

/// `ChunkLevel.generationStatus(int level)` — the generation status a chunk at
/// `level` must have, or `None` (Java `null`) when `level` is past the
/// generation radius.
pub fn generation_status(level: i32) -> Option<ChunkStatus> {
    get_status_around_full_chunk_with_default(level.wrapping_sub(FULL_CHUNK_LEVEL), None)
}

/// `ChunkLevel.getStatusAroundFullChunk(int distanceToFullChunk, @Nullable
/// ChunkStatus defaultValue)` — the status required at `distanceToFullChunk`
/// levels from a full chunk. Distances past `RADIUS_AROUND_FULL_CHUNK` return
/// `defaultValue`; `<= 0` is already full; otherwise the FULL step's
/// accumulated dependency at that distance.
pub fn get_status_around_full_chunk_with_default(
    distance_to_full_chunk: i32,
    default_value: Option<ChunkStatus>,
) -> Option<ChunkStatus> {
    if distance_to_full_chunk > RADIUS_AROUND_FULL_CHUNK {
        default_value
    } else if distance_to_full_chunk <= 0 {
        Some(ChunkStatus::Full)
    } else {
        Some(FULL_STEP_ACCUMULATED_DEPENDENCIES[distance_to_full_chunk as usize])
    }
}

/// `ChunkLevel.getStatusAroundFullChunk(int distanceToFullChunk)` — the
/// single-arg overload defaulting to `ChunkStatus.EMPTY`.
pub fn get_status_around_full_chunk(distance_to_full_chunk: i32) -> ChunkStatus {
    get_status_around_full_chunk_with_default(distance_to_full_chunk, Some(ChunkStatus::Empty))
        .unwrap_or(ChunkStatus::Empty)
}

/// `ChunkLevel.byStatus(ChunkStatus status)` — the minimum level a chunk must
/// be at for `status` to be its generation status.
pub fn by_status(status: ChunkStatus) -> i32 {
    FULL_CHUNK_LEVEL + full_step_accumulated_radius_of(status)
}

/// `ChunkLevel.fullStatus(int level)` — the `FullChunkStatus` ladder rung a
/// chunk at `level` holds.
pub fn full_status(level: i32) -> FullChunkStatus {
    if level <= ENTITY_TICKING_LEVEL {
        FullChunkStatus::EntityTicking
    } else if level <= BLOCK_TICKING_LEVEL {
        FullChunkStatus::BlockTicking
    } else if level <= FULL_CHUNK_LEVEL {
        FullChunkStatus::Full
    } else {
        FullChunkStatus::Inaccessible
    }
}

/// `ChunkLevel.byStatus(FullChunkStatus status)` — the level corresponding to
/// a `FullChunkStatus` ladder rung.
pub fn by_full_status(status: FullChunkStatus) -> i32 {
    match status {
        FullChunkStatus::Inaccessible => MAX_LEVEL,
        FullChunkStatus::Full => FULL_CHUNK_LEVEL,
        FullChunkStatus::BlockTicking => BLOCK_TICKING_LEVEL,
        FullChunkStatus::EntityTicking => ENTITY_TICKING_LEVEL,
    }
}

/// `ChunkLevel.isEntityTicking(int level)`.
pub fn is_entity_ticking(level: i32) -> bool {
    level <= ENTITY_TICKING_LEVEL
}

/// `ChunkLevel.isBlockTicking(int level)`.
pub fn is_block_ticking(level: i32) -> bool {
    level <= BLOCK_TICKING_LEVEL
}

/// `ChunkLevel.isLoaded(int level)` — `level <= MAX_LEVEL`.
pub fn is_loaded(level: i32) -> bool {
    level <= MAX_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;

    /// The Paper-captured golden fixture (`ChunkLevelProbe`, issue #185).
    fn fixture() -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/chunk-level/chunk-level-goldens.json");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "missing Paper ChunkLevelProbe fixture at {}: {e}",
                path.display()
            )
        });
        serde_json::from_slice(&bytes).expect("valid chunk-level-goldens.json")
    }

    /// Parse a `minecraft:*` status name through the persisted-ladder codec.
    fn status(name: &str) -> ChunkStatus {
        ChunkStatus::from_identifier(name).unwrap_or_else(|| panic!("unknown status {name}"))
    }

    fn full_status_from_name(name: &str) -> FullChunkStatus {
        match name {
            "INACCESSIBLE" => FullChunkStatus::Inaccessible,
            "FULL" => FullChunkStatus::Full,
            "BLOCK_TICKING" => FullChunkStatus::BlockTicking,
            "ENTITY_TICKING" => FullChunkStatus::EntityTicking,
            other => panic!("unknown FullChunkStatus {other}"),
        }
    }

    #[test]
    fn constants_match_paper() {
        let fixture = fixture();
        let c = &fixture["constants"];
        assert_eq!(
            FULL_CHUNK_LEVEL,
            c["FULL_CHUNK_LEVEL"].as_i64().unwrap() as i32
        );
        assert_eq!(
            BLOCK_TICKING_LEVEL,
            c["BLOCK_TICKING_LEVEL"].as_i64().unwrap() as i32
        );
        assert_eq!(
            ENTITY_TICKING_LEVEL,
            c["ENTITY_TICKING_LEVEL"].as_i64().unwrap() as i32
        );
        assert_eq!(
            RADIUS_AROUND_FULL_CHUNK,
            c["RADIUS_AROUND_FULL_CHUNK"].as_i64().unwrap() as i32
        );
        assert_eq!(MAX_LEVEL, c["MAX_LEVEL"].as_i64().unwrap() as i32);
        // MAX_LEVEL derives from the radius.
        assert_eq!(MAX_LEVEL, FULL_CHUNK_LEVEL + RADIUS_AROUND_FULL_CHUNK);
    }

    #[test]
    fn by_status_matches_paper() {
        for entry in fixture()["byStatus"].as_array().unwrap() {
            let status = status(entry["status"].as_str().unwrap());
            let expected = entry["level"].as_i64().unwrap() as i32;
            assert_eq!(
                by_status(status),
                expected,
                "by_status({})",
                entry["status"]
            );
        }
    }

    #[test]
    fn generation_status_matches_paper() {
        for entry in fixture()["generationStatus"].as_array().unwrap() {
            let level = entry["level"].as_i64().unwrap() as i32;
            let expected = entry.get("status").map(|s| status(s.as_str().unwrap()));
            assert_eq!(
                generation_status(level),
                expected,
                "generation_status({level})"
            );
        }
    }

    #[test]
    fn status_around_full_chunk_matches_paper() {
        for entry in fixture()["statusAroundFullChunk"].as_array().unwrap() {
            let distance = entry["distance"].as_i64().unwrap() as i32;
            let expected = status(entry["status"].as_str().unwrap());
            assert_eq!(
                get_status_around_full_chunk(distance),
                expected,
                "get_status_around_full_chunk({distance})"
            );
        }
    }

    #[test]
    fn status_around_full_chunk_with_default_matches_paper() {
        for entry in fixture()["statusAroundFullChunkDefault"]
            .as_array()
            .unwrap()
        {
            let distance = entry["distance"].as_i64().unwrap() as i32;
            let expected = status(entry["status"].as_str().unwrap());
            assert_eq!(
                get_status_around_full_chunk_with_default(distance, Some(ChunkStatus::Biomes)),
                Some(expected),
                "get_status_around_full_chunk({distance}, BIOMES)"
            );
        }
    }

    #[test]
    fn by_full_status_and_ordinals_match_paper() {
        for entry in fixture()["byFullStatus"].as_array().unwrap() {
            let fs = full_status_from_name(entry["fullStatus"].as_str().unwrap());
            let ordinal = entry["ordinal"].as_i64().unwrap() as usize;
            let expected_level = entry["level"].as_i64().unwrap() as i32;
            assert_eq!(fs as usize, ordinal, "{} ordinal", entry["fullStatus"]);
            assert_eq!(
                by_full_status(fs),
                expected_level,
                "by_full_status({})",
                entry["fullStatus"]
            );
        }
    }

    #[test]
    fn full_status_matches_paper() {
        for entry in fixture()["fullStatus"].as_array().unwrap() {
            let level = entry["level"].as_i64().unwrap() as i32;
            let expected = full_status_from_name(entry["fullStatus"].as_str().unwrap());
            assert_eq!(full_status(level), expected, "full_status({level})");
        }
    }

    #[test]
    fn predicates_match_paper() {
        for entry in fixture()["predicates"].as_array().unwrap() {
            let level = entry["level"].as_i64().unwrap() as i32;
            assert_eq!(
                is_entity_ticking(level),
                entry["isEntityTicking"].as_bool().unwrap(),
                "is_entity_ticking({level})"
            );
            assert_eq!(
                is_block_ticking(level),
                entry["isBlockTicking"].as_bool().unwrap(),
                "is_block_ticking({level})"
            );
            assert_eq!(
                is_loaded(level),
                entry["isLoaded"].as_bool().unwrap(),
                "is_loaded({level})"
            );
        }
    }

    #[test]
    fn full_chunk_status_ladder_matches_paper() {
        let fixture = fixture();
        let ladder = fixture["fullChunkStatusOrdinals"].as_array().unwrap();
        assert_eq!(ladder.len(), FullChunkStatus::ALL.len());
        for (i, entry) in ladder.iter().enumerate() {
            let fs = full_status_from_name(entry["name"].as_str().unwrap());
            assert_eq!(fs, FullChunkStatus::ALL[i], "ladder position {i}");
            assert_eq!(
                fs as usize,
                entry["ordinal"].as_i64().unwrap() as usize,
                "{} ordinal",
                entry["name"]
            );
        }
        for entry in fixture["fullChunkStatusIsOrAfter"].as_array().unwrap() {
            let this = full_status_from_name(entry["this"].as_str().unwrap());
            let step = full_status_from_name(entry["step"].as_str().unwrap());
            assert_eq!(
                this.is_or_after(step),
                entry["result"].as_bool().unwrap(),
                "{:?}.is_or_after({:?})",
                this,
                step
            );
        }
    }

    /// Hostile bounds that must not panic and must reproduce Paper exactly.
    #[test]
    fn hostile_level_bounds_are_exact() {
        // i32::MIN level wraps in `level - FULL_CHUNK_LEVEL` to a huge positive
        // distance, which is past the radius → None.
        assert_eq!(generation_status(i32::MIN), None);
        // i32::MAX distance is past the radius → EMPTY default.
        assert_eq!(get_status_around_full_chunk(i32::MAX), ChunkStatus::Empty);
        // i32::MIN distance is <= 0 → FULL.
        assert_eq!(get_status_around_full_chunk(i32::MIN), ChunkStatus::Full);
        // i32::MIN level is far below ENTITY_TICKING → entity-ticking; i32::MAX
        // is far above FULL → inaccessible (and not loaded).
        assert_eq!(full_status(i32::MIN), FullChunkStatus::EntityTicking);
        assert_eq!(full_status(i32::MAX), FullChunkStatus::Inaccessible);
        assert!(is_loaded(i32::MIN));
        assert!(!is_loaded(i32::MAX));
    }
}

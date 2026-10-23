use super::super::{
    level_chunk::{BiomeId, LevelChunk, StateId},
    server_level::overworld_dimension,
};
use super::*;
use rivet_nbt::{compound_tag::CompoundTag, tag_parser::parse_compound_fully};
use rivet_registry::{core::BlockPos, generated::block_properties::BlockPropertyId};
use rivet_world::{
    block::Block,
    poi::{POINT_OF_INTEREST_TYPE, bootstrap_poi_types},
};
use serde_json::Value;

fn access() -> RegistryAccess {
    RegistryAccess::from_pairs(vec![(
        ResourceKey::create_registry_key(POINT_OF_INTEREST_TYPE.identifier().clone()),
        Box::new(bootstrap_poi_types()),
    )])
}
fn storage(storage: &Arc<LevelStorageAccess>) -> RegionFileStorage {
    RegionFileStorage::new_with_version(
        RegionStorageInfo::new("probe".into(), overworld_dimension(), "poi".into(), false),
        storage.dimension_path(&overworld_dimension()).join("poi"),
        false,
        RegionFileVersion::VERSION_NONE,
    )
}
fn sections(mixed: bool) -> Vec<LevelChunkSection<StateId, BiomeId>> {
    let source = LevelChunk::new(ChunkPos::ZERO);
    let air = source.get_sections()[4].copy();
    let mut center = air.copy();
    if mixed {
        let bed = Block::from_name("minecraft:red_bed")
            .unwrap()
            .default_block_state();
        let head = BlockPropertyId::Part
            .values()
            .iter()
            .position(|part| *part == "head")
            .unwrap();
        for (x, y, z, state) in [
            (
                1,
                2,
                3,
                bed.set_property(BlockPropertyId::Part, head as u16)
                    .unwrap(),
            ),
            (
                2,
                1,
                3,
                Block::from_name("minecraft:bell")
                    .unwrap()
                    .default_block_state(),
            ),
            (
                2,
                2,
                1,
                Block::from_name("minecraft:composter")
                    .unwrap()
                    .default_block_state(),
            ),
            (1, 1, 2, bed),
        ] {
            center.set_block_state(
                x,
                y,
                z,
                state.id(),
                &|s| BlockState::new(*s).is_air(),
                &|_| false,
                &|_| true,
                &|_| false,
                &|_| false,
            );
        }
    }
    vec![air.copy(), center, air]
}
fn open(storage: Arc<LevelStorageAccess>) -> StoredPoiManager {
    StoredPoiManager::open_with_version(
        storage,
        overworld_dimension(),
        access(),
        -1,
        1,
        false,
        RegionFileVersion::VERSION_NONE,
    )
    .unwrap()
}
fn seed(storage_access: &Arc<LevelStorageAccess>, pos: ChunkPos, mut data: CompoundTag) {
    data.put_int("DataVersion", CURRENT_DATA_VERSION);
    let mut region = storage(storage_access);
    region.write(&pos, Some(data)).unwrap();
    region.close().unwrap();
}

#[test]
fn actual_paper_region_poi_prepare_save_close_reload_and_delete() {
    let mut cases = 0;
    for line in include_str!("../../../../../tools/rivet-oracle/fixtures/poi-storage.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        assert_eq!(row["future_conversion_unchanged"], true);
        cases += 1;
        let root = tempfile::tempdir().unwrap();
        let storage_access = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let pos = ChunkPos::new(-2, 3);
        assert!(storage(&storage_access).read(&pos).unwrap().is_none());
        if row["name"] != "fresh_populated" {
            seed(
                &storage_access,
                pos,
                parse_compound_fully(row["input"].as_str().unwrap()).unwrap(),
            );
        }
        let mut owner = open(Arc::clone(&storage_access));
        owner
            .prepare_chunk(pos, &sections(true), |state| BlockState::new(*state))
            .unwrap();
        assert_eq!(
            owner
                .manager
                .save(pos, |_, error| panic!("{error}"))
                .unwrap()
                .unwrap(),
            parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
        );
        let outcome = owner.save_chunk(pos).unwrap();
        assert_eq!(
            outcome,
            if row["save_outcome"] == "written" {
                PoiSaveOutcome::Written
            } else {
                PoiSaveOutcome::SkippedClean
            }
        );
        assert!(!owner.manager.is_dirty(pos).unwrap());
        owner.flush().unwrap();
        owner.close().unwrap();
        drop(owner);
        let mut region = storage(&storage_access);
        assert_eq!(
            region.read(&pos).unwrap().unwrap(),
            parse_compound_fully(row["reloaded"].as_str().unwrap()).unwrap()
        );
        region.close().unwrap();
        let mut owner = open(Arc::clone(&storage_access));
        owner.load_chunk(pos).unwrap();
        assert_eq!(
            owner
                .manager
                .save(pos, |_, error| panic!("{error}"))
                .unwrap()
                .unwrap(),
            parse_compound_fully(row["reload_state"]["saved"].as_str().unwrap()).unwrap()
        );
        assert!(!owner.manager.is_dirty(pos).unwrap());
        owner.unload_after_save(pos).unwrap();
        assert!(!owner.has_chunk(pos));
        owner.close().unwrap();
        let mut region = storage(&storage_access);
        region.write(&pos, None).unwrap();
        region.flush().unwrap();
        assert!(region.read(&pos).unwrap().is_none());
        region.close().unwrap();
    }
    assert_eq!(cases, 3);
}

#[test]
fn failed_write_keeps_actual_owner_dirty_and_retryable_after_path_repair() {
    let root = tempfile::tempdir().unwrap();
    let storage_access = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let pos = ChunkPos::ZERO;
    let mut owner = open(Arc::clone(&storage_access));
    owner
        .prepare_chunk(pos, &sections(true), |state| BlockState::new(*state))
        .unwrap();
    let poi = storage_access
        .dimension_path(&overworld_dimension())
        .join("poi");
    std::fs::create_dir_all(poi.parent().unwrap()).unwrap();
    std::fs::write(&poi, b"blocks-directory-creation").unwrap();
    assert!(matches!(
        owner.unload_after_save(pos),
        Err(PoiStorageError::Io { .. })
    ));
    assert!(owner.has_chunk(pos));
    assert!(owner.manager.is_dirty(pos).unwrap());
    std::fs::remove_file(&poi).unwrap();
    assert_eq!(owner.save_chunk(pos).unwrap(), PoiSaveOutcome::Written);
    assert!(!owner.manager.is_dirty(pos).unwrap());
    owner.close().unwrap();
    let mut reload = open(storage_access);
    reload.load_chunk(pos).unwrap();
    assert!(
        reload
            .manager
            .acquire_current_record(BlockPos::new(1, 2, 3))
            .unwrap()
    );
}

#[test]
fn read_errors_and_old_versions_do_not_become_absence_and_empty_repair_deletes_real_data() {
    let root = tempfile::tempdir().unwrap();
    let storage_access = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let pos = ChunkPos::ZERO;
    let poi = storage_access
        .dimension_path(&overworld_dimension())
        .join("poi");
    let wrong = poi.join(RegionFileStorage::get_region_file_name(0, 0));
    std::fs::create_dir_all(&wrong).unwrap();
    let mut owner = open(Arc::clone(&storage_access));
    assert!(matches!(
        owner.load_chunk(pos),
        Err(PoiStorageError::Io { .. })
    ));
    assert!(!owner.has_chunk(pos));
    std::fs::remove_dir(&wrong).unwrap();
    owner.load_chunk(pos).unwrap();
    assert!(owner.has_chunk(pos));
    owner.close().unwrap();
    drop(owner);
    let mut region = storage(&storage_access);
    let old = parse_compound_fully("{Sections:{'0':{Records:[]}}}").unwrap();
    region.write(&pos, Some(old)).unwrap();
    region.close().unwrap();
    let mut owner = open(Arc::clone(&storage_access));
    assert!(matches!(
        owner.load_chunk(pos),
        Err(PoiStorageError::DataConversionUnavailable { stored: 1945, .. })
    ));
    assert!(!owner.has_chunk(pos));
    owner.close().unwrap();
    drop(owner);
    seed(
        &storage_access,
        pos,
        parse_compound_fully("{Sections:{'0':{Records:[{pos:[I;1,2,3],type:'home'}]}}}").unwrap(),
    );
    let mut owner = open(Arc::clone(&storage_access));
    owner
        .prepare_chunk(pos, &sections(false), |state| BlockState::new(*state))
        .unwrap();
    assert_eq!(owner.save_chunk(pos).unwrap(), PoiSaveOutcome::Deleted);
    assert!(!owner.manager.is_dirty(pos).unwrap());
    owner.close().unwrap();
    let mut region = storage(&storage_access);
    assert!(region.read(&pos).unwrap().is_none());
    let future = parse_compound_fully("{DataVersion:2147483647,Sections:{'0':{Valid:1b,Records:[{pos:[I;1,2,3],type:'home',free_tickets:0}]}}}").unwrap();
    region.write(&pos, Some(future)).unwrap();
    region.close().unwrap();
    let mut future_owner = open(storage_access);
    future_owner.load_chunk(pos).unwrap();
    assert!(!future_owner.manager.is_dirty(pos).unwrap());
    assert_eq!(
        future_owner
            .manager
            .sections_to_village(SectionPos::of_chunk_pos(&pos, 0)),
        0
    );
}

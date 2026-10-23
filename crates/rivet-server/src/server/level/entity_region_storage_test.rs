use super::*;
use crate::server::level::{
    entity_construction::{EntityConstructionRuntime, StructureMinecartFactory},
    entity_id_allocator::{EntityIdAllocator, EntityIdOccupancy},
    entity_lookup::{EntityLifecycle, EntityLifecycleEvent, EntityLookupError},
    generation_creature_runtime::WorldGenerationCreatureFactory,
    overworld_dimension,
    structure_entity_loading::{
        load_native_world_entity, load_native_world_entity_root, require_supported_passengers,
    },
};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::{Identifier, RegistryAccess};
use rivet_util::{RandomSource, mth::Uuid};
use rivet_world::{
    data::worldgen::worldgen_bootstraps::build_worldgen_registries, flag::feature_flags,
};
use serde_json::Value;

const WORLD: Uuid = Uuid {
    most: 0x1234_5678_9abc_def0,
    least: 0xfedc_ba98_7654_3210_u64 as i64,
};
fn pos() -> ChunkPos {
    ChunkPos::new(-3, 7)
}
struct Factories {
    random: EntityConstructionRuntime,
    ids: EntityIdAllocator,
    carts: StructureMinecartFactory,
    animals: WorldGenerationCreatureFactory,
}
fn factories(access: &RegistryAccess, occupancy: EntityIdOccupancy) -> Factories {
    let mut ids = EntityIdAllocator::new();
    for _ in 0..100 {
        ids.next_id();
    }
    let runtime = EntityConstructionRuntime::with_seed(ids.sequence(), 42);
    let cart_occupancy = occupancy.clone();
    let carts = StructureMinecartFactory::new(
        runtime.clone(),
        WORLD,
        feature_flags::vanilla_set(),
        Arc::new(move |id| cart_occupancy.contains(id)),
    );
    let animals = WorldGenerationCreatureFactory::new(
        runtime.clone(),
        WORLD,
        feature_flags::vanilla_set(),
        access,
        Arc::new(move |id| occupancy.contains(id)),
    );
    Factories {
        random: runtime,
        ids,
        carts,
        animals,
    }
}
fn native(
    input: &str,
    factories: &Factories,
    access: &RegistryAccess,
) -> Option<NativeLoadedEntity> {
    load_native_world_entity(
        &parse_compound_fully(input).unwrap(),
        &factories.carts,
        &factories.animals,
        access,
    )
    .unwrap()
}
fn limits(value: &Value) -> EntitySaveLimits {
    EntitySaveLimits::from_entries(
        value
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, value)| (Identifier::parse(id), value.as_i64().unwrap() as i32)),
    )
    .unwrap()
}
#[derive(Default)]
struct Lifecycle(Vec<EntityLifecycleEvent>);
impl EntityLifecycle for Lifecycle {
    fn external_uuid_in_use(&self, _: &Uuid) -> bool {
        false
    }
    fn on_event(
        &mut self,
        event: EntityLifecycleEvent,
        _: &NativeLoadedEntity,
    ) -> Result<(), EntityLookupError> {
        self.0.push(event);
        Ok(())
    }
}
fn first_row() -> Value {
    serde_json::from_str(
        include_str!("../../../../../tools/rivet-oracle/fixtures/entity-region.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap()
}
fn open(storage: Arc<LevelStorageAccess>) -> StoredEntityRegion {
    StoredEntityRegion::open_with_version(
        storage,
        overworld_dimension(),
        false,
        EntitySaveLimits::default(),
        RegionFileVersion::VERSION_NONE,
    )
}

#[test]
fn actual_paper_entity_chunk_save_read_limits_and_transient_copy() {
    let access = build_worldgen_registries();
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/entity-region.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        match row["mode"].as_str().unwrap() {
            "save" => {
                let factories = factories(&access, EntityIdOccupancy::default());
                let entities: Vec<_> = row["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|input| native(input.as_str().unwrap(), &factories, &access))
                    .collect();
                let saved = serialize_entity_chunk(
                    &entities,
                    pos(),
                    &limits(&row["limits"]),
                    row["force"].as_bool().unwrap(),
                );
                let expected = row["saved"]
                    .as_str()
                    .map(|tag| parse_compound_fully(tag).unwrap());
                assert_eq!(saved, expected, "{}", row["name"]);
            }
            "read" => {
                let mut factories = factories(&access, EntityIdOccupancy::default());
                let input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
                let mut counts = HashMap::new();
                let limits = limits(&row["limits"]);
                let mut saved = Vec::new();
                for tag in input
                    .get_list("Entities")
                    .into_iter()
                    .flat_map(|list| &list.list)
                {
                    let Tag::Compound(tag) = tag else {
                        continue;
                    };
                    if let Some(entity) = load_native_world_entity_root(
                        tag,
                        &factories.carts,
                        &factories.animals,
                        &access,
                    )
                    .unwrap()
                        && limits.accepts(entity.encode_id(), &mut counts)
                    {
                        require_supported_passengers(tag).unwrap();
                        saved.push(entity.save().unwrap());
                    }
                }
                let expected: Vec<_> = row["saved"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|tag| parse_compound_fully(tag.as_str().unwrap()).unwrap())
                    .collect();
                assert_eq!(saved, expected, "{}", row["name"]);
                assert_eq!(
                    factories.ids.next_id() - 1,
                    row["next_id"].as_i64().unwrap() as i32,
                    "constructor count {}",
                    row["name"]
                );
                if let Some(expected) = row["rng_next"].as_i64() {
                    assert_eq!(
                        factories
                            .random
                            .with_entity_random(|random| random.next_long())
                            .unwrap(),
                        expected
                    );
                }
            }
            "copy" => {
                let from = row["from"]
                    .as_str()
                    .map(|tag| parse_compound_fully(tag).unwrap());
                let mut into = parse_compound_fully(row["into"].as_str().unwrap()).unwrap();
                copy_entities(from.as_ref(), &mut into);
                assert_eq!(
                    into,
                    parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
                );
            }
            _ => unreachable!(),
        }
        cases += 1;
    }
    assert_eq!(cases, 16);
}

#[test]
fn proto_load_limits_skip_passengers_only_after_real_root_construction() {
    use crate::server::level::entity_post_load::{
        DetachedAnimalRemoval, DuplicateUuidMode, DuplicateUuidSettings, EntityPostLoadContext,
        prepare_native_entities,
    };

    let access = build_worldgen_registries();
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/entity-region.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        if !row["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("root_limit_"))
        {
            continue;
        }
        let mut factories = factories(&access, EntityIdOccupancy::default());
        let input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
        let inputs: Vec<_> = input
            .get_list("Entities")
            .unwrap()
            .list
            .iter()
            .map(|tag| {
                let Tag::Compound(tag) = tag else {
                    panic!("compound fixture root")
                };
                tag.clone()
            })
            .collect();
        let limits = limits(&row["limits"]);
        let mut removal = DetachedAnimalRemoval;
        let mut context = EntityPostLoadContext {
            save_limits: &limits,
            cart_factory: &factories.carts,
            creature_factory: &factories.animals,
            registries: &access,
            settings: DuplicateUuidSettings {
                mode: DuplicateUuidMode::Nothing,
                safe_regen_delete_range: 32,
            },
            removal: &mut removal,
        };
        let batch = prepare_native_entities(
            &inputs,
            &mut context,
            |_| panic!("NOTHING does not query UUIDs"),
            || panic!("NOTHING does not regenerate UUIDs"),
        )
        .unwrap();
        let saved: Vec<_> = batch
            .entities
            .iter()
            .map(|entity| entity.save().unwrap())
            .collect();
        let expected: Vec<_> = row["saved"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| parse_compound_fully(tag.as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(saved, expected, "{}", row["name"]);
        assert_eq!(
            factories.ids.next_id() - 1,
            row["next_id"].as_i64().unwrap() as i32
        );
        assert_eq!(
            factories
                .random
                .with_entity_random(|random| random.next_long())
                .unwrap(),
            row["rng_next"].as_i64().unwrap()
        );
        cases += 1;
    }
    assert_eq!(cases, 2);
}

#[test]
fn native_entities_roundtrip_actual_entity_region_and_disk_admission_is_not_created() {
    let root = tempfile::tempdir().unwrap();
    let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let access = build_worldgen_registries();
    let occupancy = EntityIdOccupancy::default();
    let factories = self::factories(&access, occupancy.clone());
    let row = first_row();
    let mut lookup = EntityLookup::new(-4, 19, occupancy);
    for input in row["inputs"].as_array().unwrap() {
        lookup
            .add_entity(
                native(input.as_str().unwrap(), &factories, &access).unwrap(),
                false,
                &mut Lifecycle::default(),
            )
            .unwrap();
    }
    let mut owner = open(Arc::clone(&storage));
    assert_eq!(
        owner.save_chunk(pos(), &lookup, false).unwrap(),
        EntitySaveOutcome::SkippedTransient
    );
    assert_eq!(
        owner.save_chunk(pos(), &lookup, true).unwrap(),
        EntitySaveOutcome::Written
    );
    assert_eq!(
        owner.read_raw(pos()).unwrap().unwrap(),
        parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
    );
    owner.close().unwrap();
    drop(owner);
    drop(lookup);
    let mut owner = open(Arc::clone(&storage));
    owner.prepare_disk_load(pos()).unwrap();
    assert_eq!(owner.readiness(pos()), EntityChunkReadiness::Prepared);
    let occupancy = EntityIdOccupancy::default();
    let factories = self::factories(&access, occupancy.clone());
    let mut lookup = EntityLookup::new(-4, 19, occupancy);
    owner.begin_native_load(pos());
    for tag in owner.chunks[&pos()]
        .input
        .as_ref()
        .unwrap()
        .get_list("Entities")
        .unwrap()
        .list
        .clone()
    {
        let Tag::Compound(tag) = tag else {
            unreachable!()
        };
        owner.prepared_entities_mut(pos()).unwrap().push(
            load_native_world_entity(&tag, &factories.carts, &factories.animals, &access)
                .unwrap()
                .unwrap(),
        );
    }
    let mut events = Lifecycle::default();
    lookup
        .add_entity_chunk_entities(
            owner.prepared_entities_mut(pos()).unwrap(),
            pos(),
            &mut events,
        )
        .unwrap();
    owner.finish_load(pos());
    assert!(
        events.0.is_empty(),
        "fromDisk suppresses Created on hidden admission"
    );
    assert_eq!(lookup.chunk_ids(pos()).len(), 5);
    assert_eq!(
        owner.save_chunk(pos(), &lookup, false).unwrap(),
        EntitySaveOutcome::Written
    );
    assert_eq!(
        owner.read_raw(pos()).unwrap().unwrap(),
        parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
    );
    assert!(
        storage
            .dimension_path(&overworld_dimension())
            .join("entities/r.-1.0.mca")
            .is_file()
    );
    assert!(
        !storage
            .dimension_path(&overworld_dimension())
            .join("region")
            .exists(),
        "live entities never enter chunk-data region files"
    );
}

#[test]
fn failed_write_preserves_native_owner_and_retries_only_after_path_repair() {
    let root = tempfile::tempdir().unwrap();
    let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let access = build_worldgen_registries();
    let occupancy = EntityIdOccupancy::default();
    let factories = self::factories(&access, occupancy.clone());
    let row = first_row();
    let entity = native(row["inputs"][0].as_str().unwrap(), &factories, &access).unwrap();
    let id = entity.entity_id();
    let uuid = entity.uuid();
    let mut lookup = EntityLookup::new(-4, 19, occupancy.clone());
    lookup
        .add_entity(entity, false, &mut Lifecycle::default())
        .unwrap();
    let mut owner = open(Arc::clone(&storage));
    let folder = storage
        .dimension_path(&overworld_dimension())
        .join("entities");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::fs::write(&folder, b"not-a-directory").unwrap();
    assert!(matches!(
        owner.save_chunk(pos(), &lookup, true),
        Err(EntityRegionError::Io { .. })
    ));
    assert_eq!(lookup.get_raw_by_uuid(&uuid).unwrap().entity_id(), id);
    assert!(occupancy.contains(id));
    assert_eq!(owner.readiness(pos()), EntityChunkReadiness::Transient);
    std::fs::remove_file(&folder).unwrap();
    assert_eq!(
        owner.save_chunk(pos(), &lookup, true).unwrap(),
        EntitySaveOutcome::Written
    );
    assert_eq!(
        owner
            .read_raw(pos())
            .unwrap()
            .unwrap()
            .get_list("Entities")
            .unwrap()
            .list
            .len(),
        1
    );
}

fn raw_region(storage: &Arc<LevelStorageAccess>) -> RegionFileStorage {
    RegionFileStorage::new_with_version(
        RegionStorageInfo::new(
            "entity-oracle".into(),
            overworld_dimension(),
            "entities".into(),
            false,
        ),
        storage
            .dimension_path(&overworld_dimension())
            .join("entities"),
        false,
        RegionFileVersion::VERSION_NONE,
    )
}
#[test]
fn entity_region_absence_errors_and_version_boundaries_remain_distinct() {
    let root = tempfile::tempdir().unwrap();
    let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let folder = storage
        .dimension_path(&overworld_dimension())
        .join("entities");
    let path = folder.join("r.-1.0.mca");
    std::fs::create_dir_all(&path).unwrap();
    let mut owner = open(Arc::clone(&storage));
    assert!(matches!(
        owner.prepare_disk_load(pos()),
        Err(EntityRegionError::Io { .. })
    ));
    assert_eq!(owner.readiness(pos()), EntityChunkReadiness::Transient);
    std::fs::remove_dir(&path).unwrap();
    owner.prepare_disk_load(pos()).unwrap();
    assert!(owner.disk_inputs(pos()).unwrap().is_empty());
    owner.finish_load(pos());
    assert_eq!(
        owner
            .save_chunk(
                pos(),
                &EntityLookup::new(-4, 19, EntityIdOccupancy::default()),
                false
            )
            .unwrap(),
        EntitySaveOutcome::Deleted
    );
    assert_eq!(
        owner
            .save_chunk(
                pos(),
                &EntityLookup::new(-4, 19, EntityIdOccupancy::default()),
                false
            )
            .unwrap(),
        EntitySaveOutcome::SkippedEmpty
    );
    owner.close().unwrap();
    drop(owner);
    for version in [
        None,
        Some(CURRENT_DATA_VERSION - 1),
        Some(CURRENT_DATA_VERSION),
        Some(CURRENT_DATA_VERSION + 1),
    ] {
        let mut tag = parse_compound_fully("{Position:[I;999,999],Entities:[]}").unwrap();
        if let Some(version) = version {
            tag.put_int("DataVersion", version);
        }
        let mut disk = raw_region(&storage);
        disk.write_ref(&pos(), Some(&tag)).unwrap();
        disk.close().unwrap();
        drop(disk);
        let mut owner = open(Arc::clone(&storage));
        let loaded = owner.prepare_disk_load(pos());
        if version.is_none_or(|v| v < CURRENT_DATA_VERSION) {
            assert!(matches!(
                loaded,
                Err(EntityRegionError::DataConversionUnavailable { .. })
            ));
            assert_eq!(owner.readiness(pos()), EntityChunkReadiness::Transient);
        } else {
            loaded.unwrap();
            assert!(owner.disk_inputs(pos()).unwrap().is_empty());
            assert_eq!(
                owner.chunks[&pos()].input.as_ref().unwrap(),
                &tag,
                "future data is neither downgraded nor relocated"
            );
        }
        owner.close().unwrap();
    }
}

#[test]
fn transient_unload_prepends_genuine_unknown_disk_entities_without_reencoding_them() {
    let root = tempfile::tempdir().unwrap();
    let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let legacy = parse_compound_fully(
        "{DataVersion:12,Entities:[{id:'old:unported',unknown:[L;3L,4L]}],Position:[I;-3,7]}",
    )
    .unwrap();
    let mut disk = raw_region(&storage);
    disk.write_ref(&pos(), Some(&legacy)).unwrap();
    disk.close().unwrap();
    drop(disk);
    let access = build_worldgen_registries();
    let occupancy = EntityIdOccupancy::default();
    let factories = factories(&access, occupancy.clone());
    let row = first_row();
    let mut lookup = EntityLookup::new(-4, 19, occupancy);
    lookup
        .add_entity(
            native(row["inputs"][0].as_str().unwrap(), &factories, &access).unwrap(),
            false,
            &mut Lifecycle::default(),
        )
        .unwrap();
    let mut owner = open(storage);
    assert_eq!(
        owner.save_chunk(pos(), &lookup, true).unwrap(),
        EntitySaveOutcome::Written
    );
    let saved = owner.read_raw(pos()).unwrap().unwrap();
    let entities = &saved.get_list("Entities").unwrap().list;
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[0], legacy.get_list("Entities").unwrap().list[0]);
    assert!(
        matches!(&entities[1],Tag::Compound(tag) if tag.get_string("id").map(String::as_str)==Some("minecraft:sheep"))
    );
    assert_eq!(saved.get_int("DataVersion"), Some(CURRENT_DATA_VERSION));
}

#[test]
fn actual_world_unload_never_calls_removal_before_ack_and_retains_native_on_failure() {
    use crate::server::level::{
        OverworldGenerator, ServerLevel, ServerLevelConfig,
        entity_tracking::{
            EntityTrackingConfig, TrackingAuthority, TrackingPacket, TrackingPlayer,
            TrackingWorldEvent,
        },
    };
    use rivet_util::random_source::LegacyRandomSource;
    use rivet_world::entity::generation_creature::{
        NativeEntityIdentity, NativeGenerationCreature, NativeSheep,
    };
    struct NoTracking;
    impl TrackingAuthority for NoTracking {
        fn external_uuid_in_use(&self, _: &Uuid) -> bool {
            false
        }
        fn players(
            &mut self,
            _: &NativeLoadedEntity,
        ) -> Result<Vec<TrackingPlayer>, EntityLookupError> {
            panic!("hidden entity cannot require tracking candidates")
        }
        fn observer_order(
            &mut self,
            _: &NativeLoadedEntity,
            _: &[i32],
        ) -> Result<Vec<i32>, EntityLookupError> {
            panic!("no observers")
        }
        fn event(
            &mut self,
            _: TrackingWorldEvent,
            _: &NativeLoadedEntity,
            _: Option<i32>,
        ) -> Result<bool, EntityLookupError> {
            panic!("no source lifecycle transition")
        }
        fn send_bundle(&mut self, _: i32, _: &[TrackingPacket]) -> Result<(), EntityLookupError> {
            panic!("no observers")
        }
        fn send(&mut self, _: i32, _: &TrackingPacket) -> Result<(), EntityLookupError> {
            panic!("no observers")
        }
        fn debug_tracking(
            &mut self,
            _: &NativeLoadedEntity,
            _: Option<i32>,
            _: bool,
        ) -> Result<(), EntityLookupError> {
            panic!("hidden entity")
        }
        fn close_inventory_viewers(
            &mut self,
            _: &NativeLoadedEntity,
        ) -> Result<(), EntityLookupError> {
            panic!("animal is not inventory holder")
        }
    }
    let root = tempfile::tempdir().unwrap();
    let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
    let generator = Arc::new(OverworldGenerator::new(42));
    let access = generator.registry_access().clone();
    let config = ServerLevelConfig {
        is_flat: false,
        sea_level: generator.generator().get_sea_level(),
        missing_chunk_policy: crate::server::level::MissingChunkPolicy::RequireLoaded,
        ..ServerLevelConfig::default()
    };
    let mut level =
        ServerLevel::new_generated_with_storage(config, generator, Arc::clone(&storage)).unwrap();
    let uuid = level.world_uuid().unwrap();
    let occupancy = EntityIdOccupancy::default();
    level.bind_entity_occupancy(occupancy.clone());
    level
        .install_entity_tracking(
            access,
            EntityTrackingConfig {
                world_uuid: uuid,
                animal_range_blocks: 48,
                other_range_blocks: 64,
                animal_range_y: None,
                other_range_y: None,
            },
        )
        .unwrap();
    level
        .open_entity_storage(false, EntitySaveLimits::default())
        .unwrap();
    let mut sheep = NativeSheep::from_identity(
        NativeEntityIdentity {
            entity_id: 1,
            uuid: Uuid { most: 0, least: 7 },
            world_uuid: uuid,
        },
        &mut LegacyRandomSource::new(42),
    );
    sheep.snap_to(rivet_registry::core::Vec3::new(-47.5, 64., 112.5), 90., 0.);
    level
        .entity_lookup_mut()
        .unwrap()
        .add_entity(
            NativeLoadedEntity::Creature(NativeGenerationCreature::Sheep(sheep)),
            false,
            &mut Lifecycle::default(),
        )
        .unwrap();
    let folder = storage
        .dimension_path(&overworld_dimension())
        .join("entities");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::fs::write(&folder, b"block-write").unwrap();
    let mut calls = 0;
    let mut remove = |_: &mut NativeLoadedEntity| {
        calls += 1;
        Err(EntityLookupError::LifecycleUnavailable(
            "actual UNLOAD authority unavailable".into(),
        ))
    };
    let failed = level
        .unload_entity_chunk_after_save(pos(), &mut NoTracking, &mut remove)
        .unwrap_err();
    assert!(failed.removed.is_empty());
    assert!(failed.current.is_none());
    std::fs::remove_file(&folder).unwrap();
    let failed = level
        .unload_entity_chunk_after_save(pos(), &mut NoTracking, &mut remove)
        .unwrap_err();
    assert!(failed.removed.is_empty());
    assert!(failed.current.is_none());
    assert_eq!(calls, 1, "IO failure cannot invoke removal");
    assert!(occupancy.contains(1));
    assert!(level.entity_lookup().unwrap().get_raw(1).is_some());
    assert_eq!(
        level.entity_storage().unwrap().readiness(pos()),
        EntityChunkReadiness::Incomplete
    );
    assert_eq!(
        level
            .entity_storage_mut()
            .unwrap()
            .read_raw(pos())
            .unwrap()
            .unwrap()
            .get_list("Entities")
            .unwrap()
            .list
            .len(),
        1,
        "acknowledged copy persists before removal refusal"
    );

    // A later removal hook panic must not drop the successfully unindexed
    // prefix. The current and untouched suffix still belong to the lookup.
    let generator = Arc::new(OverworldGenerator::new(42));
    let access = generator.registry_access().clone();
    let mut level = ServerLevel::new_generated_with_storage(
        ServerLevelConfig {
            is_flat: false,
            sea_level: generator.generator().get_sea_level(),
            missing_chunk_policy: crate::server::level::MissingChunkPolicy::RequireLoaded,
            ..ServerLevelConfig::default()
        },
        generator,
        storage,
    )
    .unwrap();
    let occupancy = EntityIdOccupancy::default();
    level.bind_entity_occupancy(occupancy.clone());
    level
        .install_entity_tracking(
            access,
            EntityTrackingConfig {
                world_uuid: level.world_uuid().unwrap(),
                animal_range_blocks: 48,
                other_range_blocks: 64,
                animal_range_y: None,
                other_range_y: None,
            },
        )
        .unwrap();
    level
        .open_entity_storage(false, EntitySaveLimits::default())
        .unwrap();
    for id in 1..=3 {
        let mut sheep = NativeSheep::from_identity(
            NativeEntityIdentity {
                entity_id: id,
                uuid: Uuid {
                    most: 0,
                    least: i64::from(id),
                },
                world_uuid: level.world_uuid().unwrap(),
            },
            &mut LegacyRandomSource::new(42),
        );
        sheep.snap_to(rivet_registry::core::Vec3::new(-47.5, 64., 112.5), 90., 0.);
        level
            .entity_lookup_mut()
            .unwrap()
            .add_entity(
                NativeLoadedEntity::Creature(NativeGenerationCreature::Sheep(sheep)),
                false,
                &mut Lifecycle::default(),
            )
            .unwrap();
    }
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = level.unload_entity_chunk_after_save(pos(), &mut NoTracking, &mut |entity| {
            if entity.entity_id() == 2 {
                std::panic::panic_any(732_i32);
            }
            entity.discard();
            Ok(())
        });
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<i32>(), Some(&732));
    let lookup = level.entity_lookup().unwrap();
    assert_eq!(
        lookup
            .interrupted_removals()
            .iter()
            .map(NativeLoadedEntity::entity_id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert!(lookup.interrupted_removals()[0].is_removed());
    assert!(lookup.get_raw(1).is_none());
    assert!(lookup.get_raw(2).is_some());
    assert!(lookup.get_raw(3).is_some());
    assert!(!occupancy.contains(1));
    assert!(occupancy.contains(2));
    assert!(occupancy.contains(3));
    assert_eq!(
        level.entity_storage().unwrap().readiness(pos()),
        EntityChunkReadiness::Incomplete
    );
    let saved = level
        .entity_storage_mut()
        .unwrap()
        .read_raw(pos())
        .unwrap()
        .unwrap();
    let saved_entities = &saved.get_list("Entities").unwrap().list;
    // This second level has transient slices over the first level's saved
    // population. Source unload prepends that untouched disk entity rather
    // than silently replacing it with the three newly constructed roots.
    assert_eq!(saved_entities.len(), 4);
    let uuids: Vec<_> = saved_entities
        .iter()
        .map(|tag| {
            let Tag::Compound(tag) = tag else {
                panic!("saved native entity compound")
            };
            tag.get_int_array("UUID").unwrap().clone()
        })
        .collect();
    assert_eq!(
        uuids,
        vec![
            vec![0, 0, 0, 7],
            vec![0, 0, 0, 1],
            vec![0, 0, 0, 2],
            vec![0, 0, 0, 3]
        ]
    );
}

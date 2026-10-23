//! Genuine Moonrise lookup commands against native owned Rust entities.
use super::*;
use rivet_registry::core::Vec3;
use rivet_util::random_source::LegacyRandomSource;
use rivet_world::entity::{
    farm_variants::{FarmFamily, FarmVariantRegistries},
    generation_creature::{
        NativeEntityIdentity, NativeFarmAnimal, NativeGenerationCreature, NativeSheep,
    },
};
use serde_json::{Value, json};
#[derive(Default)]
struct Events {
    events: Vec<Value>,
    throwing: Option<String>,
    panicking: Option<String>,
    external: Vec<Uuid>,
}
impl EntityLifecycle for Events {
    fn external_uuid_in_use(&self, uuid: &Uuid) -> bool {
        self.external.contains(uuid)
    }
    fn on_event(
        &mut self,
        event: EntityLifecycleEvent,
        entity: &NativeLoadedEntity,
    ) -> Result<(), EntityLookupError> {
        let name = format!("{event:?}");
        self.events.push(json!([name, entity.entity_id()]));
        if self.panicking.as_ref() == Some(&name) {
            panic!("source panic sentinel");
        }
        if self.throwing.as_ref() == Some(&name) {
            return Err(EntityLookupError::LifecycleUnavailable(
                "source sentinel".into(),
            ));
        }
        Ok(())
    }
}
fn entity(data: &Value, registries: &FarmVariantRegistries) -> NativeLoadedEntity {
    let identity = NativeEntityIdentity {
        entity_id: data["id"].as_i64().unwrap() as i32,
        uuid: Uuid::from_string(data["uuid"].as_str().unwrap()).unwrap(),
        world_uuid: Uuid { most: 1, least: 2 },
    };
    let mut random = LegacyRandomSource::new(42);
    let mut creature = match data["family"].as_str().unwrap() {
        "sheep" => {
            NativeGenerationCreature::Sheep(NativeSheep::from_identity(identity, &mut random))
        }
        family => {
            let family = match family {
                "cow" => FarmFamily::Cow,
                "pig" => FarmFamily::Pig,
                "chicken" => FarmFamily::Chicken,
                _ => unreachable!(),
            };
            NativeGenerationCreature::Farm(NativeFarmAnimal::from_identity(
                identity,
                registries.family(family),
                &mut random,
            ))
        }
    };
    let pos: [f64; 3] = serde_json::from_value(data["pos"].clone()).unwrap();
    creature.snap_to(Vec3::new(pos[0], pos[1], pos[2]), 0., 0.);
    if data["dead"].as_bool().unwrap() {
        let input = rivet_nbt::tag_parser::parse_compound_fully(&format!(
            "{{Health:0f,Pos:[{}d,{}d,{}d]}}",
            pos[0], pos[1], pos[2]
        ))
        .unwrap();
        creature.load(&input).unwrap();
        assert!(!creature.is_alive());
    }
    let mut entity = NativeLoadedEntity::Creature(creature);
    if data["removed"].as_bool().unwrap() {
        entity.discard();
    }
    entity
}
fn snapshot(lookup: &EntityLookup, ids: &[i32], result: Value) -> Value {
    let mut seen = std::collections::HashSet::new();
    let raw: Vec<_> = ids
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .map(|id| {
            let entity = lookup.get_raw(id);
            let section = lookup.section(id);
            json!([
                id,
                lookup.has_entity(id),
                lookup.get(id).is_some(),
                entity.map(|e| e.uuid().to_string()),
                section.map(|(p, _)| p.x()),
                section.map(|(_, y)| y),
                section.map(|(p, _)| p.z())
            ])
        })
        .collect();
    json!({"result":result,"raw":raw,"accessible":lookup.all().map(NativeLoadedEntity::entity_id).collect::<Vec<_>>(),"tracked":lookup.tracked_ids()})
}
#[test]
fn actual_paper_lookup_admission_visibility_sections_and_callback_failure() {
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/entity-lookup.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        let data = row["entities"].as_array().unwrap();
        let ids: Vec<_> = data
            .iter()
            .map(|e| e["id"].as_i64().unwrap() as i32)
            .collect();
        let mut values: Vec<_> = data
            .iter()
            .map(|data| Some(entity(data, &registries)))
            .collect();
        let occupancy = EntityIdOccupancy::default();
        let mut lookup = EntityLookup::new(-4, 19, occupancy);
        let mut events = Events::default();
        for (step, command) in row["commands"].as_array().unwrap().iter().enumerate() {
            let mut result = Value::Null;
            match command[0].as_str().unwrap() {
                "add" => {
                    let index = command[1].as_u64().unwrap() as usize;
                    result = match lookup.add_entity(
                        values[index].take().unwrap(),
                        command[2].as_bool().unwrap(),
                        &mut events,
                    ) {
                        Ok(Admission::Added(_)) => json!(true),
                        Ok(Admission::Rejected { .. }) => json!(false),
                        Err(_) => json!("error"),
                    };
                }
                "status" => {
                    let pos = ChunkPos::new(
                        command[1].as_i64().unwrap() as i32,
                        command[2].as_i64().unwrap() as i32,
                    );
                    let status = match command[3].as_str().unwrap() {
                        "FULL" => FullChunkStatus::Full,
                        "BLOCK_TICKING" => FullChunkStatus::BlockTicking,
                        "ENTITY_TICKING" => FullChunkStatus::EntityTicking,
                        "INACCESSIBLE" => FullChunkStatus::Inaccessible,
                        _ => unreachable!(),
                    };
                    lookup.set_chunk_status(pos, status, &mut events).unwrap();
                }
                "worldgen" => {
                    let mut selected = command[1]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|i| values[i.as_u64().unwrap() as usize].take().unwrap())
                        .collect();
                    lookup
                        .add_worldgen_chunk_entities(
                            &mut selected,
                            Some(ChunkPos::new(
                                command[2].as_i64().unwrap() as i32,
                                command[3].as_i64().unwrap() as i32,
                            )),
                            &mut events,
                        )
                        .unwrap();
                }
                "remove" => {
                    let id = ids[command[1].as_u64().unwrap() as usize];
                    lookup
                        .remove_with_authority(
                            id,
                            command[2].as_bool().unwrap(),
                            &mut events,
                            |entity| {
                                // Fixture native animals have no effects/equipment/leash/passengers; source setter removes their actual membership.
                                entity.discard();
                                Ok(())
                            },
                        )
                        .unwrap();
                }
                "throw" => events.throwing = Some(command[1].as_str().unwrap().to_owned()),
                _ => unreachable!(),
            }
            assert_eq!(
                snapshot(&lookup, &ids, result),
                row["snapshots"][step],
                "{} step{step}",
                row["name"]
            );
        }
        assert_eq!(
            json!(events.events),
            row["events"],
            "{} lifecycle order",
            row["name"]
        );
        cases += 1;
    }
    assert_eq!(cases, 7);
}

#[test]
fn panic_resets_entity_and_chunk_status_guards_without_rolling_back_effects() {
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let mut lookup = EntityLookup::new(-4, 19, EntityIdOccupancy::default());
    let data = json!({"family":"cow","id":1,"uuid":"00000000-0000-0000-0000-000000000001","pos":[0,64,0],"dead":false,"removed":false});
    let mut events = Events::default();
    lookup
        .add_entity(entity(&data, &registries), false, &mut events)
        .unwrap();
    events.panicking = Some("TrackingStart".into());
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            lookup.set_chunk_status(ChunkPos::ZERO, FullChunkStatus::Full, &mut events)
        }))
        .is_err()
    );
    assert!(
        lookup.get(1).is_some(),
        "committed accessibility survives panic"
    );
    assert_eq!(lookup.tracked_ids(), &[1]);
    assert!(!lookup.by_id[&1].updating);
    assert!(!lookup.chunks[&ChunkPos::ZERO].prevent_status_updates);
    events.panicking = None;
    events.events.clear();
    lookup
        .set_chunk_status(ChunkPos::ZERO, FullChunkStatus::Inaccessible, &mut events)
        .unwrap();
    assert_eq!(events.events, vec![json!(["TrackingEnd", 1])]);
    assert!(lookup.get(1).is_none());
    assert!(lookup.tracked_ids().is_empty());
}

#[test]
fn batch_callback_failure_retains_unprocessed_native_values() {
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let occupancy = EntityIdOccupancy::default();
    let mut lookup = EntityLookup::new(-4, 19, occupancy.clone());
    let data = json!({"family":"cow","id":1,"uuid":"00000000-0000-0000-0000-000000000001","pos":[0,64,0],"dead":false,"removed":false});
    let first = entity(&data, &registries);
    let mut second_data = data;
    second_data["id"] = json!(2);
    second_data["uuid"] = json!("00000000-0000-0000-0000-000000000002");
    let second = entity(&second_data, &registries);
    let second_uuid = second.uuid();
    let mut batch = vec![first, second];
    let mut events = Events {
        throwing: Some("Created".into()),
        ..Events::default()
    };
    assert!(matches!(
        lookup.add_worldgen_chunk_entities(&mut batch, Some(ChunkPos::ZERO), &mut events),
        Err(EntityLookupError::LifecycleUnavailable(_))
    ));
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].entity_id(), 2);
    assert_eq!(batch[0].uuid(), second_uuid);
    assert_eq!(lookup.get_raw(1).unwrap().entity_id(), 1);
    assert!(occupancy.contains(1));
    assert!(!occupancy.contains(2));
    assert!(lookup.get_raw(2).is_none());
    assert_eq!(events.events, vec![json!(["Created", 1])]);
}

#[test]
fn external_players_share_id_occupancy_and_uuid_rollback() {
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let occupancy = EntityIdOccupancy::default();
    assert!(occupancy.register(7));
    let mut lookup = EntityLookup::new(-4, 19, occupancy.clone());
    let data = json!({"family":"cow","id":7,"uuid":"00000000-0000-0000-0000-000000000001","pos":[0,64,0],"dead":false,"removed":false});
    let mut events = Events::default();
    assert!(matches!(
        lookup
            .add_entity(entity(&data, &registries), false, &mut events)
            .unwrap(),
        Admission::Rejected {
            reason: AdmissionRejection::DuplicateId,
            ..
        }
    ));
    assert!(occupancy.contains(7));
    assert!(
        lookup.get_raw(7).is_none(),
        "native lookup must not duplicate session-owned player"
    );
    let mut data = data;
    data["id"] = json!(8);
    events
        .external
        .push(Uuid::from_string(data["uuid"].as_str().unwrap()).unwrap());
    assert!(matches!(
        lookup
            .add_entity(entity(&data, &registries), false, &mut events)
            .unwrap(),
        Admission::Rejected {
            reason: AdmissionRejection::DuplicateUuid,
            ..
        }
    ));
    assert!(
        !occupancy.contains(8),
        "UUID failure rolls back only its new ID"
    );
    assert!(events.events.is_empty());
}

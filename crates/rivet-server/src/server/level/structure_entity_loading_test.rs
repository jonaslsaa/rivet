use super::*;
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_util::{
    RandomSource,
    mth::{Uuid, create_insecure_uuid},
    random_source::LegacyRandomSource,
};
use rivet_world::entity::chest_minecart::EntityConstructionContext;
use std::sync::{
    Mutex,
    atomic::{AtomicI32, Ordering},
};

#[test]
fn actual_paper_animals_load_into_native_values_with_constructor_then_uuid_restoration() {
    use super::super::{
        entity_construction::EntityConstructionRuntime, entity_id_allocator::EntityIdAllocator,
        generation_creature_runtime::WorldGenerationCreatureFactory,
    };
    use rivet_world::{
        data::worldgen::worldgen_bootstraps::build_worldgen_registries, flag::feature_flags,
    };
    use std::sync::Arc;
    let access = build_worldgen_registries();
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/generation-creature-load.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
        let mut players = EntityIdAllocator::new();
        for _ in 0..100 {
            players.next_id();
        }
        let runtime = EntityConstructionRuntime::with_seed(players.sequence(), 42);
        let animals = WorldGenerationCreatureFactory::new(
            runtime.clone(),
            Uuid {
                most: 0x1234_5678_9abc_def0,
                least: 0xfedc_ba98_7654_3210_u64 as i64,
            },
            feature_flags::vanilla_set(),
            &access,
            Arc::new(|_| false),
        );
        let carts = Factory::new(false);
        let entity = load_native_world_entity(&input, &carts, &animals, &access).unwrap();
        let callbacks = row["callbacks"].as_array().unwrap();
        assert_eq!(
            usize::from(entity.is_some()),
            callbacks.len(),
            "{}",
            row["name"]
        );
        if let Some(entity) = entity {
            assert_eq!(
                entity.entity_id(),
                callbacks[0]["id"].as_i64().unwrap() as i32
            );
            assert_eq!(
                entity.uuid().to_string(),
                callbacks[0]["uuid"].as_str().unwrap()
            );
            assert_eq!(
                entity.generation(),
                callbacks[0]["generation"].as_bool().unwrap()
            );
            let old = entity.old_position();
            assert_eq!(
                [old.x, old.y, old.z],
                serde_json::from_value::<[f64; 3]>(row["old_pos"].clone()).unwrap()
            );
            assert_eq!(
                entity.save().is_some(),
                row["save_result"].as_bool().unwrap()
            );
            assert_eq!(
                entity.save().unwrap_or_default(),
                parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
                "{}",
                row["name"]
            );
        }
        assert_eq!(
            players.next_id() - 1,
            row["next_id"].as_i64().unwrap() as i32
        );
        assert_eq!(
            runtime.with_entity_random(|r| r.next_long()).unwrap(),
            row["rng_next"].as_i64().unwrap(),
            "{}",
            row["name"]
        );
        count += 1;
    }
    assert_eq!(count, 170);
}
struct Factory {
    random: Mutex<LegacyRandomSource>,
    last_id: AtomicI32,
    experimental: bool,
    enabled: bool,
}

struct UnavailableCreatures;
impl super::super::generation_creature_runtime::GenerationCreatureFactory for UnavailableCreatures {
    fn world_uuid(&self) -> Uuid {
        Uuid {
            most: 0x1234_5678_9abc_def0,
            least: 0xfedc_ba98_7654_3210_u64 as i64,
        }
    }
    fn require_authority(
        &self,
    ) -> Result<(), super::super::generation_creature_runtime::GenerationCreatureError> {
        Err(super::super::generation_creature_runtime::GenerationCreatureError::AuthorityUnavailable("fixture has no creature factory".into()))
    }
    fn create(
        &self,
        entity_type: &str,
        _: &rivet_registry::RegistryAccess,
    ) -> Result<
        Option<rivet_world::entity::generation_creature::NativeGenerationCreature>,
        super::super::generation_creature_runtime::GenerationCreatureError,
    > {
        Err(
            super::super::generation_creature_runtime::GenerationCreatureError::UnsupportedEntity(
                entity_type.into(),
            ),
        )
    }
}

#[test]
fn actual_paper_duplicate_uuid_stream_policy_and_removal_rng() {
    use super::super::entity_post_load::*;
    use rivet_registry::{Identifier, RegistryAccess, core::Vec3};
    use rivet_world::entity::loaded_entity::NativeLoadedEntity;
    struct RecordingRemoval<'a> {
        inner: EmptyInventoryEntityRemoval<'a, LegacyRandomSource>,
        events: Vec<&'static str>,
    }
    impl EntityRemoval for RecordingRemoval<'_> {
        fn discard(
            &mut self,
            entity: &mut NativeLoadedEntity,
            decision: DuplicateUuidDecision,
        ) -> Result<(), StructureError> {
            let events = &mut self.events;
            self.inner
                .discard_with_event_authority(entity, decision, &mut |entity| {
                    assert!(
                        !entity.is_removed(),
                        "actual event fires before removed flag"
                    );
                    events.push("DISCARD");
                    Ok(())
                })?;
            assert!(entity.is_removed());
            Ok(())
        }
    }
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/entity-post-load-policy.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let mode = match row["mode"].as_str().unwrap() {
            "SAFE_REGEN" => DuplicateUuidMode::SafeRegen,
            "DELETE" => DuplicateUuidMode::Delete,
            "WARN" => DuplicateUuidMode::Warn,
            "NOTHING" => DuplicateUuidMode::Nothing,
            _ => panic!("mode"),
        };
        let situation = row["situation"].as_str().unwrap();
        let count = row["count"].as_u64().unwrap() as usize;
        let input = parse_compound_fully(
            "{id:'minecraft:chest_minecart',UUID:[I;0,1,0,2],Pos:[0d,64d,0d]}",
        )
        .unwrap();
        let inputs = vec![input; count];
        let factory = Factory::new(false);
        let access = RegistryAccess::empty();
        let mut world_random = LegacyRandomSource::new(99);
        let mut removal = RecordingRemoval {
            inner: EmptyInventoryEntityRemoval {
                world_random: &mut world_random,
            },
            events: Vec::new(),
        };
        let mut context = EntityPostLoadContext {
            cart_factory: &factory,
            creature_factory: &UnavailableCreatures,
            registries: &access,
            settings: DuplicateUuidSettings {
                mode,
                safe_regen_delete_range: 32,
            },
            removal: &mut removal,
        };
        let mut lookups = 0;
        let mut secure_calls = 0;
        let batch = prepare_native_entities(
            &inputs,
            &mut context,
            |_| {
                lookups += 1;
                (situation != "absent").then(|| ExistingEntityView {
                    encode_id: Some(Identifier::parse(if situation == "different_type" {
                        "minecraft:minecart"
                    } else {
                        "minecraft:chest_minecart"
                    })),
                    position: Vec3::new(
                        if situation == "far" {
                            40.
                        } else if situation == "boundary" {
                            32.
                        } else {
                            1.
                        },
                        64.,
                        0.,
                    ),
                    is_removed: situation == "removed",
                })
            },
            || {
                secure_calls += 1;
                Ok(Uuid { most: 3, least: 4 })
            },
        )
        .unwrap();
        assert_eq!(
            batch.entities.len(),
            row["accepted"].as_u64().unwrap() as usize
        );
        assert_eq!(lookups, row["lookups"].as_u64().unwrap());
        let decisions:Vec<_>=batch.decisions.iter().map(|d|serde_json::json!({"discard":matches!(d,DuplicateUuidDecision::Discard|DuplicateUuidDecision::DiscardWithoutCause),"removed":matches!(d,DuplicateUuidDecision::Discard|DuplicateUuidDecision::DiscardWithoutCause),"uuid_changed":*d==DuplicateUuidDecision::Regenerated})).collect();
        assert_eq!(
            serde_json::json!(decisions),
            row["decisions"],
            "{mode:?}/{situation}"
        );
        assert_eq!(
            secure_calls,
            batch
                .decisions
                .iter()
                .filter(|d| **d == DuplicateUuidDecision::Regenerated)
                .count()
        );
        for entity in &batch.entities {
            assert!(!entity.is_removed());
            assert_eq!(
                entity.uuid() != Uuid { most: 1, least: 2 },
                batch
                    .decisions
                    .contains(&DuplicateUuidDecision::Regenerated)
            );
        }
        assert_eq!(serde_json::json!(removal.events), row["remove_events"]);
        assert_eq!(
            world_random.next_long(),
            row["world_rng_next"].as_i64().unwrap()
        );
        assert_eq!(
            factory.last_id.load(Ordering::SeqCst),
            row["next_id"].as_i64().unwrap() as i32
        );
        assert_eq!(
            factory.random.lock().unwrap().next_long(),
            row["entity_rng_next"].as_i64().unwrap()
        );
    }
}

#[test]
fn post_load_unavailable_support_preserves_typed_prefix_and_current_removal_value() {
    use super::super::entity_post_load::*;
    use rivet_registry::{Identifier, RegistryAccess, core::Vec3};
    let factory = Factory::new(false);
    let access = RegistryAccess::empty();
    let mut removal = DetachedAnimalRemoval;
    let mut context = EntityPostLoadContext {
        cart_factory: &factory,
        creature_factory: &UnavailableCreatures,
        registries: &access,
        settings: DuplicateUuidSettings {
            mode: DuplicateUuidMode::SafeRegen,
            safe_regen_delete_range: 32,
        },
        removal: &mut removal,
    };
    let input = parse_compound_fully("{id:'minecraft:chest_minecart',UUID:[I;0,1,0,2]}").unwrap();
    let error = prepare_native_entities(
        &[
            input.clone(),
            parse_compound_fully("{id:'minecraft:pig'}").unwrap(),
        ],
        &mut context,
        |_| None,
        || panic!("no duplicate"),
    )
    .unwrap_err();
    assert_eq!(error.input_index, 1);
    assert_eq!(error.prepared.entities.len(), 1);
    assert!(error.current.is_none());
    let error = prepare_native_entities(
        &[input],
        &mut context,
        |_| {
            Some(ExistingEntityView {
                encode_id: Some(Identifier::parse("minecraft:chest_minecart")),
                position: Vec3::new(0., 0., 0.),
                is_removed: false,
            })
        },
        || panic!("near duplicate does not regenerate"),
    )
    .unwrap_err();
    assert!(error.prepared.entities.is_empty());
    let entity = error.current.unwrap();
    assert!(!entity.is_removed());
    assert_eq!(entity.uuid(), Uuid { most: 1, least: 2 });
}
impl Factory {
    fn new(experimental: bool) -> Self {
        Self {
            random: Mutex::new(LegacyRandomSource::new(42)),
            last_id: AtomicI32::new(100),
            experimental,
            enabled: true,
        }
    }
}
impl StructureEntityCreation for Factory {
    fn require_creation_authority(&self) -> Result<(), StructureError> {
        Ok(())
    }
    fn create_chest_minecart(&self) -> Result<Option<NativeChestMinecart>, StructureError> {
        if !self.enabled {
            return Ok(None);
        }
        let uuid = create_insecure_uuid(&mut *self.random.lock().unwrap());
        let id = self.last_id.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(NativeChestMinecart::from_identity(
            EntityConstructionContext {
                entity_id: id,
                world_uuid: Uuid {
                    most: 0x1234_5678_9abc_def0,
                    least: 0xfedc_ba98_7654_3210_u64 as i64,
                },
                experimental_minecart_movement: self.experimental,
            },
            uuid,
        )))
    }
}
#[test]
fn actual_paper_recursive_native_load_callback_full_save_and_identity_consumption() {
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/chest-minecart-load.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let factory = Factory::new(row["experimental"].as_bool().unwrap());
        let input = parse_compound_fully(row["input"].as_str().unwrap()).unwrap();
        let loaded = load_native_entity(&input, &factory).unwrap();
        let callbacks = row["callbacks"].as_array().unwrap();
        assert_eq!(
            usize::from(loaded.is_some()),
            callbacks.len(),
            "{}",
            row["name"]
        );
        if let Some(entity) = loaded {
            assert_eq!(
                entity.entity_id(),
                callbacks[0]["id"].as_i64().unwrap() as i32
            );
            assert_eq!(
                entity.uuid().to_string(),
                callbacks[0]["uuid"].as_str().unwrap()
            );
            assert_eq!(
                entity.generation(),
                callbacks[0]["generation"].as_bool().unwrap()
            );
            assert_eq!(
                entity.save().is_some(),
                row["save_result"].as_bool().unwrap()
            );
            assert_eq!(
                entity.save().unwrap_or_default(),
                parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
                "{}",
                row["name"]
            );
            let old = entity.old_position();
            assert_eq!(
                [old.x, old.y, old.z],
                [
                    row["old_x"].as_f64().unwrap(),
                    row["old_y"].as_f64().unwrap(),
                    row["old_z"].as_f64().unwrap()
                ]
            );
        }
        assert_eq!(
            factory.last_id.load(Ordering::SeqCst),
            row["next_id"].as_i64().unwrap() as i32
        );
        assert_eq!(
            factory.random.lock().unwrap().next_long(),
            row["rng_next"].as_i64().unwrap()
        );
        count += 1;
    }
    assert_eq!(count, 12);
}
#[test]
fn valid_unported_types_and_passengers_refuse_but_real_disabled_gate_skips_without_identity_draws()
{
    let factory = Factory::new(false);
    assert!(matches!(
        load_native_entity(
            &parse_compound_fully("{id:'minecraft:zombie'}").unwrap(),
            &factory
        ),
        Err(StructureError::PlacementUnavailable(_))
    ));
    assert!(matches!(
        load_native_entity(
            &parse_compound_fully(
                "{id:'minecraft:chest_minecart',Passengers:[{id:'minecraft:chest_minecart'}]}"
            )
            .unwrap(),
            &factory
        ),
        Err(StructureError::PlacementUnavailable(_))
    ));
    assert_eq!(
        factory.last_id.load(Ordering::SeqCst),
        101,
        "genuine constructor precedes unsupported passenger load"
    );
    let mut disabled = Factory::new(false);
    disabled.enabled = false;
    assert!(
        load_native_entity(
            &parse_compound_fully("{id:'minecraft:chest_minecart'}").unwrap(),
            &disabled
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(disabled.last_id.load(Ordering::SeqCst), 100);
    assert_eq!(
        disabled.random.lock().unwrap().next_long(),
        LegacyRandomSource::new(42).next_long()
    );
}

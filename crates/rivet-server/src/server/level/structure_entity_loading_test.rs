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
struct Factory {
    random: Mutex<LegacyRandomSource>,
    last_id: AtomicI32,
    experimental: bool,
    enabled: bool,
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

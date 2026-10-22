//! Full saved tags and both RNG continuations from actual pinned constructors.
use super::*;
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_util::random_source::LegacyRandomSource;

#[test]
fn actual_paper_fresh_constructor_loot_and_full_save() {
    for line in include_str!("../../../../tools/rivet-oracle/fixtures/chest-minecart.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut entity_random = LegacyRandomSource::new(row["entity_seed"].as_i64().unwrap());
        let mut feature_random = LegacyRandomSource::new(row["feature_seed"].as_i64().unwrap());
        let experimental = row["experimental"].as_bool().unwrap();
        let mut entity = NativeChestMinecart::new(
            EntityConstructionContext {
                entity_id: row["entity_id"].as_i64().unwrap() as i32,
                world_uuid: Uuid {
                    most: 0x1234_5678_9abc_def0,
                    least: 0xfedc_ba98_7654_3210_u64 as i64,
                },
                experimental_minecart_movement: experimental,
            },
            &mut entity_random,
        );
        entity.set_initial_pos(Vec3::new(-16.5, 50.5, 32.5));
        if row["assign_loot"].as_bool().unwrap() {
            let seed = if row["zero_seed"].as_bool().unwrap() {
                0
            } else {
                feature_random.next_long()
            };
            entity.set_loot_table(
                Identifier::parse("minecraft:chests/abandoned_mineshaft"),
                seed,
            );
        }
        let expected = parse_compound_fully(row["saved"].as_str().unwrap()).unwrap();
        assert_eq!(entity.save().unwrap(), expected, "{line}");
        assert_eq!(
            entity_random.next_long(),
            row["entity_rng_next"].as_i64().unwrap()
        );
        assert_eq!(
            feature_random.next_long(),
            row["feature_rng_next"].as_i64().unwrap()
        );
        assert_eq!(
            entity.movement(),
            if experimental {
                MinecartMovement::Experimental
            } else {
                MinecartMovement::Old
            }
        );
        let expected_bounds: Vec<_> = row["bounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(entity.bounding_box().as_slice(), expected_bounds);
        assert_eq!(
            entity.inventory().len(),
            row["inventory_size"].as_u64().unwrap() as usize
        );
        assert_eq!(
            entity
                .inventory()
                .iter()
                .all(|item| *item == FreshMinecartItemStack::Empty),
            row["inventory_empty"].as_bool().unwrap()
        );
    }
}

#[test]
fn transform_generation_flag_is_transient_and_discard_prevents_save() {
    let mut entity = NativeChestMinecart::new(
        EntityConstructionContext {
            entity_id: 1,
            world_uuid: Uuid { most: 7, least: 9 },
            experimental_minecart_movement: false,
        },
        &mut LegacyRandomSource::new(42),
    );
    let saved = entity.save();
    entity.set_generation(true);
    assert!(entity.generation());
    assert_eq!(entity.save(), saved);
    entity.discard();
    assert!(!entity.is_alive());
    assert_eq!(entity.save(), None);
}

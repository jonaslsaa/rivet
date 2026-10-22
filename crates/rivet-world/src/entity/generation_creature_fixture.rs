use super::*;
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_util::{mth::create_insecure_uuid, random_source::LegacyRandomSource};

#[test]
fn actual_paper_sheep_constructor_group_finalize_full_save_and_two_rng_streams() {
    let mut count = 0;
    let mut babies = 0;
    for line in
        include_str!("../../../../tools/rivet-oracle/fixtures/generation-sheep.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let mut entity_random = LegacyRandomSource::new(42);
        let mut level_random = LegacyRandomSource::new(row["seed"].as_i64().unwrap());
        let climate = match row["biome"].as_str().unwrap() {
            "plains" => SheepClimate::Temperate,
            "desert" => SheepClimate::Warm,
            "snowy_plains" => SheepClimate::Cold,
            _ => unreachable!(),
        };
        let mut group = None;
        let mut actual = None;
        for i in 0..=row["index"].as_i64().unwrap() {
            let identity = NativeEntityIdentity {
                entity_id: 101 + i as i32,
                uuid: create_insecure_uuid(&mut entity_random),
                world_uuid: Uuid {
                    most: 0x1234_5678_9abc_def0,
                    least: 0xfedc_ba98_7654_3210_u64 as i64,
                },
            };
            let mut sheep = NativeSheep::from_identity(identity, &mut entity_random);
            if row["finalize"].as_bool().unwrap() {
                sheep.snap_to(Vec3::new(8.5, 64.0, 8.5), 123.0, 0.0);
                sheep.finalize_spawn(climate, &mut level_random, &mut group);
            }
            actual = Some(sheep);
        }
        let sheep = actual.unwrap();
        assert_eq!(
            sheep.save().unwrap(),
            parse_compound_fully(row["saved"].as_str().unwrap()).unwrap(),
            "{line}"
        );
        assert_eq!(
            sheep.entity().identity().entity_id,
            row["id"].as_i64().unwrap() as i32
        );
        assert_eq!(sheep.is_baby(), row["baby"].as_bool().unwrap());
        assert_eq!(
            entity_random.next_long(),
            row["entity_rng_next"].as_i64().unwrap(),
            "entity stream {line}"
        );
        assert_eq!(
            level_random.next_long(),
            row["level_rng_next"].as_i64().unwrap(),
            "level stream {line}"
        );
        babies += usize::from(sheep.is_baby());
        count += 1;
    }
    assert_eq!(count, 37);
    assert_eq!(babies, 3);
}

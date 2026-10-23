use super::*;
use rivet_registry::{RegistryAccess, holder_lookup::HolderGetter, registry_ops::RegistryOps};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn all_builtin_states_and_native_ticket_transitions_match_paper() {
    let mut catalog = Vec::new();
    let mut record_count = 0;
    let registry = bootstrap_poi_types();
    for line in include_str!("../../../tools/rivet-oracle/fixtures/poi-record.jsonl").lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row["kind"] == "codec" || row["kind"] == "authority" {
            continue;
        }
        let kind = PoiType::by_name(&Identifier::parse(row["name"].as_str().unwrap())).unwrap();
        if row["kind"] == "type" {
            catalog.push(kind);
            assert_eq!(
                kind.max_tickets(),
                row["max_tickets"].as_i64().unwrap() as i32
            );
            assert_eq!(
                kind.valid_range(),
                row["valid_range"].as_i64().unwrap() as i32
            );
            let states: Vec<_> = kind.matching_states().iter().map(|id| id.0).collect();
            let expected: Vec<_> = row["states"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap() as u16)
                .collect();
            assert_eq!(states, expected, "{}", kind.name());
        } else {
            record_count += 1;
            let dirty = Arc::new(AtomicUsize::new(0));
            let callback = Arc::clone(&dirty);
            let mut record = PackedPoiRecord {
                pos: BlockPos::new(-17, 63, 32),
                poi_type: registry
                    .get(&ResourceKey::create(
                        &POINT_OF_INTEREST_TYPE,
                        Identifier::parse(kind.name()),
                    ))
                    .unwrap(),
                free_tickets: row["initial"].as_i64().unwrap() as i32,
            }
            .unpack(
                &registry,
                Arc::new(move || {
                    callback.fetch_add(1, Ordering::Relaxed);
                }),
            );
            for step in row["steps"].as_array().unwrap() {
                let result = if step["action"] == "acquire" {
                    record.acquire_ticket()
                } else {
                    record.release_ticket()
                };
                assert_eq!(result, step["result"].as_bool().unwrap(), "{line}");
                assert_eq!(
                    record.free_tickets(),
                    step["free"].as_i64().unwrap() as i32,
                    "{line}"
                );
                assert_eq!(
                    dirty.load(Ordering::Relaxed),
                    step["dirty"].as_u64().unwrap() as usize,
                    "{line}"
                );
                assert_eq!(
                    record.has_space(),
                    step["space"].as_bool().unwrap(),
                    "{line}"
                );
                assert_eq!(
                    record.is_occupied(),
                    step["occupied"].as_bool().unwrap(),
                    "{line}"
                );
            }
        }
    }
    assert_eq!(catalog, PoiType::all().collect::<Vec<_>>());
    assert_eq!(record_count, 126);
}

#[test]
fn packed_record_codec_matches_actual_registry_backed_paper_codec() {
    use rivet_nbt::{nbt_ops::NbtOps, tag::Tag, tag_parser::parse_compound_fully};
    let ops = RegistryOps::create_from_access(&NbtOps, poi_access());
    let codec = PackedPoiRecord::codec();
    let mut cases = 0;
    for line in include_str!("../../../tools/rivet-oracle/fixtures/poi-record.jsonl").lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row["kind"] != "codec" {
            continue;
        }
        cases += 1;
        let input = Tag::Compound(parse_compound_fully(row["input"].as_str().unwrap()).unwrap());
        let result = codec.parse(&ops, &input);
        assert_eq!(
            result.result().is_some(),
            row["success"].as_bool().unwrap(),
            "{line}"
        );
        let partial = result.result_or_partial_silent();
        if let Some(encoded) = row["encoded"].as_str() {
            let value = partial.expect("actual Paper retains a decoded record");
            assert_eq!(
                codec.encode_start(&ops, &value).result().unwrap(),
                &Tag::Compound(parse_compound_fully(encoded).unwrap()),
                "{line}"
            );
        } else {
            assert!(partial.is_none(), "{line}");
        }
    }
    assert_eq!(cases, 8);
}

fn poi_access() -> RegistryAccess {
    RegistryAccess::from_pairs(vec![(
        ResourceKey::create_registry_key(POINT_OF_INTEREST_TYPE.identifier().clone()),
        Box::new(bootstrap_poi_types()),
    )])
}

#[test]
fn packed_codec_requires_actual_registry_and_preserves_holder_owner() {
    use rivet_nbt::{nbt_ops::NbtOps, tag::Tag, tag_parser::parse_compound_fully};
    let access = poi_access();
    let registry = access.lookup(&POINT_OF_INTEREST_TYPE).unwrap();
    let holder = registry
        .get(&ResourceKey::create(
            &POINT_OF_INTEREST_TYPE,
            Identifier::parse("minecraft:home"),
        ))
        .unwrap();
    let input = Tag::Compound(parse_compound_fully("{pos:[I;1,2,3],type:'home'}").unwrap());
    let codec = PackedPoiRecord::codec();
    let original = PackedPoiRecord {
        pos: BlockPos::new(1, 2, 3),
        poi_type: holder,
        free_tickets: 0,
    };
    let mut cases = 0;
    for line in include_str!("../../../tools/rivet-oracle/fixtures/poi-record.jsonl").lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row["kind"] != "authority" {
            continue;
        }
        cases += 1;
        // RegistryOpsLookup encodes plain-ops refusal in the Rust type system;
        // an empty RegistryOps exercises the same actual missing-registry path.
        let context = match row["context"].as_str().unwrap() {
            "plain" | "empty" => RegistryOps::create_from_access(&NbtOps, RegistryAccess::empty()),
            "foreign" => RegistryOps::create_from_access(&NbtOps, poi_access()),
            "direct" => RegistryOps::create_from_access(&NbtOps, access.clone()),
            other => panic!("unknown authority fixture {other}"),
        };
        let decode = codec.parse(&context, &input);
        assert_eq!(
            decode.result().is_some(),
            row["decode_success"].as_bool().unwrap(),
            "{line}"
        );
        if let Some(error) = row["decode_error"].as_str() {
            assert_eq!(decode.error_ref().unwrap().message(), error, "{line}");
        }
        let value = if row["context"] == "direct" {
            PackedPoiRecord {
                poi_type: Holder::direct(*original.poi_type.value(registry)),
                ..original.clone()
            }
        } else {
            original.clone()
        };
        let encoded = codec.encode_start(&context, &value);
        assert_eq!(
            encoded.result().is_some(),
            row["encode_success"].as_bool().unwrap(),
            "{line}"
        );
        if let Some(error) = row["encode_error"].as_str() {
            assert_eq!(encoded.error_ref().unwrap().message(), error, "{line}");
        }
    }
    assert_eq!(cases, 4);
}

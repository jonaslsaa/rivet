//! Live Paper command transcripts: shared object identity, both indices,
//! ticket mutations, dirty callbacks, exact short-map/set order and codecs.
use super::*;
use crate::poi::{POINT_OF_INTEREST_TYPE, bootstrap_poi_types};
use rivet_nbt::{nbt_ops::NbtOps, tag::Tag, tag_parser::parse_compound_fully};
use rivet_registry::{
    Identifier, RegistryAccess, ResourceKey, holder_lookup::HolderGetter, registry::Registry,
    registry_ops::RegistryOps,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicUsize, Ordering},
};

struct State {
    section: PoiSection,
    dirty: Arc<AtomicUsize>,
    ids: Vec<Arc<PoiRecord>>,
    retained: BTreeMap<String, Arc<PoiRecord>>,
    updater_calls: usize,
}
impl State {
    fn new() -> Self {
        let dirty = Arc::new(AtomicUsize::new(0));
        let callback = Arc::clone(&dirty);
        Self {
            section: PoiSection::new(Arc::new(move || {
                callback.fetch_add(1, Ordering::Relaxed);
            })),
            dirty,
            ids: Vec::new(),
            retained: BTreeMap::new(),
            updater_calls: 0,
        }
    }
    fn id(&mut self, record: &Arc<PoiRecord>) -> usize {
        if let Some(id) = self.ids.iter().position(|old| Arc::ptr_eq(old, record)) {
            id
        } else {
            let id = self.ids.len();
            self.ids.push(Arc::clone(record));
            id
        }
    }
}
fn pos(c: &Value, index: usize) -> BlockPos {
    BlockPos::new(
        c[index].as_i64().unwrap() as i32,
        c[index + 1].as_i64().unwrap() as i32,
        c[index + 2].as_i64().unwrap() as i32,
    )
}
fn kind(registry: &Registry<PoiType>, name: &str) -> Holder<PoiType> {
    registry
        .get(&ResourceKey::create(
            &POINT_OF_INTEREST_TYPE,
            Identifier::parse(name),
        ))
        .unwrap()
}
fn record_json(state: &mut State, record: &Arc<PoiRecord>, registry: &Registry<PoiType>) -> Value {
    json!({"id":state.id(record),"pos":[record.pos().get_x(),record.pos().get_y(),record.pos().get_z()],"type":record.poi_type().value(registry).name(),"free":record.free_tickets()})
}
fn snapshot(state: &mut State, registry: &Registry<PoiType>, step: usize) -> Value {
    let current: Vec<_> = state
        .section
        .records
        .entries()
        .map(|(_, record)| Arc::clone(record))
        .collect();
    let records: Vec<_> = current
        .iter()
        .map(|record| record_json(state, record, registry))
        .collect();
    let mut types: Vec<_> = state
        .section
        .by_type
        .iter()
        .map(|(kind, _)| kind.clone())
        .collect();
    types.sort_by_key(|kind| kind.value(registry).name());
    let types:Vec<_>=types.iter().map(|kind|{
        let records:Vec<_>=state.section.records_for_type(kind,PoiOccupancy::Any).cloned().collect();
        let records:Vec<_>=records.iter().map(|record|record_json(state,record,registry)).collect();
        let space:Vec<_>=state.section.records_for_type(kind,PoiOccupancy::HasSpace).cloned().collect();
        let space:Vec<_>=space.iter().map(|record|state.id(record)).collect();
        let occupied:Vec<_>=state.section.records_for_type(kind,PoiOccupancy::IsOccupied).cloned().collect();
        let occupied:Vec<_>=occupied.iter().map(|record|state.id(record)).collect();
        json!({"type":kind.value(registry).name(),"records":records,"space":space,"occupied":occupied})
    }).collect();
    let retained: Vec<_> = state
        .retained
        .iter()
        .map(|(name, record)| (name.clone(), Arc::clone(record)))
        .collect();
    let retained: serde_json::Map<String, Value> = retained
        .iter()
        .map(|(name, record)| (name.clone(), record_json(state, record, registry)))
        .collect();
    json!({"valid":state.section.is_valid(),"empty":state.section.is_empty(),"dirty":state.dirty.load(Ordering::Relaxed),"updater_calls":state.updater_calls,"records":records,"types":types,"retained":retained,"step":step})
}

#[test]
fn actual_paper_section_commands_preserve_identity_indices_order_and_dirty_events() {
    let access = RegistryAccess::from_pairs(vec![(
        ResourceKey::create_registry_key(POINT_OF_INTEREST_TYPE.identifier().clone()),
        Box::new(bootstrap_poi_types()),
    )]);
    let registry = access.lookup(&POINT_OF_INTEREST_TYPE).unwrap();
    let ops = RegistryOps::create_from_access(&NbtOps, access.clone());
    let codec = PackedPoiSection::codec();
    let mut scenarios = 0;
    for line in include_str!("../../../../tools/rivet-oracle/fixtures/poi-section.jsonl").lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        if row["kind"] != "section" {
            continue;
        }
        scenarios += 1;
        let mut state = State::new();
        let mut snapshots = row["snapshots"].as_array().unwrap().iter();
        let mut results = Vec::new();
        for (step, c) in row["commands"].as_array().unwrap().iter().enumerate() {
            let op = c[0].as_str().unwrap();
            let mut result = Value::Null;
            match op {
                "init" => {
                    let input =
                        Tag::Compound(parse_compound_fully(c[1].as_str().unwrap()).unwrap());
                    let packed = codec.parse(&ops, &input).result().unwrap().clone();
                    let dirty = Arc::clone(&state.dirty);
                    state.section = packed.unpack(
                        registry,
                        Arc::new(move || {
                            dirty.fetch_add(1, Ordering::Relaxed);
                        }),
                    );
                }
                "add" | "add_quiet" => {
                    if let Some(record) = state.section.add(
                        pos(c, 1),
                        kind(registry, c[4].as_str().unwrap()),
                        registry,
                    ) {
                        result = json!(state.id(&record));
                    }
                }
                "remove" | "remove_quiet" => {
                    state.section.remove(&pos(c, 1));
                }
                "acquire" => {
                    result = json!(state.section.record(&pos(c, 1)).unwrap().acquire_ticket());
                }
                "release" => {
                    result = state
                        .section
                        .release(&pos(c, 1))
                        .map_or_else(|_| json!("missing"), |value| json!(value));
                }
                "retain" => {
                    state.retained.insert(
                        c[1].as_str().unwrap().into(),
                        Arc::clone(state.section.record(&pos(c, 2)).unwrap()),
                    );
                }
                "acquire_retained" => {
                    result = json!(state.retained[c[1].as_str().unwrap()].acquire_ticket());
                }
                "release_retained" => {
                    result = json!(state.retained[c[1].as_str().unwrap()].release_ticket());
                }
                "refresh" | "refresh_throw" => {
                    let calls = &mut state.updater_calls;
                    if catch_unwind(AssertUnwindSafe(|| {
                        state.section.refresh(registry, |emit| {
                            *calls += 1;
                            for entry in c[1].as_array().unwrap() {
                                emit(pos(entry, 0), kind(registry, entry[3].as_str().unwrap()));
                                assert_ne!(op, "refresh_throw", "source updater sentinel");
                            }
                        })
                    }))
                    .is_err()
                    {
                        result = json!("interrupted");
                    }
                }
                "query" => {
                    let p = pos(c, 1);
                    result = json!({"free":state.section.get_free_tickets(&p),"exists":state.section.exists(&p,|_|true)});
                    if let Some(kind) = state.section.get_type(&p) {
                        result
                            .as_object_mut()
                            .unwrap()
                            .insert("type".into(), json!(kind.value(registry).name()));
                    }
                }
                "snapshot" => {}
                other => panic!("unknown oracle command {other}"),
            }
            results.push(result);
            if !op.ends_with("_quiet") {
                let mut expected = snapshots.next().unwrap().clone();
                let encoded = codec.encode_start(&ops, &state.section.pack());
                assert_eq!(
                    encoded.result().unwrap(),
                    &Tag::Compound(
                        parse_compound_fully(expected["packed"].as_str().unwrap()).unwrap()
                    ),
                    "{} step{step} pack",
                    row["name"]
                );
                expected.as_object_mut().unwrap().remove("packed");
                assert_eq!(
                    snapshot(&mut state, registry, step),
                    expected,
                    "{} step{step} {op}",
                    row["name"]
                );
            }
        }
        assert!(snapshots.next().is_none());
        assert_eq!(json!(results), row["results"], "{} results", row["name"]);
    }
    assert_eq!(scenarios, 6);
}

#[test]
fn actual_section_packed_codec_matches_lenient_valid_and_partial_record_lists() {
    let access = RegistryAccess::from_pairs(vec![(
        ResourceKey::create_registry_key(POINT_OF_INTEREST_TYPE.identifier().clone()),
        Box::new(bootstrap_poi_types()),
    )]);
    let ops = RegistryOps::create_from_access(&NbtOps, access);
    let codec = PackedPoiSection::codec();
    let mut cases = 0;
    for line in include_str!("../../../../tools/rivet-oracle/fixtures/poi-section.jsonl").lines() {
        let row: Value = serde_json::from_str(line).unwrap();
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
            assert_eq!(
                codec
                    .encode_start(&ops, &partial.unwrap())
                    .result()
                    .unwrap(),
                &Tag::Compound(parse_compound_fully(encoded).unwrap()),
                "{line}"
            );
        } else {
            assert!(partial.is_none(), "{line}");
        }
    }
    assert_eq!(cases, 7);
}

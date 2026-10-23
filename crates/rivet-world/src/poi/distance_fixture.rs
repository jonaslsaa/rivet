//! Complete-field and complete-ordered-event differential, not a sampled or
//! separately recomputed distance formula.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

fn number(c: &Value, i: usize) -> i64 {
    c[i].as_i64().unwrap()
}
fn coordinate(c: &Value) -> i64 {
    SectionPos::as_long(
        number(c, 1) as i32,
        number(c, 2) as i32,
        number(c, 3) as i32,
    )
}
fn level_snapshot(tracker: &Delayed26WayDistancePropagator3D) -> Value {
    let mut levels: Vec<_> = tracker.levels.iter().collect();
    levels.sort_by_key(|(key, _)| **key);
    let mut hash = Sha256::new();
    for (key, level) in levels {
        hash.update(key.to_be_bytes());
        hash.update([*level as u8]);
    }
    json!({"size":tracker.levels.len(),"hash":format!("{:x}",hash.finalize())})
}

#[test]
fn actual_moonrise_delayed_26way_trace_matches_entire_field_and_callback_order() {
    let mut cases = 0;
    for line in include_str!("../../../../tools/rivet-oracle/fixtures/poi-distance.jsonl").lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        cases += 1;
        let events = Arc::new(Mutex::new(Vec::new()));
        let callback = Arc::clone(&events);
        let mut tracker = Delayed26WayDistancePropagator3D::with_callback(move |key, old, next| {
            callback.lock().unwrap().push((key, old, next))
        });
        for (step, c) in row["commands"].as_array().unwrap().iter().enumerate() {
            events.lock().unwrap().clear();
            let mut result = Value::Null;
            match c[0].as_str().unwrap() {
                "set" => {
                    if let Err(error) = tracker.set_source(coordinate(c), number(c, 4) as i32) {
                        result = json!(error.to_string());
                    }
                }
                "set_packed" => {
                    if let Err(error) = tracker.set_source(number(c, 1), number(c, 2) as i32) {
                        result = json!(error.to_string());
                    }
                }
                "remove" => tracker.remove_source(coordinate(c)),
                "remove_packed" => tracker.remove_source(number(c, 1)),
                "get" => result = json!(tracker.get_level(coordinate(c))),
                "get_packed" => result = json!(tracker.get_level(number(c, 1))),
                "propagate" => result = json!(tracker.propagate_updates()),
                other => panic!("unknown source command {other}"),
            }
            let events = events.lock().unwrap();
            let mut hash = Sha256::new();
            for (key, old, next) in events.iter() {
                hash.update(key.to_be_bytes());
                hash.update([*old as u8, *next as u8]);
            }
            let head: Vec<_> = events
                .iter()
                .take(4)
                .map(|(key, old, next)| json!([key, old, next]))
                .collect();
            let tail: Vec<_> = events
                .iter()
                .skip(events.len().saturating_sub(4))
                .map(|(key, old, next)| json!([key, old, next]))
                .collect();
            let actual = json!({"result":result,"levels":level_snapshot(&tracker),"events":events.len(),"equal_events":events.iter().filter(|(_,old,next)|old==next).count(),"events_hash":format!("{:x}",hash.finalize()),"head":head,"tail":tail});
            assert_eq!(actual, row["steps"][step], "{} step{step} {c}", row["name"]);
        }
    }
    assert_eq!(cases, 8);
}

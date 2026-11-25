//! The worldgen sweep ops (VERIFICATION section 5, item 5): `feature.place`,
//! `carver.carve` and `structure.start`.
//!
//! Each op sweeps a whole vanilla registry (every configured feature, every
//! configured carver, every structure set) for the same eight world seeds, so
//! the declared count is registry size x 8 on Paper's own registry. The
//! committed `corpus-src/worldgen-registries.json` is that registry's key list
//! (regenerated from the Paper datapack by `harvest_datapack.py`); the Java
//! ops refuse to record a corpus that omits an entry (`corpusCheck`).
//!
//! The Rust handlers run only what Rivet has ported. An input whose feature,
//! carver or structure has no Rust implementation, or whose configured feature
//! is outside the generated closure (a data-scope gap), answers
//! [`Outcome::Unported`] with the reason (counted as `unavailable`, never as
//! matched or silently dropped); every other divergence is a hard mismatch.
//! The region and chunk-grid contracts are documented on the Java ops and on
//! each module; the op result shapes are in `docs/VERIFICATION.md` section 5
//! item 5.

pub mod carver;
pub mod feature;
pub mod structure;

use std::sync::OnceLock;

use serde_json::{Map, Value, json};

use super::compare::Outcome;

/// The eight world seeds every worldgen sweep op runs for
/// (`RivetReferenceOracle.WORLDGEN_SEEDS`).
pub const SEEDS: [i64; 8] = [
    0,
    1,
    42,
    1337,
    -1,
    8_675_309,
    4_611_686_018_427_387_905,
    i64::MIN,
];

/// The seeds `ops live` runs the Rust handlers for (the Java side always runs
/// every recorded seed). `structure.start` costs about a minute of Rust per
/// seed in the debug profile `gate.sh` builds, so the gate's live comparison
/// of the three worldgen ops covers these two of the eight recorded seeds;
/// `ops check` and the cargo tests replay all eight offline.
const LIVE_SEEDS: &str = include_str!(
    "../../../../rivet-reference-oracle/fixtures/ops/corpus-src/worldgen-live-seeds.json"
);

/// The ops that sweep a registry for [`SEEDS`].
pub const OPS: [&str; 3] = ["feature.place", "carver.carve", "structure.start"];

/// The seeds `ops live` compares Rust against live Paper for, for a worldgen op.
pub fn live_seeds(op: &str) -> Option<Vec<i64>> {
    OPS.contains(&op).then(|| {
        serde_json::from_str(LIVE_SEEDS).expect("worldgen-live-seeds.json is committed valid JSON")
    })
}

const REGISTRIES: &str = include_str!(
    "../../../../rivet-reference-oracle/fixtures/ops/corpus-src/worldgen-registries.json"
);

/// The registry key lists the sweep covers.
pub fn registries() -> &'static Value {
    static PARSED: OnceLock<Value> = OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str(REGISTRIES).expect("worldgen-registries.json is committed valid JSON")
    })
}

fn entries(registry: &str) -> &'static [Value] {
    registries()[registry]
        .as_array()
        .unwrap_or_else(|| panic!("worldgen-registries.json has no {registry} list"))
}

fn text<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry[key]
        .as_str()
        .unwrap_or_else(|| panic!("worldgen-registries.json entry lacks {key}"))
}

fn corpus_line(key: &str, seed: i64, fields: Vec<(&str, Value)>) -> String {
    let mut object = Map::new();
    object.insert("id".into(), json!(format!("{key}#{seed}")));
    for (name, value) in fields {
        object.insert(name.to_owned(), value);
    }
    object.insert("seed".into(), json!(seed));
    serde_json::to_string(&Value::Object(object)).expect("JSON values always serialize") + "\n"
}

/// `feature.place` corpus: every configured feature x every seed, each in the
/// region profile that gives the feature its natural surroundings.
pub fn emit_feature_corpus() -> String {
    let mut out = String::new();
    for entry in entries("configured_feature") {
        let key = text(entry, "key");
        let region = feature::region_for(key, text(entry, "type"));
        for seed in SEEDS {
            out += &corpus_line(
                key,
                seed,
                vec![("feature", json!(key)), ("region", json!(region))],
            );
        }
    }
    out
}

/// `carver.carve` corpus: every configured carver x every seed. `index` is the
/// carver's position in its dimension's carver list (it salts the seed).
pub fn emit_carver_corpus() -> String {
    let mut out = String::new();
    for entry in entries("configured_carver") {
        let key = text(entry, "key");
        for seed in SEEDS {
            out += &corpus_line(
                key,
                seed,
                vec![
                    ("carver", json!(key)),
                    ("index", json!(carver::index_of(key))),
                ],
            );
        }
    }
    out
}

/// `structure.start` corpus: every structure set x every seed, in the
/// dimension the set generates in.
pub fn emit_structure_corpus() -> String {
    let mut out = String::new();
    for entry in entries("structure_set") {
        let key = text(entry, "key");
        for seed in SEEDS {
            out += &corpus_line(
                key,
                seed,
                vec![
                    ("structure_set", json!(key)),
                    ("dimension", json!(structure::dimension_of(key))),
                ],
            );
        }
    }
    out
}

/// A panic payload as text.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "non-string panic".to_owned())
}

/// Whether a Rust panic says "this capability is not ported" (the ported code
/// refuses loudly instead of guessing) rather than being a defect.
pub(crate) fn is_unported_panic(message: &str) -> bool {
    message.contains("not implemented") || message.contains("RivetTodo")
}

pub(crate) fn request_str<'a>(
    request: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, Outcome> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Outcome::Unavailable(format!("request property '{key}' must be a string")))
}

pub(crate) fn request_i64(request: &Map<String, Value>, key: &str) -> Result<i64, Outcome> {
    request
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| Outcome::Unavailable(format!("request property '{key}' must be an integer")))
}

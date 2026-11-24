//! `loot.roll`: every vanilla loot table over 16 seeds with fixed, documented
//! `LootParams`.
//!
//! Request `{table, seed, luck, entity, block}`. Paper builds the `LootParams`
//! from the table's own context-key set (the Java op documents each key's value):
//! ORIGIN (0.5, 64, 0.5), `luck`, a tool chosen by the set, BLOCK_STATE = the
//! default state of `block`, THIS_ENTITY / TARGET_ENTITY (where the set requires
//! them) = a fresh `entity` created in an unconstructed `ServerLevel`, DAMAGE_SOURCE
//! = generic with no attacker; no killer player is supplied. The answer is the
//! ordered item stacks as SNBT.
//!
//! `entity` is the last path segment of the table (scanning towards the front)
//! that names an entity type, else a pig; `block` is `minecraft:<path>` of a
//! `blocks/<path>` table when that is a block, else stone. The Rust side has the
//! loot runtime but no decoder for loot-table documents and none of the
//! entity/level context, so every cell is unported.

use rivet_registry::generated::blocks::BLOCK_BY_NAME;
use rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME;
use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{SEEDS, data, line};

/// The luck every roll runs with (`LootParams.Builder.withLuck`).
pub const LUCK: f64 = 1.0;

fn entity_for(path: &str) -> String {
    path.split('/')
        .rev()
        .map(|segment| format!("minecraft:{segment}"))
        .find(|name| ENTITY_TYPE_BY_NAME.contains_key(name.as_str()))
        .unwrap_or_else(|| "minecraft:pig".into())
}

fn block_for(path: &str) -> String {
    path.strip_prefix("blocks/")
        .map(|block| format!("minecraft:{block}"))
        .filter(|name| BLOCK_BY_NAME.contains_key(name.as_str()))
        .unwrap_or_else(|| "minecraft:stone".into())
}

pub fn emit() -> String {
    let mut out = String::new();
    for table in &data().loot_tables {
        let path = table.strip_prefix("minecraft:").unwrap_or(table);
        for seed in SEEDS {
            out += &line(
                &format!("{table}#{seed}"),
                vec![
                    ("table", json!(table)),
                    ("seed", json!(seed)),
                    ("luck", json!(LUCK)),
                    ("entity", json!(entity_for(path))),
                    ("block", json!(block_for(path))),
                ],
            );
        }
    }
    out
}

pub fn loot_roll(_request: &Map<String, Value>) -> Outcome {
    Outcome::Unported(
        "loot-table documents have no Rust decoder, and the level/entity context is not ported (rivet_world::loot holds the runtime only)".into(),
    )
}

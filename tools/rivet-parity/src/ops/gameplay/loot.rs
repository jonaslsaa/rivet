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
//! `blocks/<path>` table when that is a block, else stone.
//!
//! The Rust side answers the `blocks/*` tables only: `rivet_world::block::drops`
//! parses and evaluates block loot tables (`BlockLootTable`, the `BLOCK` parameter
//! set). Every other table (chests, entities, gameplay, fishing, ...) needs loot
//! functions, conditions and entity/level context that are not ported, so those
//! cells are unported. The block tables run with the oracle's context: the default
//! state of `block`, a diamond pickaxe, luck 1.0, no explosion radius, no block
//! entity, over `LegacyRandomSource(seed)` (`RandomSource.create(seed)`). A
//! stack is answered as `{count:N,id:"ID"}` (the stack codec with no components);
//! a stack that carries components answers with them appended so that it can
//! never equal a component-less golden.

use rivet_registry::generated::blocks::BLOCK_BY_NAME;
use rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME;
use std::collections::BTreeMap;
use std::fs;
use std::sync::OnceLock;

use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_util::random::LegacyRandomSource;
use rivet_world::block::drops::{
    BlockDropContext, BlockDropStack, BlockLootTable, ToolKind, ToolStack, ToolTier,
};
use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{SEEDS, data, integer, line, string};

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

/// The documents of every `minecraft:blocks/*` loot table (`harvest_gameplay.py`).
fn block_tables() -> &'static BTreeMap<String, Value> {
    static TABLES: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    TABLES.get_or_init(|| {
        let path = super::super::ops_dir().join("corpus-src/block-loot-tables.json");
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
    })
}

/// The stack as the oracle's stack codec prints it.
fn stack_snbt(stack: &BlockDropStack) -> String {
    let mut out = format!("{{count:{},id:\"{}\"", stack.count, stack.item_id);
    if !stack.components.is_empty() {
        out += &format!(
            ",components:{}",
            Value::Object(stack.components.clone().into_iter().collect())
        );
    }
    out + "}"
}

pub fn loot_roll(request: &Map<String, Value>) -> Outcome {
    let (table, block) = match (string(request, "table"), string(request, "block")) {
        (Ok(table), Ok(block)) => (table, block),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let seed = match integer(request, "seed") {
        Ok(seed) => seed,
        Err(e) => return e,
    };
    let Some(document) = block_tables().get(table) else {
        return Outcome::Unported(
            "only the blocks/* loot tables have a Rust evaluator (rivet_world::block::drops); chest, entity, gameplay and fishing tables need loot functions, conditions and entity/level context that are not ported".into(),
        );
    };
    let Some(block) = rivet_registry::generated::blocks::BlockId::from_name(block) else {
        return Outcome::Unavailable(format!("request names no block: {block}"));
    };
    let Some(luck) = request.get("luck").and_then(Value::as_f64) else {
        return Outcome::Unavailable("request property 'luck' must be a number".into());
    };
    // A vanilla document the Rust parser rejects is a port bug (Paper loads it): a mismatch.
    let Ok(parsed) = BlockLootTable::from_value(document) else {
        return Outcome::Rejected;
    };
    let context = BlockDropContext::new(
        BlockState::of(block),
        BlockPos::new(0, 64, 0),
        ToolStack::tool(ToolTier::Diamond, ToolKind::Pickaxe),
    )
    .with_luck(luck as f32);
    let mut random = LegacyRandomSource::new(seed);
    let items: Vec<Value> = parsed
        .evaluate(&context, &mut random)
        .iter()
        .map(|stack| json!(stack_snbt(stack)))
        .collect();
    let mut out = Map::new();
    out.insert("items".into(), Value::Array(items));
    Outcome::Accepted(out)
}

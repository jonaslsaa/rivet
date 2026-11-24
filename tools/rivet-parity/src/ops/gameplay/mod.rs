//! The gameplay-data ops (VERIFICATION §5 item 8): `recipe.match`, `loot.roll`,
//! `enchant.select`, `effect`, `trade.offers` and `scoreboard.saved_data`.
//!
//! Each op sweeps a vanilla registry (every recipe, loot table, enchantment,
//! mob effect, trade set) over a fixed input matrix, and Paper (the Java ops in
//! `GameplayOps.java`) answers every cell. The corpus generators here are pure
//! functions of the committed harvest (`corpus-src/gameplay-data.json`, refreshed
//! by `harvest_gameplay.py`) and Rivet's own registries; the Java ops verify at
//! record time that the corpus closes over what the live server loads.
//!
//! The Rust handlers call only surfaces already ported in `rivet-world`
//! (crafting, enchantment, effect, scores). A cell whose surface is not ported
//! answers [`Outcome::Unported`] with its reason: it is counted `unavailable`,
//! apart from matched and mismatched, never dropped.

pub mod effect;
pub mod enchant;
pub mod loot;
pub mod recipe;
pub mod scoreboard;
pub mod trade;

use std::collections::BTreeMap;
use std::fs;
use std::sync::OnceLock;

use serde_json::{Map, Value, json};

use super::compare::Outcome;
use super::packet_corpus::Rng;

/// The seeds of every seeded sweep cell: sixteen, none zero (a zero seed means
/// "no seed" to `LootContext.Builder.withOptionalRandomSeed`).
pub const SEEDS: [i64; 16] = [
    1,
    2,
    3,
    42,
    100,
    1337,
    4096,
    65_537,
    123_456_789,
    987_654_321,
    -1,
    -42,
    i64::MAX,
    i64::MIN + 1,
    5489,
    31_415_926_535,
];

/// The committed harvest of the vanilla gameplay data (`harvest_gameplay.py`).
pub struct GameplayData {
    /// Recipe documents by id (`minecraft:stick`), in id order.
    pub recipes: BTreeMap<String, Value>,
    /// Resolved item tags by `namespace:path`.
    pub item_tags: BTreeMap<String, Vec<String>>,
    pub loot_tables: Vec<String>,
    pub trade_sets: Vec<String>,
}

/// The harvested data, read once from `corpus-src/gameplay-data.json`.
pub fn data() -> &'static GameplayData {
    static DATA: OnceLock<GameplayData> = OnceLock::new();
    DATA.get_or_init(|| {
        let path = super::ops_dir().join("corpus-src/gameplay-data.json");
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let root: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()));
        let object = |key: &str| -> &Map<String, Value> {
            root.get(key)
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("gameplay-data.json has no object `{key}`"))
        };
        let strings = |key: &str| -> Vec<String> {
            root.get(key)
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("gameplay-data.json has no array `{key}`"))
                .iter()
                .map(|v| v.as_str().expect("string id").to_owned())
                .collect()
        };
        GameplayData {
            recipes: object("recipes")
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            item_tags: object("item_tags")
                .iter()
                .map(|(k, v)| {
                    let members = v
                        .as_array()
                        .expect("tag members")
                        .iter()
                        .map(|m| m.as_str().expect("item id").to_owned())
                        .collect();
                    (k.clone(), members)
                })
                .collect(),
            loot_tables: strings("loot_tables"),
            trade_sets: strings("trade_sets"),
        }
    })
}

/// One corpus line: `{"id":<id>,<fields in order>}`.
pub fn line(id: &str, fields: Vec<(&str, Value)>) -> String {
    let mut object = Map::new();
    object.insert("id".into(), json!(id));
    for (key, value) in fields {
        object.insert(key.to_owned(), value);
    }
    serde_json::to_string(&Value::Object(object)).expect("JSON values always serialize") + "\n"
}

/// A deterministic generator for one named stream of the gameplay corpora.
pub fn rng(stream: &str) -> Rng {
    Rng::for_stream(&format!("gameplay.{stream}"))
}

/// `Double.doubleToLongBits` as the oracle prints it: `0x` + 16 hex digits.
/// Java canonicalizes every NaN to `0x7ff8000000000000` (D14).
pub fn f64_bits(value: f64) -> String {
    let bits = if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits()
    };
    format!("0x{bits:016x}")
}

/// Required string request property, or the harness-fault outcome.
pub fn string<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a str, Outcome> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Outcome::Unavailable(format!("request property '{key}' must be a string")))
}

/// Required integer request property, or the harness-fault outcome.
pub fn integer(request: &Map<String, Value>, key: &str) -> Result<i64, Outcome> {
    request
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| Outcome::Unavailable(format!("request property '{key}' must be an integer")))
}

/// The subject (the ported surface a request exercises) for the per-subject tallies.
pub fn subject(op: &str, request: &Map<String, Value>) -> Option<String> {
    match op {
        "recipe.match" => recipe::subject_of(request),
        "loot.roll" => Some("loot".into()),
        "enchant.select" => request
            .get("kind")
            .and_then(Value::as_str)
            .map(|kind| format!("enchant.{kind}")),
        "effect" => request
            .get("effect")
            .and_then(Value::as_str)
            .map(str::to_owned),
        "trade.offers" => Some("trade".into()),
        "scoreboard.saved_data" => Some("scoreboard".into()),
        _ => None,
    }
}

/// The corpora of every gameplay op, by op name.
pub fn emit() -> Vec<(&'static str, String)> {
    vec![
        ("effect", effect::emit()),
        ("enchant.select", enchant::emit()),
        ("loot.roll", loot::emit()),
        ("recipe.match", recipe::emit()),
        ("scoreboard.saved_data", scoreboard::emit()),
        ("trade.offers", trade::emit()),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::super::{OpFixture, load_fixtures, op_source, ops_dir};
    use super::*;

    fn fixture(op: &str) -> OpFixture {
        load_fixtures(&ops_dir(), &op_source())
            .expect("committed recorded ops load and close")
            .into_iter()
            .find(|f| f.op == op)
            .unwrap_or_else(|| panic!("{op} is not recorded"))
    }

    fn distinct(fixture: &OpFixture, key: &str) -> BTreeSet<String> {
        fixture
            .corpus
            .iter()
            .map(|entry| entry.request[key].to_string())
            .collect()
    }

    /// The sweeps close over their registries: declared == registry size x seeds
    /// (the Java ops re-check this against the live registries at record time).
    #[test]
    fn sweeps_close_over_their_registries() {
        let loot = fixture("loot.roll");
        assert_eq!(distinct(&loot, "table").len(), data().loot_tables.len());
        assert_eq!(loot.declared, data().loot_tables.len() * SEEDS.len());
        let trade = fixture("trade.offers");
        assert_eq!(distinct(&trade, "trade_set").len(), data().trade_sets.len());
        assert_eq!(trade.declared, data().trade_sets.len() * SEEDS.len());
        let effect = fixture("effect");
        assert_eq!(
            effect.declared,
            rivet_world::effect::MobEffectId::COUNT * effect::AMPLIFIERS.len()
        );
        let recipes = fixture("recipe.match");
        assert_eq!(distinct(&recipes, "recipe").len(), data().recipes.len());
        let enchant = fixture("enchant.select");
        let enchantments = rivet_world::enchantment::EnchantmentRegistry::vanilla().len();
        let kinds: BTreeMap<String, usize> =
            enchant
                .corpus
                .iter()
                .fold(BTreeMap::new(), |mut acc, entry| {
                    *acc.entry(entry.request["kind"].as_str().unwrap().to_owned())
                        .or_default() += 1;
                    acc
                });
        assert_eq!(kinds["cost"], enchantments * enchant::COST_LEVELS.len());
        assert_eq!(
            kinds["select"],
            enchant::ITEMS.len() * enchant::POWERS.len() * SEEDS.len()
        );
    }

    /// Paper's own recorded answers hold the documented harness gaps and nothing
    /// else: the loot tables and trade sets whose roll throws inside Paper because
    /// the unconstructed level has no chunk source, structure search, Bukkit server,
    /// raids, scoreboard, or (for the player) cannot create the entity. A new
    /// `threw` is a regression of the context, not a verdict.
    #[test]
    fn recorded_throws_are_the_documented_level_shell_gaps() {
        let threw = |op: &str, key: &str| -> BTreeSet<String> {
            let fixture = fixture(op);
            fixture
                .corpus
                .iter()
                .zip(&fixture.recorded)
                .filter(|(_, (_, outcome))| {
                    matches!(outcome, Outcome::Accepted(result) if result.contains_key("threw"))
                })
                .map(|(entry, _)| entry.request[key].as_str().unwrap().to_owned())
                .collect()
        };
        let expected = |names: &[&str]| -> BTreeSet<String> {
            names.iter().map(|n| (*n).to_owned()).collect()
        };
        assert_eq!(
            threw("loot.roll", "table"),
            expected(&[
                "minecraft:chests/shipwreck_map",
                "minecraft:chests/underwater_ruin_big",
                "minecraft:chests/underwater_ruin_small",
                "minecraft:entities/drowned",
                "minecraft:entities/ender_dragon",
                "minecraft:entities/pillager",
                "minecraft:entities/player",
                "minecraft:entities/wither",
            ])
        );
        assert_eq!(
            threw("trade.offers", "trade_set"),
            expected(&[
                "minecraft:cartographer/level_2",
                "minecraft:cartographer/level_3",
                "minecraft:cartographer/level_5",
            ])
        );
        assert!(threw("recipe.match", "recipe").is_empty());
    }

    /// A handler answer that disagrees with Paper's recorded one on a gameplay
    /// op is detected and named: the comparator's own tamper negative, per op, on
    /// the first input the Rust side executes.
    #[test]
    fn gameplay_ops_detect_a_tampered_answer() {
        let exclusions =
            super::super::compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap();
        for fixture in load_fixtures(&ops_dir(), &op_source()).unwrap() {
            if !matches!(
                fixture.op.as_str(),
                "effect"
                    | "enchant.select"
                    | "loot.roll"
                    | "recipe.match"
                    | "scoreboard.saved_data"
            ) {
                continue;
            }
            let actual: Vec<(String, Outcome)> = fixture
                .corpus
                .iter()
                .map(|entry| {
                    let handler = super::super::handlers::handler(&fixture.op).unwrap();
                    (entry.id.clone(), handler(&entry.request))
                })
                .collect();
            let negative = super::super::compare::tamper_negative(
                &fixture.op,
                &fixture.recorded,
                &actual,
                &exclusions,
            )
            .unwrap_or_else(|| panic!("{}: no input to tamper", fixture.op));
            assert!(negative.passed(), "{}: {negative}", fixture.op);
        }
    }
    /// `unavailable.tsv` (read by `scripts/verify_ledger.py` to close a row's
    /// count) states exactly what the Rust handlers answer `Unported`, per op and
    /// reason: a handler that starts answering an input, or a new unported
    /// input, moves this table with it.
    #[test]
    fn unavailable_table_is_what_the_handlers_report() {
        let text = fs::read_to_string(ops_dir().join("unavailable.tsv")).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("op\tcount\treason"));
        let mut committed: BTreeMap<(String, String), usize> = BTreeMap::new();
        for line in lines {
            let cells: Vec<&str> = line.split('\t').collect();
            assert_eq!(cells.len(), 3, "{line}");
            committed.insert(
                (cells[0].to_owned(), cells[2].to_owned()),
                cells[1].parse().unwrap(),
            );
        }
        let exclusions =
            super::super::compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap();
        let fixtures = load_fixtures(&ops_dir(), &op_source()).unwrap();
        let mut actual: BTreeMap<(String, String), usize> = BTreeMap::new();
        for row in super::super::check(&fixtures, &exclusions, None) {
            for (reason, count) in row.unavailable_reasons {
                actual.insert((row.op.clone(), reason), count);
            }
        }
        assert_eq!(committed, actual);
    }
}

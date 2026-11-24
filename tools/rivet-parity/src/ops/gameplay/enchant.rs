//! `enchant.select`: `EnchantmentHelper` over fixed items, enchanting powers and
//! seeds, plus the enchantment cost tables.
//!
//! Three request kinds (the Java op documents them):
//!
//! - `cost {enchantment, level}` -> `{costs:{min, max}}`: `Enchantment.getMinCost/getMaxCost`
//!   for every enchantment of the vanilla registry;
//! - `slot_cost {item, enchantable, slot, bookcases, seed}` -> `{slot_cost}`:
//!   `getEnchantmentCost(RandomSource.create(seed), ...)`;
//! - `select {item, enchantable, power, seed}` -> `{enchantments:[{enchantment, level}]}`:
//!   `selectEnchantment(RandomSource.create(seed), stack, power, #in_enchanting_table)`.
//!
//! The item is a stack of the named item whose enchantability is the request's
//! `enchantable` (0 removes the component): an input, never looked up, so both
//! sides see the same item without either needing an item-component table.

use rivet_registry::Identifier;
use rivet_util::random::LegacyRandomSource;
use rivet_world::enchantment::helper::{get_enchantment_cost, select_enchantment};
use rivet_world::enchantment::{Enchantable, EnchantableItemStack, EnchantmentRegistry};
use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{SEEDS, integer, line, string};

/// Levels the cost tables are read at: the vanilla range, a deep level and the maximum.
pub const COST_LEVELS: [i32; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 127, 255];
/// Enchanting powers `select` runs at (the enchanting table offers 1 to 30; above is anvil and command range).
pub const POWERS: [i32; 9] = [1, 5, 10, 15, 20, 30, 40, 50, 60];
/// Bookshelf counts `slot_cost` runs at (the table caps at 15).
pub const BOOKCASES: [i32; 6] = [0, 1, 5, 8, 15, 20];

/// Items with their vanilla `minecraft:enchantable` value: tools and armor by material
/// (wood 15, stone 5, iron 14, gold 22/25 armor, diamond 10, netherite 15, leather 15,
/// chain 12, turtle 9), the single-value tools (1) and an unenchantable item (0).
pub const ITEMS: [(&str, i32); 13] = [
    ("minecraft:book", 1),
    ("minecraft:bow", 1),
    ("minecraft:crossbow", 1),
    ("minecraft:trident", 1),
    ("minecraft:fishing_rod", 1),
    ("minecraft:wooden_sword", 15),
    ("minecraft:stone_pickaxe", 5),
    ("minecraft:iron_axe", 14),
    ("minecraft:golden_shovel", 22),
    ("minecraft:diamond_boots", 10),
    ("minecraft:netherite_helmet", 15),
    ("minecraft:chainmail_leggings", 12),
    ("minecraft:stick", 0),
];

/// The items `slot_cost` runs on (it reads only the enchantability, so a few suffice).
const SLOT_COST_ITEMS: [(&str, i32); 3] = [
    ("minecraft:diamond_boots", 10),
    ("minecraft:golden_shovel", 22),
    ("minecraft:stick", 0),
];

pub fn emit() -> String {
    let mut out = String::new();
    for enchantment in EnchantmentRegistry::vanilla().iter() {
        for level in COST_LEVELS {
            out += &line(
                &format!("cost.{}#{level}", enchantment.id),
                vec![
                    ("kind", json!("cost")),
                    ("enchantment", json!(enchantment.id.to_string())),
                    ("level", json!(level)),
                ],
            );
        }
    }
    for (item, enchantable) in SLOT_COST_ITEMS {
        for slot in 0..3 {
            for bookcases in BOOKCASES {
                for seed in SEEDS {
                    out += &line(
                        &format!("slot_cost.{item}.{enchantable}.{slot}.{bookcases}#{seed}"),
                        vec![
                            ("kind", json!("slot_cost")),
                            ("item", json!(item)),
                            ("enchantable", json!(enchantable)),
                            ("slot", json!(slot)),
                            ("bookcases", json!(bookcases)),
                            ("seed", json!(seed)),
                        ],
                    );
                }
            }
        }
    }
    for (item, enchantable) in ITEMS {
        for power in POWERS {
            for seed in SEEDS {
                out += &line(
                    &format!("select.{item}.{enchantable}.{power}#{seed}"),
                    vec![
                        ("kind", json!("select")),
                        ("item", json!(item)),
                        ("enchantable", json!(enchantable)),
                        ("power", json!(power)),
                        ("seed", json!(seed)),
                    ],
                );
            }
        }
    }
    out
}

fn stack(request: &Map<String, Value>) -> Result<EnchantableItemStack, Outcome> {
    let item = string(request, "item")?;
    let enchantable = integer(request, "enchantable")?;
    let Some(id) = Identifier::try_parse(item) else {
        return Err(Outcome::Unavailable(format!("bad item id {item}")));
    };
    let enchantable = i32::try_from(enchantable)
        .ok()
        .filter(|v| *v > 0)
        .map(Enchantable::new)
        .and_then(Result::ok);
    Ok(EnchantableItemStack::new(id, enchantable))
}

pub fn enchant_select(request: &Map<String, Value>) -> Outcome {
    let kind = match string(request, "kind") {
        Ok(kind) => kind,
        Err(e) => return e,
    };
    let registry = EnchantmentRegistry::vanilla();
    let mut result = Map::new();
    match kind {
        "cost" => {
            let (name, level) = match (string(request, "enchantment"), integer(request, "level")) {
                (Ok(name), Ok(level)) => (name, level),
                (Err(e), _) | (_, Err(e)) => return e,
            };
            let Some(enchantment) = registry.get_by_str(name) else {
                return Outcome::Unavailable(format!(
                    "enchantment {name} is not in the Rust registry"
                ));
            };
            let level = level as i32;
            result.insert(
                "costs".into(),
                json!({"min": enchantment.min_cost(level), "max": enchantment.max_cost(level)}),
            );
        }
        "slot_cost" => {
            let (item, slot, bookcases, seed) = match (
                stack(request),
                integer(request, "slot"),
                integer(request, "bookcases"),
                integer(request, "seed"),
            ) {
                (Ok(item), Ok(slot), Ok(bookcases), Ok(seed)) => (item, slot, bookcases, seed),
                (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (.., Err(e)) => return e,
            };
            let mut random = LegacyRandomSource::new(seed);
            result.insert(
                "slot_cost".into(),
                json!(get_enchantment_cost(
                    &mut random,
                    slot as i32,
                    bookcases as i32,
                    &item
                )),
            );
        }
        "select" => {
            let (item, power, seed) = match (
                stack(request),
                integer(request, "power"),
                integer(request, "seed"),
            ) {
                (Ok(item), Ok(power), Ok(seed)) => (item, power, seed),
                (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => return e,
            };
            let source = registry.tag_members("minecraft:in_enchanting_table");
            let mut random = LegacyRandomSource::new(seed);
            let picked = select_enchantment(&mut random, &item, power as i32, &source);
            result.insert(
                "enchantments".into(),
                Value::Array(
                    picked
                        .iter()
                        .map(
                            |p| json!({"enchantment": p.enchantment.to_string(), "level": p.level}),
                        )
                        .collect(),
                ),
            );
        }
        other => return Outcome::Unavailable(format!("unknown enchant.select kind {other}")),
    }
    Outcome::Accepted(result)
}

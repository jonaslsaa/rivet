//! `recipe.match`: every vanilla recipe against generated inputs.
//!
//! Request `{recipe, kind, ...}` with `kind` one of
//!
//! - `crafting {width, height, grid}`: a `CraftingInput`; `grid` is `width * height`
//!   cells in row-major order;
//! - `single {input}`: a `SingleRecipeInput` (cooking, stonecutting);
//! - `smithing {template, base, addition}`: a `SmithingRecipeInput`.
//!
//! A cell is `""` (empty) or `<item id>[#<count>]`. Paper answers
//! `{match, result?}`: `Recipe.matches(input, level)` and, on a match, the
//! assembled stack as `{item, count, components}` (sorted component-patch ids,
//! `!id` for a removal) — or `threw` when its own code throws. The Rust handler
//! parses the recipe document from the same harvest (`recipes[recipe]`) with
//! `rivet_world::crafting::parse_recipe_json`, resolving item tags with
//! `rivet-registry`'s generated table, and runs the ported matcher.
//!
//! The grids are derived from each recipe's document. Every ingredient resolves
//! to concrete items (an item id, a list, or a tag's members); the cases are:
//!
//! - shaped: `exact_3x3` (pattern at the top left), `exact_2x2` (when it fits),
//!   `alt` (last candidate of every ingredient), `offset` (pattern at the bottom
//!   right), `mirrored`, `wrong_item` (first ingredient replaced by bedrock),
//!   `missing` (first ingredient removed), `extra` (bedrock in an empty cell),
//!   `count2` (first ingredient stacked), `empty`;
//! - shapeless and the ingredient-pair recipes (dye, imbue, transmute):
//!   `exact`, `alt`, `reversed`, `shuffled` (a seeded shuffle of the nine
//!   cells), `wrong_item`, `missing`, `extra`, `count2`, `empty`;
//! - single-input recipes: `exact`, `alt`, `wrong_item`, `count16`, `empty`;
//! - smithing: `exact`, `alt`, `wrong_template`, `wrong_base`, `wrong_addition`,
//!   `missing_template`, `missing_base`, `missing_addition`, `swapped`, `empty`;
//! - the special crafting recipes (no ingredient data: banner duplicate, book
//!   cloning, decorated pot, firework rocket/star/fade, map extending, repair
//!   item, shield decoration): a few hand-written grids each ([`special`]).
//!
//! Bedrock stands for "an item no recipe accepts".

use std::cell::RefCell;
use std::collections::HashMap;

use rivet_registry::Identifier;
use rivet_registry::generated::tags::ITEM_TAG_BY_NAME;
use rivet_world::crafting::{
    CraftingInput, ItemId, ItemStack, Recipe, SingleRecipeInput, SmithingRecipeInput,
    parse_recipe_json,
};
use serde_json::{Map, Value, json};

use super::super::compare::Outcome;
use super::{data, line, rng, string};

/// An item no vanilla recipe accepts as an ingredient.
const WRONG: &str = "minecraft:bedrock";

/// One generated input: the label (unique per recipe) and its request fields.
struct Case {
    label: String,
    fields: Vec<(&'static str, Value)>,
}

/// An ingredient's concrete candidates, in order (an item, a list, or a tag's members).
fn candidates(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => match s.strip_prefix('#') {
            Some(tag) => data().item_tags.get(tag).cloned().unwrap_or_default(),
            None => vec![s.clone()],
        },
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn first(value: &Value) -> String {
    candidates(value).into_iter().next().unwrap_or_default()
}

fn last(value: &Value) -> String {
    candidates(value).pop().unwrap_or_default()
}

fn crafting(label: &str, width: usize, height: usize, grid: Vec<String>) -> Case {
    Case {
        label: label.into(),
        fields: vec![
            ("kind", json!("crafting")),
            ("width", json!(width)),
            ("height", json!(height)),
            ("grid", json!(grid)),
        ],
    }
}

/// `pattern` rows of ingredient values (`None` = empty cell), as `width x height`.
struct Pattern {
    width: usize,
    height: usize,
    cells: Vec<Option<Value>>,
}

impl Pattern {
    /// The grid with the pattern's top-left at (`left`, `top`) in a `size x size` grid.
    fn place(
        &self,
        size: usize,
        left: usize,
        top: usize,
        pick: impl Fn(&Value) -> String,
        mirror: bool,
    ) -> Vec<String> {
        let mut grid = vec![String::new(); size * size];
        for y in 0..self.height {
            for x in 0..self.width {
                let source = if mirror { self.width - 1 - x } else { x };
                if let Some(value) = &self.cells[y * self.width + source] {
                    grid[(top + y) * size + left + x] = pick(value);
                }
            }
        }
        grid
    }

    fn symmetric(&self) -> bool {
        (0..self.height).all(|y| {
            (0..self.width).all(|x| {
                self.cells[y * self.width + x] == self.cells[y * self.width + self.width - 1 - x]
            })
        })
    }
}

fn shaped_cases(doc: &Value) -> Vec<Case> {
    let key = doc["key"].as_object().expect("shaped key");
    let rows: Vec<&str> = doc["pattern"]
        .as_array()
        .expect("shaped pattern")
        .iter()
        .map(|r| r.as_str().expect("pattern row"))
        .collect();
    let pattern = Pattern {
        width: rows[0].chars().count(),
        height: rows.len(),
        cells: rows
            .iter()
            .flat_map(|row| row.chars())
            .map(|c| key.get(&c.to_string()).cloned())
            .collect(),
    };
    let mut cases = vec![crafting(
        "exact_3x3",
        3,
        3,
        pattern.place(3, 0, 0, first, false),
    )];
    if pattern.width <= 2 && pattern.height <= 2 {
        cases.push(crafting(
            "exact_2x2",
            2,
            2,
            pattern.place(2, 0, 0, first, false),
        ));
    }
    if pattern.cells.iter().flatten().any(|v| first(v) != last(v)) {
        cases.push(crafting("alt", 3, 3, pattern.place(3, 0, 0, last, false)));
    }
    if pattern.width < 3 || pattern.height < 3 {
        cases.push(crafting(
            "offset",
            3,
            3,
            pattern.place(3, 3 - pattern.width, 3 - pattern.height, first, false),
        ));
    }
    if !pattern.symmetric() {
        cases.push(crafting(
            "mirrored",
            3,
            3,
            pattern.place(3, 0, 0, first, true),
        ));
    }
    cases.extend(damaged("", pattern.place(3, 0, 0, first, false)));
    cases.push(crafting("empty", 3, 3, vec![String::new(); 9]));
    cases
}

/// The `wrong_item`, `missing`, `extra` and `count2` variants of an exact 3x3 grid.
fn damaged(_prefix: &str, exact: Vec<String>) -> Vec<Case> {
    let mut cases = Vec::new();
    let Some(index) = exact.iter().position(|c| !c.is_empty()) else {
        return cases;
    };
    let mut wrong = exact.clone();
    wrong[index] = WRONG.into();
    cases.push(crafting("wrong_item", 3, 3, wrong));
    let mut missing = exact.clone();
    missing[index].clear();
    cases.push(crafting("missing", 3, 3, missing));
    if let Some(empty) = exact.iter().position(String::is_empty) {
        let mut extra = exact.clone();
        extra[empty] = WRONG.into();
        cases.push(crafting("extra", 3, 3, extra));
    }
    let mut stacked = exact;
    stacked[index] = format!("{}#2", stacked[index]);
    cases.push(crafting("count2", 3, 3, stacked));
    cases
}

/// Cases of a recipe made of an ordered ingredient list (shapeless, dye, imbue, transmute).
fn list_cases(recipe: &str, ingredients: &[&Value]) -> Vec<Case> {
    let pad = |items: Vec<String>| {
        let mut grid = items;
        grid.resize(9, String::new());
        grid
    };
    let exact: Vec<String> = ingredients.iter().map(|v| first(v)).collect();
    let mut cases = vec![crafting("exact", 3, 3, pad(exact.clone()))];
    if ingredients.iter().any(|v| first(v) != last(v)) {
        cases.push(crafting(
            "alt",
            3,
            3,
            pad(ingredients.iter().map(|v| last(v)).collect()),
        ));
    }
    if exact.len() > 1 {
        cases.push(crafting(
            "reversed",
            3,
            3,
            pad(exact.iter().rev().cloned().collect()),
        ));
        // A seeded Fisher-Yates shuffle of all nine cells (the empty cells included),
        // so the ingredients land in scattered positions.
        let mut grid = pad(exact.clone());
        let mut random = rng(&format!("recipe.shuffle.{recipe}"));
        for i in (1..grid.len()).rev() {
            grid.swap(i, random.below(i as u64 + 1) as usize);
        }
        cases.push(crafting("shuffled", 3, 3, grid));
    }
    cases.extend(damaged("", pad(exact)));
    cases.push(crafting("empty", 3, 3, vec![String::new(); 9]));
    cases
}

fn single(label: &str, input: &str) -> Case {
    Case {
        label: label.into(),
        fields: vec![("kind", json!("single")), ("input", json!(input))],
    }
}

fn smithing(label: &str, template: &str, base: &str, addition: &str) -> Case {
    Case {
        label: label.into(),
        fields: vec![
            ("kind", json!("smithing")),
            ("template", json!(template)),
            ("base", json!(base)),
            ("addition", json!(addition)),
        ],
    }
}

/// Hand-written grids of the special crafting recipes, whose documents carry no
/// ingredient list. Plain stacks (no components), so most do not match; they pin
/// the matcher's accept/reject on the item level.
fn special(serializer: &str, doc: &Value) -> Vec<Case> {
    let field = |name: &str, fallback: &str| {
        doc.get(name)
            .map(first)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback.to_owned())
    };
    let pad = |cells: &[&str]| cells.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>();
    let mut cases = vec![crafting("empty", 3, 3, vec![String::new(); 9])];
    let mut add = |label: &str, width: usize, height: usize, cells: Vec<String>| {
        cases.push(crafting(label, width, height, cells));
    };
    match serializer {
        "minecraft:crafting_special_bannerduplicate" => {
            let banner = field("banner", "minecraft:white_banner");
            add("pair", 2, 1, vec![banner.clone(), banner.clone()]);
            add("single", 1, 1, vec![banner.clone()]);
            add("mixed", 2, 1, vec![banner, "minecraft:white_banner".into()]);
        }
        "minecraft:crafting_special_bookcloning" => {
            add(
                "clone",
                2,
                1,
                pad(&["minecraft:written_book", "minecraft:writable_book"]),
            );
            add("blank_only", 1, 1, pad(&["minecraft:writable_book"]));
            add("written_only", 1, 1, pad(&["minecraft:written_book"]));
        }
        "minecraft:crafting_decorated_pot" => {
            let side = |name: &str| field(name, "minecraft:brick");
            let (back, left, right, front) =
                (side("back"), side("left"), side("right"), side("front"));
            add(
                "exact",
                3,
                3,
                vec![
                    String::new(),
                    back.clone(),
                    String::new(),
                    left.clone(),
                    String::new(),
                    right.clone(),
                    String::new(),
                    front.clone(),
                    String::new(),
                ],
            );
            add(
                "wrong_item",
                3,
                3,
                vec![
                    String::new(),
                    WRONG.into(),
                    String::new(),
                    left,
                    String::new(),
                    right,
                    String::new(),
                    front,
                    String::new(),
                ],
            );
            add("single", 1, 1, vec![back]);
        }
        "minecraft:crafting_special_firework_rocket" => {
            add(
                "one_gunpowder",
                2,
                1,
                pad(&["minecraft:paper", "minecraft:gunpowder"]),
            );
            add(
                "three_gunpowder",
                4,
                1,
                pad(&[
                    "minecraft:paper",
                    "minecraft:gunpowder",
                    "minecraft:gunpowder",
                    "minecraft:gunpowder",
                ]),
            );
            add("no_paper", 1, 1, pad(&["minecraft:gunpowder"]));
        }
        "minecraft:crafting_special_firework_star" => {
            let fuel = field("fuel", "minecraft:gunpowder");
            let dye = field("dye", "minecraft:red_dye");
            add("base", 2, 1, vec![fuel.clone(), dye.clone()]);
            add("no_dye", 1, 1, vec![fuel.clone()]);
            add(
                "twinkle",
                3,
                1,
                vec![
                    fuel.clone(),
                    dye.clone(),
                    field("twinkle", "minecraft:glowstone_dust"),
                ],
            );
            add(
                "trail",
                3,
                1,
                vec![fuel, dye, field("trail", "minecraft:diamond")],
            );
        }
        "minecraft:crafting_special_firework_star_fade" => {
            add(
                "plain_star",
                2,
                1,
                pad(&["minecraft:firework_star", "minecraft:red_dye"]),
            );
            add("no_star", 1, 1, pad(&["minecraft:red_dye"]));
        }
        "minecraft:crafting_special_mapextending" => {
            let mut ring = vec!["minecraft:paper".to_owned(); 9];
            ring[4] = "minecraft:filled_map".into();
            add("ring", 3, 3, ring);
            add("map_only", 1, 1, pad(&["minecraft:filled_map"]));
        }
        "minecraft:crafting_special_repairitem" => {
            add(
                "pair",
                2,
                1,
                pad(&["minecraft:iron_pickaxe", "minecraft:iron_pickaxe"]),
            );
            add(
                "mismatch",
                2,
                1,
                pad(&["minecraft:iron_pickaxe", "minecraft:diamond_pickaxe"]),
            );
            add("single", 1, 1, pad(&["minecraft:iron_pickaxe"]));
        }
        "minecraft:crafting_special_shielddecoration" => {
            add(
                "decorate",
                2,
                1,
                pad(&["minecraft:shield", "minecraft:white_banner"]),
            );
            add("no_banner", 1, 1, pad(&["minecraft:shield"]));
        }
        other => panic!("special crafting recipe {other} has no hand-written grids"),
    }
    cases
}

fn cases_for(id: &str, doc: &Value) -> Vec<Case> {
    let serializer = doc["type"].as_str().expect("recipe type");
    let ingredient = |name: &str| &doc[name];
    match serializer {
        "minecraft:crafting_shaped" => shaped_cases(doc),
        "minecraft:crafting_shapeless" => {
            let list: Vec<&Value> = doc["ingredients"]
                .as_array()
                .expect("ingredients")
                .iter()
                .collect();
            list_cases(id, &list)
        }
        "minecraft:crafting_transmute" => {
            list_cases(id, &[ingredient("input"), ingredient("material")])
        }
        "minecraft:crafting_dye" => list_cases(id, &[ingredient("target"), ingredient("dye")]),
        "minecraft:crafting_imbue" => {
            list_cases(id, &[ingredient("source"), ingredient("material")])
        }
        "minecraft:smelting"
        | "minecraft:blasting"
        | "minecraft:smoking"
        | "minecraft:campfire_cooking"
        | "minecraft:stonecutting" => {
            let ingredient = ingredient("ingredient");
            let mut cases = vec![single("exact", &first(ingredient))];
            if first(ingredient) != last(ingredient) {
                cases.push(single("alt", &last(ingredient)));
            }
            cases.push(single("wrong_item", WRONG));
            cases.push(single("count16", &format!("{}#16", first(ingredient))));
            cases.push(single("empty", ""));
            cases
        }
        "minecraft:smithing_transform" | "minecraft:smithing_trim" => {
            let part = |name: &str, pick: fn(&Value) -> String| {
                doc.get(name).map(pick).unwrap_or_default()
            };
            let (t, b, a) = (
                part("template", first),
                part("base", first),
                part("addition", first),
            );
            let mut cases = vec![smithing("exact", &t, &b, &a)];
            if ["template", "base", "addition"]
                .iter()
                .any(|n| doc.get(*n).is_some_and(|v| first(v) != last(v)))
            {
                cases.push(smithing(
                    "alt",
                    &part("template", last),
                    &part("base", last),
                    &part("addition", last),
                ));
            }
            cases.push(smithing("wrong_template", WRONG, &b, &a));
            cases.push(smithing("wrong_base", &t, WRONG, &a));
            cases.push(smithing("wrong_addition", &t, &b, WRONG));
            cases.push(smithing("missing_template", "", &b, &a));
            cases.push(smithing("missing_base", &t, "", &a));
            cases.push(smithing("missing_addition", &t, &b, ""));
            cases.push(smithing("swapped", &b, &t, &a));
            cases.push(smithing("empty", "", "", ""));
            cases
        }
        _ => special(serializer, doc),
    }
}

pub fn emit() -> String {
    let mut out = String::new();
    for (id, doc) in &data().recipes {
        for case in cases_for(id, doc) {
            let mut fields = vec![("recipe", json!(id))];
            fields.extend(case.fields);
            out += &line(&format!("{id}/{}", case.label), fields);
        }
    }
    out
}

/// The ported-surface subject of a request: the recipe's serializer.
pub fn subject_of(request: &Map<String, Value>) -> Option<String> {
    let id = request.get("recipe")?.as_str()?;
    data()
        .recipes
        .get(id)
        .and_then(|doc| doc["type"].as_str())
        .map(str::to_owned)
}

// ---- the Rust handler ----------------------------------------------------------------

fn resolve_tag(tag: &Identifier) -> Vec<ItemId> {
    ITEM_TAG_BY_NAME
        .get(tag.to_string().as_str())
        .map(|members| {
            members
                .iter()
                .filter_map(|m| ItemId::from_name(m))
                .collect()
        })
        .unwrap_or_default()
}

type Parsed = Result<Recipe, String>;

thread_local! {
    static PARSED: RefCell<HashMap<String, Parsed>> = RefCell::new(HashMap::new());
}

fn with_recipe<R>(id: &str, f: impl FnOnce(&Parsed) -> R) -> Option<R> {
    let doc = data().recipes.get(id)?;
    Some(PARSED.with(|cache| {
        let mut cache = cache.borrow_mut();
        let parsed = cache
            .entry(id.to_owned())
            .or_insert_with(|| parse_recipe_json(doc, &resolve_tag).map_err(|e| e.to_string()));
        f(parsed)
    }))
}

fn stack(cell: &str) -> Result<ItemStack, Outcome> {
    if cell.is_empty() {
        return Ok(ItemStack::empty());
    }
    let (name, count) = match cell.split_once('#') {
        Some((name, count)) => (
            name,
            count
                .parse::<i32>()
                .map_err(|e| Outcome::Unavailable(e.to_string()))?,
        ),
        None => (cell, 1),
    };
    let item = ItemId::from_name(name)
        .ok_or_else(|| Outcome::Unavailable(format!("item {name} is not in the Rust registry")))?;
    Ok(ItemStack::new(item, count))
}

fn describe(stack: &ItemStack) -> Value {
    // BTreeMap keys iterate sorted by identifier, matching Java's sorted id list.
    let mut ids: Vec<String> = stack.components().keys().map(ToString::to_string).collect();
    ids.sort();
    json!({"item": stack.item().name(), "count": stack.count(), "components": ids})
}

fn answer(matched: bool, result: Option<ItemStack>) -> Outcome {
    let mut out = Map::new();
    out.insert("match".into(), json!(matched));
    if matched && let Some(result) = result {
        out.insert("result".into(), describe(&result));
    }
    Outcome::Accepted(out)
}

pub fn recipe_match(request: &Map<String, Value>) -> Outcome {
    let (id, kind) = match (string(request, "recipe"), string(request, "kind")) {
        (Ok(id), Ok(kind)) => (id, kind),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let outcome = with_recipe(id, |parsed| {
        let recipe = match parsed {
            Ok(recipe) => recipe,
            // Paper loads every vanilla recipe: a rejection here is a port bug, a mismatch.
            Err(_) => return Outcome::Rejected,
        };
        if let Recipe::Special { serializer, .. } = recipe {
            return Outcome::Unported(format!(
                "special recipe {serializer:?} has no matching logic"
            ));
        }
        match kind {
            "crafting" => {
                let (
                    Some(Value::Number(width)),
                    Some(Value::Number(height)),
                    Some(Value::Array(grid)),
                ) = (
                    request.get("width"),
                    request.get("height"),
                    request.get("grid"),
                )
                else {
                    return Outcome::Unavailable(
                        "crafting request needs width, height, grid".into(),
                    );
                };
                let items: Result<Vec<ItemStack>, Outcome> = grid
                    .iter()
                    .map(|cell| stack(cell.as_str().unwrap_or_default()))
                    .collect();
                let items = match items {
                    Ok(items) => items,
                    Err(e) => return e,
                };
                let (width, height) = (
                    width.as_i64().unwrap_or(0) as i32,
                    height.as_i64().unwrap_or(0) as i32,
                );
                if items.len() != (width * height) as usize {
                    return Outcome::Unavailable("grid length is not width * height".into());
                }
                let input = CraftingInput::of(width, height, items);
                if !recipe.matches_crafting(&input) {
                    return answer(false, None);
                }
                answer(true, recipe.assemble_crafting(&input))
            }
            "single" => {
                let input = match stack(
                    request
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ) {
                    Ok(input) => SingleRecipeInput::new(input),
                    Err(e) => return e,
                };
                if !recipe.matches_single(&input) {
                    return answer(false, None);
                }
                answer(true, recipe.assemble_single(&input))
            }
            "smithing" => {
                let part = |name: &str| {
                    stack(
                        request
                            .get(name)
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
                };
                let (template, base, addition) =
                    match (part("template"), part("base"), part("addition")) {
                        (Ok(t), Ok(b), Ok(a)) => (t, b, a),
                        (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => return e,
                    };
                let input = SmithingRecipeInput::new(template, base, addition);
                if !recipe.matches_smithing(&input) {
                    return answer(false, None);
                }
                answer(true, recipe.assemble_smithing(&input))
            }
            other => Outcome::Unavailable(format!("unknown recipe.match kind {other}")),
        }
    });
    outcome.unwrap_or_else(|| {
        Outcome::Unavailable(format!("recipe {id} is not in the harvested data"))
    })
}

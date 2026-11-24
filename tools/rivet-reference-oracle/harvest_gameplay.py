#!/usr/bin/env python3
"""Harvest the vanilla gameplay data the `recipe.match`, `loot.roll` and `trade.offers` corpora sweep.

Writes fixtures/ops/corpus-src/gameplay-data.json (deterministic: sorted keys and ids):

- `recipes`: every document of data/minecraft/recipe/ in the Paper jar, by id;
- `item_tags`: every item tag a recipe ingredient names (`#namespace:tag`), fully
  resolved to item ids (nested `#tag` entries expanded, order preserved,
  duplicates dropped), from data/minecraft/tags/item/;
- `loot_tables`: the id of every document of data/minecraft/loot_table/;
- `trade_sets`: the id of every document of data/minecraft/trade_set/.

and fixtures/ops/corpus-src/block-loot-tables.json: the document of every
`minecraft:blocks/*` loot table, by id (the Rust `loot.roll` handler evaluates them with
`rivet_world::block::drops::BlockLootTable`).

The Rust corpus generator (`rivet-parity ops emit-corpus`) and the Rust recipe handler
read that file, so regenerating a corpus needs no jar; only refreshing the data does.
The Java ops verify at record time that the corpora close over what the live server
loads (a recipe, loot table or trade set missing here fails the recording).

usage: harvest_gameplay.py [paper-jar]
"""
import json
import sys
import zipfile
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
default_jar = repo / "tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
jar = Path(sys.argv[1]) if len(sys.argv) > 1 else default_jar
out = repo / "tools/rivet-reference-oracle/fixtures/ops/corpus-src/gameplay-data.json"

z = zipfile.ZipFile(jar)


def documents(folder):
    prefix = f"data/minecraft/{folder}/"
    return {
        "minecraft:" + name[len(prefix):-len(".json")]: json.loads(z.read(name))
        for name in sorted(z.namelist())
        if name.startswith(prefix) and name.endswith(".json")
    }


recipes = documents("recipe")
tag_docs = documents("tags/item")


def resolve(tag, seen=()):
    if tag in seen:
        raise SystemExit(f"cyclic item tag {tag}")
    items = []
    for entry in tag_docs[tag]["values"]:
        entry = entry["id"] if isinstance(entry, dict) else entry
        members = resolve(entry[1:], seen + (tag,)) if entry.startswith("#") else [entry]
        items += [m for m in members if m not in items]
    return items


def referenced(value):
    if isinstance(value, str):
        if value.startswith("#") and ":" in value:
            yield value[1:]
    elif isinstance(value, list):
        for v in value:
            yield from referenced(v)
    elif isinstance(value, dict):
        for v in value.values():
            yield from referenced(v)


tags = sorted({t for doc in recipes.values() for t in referenced(doc)})
data = {
    "recipes": recipes,
    "item_tags": {t: resolve(t) for t in tags},
    "loot_tables": sorted(documents("loot_table")),
    "trade_sets": sorted(documents("trade_set")),
}
out.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
block_out = out.with_name("block-loot-tables.json")
block_tables = {k: v for k, v in documents("loot_table").items() if k.startswith("minecraft:blocks/")}
block_out.write_text(json.dumps(block_tables, indent=1, sort_keys=True) + "\n")
print(f"{block_out}: {len(block_tables)} block loot tables")
print(f"{out}: {len(recipes)} recipes, {len(tags)} item tags, {len(data['loot_tables'])} loot tables, {len(data['trade_sets'])} trade sets")

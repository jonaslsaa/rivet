#!/usr/bin/env python3
"""Harvest codec.reencode seeds from the vanilla data pack inside the Paper jar.

Writes fixtures/ops/corpus-src/codec-datapack.jsonl: {"codec", "source", "json"}
lines, deterministic (sorted, evenly sampled). The Rust corpus generator
(rivet-parity ops emit-corpus) reads that file, so regenerating the corpus needs
no jar; only refreshing the seeds does.

usage: harvest_datapack.py [paper-jar]
"""
import json
import sys
import zipfile
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
default_jar = repo / "tools/rivet-oracle/work/run/versions/26.2/paper-26.2.jar"
jar = Path(sys.argv[1]) if len(sys.argv) > 1 else default_jar
out = repo / "tools/rivet-reference-oracle/fixtures/ops/corpus-src/codec-datapack.jsonl"

PER_CODEC = 14
PROVIDER_TYPES = {
    "minecraft:" + t
    for t in (
        "constant uniform biased_to_bottom clamped clamped_normal weighted_list "
        "trapezoid very_biased_to_bottom"
    ).split()
}
ANCHORS = {"absolute", "above_bottom", "below_top"}

found = {}  # codec -> {canonical json text: source}


def add(codec, source, value):
    text = json.dumps(value, sort_keys=False, separators=(",", ":"))
    if len(text) > 4000:
        return
    found.setdefault(codec, {}).setdefault(text, source)


def walk(source, node, key=None):
    if isinstance(node, dict):
        t = node.get("type")
        if isinstance(t, str) and t in PROVIDER_TYPES:
            for codec in ("IntProviders.CODEC", "FloatProviders.CODEC", "HeightProvider.CODEC"):
                add(codec, source, node)
        if node and set(node) <= ANCHORS and len(node) == 1:
            add("VerticalAnchor.CODEC", source, node)
        if "Name" in node and set(node) <= {"Name", "Properties"}:
            add("BlockState.CODEC", source, node)
        if key == "components" and node:
            add("DataComponentPatch.CODEC", source, node)
        for k, v in node.items():
            walk(source, v, k)
    elif isinstance(node, list):
        for v in node:
            walk(source, v, key)
    elif isinstance(node, (int, float)) and not isinstance(node, bool):
        if key in ("count", "min_inclusive", "max_inclusive", "base", "spread", "y_offset"):
            add("IntProviders.CODEC", source, node)
            add("FloatProviders.CODEC", source, node)


with zipfile.ZipFile(jar) as z:
    names = sorted(n for n in z.namelist() if n.startswith("data/minecraft/") and n.endswith(".json"))
    for name in names:
        rel = name[len("data/minecraft/"):]
        kind = rel.split("/", 1)[0]
        doc = json.loads(z.read(name))
        if rel.startswith("worldgen/noise/"):
            add("NormalNoise.NoiseParameters.DIRECT_CODEC", rel, doc)
        elif rel.startswith("worldgen/world_preset/"):
            add("WorldPreset.DIRECT_CODEC", rel, doc)
        elif kind == "recipe":
            result = doc.get("result")
            if isinstance(result, dict):
                add("ItemStack.CODEC", rel, result)
                add("ItemStackTemplate.CODEC", rel, result)
            elif isinstance(result, str):
                add("ItemStackTemplate.CODEC", rel, result)
        elif rel.startswith("worldgen/processor_list/"):
            for proc in doc.get("processors", []):
                for rule in proc.get("rules", []):
                    add("ProcessorRule.CODEC", rel, rule)
                    for field, codec in (
                        ("input_predicate", "RuleTest.CODEC"),
                        ("location_predicate", "RuleTest.CODEC"),
                        ("position_predicate", "PosRuleTest.CODEC"),
                        ("block_entity_modifier", "RuleBlockEntityModifier.CODEC"),
                    ):
                        if field in rule:
                            add(codec, rel, rule[field])
        elif rel.startswith("worldgen/configured_feature/"):
            config = doc.get("config", {})
            if isinstance(config, dict) and "minimum_size" in config:
                add("FeatureSize.CODEC", rel, config["minimum_size"])
        if kind == "recipe":
            walk(rel, doc.get("result"))
        else:
            walk(rel, doc)

lines = []
for codec in sorted(found):
    items = sorted(found[codec].items(), key=lambda kv: (kv[1], kv[0]))
    step = max(1, len(items) // PER_CODEC)
    for text, source in items[::step][:PER_CODEC]:
        lines.append(json.dumps({"codec": codec, "source": source, "json": json.loads(text)}, separators=(",", ":")))
out.write_text("\n".join(lines) + "\n")
print(f"wrote {len(lines)} seeds for {len(found)} codecs to {out}")

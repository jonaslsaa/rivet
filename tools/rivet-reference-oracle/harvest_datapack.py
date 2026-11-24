#!/usr/bin/env python3
"""Harvest codec.reencode seeds from the vanilla data pack inside the Paper jar.

Writes fixtures/ops/corpus-src/codec-datapack.jsonl: {"codec", "source", "json"}
lines, deterministic (sorted by source file). Every distinct document this script
routes to a table codec is kept: nothing is sampled or size-capped, but only the
folders and fields named below are routed (not every vanilla document). Routing is by
data-pack folder and, for configured features and placement modifiers, by `type`
(see the tables below); codecs without a router here, or without vanilla JSON,
are covered by hand-typed corpus inputs only. The Rust corpus generator
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

PROVIDER_TYPES = {
    "minecraft:" + t
    for t in (
        "constant uniform biased_to_bottom clamped clamped_normal weighted_list "
        "trapezoid very_biased_to_bottom"
    ).split()
}
ANCHORS = {"absolute", "above_bottom", "below_top"}

# configured_feature `type` -> (codec of `config`).
FEATURE_CONFIG = {
    "minecraft:ore": "OreConfiguration.CODEC",
    "minecraft:scattered_ore": "OreConfiguration.CODEC",
    "minecraft:end_spike": "EndSpikeConfiguration.CODEC",
    "minecraft:end_gateway": "EndGatewayConfiguration.CODEC",
    "minecraft:sculk_patch": "SculkPatchConfiguration.CODEC",
    "minecraft:twisting_vines": "TwistingVinesConfig.CODEC",
    "minecraft:bamboo": "ProbabilityFeatureConfiguration.CODEC",
    "minecraft:seagrass": "ProbabilityFeatureConfiguration.CODEC",
    "minecraft:sea_pickle": "CountConfiguration.CODEC",
    "minecraft:delta_feature": "DeltaFeatureConfiguration.CODEC",
    "minecraft:netherrack_replace_blobs": "ReplaceSphereConfiguration.CODEC",
    "minecraft:underwater_magma": "UnderwaterMagmaConfiguration.CODEC",
    "minecraft:basalt_columns": "ColumnFeatureConfiguration.CODEC",
    "minecraft:iceberg": "BlockStateConfiguration.CODEC",
}
# placed_feature placement modifier `type` -> the modifier's own (MapCodec) codec.
PLACEMENT = {
    "minecraft:rarity_filter": "RarityFilter.CODEC",
    "minecraft:height_range": "HeightRangePlacement.CODEC",
    "minecraft:count_on_every_layer": "CountOnEveryLayerPlacement.CODEC",
    "minecraft:fixed_placement": "FixedPlacement.CODEC",
    "minecraft:surface_water_depth_filter": "SurfaceWaterDepthFilter.CODEC",
    "minecraft:in_square": "InSquarePlacement.CODEC",
    "minecraft:heightmap": "HeightmapPlacement.CODEC",
    "minecraft:random_offset": "RandomOffsetPlacement.CODEC",
    "minecraft:surface_relative_threshold_filter": "SurfaceRelativeThresholdFilter.CODEC",
    "minecraft:count": "CountPlacement.CODEC",
    "minecraft:noise_based_count": "NoiseBasedCountPlacement.CODEC",
    "minecraft:noise_threshold_count": "NoiseThresholdCountPlacement.CODEC",
    "minecraft:biome": "BiomeFilter.CODEC",
}
# `components` keys (loot tables, trades) whose value is a table codec.
COMPONENT = {
    "minecraft:suspicious_stew_effects": "SuspiciousStewEffects.CODEC",
    "minecraft:tooltip_display": "TooltipDisplay.CODEC",
    "minecraft:item_name": "ComponentSerialization.CODEC",
    "minecraft:custom_name": "ComponentSerialization.CODEC",
    "minecraft:tool": "Tool.CODEC",
    "minecraft:repairable": "Repairable.CODEC",
    "minecraft:damage_resistant": "DamageResistant.CODEC",
    "minecraft:map_decorations": "MapDecorations.CODEC",
    "minecraft:attribute_modifiers": "ItemAttributeModifiers.CODEC",
    "minecraft:instrument": "InstrumentComponent.CODEC",
    "minecraft:provides_trim_material": "ProvidesTrimMaterial.CODEC",
    "minecraft:blocks_attacks": "BlocksAttacks.CODEC",
    "minecraft:kinetic_weapon": "KineticWeapon.CODEC",
    "minecraft:piercing_weapon": "PiercingWeapon.CODEC",
    "minecraft:swing_animation": "SwingAnimation.CODEC",
    "minecraft:lore": "ItemLore.CODEC",
    "minecraft:enchantable": "Enchantable.CODEC",
    "minecraft:custom_model_data": "CustomModelData.CODEC",
    "minecraft:dyed_color": "DyedItemColor.CODEC",
    "minecraft:map_color": "MapItemColor.CODEC",
    "minecraft:fireworks": "Fireworks.CODEC",
    "minecraft:firework_explosion": "FireworkExplosion.CODEC",
    "minecraft:lodestone_tracker": "LodestoneTracker.CODEC",
    "minecraft:written_book_content": "WrittenBookContent.CODEC",
    "minecraft:writable_book_content": "WritableBookContent.CODEC",
    "minecraft:container_loot": "SeededContainerLoot.CODEC",
    "minecraft:block_state": "BlockItemStateProperties.CODEC",
    "minecraft:custom_data": "CustomData.CODEC",
    "minecraft:weapon": "Weapon.CODEC",
    "minecraft:attack_range": "AttackRange.CODEC",
    "minecraft:consumable": "Consumable.CODEC",
    "minecraft:death_protection": "DeathProtection.CODEC",
    "minecraft:use_cooldown": "UseCooldown.CODEC",
    "minecraft:use_remainder": "UseRemainder.CODEC",
    "minecraft:use_effects": "UseEffects.CODEC",
    "minecraft:food": "FoodProperties.DIRECT_CODEC",
}
# Vanilla pack folders whose documents hold text components, and the top-level
# fields (dotted path into the document) that carry them: routed to
# ComponentSerialization.CODEC. A field that is absent in a document is skipped.
TEXT_FIELDS = {
    "advancement": ("display.title", "display.description"),
    "painting_variant": ("title", "author"),
    "enchantment": ("description",),
    "jukebox_song": ("description",),
    "trim_pattern": ("description",),
    "trim_material": ("description",),
    "instrument": ("description",),
    "dialog": ("title", "external_title", "description", "body.contents", "body"),
}

found = {}  # codec -> {canonical json text: source}
late = {}  # the same, for documents routed after the first recorded corpus (see the output order below)


def add(codec, source, value, later=False):
    text = json.dumps(value, sort_keys=False, separators=(",", ":"))
    (late if later else found).setdefault(codec, {}).setdefault(text, source)


def text_fields(rel, kind, doc):
    for path in TEXT_FIELDS.get(kind, ()):
        node = doc
        for part in path.split("."):
            node = node.get(part) if isinstance(node, dict) else None
        yield from _components(node)


def _components(node):
    """A dialog `body` is an object or a list of objects whose `contents`/`description` are text."""
    if node is None:
        return
    if isinstance(node, list):
        for v in node:
            yield from _components(v)
    elif isinstance(node, dict) and "type" in node and node["type"] in ("minecraft:plain_message", "minecraft:item"):
        for f in ("contents", "description"):
            if f in node:
                yield from _components(node[f])
    elif isinstance(node, dict) and "contents" in node and "type" not in node:
        yield node["contents"]
    else:
        yield node


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
            for component, codec in COMPONENT.items():
                if component in node:
                    add(codec, source, node[component])
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
            if isinstance(config, dict) and "foliage_placer" in config:
                add("FoliagePlacer.CODEC", rel, config["foliage_placer"])
            codec = FEATURE_CONFIG.get(doc.get("type"))
            if codec:
                add(codec, rel, config)
            if doc.get("type") == "minecraft:end_spike":
                for spike in config.get("spikes", []):
                    add("EndSpikeFeature.EndSpike.CODEC", rel, spike)
            if doc.get("type") == "minecraft:geode":
                add("GeodeCrackSettings.CODEC", rel, config["crack"])
                add("GeodeLayerSettings.CODEC", rel, config["layers"])
        elif rel.startswith("worldgen/placed_feature/"):
            for modifier in doc.get("placement", []):
                codec = PLACEMENT.get(modifier.get("type"))
                if codec:
                    # The modifier's own codec reads fields only; the dispatch `type` is the registry's.
                    add(codec, rel, {k: v for k, v in modifier.items() if k != "type"})
        elif rel.startswith("worldgen/configured_carver/"):
            config = doc.get("config", {})
            if "shape" in config:
                add("CanyonCarverConfiguration.CanyonShapeConfiguration.CODEC", rel, config["shape"])
            if "debug_settings" in config:
                add("CarverDebugSettings.CODEC", rel, config["debug_settings"])
        elif rel.startswith("worldgen/noise_settings/"):
            if "noise" in doc:
                add("NoiseSettings.CODEC", rel, doc["noise"])
        elif rel.startswith("worldgen/biome/"):
            if "effects" in doc:
                add("BiomeSpecialEffects.CODEC", rel, doc["effects"])
                if "grass_color_modifier" in doc["effects"]:
                    add("BiomeSpecialEffects.GrassColorModifier.CODEC", rel, doc["effects"]["grass_color_modifier"])
            for cost in doc.get("spawn_costs", {}).values():
                add("MobSpawnSettings.MobSpawnCost.CODEC", rel, cost)
            if "temperature_modifier" in doc:
                add("Biome.TemperatureModifier.CODEC", rel, doc["temperature_modifier"])
        if rel.startswith("worldgen/multi_noise_biome_source_parameter_list/") and "preset" in doc:
            add("MultiNoiseBiomeSourceParameterList.Preset.CODEC", rel, doc["preset"], later=True)
        for text in text_fields(rel, kind, doc):
            add("ComponentSerialization.CODEC", rel, text, later=True)
        if kind == "recipe":
            walk(rel, doc.get("result"))
        else:
            walk(rel, doc)

# The corpus names harvested inputs by their line number (`pack<N>`), and the
# recorded goldens and exclusions.tsv are keyed by those ids. The codecs the
# first harvest covered therefore stay first, in their original order; codecs
# added later are appended after them (alphabetically), so growing the harvest
# never renumbers an input that is already recorded.
FIRST_HARVEST = {
    "BlockState.CODEC", "DataComponentPatch.CODEC", "FeatureSize.CODEC", "FloatProviders.CODEC",
    "HeightProvider.CODEC", "IntProviders.CODEC", "ItemStack.CODEC", "ItemStackTemplate.CODEC",
    "NormalNoise.NoiseParameters.DIRECT_CODEC", "PosRuleTest.CODEC", "ProcessorRule.CODEC",
    "RuleTest.CODEC", "VerticalAnchor.CODEC", "WorldPreset.DIRECT_CODEC",
}

lines = []


def emit(table, codecs):
    for codec in codecs:
        for text, source in sorted(table[codec].items(), key=lambda kv: (kv[1], kv[0])):
            lines.append(json.dumps({"codec": codec, "source": source, "json": json.loads(text)}, separators=(",", ":")))


emit(found, sorted(found, key=lambda c: (c not in FIRST_HARVEST, c)))
# Documents routed after the second harvest (text components of the pack's
# description/title/author fields, the multi-noise preset) go last, so no
# earlier input is renumbered or reseeded. A text already harvested elsewhere
# is not repeated.
for codec in late:
    late[codec] = {t: src for t, src in late[codec].items() if t not in found.get(codec, {})}
emit(late, sorted(c for c in late if late[c]))
out.write_text("\n".join(lines) + "\n")
print(f"wrote {len(lines)} seeds for {len(found.keys() | late.keys())} codecs to {out}")

#!/usr/bin/env python3
"""Write VERIFY.tsv: one row per verification surface (VERIFICATION.md section 3.1).

Run:  python3 scripts/verify_ledger.py                 # regenerate VERIFY.tsv
      python3 scripts/verify_ledger.py --check         # exit 1 if stale or malformed
      python3 scripts/verify_ledger.py record ID --status PASS --matched N \\
          --mismatched M --commit SHA [--note TEXT]    # write one row's result
      python3 scripts/verify_ledger.py record-tests --commit SHA LOG...
                                                       # results of L1 rows from
                                                       # `cargo nextest run` logs

Deterministic and offline. The file has two halves:

- Structural columns (id, layer, rust_globs, java_classes, mechanism,
  fixtures, corpus_n, negative, tier) are recomputed from the tree on every
  run. A hand edit to them is overwritten and fails `--check`.
- Result columns (status, matched, mismatched, last_verified_commit, note) are
  carried over from the committed VERIFY.tsv by row id; only the two `record`
  subcommands change them. A new row starts with its seed (below): `INACTIVE`
  for mechanisms that exist but never run, otherwise `UNVERIFIED`. Nothing is
  ever seeded `PASS`.

Rows
----
  parity.*         the five `rivet-parity` checks (L2 live op vs the Java reference
                   oracle); corpus sizes from tools/rivet-parity/fixtures/corpus-manifest.json
  probe.*          every tools/rivet-oracle/scripts/run_*_probe.sh (and the worldgen
                   sampler): L1 recorded goldens consumed by cargo tests
  oracle-kind.*    the committed fixture kinds that `rivet-oracle regen` writes and
                   `rivet-oracle verify` re-derives from a live Paper boot (L1)
  golden.*         other committed Paper-sourced goldens consumed by cargo tests (L1)
  oracle.verify.*  `rivet-oracle verify` M0 / M2 / FULL boots with `--expect-fail`
                   negatives: Paper vs its own committed baseline (L3 harness health;
                   no Rust code, so no rust_globs)
  capture.join     `rivet-capture verify` + `--expect-fail` + 9 `--mutate` kinds
  anvil.v1a        the storage-only Anvil round trip of the 432 M0 payloads
  codegen.*        the five live rivet-codegen probes (regenerate generated tables)
  scenario.*       the eight run-scenario.sh rows of gate.sh (L5)
  reference.selftest, hash.selftest, hash.paper, hash.diff (INACTIVE), generated-full (INACTIVE),
  junit.census     (INACTIVE 0/9,653: Paper JUnit runs against the adapter, L8)

Derived columns
---------------
  fixtures      `;`-joined repo paths of the row's committed corpus
  corpus_n      records in the corpus: non-empty lines of a .jsonl/.txt, non-empty
                non-`#` lines minus one header line of a .tsv, the top-level entries
                of a .json (json_records: an array's length or an object's largest
                member), one per other file; directories sum their tracked files (manifest.json and
                *.md excluded); `-` when the row has no countable corpus
  rust_globs    files the row verifies, primary files first (verify_debt.py
                spends a row's capped credit in this order). Consumers are the
                tracked `.rs` files that name a fixture path (`fixtures/<name>`).
                Module structure comes from rust_scan.module_map (crate roots
                through `mod x;`, honouring `#[path]`, `#[cfg(test)]` and
                `include!`). Primary: a production consumer itself; a test-only
                consumer (test file name, or a module declared under
                #[cfg(test)] here or in an ancestor) credits the module that
                declares it and, for `x_test.rs` / `x_tests.rs` / `x_fixture.rs`,
                its sibling `x.rs`. Secondary: the production files a test-only
                consumer `use`s (crate::, super::, rivet_*:: resolved through the
                module tree to the longest existing module; crate roots never),
                only within the consumer's own crate; a tools/ consumer's
                imports of any workspace crate. Curated rows list globs by hand
                in this file.
  java_classes  Paper classes the probe's main Java source imports, names fully
                qualified, or uses by simple name under a wildcard import (outer
                classes, resolved against COVERAGE.tsv)

record-tests
------------
For every row whose rust consumers include cargo-test code, the tests whose
nextest name lies in a consumer file's module (its module-tree path, so
`#[path]` modules resolve; or that integration-test binary) decide the
result: all passed -> PASS, any failed -> FAIL, none ran -> UNVERIFIED. matched/mismatched stay `-`: cargo reports tests, not corpus
inputs (per-input counts arrive with the generic oracle-op framework,
VERIFICATION item 3). The note records the test counts.

Known blind spots
-----------------
- A fixture path assembled at runtime (`.join("fixtures").join(name)`) is
  not seen, so its consumer is missed.
- A production consumer that loads a fixture as runtime data, not in a test,
  is credited like a test consumer.
- Consumers are found in raw text, comments included: a file that only names
  a fixture in a comment counts as a consumer.
- Result columns are carried over by row id and `--check` never re-derives
  them: a row stays PASS at its old last_verified_commit until the next
  `record-tests` (or `record`) run, even if its consumer tests changed.
- Probe -> fixture ownership is the PROBE_OUTPUTS table below (the probes
  print to stdout or an argument; nothing records which file they wrote).
  `rivet-oracle regen --check` (VERIFICATION item 2) is the real fix.
"""

import argparse
import csv
import io
import json
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import java_scan  # noqa: E402
import rust_scan  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
OUTPUT = REPO / "VERIFY.tsv"
COVERAGE = REPO / "COVERAGE.tsv"
ORACLE = "tools/rivet-oracle"
ORACLE_FIXTURES = f"{ORACLE}/fixtures"

COLUMNS = [
    "id", "layer", "rust_globs", "java_classes", "mechanism", "fixtures", "corpus_n",
    "negative", "tier", "status", "matched", "mismatched", "last_verified_commit", "note",
]
STRUCTURAL = COLUMNS[:9]
RESULT = COLUMNS[9:]
STATUSES = {"PASS", "FAIL", "UNVERIFIED", "INACTIVE"}
LAYERS = {f"L{i}" for i in range(9)}
TIERS = {"T0", "T1", "T2", "T3"}

BOOT_NOTE = "needs a Paper boot; not run by the ledger tool"
NO_NEGATIVE = "no tamper negative yet, so it cannot be PASS (VERIFICATION 2.4)"

# Probe script -> oracle fixtures it writes (paths under tools/rivet-oracle/fixtures
# or repo-relative). Names follow the script except where the probe writes several
# files or the fixture is named differently; those were matched by the probe's
# output mode (args) and the adding commit.
PROBE_OUTPUTS = {
    "run_beardifier_noise_probe.sh": ["beardifier-noise.jsonl"],
    "run_beardifier_probe.sh": ["beardifier.jsonl"],
    "run_biome_temperature_probe.sh": ["biome-temperature"],
    "run_block_entity_debug_probe.sh": ["block-entity-debug.jsonl"],
    "run_block_entity_lifecycle_probe.sh": ["block-entity-lifecycle.jsonl", "spawner-entity-id.jsonl"],
    "run_block_entity_map_order_probe.sh": ["block-entity-map-order.jsonl"],
    "run_block_entity_ticker_probe.sh": ["block-entity-ticker.jsonl"],
    "run_block_mutation_metadata_probe.sh": [
        "crates/rivet-registry/src/block_state/section_flags.tsv",
        "crates/rivet-server/src/server/level/block_mutation_callbacks.tsv",
    ],
    "run_block_pos_set_probe.sh": ["block-pos-set.tsv"],
    "run_block_shape_behavior_probe.sh": ["block-shape-behavior.jsonl", "structure-block-shape.jsonl"],
    "run_bubble_column_probe.sh": ["bubble-column.jsonl"],
    "run_builtin_pack_repository_probe.sh": ["builtin-pack-repository.jsonl"],
    "run_buried_treasure_generation_probe.sh": ["buried-treasure-generation.jsonl"],
    "run_chest_minecart_load_probe.sh": ["chest-minecart-load.jsonl"],
    "run_chest_minecart_probe.sh": ["chest-minecart.jsonl"],
    "run_chunk_block_batch_probe.sh": ["chunk-block-batch.jsonl"],
    "run_chunk_level_probe.sh": ["chunk-level"],
    "run_chunk_post_process_probe.sh": ["chunk-post-process.jsonl"],
    "run_composed_noise_probe.sh": ["composed-noise"],
    "run_data_worldgen_probe.sh": ["data-worldgen"],
    "run_dataconverter_probe.sh": ["dataconverter"],
    "run_directory_lock_probe.sh": [],
    "run_entity_lookup_probe.sh": ["entity-lookup.jsonl"],
    "run_entity_post_load_policy_probe.sh": ["entity-post-load-policy.jsonl"],
    "run_entity_region_probe.sh": ["entity-region.jsonl"],
    "run_entity_tracking_probe.sh": ["entity-tracking.jsonl"],
    "run_environment_attributes_probe.sh": [
        "environment-attributes.jsonl", "environment-boolean-maps.jsonl",
        "crates/rivet-world/src/biome/gameplay_environment.jsonl",
    ],
    "run_fallen_tree_probe.sh": ["fallen-trees.jsonl"],
    "run_flowing_fluid_probe.sh": ["flowing-fluid.jsonl", "fluid-block-states.tsv", "fluid-collision-faces.tsv"],
    "run_freeze_snow_probe.sh": ["freeze-snow.tsv"],
    "run_full_activation_lifecycle_probe.sh": ["full-activation-lifecycle.jsonl"],
    "run_generated_block_light_probe.sh": ["generated-block-light.jsonl"],
    "run_generated_boot_settings_probe.sh": ["generated-boot-settings.jsonl"],
    "run_generation_creature_load_probe.sh": ["generation-creature-load.jsonl"],
    "run_generation_farm_animal_probe.sh": [
        "generation-farm-animals.jsonl", "crates/rivet-world/src/entity/farm-variant-registries.jsonl",
    ],
    "run_generation_population_trace_probe.sh": [
        "generation-population-trace.jsonl", "generation-population-positive.jsonl",
    ],
    "run_generation_sheep_probe.sh": ["generation-sheep.jsonl"],
    "run_heart_tick_probe.sh": ["heart-tick.jsonl"],
    "run_hive_storage_probe.sh": ["hive-storage.jsonl"],
    "run_holder_readiness_probe.sh": ["holder-readiness.jsonl"],
    "run_incremental_light_probe.sh": ["incremental-light.jsonl"],
    "run_initial_spawn_probe.sh": ["initial-spawn.jsonl"],
    "run_interaction_packets_probe.sh": ["interaction-packets.jsonl"],
    "run_lazy_block_entity_probe.sh": ["lazy-block-entity.jsonl"],
    "run_lazy_structure_point_probe.sh": ["lazy-structure-point.jsonl"],
    "run_level_ticks_probe.sh": ["level-ticks.jsonl"],
    "run_lighting_core_probe.sh": [],
    "run_liquid_block_probe.sh": ["liquid-block.jsonl"],
    "run_live_block_mutation_probe.sh": ["live-block-mutation.jsonl"],
    "run_live_block_packets_probe.sh": ["live-block-packets.jsonl"],
    "run_long_reference_set_probe.sh": ["long-reference-set.tsv"],
    "run_mineshaft_authority_probe.sh": ["mineshaft-authority.jsonl"],
    "run_mineshaft_chest_probe.sh": ["mineshaft-chest.jsonl"],
    "run_mineshaft_generation_probe.sh": ["mineshaft-generation.jsonl", "mineshaft-move.jsonl"],
    "run_mineshaft_placement_probe.sh": ["mineshaft-placement.jsonl", "structure-falling-blocks.txt"],
    "run_moss_carpet_placement_probe.sh": ["moss-carpet-placement.jsonl"],
    "run_neighbor_collector_probe.sh": ["neighbor-collector.jsonl"],
    "run_placement_count_probe.sh": ["placement-count"],
    "run_plant_survival_probe.sh": ["plant-survival.tsv"],
    "run_poi_chunk_probe.sh": ["poi-chunk.jsonl"],
    "run_poi_distance_probe.sh": ["poi-distance.jsonl"],
    "run_poi_manager_probe.sh": ["poi-manager.jsonl"],
    "run_poi_record_probe.sh": ["poi-record.jsonl"],
    "run_poi_section_probe.sh": ["poi-section.jsonl"],
    "run_poi_storage_probe.sh": ["poi-storage.jsonl"],
    "run_region_deflate_probe.sh": ["region-deflate.jsonl"],
    "run_seq_probe.sh": ["seq"],
    "run_spawner_tick_probe.sh": ["spawner-tick.jsonl", "spawner-reload.jsonl"],
    "run_spline_probe.sh": ["spline"],
    "run_structure_core_probe.sh": ["structure-core.jsonl"],
    "run_structure_event_dispatch_probe.sh": ["structure-event-dispatch.jsonl"],
    "run_structure_placement_probe.sh": ["structure-placement.jsonl"],
    "run_structure_references_probe.sh": ["structure-references.jsonl"],
    "run_structure_registry_probe.sh": ["structure-registry.jsonl", "entity-types.jsonl"],
    "run_structure_ring_probe.sh": ["structure-rings.jsonl"],
    "run_structure_selection_probe.sh": ["structure-selection.jsonl"],
    "run_structure_snapshot_probe.sh": ["structure-snapshots.jsonl"],
    "run_structure_spawn_trace_probe.sh": ["structure-spawn-trace.jsonl"],
    "run_surface_column_probe.sh": ["surface-column"],
    "run_surface_rule_data_probe.sh": ["surface-rule-data"],
    "run_synth_noise_probe.sh": ["synth"],
    "run_thread_unsafe_random_probe.sh": [],
    "run_tick_container_lifecycle_probe.sh": ["tick-container-lifecycle.jsonl"],
    "run_timer_queue_probe.sh": ["timer-queue.jsonl", "timer-queue-codec.jsonl"],
    "run_tree_core_probe.sh": ["tree-core.tsv"],
    "run_tree_placement_probe.sh": ["tree-placement.jsonl"],
    "run_update_shape_probe.sh": ["block-update-shape-owners.tsv", "voxel-edge-faces.tsv"],
    "run_world_clock_bootstrap_probe.sh": ["world-clock-bootstrap.jsonl"],
    "run_world_clock_probe.sh": ["world-clock.jsonl"],
    "run_world_event_probe.sh": ["world-events.jsonl"],
    "run_world_identity_probe.sh": [],
    "run_world_runtime_settings_probe.sh": ["world-runtime-settings.jsonl"],
    "run_worldgen_sampler.sh": ["worldgen"],
}

# Fixture kinds `rivet-oracle regen` writes and `rivet-oracle verify` re-derives
# from a live Paper boot: kind -> (fixtures, producer).
ORACLE_KINDS = {
    "m0-chunks": (["chunk", "level.dat", "level.dat_old", "manifest.json"], "scripts/extract_fixtures.py"),
    "m2-regions": (["regions/overworld-normal"], "rivet-oracle regen (M2 normal-overworld regions)"),
    "full-regions": (["regions/superflat-full"], "rivet-oracle regen (superflat status-FULL regions)"),
    "text": (["text"], "scripts/extract_text_fixtures.py"),
    "generated-expected": (["generated-expected"], "rivet-oracle regen (seed-42 handoff, twin boot)"),
    "features": (["features"], "rivet-oracle regen (seed-42 FEATURES checkpoint, twin boot)"),
    "light": (["light"], "rivet-oracle regen (seed-42 LIGHT checkpoint, twin boot)"),
    "loaded-world": (["loaded-world"], "scripts/extract_loaded_world.py"),
    "block-entities": (["block-entities"], "rivet-oracle regen (block-entity payloads)"),
}

# Oracle fixture entries that are server configuration, not goldens, or that a
# dedicated row below owns.
ORACLE_NON_GOLDEN = {
    "paper-global.yml", "paper-world-defaults.yml", "server.properties", "server-full.properties",
    "server-normal.properties", "server-single-stone.properties", "environment-attributes.manifest.json",
    "world-clock-bootstrap.manifest.json", "generated-full", "chunk-hash", "regions",
}

# Other Paper-sourced goldens consumed by cargo tests: id -> (fixtures, producer).
GOLDENS = {
    "protocol-join-capture-bodies": (
        ["crates/rivet-protocol/tests/fixtures"], "Paper join capture (rivet-capture)"),
    "server-registry-data-capture": (
        ["crates/rivet-server/tests/fixtures/registry_data_capture.json"], "Paper join capture (rivet-capture)"),
    "serialization-gson-float": (
        ["crates/rivet-serialization/tests/golden/gson-float-to-string.txt"], "Gson on the pinned JDK"),
    "decode-serverbound-corpus": (
        ["tools/rivet-decode/fixtures/corpus"], "Paper-captured serverbound frames (rivet-decode)"),
    "util-mth-vectors": (["tools/rivet-codegen/data/mth_vectors.tsv"], "rivet-codegen Mth golden probe"),
    "fluid-block-behaviors": (["crates/rivet-world/src/fluid/block_behaviors.tsv"], "rivet-codegen probe-block-behaviors"),
}
# Goldens whose vectors are compiled into a generated test file instead of being
# read by path: golden id -> consumer files.
GOLDEN_CONSUMERS = {
    "util-mth-vectors": ["crates/rivet-util/src/mth_golden_tests.rs"],
}

CURATED: list[dict] = [
    # -- rivet-parity: live differential against the Java reference oracle (L2).
    {"id": "parity.nbt.decode", "layer": "L2", "tier": "T1", "negative": "n",
     "mechanism": "op:rivet-parity nbt.decode", "corpus": ("parity", "nbt.decode"),
     "fixtures": ["tools/rivet-parity/fixtures/corpus-manifest.json", "tools/rivet-oracle/fixtures/chunk"],
     "rust_globs": ["crates/rivet-nbt/src/nbt_io.rs", "crates/rivet-nbt/src/*_tag.rs",
                    "crates/rivet-nbt/src/tag.rs", "crates/rivet-nbt/src/string_tag_visitor.rs",
                    "crates/rivet-nbt/src/snbt_printer_tag_visitor.rs"],
     "java_classes": ["net.minecraft.nbt.NbtIo", "net.minecraft.nbt.SnbtPrinterTagVisitor",
                      "net.minecraft.nbt.StringTagVisitor"],
     "note": f"{BOOT_NOTE}; {NO_NEGATIVE}"},
    {"id": "parity.snbt.parse", "layer": "L2", "tier": "T1", "negative": "n",
     "mechanism": "op:rivet-parity snbt.parse", "corpus": ("parity", "snbt.parse"),
     "fixtures": ["tools/rivet-parity/fixtures/corpus-manifest.json"],
     "rust_globs": ["crates/rivet-nbt/src/tag_parser.rs", "crates/rivet-nbt/src/snbt_grammar.rs",
                    "crates/rivet-nbt/src/snbt_operations.rs", "crates/rivet-nbt/src/string_tag_visitor.rs",
                    "crates/rivet-nbt/src/snbt_printer_tag_visitor.rs"],
     "java_classes": ["net.minecraft.nbt.TagParser", "net.minecraft.nbt.SnbtGrammar",
                      "net.minecraft.nbt.SnbtOperations", "net.minecraft.nbt.StringTagVisitor",
                      "net.minecraft.nbt.SnbtPrinterTagVisitor"],
     "note": f"{BOOT_NOTE}; {NO_NEGATIVE}"},
    {"id": "parity.nbt.encode", "layer": "L2", "tier": "T1", "negative": "n",
     "mechanism": "op:rivet-parity nbt.encode", "corpus": ("parity", "nbt.encode"),
     "fixtures": ["tools/rivet-parity/fixtures/corpus-manifest.json"],
     "rust_globs": ["crates/rivet-nbt/src/nbt_io.rs", "crates/rivet-nbt/src/compound_tag.rs"],
     "java_classes": ["net.minecraft.nbt.NbtIo", "net.minecraft.nbt.CompoundTag"],
     "note": f"{BOOT_NOTE}; {NO_NEGATIVE}; 437 of 467 were semantic-only (compound_key_order) on the last scoreboard"},
    {"id": "parity.idem", "layer": "L0", "tier": "T1", "negative": "n",
     "mechanism": "self-check:rivet-parity idem (Rust read-write-read, no Paper)", "corpus": ("parity", "idem"),
     "fixtures": ["tools/rivet-parity/fixtures/corpus-manifest.json"],
     "rust_globs": ["crates/rivet-nbt/src/nbt_io.rs"], "java_classes": [],
     "note": "Rust-only round trip: no Java-sourced expectation, earns no verification credit"},
    {"id": "parity.component.json", "layer": "L2", "tier": "T1", "negative": "n",
     "mechanism": "op:rivet-parity component.json", "corpus": ("parity", "component.json"),
     "fixtures": ["tools/rivet-parity/fixtures/corpus-manifest.json", "tools/rivet-oracle/fixtures/text"],
     "rust_globs": ["crates/rivet-text/src/**"],
     "java_classes": ["net.minecraft.network.chat.ComponentSerialization"],
     "note": f"{BOOT_NOTE}; {NO_NEGATIVE}"},
    # -- rivet-oracle verify: Paper against its own committed baseline (L3 harness).
    {"id": "oracle.verify.m0", "layer": "L3", "tier": "T2", "negative": "y",
     "mechanism": "harness:rivet-oracle verify (+ --expect-fail)", "fixtures": ["tools/rivet-oracle/fixtures/manifest.json"],
     "corpus": ("files", "tools/rivet-oracle/fixtures/chunk"), "rust_globs": [], "java_classes": [],
     "note": BOOT_NOTE + "; proves Paper == committed baseline, not Rivet parity"},
    {"id": "oracle.verify.m2", "layer": "L3", "tier": "T2", "negative": "y",
     "mechanism": "harness:rivet-oracle verify --m2 (+ --expect-fail)",
     "fixtures": ["tools/rivet-oracle/fixtures/regions/overworld-normal"],
     "corpus": ("files", "tools/rivet-oracle/fixtures/regions/overworld-normal"), "rust_globs": [], "java_classes": [],
     "note": BOOT_NOTE + "; proves Paper == committed baseline, not Rivet parity"},
    {"id": "oracle.verify.full", "layer": "L3", "tier": "T2", "negative": "y",
     "mechanism": "harness:rivet-oracle verify --full (+ --expect-fail)",
     "fixtures": ["tools/rivet-oracle/fixtures/regions/superflat-full"],
     "corpus": ("files", "tools/rivet-oracle/fixtures/regions/superflat-full"), "rust_globs": [], "java_classes": [],
     "note": BOOT_NOTE + "; proves Paper == committed baseline, not Rivet parity"},
    {"id": "oracle.verify.all-kinds", "layer": "L3", "tier": "T2", "negative": "n",
     "mechanism": "harness:rivet-oracle verify (all committed fixture kinds: manifest SHA-256s + checkpoint contracts)",
     "fixtures": [], "corpus": None, "rust_globs": [], "java_classes": [],
     "note": "offline manifest/contract check of every fixture kind; not run by the ledger tool"},
    {"id": "capture.join", "layer": "L3", "tier": "T2", "negative": "y",
     "mechanism": "harness:rivet-capture verify (+ --expect-fail, 9 --mutate kinds)",
     "fixtures": ["tools/rivet-capture/fixtures/join/capture.jsonl"],
     "corpus": ("lines", "tools/rivet-capture/fixtures/join/capture.jsonl"), "rust_globs": [], "java_classes": [],
     "note": BOOT_NOTE + "; proves Paper's join == committed capture"},
    {"id": "anvil.v1a", "layer": "L1", "tier": "T2", "negative": "y",
     "mechanism": "test:tools/rivet-oracle/scripts/test_anvil_roundtrip_v1a.sh (rivet-oracle anvil-roundtrip)",
     "fixtures": ["tools/rivet-oracle/fixtures/chunk"], "corpus": ("files", "tools/rivet-oracle/fixtures/chunk"),
     "rust_globs": ["crates/rivet-world/src/chunk/storage/region_file.rs",
                    "crates/rivet-world/src/chunk/storage/region_file_version.rs",
                    "crates/rivet-world/src/chunk/storage/region_file_storage.rs"],
     "java_classes": ["net.minecraft.world.level.chunk.storage.RegionFile",
                      "net.minecraft.world.level.chunk.storage.RegionFileVersion",
                      "net.minecraft.world.level.chunk.storage.RegionFileStorage"],
     "note": "offline, not run by the ledger tool"},
    {"id": "reference.selftest", "layer": "L3", "tier": "T2", "negative": "n",
     "mechanism": "harness:tools/rivet-reference-oracle self-test", "fixtures": [], "corpus": None,
     "rust_globs": [], "java_classes": [], "note": BOOT_NOTE},
    {"id": "hash.selftest", "layer": "L3", "tier": "T2", "negative": "n",
     "mechanism": "harness:rivet-oracle hash-self-check (xxh3_64 known-answer vectors, gate.sh run_oracle_hash)",
     "fixtures": [], "corpus": None, "rust_globs": [], "java_classes": [],
     "note": "offline known-answer check, run by gate.sh, not by the ledger tool"},
    {"id": "hash.paper", "layer": "L3", "tier": "T2", "negative": "n",
     "mechanism": "harness:rivet-oracle hash-paper (rebuild committed Paper FULL-chunk manifest)",
     "fixtures": ["tools/rivet-oracle/fixtures/chunk-hash/paper"], "corpus": None,
     "rust_globs": [], "java_classes": [], "note": "Paper-only manifest; no Rivet side"},
    {"id": "hash.diff", "layer": "L3", "tier": "T2", "negative": "y", "seed_status": "INACTIVE",
     "mechanism": "op:rivet-oracle hash-diff (Paper vs Rivet FULL-chunk xxh3_64, + --expect-fail)",
     "fixtures": ["tools/rivet-oracle/fixtures/chunk-hash/paper"], "corpus": None,
     "rust_globs": ["crates/rivet-world/src/levelgen/**", "crates/rivet-world/src/chunk/**",
                    "crates/rivet-server/src/server/level/**"],
     "java_classes": [],
     "note": "INACTIVE: opt-in via RIVET_HASH_DIR; no Rivet FULL manifest exists yet (VERIFICATION item 6)"},
    {"id": "generated-full", "layer": "L3", "tier": "T2", "negative": "y", "seed_status": "INACTIVE",
     "mechanism": "op:rivet-oracle verify-generated-full (+ --tamper all)",
     "fixtures": ["tools/rivet-oracle/fixtures/generated-full"], "corpus": None,
     "rust_globs": ["crates/rivet-world/src/levelgen/**", "crates/rivet-server/src/server/level/**"],
     "java_classes": [],
     "note": "INACTIVE: opt-in via RIVET_GENERATED_FULL; promotion inactive (issue #175)"},
    # -- live codegen probes: regenerate the generated tables against Paper (L1).
    *[
        {"id": f"codegen.{name}", "layer": "L1", "tier": "T2", "negative": "n",
         "mechanism": f"probe:rivet-codegen {name}", "fixtures": [], "corpus": None,
         "rust_globs": ["crates/rivet-registry/src/generated/**"], "java_classes": [],
         "note": BOOT_NOTE + "; verifies generated tables (excluded from debt LOC)"}
        for name in ("probe-block-states", "probe-biomes-tags", "probe-block-behaviors",
                     "probe-worldgen", "probe-feature-data")
    ],
    # -- scenarios (L5): tools/rivet-client/run-scenario.sh as gate.sh runs them.
    {"id": "scenario.join.paper", "layer": "L5", "tier": "T2", "negative": "y",
     "mechanism": "scenario:run-scenario.sh join (Paper-vs-Paper + negative case)", "fixtures": [],
     "corpus": ("const", 1), "rust_globs": [], "java_classes": [],
     "note": BOOT_NOTE + "; harness health (no Rivet side)"},
    {"id": "scenario.join.both", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh join --server both (Paper-vs-Rivet play transcript)",
     "fixtures": [], "corpus": ("const", 1),
     "rust_globs": ["crates/rivet-server/src/server/network/**", "crates/rivet-server/src/server/player/**"],
     "java_classes": ["net.minecraft.server.network.ServerLoginPacketListenerImpl",
                      "net.minecraft.server.network.ServerConfigurationPacketListenerImpl",
                      "net.minecraft.server.players.PlayerList"],
     "note": f"{BOOT_NOTE}; {NO_NEGATIVE} (the negative runs in scenario.join.paper)"},
    {"id": "scenario.move.both", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh move --server both (Paper-vs-Rivet authoritative walk)",
     "fixtures": [], "corpus": ("const", 1),
     "rust_globs": ["crates/rivet-server/src/server/movement_math.rs", "crates/rivet-server/src/server/movement_trace.rs",
                    "crates/rivet-server/src/server/teleport_ack.rs"],
     "java_classes": ["net.minecraft.server.network.ServerGamePacketListenerImpl"],
     "note": f"{BOOT_NOTE}; {NO_NEGATIVE}"},
    {"id": "scenario.loaded-world", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh loaded-world (official client vs a copied Paper world)",
     "fixtures": ["tools/rivet-oracle/fixtures/loaded-world"], "corpus": ("const", 1),
     "rust_globs": [], "java_classes": [], "note": BOOT_NOTE + "; Rivet-only acceptance"},
    {"id": "scenario.recenter", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh recenter (sustained walking, chunk-view recentering)",
     "fixtures": [], "corpus": ("const", 1), "rust_globs": [], "java_classes": [],
     "note": BOOT_NOTE + "; Rivet-only acceptance"},
    {"id": "scenario.generated-world", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh generated-world (seed-42 generated world)",
     "fixtures": ["tools/rivet-oracle/fixtures/generated-expected"], "corpus": ("const", 1),
     "rust_globs": [], "java_classes": [], "note": "pinned UNVERIFIED by gate.sh"},
    {"id": "scenario.dwell", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh dwell --server rivet (keepalive survival)", "fixtures": [],
     "corpus": ("const", 1), "rust_globs": [], "java_classes": [],
     "note": "Rivet-only wall-clock threshold; no Paper comparison"},
    {"id": "scenario.kick", "layer": "L5", "tier": "T2", "negative": "n",
     "mechanism": "scenario:run-scenario.sh kick --server rivet (decoded disconnect reason)", "fixtures": [],
     "corpus": ("const", 1), "rust_globs": [], "java_classes": [],
     "note": "Rivet-only; no Paper comparison"},
    # -- Paper JUnit: runs against the adapter at M4 (L8).
    {"id": "junit.census", "layer": "L8", "tier": "T2", "negative": "n", "seed_status": "INACTIVE",
     "seed_counts": ("0", "-"),
     "mechanism": "junit:tools/paper-junit-baseline (Paper JUnit against the JVM adapter)",
     "fixtures": ["tools/paper-junit-baseline/fixtures/baseline-census.json"], "corpus": ("census", None),
     "rust_globs": ["crates/rivet-ffi/src/**"], "java_classes": [],
     "note": "INACTIVE: 0 of the census's executed Paper JUnit tests run against Rivet (adapter not built)"},
]


# ---------------------------------------------------------------- helpers

def git_files() -> list[str]:
    out = subprocess.run(["git", "ls-files", "-z"], cwd=REPO, capture_output=True, text=True, check=True)
    return sorted(p for p in out.stdout.split("\0") if p)


def tracked_entries(directory: str, tracked: list[str]) -> list[str]:
    """Names of the tracked files and directories directly under `directory`
    (untracked local leftovers are not part of the ledger)."""
    prefix = directory + "/"
    return sorted({t[len(prefix):].split("/", 1)[0] for t in tracked if t.startswith(prefix)})


def is_tracked(path: str, tracked: list[str]) -> bool:
    """`path` is a tracked file or a directory holding tracked files."""
    return any(t == path or t.startswith(path + "/") for t in tracked)


def fixture_path(name: str) -> str:
    return name if "/" in name and (name.startswith("crates/") or name.startswith("tools/")) \
        else f"{ORACLE_FIXTURES}/{name}"


def file_records(path: Path) -> int:
    """Records in one corpus file (see the module docstring)."""
    name = path.name
    if name.endswith((".jsonl", ".txt")):
        return sum(1 for line in path.read_text(encoding="utf-8", errors="replace").splitlines() if line.strip())
    if name.endswith(".tsv"):
        lines = [ln for ln in path.read_text(encoding="utf-8", errors="replace").splitlines()
                 if ln.strip() and not ln.startswith("#")]
        return max(len(lines) - 1, 0)
    if name.endswith(".json"):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except ValueError:
            return 1
        return max(json_records(data), 1)
    return 1


def json_records(data) -> int:
    """Top-level entries of a JSON document: an array's length, or an object's
    largest array/object member (`{"chunks": {25 entries}}` -> 25). Deeper
    nesting is per-input detail, not more inputs, so it never adds records."""
    if isinstance(data, list):
        return len(data)
    if isinstance(data, dict):
        return max((len(v) for v in data.values() if isinstance(v, (list, dict))), default=0)
    return 0


def count_corpus(paths: list[str], tracked: list[str]) -> str:
    total = 0
    seen = False
    for p in paths:
        if p in tracked:
            seen = True
            # A manifest describes the corpus files next to it; it is no input.
            total += 0 if p.endswith("manifest.json") else file_records(REPO / p)
        else:
            files = [t for t in tracked if t.startswith(p + "/")
                     and not t.endswith("manifest.json") and not t.endswith(".md")]
            total += sum(file_records(REPO / t) for t in files)
            seen = seen or bool(files)
    return str(total) if seen else "-"


def curated_corpus(spec, tracked: list[str]) -> str:
    if spec is None:
        return "-"
    kind, arg = spec
    if kind == "const":
        return str(arg)
    if kind == "files":
        return count_corpus([arg], tracked)
    if kind == "lines":
        return count_corpus([arg], tracked)
    if kind == "census":
        data = json.loads((REPO / "tools/paper-junit-baseline/fixtures/baseline-census.json").read_text())
        return str(data["totals"]["executed"])
    if kind == "parity":
        exp = json.loads((REPO / "tools/rivet-parity/fixtures/corpus-manifest.json").read_text())["expected"]
        chunks = exp["chunk_nbt"]
        return str({
            "nbt.decode": chunks, "idem": chunks,
            "snbt.parse": chunks + exp["snbt_valid"] + exp["snbt_invalid"],
            "nbt.encode": chunks + exp["nbt_encode"],
            "component.json": exp["text_entries"],
        }[arg])
    raise ValueError(kind)


# ------------------------------------------------- rust consumers -> globs

USE_RE = re.compile(r"\buse\s+([^;]+);")


def expand_use(tree: str) -> list[str]:
    """`a::{b, c::{d, e}}` -> [a::b, a::c::d, a::c::e] (whitespace removed)."""
    tree = re.sub(r"\s+as\s+[A-Za-z_]\w*", "", tree)
    tree = re.sub(r"\s+", "", tree)
    if "{" not in tree:
        return [tree]
    i = tree.index("{")
    prefix = tree[:i]
    depth = 0
    for j in range(i, len(tree)):
        if tree[j] == "{":
            depth += 1
        elif tree[j] == "}":
            depth -= 1
            if depth == 0:
                break
    inner = tree[i + 1:j]
    parts, depth, cur = [], 0, ""
    for ch in inner:
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
            continue
        depth += ch == "{"
        depth -= ch == "}"
        cur += ch
    if cur:
        parts.append(cur)
    out = []
    for p in parts:
        out.extend(expand_use(prefix + p) if p else [])
    return out


class RustTree:
    def __init__(self, tracked: list[str]):
        self.files = {p for p in tracked if p.endswith(".rs")}
        self._text_cache: dict[str, str] = {}
        self._test_only: dict[str, bool] = {}
        self.crate_roots = {}
        for p in sorted(self.files):
            parts = p.split("/")
            if len(parts) == 4 and parts[0] in ("crates", "tools") and parts[2] == "src" \
                    and parts[3] in ("lib.rs", "main.rs"):
                self.crate_roots.setdefault(parts[1].replace("-", "_"), p)
        self.modules = rust_scan.module_map(self.files, self._text)
        self.by_module = {(m.root, m.path): f for f, m in sorted(self.modules.items())}

    def _text(self, path: str) -> str:
        if path not in self._text_cache:
            self._text_cache[path] = (REPO / path).read_text(encoding="utf-8", errors="replace")
        return self._text_cache[path]

    def module_file(self, root: str, segs: list[str]) -> str | None:
        """File of the longest prefix of module path `segs` under crate root
        file `root` (never the root itself)."""
        for n in range(len(segs), 0, -1):
            f = self.by_module.get((root, tuple(segs[:n])))
            if f:
                return f
        return None

    def resolve_use(self, consumer: str, path: str) -> str | None:
        """File a `use` path names, via the module tree; None outside it."""
        segs = [s for s in path.split("::") if s and s != "*" and s != "self"]
        if not segs:
            return None
        if segs[0] in self.crate_roots:
            return self.module_file(self.crate_roots[segs[0]], segs[1:])
        info = self.modules.get(consumer)
        if info is None:
            return None
        if segs[0] == "crate":
            return self.module_file(info.root, segs[1:])
        if segs[0] == "super":
            base = list(info.path)
            while segs and segs[0] == "super":
                base = base[:-1]
                segs = segs[1:]
            return self.module_file(info.root, base + segs)
        return None

    def is_test_only(self, path: str, text: str) -> bool:
        """Test code: a test file name, no production lines, or a module
        declared under #[cfg(test)] (itself or an ancestor)."""
        if path not in self._test_only:
            info = self.modules.get(path)
            self._test_only[path] = rust_scan.is_test_file(Path(path)) \
                or (info is not None and info.test_only) or rust_scan.production_line_count(text) == 0
        return self._test_only[path]

    def globs_for(self, consumer: str, text: str) -> tuple[set[str], set[str]]:
        """(primary, secondary) files a consumer verifies. Primary: a
        production consumer itself; for a test-only consumer, the module that
        declares it and its `x_test(s).rs`/`x_fixture.rs` sibling `x.rs`.
        Secondary: production files a test-only (or tools/) consumer `use`s,
        limited to the consumer's own crate for a crates/ consumer (imports
        from other crates are fixture plumbing: SNBT parsing, registries)."""
        primary: set[str] = set()
        secondary: set[str] = set()
        test_only = self.is_test_only(consumer, text)
        in_crates = consumer.startswith("crates/")
        if not test_only and in_crates:
            primary.add(consumer)
        if test_only:
            stem = re.sub(r"_(?:tests?|fixture)\.rs$", ".rs", consumer)
            if stem != consumer and stem in self.files and not self.is_test_only(stem, self._text(stem)):
                primary.add(stem)
            info = self.modules.get(consumer)
            parent = info.parent if info else None
            if parent and parent.startswith("crates/") and self.modules[parent].parent is not None \
                    and not self.is_test_only(parent, self._text(parent)):
                primary.add(parent)
        if test_only or not in_crates:
            own_crate = "/".join(consumer.split("/")[:2]) + "/"
            code = rust_scan.blank_comments_and_strings(text)
            for m in USE_RE.finditer(code):
                for path in expand_use(m.group(1)):
                    target = self.resolve_use(consumer, path)
                    if target and target.startswith("crates/") and target in self.modules \
                            and self.modules[target].parent is not None \
                            and (not in_crates or target.startswith(own_crate)) \
                            and not self.is_test_only(target, self._text(target)):
                        secondary.add(target)
        return primary, secondary - primary

    def test_scope(self, consumer: str) -> tuple[str, str] | None:
        """(nextest binary id, module prefix) of the tests a consumer file holds."""
        parts = consumer.split("/")
        if len(parts) < 4 or parts[0] not in ("crates", "tools"):
            return None
        crate = parts[1]
        if parts[2] == "tests":
            return (f"{crate}::{Path(parts[3]).stem}", "") if len(parts) == 4 else None
        info = self.modules.get(consumer)
        if info is None or not info.path:
            return None
        return (crate, "::".join(info.path))


def consumers_of(fixtures: list[str], rs_texts: dict[str, str]) -> list[str]:
    pats = []  # (pattern, path prefix a consumer must have, or "")
    for f in fixtures:
        if f.endswith("/fixtures"):
            # A crate's own fixtures directory (`crates/x/tests/fixtures`): its
            # basename names every fixture in the tree, so only that crate's
            # files referencing `tests/fixtures/...` consume it.
            parts = f.split("/")
            pats.append((re.compile(re.escape("/".join(parts[2:]) + "/")), "/".join(parts[:2]) + "/"))
            continue
        rel = f.split("/fixtures/", 1)[1] if "/fixtures/" in f else f
        anchor = f"fixtures/{rel}" if "/fixtures/" in f else rel
        pats.append((re.compile(re.escape(anchor) + r"(?![\w.-])"), ""))
        if "/fixtures/" not in f:
            pats.append((re.compile(re.escape(Path(f).name) + r"(?![\w.-])"), ""))
    found = []
    for path, text in rs_texts.items():
        if any(path.startswith(pre) and p.search(text) for p, pre in pats):
            found.append(path)
    return sorted(found)


# ------------------------------------------------------------ java classes

def load_java_classes() -> set[str]:
    with COVERAGE.open(newline="", encoding="utf-8") as fh:
        return {row["java_class"] for row in csv.DictReader(fh, delimiter="\t")}


QUALIFIED_RE = re.compile(r"\b(?:net\.minecraft|ca\.spottedleaf|io\.papermc|com\.mojang)(?:\.[A-Za-z_$][\w$]*)+")


def probe_java_classes(script_text: str, known: set[str], tracked: list[str]) -> list[str]:
    mains = sorted(set(re.findall(r"\b([A-Z]\w*(?:Probe|Sampler))\b(?!\.java)", script_text)))
    out: set[str] = set()
    for main in mains:
        rel = f"{ORACLE}/src/java/{main}.java"
        if rel not in tracked:
            continue
        src = REPO / rel
        text = src.read_text(encoding="utf-8", errors="replace")
        code = java_scan.blank_comments_and_strings(text)
        tokens = set(re.findall(r"[A-Za-z_$][\w$]*", code))
        for imp in java_scan.java_imports(text):
            if imp.endswith(".*"):
                pkg = imp[:-2]
                out.update(f"{pkg}.{t}" for t in tokens if f"{pkg}.{t}" in known)
        names = [i for i in java_scan.java_imports(text) if not i.endswith(".*")] + QUALIFIED_RE.findall(code)
        for name in names:
            parts = name.split(".")
            for n in range(len(parts), 1, -1):
                cand = ".".join(parts[:n])
                if cand in known:
                    out.add(cand)
                    break
    return sorted(out)


# ---------------------------------------------------------------- build

def build_rows(tree: "RustTree | None" = None) -> list[dict[str, str]]:
    tracked = git_files()
    rs_texts = {p: (REPO / p).read_text(encoding="utf-8", errors="replace")
                for p in tracked if p.endswith(".rs") and (p.startswith("crates/") or p.startswith("tools/"))
                and not p.startswith("tools/rivet-codegen/")}
    tree = tree or RustTree(tracked)
    known = load_java_classes()

    scripts_dir = REPO / ORACLE / "scripts"
    probe_scripts = [n for n in tracked_entries(f"{ORACLE}/scripts", tracked)
                     if re.match(r"run_.*_probe\.sh$", n) or n == "run_worldgen_sampler.sh"]
    unmapped = sorted(set(probe_scripts) ^ set(PROBE_OUTPUTS))
    if unmapped:
        raise SystemExit(f"PROBE_OUTPUTS out of sync with tracked {ORACLE}/scripts: {unmapped}")

    claimed: set[str] = set()
    rows: list[dict] = []

    def golden_row(rid: str, mechanism: str, fixtures: list[str], java: list[str], note: str,
                   extra_consumers: tuple[str, ...] = ()) -> dict:
        paths = [fixture_path(f) for f in fixtures]
        for p in paths + list(extra_consumers):
            if not is_tracked(p, tracked):
                raise SystemExit(f"{rid}: {p} is not tracked")
        claimed.update(paths)
        consumers = sorted(set(consumers_of(paths, rs_texts)) | set(extra_consumers))
        primary: set[str] = set()
        secondary: set[str] = set()
        for c in consumers:
            p, q = tree.globs_for(c, rs_texts[c])
            primary |= p
            secondary |= q
        # Primary files first: verify_debt.py spends a row's capped credit in this order.
        globs = sorted(primary) + sorted(secondary - primary)
        return {
            "id": rid, "layer": "L1", "rust_globs": globs, "java_classes": java,
            "mechanism": mechanism, "fixtures": paths, "corpus_n": count_corpus(paths, tracked),
            "negative": "n", "tier": "T0", "consumers": consumers,
            "seed_status": "UNVERIFIED",
            "note": note if consumers else (note + "; " if note else "") + "no Rust consumer found",
        }

    for script in probe_scripts:
        text = (scripts_dir / script).read_text(encoding="utf-8", errors="replace")
        name = re.sub(r"^run_|(_probe)?\.sh$", "", script).replace("_", "-")
        outputs = PROBE_OUTPUTS[script]
        java = probe_java_classes(text, known, tracked)
        note = "" if outputs else "output fixture not identified (probe writes stdout or a test-time file)"
        rows.append(golden_row(f"probe.{name}", f"probe:{ORACLE}/scripts/{script}", outputs, java, note))

    for kind, (fixtures, producer) in sorted(ORACLE_KINDS.items()):
        rows.append(golden_row(f"oracle-kind.{kind}", f"regen:{producer}", fixtures, [], ""))

    for gid, (fixtures, producer) in sorted(GOLDENS.items()):
        rows.append(golden_row(f"golden.{gid}", f"golden:{producer}", fixtures, [], "",
                               tuple(GOLDEN_CONSUMERS.get(gid, ()))))

    oracle_entries = tracked_entries(ORACLE_FIXTURES, tracked)
    leftover = [e for e in oracle_entries
                if f"{ORACLE_FIXTURES}/{e}" not in claimed and e not in ORACLE_NON_GOLDEN]
    if leftover:
        raise SystemExit(f"oracle fixtures without a producing row: {leftover}")

    for spec in CURATED:
        fixtures = spec["fixtures"]
        for p in fixtures:
            if not is_tracked(p, tracked):
                raise SystemExit(f"{spec['id']}: fixture {p} is not tracked")
        row = {
            "id": spec["id"], "layer": spec["layer"], "rust_globs": spec["rust_globs"],
            "java_classes": spec["java_classes"], "mechanism": spec["mechanism"], "fixtures": fixtures,
            "corpus_n": curated_corpus(spec["corpus"], tracked), "negative": spec["negative"],
            "tier": spec["tier"], "consumers": [], "seed_status": spec.get("seed_status", "UNVERIFIED"),
            "note": spec["note"],
        }
        if "seed_counts" in spec:
            row["seed_counts"] = spec["seed_counts"]
        rows.append(row)

    rows.sort(key=lambda r: r["id"])
    ids = [r["id"] for r in rows]
    if len(ids) != len(set(ids)):
        raise SystemExit("duplicate VERIFY row ids")
    return rows


def load_results(path: Path) -> dict[str, dict[str, str]]:
    if not path.exists():
        return {}
    with path.open(newline="", encoding="utf-8") as fh:
        return {r["id"]: r for r in csv.DictReader(fh, delimiter="\t", quoting=csv.QUOTE_NONE)}


def render(rows: list[dict], previous: dict[str, dict[str, str]]) -> str:
    buf = io.StringIO()
    buf.write("\t".join(COLUMNS) + "\n")
    for r in rows:
        prev = previous.get(r["id"])
        if prev:
            result = {k: prev.get(k, "") for k in RESULT}
        else:
            matched, mismatched = r.get("seed_counts", ("-", "-"))
            result = {"status": r["seed_status"], "matched": matched, "mismatched": mismatched,
                      "last_verified_commit": "-", "note": r["note"] or "-"}
        cells = {
            "id": r["id"], "layer": r["layer"], "rust_globs": ";".join(r["rust_globs"]) or "-",
            "java_classes": ";".join(r["java_classes"]) or "-", "mechanism": r["mechanism"],
            "fixtures": ";".join(r["fixtures"]) or "-", "corpus_n": r["corpus_n"],
            "negative": r["negative"], "tier": r["tier"], **result,
        }
        for k, v in cells.items():
            if "\t" in v or "\n" in v:
                raise SystemExit(f"{r['id']}.{k} contains a tab or newline")
        buf.write("\t".join(cells[k] for k in COLUMNS) + "\n")
    return buf.getvalue()


def validate(text: str) -> list[str]:
    """Malformed-ledger findings for a rendered VERIFY.tsv."""
    errors = []
    lines = text.splitlines()
    if not lines or lines[0].split("\t") != COLUMNS:
        return ["header does not match the VERIFY.tsv columns"]
    seen = set()
    for n, line in enumerate(lines[1:], start=2):
        cells = line.split("\t")
        if len(cells) != len(COLUMNS):
            errors.append(f"line {n}: {len(cells)} cells, expected {len(COLUMNS)}")
            continue
        r = dict(zip(COLUMNS, cells))
        if r["id"] in seen:
            errors.append(f"line {n}: duplicate id {r['id']}")
        seen.add(r["id"])
        if r["layer"] not in LAYERS:
            errors.append(f"{r['id']}: bad layer {r['layer']}")
        if r["tier"] not in TIERS:
            errors.append(f"{r['id']}: bad tier {r['tier']}")
        if r["negative"] not in ("y", "n"):
            errors.append(f"{r['id']}: negative must be y or n")
        if r["status"] not in STATUSES:
            errors.append(f"{r['id']}: bad status {r['status']}")
        for k in ("corpus_n", "matched", "mismatched"):
            if r[k] != "-" and not r[k].isdigit():
                errors.append(f"{r['id']}: {k} must be a count or -")
        if r["status"] in ("PASS", "FAIL") and r["last_verified_commit"] in ("", "-"):
            errors.append(f"{r['id']}: {r['status']} without last_verified_commit")
        if r["status"] == "PASS" and r["layer"] in {"L2", "L3", "L4", "L5", "L6"} and r["negative"] != "y":
            errors.append(f"{r['id']}: PASS at {r['layer']} without a negative control (VERIFICATION 2.4)")
        if r["status"] == "PASS" and r["mismatched"] not in ("-", "0"):
            errors.append(f"{r['id']}: PASS with mismatches")
    return errors


# ----------------------------------------------------------- record-tests

NEXTEST_LINE_RE = re.compile(
    r"^\s*(PASS|FAIL|SIGSEGV|SIGABRT|TIMEOUT|LEAK-FAIL)\s+\[[^\]]*\]\s+(?:\(\s*\d+/\d+\)\s+)?(\S+)\s+(\S+)\s*$")


def parse_nextest(logs: list[Path]) -> dict[tuple[str, str], bool]:
    """(binary id, test name) -> passed. Retries: the last outcome wins."""
    results: dict[tuple[str, str], bool] = {}
    for log in logs:
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            m = NEXTEST_LINE_RE.match(line)
            if m:
                results[(m.group(2), m.group(3))] = m.group(1) == "PASS"
    return results


def record_tests(rows: list[dict], previous: dict[str, dict[str, str]], logs: list[Path], commit: str,
                 tree: RustTree) -> int:
    results = parse_nextest(logs)
    if not results:
        raise SystemExit("no nextest PASS/FAIL lines found in the given logs")
    changed = 0
    for r in rows:
        if r["layer"] != "L1" or not r["consumers"]:
            continue
        passed = failed = 0
        for c in r["consumers"]:
            scope = tree.test_scope(c)
            if scope is None:
                continue
            binary, prefix = scope
            for (b, name), ok in results.items():
                if prefix:
                    # Unit tests: the crate's lib binary or one of its bin targets.
                    if b != binary and not b.startswith(binary + "::bin/"):
                        continue
                    if not name.startswith(prefix + "::"):
                        continue
                elif b != binary:
                    continue
                passed += ok
                failed += not ok
        status = "FAIL" if failed else "PASS" if passed else "UNVERIFIED"
        note = f"cargo tests in consumer files: {passed} passed, {failed} failed"
        if status == "UNVERIFIED":
            note = "no cargo test in the consumer files ran"
        previous[r["id"]] = {"status": status, "matched": "-", "mismatched": "-",
                             "last_verified_commit": commit if status != "UNVERIFIED" else "-",
                             "note": note}
        changed += 1
    return changed


# ---------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="exit 1 if VERIFY.tsv is stale or malformed")
    ap.add_argument("--output", type=Path, default=OUTPUT)
    sub = ap.add_subparsers(dest="cmd")
    rec = sub.add_parser("record", help="write one row's result columns")
    rec.add_argument("row_id")
    rec.add_argument("--status", required=True, choices=sorted(STATUSES))
    rec.add_argument("--matched", default="-")
    rec.add_argument("--mismatched", default="-")
    rec.add_argument("--commit", default="-")
    rec.add_argument("--note", default="-")
    rt = sub.add_parser("record-tests", help="results of L1 rows from cargo nextest logs")
    rt.add_argument("--commit", required=True)
    rt.add_argument("logs", nargs="+", type=Path)
    args = ap.parse_args()

    tree = RustTree(git_files())
    rows = build_rows(tree)
    previous = load_results(args.output)

    if args.cmd == "record":
        if args.row_id not in {r["id"] for r in rows}:
            raise SystemExit(f"unknown row id {args.row_id}")
        previous[args.row_id] = {"status": args.status, "matched": args.matched,
                                 "mismatched": args.mismatched, "last_verified_commit": args.commit,
                                 "note": args.note}
    elif args.cmd == "record-tests":
        n = record_tests(rows, previous, args.logs, args.commit, tree)
        print(f"recorded {n} L1 rows from {len(args.logs)} log(s)")

    text = render(rows, previous)
    errors = validate(text)
    if errors:
        for e in errors:
            print(f"VERIFY.tsv: {e}", file=sys.stderr)
        return 1
    if args.check:
        current = args.output.read_text(encoding="utf-8") if args.output.exists() else ""
        if current != text:
            print(f"{args.output.name} is stale: run python3 scripts/verify_ledger.py", file=sys.stderr)
            return 1
        print(f"{args.output.name} is current ({len(rows)} rows)")
        return 0
    args.output.write_text(text, encoding="utf-8")
    print(f"wrote {args.output.name} ({len(rows)} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

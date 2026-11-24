#!/usr/bin/env python3
"""Regression tests for scripts/verify_ledger.py and scripts/verify_debt.py.

Run with: python3 scripts/test_verify_ledger.py   (exit 0 = all pass)

Pure-function tests on crafted inputs plus a few assertions over the real
tree whose expected values were read off the named files by hand.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import verify_debt as vd  # noqa: E402
import verify_ledger as vl  # noqa: E402

HEADER = "\t".join(vl.COLUMNS)


def row(**kw) -> dict[str, str]:
    base = {"id": "x", "layer": "L1", "rust_globs": "-", "java_classes": "-", "mechanism": "m",
            "fixtures": "-", "corpus_n": "1", "negative": "n", "tier": "T0", "status": "UNVERIFIED",
            "matched": "-", "mismatched": "-", "last_verified_commit": "-", "note": "-"}
    base.update(kw)
    return base


def render(*rows: dict[str, str]) -> str:
    return HEADER + "\n" + "".join("\t".join(r[k] for k in vl.COLUMNS) + "\n" for r in rows)


class UseParsingTest(unittest.TestCase):
    def test_expand_use_groups_and_aliases(self):
        self.assertEqual(
            vl.expand_use("crate::a::{b, c::{d, e as f}, g::*}"),
            ["crate::a::b", "crate::a::c::d", "crate::a::c::e", "crate::a::g::*"],
        )
        self.assertEqual(vl.expand_use("super::Foo as Bar"), ["super::Foo"])

    def test_test_scope(self):
        """fluid/paper_fixture.rs is `mod paper_fixture;` (under #[cfg(test)])
        in fluid.rs; levelgen/noisegen/mod.rs is that module; levelgen/structure/
        placement_fixture.rs is `#[path = "placement_fixture.rs"] mod tests;`
        in placement.rs, so its tests are named placement::tests::*;
        mth_golden_tests.rs is `#[path] mod mth_golden_tests;` in mth.rs."""
        tree = vl.RustTree(vl.git_files())
        self.assertEqual(tree.test_scope("crates/rivet-world/src/fluid/paper_fixture.rs"),
                         ("rivet-world", "fluid::paper_fixture"))
        self.assertEqual(tree.test_scope("crates/rivet-world/src/levelgen/noisegen/mod.rs"),
                         ("rivet-world", "levelgen::noisegen"))
        self.assertEqual(tree.test_scope("crates/rivet-world/src/levelgen/structure/placement_fixture.rs"),
                         ("rivet-world", "levelgen::structure::placement::tests"))
        self.assertEqual(tree.test_scope("crates/rivet-util/src/mth_golden_tests.rs"),
                         ("rivet-util", "mth::mth_golden_tests"))
        self.assertEqual(tree.test_scope("crates/rivet-world/tests/biome_temperature_golden.rs"),
                         ("rivet-world::biome_temperature_golden", ""))
        self.assertIsNone(tree.test_scope("crates/rivet-world/src/lib.rs"))
        # A #[path] test module is test-only and credits the module declaring it.
        fixture = "crates/rivet-world/src/levelgen/structure/placement_fixture.rs"
        text = (vl.REPO / fixture).read_text()
        self.assertTrue(tree.is_test_only(fixture, text))
        primary, secondary = tree.globs_for(fixture, text)
        self.assertEqual(primary, {"crates/rivet-world/src/levelgen/structure/placement.rs"})
        self.assertTrue(all(p.startswith("crates/rivet-world/") for p in secondary))


class NextestTest(unittest.TestCase):
    def test_parse_lines_both_formats(self, tmp=Path(__file__).resolve().parent.parent / "target"):
        tmp.mkdir(exist_ok=True)
        log = tmp / "test_verify_ledger_nextest.log"
        log.write_text(
            "        PASS [   0.004s] rivet-world fluid::paper_fixture::tests::a\n"
            "        FAIL [   0.010s] (12/99) rivet-world fluid::paper_fixture::tests::b\n"
            "        PASS [   0.002s] rivet-world::biome_temperature_golden matches\n"
            "   Compiling rivet-world v0.1.0\n",
            encoding="utf-8",
        )
        try:
            got = vl.parse_nextest([log])
        finally:
            log.unlink()
        self.assertEqual(got, {
            ("rivet-world", "fluid::paper_fixture::tests::a"): True,
            ("rivet-world", "fluid::paper_fixture::tests::b"): False,
            ("rivet-world::biome_temperature_golden", "matches"): True,
        })


class ValidateTest(unittest.TestCase):
    def test_well_formed(self):
        self.assertEqual(vl.validate(render(row())), [])

    def test_pass_needs_commit_and_negative_above_l1(self):
        errs = vl.validate(render(row(status="PASS")))
        self.assertTrue(any("last_verified_commit" in e for e in errs))
        errs = vl.validate(render(row(id="p", layer="L2", status="PASS", last_verified_commit="abc")))
        self.assertTrue(any("negative control" in e for e in errs))
        self.assertEqual(vl.validate(render(
            row(id="p", layer="L2", negative="y", status="PASS", last_verified_commit="abc"))), [])

    def test_bad_values(self):
        errs = vl.validate(render(row(status="GREEN"), row(id="y", corpus_n="many")))
        self.assertTrue(any("bad status" in e for e in errs))
        self.assertTrue(any("corpus_n" in e for e in errs))
        self.assertTrue(vl.validate(render(row(), row())))  # duplicate id


class CorpusTest(unittest.TestCase):
    def test_json_records(self):
        self.assertEqual(vl.json_records([1, 2, 3]), 3)
        self.assertEqual(vl.json_records({"seed": 42, "chunks": {"0,0": {}, "0,1": {}}, "x": [1]}), 2)
        self.assertEqual(vl.json_records({"seed": 42}), 0)

    def test_real_fixture_counts(self):
        """flowing-fluid.jsonl has 58 lines; fluid-block-states.tsv and
        fluid-collision-faces.tsv have no header line, so the counter (which
        always drops one header line of a .tsv) under-counts each by one:
        a conservative bias recorded in the docstring."""
        tracked = vl.git_files()
        fixture = "tools/rivet-oracle/fixtures/flowing-fluid.jsonl"
        lines = (vl.REPO / fixture).read_text().splitlines()
        self.assertEqual(vl.count_corpus([fixture], tracked), str(sum(1 for ln in lines if ln.strip())))
        self.assertEqual(vl.count_corpus(["tools/rivet-capture/fixtures/join/capture.jsonl"], tracked), "205")


class TrackedListingTest(unittest.TestCase):
    def test_only_tracked_paths_are_listed(self):
        """Listings come from git ls-files, so an untracked probe script or a
        fixture a local probe run left behind never reaches the ledger."""
        tracked = ["o/fixtures/a.json", "o/fixtures/dir/x.tsv", "o/fixtures/dir/y.tsv",
                   "o/scripts/run_a_probe.sh", "o/scripts-other/z"]
        self.assertEqual(vl.tracked_entries("o/fixtures", tracked), ["a.json", "dir"])
        self.assertEqual(vl.tracked_entries("o/scripts", tracked), ["run_a_probe.sh"])
        self.assertTrue(vl.is_tracked("o/fixtures/dir", tracked))
        self.assertTrue(vl.is_tracked("o/fixtures/a.json", tracked))
        self.assertFalse(vl.is_tracked("o/fixtures/di", tracked))
        self.assertFalse(vl.is_tracked("o/fixtures/stray.json", tracked))


class DebtTest(unittest.TestCase):
    def test_glob_regex(self):
        rx = vd.glob_regex("crates/rivet-nbt/src/*_tag.rs")
        self.assertTrue(rx.match("crates/rivet-nbt/src/int_tag.rs"))
        self.assertFalse(rx.match("crates/rivet-nbt/src/sub/int_tag.rs"))
        self.assertTrue(vd.glob_regex("crates/rivet-text/src/**").match("crates/rivet-text/src/a/b.rs"))

    def test_classes_and_corpus_cap(self):
        files = {"crates/a/src/x.rs": 400, "crates/a/src/y.rs": 400}
        rows = [
            # L1 PASS with a 4-input corpus: credits at most 4 * 50 = 200 LOC.
            row(id="r1", rust_globs="crates/a/src/*.rs", corpus_n="4", status="PASS",
                last_verified_commit="c"),
            # L2 PASS with a negative: V2, allocated before V1.
            row(id="r2", layer="L2", negative="y", rust_globs="crates/a/src/y.rs", corpus_n="2",
                status="PASS", last_verified_commit="c"),
            # Not PASS: no credit.
            row(id="r3", rust_globs="crates/a/src/x.rs", corpus_n="100"),
        ]
        self.assertEqual(vd.row_class(rows[0]), "V1")
        self.assertEqual(vd.row_class(rows[1]), "V2")
        self.assertIsNone(vd.row_class(rows[2]))
        self.assertIsNone(vd.row_class(row(layer="L2", status="PASS")))
        v2, v1, _ = vd.allocate(rows, files)
        self.assertEqual(dict(v2), {"crates/a/src/y.rs": 100})
        self.assertEqual(dict(v1), {"crates/a/src/x.rs": 200})
        agg = vd.table(files, v2, v1)["TOTAL"]
        self.assertEqual((agg["loc"], agg["v2_loc"], agg["thin_loc"], agg["debt_loc"]), (800, 100, 200, 500))

    def test_subsystems(self):
        self.assertEqual(vd.subsystem_of("crates/rivet-world/src/levelgen/feature/tree_feature.rs"),
                         "worldgen-features")
        self.assertEqual(vd.subsystem_of("crates/rivet-world/src/levelgen/noise/x.rs"),
                         "worldgen-noise-biome-surface")
        self.assertEqual(vd.subsystem_of("crates/rivet-server/src/server/level/chunk_map.rs"),
                         "server-level-runtime")


class RealTreeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = {r["id"]: r for r in vl.build_rows()}

    def test_every_probe_script_has_a_row(self):
        scripts = sorted(p.name for p in (vl.REPO / vl.ORACLE / "scripts").glob("run_*_probe.sh"))
        for s in scripts:
            rid = "probe." + s.removeprefix("run_").removesuffix("_probe.sh").replace("_", "-")
            self.assertIn(rid, self.rows, s)

    def test_flowing_fluid_probe_row(self):
        """run_flowing_fluid_probe.sh writes flowing-fluid.jsonl plus the two fluid
        TSVs and (its first argument, FlowingFluidProbe.java args[0])
        crates/rivet-world/src/fluid/block_behaviors.tsv, per probes.tsv and
        `rivet-oracle regen --check`; crates/rivet-world/src/fluid/paper_fixture.rs
        reads the three oracle fixtures via include_str! and is a #[cfg(test)]
        module of fluid/mod.rs, so the row credits the production files it
        imports, not the fixture module itself; fluid/containers.rs
        include_str!s block_behaviors.tsv in production code.
        FlowingFluidProbe.java imports net.minecraft.world.level.material.* and
        names `FlowingFluid`, so the wildcard resolves it."""
        r = self.rows["probe.flowing-fluid"]
        self.assertEqual(r["consumers"], ["crates/rivet-world/src/fluid/containers.rs",
                                          "crates/rivet-world/src/fluid/paper_fixture.rs"])
        self.assertIn("crates/rivet-world/src/fluid/block_behaviors.tsv", r["fixtures"])
        self.assertNotIn("crates/rivet-world/src/fluid/paper_fixture.rs", r["rust_globs"])
        self.assertTrue(r["rust_globs"])
        self.assertIn("net.minecraft.world.level.material.FlowingFluid", r["java_classes"])

    def test_probe_ownership_comes_from_probes_tsv(self):
        """Every probe.* row claims exactly what tools/rivet-oracle/probes.tsv
        lists for its script (directories of the oracle fixtures as one entry),
        and every recorded oracle op gets a recorded and a live row with a
        negative."""
        owned = vl.probe_outputs(vl.probe_table())
        update = owned["run_update_shape_probe.sh"]
        self.assertEqual(update, ["block-update-shape-owners.tsv", "voxel-edge-faces.tsv"])
        self.assertEqual(owned["run_worldgen_sampler.sh"], ["worldgen"])
        self.assertEqual(owned["run_directory_lock_probe.sh"], [])
        self.assertNotIn("run.sh", owned)
        for op in ("snbt.parse", "nbt.decode", "component.json"):
            for kind in ("recorded", "live"):
                r = self.rows[f"op.{op}.{kind}"]
                self.assertEqual(r["negative"], "y")
                self.assertEqual(r["seed_status"], "UNVERIFIED")
        self.assertEqual(self.rows["op.snbt.parse.recorded"]["corpus_n"], "170")
        self.assertEqual(self.rows["regen.probes"]["tier"], "T3")

    def test_inactive_and_boot_rows_are_never_pass(self):
        self.assertEqual(self.rows["hash.diff"]["seed_status"], "INACTIVE")
        self.assertEqual(self.rows["generated-full"]["seed_status"], "INACTIVE")
        self.assertEqual(self.rows["junit.census"]["seed_status"], "INACTIVE")
        self.assertTrue(all(r["seed_status"] != "PASS" for r in self.rows.values()))


if __name__ == "__main__":
    unittest.main()

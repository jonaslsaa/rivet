#!/usr/bin/env python3
"""Regression tests for scripts/coverage_ledger.py and its lexers.

Run with: python3 scripts/test_coverage_ledger.py   (exit 0 = all pass)

Two layers:
  - synthetic snippets pin the Java method scanner and the Rust lexer;
  - real classes from working/Paper and crates/ pin the end-to-end heuristics.
    Each expected value below was derived by reading the named Java and Rust
    files by hand (the derivation is in the test's docstring), never by
    copying the script's output. If a port changes, re-derive the value from
    the files and update the docstring with it.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import coverage_ledger as cl  # noqa: E402
import java_scan  # noqa: E402
import rust_scan  # noqa: E402


class JavaScanTest(unittest.TestCase):
    def methods(self, src: str) -> list[tuple[str, str]]:
        return java_scan.scan_methods(java_scan.clean_java(src))

    def test_methods_constructors_anonymous_and_nested(self):
        src = '''
        package a.b;
        @SuppressWarnings({"unchecked", "rawtypes"})
        public final class Foo<T extends Comparable<T>> implements Bar {
            static final Runnable R = new Runnable() { public void run() { } };
            private int x = 1;
            static { init(); }
            public Foo(int x) { this.x = x; }
            @Override public <U> java.util.List<U> map(final java.util.function.Function<T, U> f) throws java.io.IOException { return null; }
            int[] arr() { String s = "}{"; char c = '}'; return new int[] {1, 2}; }
            abstract void hook();
            void lambda() { Runnable r = () -> { class Local { void inner() {} } }; }
            public static class Inner { void innerMethod() {} Inner() {} }
            enum E { A(1) { void constantBody() {} }, B(2); E(int v) {} int value() { return 0; } }
            interface I { void abstractOne(); default void defaulted() {} }
            record R(int a) { int doubled() { return a * 2; } }
        }
        '''
        self.assertEqual(
            self.methods(src),
            [("Foo", "map"), ("Foo", "arr"), ("Foo", "hook"), ("Foo", "lambda"),
             ("Inner", "innerMethod"), ("E", "value"), ("I", "abstractOne"),
             ("I", "defaulted"), ("R", "doubled")],
        )

    def test_fields_and_calls_are_not_methods(self):
        src = '''
        class F {
            public static final Codec<F> CODEC = RecordCodecBuilder.create(i -> i.group(x()).apply(i, F::new));
            Foo bar = Foo.of(1);
            int[] table = {1, 2, 3};
        }
        '''
        self.assertEqual(self.methods(src), [])

    def test_snake_case(self):
        self.assertEqual(java_scan.snake_case("getXRot"), "get_x_rot")
        self.assertEqual(java_scan.snake_case("toNBT"), "to_nbt")
        self.assertEqual(java_scan.snake_case("getUUID"), "get_uuid")
        self.assertEqual(java_scan.snake_case("readVarInt"), "read_var_int")


class RustScanTest(unittest.TestCase):
    def test_cfg_test_items_and_literals_are_removed(self):
        text = (
            'pub fn real() { let s = "fn fake() {"; }\n'
            "// fn commented() {}\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn test_only() {}\n"
            "}\n"
            "fn r#type() {}\n"
        )
        code = rust_scan.strip_test_code(rust_scan.blank_comments_and_strings(text))
        self.assertEqual(rust_scan.fn_names(code), ["real", "r#type"])
        self.assertEqual(len(code), len(text))
        self.assertEqual(rust_scan.production_line_count(text), 4)

    def test_module_map(self):
        src = {
            "crates/c/src/lib.rs": "pub mod a;\nmod b;\n// mod commented;\n",
            "crates/c/src/a.rs": (
                "#[cfg(test)]\n#[path = \"a_fixture.rs\"]\nmod tests;\n"
                "pub(crate) mod inner;\n"
                "mod inline { mod skipped; }\n"
                "#[cfg(test)]\nmod t { include!(\"spliced.rs\"); }\n"
            ),
            "crates/c/src/a_fixture.rs": "use super::*;\n",
            "crates/c/src/a/inner.rs": "",
            "crates/c/src/a/inline/skipped.rs": "",
            "crates/c/src/spliced.rs": "",
            "crates/c/src/b/mod.rs": "mod leaf;\n",
            "crates/c/src/b/leaf.rs": "",
            "crates/c/src/orphan.rs": "",
        }
        m = rust_scan.module_map(set(src), src.__getitem__)
        got = {p: (i.path, i.test_only, i.parent) for p, i in m.items()}
        self.assertEqual(got, {
            "crates/c/src/lib.rs": ((), False, None),
            "crates/c/src/a.rs": (("a",), False, "crates/c/src/lib.rs"),
            "crates/c/src/a_fixture.rs": (("a", "tests"), True, "crates/c/src/a.rs"),
            "crates/c/src/a/inner.rs": (("a", "inner"), False, "crates/c/src/a.rs"),
            "crates/c/src/spliced.rs": (("a",), True, "crates/c/src/a.rs"),
            "crates/c/src/b/mod.rs": (("b",), False, "crates/c/src/lib.rs"),
            "crates/c/src/b/leaf.rs": (("b", "leaf"), False, "crates/c/src/b/mod.rs"),
        })

    def test_test_file_rules(self):
        self.assertTrue(rust_scan.is_test_file(Path("a/tests.rs")))
        self.assertTrue(rust_scan.is_test_file(Path("a/foo_tests.rs")))
        self.assertTrue(rust_scan.is_test_file(Path("a/tests/foo.rs")))
        self.assertFalse(rust_scan.is_test_file(Path("a/testsuite.rs")))


class MirrorTest(unittest.TestCase):
    def rf(self, rel: str) -> cl.RustFile:
        return cl.RustFile(Path(rel))

    def test_exact_mount_and_suffix(self):
        nbt = self.rf("crates/rivet-nbt/src/compound_tag.rs")
        self.assertEqual(cl.mirror_score(nbt, "net.minecraft.nbt"), (1, 0))
        feat = self.rf("crates/rivet-world/src/levelgen/feature/tree_feature.rs")
        self.assertEqual(cl.mirror_score(feat, "net.minecraft.world.level.levelgen.feature"), (1, 2))
        be = self.rf("crates/rivet-world/src/block_entity/beehive_block_entity.rs")
        self.assertEqual(cl.mirror_score(be, "net.minecraft.world.level.block.entity"), (1, 2))
        # A crate-root file only mirrors packages its crate mounts.
        reg = self.rf("crates/rivet-registry/src/identifier_exception.rs")
        self.assertIsNone(cl.mirror_score(reg, "net.minecraft"))
        # A package's module file is not a mirror of a class in that package.
        pkg_mod = self.rf("crates/rivet-world/src/levelgen/feature.rs")
        self.assertIsNone(cl.mirror_score(pkg_mod, "net.minecraft.world.level.levelgen.feature"))

    def test_buckets(self):
        self.assertEqual(cl.bucket_for("minecraft", "net.minecraft.world.entity"), "A")
        self.assertEqual(cl.bucket_for("minecraft", "ca.spottedleaf.dataconverter.types"), "A")
        self.assertEqual(cl.bucket_for("minecraft", "net.minecraft.data.worldgen"), "B")
        self.assertEqual(cl.bucket_for("minecraft", "net.minecraft.util.datafix.fixes"), "C")
        self.assertEqual(cl.bucket_for("paper-server", "io.papermc.paper.configuration"), "D")
        self.assertEqual(cl.bucket_for("paper-server", "org.bukkit.craftbukkit.entity"), "E")
        self.assertEqual(cl.bucket_for("paper-api", "org.bukkit"), "F")


class RealClassTest(unittest.TestCase):
    rows: dict[str, dict[str, str]] = {}
    row_list: list[dict[str, str]] = []

    @classmethod
    def setUpClass(cls):
        cls.row_list = cl.build_rows()
        cls.rows = {r["java_class"]: r for r in cls.row_list}

    def test_var_int_complete(self):
        """net/minecraft/network/VarInt.java declares six methods (getByteSize,
        getByteSizeSlow, hasContinuationBit, read, write, writeSlow; the
        `static {}` block is not a method). crates/rivet-protocol/src/var_int.rs
        has a free fn for each (get_byte_size ... write_slow) and no markers;
        its #[cfg(test)] helpers are ignored. Mirrored at the rivet-protocol
        root (mount net.minecraft.network)."""
        r = self.rows["net.minecraft.network.VarInt"]
        self.assertEqual(r["rust_path"], "crates/rivet-protocol/src/var_int.rs")
        self.assertEqual((r["method_total"], r["method_covered"]), ("6", "6"))
        self.assertEqual((r["stub_count"], r["rivettodo_count"], r["status"]), ("0", "0", "complete"))

    def test_end_tag_partial(self):
        """net/minecraft/nbt/EndTag.java: the named class declares write,
        sizeInBytes, getId, getType, toString, copy and accept (overloaded
        twice) = 7 distinct names; the anonymous TagType body (load, parse,
        skip, getName, getPrettyName) is excluded. crates/rivet-nbt/src/end_tag.rs
        defines size_in_bytes, copy_tag and as_tag: only sizeInBytes matches
        (`copy` is not overloaded, so `copy_tag` is no overload suffix; the
        trait methods live on the `Tag` enum in tag.rs, a documented blind
        spot). 1 of 7 -> partial."""
        r = self.rows["net.minecraft.nbt.EndTag"]
        self.assertEqual(r["rust_path"], "crates/rivet-nbt/src/end_tag.rs")
        self.assertEqual((r["method_total"], r["method_covered"], r["status"]), ("7", "1", "partial"))

    def test_float_goal_assigned_to_server(self):
        """net/minecraft/world/entity/ai/goal/FloatGoal.java declares canUse,
        requiresUpdateEveryTick and tick (plus a constructor, not counted);
        crates/rivet-entity/src/ai/goal/float_goal.rs implements can_use,
        requires_update_every_tick and tick. It is rivet-entity content, so
        D15 step 1 assigns it to rivet-server while the home stays where it is."""
        r = self.rows["net.minecraft.world.entity.ai.goal.FloatGoal"]
        self.assertEqual(r["current_crate"], "rivet-entity")
        self.assertEqual(r["assigned_crate"], "rivet-server")
        self.assertEqual(r["rust_path"], "crates/rivet-entity/src/ai/goal/float_goal.rs")
        self.assertEqual((r["method_total"], r["method_covered"], r["status"]), ("3", "3", "complete"))

    def test_entity_absent_spine_hub(self):
        """Entity.java (5,462 LOC) has no mirrored home: rivet-world/src/entity/mod.rs
        is the world.entity package module and rivet-api's facades/entity.rs is
        not at a mirrored path, so status is absent; it is an Appendix A spine hub."""
        r = self.rows["net.minecraft.world.entity.Entity"]
        self.assertEqual((r["rust_path"], r["status"], r["hub"]), ("-", "absent", "spine"))
        self.assertEqual(r["method_covered"], "0")
        self.assertIn("crates/rivet-api/src/native/facades/entity.rs", r["other_paths"].split(";"))
        self.assertNotIn("crates/rivet-world/src/entity/mod.rs", r["other_paths"].split(";"))

    def test_d15_block_behaviour_moves_chunk_stays(self):
        """world.level.block (block behaviour) references world.entity/server
        types in most classes, so it moves to rivet-server; world.level.chunk
        does not (a minority of classes), so it stays in rivet-world."""
        self.assertEqual(self.rows["net.minecraft.world.level.block.Block"]["assigned_crate"], "rivet-server")
        self.assertEqual(self.rows["net.minecraft.world.level.chunk.LevelChunk"]["assigned_crate"], "rivet-world")

    def test_non_target_buckets(self):
        craft = self.rows["org.bukkit.craftbukkit.CraftServer"]
        self.assertEqual((craft["bucket"], craft["hub"], craft["status"]), ("E", "adapter", "adapter"))
        self.assertEqual(craft["rust_path"], "-")
        api = self.rows["org.bukkit.Bukkit"]
        self.assertEqual((api["bucket"], api["status"]), ("F", "adapter"))
        dfu = self.rows["net.minecraft.util.datafix.DataFixers"]
        self.assertEqual((dfu["bucket"], dfu["hub"], dfu["status"]), ("C", "dfu-audit", "deferred(dfu-audit)"))
        # A datagen class on the spine hub list is a translation target.
        pu = self.rows["net.minecraft.data.worldgen.placement.PlacementUtils"]
        self.assertEqual((pu["bucket"], pu["hub"]), ("B", "spine"))
        self.assertIn(pu["status"], ("absent", "partial", "complete"))
        other = self.rows["net.minecraft.data.BlockFamilies"]
        self.assertEqual(other["status"], "n/a(datagen)")

    def test_rows_are_sorted_and_unique(self):
        paths = [r["java_path"] for r in self.row_list]
        self.assertEqual(paths, sorted(paths))
        self.assertEqual(len(paths), len(set(paths)))
        self.assertEqual(len(self.rows), len(self.row_list))  # java_class unique too
        self.assertTrue(all(set(r) == set(cl.COLUMNS) for r in self.row_list))


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Sandbox regression tests for scripts/check_markers.py.

Run with: python3 scripts/test_check_markers.py   (exit 0 = all pass)

Deterministic and fully offline: exercises the checker's pure functions
(`check_file`, `load_manifest`, `is_excluded`) against crafted temporary
trees, plus a `main()`-level determinism run over a temp tree with the real
`git ls-files` scan stubbed to a fixed file list. It never queries GitHub.

Covered rules:
  1. a file with well-formed STUB/RivetTodo markers is clean;
  2. malformed marker shapes are flagged (bare `STUB —`, bare `STUB:`, a line
     that *begins* with the token in the wrong shape);
  3. a STUB whose unit id is absent from MANIFEST.tsv is flagged;
  4. a STUB whose unit is `done` is flagged as stale;
  5. two marker bodies on one line is flagged as ambiguous — cross-form
     (STUB+RivetTodo) and same-form (two STUBs / two RivetTodos) — while a
     mid-sentence mention of the other form stays prose (not flagged);
  6. a STUB / RivetTodo with an empty (whitespace-only) reason is flagged;
  7. every bare `todo!()` / `unimplemented!()` must carry an adjacent
     RivetTodo (same line or immediately-preceding comment);
  8. a mid-sentence mention of "STUB"/"RivetTodo" is prose, not a marker, and
     is NOT flagged;
  9. `load_manifest` reads the done-set from column 12;
  10. the exclusion rules hide `fuzz/**`, `**/tests/**`, `*_test.rs`,
      `**/src/generated/**`, `tools/rivet-codegen/**`, and `spikes/**`;
  11. `main()` over a fixed file list is deterministic and counts markers.
"""

import contextlib
import importlib.util
import io
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

SPEC = importlib.util.spec_from_file_location("check_markers", HERE / "check_markers.py")
check_markers = importlib.util.module_from_spec(SPEC)
assert SPEC and SPEC.loader
SPEC.loader.exec_module(check_markers)

PASS = 0
FAIL = 0


def check(name: str, cond: bool) -> None:
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  ok: {name}")
    else:
        FAIL += 1
        print(f"FAIL: {name}")


def messages(file_text: str, units: set[str], done: set[str]) -> list[str]:
    """Run check_file over an in-memory file and return just the messages."""
    with tempfile.TemporaryDirectory() as td:
        path = Path(td) / "probe.rs"
        path.write_text(file_text, encoding="utf-8")
        violations, _ns, _nt = check_markers.check_file(path, units, done)
        return [msg for _p, _ln, msg in violations]


def main() -> int:
    # Representative manifest sets: the checker only needs these to exist, so
    # the sandbox provides a small closed set (mirrors real MANIFEST units).
    UNITS = {"mc.nbt.io", "mc.nbt", "mc.network.protocol.common", "mc.util"}
    DONE = {"mc.nbt", "mc.nbt.io"}

    print("== 1. clean file ==")
    clean = """\
//! Module header.
//!
//! STUB(mc.network.protocol.common) not ported here.
//! RivetTodo(#126): the registry-wired codecs are deferred.
fn main() {}
"""
    check("well-formed markers yield no violations", messages(clean, UNITS, DONE) == [])

    print("== 2. malformed marker shapes ==")
    check(
        "bare `STUB —` is flagged",
        messages("// STUB — `Foo` not ported.\nfn main() {}\n", UNITS, DONE)
        == ["STUB must be `STUB(<manifest-unit-id>) reason`"],
    )
    check(
        "bare `STUB:` is flagged",
        messages("// STUB: nothing here.\nfn main() {}\n", UNITS, DONE)
        == ["STUB must be `STUB(<manifest-unit-id>) reason`"],
    )
    check(
        "RivetTodo in wrong shape is flagged",
        messages("// RivetTodo: reason text\nfn main() {}\n", UNITS, DONE)
        == ["RivetTodo must be `RivetTodo(#N): reason`"],
    )
    check(
        "RivetTodo with non-positive issue number is flagged",
        messages("// RivetTodo(#0): reason\nfn main() {}\n", UNITS, DONE)
        == ["RivetTodo must be `RivetTodo(#N): reason`"],
    )
    check(
        "RivetTodo missing reason is flagged",
        messages("// RivetTodo(#201)\nfn main() {}\n", UNITS, DONE)
        == ["RivetTodo must be `RivetTodo(#N): reason`"],
    )

    print("== 3. unknown STUB unit ==")
    check(
        "STUB unit not in MANIFEST is flagged",
        messages("// STUB(no.such.unit) absent.\nfn main() {}\n", UNITS, DONE)
        == ["STUB unit `no.such.unit` not in MANIFEST.tsv"],
    )

    print("== 4. stale STUB on a done unit ==")
    check(
        "STUB on a done unit is stale",
        messages("// STUB(mc.nbt) not ported.\nfn main() {}\n", UNITS, DONE)
        == ["stale STUB: unit `mc.nbt` is `done`"],
    )

    print("== 5. two marker bodies on one line ==")
    check(
        "STUB and RivetTodo on one line is ambiguous",
        messages(
            "// STUB(mc.util) X. RivetTodo(#1): Y.\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == ["STUB and RivetTodo on one line"],
    )
    check(
        "RivetTodo and STUB on one line is ambiguous",
        messages(
            "// RivetTodo(#1): Y. STUB(mc.util) X.\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == ["RivetTodo and STUB on one line"],
    )
    check(
        "two STUBs on one line is ambiguous",
        messages(
            "// STUB(mc.util) X. STUB(mc.nbt) Y.\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == ["multiple STUB markers on one line"],
    )
    check(
        "two RivetTodos on one line is ambiguous",
        messages(
            "// RivetTodo(#1): X. RivetTodo(#2): Y.\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == ["multiple RivetTodo markers on one line"],
    )
    check(
        "a mid-sentence RivetTodo mention in a STUB reason is prose",
        messages(
            "// STUB(mc.util) tracked as RivetTodo(#12).\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == [],
    )
    check(
        "a mid-sentence STUB mention in a RivetTodo reason is prose",
        messages(
            "// RivetTodo(#12): replaced old STUB(abc) note.\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == [],
    )

    print("== 6. empty reasons ==")
    check(
        "STUB with no reason is flagged",
        messages("// STUB(mc.util)   \nfn main() {}\n", UNITS, DONE)
        == ["STUB with an empty reason"],
    )
    check(
        "RivetTodo with whitespace-only reason is flagged",
        messages("// RivetTodo(#5):   \nfn main() {}\n", UNITS, DONE)
        == ["RivetTodo with an empty reason"],
    )

    print("== 7. bare todo!/unimplemented! needs an adjacent RivetTodo ==")
    check(
        "bare todo!() is flagged",
        messages("fn main() { todo!() }\n", UNITS, DONE)
        == ["todo!()/unimplemented!() requires an adjacent RivetTodo"],
    )
    check(
        "bare unimplemented!() is flagged",
        messages("fn main() { unimplemented!() }\n", UNITS, DONE)
        == ["todo!()/unimplemented!() requires an adjacent RivetTodo"],
    )
    check(
        "todo!() with RivetTodo on the same line is accepted",
        messages("fn main() { todo!() } // RivetTodo(#201): not ported\n", UNITS, DONE)
        == [],
    )
    check(
        "todo!() with RivetTodo on the preceding comment is accepted",
        messages(
            "// RivetTodo(#201): not ported.\nfn main() { todo!() }\n",
            UNITS,
            DONE,
        )
        == [],
    )
    check(
        "todo!() with a non-RivetTodo preceding comment is flagged",
        messages("// plain comment\nfn main() { todo!() }\n", UNITS, DONE)
        == ["todo!()/unimplemented!() requires an adjacent RivetTodo"],
    )

    print("== 8. mid-sentence mentions are prose, not markers ==")
    check(
        "prose mention of STUB(mc.nbt) is not flagged",
        messages(
            "//! The STUB(mc.nbt) shape keeps a pub value field.\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == [],
    )
    check(
        "prose mention of RivetTodo mid-sentence is not flagged",
        messages(
            "//! Deferred (see the RivetTodo(#201) note).\nfn main() {}\n",
            UNITS,
            DONE,
        )
        == [],
    )
    check(
        "marker inside a string literal is not a comment",
        messages(
            'const MSG: &str = "STUB(mc.nbt) in a string";\nfn main() {}\n',
            UNITS,
            DONE,
        )
        == [],
    )

    print("== 9. load_manifest reads the done-set from column 12 ==")
    with tempfile.TemporaryDirectory() as td:
        tmp_manifest = Path(td) / "MANIFEST.tsv"
        tmp_manifest.write_text(
            "id\tdeps\t\t\t\t\t\t\t\t\t\tstatus\n"
            "mc.nbt\t\t\t\t\t\t\t\t\t\t\tdone\n"
            "mc.util\t\t\t\t\t\t\t\t\t\t\tpending\n"
            "mc.network.buf\t\t\t\t\t\t\t\t\t\t\tdone\n",
            encoding="utf-8",
        )
        orig = check_markers.MANIFEST
        check_markers.MANIFEST = tmp_manifest
        try:
            units, done = check_markers.load_manifest()
            check("all unit ids parsed", units == {"mc.nbt", "mc.util", "mc.network.buf"})
            check("done set from column 12", done == {"mc.nbt", "mc.network.buf"})
        finally:
            check_markers.MANIFEST = orig

    print("== 10. exclusion rules ==")
    ex = check_markers.is_excluded
    check("fuzz/** excluded", ex(Path("fuzz/frame_fuzz.rs")))
    check("*_test.rs excluded", ex(Path("crates/rivet-util/src/lib_test.rs")))
    check("tests/ dir excluded", ex(Path("crates/rivet-protocol/tests/roundtrip.rs")))
    check("src/generated/** excluded", ex(Path("crates/rivet-protocol/src/generated/protocol.rs")))
    check("src/generated subpath excluded", ex(Path("crates/rivet-protocol/src/generated/blocks/tables.rs")))
    check("tools/rivet-codegen golden excluded", ex(Path("tools/rivet-codegen/data/mth_golden_skeleton.rs")))
    check("tools/rivet-codegen src excluded", ex(Path("tools/rivet-codegen/src/generate.rs")))
    check("spikes/** excluded", ex(Path("spikes/ffi-latency/src/lib.rs")))
    check("regular source NOT excluded", not ex(Path("crates/rivet-util/src/lib.rs")))
    check("src/generated/ (bare, no file) excluded", ex(Path("crates/rivet-protocol/src/generated/")))
    check("a generated dir that is not src/generated is kept",
          not ex(Path("crates/rivet-protocol/src/not_generated/protocol.rs")))
    check("workspace tool code kept", not ex(Path("tools/rivet-decode/src/main.rs")))

    print("== 11. main() determinism over a temp tree ==")
    probe = """\
//! STUB(mc.nbt) stale here.
// STUB(bad.unit) unknown.
// STUB(mc.nbt.io) — bare dash.
// RivetTodo(0): bad issue.
fn main() { todo!() }
"""
    expected_messages = [
        "RivetTodo must be `RivetTodo(#N): reason`",
        "STUB unit `bad.unit` not in MANIFEST.tsv",
        "stale STUB: unit `mc.nbt.io` is `done`",
        "stale STUB: unit `mc.nbt` is `done`",
        "todo!()/unimplemented!() requires an adjacent RivetTodo",
    ]
    with tempfile.TemporaryDirectory() as td:
        path = Path(td) / "probe.rs"
        path.write_text(probe, encoding="utf-8")
        r1, _s1, _t1 = check_markers.check_file(path, UNITS, DONE)
        r2, _s2, _t2 = check_markers.check_file(path, UNITS, DONE)
        check("identical violations across two runs", r1 == r2)
        msgs = sorted(msg for _p, _ln, msg in r1)
        check("deterministic run reports all five violations", msgs == expected_messages)

        # Exercise the real main() path — the git scan is stubbed to the temp
        # tree so the run stays offline and deterministic.
        clean_probe = "// STUB(mc.util) pending stub.\n// RivetTodo(#126): deferred.\nfn main() {}\n"
        (Path(td) / "a.rs").write_text(clean_probe, encoding="utf-8")
        (Path(td) / "MANIFEST.tsv").write_text(
            "id\tdeps\t\t\t\t\t\t\t\t\t\tstatus\n"
            "mc.util\t\t\t\t\t\t\t\t\t\t\tpending\n",
            encoding="utf-8",
        )
        orig_repo = check_markers.REPO
        orig_manifest = check_markers.MANIFEST
        orig_tracked = check_markers.tracked_rs_files
        check_markers.REPO = Path(td)
        check_markers.MANIFEST = Path(td) / "MANIFEST.tsv"
        check_markers.tracked_rs_files = lambda: [Path("a.rs")]
        try:

            def run_main_captured():
                buf = io.StringIO()
                with contextlib.redirect_stdout(buf):
                    code = check_markers.main()
                return buf.getvalue(), code

            o1, c1 = run_main_captured()
            o2, c2 = run_main_captured()
            check("main() output identical across two runs", o1 == o2)
            check("main() exit identical across two runs", c1 == c2)
            check("main() counts 1 STUB and 1 RivetTodo", "1 STUB, 1 RivetTodo" in o1)
            check("main() clean run exits 0", c1 == 0)
        finally:
            check_markers.REPO = orig_repo
            check_markers.MANIFEST = orig_manifest
            check_markers.tracked_rs_files = orig_tracked

    print(f"\n{PASS} passed, {FAIL} failed")
    return 1 if FAIL else 0


if __name__ == "__main__":
    sys.exit(main())

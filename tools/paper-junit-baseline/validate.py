#!/usr/bin/env python3
"""Fail-closed validator for the Paper JUnit baseline census artifact (issue #517).

Validates schema, content digest, pinned Paper/Gradle provenance, task outcomes,
JUnit suite/filter/tag configuration, disabled module exclusion, nonzero
selected/executed counts, zero failures/errors, and exact rollups. When
`--paper-source` is passed (and present), also verifies the live Paper checkout's
config hashes, disabled modules, and (if `--check-results` is set) the on-disk
JUnit XML artifacts match the census digests.

Exit codes:
  0  VERIFIED
  1  FAILED (tampering, drift, missing required fields, non-green control)
  3  UNVERIFIED (missing census file or missing --paper-source checkout)
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from census import (
    DISABLED_MODULES,
    EXPECTED_TASKS,
    EXIT_FAIL,
    EXIT_OK,
    EXIT_UNVERIFIED,
    MODULE_SPECS,
    PINNED_GRADLE_VERSION,
    PINNED_PAPER_COMMIT,
    SCHEMA_VERSION,
    CensusError,
    compute_content_sha256,
    inventory_module,
    parse_junit_results,
    verify_paper_config,
)


def _require(cond: bool, msg: str) -> None:
    if not cond:
        raise CensusError(msg, EXIT_FAIL)


def validate_census_doc(doc: dict[str, Any]) -> None:
    _require(isinstance(doc, dict), "census artifact must be a JSON object")
    _require(
        doc.get("schema_version") == SCHEMA_VERSION,
        f"unexpected schema_version: {doc.get('schema_version')} != {SCHEMA_VERSION}",
    )

    stored_sha = doc.get("content_sha256")
    _require(isinstance(stored_sha, str) and len(stored_sha) == 64, "missing/invalid content_sha256")
    expected_sha = compute_content_sha256(doc)
    _require(
        stored_sha == expected_sha,
        f"content_sha256 mismatch: stored {stored_sha} != computed {expected_sha}",
    )

    prov = doc.get("provenance")
    _require(isinstance(prov, dict), "missing provenance object")
    _require(
        prov.get("paper_commit") == PINNED_PAPER_COMMIT,
        f"paper_commit mismatch: {prov.get('paper_commit')} != {PINNED_PAPER_COMMIT}",
    )
    _require(
        prov.get("gradle_version") == PINNED_GRADLE_VERSION,
        f"gradle_version mismatch: {prov.get('gradle_version')} != {PINNED_GRADLE_VERSION}",
    )
    java_ver = prov.get("java_version", "")
    _require(
        isinstance(java_ver, str) and ('"25.' in java_ver or " 25." in java_ver),
        f"java_version must be JDK 25, got: {java_ver!r}",
    )
    _require(
        prov.get("tasks") == list(EXPECTED_TASKS),
        f"tasks mismatch: {prov.get('tasks')} != {list(EXPECTED_TASKS)}",
    )
    cfg_hashes = prov.get("config_sha256")
    _require(isinstance(cfg_hashes, dict), "missing provenance.config_sha256")
    required_cfg_keys = {
        "settings.gradle.kts",
        "gradle/wrapper/gradle-wrapper.properties",
        *(spec["build_gradle"] for spec in MODULE_SPECS.values()),
    }
    _require(
        set(cfg_hashes.keys()) == required_cfg_keys,
        f"config_sha256 keys mismatch: {sorted(cfg_hashes.keys())} != {sorted(required_cfg_keys)}",
    )
    for k, v in cfg_hashes.items():
        _require(isinstance(v, str) and len(v) == 64, f"invalid sha256 for {k}")

    disabled = doc.get("disabled_modules")
    _require(isinstance(disabled, list), "missing disabled_modules list")
    disabled_by_name = {d.get("module"): d for d in disabled if isinstance(d, dict)}
    _require(
        set(disabled_by_name.keys()) == set(DISABLED_MODULES)
        and len(disabled) == len(DISABLED_MODULES),
        f"disabled_modules must be exactly {list(DISABLED_MODULES)}",
    )
    for mod in DISABLED_MODULES:
        entry = disabled_by_name[mod]
        _require(entry.get("status") == "disabled", f"{mod}: status must be 'disabled'")
        _require(entry.get("evidence") is False, f"{mod}: evidence must be false")
        _require(
            entry.get("counted_source_files") == 0 and entry.get("counted_test_files") == 0,
            f"{mod}: disabled module source/test counts must be 0",
        )
        _require(
            entry.get("classification") == "disabled_non_evidence",
            f"{mod}: classification must be 'disabled_non_evidence'",
        )

    modules = doc.get("modules")
    _require(isinstance(modules, dict), "missing modules object")
    _require(
        set(modules.keys()) == set(MODULE_SPECS.keys()),
        f"modules must be exactly {sorted(MODULE_SPECS.keys())}, got {sorted(modules.keys())}",
    )

    rollup = {
        "active_modules": 0,
        "suite_count": 0,
        "selected": 0,
        "executed": 0,
        "passed": 0,
        "failed": 0,
        "failures": 0,
        "errors": 0,
        "skipped": 0,
    }

    for mod, spec in MODULE_SPECS.items():
        m = modules[mod]
        _require(isinstance(m, dict), f"{mod}: module entry must be object")
        _require(m.get("task") == spec["task"], f"{mod}: unexpected task {m.get('task')}")
        _require(
            m.get("task_outcome") == "EXECUTED",
            f"{mod}: task_outcome must be EXECUTED, got {m.get('task_outcome')}",
        )

        jcfg = m.get("junit_config")
        _require(isinstance(jcfg, dict), f"{mod}: missing junit_config")
        _require(jcfg.get("uses_junit_platform") is True, f"{mod}: uses_junit_platform must be true")
        _require(
            jcfg.get("suite_filters") == spec["suite_filters"],
            f"{mod}: suite_filters {jcfg.get('suite_filters')} != {spec['suite_filters']}",
        )
        _require(
            jcfg.get("excluded_tags") == spec["excluded_tags"],
            f"{mod}: excluded_tags {jcfg.get('excluded_tags')} != {spec['excluded_tags']}",
        )

        ex = m.get("execution")
        _require(isinstance(ex, dict), f"{mod}: missing execution object")
        suites = ex.get("suites")
        _require(isinstance(suites, list) and len(suites) > 0, f"{mod}: empty suites list")
        _require(
            ex.get("suite_count") == len(suites),
            f"{mod}: suite_count mismatch ({ex.get('suite_count')} != {len(suites)})",
        )

        s_sel = s_exe = s_pass = s_fail = s_err = s_skip = 0
        seen_files: set[str] = set()
        for s in suites:
            _require(isinstance(s, dict), f"{mod}: suite entry must be object")
            fname = s.get("file", "")
            _require(fname not in seen_files, f"{mod}: duplicate suite file {fname}")
            seen_files.add(fname)
            _require(
                isinstance(s.get("sha256"), str) and len(s["sha256"]) == 64,
                f"{mod}: invalid suite sha256 for {fname}",
            )
            sel = s["selected"]
            exe = s["executed"]
            pas = s["passed"]
            fai = s["failures"]
            err = s["errors"]
            ski = s["skipped"]
            _require(
                all(isinstance(x, int) and x >= 0 for x in (sel, exe, pas, fai, err, ski)),
                f"{mod}: invalid counts in suite {fname}",
            )
            _require(exe == sel - ski, f"{mod}: {fname} executed != selected - skipped")
            _require(pas == exe - fai - err, f"{mod}: {fname} passed != executed - failures - errors")
            s_sel += sel
            s_exe += exe
            s_pass += pas
            s_fail += fai
            s_err += err
            s_skip += ski

        _require(ex.get("selected") == s_sel and s_sel > 0, f"{mod}: selected rollup mismatch or zero")
        _require(ex.get("executed") == s_exe and s_exe > 0, f"{mod}: executed rollup mismatch or zero")
        _require(ex.get("passed") == s_pass, f"{mod}: passed rollup mismatch")
        _require(ex.get("failures") == s_fail and s_fail == 0, f"{mod}: failures must be 0")
        _require(ex.get("errors") == s_err and s_err == 0, f"{mod}: errors must be 0")
        _require(ex.get("failed") == s_fail + s_err == 0, f"{mod}: failed must be 0")
        _require(ex.get("skipped") == s_skip, f"{mod}: skipped rollup mismatch")

        src_inv = m.get("source_inventory")
        assets = m.get("non_test_assets")
        _require(isinstance(src_inv, dict), f"{mod}: missing source_inventory")
        _require(isinstance(assets, dict), f"{mod}: missing non_test_assets")
        _require(
            assets.get("classification") == "inputs_and_harness_assets_never_tests",
            f"{mod}: non_test_assets classification mismatch",
        )
        _require(
            src_inv.get("test_java_files")
            == src_inv.get("test_like_java_files", -1) + assets.get("support_java_files", -1),
            f"{mod}: test_java_files must equal test_like_java_files + support_java_files",
        )

        rollup["active_modules"] += 1
        rollup["suite_count"] += len(suites)
        rollup["selected"] += s_sel
        rollup["executed"] += s_exe
        rollup["passed"] += s_pass
        rollup["failed"] += s_fail + s_err
        rollup["failures"] += s_fail
        rollup["errors"] += s_err
        rollup["skipped"] += s_skip

    _require(
        doc.get("totals") == rollup,
        f"totals mismatch: stored {doc.get('totals')} != computed {rollup}",
    )


def validate_against_paper(
    doc: dict[str, Any], paper_dir: Path, check_results: bool = False, check_git: bool = True
) -> None:
    cfg = verify_paper_config(paper_dir, check_git=check_git)
    _require(
        doc["provenance"]["paper_commit"] == cfg["paper_commit"],
        "census paper_commit != live Paper checkout commit",
    )
    _require(
        doc["provenance"]["config_sha256"] == cfg["config_sha256"],
        "census config_sha256 != live Paper checkout build/settings files",
    )
    for mod, spec in MODULE_SPECS.items():
        inv = inventory_module(paper_dir, mod)
        _require(
            doc["modules"][mod]["source_inventory"] == inv["source_inventory"],
            f"{mod}: source_inventory drift vs live Paper checkout",
        )
        _require(
            doc["modules"][mod]["non_test_assets"] == inv["non_test_assets"],
            f"{mod}: non_test_assets drift vs live Paper checkout",
        )
        if check_results:
            parsed = parse_junit_results(paper_dir / spec["results_dir"], mod)
            stored_suites = [
                (s["name"], s["file"], s["selected"], s["executed"], s["passed"], s["skipped"])
                for s in doc["modules"][mod]["execution"]["suites"]
            ]
            live_suites = [
                (s["name"], s["file"], s["selected"], s["executed"], s["passed"], s["skipped"])
                for s in parsed["suites"]
            ]
            _require(
                stored_suites == live_suites,
                f"{mod}: live JUnit suite results drift from census",
            )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "census_path",
        type=Path,
        nargs="?",
        default=Path("tools/paper-junit-baseline/fixtures/baseline-census.json"),
        help="Path to baseline-census.json",
    )
    parser.add_argument(
        "--paper-source",
        type=Path,
        default=None,
        help="Optional Paper checkout path to verify provenance/config/inventory against",
    )
    parser.add_argument(
        "--check-results",
        action="store_true",
        help="When --paper-source is set, also verify live JUnit XML results match",
    )
    args = parser.parse_args(argv)

    if not args.census_path.is_file():
        print(f"UNVERIFIED: census artifact not found at {args.census_path}", file=sys.stderr)
        return EXIT_UNVERIFIED

    try:
        doc = json.loads(args.census_path.read_text(encoding="utf-8"))
        validate_census_doc(doc)
        if args.paper_source is not None:
            validate_against_paper(doc, args.paper_source, check_results=args.check_results)
    except json.JSONDecodeError as exc:
        print(f"FAILED: invalid JSON in {args.census_path}: {exc}", file=sys.stderr)
        return EXIT_FAIL
    except CensusError as exc:
        label = "UNVERIFIED" if exc.exit_code == EXIT_UNVERIFIED else "FAILED"
        print(f"{label}: {exc}", file=sys.stderr)
        return exc.exit_code

    t = doc["totals"]
    print(
        f"VERIFIED: {args.census_path} "
        f"(commit={doc['provenance']['paper_commit'][:10]} "
        f"suites={t['suite_count']} selected={t['selected']} "
        f"executed={t['executed']} passed={t['passed']} "
        f"failed={t['failed']} skipped={t['skipped']})"
    )
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())

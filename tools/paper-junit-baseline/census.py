#!/usr/bin/env python3
"""Filter-aware executed Paper JUnit baseline census producer (issue #517).

Runs `:paper-api:test` and `:paper-server:test` unchanged on the pinned Paper
checkout, verifies Gradle/JUnit configuration and disabled modules, parses
machine-readable JUnit XML results, classifies source/test/harness/input files
separately from executed cases, and emits a versioned census JSON artifact.

Exit codes:
  0  VERIFIED / census emitted
  1  FAILED (contract violation, test failure, filter mutation, missing artifact)
  3  UNVERIFIED (missing Paper checkout or JDK 25 prerequisite)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
PINNED_PAPER_COMMIT = "0a993450f129c4942c2a9ed45ba047412b4667cf"
PINNED_GRADLE_VERSION = "9.4.1"
EXPECTED_TASKS = (":paper-api:test", ":paper-server:test")
DISABLED_MODULES = ("paper-generator", "test-plugin")

MODULE_SPECS: dict[str, dict[str, Any]] = {
    "paper-api": {
        "task": ":paper-api:test",
        "build_gradle": "paper-api/build.gradle.kts",
        "results_dir": "paper-api/build/test-results/test",
        "suite_filters": [],
        "excluded_tags": [],
        "expected_suite_classes": [],
        "source_roots": ["paper-api/src/main/java"],
        "generated_roots": [],
        "patch_roots": [],
    },
    "paper-server": {
        "task": ":paper-server:test",
        "build_gradle": "paper-server/build.gradle.kts",
        "results_dir": "paper-server/build/test-results/test",
        "suite_filters": ["**/**TestSuite.class"],
        "excluded_tags": ["Slow"],
        "expected_suite_classes": [
            "AllFeaturesTestSuite",
            "LegacyTestSuite",
            "NormalTestSuite",
            "SlowTestSuite",
            "VanillaFeatureTestSuite",
        ],
        "source_roots": [
            "paper-server/src/main/java",
            "paper-server/src/minecraft/java",
        ],
        "generated_roots": ["paper-server/src/generated/java"],
        "patch_roots": ["paper-server/patches"],
    },
}

EXIT_OK = 0
EXIT_FAIL = 1
EXIT_UNVERIFIED = 3

JUNIT_ANNOTATION_RE = re.compile(
    r"@(?:Test|ParameterizedTest|RepeatedTest|TestFactory|Suite)\b"
)
TEST_TASK_BLOCK_RE = re.compile(r"tasks\.test\s*\{(?P<body>.*?)\n\}", re.DOTALL)
INCLUDE_RE = re.compile(r'include\(\s*"([^"]+)"\s*\)')
EXCLUDE_TAGS_RE = re.compile(r'excludeTags\(([^)]*)\)')
QUOTED_STR_RE = re.compile(r'"([^"]+)"')
TASK_LINE_RE = re.compile(r"^> Task (:[A-Za-z0-9_:-]+)(?:\s+([A-Z-]+))?\s*$")


class CensusError(Exception):
    def __init__(self, message: str, exit_code: int = EXIT_FAIL) -> None:
        super().__init__(message)
        self.exit_code = exit_code


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def canonical_json(obj: Any) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


def compute_content_sha256(doc: dict[str, Any]) -> str:
    payload = {k: v for k, v in doc.items() if k != "content_sha256"}
    return sha256_bytes(canonical_json(payload).encode("utf-8"))


def extract_test_task_config(build_gradle_text: str, module_name: str) -> dict[str, Any]:
    match = TEST_TASK_BLOCK_RE.search(build_gradle_text)
    if not match:
        raise CensusError(
            f"{module_name}: missing `tasks.test {{ ... }}` block in build.gradle.kts"
        )
    body = match.group("body")
    if "useJUnitPlatform" not in body:
        raise CensusError(
            f"{module_name}: `tasks.test` does not configure `useJUnitPlatform`"
        )
    includes = INCLUDE_RE.findall(body)
    excluded_tags: list[str] = []
    for raw_args in EXCLUDE_TAGS_RE.findall(body):
        excluded_tags.extend(QUOTED_STR_RE.findall(raw_args))
    return {
        "uses_junit_platform": True,
        "suite_filters": includes,
        "excluded_tags": excluded_tags,
    }


def check_disabled_modules(paper_dir: Path) -> list[dict[str, Any]]:
    settings_path = paper_dir / "settings.gradle.kts"
    if not settings_path.is_file():
        raise CensusError(f"missing {settings_path}")
    settings_text = settings_path.read_text(encoding="utf-8")

    results: list[dict[str, Any]] = []
    for mod in DISABLED_MODULES:
        if f'optionalInclude("{mod}")' not in settings_text:
            raise CensusError(
                f"settings.gradle.kts missing expected optionalInclude(\"{mod}\")"
            )
        enabled = False
        for raw_line in settings_text.splitlines():
            line = raw_line.strip()
            if not line or line.startswith("//"):
                continue
            if f'include(":{mod}")' in line or f'include("{mod}")' in line:
                enabled = True
                break
        opt_file = paper_dir / f"{mod}.settings.gradle.kts"
        if not enabled and opt_file.is_file():
            for raw_line in opt_file.read_text(encoding="utf-8").splitlines():
                line = raw_line.strip()
                if not line or line.startswith("//"):
                    continue
                if f'include(":{mod}")' in line or f'include("{mod}")' in line:
                    enabled = True
                    break
        if enabled:
            raise CensusError(
                f"disabled module `{mod}` is enabled in settings; "
                "disabled modules must stay disabled and excluded from baseline evidence"
            )
        results.append(
            {
                "module": mod,
                "status": "disabled",
                "evidence": False,
                "counted_source_files": 0,
                "counted_test_files": 0,
                "classification": "disabled_non_evidence",
            }
        )
    return results


def verify_paper_config(paper_dir: Path, check_git: bool = True) -> dict[str, Any]:
    if not paper_dir.is_dir():
        raise CensusError(
            f"Paper checkout not found at {paper_dir}", exit_code=EXIT_UNVERIFIED
        )

    commit = PINNED_PAPER_COMMIT
    if check_git:
        if not (paper_dir / ".git").exists():
            raise CensusError(
                f"{paper_dir} is not a git checkout", exit_code=EXIT_UNVERIFIED
            )
        head = subprocess.run(
            ["git", "-C", str(paper_dir), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            check=False,
        )
        if head.returncode != 0:
            raise CensusError(
                f"git rev-parse HEAD failed in {paper_dir}: {head.stderr.strip()}"
            )
        commit = head.stdout.strip()
        if commit != PINNED_PAPER_COMMIT:
            raise CensusError(
                f"Paper commit mismatch: got {commit}, expected {PINNED_PAPER_COMMIT}"
            )
        status = subprocess.run(
            ["git", "-C", str(paper_dir), "status", "--porcelain"],
            capture_output=True,
            text=True,
            check=False,
        )
        if status.returncode != 0:
            raise CensusError(f"git status failed in {paper_dir}")
        dirty_lines = [ln for ln in status.stdout.splitlines() if ln.strip()]
        if dirty_lines:
            raise CensusError(
                f"Paper checkout at {paper_dir} is dirty ({len(dirty_lines)} entries): "
                + ", ".join(dirty_lines[:5])
            )

    wrapper_path = paper_dir / "gradle/wrapper/gradle-wrapper.properties"
    if not wrapper_path.is_file():
        raise CensusError(f"missing {wrapper_path}")
    wrapper_text = wrapper_path.read_text(encoding="utf-8")
    expected_dist = f"gradle-{PINNED_GRADLE_VERSION}-bin.zip"
    if expected_dist not in wrapper_text:
        raise CensusError(
            f"Gradle wrapper mismatch: expected {expected_dist} in {wrapper_path}"
        )

    build_hashes: dict[str, str] = {}
    parsed_configs: dict[str, dict[str, Any]] = {}
    for mod, spec in MODULE_SPECS.items():
        bg_path = paper_dir / spec["build_gradle"]
        if not bg_path.is_file():
            raise CensusError(f"missing {bg_path}")
        bg_text = bg_path.read_text(encoding="utf-8")
        cfg = extract_test_task_config(bg_text, mod)
        if cfg["suite_filters"] != spec["suite_filters"]:
            raise CensusError(
                f"{mod}: suite_filters mismatch: got {cfg['suite_filters']}, "
                f"expected {spec['suite_filters']}"
            )
        if cfg["excluded_tags"] != spec["excluded_tags"]:
            raise CensusError(
                f"{mod}: excluded_tags mismatch: got {cfg['excluded_tags']}, "
                f"expected {spec['excluded_tags']}"
            )
        build_hashes[spec["build_gradle"]] = sha256_file(bg_path)
        parsed_configs[mod] = cfg

    settings_path = paper_dir / "settings.gradle.kts"
    build_hashes["settings.gradle.kts"] = sha256_file(settings_path)
    build_hashes["gradle/wrapper/gradle-wrapper.properties"] = sha256_file(wrapper_path)

    disabled = check_disabled_modules(paper_dir)

    return {
        "paper_commit": commit,
        "gradle_version": PINNED_GRADLE_VERSION,
        "config_sha256": build_hashes,
        "module_configs": parsed_configs,
        "disabled_modules": disabled,
    }


def _count_files(root: Path, suffix: str | None = None) -> list[Path]:
    if not root.is_dir():
        return []
    out: list[Path] = []
    for p in root.rglob("*"):
        if p.is_file() and (suffix is None or p.name.endswith(suffix)):
            out.append(p)
    out.sort()
    return out


def inventory_module(paper_dir: Path, module_name: str) -> dict[str, Any]:
    spec = MODULE_SPECS[module_name]
    test_java_root = paper_dir / module_name / "src/test/java"
    test_res_root = paper_dir / module_name / "src/test/resources"

    test_java_files = _count_files(test_java_root, ".java")
    test_like_files = 0
    support_files = 0
    discovered_suite_classes: list[str] = []

    for path in test_java_files:
        name = path.name
        stem = path.stem
        if name.endswith("TestSuite.java"):
            discovered_suite_classes.append(stem)
        is_test_like = (
            name.endswith("Test.java")
            or name.endswith("Tests.java")
            or name.endswith("TestSuite.java")
        )
        if not is_test_like:
            text = path.read_text(encoding="utf-8", errors="replace")
            if JUNIT_ANNOTATION_RE.search(text):
                is_test_like = True
        if is_test_like:
            test_like_files += 1
        else:
            support_files += 1

    discovered_suite_classes.sort()
    expected_suites = sorted(spec["expected_suite_classes"])
    if expected_suites and discovered_suite_classes != expected_suites:
        raise CensusError(
            f"{module_name}: discovered TestSuite classes {discovered_suite_classes} "
            f"!= expected {expected_suites}"
        )

    main_source_files = sum(
        len(_count_files(paper_dir / rel, ".java")) for rel in spec["source_roots"]
    )
    generated_source_files = sum(
        len(_count_files(paper_dir / rel, ".java")) for rel in spec["generated_roots"]
    )
    patch_files = sum(
        len(_count_files(paper_dir / rel, ".patch")) for rel in spec["patch_roots"]
    )
    test_resource_files = len(_count_files(test_res_root))

    return {
        "source_inventory": {
            "main_java_files": main_source_files,
            "test_java_files": len(test_java_files),
            "test_like_java_files": test_like_files,
            "discovered_suite_classes": discovered_suite_classes,
        },
        "non_test_assets": {
            "support_java_files": support_files,
            "test_resource_files": test_resource_files,
            "generated_source_files": generated_source_files,
            "patch_files": patch_files,
            "classification": "inputs_and_harness_assets_never_tests",
        },
    }


def parse_junit_results(results_dir: Path, module_name: str) -> dict[str, Any]:
    if not results_dir.is_dir():
        raise CensusError(
            f"{module_name}: JUnit results directory missing: {results_dir}"
        )
    xml_files = sorted(results_dir.glob("TEST-*.xml"))
    if not xml_files:
        raise CensusError(
            f"{module_name}: no TEST-*.xml result artifacts found in {results_dir}"
        )

    suites: list[dict[str, Any]] = []
    synthetic_wrappers: list[dict[str, Any]] = []

    total_selected = 0
    total_executed = 0
    total_passed = 0
    total_failures = 0
    total_errors = 0
    total_skipped = 0

    for xml_path in xml_files:
        try:
            tree = ET.parse(xml_path)
            root = tree.getroot()
        except ET.ParseError as exc:
            raise CensusError(
                f"{module_name}: malformed JUnit XML {xml_path.name}: {exc}"
            ) from exc

        if root.tag != "testsuite":
            raise CensusError(
                f"{module_name}: unexpected root tag <{root.tag}> in {xml_path.name}"
            )

        name = root.attrib.get("name", "")
        if not name:
            raise CensusError(f"{module_name}: missing testsuite name in {xml_path.name}")

        try:
            tests = int(root.attrib["tests"])
            skipped = int(root.attrib.get("skipped", "0"))
            failures = int(root.attrib.get("failures", "0"))
            errors = int(root.attrib.get("errors", "0"))
        except (KeyError, ValueError) as exc:
            raise CensusError(
                f"{module_name}: invalid numeric counts in {xml_path.name}: {exc}"
            ) from exc

        if min(tests, skipped, failures, errors) < 0:
            raise CensusError(f"{module_name}: negative count in {xml_path.name}")
        if skipped + failures + errors > tests:
            raise CensusError(
                f"{module_name}: inconsistent counts in {xml_path.name}: "
                f"tests={tests} skipped={skipped} failures={failures} errors={errors}"
            )

        testcases = root.findall("testcase")
        if len(testcases) != tests:
            raise CensusError(
                f"{module_name}: testcase element count ({len(testcases)}) != "
                f"tests attribute ({tests}) in {xml_path.name}"
            )

        executed = tests - skipped
        passed = executed - failures - errors

        entry = {
            "name": name,
            "file": xml_path.name,
            "sha256": sha256_file(xml_path),
            "selected": tests,
            "executed": executed,
            "passed": passed,
            "failures": failures,
            "errors": errors,
            "skipped": skipped,
        }

        if name.startswith("Gradle Test Run "):
            synthetic_wrappers.append(entry)
            continue

        suites.append(entry)
        total_selected += tests
        total_executed += executed
        total_passed += passed
        total_failures += failures
        total_errors += errors
        total_skipped += skipped

    if not suites:
        raise CensusError(
            f"{module_name}: zero real JUnit test suites found in {results_dir}"
        )
    if total_selected <= 0 or total_executed <= 0:
        raise CensusError(
            f"{module_name}: zero-test run (selected={total_selected}, executed={total_executed})"
        )
    if total_failures > 0 or total_errors > 0:
        raise CensusError(
            f"{module_name}: Paper-vs-Paper control not green "
            f"(failures={total_failures}, errors={total_errors})"
        )

    suites.sort(key=lambda s: (s["name"], s["file"]))
    synthetic_wrappers.sort(key=lambda s: (s["name"], s["file"]))

    return {
        "suite_count": len(suites),
        "selected": total_selected,
        "executed": total_executed,
        "passed": total_passed,
        "failed": total_failures + total_errors,
        "failures": total_failures,
        "errors": total_errors,
        "skipped": total_skipped,
        "synthetic_wrappers": synthetic_wrappers,
        "suites": suites,
    }


def check_gradle_task_execution(
    gradle_output: str, expected_tasks: tuple[str, ...] = EXPECTED_TASKS
) -> dict[str, str]:
    statuses: dict[str, str] = {}
    for line in gradle_output.splitlines():
        m = TASK_LINE_RE.match(line.strip())
        if not m:
            continue
        task_name = m.group(1)
        outcome = m.group(2) or "EXECUTED"
        statuses[task_name] = outcome

    for task in expected_tasks:
        if task not in statuses:
            raise CensusError(f"expected Gradle task {task} did not appear in build output")
        if statuses[task] != "EXECUTED":
            raise CensusError(
                f"expected Gradle task {task} to be EXECUTED, got {statuses[task]}"
            )
    return {t: statuses[t] for t in expected_tasks}


def detect_java_version(java_home: str | None) -> str:
    java_bin = str(Path(java_home) / "bin" / "java") if java_home else "java"
    proc = subprocess.run(
        [java_bin, "-version"],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise CensusError(
            f"failed to run `{java_bin} -version`", exit_code=EXIT_UNVERIFIED
        )
    combined = (proc.stderr + "\n" + proc.stdout).strip()
    version_line = ""
    for line in combined.splitlines():
        s = line.strip()
        if "version" in s.lower() and ('"25.' in s or " 25." in s):
            version_line = s
            break
    if not version_line:
        non_note = [
            l.strip()
            for l in combined.splitlines()
            if l.strip() and not l.strip().startswith("NOTE:") and not l.strip().startswith("Picked up ")
        ]
        detected = non_note[0] if non_note else (combined.splitlines()[0] if combined else "")
        raise CensusError(
            f"Paper requires JDK 25; detected: {detected}", exit_code=EXIT_UNVERIFIED
        )
    return version_line


def run_paper_tests(paper_dir: Path, java_home: str | None) -> tuple[str, str]:
    java_version = detect_java_version(java_home)
    env = os.environ.copy()
    if java_home:
        env["JAVA_HOME"] = java_home
        env["PATH"] = f"{Path(java_home) / 'bin'}{os.pathsep}{env.get('PATH', '')}"

    cmd = [
        "./gradlew",
        *EXPECTED_TASKS,
        "--rerun-tasks",
        "--no-daemon",
        "--console=plain",
    ]
    proc = subprocess.run(
        cmd,
        cwd=str(paper_dir),
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    output = proc.stdout + "\n" + proc.stderr
    if proc.returncode != 0:
        tail = "\n".join(output.splitlines()[-40:])
        raise CensusError(
            f"Gradle test execution failed (exit {proc.returncode}):\n{tail}"
        )
    return java_version, output


def produce_census(
    paper_dir: Path,
    java_home: str | None = None,
    gradle_log_text: str | None = None,
    java_version_override: str | None = None,
    check_git: bool = True,
) -> dict[str, Any]:
    cfg = verify_paper_config(paper_dir, check_git=check_git)

    if gradle_log_text is None:
        java_version, gradle_output = run_paper_tests(paper_dir, java_home)
    else:
        java_version = java_version_override or detect_java_version(java_home)
        gradle_output = gradle_log_text

    task_outcomes = check_gradle_task_execution(gradle_output, EXPECTED_TASKS)

    modules_out: dict[str, Any] = {}
    totals = {
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
        inv = inventory_module(paper_dir, mod)
        parsed = parse_junit_results(paper_dir / spec["results_dir"], mod)
        mod_cfg = cfg["module_configs"][mod]
        modules_out[mod] = {
            "task": spec["task"],
            "task_outcome": task_outcomes[spec["task"]],
            "junit_config": {
                "uses_junit_platform": mod_cfg["uses_junit_platform"],
                "suite_filters": mod_cfg["suite_filters"],
                "excluded_tags": mod_cfg["excluded_tags"],
            },
            "execution": parsed,
            "source_inventory": inv["source_inventory"],
            "non_test_assets": inv["non_test_assets"],
        }
        totals["active_modules"] += 1
        totals["suite_count"] += parsed["suite_count"]
        totals["selected"] += parsed["selected"]
        totals["executed"] += parsed["executed"]
        totals["passed"] += parsed["passed"]
        totals["failed"] += parsed["failed"]
        totals["failures"] += parsed["failures"]
        totals["errors"] += parsed["errors"]
        totals["skipped"] += parsed["skipped"]

    doc: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "provenance": {
            "paper_commit": cfg["paper_commit"],
            "gradle_version": cfg["gradle_version"],
            "java_version": java_version,
            "tasks": list(EXPECTED_TASKS),
            "config_sha256": cfg["config_sha256"],
        },
        "modules": modules_out,
        "disabled_modules": cfg["disabled_modules"],
        "totals": totals,
    }
    doc["content_sha256"] = compute_content_sha256(doc)
    return doc


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--paper-source",
        type=Path,
        default=Path("working/Paper"),
        help="Path to the clean pinned Paper checkout (default: working/Paper)",
    )
    parser.add_argument(
        "--java-home",
        type=str,
        default=os.environ.get("JAVA_HOME"),
        help="Path to JDK 25 JAVA_HOME",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("tools/paper-junit-baseline/fixtures/baseline-census.json"),
        help="Path to write the versioned census JSON artifact",
    )
    parser.add_argument(
        "--gradle-log",
        type=Path,
        default=None,
        help="Optional captured Gradle --console=plain log (skips re-running Gradle when results already exist)",
    )
    args = parser.parse_args(argv)

    try:
        gradle_log_text = (
            args.gradle_log.read_text(encoding="utf-8") if args.gradle_log else None
        )
        census = produce_census(
            paper_dir=args.paper_source,
            java_home=args.java_home,
            gradle_log_text=gradle_log_text,
        )
    except CensusError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return exc.exit_code

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(census, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    t = census["totals"]
    print(
        f"VERIFIED: wrote {args.output} "
        f"(suites={t['suite_count']} selected={t['selected']} "
        f"executed={t['executed']} passed={t['passed']} "
        f"failed={t['failed']} skipped={t['skipped']})"
    )
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Controlled-negative and contract tests for tools/paper-junit-baseline."""

from __future__ import annotations

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import census
import validate


API_BUILD = """
tasks.test {
    useJUnitPlatform()
}
"""

SERVER_BUILD = """
tasks.test {
    include("**/**TestSuite.class")
    workingDir = temporaryDir
    useJUnitPlatform {
        forkEvery = 1
        excludeTags("Slow")
    }
}
"""

SETTINGS_GRADLE = """
rootProject.name = "paper"
include("paper-api", "paper-server")
optionalInclude("paper-generator")
optionalInclude("test-plugin")
"""

GRADLE_WRAPPER = f"""
distributionUrl=https\\://services.gradle.org/distributions/gradle-{census.PINNED_GRADLE_VERSION}-bin.zip
"""

SAMPLE_GRADLE_LOG = """
> Task :paper-api:compileJava UP-TO-DATE
> Task :paper-api:test
> Task :paper-server:compileJava UP-TO-DATE
> Task :paper-server:test
BUILD SUCCESSFUL in 12s
"""

SAMPLE_API_XML = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="org.bukkit.SampleApiTest" tests="3" skipped="1" failures="0" errors="0" timestamp="2026-11-21T11:41:37Z" hostname="box" time="0.01">
  <testcase name="testOne()" classname="org.bukkit.SampleApiTest" time="0.005"/>
  <testcase name="testTwo()" classname="org.bukkit.SampleApiTest" time="0.005"/>
  <testcase name="testThree()" classname="org.bukkit.SampleApiTest" time="0.0"><skipped/></testcase>
</testsuite>
"""

SAMPLE_SERVER_WRAPPER_XML = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="Gradle Test Run :paper-server:test" tests="1" skipped="0" failures="0" errors="0" timestamp="2026-11-21T11:40:56Z" hostname="box" time="10.0">
  <testcase name="suite" classname="Gradle Test Run :paper-server:test" time="10.0"/>
</testsuite>
"""

SAMPLE_SERVER_XML = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="io.papermc.paper.SampleServerTestSuite" tests="2" skipped="0" failures="0" errors="0" timestamp="2026-11-21T11:41:38Z" hostname="box" time="0.02">
  <testcase name="testAlpha()" classname="io.papermc.paper.SampleServerTestSuite" time="0.01"/>
  <testcase name="testBeta()" classname="io.papermc.paper.SampleServerTestSuite" time="0.01"/>
</testsuite>
"""


def build_synthetic_paper(root: Path) -> Path:
    (root / "gradle" / "wrapper").mkdir(parents=True)
    (root / "gradle" / "wrapper" / "gradle-wrapper.properties").write_text(
        GRADLE_WRAPPER, encoding="utf-8"
    )
    (root / "settings.gradle.kts").write_text(SETTINGS_GRADLE, encoding="utf-8")

    # paper-api
    (root / "paper-api" / "src" / "main" / "java" / "org" / "bukkit").mkdir(parents=True)
    (root / "paper-api" / "src" / "main" / "java" / "org" / "bukkit" / "Api.java").write_text(
        "package org.bukkit; public class Api {}\n", encoding="utf-8"
    )
    (root / "paper-api" / "src" / "test" / "java" / "org" / "bukkit").mkdir(parents=True)
    (root / "paper-api" / "src" / "test" / "java" / "org" / "bukkit" / "SampleApiTest.java").write_text(
        "package org.bukkit; public class SampleApiTest {}\n", encoding="utf-8"
    )
    (root / "paper-api" / "src" / "test" / "java" / "org" / "bukkit" / "TestSupport.java").write_text(
        "package org.bukkit; public class TestSupport {}\n", encoding="utf-8"
    )
    (root / "paper-api" / "src" / "test" / "resources").mkdir(parents=True)
    (root / "paper-api" / "src" / "test" / "resources" / "fixture.json").write_text(
        "{}\n", encoding="utf-8"
    )
    (root / "paper-api" / "build.gradle.kts").write_text(API_BUILD, encoding="utf-8")
    api_res = root / "paper-api" / "build" / "test-results" / "test"
    api_res.mkdir(parents=True)
    (api_res / "TEST-org.bukkit.SampleApiTest.xml").write_text(
        SAMPLE_API_XML, encoding="utf-8"
    )

    # paper-server
    (root / "paper-server" / "src" / "main" / "java" / "io" / "papermc").mkdir(parents=True)
    (root / "paper-server" / "src" / "main" / "java" / "io" / "papermc" / "Srv.java").write_text(
        "package io.papermc; public class Srv {}\n", encoding="utf-8"
    )
    (root / "paper-server" / "src" / "test" / "java" / "io" / "papermc").mkdir(parents=True)
    for suite_cls in census.MODULE_SPECS["paper-server"]["expected_suite_classes"]:
        (
            root / "paper-server" / "src" / "test" / "java" / "io" / "papermc" / f"{suite_cls}.java"
        ).write_text(
            f"package io.papermc; public class {suite_cls} {{}}\n", encoding="utf-8"
        )
    (root / "paper-server" / "patches" / "sources").mkdir(parents=True)
    (root / "paper-server" / "patches" / "sources" / "sample.patch").write_text(
        "--- a\n+++ b\n", encoding="utf-8"
    )
    (root / "paper-server" / "build.gradle.kts").write_text(SERVER_BUILD, encoding="utf-8")
    srv_res = root / "paper-server" / "build" / "test-results" / "test"
    srv_res.mkdir(parents=True)
    (srv_res / "TEST-Gradle-Test-Run--paper-server-test.xml").write_text(
        SAMPLE_SERVER_WRAPPER_XML, encoding="utf-8"
    )
    (srv_res / "TEST-io.papermc.paper.SampleServerTestSuite.xml").write_text(
        SAMPLE_SERVER_XML, encoding="utf-8"
    )

    # disabled modules (must NOT be counted as active evidence)
    (root / "paper-generator" / "src" / "test" / "java").mkdir(parents=True)
    (root / "paper-generator" / "src" / "test" / "java" / "GenTest.java").write_text(
        "public class GenTest {}\n", encoding="utf-8"
    )
    (root / "test-plugin" / "src" / "main" / "java").mkdir(parents=True)
    (root / "test-plugin" / "src" / "main" / "java" / "Plugin.java").write_text(
        "public class Plugin {}\n", encoding="utf-8"
    )
    return root


class PaperJunitBaselineTests(unittest.TestCase):
    def _produce_valid(self, root: Path) -> dict:
        return census.produce_census(
            paper_dir=root,
            gradle_log_text=SAMPLE_GRADLE_LOG,
            java_version_override='openjdk version "25.0.5" 2025-10-21 LTS',
            check_git=False,
        )

    def test_valid_round_trip_and_counts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            doc = self._produce_valid(root)
            validate.validate_census_doc(doc)
            validate.validate_against_paper(doc, root, check_results=True, check_git=False)

            self.assertEqual(doc["totals"]["selected"], 5)
            self.assertEqual(doc["totals"]["executed"], 4)
            self.assertEqual(doc["totals"]["passed"], 4)
            self.assertEqual(doc["totals"]["skipped"], 1)
            self.assertEqual(doc["totals"]["failed"], 0)
            self.assertEqual(doc["totals"]["suite_count"], 2)
            # Synthetic Gradle Test Run wrapper is separated from real suites
            srv_exec = doc["modules"]["paper-server"]["execution"]
            self.assertEqual(srv_exec["suite_count"], 1)
            self.assertEqual(len(srv_exec["synthetic_wrappers"]), 1)
            # Disabled modules recorded as non-evidence and never counted in active modules
            self.assertEqual(set(doc["modules"].keys()), {"paper-api", "paper-server"})
            disabled_by_name = {d["module"]: d for d in doc["disabled_modules"]}
            for disabled in census.DISABLED_MODULES:
                self.assertEqual(disabled_by_name[disabled]["classification"], "disabled_non_evidence")
                self.assertFalse(disabled_by_name[disabled]["evidence"])

    def test_suite_filter_mutation_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            (root / "paper-server" / "build.gradle.kts").write_text(
                SERVER_BUILD.replace('include("**/**TestSuite.class")', 'include("**/*Test.class")'),
                encoding="utf-8",
            )
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_excluded_tag_mutation_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            (root / "paper-server" / "build.gradle.kts").write_text(
                SERVER_BUILD.replace('excludeTags("Slow")', 'excludeTags("Fast")'),
                encoding="utf-8",
            )
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_deleted_result_file_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            doc = self._produce_valid(root)
            (root / "paper-api" / "build" / "test-results" / "test" / "TEST-org.bukkit.SampleApiTest.xml").unlink()
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)
            with self.assertRaises(census.CensusError):
                validate.validate_against_paper(doc, root, check_results=True, check_git=False)

    def test_zero_test_run_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            zero_xml = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="org.bukkit.EmptyTest" tests="0" skipped="0" failures="0" errors="0"/>
"""
            (
                root / "paper-api" / "build" / "test-results" / "test" / "TEST-org.bukkit.SampleApiTest.xml"
            ).write_text(zero_xml, encoding="utf-8")
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_all_skipped_zero_executed_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            skipped_xml = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="org.bukkit.SkippedTest" tests="1" skipped="1" failures="0" errors="0">
  <testcase name="testOne()" classname="org.bukkit.SkippedTest" time="0.0"><skipped/></testcase>
</testsuite>
"""
            (
                root / "paper-api" / "build" / "test-results" / "test" / "TEST-org.bukkit.SampleApiTest.xml"
            ).write_text(skipped_xml, encoding="utf-8")
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_skipped_gradle_task_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            for bad_log in (
                "> Task :paper-api:test\n> Task :paper-server:test SKIPPED\n",
                "> Task :paper-api:test UP-TO-DATE\n> Task :paper-server:test\n",
                "> Task :paper-api:test\n> Task :paper-server:test NO-SOURCE\n",
                "> Task :paper-api:test\n",
            ):
                with self.assertRaises(census.CensusError):
                    census.produce_census(
                        paper_dir=root,
                        gradle_log_text=bad_log,
                        java_version_override='openjdk version "25.0.5"',
                        check_git=False,
                    )

    def test_disabled_module_enabled_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            (root / "settings.gradle.kts").write_text(
                SETTINGS_GRADLE + '\ninclude("paper-generator")\n', encoding="utf-8"
            )
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_failing_paper_control_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            fail_xml = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="org.bukkit.FailTest" tests="1" skipped="0" failures="1" errors="0">
  <testcase name="testFail()" classname="org.bukkit.FailTest" time="0.0"><failure message="boom"/></testcase>
</testsuite>
"""
            (
                root / "paper-api" / "build" / "test-results" / "test" / "TEST-org.bukkit.SampleApiTest.xml"
            ).write_text(fail_xml, encoding="utf-8")
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_tampered_digest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            doc = self._produce_valid(root)
            tampered = copy.deepcopy(doc)
            tampered["totals"]["passed"] += 1
            with self.assertRaises(census.CensusError):
                validate.validate_census_doc(tampered)

    def test_missing_paper_checkout_returns_unverified(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "does-not-exist"
            with self.assertRaises(census.CensusError) as ctx:
                census.verify_paper_config(missing, check_git=False)
            self.assertEqual(ctx.exception.exit_code, census.EXIT_UNVERIFIED)

    def test_synthetic_wrapper_failure_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            bad_wrapper = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="Gradle Test Run :paper-server:test" tests="1" skipped="0" failures="0" errors="1">
  <testcase name="executionError" classname="Gradle Test Run :paper-server:test" time="0.0"><error message="container boom"/></testcase>
</testsuite>
"""
            (
                root / "paper-server" / "build" / "test-results" / "test" / "TEST-Gradle-Test-Run--paper-server-test.xml"
            ).write_text(bad_wrapper, encoding="utf-8")
            with self.assertRaises(census.CensusError):
                self._produce_valid(root)

    def test_renamed_testcase_fails_live_validation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            doc = self._produce_valid(root)
            api_xml = root / "paper-api" / "build" / "test-results" / "test" / "TEST-org.bukkit.SampleApiTest.xml"
            api_xml.write_text(
                api_xml.read_text(encoding="utf-8").replace("testOne()", "testRenamed()"),
                encoding="utf-8",
            )
            with self.assertRaises(census.CensusError):
                validate.validate_against_paper(doc, root, check_results=True, check_git=False)

    def test_missing_java_returns_unverified(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(census.CensusError) as ctx:
                census.detect_java_version(str(Path(tmp) / "no-such-jdk"))
            self.assertEqual(ctx.exception.exit_code, census.EXIT_UNVERIFIED)

    def test_jdk25_ga_version_accepted_and_other_majors_rejected(self) -> None:
        self.assertTrue(census.is_jdk25_version('openjdk version "25" 2025-09-16'))
        self.assertTrue(census.is_jdk25_version('openjdk version "25.0.5" 2025-10-21 LTS'))
        self.assertTrue(census.is_jdk25_version('openjdk version "25-ea" 2025-09-16'))
        self.assertFalse(census.is_jdk25_version('openjdk version "24.0.2" 2025-07-15'))
        self.assertFalse(census.is_jdk25_version('openjdk version "250.0.1" 2030-01-01'))
        with tempfile.TemporaryDirectory() as tmp:
            root = build_synthetic_paper(Path(tmp))
            doc = census.produce_census(
                paper_dir=root,
                gradle_log_text=SAMPLE_GRADLE_LOG,
                java_version_override='openjdk version "25" 2025-09-16',
                check_git=False,
            )
            validate.validate_census_doc(doc)


if __name__ == "__main__":
    unittest.main()

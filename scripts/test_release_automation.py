#!/usr/bin/env python3
"""Regression tests for release metadata validation and archive packaging."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent
VALIDATOR = SCRIPT_DIR / "validate_release_tag.py"
PACKAGER = SCRIPT_DIR / "package_release.py"


class ReleaseAutomationTests(unittest.TestCase):
    def run_validator(self, tag: str, *extra: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(VALIDATOR), tag, *extra],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_stable_tag_matches_committed_metadata(self) -> None:
        result = self.run_validator("rivet-v0.1.0-mc26.2", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        values = json.loads(result.stdout)
        self.assertEqual(values["rivet_version"], "0.1.0")
        self.assertEqual(values["minecraft_version"], "26.2")
        self.assertFalse(values["prerelease"])
        self.assertEqual(values["image_full_tag"], "rivet-v0.1.0-mc26.2")
        self.assertEqual(values["image_line_tag"], "mc26.2")

    def test_supported_prerelease_tags_are_marked_prerelease(self) -> None:
        for channel in ("alpha", "beta", "rc"):
            with self.subTest(channel=channel):
                result = self.run_validator(f"rivet-v0.1.0-{channel}.1-mc26.2", "--json")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(json.loads(result.stdout)["prerelease"])

    def test_wrong_versions_and_unknown_prereleases_are_rejected(self) -> None:
        for tag in (
            "v0.1.0-mc26.2",
            "rivet-v0.2.0-mc26.2",
            "rivet-v0.1.0-mc1.21.8",
            "rivet-v0.1.0-dev-mc26.2",
            "rivet-v0.1.0-preview.1-mc26.2",
            "rivet-v0.1.0-alpha.1+build-mc26.2",
            "rivet-v0.1.0-rc.01-mc26.2",
            "rivet-v0.1.0-alpha.00-mc26.2",
            "rivet-v0.1.0-beta.001-mc26.2",
        ):
            with self.subTest(tag=tag):
                result = self.run_validator(tag)
                self.assertEqual(result.returncode, 2)
                self.assertIn("release tag validation failed", result.stderr)

    def test_github_output_is_machine_readable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "github-output"
            result = self.run_validator(
                "rivet-v0.1.0-rc.2-mc26.2", "--github-output", str(output)
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                output.read_text(),
                "tag=rivet-v0.1.0-rc.2-mc26.2\n"
                "rivet_version=0.1.0-rc.2\n"
                "rivet_base_version=0.1.0\n"
                "minecraft_version=26.2\n"
                "prerelease=true\n"
                "image_full_tag=rivet-v0.1.0-rc.2-mc26.2\n"
                "image_line_tag=mc26.2\n",
            )

    def test_archive_contains_only_release_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            temporary_path = Path(temporary)
            binary = temporary_path / "rivet-server"
            binary.write_bytes(b"test executable")
            binary.chmod(0o755)
            output_dir = temporary_path / "dist"
            result = subprocess.run(
                [
                    sys.executable,
                    str(PACKAGER),
                    "rivet-v0.1.0-mc26.2",
                    "--target",
                    "x86_64-unknown-linux-gnu",
                    "--binary",
                    str(binary),
                    "--output-dir",
                    str(output_dir),
                ],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env={**os.environ, "PYTHONPATH": str(SCRIPT_DIR)},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            archive = Path(result.stdout.strip())
            self.assertTrue(archive.is_file())
            with tarfile.open(archive, "r:gz") as tar:
                names = sorted(tar.getnames())
                prefix = "rivet-v0.1.0-mc26.2-x86_64-unknown-linux-gnu"
                self.assertEqual(
                    names,
                    [f"{prefix}", f"{prefix}/LICENSE", f"{prefix}/README.md", f"{prefix}/rivet-server"],
                )
                member = tar.getmember(f"{prefix}/rivet-server")
                self.assertEqual(member.mode & 0o111, 0o111)
                self.assertEqual(tar.extractfile(member).read(), b"test executable")


if __name__ == "__main__":
    unittest.main()

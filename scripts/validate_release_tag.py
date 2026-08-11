#!/usr/bin/env python3
"""Validate a Rivet release tag against Cargo's committed metadata.

The accepted form is ``rivet-v<semver>-mc<minecraft-version>``. Only the
alpha, beta, and rc prerelease identifiers are accepted; the stable form has
no prerelease identifier.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 and older
    tomllib = None

ROOT = Path(__file__).resolve().parents[1]
CARGO_MANIFEST = ROOT / "Cargo.toml"
BASE_VERSION = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
MINECRAFT_VERSION = r"[0-9]+(?:\.[0-9]+)+"
TAG_PATTERN = re.compile(
    rf"^rivet-v(?P<base>{BASE_VERSION})"
    rf"(?P<prerelease>-(?:alpha|beta|rc)(?:\.(?:0|[1-9][0-9]*))?)?"
    rf"-mc(?P<minecraft>{MINECRAFT_VERSION})$"
)


class ReleaseTagError(ValueError):
    """A release tag or its committed metadata is invalid."""


def _minimal_toml_metadata(text: str) -> dict[str, Any]:
    """Read the three string keys this script needs without external packages."""
    result: dict[str, Any] = {"workspace": {"package": {}, "metadata": {"rivet": {}}}}
    sections = (
        ("workspace.package", result["workspace"]["package"], ("version",)),
        (
            "workspace.metadata.rivet",
            result["workspace"]["metadata"]["rivet"],
            ("release-version", "minecraft-version"),
        ),
    )
    for section, target, keys in sections:
        section_match = re.search(rf"(?ms)^\[{re.escape(section)}\]\s*(.*?)(?=^\[|\Z)", text)
        if section_match is None:
            raise ValueError(f"missing [{section}] section")
        for key in keys:
            value_match = re.search(
                rf"(?m)^{re.escape(key)}\s*=\s*\"([^\"]+)\"\s*$", section_match.group(1)
            )
            if value_match is None:
                raise ValueError(f"missing {key!r} in [{section}]")
            target[key] = value_match.group(1)
    return result


def _metadata() -> tuple[str, str]:
    try:
        if tomllib is None:
            document = _minimal_toml_metadata(CARGO_MANIFEST.read_text(encoding="utf-8"))
        else:
            with CARGO_MANIFEST.open("rb") as manifest:
                document: dict[str, Any] = tomllib.load(manifest)
        rivet = document["workspace"]["metadata"]["rivet"]
        release_version = rivet["release-version"]
        minecraft_version = rivet["minecraft-version"]
        package_version = document["workspace"]["package"]["version"]
    except (KeyError, TypeError, OSError, ValueError) as error:
        raise ReleaseTagError(f"cannot read release metadata from {CARGO_MANIFEST}: {error}") from error

    if not isinstance(release_version, str) or not re.fullmatch(BASE_VERSION, release_version):
        raise ReleaseTagError("workspace.metadata.rivet.release-version must be a stable x.y.z version")
    if not isinstance(minecraft_version, str) or not re.fullmatch(MINECRAFT_VERSION, minecraft_version):
        raise ReleaseTagError("workspace.metadata.rivet.minecraft-version must be a dotted numeric version")
    expected_package_version = f"{release_version}-dev+mc{minecraft_version}"
    if package_version != expected_package_version:
        raise ReleaseTagError(
            "workspace.package.version must match "
            f"{expected_package_version!r}; found {package_version!r}"
        )
    return release_version, minecraft_version


def validate_tag(tag: str) -> dict[str, str | bool]:
    """Return normalized release values or raise ``ReleaseTagError``."""
    release_version, minecraft_version = _metadata()
    match = TAG_PATTERN.fullmatch(tag)
    if match is None:
        raise ReleaseTagError(
            "tag must match rivet-v<major>.<minor>.<patch>[-alpha[.N]|-beta[.N]|-rc[.N]]-mc<version>"
        )

    base_version = match.group("base")
    tag_minecraft_version = match.group("minecraft")
    prerelease = match.group("prerelease")
    if base_version != release_version:
        raise ReleaseTagError(
            f"tag Rivet version {base_version!r} does not match committed {release_version!r}"
        )
    if tag_minecraft_version != minecraft_version:
        raise ReleaseTagError(
            "tag Minecraft version "
            f"{tag_minecraft_version!r} does not match committed {minecraft_version!r}"
        )

    rivet_version = f"{base_version}{prerelease or ''}"
    return {
        "tag": tag,
        "rivet_version": rivet_version,
        "rivet_base_version": base_version,
        "minecraft_version": minecraft_version,
        "prerelease": bool(prerelease),
        "image_full_tag": tag,
        "image_line_tag": f"mc{minecraft_version}",
    }


def _github_output(values: dict[str, str | bool]) -> str:
    return "".join(f"{key}={str(value).lower() if isinstance(value, bool) else value}\n" for key, value in values.items())


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="release tag to validate")
    parser.add_argument("--json", action="store_true", help="print normalized values as JSON")
    parser.add_argument(
        "--github-output",
        type=Path,
        help="append normalized values in GitHub Actions output-file format",
    )
    args = parser.parse_args(argv)

    try:
        values = validate_tag(args.tag)
    except ReleaseTagError as error:
        print(f"release tag validation failed: {error}", file=sys.stderr)
        return 2

    if args.github_output is not None:
        with args.github_output.open("a", encoding="utf-8") as output:
            output.write(_github_output(values))
    if args.json:
        print(json.dumps(values, sort_keys=True))
    elif args.github_output is None:
        print(_github_output(values), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

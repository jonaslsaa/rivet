#!/usr/bin/env python3
"""Create a portable Rivet release archive for one Rust target."""

from __future__ import annotations

import argparse
import shutil
import sys
import tarfile
import tempfile
from pathlib import Path

from validate_release_tag import ReleaseTagError, validate_tag

SUPPORTED_TARGETS = {
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
}


def package_release(tag: str, target: str, binary: Path, output_dir: Path) -> Path:
    if target not in SUPPORTED_TARGETS:
        supported = ", ".join(sorted(SUPPORTED_TARGETS))
        raise ValueError(f"unsupported target {target!r}; expected one of: {supported}")
    try:
        values = validate_tag(tag)
    except ReleaseTagError:
        raise
    if not binary.is_file():
        raise ValueError(f"release binary does not exist: {binary}")

    archive_stem = f"rivet-{tag.removeprefix('rivet-')}-{target}"
    output_dir.mkdir(parents=True, exist_ok=True)
    archive = output_dir / f"{archive_stem}.tar.gz"
    with tempfile.TemporaryDirectory(prefix="rivet-release-") as temporary:
        staging_root = Path(temporary) / archive_stem
        staging_root.mkdir()
        shutil.copy2(binary, staging_root / "rivet-server")
        shutil.copy2(Path(__file__).resolve().parents[1] / "README.md", staging_root / "README.md")
        shutil.copy2(Path(__file__).resolve().parents[1] / "LICENSE", staging_root / "LICENSE")
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(staging_root, arcname=archive_stem)
    return archive


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("--target", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        archive = package_release(args.tag, args.target, args.binary, args.output_dir)
    except (OSError, ValueError, ReleaseTagError) as error:
        print(f"release packaging failed: {error}", file=sys.stderr)
        return 2
    print(archive)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

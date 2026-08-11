#!/usr/bin/env python3
"""Extract / verify the #371 loaded-world auxiliary-data fixtures.

Read-only extraction of the smallest deterministic 26.2 fixture corpus needed
by the loaded-world aux-data consumers (#369 / #339 / #316), taken from the
disposable copy at `working/client-worlds/New World`:

  * one clean FULL spawn chunk (no structure refs, no saved ticks, no block
    entities) — the radius-0 baseline;
  * one chunk with mineshaft `structures.References` (a radius-1 blocker);
  * one chunk with saved `fluid_ticks` (a radius-1 blocker);
  * one chunk with saved `block_ticks`;
  * one chunk with a chest `block_entities` entry.

The original launcher save is never accessed and never mutated: the source is
ONLY the disposable `working/client-worlds/New World` copy, and the whole copy
is fingerprinted (relative path + SHA-256 for every regular file, symlinks
refused) before and after extraction and asserted unchanged. The fixture corpus
itself contains nothing but the five decompressed chunk-NBT payloads — no
level.dat, player data, world data, or other private metadata.

Determinism follows the M0/M2 chunk fixtures (see extract_fixtures.py): raw
region files are not byte-stable, but the decompressed chunk-NBT payloads of a
fixed save ARE. `--verify` re-extracts the corpus in memory and diffs the
payloads byte-for-byte against the committed `.nbt` fixtures, then re-verifies
the committed manifest SHA-256s and the source fingerprint. When the disposable
source is absent (e.g. CI), verification exits nonzero with UNVERIFIED — it
never silently passes.

Negative controls (`--expect-fail`): the committed fixtures are copied to a
dedicated scratch dir that THIS invocation alone creates and owns (a
`tempfile.TemporaryDirectory` under the system temp namespace — never a
user-supplied path, so no `rmtree` can ever reach anything but the exact
directory this run created). One file is deleted and one file's content is
flipped there, and both the static verification and the --verify re-extraction
byte-compare must fail loudly naming the tamper. `--expect-fail` never touches
the committed fixtures, and re-verifies them unchanged afterwards.

Usage:
  python3 tools/rivet-oracle/scripts/extract_loaded_world.py            # extract fixtures from the disposable copy
  python3 tools/rivet-oracle/scripts/extract_loaded_world.py --verify   # prove committed fixtures byte-identical to the source
  python3 tools/rivet-oracle/scripts/extract_loaded_world.py --expect-fail
                                                                        # prove missing/tampered fixtures are detected
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

# Shared hashing + region-parsing helpers live in extract_fixtures.py (same dir,
# stdlib-only, import-safe) so the M0/M2 pipeline and this pipeline never drift:
# the parent clamps an oversized region length header and its sha256 helpers are
# byte-for-byte the ones needed here.
from extract_fixtures import read_region_chunks, sha256_bytes, sha256_file

# ---------------------------------------------------------------------------
# Identity of the corpus: chunk world coordinates -> role. Smallest genuine
# representatives found in the disposable copy by the radius-1 probe (issue
# #371 evidence, DataVersion 4903, spawn chunk (-1,-3)).
# ---------------------------------------------------------------------------

# (world_x, world_z, role)
CORPUS: list[tuple[int, int, str]] = [
    (-1, -3, "clean-spawn"),
    (0, -4, "mineshaft-structure-refs"),
    (-2, -2, "fluid-ticks"),
    (-17, -19, "block-ticks"),
    (-19, -21, "chest-block-entity"),
]

# Paper pin of the save window (matches the M0/M2 fixture pin).
PAPER_PIN = "26.2-DEV-main@0a99345"
PAPER_COMMIT = "0a993450f129c4942c2a9ed45ba047412b4667cf"
MINECRAFT = "26.2"
DATA_VERSION = 4903

SCRIPT_DIR = Path(__file__).resolve().parent
FIXTURES_DIR = SCRIPT_DIR.parent / "fixtures" / "loaded-world"
FINGERPRINT_FILE = "source-fingerprint.txt"
MANIFEST_FILE = "manifest.json"
REGION_DIR_REL = "dimensions/minecraft/overworld/region"

# The default disposable source: the worktree copy, never the original save.
DEFAULT_SOURCE = SCRIPT_DIR.parents[2] / "working" / "client-worlds" / "New World"

# Known launcher save roots by platform. The original "New World" save is never
# a valid extraction source; resolve_source refuses it wherever it lives, not
# just on the macOS path this repo was developed on.
LAUNCHER_SAVES_DIRS = [
    Path.home() / "Library/Application Support/minecraft/saves",
    Path.home() / ".minecraft/saves",
]
if os.environ.get("APPDATA"):
    LAUNCHER_SAVES_DIRS.append(Path(os.environ["APPDATA"]) / ".minecraft" / "saves")


def resolve_source(source: Path) -> Path:
    """Resolve the disposable copy and refuse the original launcher save."""
    src = source.resolve()
    if not src.is_dir():
        raise SystemExit(
            f"UNVERIFIED: disposable world source not found at {src}\n"
            f"(run extraction on a machine that has working/client-worlds/New World)"
        )
    for saves_dir in LAUNCHER_SAVES_DIRS:
        if (saves_dir / "New World").resolve() == src:
            raise SystemExit(
                f"REFUSED: {src} is the original launcher save — #371 uses only the "
                f"disposable copy under working/client-worlds/ and never accesses the original"
            )
    return src


def fingerprint_tree(root: Path) -> list[tuple[str, str]]:
    """[(relative_path, sha256)] for every regular file under `root`, sorted by
    relative path. Symlinks are refused (a walk that follows a link could read
    outside the tree)."""
    out: list[tuple[str, str]] = []
    for dirpath, dirnames, filenames in os.walk(root):
        for name in list(dirnames):
            d = os.path.join(dirpath, name)
            if os.path.islink(d):
                raise SystemExit(f"REFUSED: symlink in source tree: {d}")
        for name in filenames:
            p = Path(dirpath) / name
            if p.is_symlink():
                raise SystemExit(f"REFUSED: symlink in source tree: {p}")
            rel = str(p.relative_to(root))
            out.append((rel, sha256_file(p)))
    out.sort(key=lambda t: t[0])
    return out


def fingerprint_lines(fp: list[tuple[str, str]]) -> str:
    return "".join(f"{rel}\t{h}\n" for rel, h in fp)


def parse_fingerprint(text: str) -> list[tuple[str, str]]:
    out = []
    for line in text.splitlines():
        line = line.rstrip("\n")
        if not line:
            continue
        rel, h = line.split("\t", 1)
        out.append((rel, h))
    return out


def region_file_for(region_dir: Path, wx: int, wz: int) -> tuple[Path, int, int]:
    rx, rz = wx // 32, wz // 32
    return region_dir / f"r.{rx}.{rz}.mca", rx, rz


def extract_corpus(source: Path) -> list[tuple[int, int, str, bytes]]:
    """Read the five corpus chunk payloads from the disposable source."""
    region_dir = source / REGION_DIR_REL
    if not region_dir.is_dir():
        raise SystemExit(f"UNVERIFIED: no overworld region dir under {source}")
    cache: dict[Path, dict[tuple[int, int], bytes]] = {}
    out = []
    for wx, wz, role in CORPUS:
        mca, rx, rz = region_file_for(region_dir, wx, wz)
        if not mca.is_file():
            raise SystemExit(
                f"UNVERIFIED: region file for chunk ({wx},{wz}) missing: {mca}"
            )
        if mca not in cache:
            cache[mca] = read_region_chunks(mca)
        lcx, lcz = wx - rx * 32, wz - rz * 32
        payload = cache[mca].get((lcx, lcz))
        if payload is None:
            raise SystemExit(
                f"UNVERIFIED: chunk ({wx},{wz}) absent from {mca.name}"
            )
        out.append((wx, wz, role, payload))
    return out


def chunk_path(wx: int, wz: int) -> str:
    return f"chunk/{wx}.{wz}.nbt"


def source_label(source: Path) -> str:
    """Human-readable provenance label for the manifest.

    The default disposable copy keeps its stable repo-relative label so the
    committed manifest stays portable across machines; any other `--source` is
    recorded verbatim (resolved) so provenance is never misstated."""
    if source.resolve() == DEFAULT_SOURCE.resolve():
        return "working/client-worlds/New World (disposable copy)"
    return str(source.resolve())


def build_manifest(captured: list[dict], source: Path) -> dict:
    return {
        "format": 1,
        "kind": "loaded-world",
        "paper": PAPER_PIN,
        "paper-commit": PAPER_COMMIT,
        "minecraft": MINECRAFT,
        "data-version": DATA_VERSION,
        "source": {
            "path": source_label(source),
            "fingerprint-file": FINGERPRINT_FILE,
            "launcher-world-mutated": False,
        },
        "chunk-count": len(captured),
        "captured": captured,
    }


# ---------------------------------------------------------------------------
# Static verification (also mirrored by the Rust `loaded_world_fixtures_verify`
# test): every captured file must exist with the recorded byte count and
# SHA-256.
# ---------------------------------------------------------------------------


def verify_fixtures_dir(fixtures_dir: Path, manifest: dict) -> list[str]:
    """Verify committed fixtures against their manifest. Returns error strings
    (empty = clean). Never mutates fixtures_dir."""
    errors: list[str] = []
    captured = manifest.get("captured", [])
    for cap in captured:
        file = fixtures_dir / cap["path"]
        if not file.is_file():
            errors.append(f"missing captured file {cap['path']}")
            continue
        data = file.read_bytes()
        if len(data) != cap["bytes"]:
            errors.append(
                f"captured file {cap['path']} size mismatch: manifest {cap['bytes']}, "
                f"on disk {len(data)}"
            )
        if sha256_bytes(data) != cap["sha256"]:
            errors.append(
                f"captured file {cap['path']} SHA-256 mismatch: expected {cap['sha256']}, "
                f"actual {sha256_bytes(data)}"
            )
    return errors


def load_committed_manifest(fixtures_dir: Path) -> dict:
    p = fixtures_dir / MANIFEST_FILE
    if not p.is_file():
        raise SystemExit(f"UNVERIFIED: committed manifest missing: {p}")
    return json.loads(p.read_text())


def diff_fingerprint(actual: list[tuple[str, str]], committed_lines: list[tuple[str, str]]) -> list[str]:
    """Diff a freshly computed source fingerprint against a committed baseline.
    Any diff means the source changed since capture (mutation). Returns error
    strings (empty = identical)."""
    if actual == committed_lines:
        return []
    actual_map = dict(actual)
    committed_map = dict(committed_lines)
    # Only the files whose hash actually changed (or that were added/removed)
    # belong in the detail, not every file in the union — an unchanged tree
    # would otherwise print all 39 entries with identical hashes.
    changed = sorted(
        rel
        for rel in set(actual_map) | set(committed_map)
        if committed_map.get(rel) != actual_map.get(rel)
    )
    detail = [
        f"  {rel}: committed {committed_map.get(rel, '<absent>')} actual {actual_map.get(rel, '<absent>')}"
        for rel in changed
    ]
    return [f"source fingerprint changed since capture:\n" + "\n".join(detail)]


# ---------------------------------------------------------------------------
# Modes
# ---------------------------------------------------------------------------


def cmd_extract(source: Path) -> int:
    """Extract the corpus into FIXTURES_DIR, fingerprinting the source before and
    after and asserting it never changed. Regenerates manifest + fingerprint."""
    src = resolve_source(source)
    fixtures_dir = FIXTURES_DIR
    before = fingerprint_tree(src)
    payloads = extract_corpus(src)
    after = fingerprint_tree(src)
    if before != after:
        raise SystemExit("FAIL: disposable source changed during extraction (mutation)")

    fixtures_dir.mkdir(parents=True, exist_ok=True)
    chunk_dir = fixtures_dir / "chunk"
    chunk_dir.mkdir(parents=True, exist_ok=True)
    captured = []
    for wx, wz, role, payload in payloads:
        rel = chunk_path(wx, wz)
        dst = fixtures_dir / rel
        dst.write_bytes(payload)
        captured.append(
            {
                "path": rel,
                "sha256": sha256_bytes(payload),
                "bytes": len(payload),
                "dim": "overworld",
                "region": f"{wx // 32}.{wz // 32}",
                "chunk": f"{wx}.{wz}",
                "role": role,
            }
        )
    captured.sort(key=lambda c: c["path"])

    manifest = build_manifest(captured, src)
    (fixtures_dir / MANIFEST_FILE).write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    )
    (fixtures_dir / FINGERPRINT_FILE).write_text(fingerprint_lines(before))

    # The sha256 self-check below catches write corruption/truncation (on-disk
    # bytes must hash to the in-memory payload the manifest was built from).
    # Content/role validation of the payloads is the Rust
    # loaded_world_fixtures_verify test's job, which parses them as NBT.
    errors = verify_fixtures_dir(fixtures_dir, manifest)
    if errors:
        for e in errors:
            print(f"  FAIL: {e}")
        return 1
    print(f"extracted {len(payloads)} corpus chunks into {fixtures_dir}")
    print(f"manifest: {fixtures_dir / MANIFEST_FILE}")
    print(f"source fingerprint ({len(before)} files) unchanged before/after: OK")
    return 0


def verify_re_extracted(
    payloads: list[tuple[int, int, str, bytes]], manifest: dict, fixtures_dir: Path
) -> list[str]:
    """Re-extracted payloads must be byte-identical to the committed fixtures
    (the dynamic half of --verify). Returns error strings (empty = clean)."""
    errors: list[str] = []
    by_coord = {(wx, wz): (role, payload) for wx, wz, role, payload in payloads}
    for cap in manifest.get("captured", []):
        wx, wz = cap["chunk"].split(".")
        wx, wz = int(wx), int(wz)
        role, payload = by_coord.get((wx, wz), (None, None))
        if payload is None:
            errors.append(f"chunk ({wx},{wz}) not re-extractable from source")
            continue
        committed = fixtures_dir / cap["path"]
        if not committed.is_file():
            errors.append(f"chunk ({wx},{wz}) committed fixture missing: {cap['path']}")
            continue
        if payload != committed.read_bytes():
            errors.append(
                f"chunk ({wx},{wz}) re-extracted bytes differ from committed fixture {cap['path']}"
            )
    return errors


def cmd_verify(source: Path) -> int:
    """Re-extract the corpus in memory and prove byte-identity with the
    committed fixtures, plus manifest hashes and the source fingerprint."""
    src = resolve_source(source)
    fixtures_dir = FIXTURES_DIR
    if not fixtures_dir.is_dir():
        raise SystemExit(f"UNVERIFIED: committed fixtures dir missing: {fixtures_dir}")
    manifest = load_committed_manifest(fixtures_dir)

    # Fingerprint the source once and reuse it for both the mutation guard and
    # the committed-baseline comparison.
    before = fingerprint_tree(src)
    payloads = extract_corpus(src)
    after = fingerprint_tree(src)
    if before != after:
        raise SystemExit("FAIL: disposable source changed during verification (mutation)")

    fp_path = fixtures_dir / FINGERPRINT_FILE
    if fp_path.is_file():
        fingerprint_errors = diff_fingerprint(before, parse_fingerprint(fp_path.read_text()))
    else:
        fingerprint_errors = [f"committed source fingerprint missing: {fp_path}"]

    errors: list[str] = []
    errors.extend(verify_re_extracted(payloads, manifest, fixtures_dir))
    errors.extend(fingerprint_errors)
    errors.extend(verify_fixtures_dir(fixtures_dir, manifest))

    if errors:
        for e in errors:
            print(f"  FAIL: {e}")
        return 1
    print("loaded-world fixtures byte-identical to the disposable source: OK")
    print(f"manifest hashes match: OK ({len(manifest['captured'])} files)")
    print(f"source fingerprint unchanged since capture: OK ({len(before)} files)")
    return 0


def cmd_expect_fail(source: Path) -> int:
    """Negative control: a missing and a tampered fixture must both be detected
    and named by static verification, the --verify re-extraction byte-compare
    must also flag the tamper set, and a tampered source fingerprint must be
    flagged and named by the live comparison. Operates on a dedicated scratch
    copy that THIS invocation creates and owns via TemporaryDirectory (always
    under the system temp namespace, cleaned up as the exact dir this run made)
    and in memory — never the committed fixtures and never the source tree. The
    committed fixtures are re-verified unchanged afterwards."""
    src = resolve_source(source)
    fixtures_dir = FIXTURES_DIR
    manifest = load_committed_manifest(fixtures_dir)

    with tempfile.TemporaryDirectory(prefix="rivet-loaded-world-control-") as td:
        scratch = Path(td)
        # Copy the fixture tree INTO the scratch dir this invocation owns (the
        # dir already exists, so copy contents, never rmtree anything).
        for entry in fixtures_dir.iterdir():
            if entry.is_dir():
                shutil.copytree(entry, scratch / entry.name)
            else:
                shutil.copy2(entry, scratch / entry.name)

        # Missing: delete one fixture file in the scratch copy.
        missing_rel = chunk_path(*CORPUS[1][:2])
        missing_path = scratch / missing_rel
        missing_path.unlink()
        # Tampered: flip a byte in another fixture file in the scratch copy.
        tamper_rel = chunk_path(*CORPUS[0][:2])
        tamper_path = scratch / tamper_rel
        data = bytearray(tamper_path.read_bytes())
        data[0] ^= 0xFF
        tamper_path.write_bytes(bytes(data))

        errors = verify_fixtures_dir(scratch, manifest)
        missing_named = any(f"missing captured file {missing_rel}" in e for e in errors)
        tamper_named = any("SHA-256 mismatch" in e and tamper_rel in e for e in errors)
        if not errors:
            raise SystemExit("FAIL: negative control passed — tampered fixtures were not detected")
        if not (missing_named and tamper_named):
            raise SystemExit(
                "FAIL: negative control did not name both the missing and the tampered fixture: "
                + "; ".join(errors)
            )
        print("negative control: missing fixture detected: OK")
        print("negative control: tampered fixture detected: OK")

        # Dynamic negative: the --verify re-extraction byte-compare (not just the
        # static hash check) must flag the scratch tamper set, so a bug that makes
        # the byte-compare always-equal is caught.
        payloads = extract_corpus(src)
        dyn_errors = verify_re_extracted(payloads, manifest, scratch)
        if not dyn_errors:
            raise SystemExit(
                "FAIL: negative control passed — re-extraction byte-compare did not detect "
                "the missing/tampered scratch fixtures"
            )
        print("negative control: re-extraction byte-compare detects tamper: OK")

    # The scratch dir this invocation created is already removed by
    # TemporaryDirectory's exit (only that exact dir is ever deleted).

    # Source-fingerprint negative: a tampered committed baseline must be flagged
    # by the live source comparison and the tampered file must be named (in
    # memory; never mutating the committed fingerprint file or the source).
    fp_path = fixtures_dir / FINGERPRINT_FILE
    if not fp_path.is_file():
        raise SystemExit(f"FAIL: committed source fingerprint missing: {fp_path}")
    committed_lines = parse_fingerprint(fp_path.read_text())
    if not committed_lines:
        raise SystemExit("FAIL: committed source fingerprint is empty")
    actual = fingerprint_tree(src)
    tampered_baseline = committed_lines[:]
    rel, h = tampered_baseline[0]
    tampered_baseline[0] = (rel, ("0" if h[0] != "0" else "1") + h[1:])
    fp_errors = diff_fingerprint(actual, tampered_baseline)
    if not fp_errors:
        raise SystemExit("FAIL: tampered source fingerprint was not detected")
    if not any(rel in e for e in fp_errors):
        raise SystemExit(
            "FAIL: tampered source fingerprint diff did not name the tampered file: "
            + "; ".join(fp_errors)
        )
    print("negative control: tampered source fingerprint detected: OK")

    # Post-condition: the committed fixtures must be byte-identical to what the
    # manifest records (self-verification that the negative control only ever
    # ran against its own scratch copy and left the corpus untouched).
    committed_errors = verify_fixtures_dir(fixtures_dir, manifest)
    if committed_errors:
        for e in committed_errors:
            print(f"  FAIL: committed fixtures changed: {e}")
        return 1
    print("negative control: committed fixtures unchanged after the run: OK")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--source",
        type=Path,
        default=DEFAULT_SOURCE,
        help=f"disposable world copy (default: {DEFAULT_SOURCE})",
    )
    ap.add_argument(
        "--verify",
        action="store_true",
        help="re-extract the corpus and prove byte-identity with the committed fixtures",
    )
    ap.add_argument(
        "--expect-fail",
        action="store_true",
        help="negative control: prove missing/tampered fixtures are detected "
        "(runs on its own TemporaryDirectory scratch copy)",
    )
    args = ap.parse_args()

    if args.expect_fail:
        return cmd_expect_fail(args.source)
    if args.verify:
        return cmd_verify(args.source)
    return cmd_extract(args.source)


if __name__ == "__main__":
    sys.exit(main())

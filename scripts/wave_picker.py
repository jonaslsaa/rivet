#!/usr/bin/env python3
"""Select ready units from MANIFEST.tsv for the next translate wave.

A unit is ready when:
  - its status is in --status (default: pending — not done, not blocked),
  - it is cycle-free (empty `cycle` column) unless --include-cycles,
  - it is not flagged needs_split=yes (splitting is a prerequisite) unless
    --include-needs-split,
  - every dep in its `deps` column is done.

Deps are resolved to units robustly, so the nbt class-cluster split never
self-depends even though its units share the `net.minecraft.nbt` java_package.
Each dep token is resolved to exactly one unit in this order:
  1. a unit whose `id` equals the token (exact unit-id dep),
  2. the unit whose id is the derived id of the token java package
     (net.minecraft.nbt -> mc.nbt, the core unit),
  3. the single unit whose `java_package` equals the token; if several share it
     (a split package), the lowest-wave one (deterministic tie-break by id).
A dep is done iff that unit's status is exactly `done`.

By default only the next wave (the lowest wave among ready units) is printed;
pass --all-waves to list every ready unit in wave order.

Usage:
  wave_picker.py [MANIFEST.tsv] [--status pending] [--max-units N]
                 [--all-waves] [--include-cycles] [--include-needs-split]
"""

import argparse
import csv
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO / "MANIFEST.tsv"
DEFAULT_STATUS = {"pending"}


def derive_id(pkg: str) -> str:
    return (
        pkg.replace("net.minecraft.", "mc.")
        .replace("org.bukkit.", "bukkit.")
        .replace("io.papermc.paper.", "paper.")
    )


def read_manifest(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as fh:
        rows = list(csv.DictReader(fh, delimiter="\t"))
    if not rows:
        sys.exit(f"no rows in {path}")
    return rows


def build_indexes(rows: list[dict[str, str]]) -> tuple[dict[str, dict], dict[str, list[dict]]]:
    """Return (by_id, by_package). by_id maps unit id -> row; by_package maps
    java_package -> rows (multiple when a package is split into units)."""
    by_id: dict[str, dict] = {}
    by_package: dict[str, list[dict]] = {}
    for r in rows:
        by_id[r["id"]] = r
        by_package.setdefault(r["java_package"], []).append(r)
    for lst in by_package.values():
        lst.sort(key=lambda r: (int(r["wave"]), r["id"]))
    return by_id, by_package


def resolve_dep(token: str, by_id: dict[str, dict], by_package: dict[str, list[dict]]) -> dict | None:
    """Resolve a dep token to exactly one unit, or None if unknown."""
    if token in by_id:
        return by_id[token]
    derived = derive_id(token)
    if derived in by_id:
        return by_id[derived]
    matches = by_package.get(token, [])
    if len(matches) == 1:
        return matches[0]
    if matches:
        # Shared java_package (nbt split): the core unit is the lowest-wave one.
        return matches[0]
    return None


def deps_done(dep_str: str, row: dict[str, str],
              by_id: dict[str, dict], by_package: dict[str, list[dict]]) -> bool:
    for token in (t.strip() for t in dep_str.split(",") if t.strip()):
        target = resolve_dep(token, by_id, by_package)
        if target is None:
            print(f"  WARN {row['id']}: unresolved dep '{token}'", file=sys.stderr)
            return False
        if target["status"] != "done":
            return False
    return True


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", nargs="?", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--status", default=",".join(sorted(DEFAULT_STATUS)),
                        help="comma-separated statuses eligible for picking (default: pending)")
    parser.add_argument("--max-units", type=int, default=30,
                        help="cap on units selected (default: 30)")
    parser.add_argument("--all-waves", action="store_true",
                        help="list every ready unit instead of just the next wave")
    parser.add_argument("--include-cycles", action="store_true",
                        help="also consider units with a non-empty cycle id")
    parser.add_argument("--include-needs-split", action="store_true",
                        help="also consider units flagged needs_split=yes")
    args = parser.parse_args()

    allowed = {s.strip() for s in args.status.split(",") if s.strip()}
    rows = read_manifest(args.manifest)
    by_id, by_package = build_indexes(rows)

    ready: list[dict[str, str]] = []
    for r in rows:
        if r["status"] not in allowed:
            continue
        if not args.include_cycles and r["cycle"]:
            continue
        if not args.include_needs_split and r["needs_split"]:
            continue
        if not deps_done(r["deps"], r, by_id, by_package):
            continue
        ready.append(r)

    if not ready:
        print("no ready units")
        return

    ready.sort(key=lambda r: (int(r["wave"]), r["id"]))
    if not args.all_waves:
        next_wave = int(ready[0]["wave"])
        ready = [r for r in ready if int(r["wave"]) == next_wave]

    selected = ready[: args.max_units]
    print(f"{len(selected)} ready unit(s) for the next wave"
          f" (of {len(ready)} ready in total):")
    for r in selected:
        print(f"  wave={r['wave']:>1}  {r['id']:<38} {r['crate']:<18} "
              f"cycle={r['cycle'] or '-':<4} deps={r['deps'] or '-'}")


if __name__ == "__main__":
    main()

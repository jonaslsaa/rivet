#!/usr/bin/env python3
"""Extract the M0 golden fixtures from a Paper server run.

The Minecraft world on disk is not byte-deterministic across boots: the
region-file framing (timestamp table, sector padding, trailing sectors) and
some world-level state change every run.  The *deterministic* layer is the
chunk NBT content: for a fixed seed + generator settings, the decompressed
chunk payloads come out byte-identical across boots (verified against the
26.2 Paper bundler on macOS).

This script therefore captures, from the spawn region r.0.0 of every
dimension:

  * each chunk as its *decompressed NBT payload* (`.nbt`), independent of
    region-file framing;
  * the raw `level.dat` / `level.dat_old` (gzip-NBT world metadata);
  * the exact `server.properties` used;
  * a `manifest.json` recording every captured file, its SHA-256, the seed,
    and the Paper/Java versions.

Output tree (relative to the fixtures dir)::

    manifest.json
    chunk/<dim>/0.0/<cx>.<cz>.nbt
    level.dat
    level.dat_old
    server.properties

The manifest is the source of truth for `rivet-oracle` (it verifies hashes).
Never hand-edit fixtures; regenerate from a clean run instead.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import struct
import sys
import zlib
from pathlib import Path

REGION_RE = re.compile(r"^r\.(-?\d+)\.(-?\d+)\.mca$")

# The deterministic slice: only this region is captured for M0.
SPAWN_REGION = (0, 0)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def read_region_chunks(path: Path) -> dict[tuple[int, int], bytes]:
    """Return {(local_x, local_z): decompressed NBT payload} for a region file."""
    data = path.read_bytes()
    chunks: dict[tuple[int, int], bytes] = {}
    for i in range(1024):
        val = struct.unpack(">I", data[i * 4 : i * 4 + 4])[0]
        off_sec = val >> 8
        if off_sec == 0:
            continue
        base = off_sec * 4096
        if base + 5 > len(data):
            continue
        length, comp = struct.unpack(">IB", data[base : base + 5])
        # The `length` field counts the compression-type byte, so the payload is
        # `length - 1` data bytes (5-byte chunk header: 4 length + 1 compression).
        data_bytes = length - 1
        if base + 5 + data_bytes > len(data):
            data_bytes = len(data) - base - 5
        raw = data[base + 5 : base + 5 + data_bytes]
        try:
            if comp == 1:
                import gzip

                payload = gzip.decompress(raw)
            elif comp == 2:
                # zlib-wrapped deflate (region-file-compression=deflate)
                payload = zlib.decompress(raw)
            elif comp == 3:
                payload = raw  # uncompressed
            else:
                continue  # lz4/zstd or unknown; skip (not used in M0 fixtures)
        except (zlib.error, OSError):
            continue  # partial/corrupt tail sector; not deterministic content
        cx, cz = (i % 32), (i // 32)
        chunks[(cx, cz)] = payload
    return chunks


def parse_server_properties(path: Path) -> dict[str, str]:
    props: dict[str, str] = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        props[k.strip()] = v.strip()
    return props


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("world_dir", type=Path, help="world dir of a completed server run")
    ap.add_argument(
        "out_dir", type=Path, nargs="?",
        default=Path(__file__).resolve().parent.parent / "fixtures",
        help="fixtures output dir (default: tools/rivet-oracle/fixtures)",
    )
    ap.add_argument(
        "--chunks-only", action="store_true",
        help="capture only the deterministic chunk-NBT payloads (skip level.dat / "
             "server.properties, which carry wall-clock timestamps). Used for the "
             "M2 normal-overworld region fixture so regeneration is git-clean.",
    )
    args = ap.parse_args()

    world: Path = args.world_dir
    out: Path = args.out_dir
    out.mkdir(parents=True, exist_ok=True)

    dims: dict[str, Path] = {}
    dims_base = world / "dimensions" / "minecraft"
    if dims_base.is_dir():
        for name in ("overworld", "the_nether", "the_end"):
            d = dims_base / name
            if d.is_dir():
                dims[name] = d
    else:
        dims["overworld"] = world  # pre-flattened layout

    captured: list[dict] = []
    chunk_count = 0

    # --- server.properties ---
    # Always parsed for the manifest provenance fields (seed / level-type /
    # region-file-compression). The M2 region fixtures are regenerated in-place;
    # level.dat carries wall-clock timestamps, so chunks-only captures skip the
    # level.dat copy and the server.properties copy (provenance lives in the
    # manifest + the committed fixtures/server-normal.properties) to stay
    # git-clean.
    props_src = world.parent / "server.properties"
    if props_src.is_file():
        props = parse_server_properties(props_src)
        if not args.chunks_only:
            dst = out / "server.properties"
            shutil.copyfile(props_src, dst)
            captured.append(
                {"path": "server.properties", "sha256": sha256_file(dst), "bytes": dst.stat().st_size}
            )
    else:
        props = {}

    # --- level.dat / level.dat_old (raw gzip-NBT world metadata) ---
    if not args.chunks_only:
        for fn in ("level.dat", "level.dat_old"):
            src = world / fn
            if src.is_file():
                dst = out / fn
                shutil.copyfile(src, dst)
                captured.append(
                    {"path": fn, "sha256": sha256_file(dst), "bytes": dst.stat().st_size}
                )

    # --- chunk NBT payloads from the spawn region of each dimension ---
    for dim_name, dim_dir in dims.items():
        region_dir = dim_dir / "region"
        if not region_dir.is_dir():
            continue
        rx, rz = SPAWN_REGION
        mca = region_dir / f"r.{rx}.{rz}.mca"
        if not mca.is_file():
            continue
        chunks = read_region_chunks(mca)
        rel_dir = out / "chunk" / dim_name / f"{rx}.{rz}"
        rel_dir.mkdir(parents=True, exist_ok=True)
        for (cx, cz), payload in sorted(chunks.items()):
            dst = rel_dir / f"{cx}.{cz}.nbt"
            dst.write_bytes(payload)
            captured.append(
                {
                    "path": str(dst.relative_to(out)),
                    "sha256": sha256_bytes(payload),
                    "bytes": len(payload),
                    "dim": dim_name,
                    "region": f"{rx}.{rz}",
                    "chunk": f"{cx}.{cz}",
                }
            )
            chunk_count += 1

    manifest = {
        "format": 1,
        "paper": "26.2-DEV-main@0a99345",
        "seed": props.get("level-seed"),
        "level-type": props.get("level-type"),
        "level-name": props.get("level-name"),
        "region-file-compression": props.get("region-file-compression", "deflate"),
        "server-properties": props,
        "spawn-region": f"{SPAWN_REGION[0]}.{SPAWN_REGION[1]}",
        "chunk-count": chunk_count,
        "captured": captured,
    }

    manifest_path = out / "manifest.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2, sort_keys=True)
        f.write("\n")

    total = sum(c["bytes"] for c in captured)
    print(f"captured {chunk_count} chunks across {len(dims)} dimension(s); {len(captured)} files; {total} bytes")
    print(f"fixtures written to {out}")
    print(f"manifest: {manifest_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

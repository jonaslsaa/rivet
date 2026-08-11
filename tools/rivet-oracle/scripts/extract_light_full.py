#!/usr/bin/env python3
"""Extract FULL Starlight light arrays from FULL-status chunk NBT.

The M0 golden fixtures (`fixtures/chunk/<dim>/0.0/<cx>.<cz>.nbt`) are captured
at `minecraft:full`, so they carry Starlight's complete light state: per-section
`starlight.skylight_state` / `starlight.blocklight_state` ints and the full
`SkyLight` / `BlockLight` 2048-byte nibble-packed arrays, plus the chunk-root
`starlight.light_version`.

The sampled `light.json` (extract_light_samples.py) keeps only 8 points per
section — enough for semantic parity but not for byte identity. This script is
the spike #229 / Phase B full-array oracle: it dumps the COMPLETE 2048-byte
arrays (base64, so the fixture stays JSON and the comparison is byte-exact)
together with the state ints and the light version, so Rivet can lock the
Starlight save-format byte identity before the engine is committed.

The output is deterministic: for the pinned Paper + seed the chunk payloads it
reads are themselves the deterministic M0 slice.

Usage:
  python3 scripts/extract_light_full.py <chunk-nbt-dir> <out.json>
  # chunk-nbt-dir defaults to fixtures/chunk; out.json to fixtures/worldgen/light-full.json
"""

from __future__ import annotations

import argparse
import base64
import json
import struct
import sys
from pathlib import Path

# --- minimal NBT reader (deterministic; the input is the committed M0 slice) ---


class Tag:
    def __init__(self, typ: int, val):
        self.typ = typ
        self.val = val


def parse_nbt(data: bytes) -> Tag:
    off = [0]

    def rd(n: int) -> bytes:
        v = data[off[0] : off[0] + n]
        off[0] += n
        return v

    def r_byte() -> int:
        return rd(1)[0]

    def r_str() -> str:
        n = struct.unpack(">H", rd(2))[0]
        return rd(n).decode()

    def read_t(t: int) -> Tag:
        if t == 1:
            return Tag(t, r_byte())
        if t == 2:
            return Tag(t, struct.unpack(">h", rd(2))[0])
        if t == 3:
            return Tag(t, struct.unpack(">i", rd(4))[0])
        if t == 4:
            return Tag(t, struct.unpack(">q", rd(8))[0])
        if t == 5:
            return Tag(t, struct.unpack(">f", rd(4))[0])
        if t == 6:
            return Tag(t, struct.unpack(">d", rd(8))[0])
        if t == 7:
            n = struct.unpack(">i", rd(4))[0]
            return Tag(t, rd(n))
        if t == 8:
            return Tag(t, r_str())
        if t == 9:
            et = r_byte()
            n = struct.unpack(">i", rd(4))[0]
            return Tag(t, [read_t(et) for _ in range(n)])
        if t == 10:
            d: dict[str, Tag] = {}
            while True:
                tt = r_byte()
                if tt == 0:
                    break
                name = r_str()
                d[name] = read_t(tt)
            return Tag(t, d)
        if t == 11:
            n = struct.unpack(">i", rd(4))[0]
            return Tag(t, list(struct.unpack(f">{n}i", rd(4 * n))))
        if t == 12:
            n = struct.unpack(">i", rd(4))[0]
            return Tag(t, list(struct.unpack(f">{n}q", rd(8 * n))))
        raise ValueError(f"unsupported tag type {t}")

    t0 = r_byte()
    if t0 != 0:
        r_str()  # root name
    return read_t(t0)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "chunk_dir", type=Path, nargs="?",
        default=Path(__file__).resolve().parent.parent / "fixtures" / "chunk",
    )
    ap.add_argument(
        "out_json", type=Path, nargs="?",
        default=Path(__file__).resolve().parent.parent / "fixtures" / "worldgen" / "light-full.json",
    )
    args = ap.parse_args()

    chunks: list[dict] = []
    version_seen = set()
    section_count = 0
    for dim_dir in sorted(args.chunk_dir.iterdir()):
        if not dim_dir.is_dir():
            continue
        dim = dim_dir.name
        for region_dir in sorted(dim_dir.iterdir()):
            for nbt in sorted(region_dir.glob("*.nbt")):
                root = parse_nbt(nbt.read_bytes())
                if root.typ != 10:
                    continue
                root_map = root.val
                status = root_map.get("Status").val if "Status" in root_map else None
                if status != "minecraft:full":
                    continue
                cx = root_map.get("xPos").val
                cz = root_map.get("zPos").val
                lv = root_map.get("starlight.light_version")
                version_seen.add(lv.val if lv else None)
                sections = root_map.get("sections")
                if sections is None:
                    continue
                entries = []
                for sec in sections.val:
                    if sec.typ != 10:
                        continue
                    m = sec.val
                    # Section Y is a signed TAG_Byte (overworld bottom is -4, not
                    # the unsigned 252 the byte's bits would read as).
                    y = m.get("Y").val
                    if y >= 128:
                        y -= 256
                    sl_state = m.get("starlight.skylight_state").val if "starlight.skylight_state" in m else None
                    bl_state = m.get("starlight.blocklight_state").val if "starlight.blocklight_state" in m else None
                    sl_arr = m["SkyLight"].val if "SkyLight" in m else None
                    bl_arr = m["BlockLight"].val if "BlockLight" in m else None
                    entry = {
                        "sectionY": y,
                        "skylight_state": sl_state,
                        "blocklight_state": bl_state,
                    }
                    if sl_arr is not None:
                        entry["skylight"] = base64.b64encode(bytes(sl_arr)).decode("ascii")
                    if bl_arr is not None:
                        entry["blocklight"] = base64.b64encode(bytes(bl_arr)).decode("ascii")
                    entries.append(entry)
                    section_count += 1
                if entries:
                    chunks.append({
                        "dim": dim,
                        "chunk": f"{cx}.{cz}",
                        "region": f"{region_dir.name}",
                        "light_version": lv.val if lv else None,
                        "sections": entries,
                    })

    out = {
        "format": 1,
        "paper": "26.2-DEV-main@0a99345",
        "source": "M0 FULL-status chunk NBT (fixtures/chunk) — complete Starlight arrays",
        # Numeric sort, not string sort: "10" must come after "9".
        "starlight.light_version": sorted(str(v) for v in sorted(version_seen) if v is not None),
        "array-encoding": "base64 of the raw 2048-byte nibble-packed light layer; nibble i in byte i>>1 (low nibble even, high nibble odd)",
        "chunks": chunks,
        "section-count": section_count,
    }
    args.out_json.parent.mkdir(parents=True, exist_ok=True)
    args.out_json.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print(f"extracted {section_count} light sections across {len(chunks)} chunks -> {args.out_json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

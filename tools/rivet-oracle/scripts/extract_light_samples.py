#!/usr/bin/env python3
"""Extract deterministic light-array samples from FULL-status chunk NBT.

The M0 golden fixtures (`fixtures/chunk/<dim>/0.0/<cx>.<cz>.nbt`) are captured
at `minecraft:full` (the M0 superflat run reaches full status without a
player), so they carry Starlight's light state: per-section
`starlight.skylight_state` / `starlight.blocklight_state` ints and the
`SkyLight` / `BlockLight` 2048-byte nibble-packed arrays (index =
x | (z<<4) | (y<<8); nibble = (data[i>>1] >>> ((i&1)<<2)) & 0xF), plus the
chunk-root `starlight.light_version`.

This script reads those chunk payloads and emits a *semantic* light sample —
positions + states + a few nibble values per section — into a single JSON, so
the Starlight wave (#184) and the M2 light-array acceptance row have a stable
fixture that does not depend on full NBT re-parsing. The output is
deterministic: for the pinned Paper + seed it is byte-identical across boots
(the chunk payloads it reads are themselves the deterministic M0 slice).

Normal-overworld light arrays are NOT captured here: a headless Paper boot
stops the normal overworld at pre-FULL status (structure_starts/carvers), and
full-status generation + Starlight is only driven by an in-world player (the
later #183/#184 chunk-pipeline wave). The samples.json provenance notes this.

Usage:
  python3 scripts/extract_light_samples.py <chunk-nbt-dir> <out.json>
  # chunk-nbt-dir defaults to fixtures/chunk; out.json to fixtures/worldgen/light.json
"""

from __future__ import annotations

import argparse
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


def nibble(data: bytes, i: int) -> int:
    return (data[i >> 1] >> ((i & 1) << 2)) & 0xF


# Light index within a 16x16x16 section: x | (z<<4) | (y<<8).
# Deterministic sample positions: 4 corners + 2 interior (x/z vary, y spans).
SAMPLE_YZ_XZ: list[tuple[int, int, int]] = [
    (0, 0, 0),
    (0, 15, 15),
    (4, 0, 0),
    (4, 15, 15),
    (8, 7, 7),
    (12, 0, 15),
    (12, 15, 0),
    (15, 8, 8),
]


def extract_light_payload(tag: Tag) -> bytes:
    return bytes(tag.val)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "chunk_dir", type=Path, nargs="?",
        default=Path(__file__).resolve().parent.parent / "fixtures" / "chunk",
    )
    ap.add_argument(
        "out_json", type=Path, nargs="?",
        default=Path(__file__).resolve().parent.parent / "fixtures" / "worldgen" / "light.json",
    )
    args = ap.parse_args()

    samples = []
    version_seen = set()
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
                    # Only FULL-status chunks carry the final light arrays.
                    continue
                cx = root_map.get("xPos").val
                cz = root_map.get("zPos").val
                lv = root_map.get("starlight.light_version")
                version_seen.add(lv.val if lv else None)
                sections = root_map.get("sections")
                if sections is None:
                    continue
                for sec in sections.val:
                    if sec.typ != 10:
                        continue
                    m = sec.val
                    # Section Y is a signed TAG_Byte (overworld bottom is -4, not
                    # the unsigned 252 the byte's bits would read as). The absolute
                    # y coordinate of a nibble is sectionY*16 + local-y, so the
                    # label would be off by 256 without the sign correction.
                    y = m.get("Y").val
                    if y >= 128:
                        y -= 256
                    sl_state = m.get("starlight.skylight_state").val if "starlight.skylight_state" in m else None
                    bl_state = m.get("starlight.blocklight_state").val if "starlight.blocklight_state" in m else None
                    sl_arr = extract_light_payload(m["SkyLight"]) if "SkyLight" in m else None
                    bl_arr = extract_light_payload(m["BlockLight"]) if "BlockLight" in m else None
                    # Only emit a sample when a section actually carries light
                    # arrays (superflat: above-sea sky sections, plus empty-air).
                    if sl_arr is None and bl_arr is None:
                        continue
                    entry = {
                        "dim": dim,
                        "chunk": f"{cx}.{cz}",
                        "region": f"{region_dir.name}",
                        "sectionY": y,
                        "skylight_state": sl_state,
                        "blocklight_state": bl_state,
                    }
                    if sl_arr is not None:
                        entry["skylight"] = {
                            f"{y+sy},{x},{z}": nibble(sl_arr, x | (z << 4) | (sy << 8))
                            for (sy, x, z) in SAMPLE_YZ_XZ
                        }
                    if bl_arr is not None:
                        entry["blocklight"] = {
                            f"{y+sy},{x},{z}": nibble(bl_arr, x | (z << 4) | (sy << 8))
                            for (sy, x, z) in SAMPLE_YZ_XZ
                        }
                    samples.append(entry)

    out = {
        "format": 1,
        "paper": "26.2-DEV-main@0a99345",
        "source": "M0 FULL-status superflat chunk NBT (fixtures/chunk) — Starlight arrays",
        "starlight.light_version": sorted(str(v) for v in version_seen if v is not None),
        "sample-position-schema": "per-section nibble at (x,z) corners + interior, y absolute within chunk",
        "samples": samples,
    }
    args.out_json.parent.mkdir(parents=True, exist_ok=True)
    args.out_json.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print(f"extracted light samples from {len(samples)} sections -> {args.out_json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

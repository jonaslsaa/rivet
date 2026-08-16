#!/usr/bin/env python3
"""Decode the canonical join capture's registry_data NBT element payloads into
a deterministic JSON fixture (issue #109 pre-baked full registry NBT).

Input : tools/rivet-capture/fixtures/join/capture.jsonl (the pinned #153/#194
        canonical join capture; packet id 7 = ClientboundRegistryDataPacket)
Output: data/registry_data.json — per synchronized registry, each element's
        id name and its NBT payload (base64, the exact `Tag` bytes Paper wrote:
        an unnamed tag, type byte + payload, as `writeAnyTag`).

Byte-faithful: the payload is the raw captured bytes between the element
id string and the next element, i.e. exactly what the `PackedRegistryEntry`
`data` `Optional<Tag>` carried. It is NOT re-encoded — a decode+encode that
disagrees would silently rewrite the wire bytes. The decoder here only walks
(validates structure) to find the payload boundaries.

Determinism: reads the capture in line order, iterates the packet bodies in
order, writes each registry's element list in wire (ascending registry id)
order. Regeneration is byte-idempotent.
"""
import base64
import hashlib
import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # tools/rivet-codegen -> repo root
CAPTURE = ROOT / "tools/rivet-capture/fixtures/join/capture.jsonl"
OUT = ROOT / "tools/rivet-codegen/data/registry_data.json"
MANIFEST = ROOT / "tools/rivet-codegen/data/registry_data.manifest.json"
# Byte-identity fixture for the runtime consumer test: registry key -> the
# packet body (`ClientboundRegistryDataPacket` stream codec output) exactly as
# the canonical capture recorded it for a client that accepted no known packs.
BODIES = ROOT / "crates/rivet-server/tests/fixtures/registry_data_capture.json"


def read_varint(b, i):
    val = 0
    shift = 0
    while True:
        x = b[i]; i += 1
        val |= (x & 0x7F) << shift
        shift += 7
        if not (x & 0x80):
            break
    return val, i


def read_string(b, i):
    n, i = read_varint(b, i)
    return b[i:i + n], i + n


def skip_value(b, i, t):
    # NBT type ids: 1 Byte, 2 Short, 3 Int, 4 Long, 5 Float, 6 Double,
    # 7 ByteArray, 8 String, 9 List, 10 Compound, 11 IntArray, 12 LongArray.
    if t == 0:
        return i
    if t == 1:
        return i + 1
    if t == 2:
        return i + 2
    if t in (3, 5):
        return i + 4
    if t in (4, 6):
        return i + 8
    if t in (7, 11, 12):
        unit = 1 if t == 7 else (4 if t == 11 else 8)
        ln = struct.unpack(">i", b[i:i + 4])[0]
        return i + 4 + ln * unit
    if t == 8:  # string
        ln = struct.unpack(">H", b[i:i + 2])[0]
        return i + 2 + ln
    if t == 9:  # list
        et = b[i]; i += 1
        cnt = struct.unpack(">i", b[i:i + 4])[0]; i += 4
        for _ in range(cnt):
            i = skip_value(b, i, et)
        return i
    if t == 10:  # compound
        while True:
            t2 = b[i]; i += 1
            if t2 == 0:
                return i
            ln = struct.unpack(">H", b[i:i + 2])[0]
            i += 2 + ln
            i = skip_value(b, i, t2)
    raise ValueError(f"bad tag id {t} at offset {i}")


def skip_tag(b, i):
    t = b[i]; i += 1
    return skip_value(b, i, t)


def main():
    registries = {}  # key -> {"elements": [{"id": str, "data_b64": str|None}]}
    order = []
    for line in CAPTURE.open(encoding="utf-8"):
        obj = json.loads(line)
        if (obj.get("state") != "configuration"
                or obj.get("direction") != "clientbound"
                or obj.get("id") != 7):
            continue
        body = bytes.fromhex(obj["body"])
        kl, i = read_varint(body, 0)
        key = body[i:i + kl].decode("utf-8"); i += kl
        cnt, i = read_varint(body, i)
        entries = []
        for _ in range(cnt):
            el, i = read_string(body, i)
            name = el.decode("utf-8")
            pres = body[i]; i += 1
            if pres == 1:
                start = i
                i = skip_tag(body, i)
                entries.append({"id": name, "data_b64": base64.b64encode(body[start:i]).decode("ascii")})
            else:
                entries.append({"id": name, "data_b64": None})
        if i != len(body):
            sys.exit(f"misaligned decode for {key}: consumed {i}/{len(body)}")
        if key in registries:
            sys.exit(f"duplicate registry_data packet for {key}")
        registries[key] = entries
        order.append(key)

    payload = {
        "generator": "decode_capture_nbt.py (canonical join capture registry_data packets, decoded via a byte-faithful NBT walker)",
        "minecraft_version": "26.2",
        "protocol_version": 776,
        "world_version": 4903,
        "capture_sha256": hashlib.sha256(CAPTURE.read_bytes()).hexdigest(),
        "registries": {k: registries[k] for k in order},
    }
    OUT.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    manifest = {
        "generator": "decode_capture_nbt.py",
        "source": {
            "jar": "tools/rivet-capture/fixtures/join/capture.jsonl (canonical join capture, clientbound configuration registry_data)",
            "jar_sha256": payload["capture_sha256"],
            "paper_git": "0a993450f129c4942c2a9ed45ba047412b4667cf",
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
        },
        "file": {
            "bytes": OUT.stat().st_size,
            "sha256": hashlib.sha256(OUT.read_bytes()).hexdigest(),
        },
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

    # Byte-identity fixture for the runtime consumer test: the full packet body
    # (registry key varint + entry count + each `PackedRegistryEntry`) per
    # registry, exactly as the capture recorded it. The consumer test encodes
    # `pack_registries(&[])` and asserts each packet body matches this file.
    bodies = {key: None for key in order}
    for line in CAPTURE.open(encoding="utf-8"):
        obj = json.loads(line)
        if (obj.get("state") != "configuration"
                or obj.get("direction") != "clientbound"
                or obj.get("id") != 7):
            continue
        body = bytes.fromhex(obj["body"])
        kl, i = read_varint(body, 0)
        key = body[i:i + kl].decode("utf-8")
        bodies[key] = obj["body"]
    if any(v is None for v in bodies.values()):
        sys.exit("some registry_data packet body was not found in the capture")
    BODIES.parent.mkdir(parents=True, exist_ok=True)
    BODIES.write_text(json.dumps(bodies, indent=2) + "\n", encoding="utf-8")

    total = sum(len(v) for v in registries.values())
    print(f"Wrote {len(registries)} registries / {total} elements -> {OUT}")
    print(f"Wrote {len(bodies)} registry_data capture bodies -> {BODIES}")


if __name__ == "__main__":
    main()

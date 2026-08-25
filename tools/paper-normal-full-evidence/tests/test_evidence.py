#!/usr/bin/env python3
import json
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HERE))

import capture  # noqa: E402
import validate  # noqa: E402
from nbt import NbtError, Tag, encode, parse  # noqa: E402


def packed_heightmap(offset: int) -> list[int]:
    words = []
    for word_index in range(37):
        word = 0
        for slot in range(7):
            index = word_index * 7 + slot
            value = (index % 32) + offset if index < 256 else 0
            word |= value << (slot * 9)
        words.append(word)
    return words


def valid_chunk(status: str = "minecraft:full", light: bool = True, height_len: int = 37) -> bytes:
    names = ["minecraft:air", "minecraft:stone", "minecraft:dirt", "minecraft:grass_block", "minecraft:water", "minecraft:sand"]
    palette = Tag(9, (10, [Tag(10, {"Name": Tag(8, name)}) for name in names]))
    section = Tag(10, {"block_states": Tag(10, {"palette": palette})})
    heightmaps = {
        "WORLD_SURFACE": Tag(12, packed_heightmap(0)[:height_len]),
        "MOTION_BLOCKING": Tag(12, packed_heightmap(1)[:height_len]),
        "OCEAN_FLOOR": Tag(12, packed_heightmap(2)[:height_len]),
    }
    root = Tag(10, {
        "Status": Tag(8, status),
        "isLightOn": Tag(1, int(light)),
        "Heightmaps": Tag(10, heightmaps),
        "sections": Tag(9, (10, [section])),
    })
    return encode(root)


class EvidenceTests(unittest.TestCase):
    def test_strict_nbt_rejects_trailing_and_duplicate_keys(self):
        with self.assertRaises(NbtError):
            parse(encode(Tag(10, {"x": Tag(3, 1)})) + b"trailing")
        duplicate = (
            b"\x0a\x00\x00"
            b"\x03\x00\x01x\x00\x00\x00\x01"
            b"\x03\x00\x01x\x00\x00\x00\x02"
            b"\x00"
        )
        with self.assertRaises(NbtError):
            parse(duplicate)

    def test_seed_signed_conversion_and_scheduler_order(self):
        self.assertEqual(capture.java_seed("12807505919197044144"), -5639238154512507472)
        closure = capture.scheduler_closure(capture.TARGETS, capture.RADIUS)
        self.assertEqual(len(closure), 2451)
        self.assertEqual(closure, sorted(set(closure)))
        self.assertEqual(closure[:3], [(-42, -42), (-42, -41), (-42, -40)])

    def test_full_light_and_heightmap_evidence(self):
        raw = valid_chunk()
        status, semantic, details = validate.validate_chunk(raw, (0, 0), target=True)
        self.assertEqual(status, "minecraft:full")
        self.assertRegex(semantic, r"^[0-9a-f]{64}$")
        self.assertTrue(details["light_correct"])
        self.assertEqual(details["heightmaps"], ["WORLD_SURFACE", "MOTION_BLOCKING", "OCEAN_FLOOR"])

    def test_negative_chunk_evidence_cases(self):
        with self.assertRaises(validate.Failed):
            validate.validate_chunk(valid_chunk(status="minecraft:biomes"), (0, 0), target=True)
        with self.assertRaises(validate.Failed):
            validate.validate_chunk(valid_chunk(light=False), (0, 0), target=True)
        with self.assertRaises(validate.Failed):
            validate.validate_chunk(valid_chunk(height_len=36), (0, 0), target=True)
        with self.assertRaises(validate.Failed):
            validate.validate_chunk(valid_chunk() + b"\x00", (0, 0), target=True)

    def test_missing_bundle_is_unverified(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [sys.executable, str(HERE / "validate.py"), str(Path(directory) / "missing")],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("UNVERIFIED", result.stdout)

    def test_copied_run_root_is_failed(self):
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "copy"
            run.mkdir()
            manifest = {
                "format": 1,
                "kind": json.loads((HERE / "fixtures/contract.json").read_text())["kind"],
                "producer": validate.PRODUCER,
                "parity_claim": None,
                "rivet_commit": None,
                "seed": validate.EXPECTED_SEEDS[0],
                "java_seed": str(capture.java_seed(validate.EXPECTED_SEEDS[0])),
                "attempt": 1,
                "paper_revision": validate.EXPECTED_PAPER,
                "java": {"major": 25, "vendor": "Eclipse Adoptium / Temurin"},
                "run_root": str((run / "different").resolve()),
                "run_id": run.name,
                "capture_token": "0" * 64,
            }
            (run / "capture.json").write_text(json.dumps(manifest))
            with self.assertRaises(validate.Failed):
                validate.validate_run(run, validate.EXPECTED_SEEDS[0], 1, json.loads((HERE / "fixtures/contract.json").read_text()))


if __name__ == "__main__":
    unittest.main()

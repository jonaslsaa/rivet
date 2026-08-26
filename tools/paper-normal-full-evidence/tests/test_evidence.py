#!/usr/bin/env python3
import json
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
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


def valid_chunk(status: str = "minecraft:full", light: bool = True, height_len: int = 37, flat_heightmaps: bool = False) -> bytes:
    names = ["minecraft:air", "minecraft:stone", "minecraft:dirt", "minecraft:grass_block", "minecraft:water", "minecraft:sand"]
    palette = Tag(9, (10, [Tag(10, {"Name": Tag(8, name)}) for name in names]))
    section = Tag(10, {"block_states": Tag(10, {"palette": palette})})
    offsets = (0, 0, 0) if flat_heightmaps else (0, 1, 2)
    heightmaps = {
        "WORLD_SURFACE": Tag(12, packed_heightmap(offsets[0])[:height_len]),
        "MOTION_BLOCKING": Tag(12, packed_heightmap(offsets[1])[:height_len]),
        "OCEAN_FLOOR": Tag(12, packed_heightmap(offsets[2])[:height_len]),
    }
    root = Tag(10, {
        "DataVersion": Tag(3, validate.EXPECTED_DATA_VERSION),
        "xPos": Tag(3, 0),
        "zPos": Tag(3, 0),
        "Status": Tag(8, status),
        "isLightOn": Tag(1, int(light)),
        "starlight.light_version": Tag(3, 10 if light else 9),
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

    def test_compressed_stream_requires_exact_end_marker(self):
        compressed = zlib.compress(b"abc")
        for malformed in (compressed[:-1], compressed + b"trailing"):
            with self.assertRaises(capture.Failed):
                capture._strict_decompress(malformed)
            with self.assertRaises(validate.Failed):
                validate.strict_decompress(malformed)

    def test_external_mcc_chunk_is_loaded_from_stub(self):
        with tempfile.TemporaryDirectory() as directory:
            region_dir = Path(directory)
            region = bytearray(3 * 4096)
            index = 3 + 4 * 32
            struct.pack_into(">I", region, index * 4, (2 << 8) | 1)
            struct.pack_into(">I", region, 2 * 4096, 1)
            region[2 * 4096 + 4] = 0x82  # external zlib/deflate stream
            path = region_dir / "r.1.-2.mca"
            path.write_bytes(region)
            (region_dir / "c.35.-60.mcc").write_bytes(zlib.compress(b"external-nbt"))
            self.assertEqual(capture.read_region(path, (1, -2))[(3, 4)], b"external-nbt")
            self.assertEqual(validate.read_region(path, (1, -2))[(3, 4)], b"external-nbt")

    def test_persisted_ticket_map_order_is_validated_as_a_set(self):
        closure = [(0, 0), (0, 1), (1, 0)]
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            injected = run / "provenance/chunk_tickets.dat"
            post = run / "world/dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat"
            injected.parent.mkdir(parents=True)
            post.parent.mkdir(parents=True)
            injected.write_bytes(capture.ticket_nbt(closure))
            post.write_bytes(capture.ticket_nbt([closure[1], closure[2], closure[0]]))
            manifest = {
                "ticket": {
                    "coordinates": [list(item) for item in closure],
                    "injected_path": "provenance/chunk_tickets.dat",
                    "injected_sha256": capture.sha256(injected.read_bytes()),
                    "post_exit_path": "world/dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat",
                    "post_exit_sha256": capture.sha256(post.read_bytes()),
                    "held_through_stop": True,
                }
            }
            validate._validate_tickets(run, closure, manifest)

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
        self.assertEqual(capture.chunk_details(raw, (0, 0), target=True)["heightmaps"], details["heightmaps"])
        _, _, support_details = validate.validate_chunk(valid_chunk(light=False), (0, 0), target=False)
        self.assertFalse(support_details["light_correct"])
        self.assertFalse(capture.chunk_details(valid_chunk(light=False), (0, 0), target=False)["light_correct"])
        self.assertEqual(validate.validate_chunk(valid_chunk(), (0, 0), target=False)[0], "minecraft:full")
        self.assertEqual(capture.chunk_details(valid_chunk(), (0, 0), target=False)["status"], "minecraft:full")
        self.assertEqual(validate.validate_chunk(valid_chunk(flat_heightmaps=True), (0, 0), target=True)[0], "minecraft:full")

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

    def test_nested_evidence_symlink_is_failed_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "bundle"
            root.mkdir()
            outside = Path(directory) / "outside"
            outside.mkdir()
            (root / "runs").symlink_to(outside, target_is_directory=True)
            with self.assertRaises(validate.Failed):
                validate._reject_symlinks_under(root, "evidence bundle")

    def test_bundle_rejects_malformed_run_entry_and_symlink_root(self):
        contract = json.loads((HERE / "fixtures/contract.json").read_text())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bundle = root / "bundle"
            bundle.mkdir()
            (bundle / "bundle.json").write_text(json.dumps({
                "format": 1,
                "kind": contract["kind"],
                "producer": validate.PRODUCER,
                "parity_claim": None,
                "rivet_commit": None,
                "paper_revision": validate.EXPECTED_PAPER,
                "contract_sha256": validate.sha256((HERE / "fixtures/contract.json").read_bytes()),
                "seeds": validate.EXPECTED_SEEDS,
                "targets": [list(item) for item in validate.EXPECTED_TARGETS],
                "closure_radius": validate.EXPECTED_RADIUS,
                "attempts_per_seed": 3,
                "runs": [{"seed": int(validate.EXPECTED_SEEDS[0]), "attempt": 1, "path": "runs/seed/1"}],
            }))
            with self.assertRaises(validate.Failed):
                validate.validate_bundle(bundle)
            link = root / "bundle-link"
            link.symlink_to(bundle, target_is_directory=True)
            result = subprocess.run(
                [sys.executable, str(HERE / "validate.py"), str(link)],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 3)
            self.assertIn("UNVERIFIED", result.stdout)

    def test_error_log_is_failed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "server.log"
            path.write_text("Done (1s)!\\n[ERROR] self-authored fallback\\n")
            with self.assertRaises(validate.Failed):
                validate._validate_log(path, "0" * 64, capture=False)

    def test_runtime_config_and_marker_order_are_fail_closed(self):
        seed = validate.EXPECTED_SEEDS[0]
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            capture.write_configs(run, seed, 24001, 24002)
            config = {
                "server_properties": capture.file_record(run / "provenance/server.properties", "provenance/server.properties"),
                "runtime_server_properties": capture.file_record(run / "server.properties", "server.properties"),
                "paper_global": capture.file_record(run / "provenance/config/paper-global.yml", "provenance/config/paper-global.yml"),
                "paper_world_defaults": capture.file_record(run / "provenance/config/paper-world-defaults.yml", "provenance/config/paper-world-defaults.yml"),
                "runtime_paper_global": capture.file_record(run / "config/paper-global.yml", "config/paper-global.yml"),
                "runtime_paper_world_defaults": capture.file_record(run / "config/paper-world-defaults.yml", "config/paper-world-defaults.yml"),
                "eula": capture.file_record(run / "provenance/eula.txt", "provenance/eula.txt"),
                "runtime_eula": capture.file_record(run / "eula.txt", "eula.txt"),
            }
            manifest = {
                "ports": {"configured_server": 24001, "configured_query": 24002},
                "config": config,
                "simulation": {"random_tick_speed": 0, "do_daylight_cycle": False, "do_weather_cycle": False, "do_mob_spawning": False, "spawn_limits": 0},
            }
            runtime_server = run / "server.properties"
            runtime_server.write_text(runtime_server.read_text().replace("level-type=minecraft:normal", "level-type=minecraft\\:normal") + "unknown-paper-default=true\\n")
            manifest["config"]["runtime_server_properties"] = capture.file_record(runtime_server, "server.properties")
            runtime_global = run / "config/paper-global.yml"
            runtime_global.write_text(runtime_global.read_text() + "extra-paper-default: true\\n")
            manifest["config"]["runtime_paper_global"] = capture.file_record(runtime_global, "config/paper-global.yml")
            validate._validate_config(run, seed, manifest)
            manifest["ports"]["configured_server"] = 0
            with self.assertRaises(validate.Failed):
                validate._validate_config(run, seed, manifest)
            manifest["ports"]["configured_server"] = 24001
            (run / "config/paper-global.yml").write_text("tampered\\n")
            manifest["config"]["runtime_paper_global"] = capture.file_record(run / "config/paper-global.yml", "config/paper-global.yml")
            with self.assertRaises(validate.Failed):
                validate._validate_config(run, seed, manifest)

            token = "a" * 64
            log = run / "ordered.log"
            log.write_text(
                "RIVET_SIMULATION_FROZEN randomTickSpeed=0\\n"
                "RIVET_PROBE_READY targets=8 closure=2451\\n"
                f"RIVET_CAPTURE_TOKEN={token}\\n"
                "Done (1s)!\\n"
                "[MoonriseCommon] Paper is using 1 worker threads, 1 I/O threads\\n"
                "Stopping server\\n"
                "All dimensions are saved\\n"
            )
            validate._validate_log(log, token, capture=True)
            log.write_text(log.read_text().replace(f"RIVET_CAPTURE_TOKEN={token}", "RIVET_CAPTURE_TOKEN=" + token + "\\nRIVET_PROBE_READY"))
            with self.assertRaises(validate.Failed):
                validate._validate_log(log, token, capture=True)

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

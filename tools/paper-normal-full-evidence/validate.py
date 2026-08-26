#!/usr/bin/env python3
"""Fail-closed validator for independent Paper normal-overworld FULL evidence.

Exit tri-state is intentional: 0 means VERIFIED Paper evidence, 1 means the
bundle is present but FAILED an evidence check, and 3 means UNVERIFIED because
no bundle/prerequisite evidence is available.  This validator never invokes
Rivet, never reads the active generated-full harness, and never makes a parity
claim.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import subprocess
import sys
import zlib
import zipfile
from pathlib import Path
from typing import Any

from nbt import NbtError, Tag, canonical_without_dynamic, get_any, parse

HERE = Path(__file__).resolve().parent
CONTRACT_PATH = HERE / "fixtures" / "contract.json"
PRODUCER = "paper-normal-full-capture/1"
PROBE = "PaperNormalFullProbe"
EXPECTED_PAPER = "0a993450f129c4942c2a9ed45ba047412b4667cf"
EXPECTED_PAPER_SHORT = EXPECTED_PAPER[:7]
EXPECTED_JAVA_MAJOR = 25
EXPECTED_SEEDS = [
    "5207638315753790570",
    "12807505919197044144",
    "5246862266665176429",
    "3423572188437197996",
]
EXPECTED_TARGETS = [(0, 0), (15, 15), (31, 31), (-1, -1), (-16, -16), (-31, -31), (-1, 0), (0, -1)]
EXPECTED_RADIUS = 11
EXPECTED_TICKET_LEVEL = 33
EXPECTED_TICKS_LEFT = -(1 << 63)
EXPECTED_DATA_VERSION = 4903
REGION_RE = re.compile(r"^r\.(-?\d+)\.(-?\d+)\.mca$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
TOKEN_RE = re.compile(r"^[0-9a-f]{64}$")


class Failed(ValueError):
    pass


class Unverified(ValueError):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def jar_manifest(path: Path) -> dict[str, str]:
    try:
        with zipfile.ZipFile(path) as archive:
            text = archive.read("META-INF/MANIFEST.MF").decode("utf-8", "replace")
    except (OSError, KeyError, zipfile.BadZipFile) as exc:
        raise Failed(f"Paper jar manifest is unreadable: {path}: {exc}") from exc
    result: dict[str, str] = {}
    for line in text.replace("\r\n", "\n").splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            result[key.strip()] = value.strip()
    return result


def java_seed(seed: str) -> int:
    value = int(seed)
    if not 0 <= value < 1 << 64:
        raise Failed(f"seed is not an unsigned Java-long representation: {seed}")
    return value - (1 << 64) if value >= 1 << 63 else value


def validate_paper_source(source_root: Path) -> None:
    if source_root.name != "Paper" or len(source_root.parts) < 2 or source_root.parts[-2:] != ("working", "Paper") or not source_root.is_dir() or source_root.is_symlink():
        raise Failed("Paper source provenance does not identify the pinned working/Paper tree")
    revision = subprocess.run(["git", "-C", str(source_root), "rev-parse", "HEAD"], capture_output=True, text=True, check=False)
    if revision.returncode != 0 or revision.stdout.strip() != EXPECTED_PAPER:
        raise Failed("Paper source tree is not at the pinned 26.2 revision")
    status = subprocess.run(["git", "-C", str(source_root), "status", "--porcelain"], capture_output=True, text=True, check=False)
    if status.returncode != 0 or status.stdout.strip():
        raise Failed("Paper source tree is dirty")


def closure(targets: list[tuple[int, int]], radius: int) -> list[tuple[int, int]]:
    if radius < 0:
        raise Failed("negative support radius")
    return sorted(
        {(x + dx, z + dz) for x, z in targets for dx in range(-radius, radius + 1) for dz in range(-radius, radius + 1)}
    )


def strict_decompress(data: bytes, *, gzip_stream: bool = False) -> bytes:
    obj = zlib.decompressobj(31 if gzip_stream else 15)
    try:
        raw = obj.decompress(data) + obj.flush()
    except zlib.error as exc:
        raise Failed(f"compressed payload is malformed: {exc}") from exc
    if not obj.eof:
        raise Failed("compressed payload ended before end-of-stream")
    if obj.unused_data or obj.unconsumed_tail:
        raise Failed("compressed payload has trailing or unconsumed bytes")
    return raw


def parse_properties(text: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            key = key.strip()
            if key in result:
                raise Failed(f"duplicate server.properties key: {key}")
            result[key] = value.strip()
    return result


def properties_text(seed: str) -> str:
    return (HERE / "fixtures" / "server.properties").read_text().replace("<seed>", str(java_seed(seed)))


def expected_properties(seed: str) -> dict[str, str]:
    fixture = parse_properties((HERE / "fixtures" / "server.properties").read_text())
    return {key: (str(java_seed(seed)) if value == "<seed>" else value) for key, value in fixture.items()}


def read_region(path: Path, region_coordinates: tuple[int, int]) -> dict[tuple[int, int], bytes]:
    data = path.read_bytes()
    if len(data) < 8192 or len(data) % 4096:
        raise Failed(f"malformed region framing: {path}")
    result: dict[tuple[int, int], bytes] = {}
    used: set[int] = {0, 1}
    for index in range(1024):
        location = struct.unpack_from(">I", data, index * 4)[0]
        sector, count = location >> 8, location & 0xFF
        if sector == 0:
            if count:
                raise Failed(f"empty region slot has nonzero sector count: {path}")
            continue
        if count == 0 or sector < 2 or sector + count > len(data) // 4096:
            raise Failed(f"region slot points outside file: {path}")
        if any(item in used for item in range(sector, sector + count)):
            raise Failed(f"overlapping region allocation: {path}")
        used.update(range(sector, sector + count))
        start = sector * 4096
        length = struct.unpack_from(">I", data, start)[0]
        compression = data[start + 4]
        if length < 1 or length > count * 4096 - 4:
            raise Failed(f"invalid chunk length in {path}")
        payload = data[start + 5 : start + 4 + length]
        coordinate = (index % 32, index // 32)
        global_coordinate = (region_coordinates[0] * 32 + coordinate[0], region_coordinates[1] * 32 + coordinate[1])
        external = compression & 0x80
        codec = compression & 0x7F
        if external:
            if length != 1:
                raise Failed(f"external region stub has unexpected length in {path}")
            external_path = path.parent / f"c.{global_coordinate[0]}.{global_coordinate[1]}.mcc"
            if not external_path.is_file() or external_path.is_symlink():
                raise Failed(f"external chunk payload is absent: {external_path}")
            payload = external_path.read_bytes()
        if codec == 1:
            raw = strict_decompress(payload, gzip_stream=True)
        elif codec == 2:
            raw = strict_decompress(payload)
        elif codec == 3:
            raw = payload
        else:
            raise Failed(f"unsupported chunk compression {codec} in {path}")
        result[coordinate] = raw
    return result


def chunks_from_world(world: Path) -> dict[tuple[int, int], bytes]:
    region_dir = world / "dimensions" / "minecraft" / "overworld" / "region"
    if not region_dir.is_dir() or region_dir.is_symlink():
        raise Failed(f"overworld region directory is absent or symlinked: {region_dir}")
    result: dict[tuple[int, int], bytes] = {}
    for path in sorted(region_dir.iterdir()):
        if path.name.endswith(".mcc"):
            continue
        if path.is_symlink():
            raise Failed(f"region file is a symlink: {path}")
        match = REGION_RE.fullmatch(path.name)
        if not match:
            raise Failed(f"unexpected file in region directory: {path.name}")
        rx, rz = int(match.group(1)), int(match.group(2))
        for (lx, lz), raw in read_region(path, (rx, rz)).items():
            coordinate = (rx * 32 + lx, rz * 32 + lz)
            if coordinate in result:
                raise Failed(f"duplicate global chunk {coordinate}")
            result[coordinate] = raw
    return result


def _status(root: Tag) -> str:
    tag = get_any(root, "Status")
    if tag is None or tag.kind != 8:
        raise Failed("chunk has no string Status")
    return tag.value


def _heightmaps(root: Tag, coordinate: tuple[int, int], *, target: bool) -> tuple[dict[str, list[int]], list[str]]:
    tag = get_any(root, "Heightmaps")
    if tag is None or tag.kind != 10:
        raise Failed(f"{coordinate} has no Heightmaps compound")
    required = ("WORLD_SURFACE", "MOTION_BLOCKING", "OCEAN_FLOOR")
    decoded: dict[str, list[int]] = {}
    for name in required:
        item = tag.value.get(name)
        if item is None or item.kind != 12 or len(item.value) != 37:
            raise Failed(f"{coordinate} has malformed {name} heightmap")
        values: list[int] = []
        for word in item.value:
            for slot in range(7):
                values.append((word >> (slot * 9)) & 0x1FF)
        values = values[:256]
        if len(values) != 256 or any(value > 384 for value in values):
            raise Failed(f"{coordinate} has out-of-range {name} heightmap")
        decoded[name] = values
    if target and len({tuple(values) for values in decoded.values()}) < 2:
        raise Failed(f"{coordinate} has identical required heightmaps")
    return decoded, list(required)


def _block_palette_names(root: Tag, coordinate: tuple[int, int], *, target: bool) -> set[str]:
    names: set[str] = set()
    sections = get_any(root, "sections")
    if sections is None or sections.kind != 9:
        raise Failed(f"{coordinate} has no sections list")
    for section in sections.value[1]:
        if section.kind != 10:
            raise Failed(f"{coordinate} section list contains non-compounds")
        states = section.value.get("block_states")
        if states is None or states.kind != 10:
            continue
        palette = states.value.get("palette")
        if palette is None or palette.kind != 9:
            continue
        for entry in palette.value[1]:
            if entry.kind == 10:
                name = entry.value.get("Name")
                if name is not None and name.kind == 8:
                    names.add(name.value)
    if target and len(names) < 6:
        raise Failed(f"{coordinate} has a flat/under-varied block palette")
    return names


def validate_chunk(raw: bytes, coordinate: tuple[int, int], *, target: bool) -> tuple[str, str, dict[str, Any]]:
    try:
        root = parse(raw)
    except NbtError as exc:
        raise Failed(f"malformed/trailing NBT at {coordinate}: {exc}") from exc
    if root.kind != 10 or root.value.get("DataVersion") != Tag(3, EXPECTED_DATA_VERSION) or root.value.get("xPos") != Tag(3, coordinate[0]) or root.value.get("zPos") != Tag(3, coordinate[1]):
        raise Failed(f"{coordinate} has wrong chunk DataVersion or coordinates")
    status = _status(root)
    if status != "minecraft:full":
        raise Failed(f"{coordinate} is {status}, not minecraft:full")
    light = get_any(root, "isLightOn")
    if light is None or light.kind != 1:
        raise Failed(f"{coordinate} has no isLightOn byte")
    light_correct = bool(light.value)
    if target and not light_correct:
        raise Failed(f"{coordinate} is not light-correct")
    heightmaps, heightmap_names = _heightmaps(root, coordinate, target=target)
    names = _block_palette_names(root, coordinate, target=target)
    if target and len(names) < 6:
        raise Failed(f"{coordinate} looks flat")
    semantic = sha256(canonical_without_dynamic(root))
    details = {
        "status": status,
        "light_correct": light_correct,
        "heightmaps": heightmap_names,
        "heightmap_ranges": {name: [min(values), max(values)] for name, values in heightmaps.items()},
        "palette_names": sorted(names),
        "raw_sha256": sha256(raw),
        "raw_bytes": len(raw),
        "semantic_sha256": semantic,
    }
    return status, semantic, details


def inventory_paths(world: Path) -> set[str]:
    roots = [
        world / "dimensions" / "minecraft" / "overworld" / "region",
        world / "dimensions" / "minecraft" / "overworld" / "poi",
        world / "dimensions" / "minecraft" / "overworld" / "entities",
    ]
    result: set[str] = set()
    for root in roots:
        if root.is_dir():
            if root.is_symlink():
                raise Failed(f"inventory root is a symlink: {root}")
            for path in root.rglob("*"):
                if path.is_symlink() or (path.exists() and not path.is_file()):
                    raise Failed(f"inventory path is not a regular file: {path}")
                if path.is_file():
                    result.add(str(path.relative_to(world)))
    for path in world.rglob("*.mcc"):
        if path.is_symlink() or not path.is_file():
            raise Failed(f"inventory .mcc is not a regular file: {path}")
        result.add(str(path.relative_to(world)))
    return result


def _manifest_entry(manifest: dict[str, Any], rel: str) -> dict[str, Any]:
    for entry in manifest.get("inventory", []):
        if entry.get("path") == rel:
            return entry
    raise Failed(f"inventory omitted {rel}")


def validate_inventory(run: Path, manifest: dict[str, Any]) -> None:
    world = run / "world"
    actual = inventory_paths(world)
    listed = {entry.get("path") for entry in manifest.get("inventory", [])}
    if None in listed or actual != listed:
        raise Failed(f"POI/entity/region/.mcc inventory mismatch (actual={len(actual)} listed={len(listed)})")
    for rel in sorted(actual):
        path = world / rel
        if path.is_symlink() or not path.is_file():
            raise Failed(f"inventory path is not a regular file: {rel}")
        digest = sha256(path.read_bytes())
        entry = _manifest_entry(manifest, rel)
        if entry.get("sha256") != digest or entry.get("bytes") != path.stat().st_size:
            raise Failed(f"inventory hash mismatch: {rel}")
        if entry.get("kind") not in {"region", "poi", "entities", "mcc", "other"}:
            raise Failed(f"inventory kind is unknown: {rel}")


def _read_json(path: Path, label: str) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise Failed(f"{label} is absent or symlinked")
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise Failed(f"{label} is malformed: {exc}") from exc
    if not isinstance(value, dict):
        raise Failed(f"{label} is not an object")
    return value


def _validate_worldgen_settings(run: Path, expected_seed: str, manifest: dict[str, Any]) -> None:
    source = run / "world/dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat"
    contract_copy = run / "world/data/minecraft/worldgen_settings.dat"
    if source.is_symlink() or contract_copy.is_symlink() or not source.is_file() or not contract_copy.is_file():
        raise Failed("exact Paper worldgen_settings.dat capture is absent or symlinked")
    source_bytes = source.read_bytes()
    if contract_copy.read_bytes() != source_bytes:
        raise Failed("worldgen settings contract copy differs from Paper source")
    recorded = manifest.get("worldgen_settings", {})
    if recorded.get("path") != "world/data/minecraft/worldgen_settings.dat" or recorded.get("source_path") != "world/dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat":
        raise Failed("worldgen settings provenance paths are wrong")
    if (
        recorded.get("sha256") != sha256(source_bytes)
        or recorded.get("source_sha256") != sha256(source_bytes)
        or recorded.get("bytes") != len(source_bytes)
        or recorded.get("source_bytes") != len(source_bytes)
    ):
        raise Failed("worldgen settings hash/size provenance mismatch")
    if recorded.get("data_version") != EXPECTED_DATA_VERSION or recorded.get("seed") != str(java_seed(expected_seed)) or recorded.get("generator") != "minecraft:noise" or recorded.get("generate_structures") is not True:
        raise Failed("worldgen settings metadata provenance is incomplete or wrong")
    try:
        root = parse(strict_decompress(source_bytes, gzip_stream=True))
    except NbtError as exc:
        raise Failed(f"worldgen settings NBT is malformed/trailing: {exc}") from exc
    data = root.value.get("data") if root.kind == 10 else None
    if data is None or data.kind != 10:
        raise Failed("worldgen settings has no data compound")
    if root.value.get("DataVersion") != Tag(3, EXPECTED_DATA_VERSION):
        raise Failed("worldgen settings DataVersion is not 4903")
    if data.value.get("seed") != Tag(4, java_seed(expected_seed)):
        raise Failed("worldgen settings seed is wrong")
    if data.value.get("generate_structures") != Tag(1, 1):
        raise Failed("worldgen settings generate_structures is not true")
    dimensions = data.value.get("dimensions")
    overworld = dimensions.value.get("minecraft:overworld") if dimensions and dimensions.kind == 10 else None
    generator = overworld.value.get("generator") if overworld and overworld.kind == 10 else None
    if overworld is None or generator is None or generator.kind != 10 or generator.value.get("type") != Tag(8, "minecraft:noise"):
        raise Failed("worldgen settings route is flat or not normal-overworld noise")


def _validate_log(path: Path, token: str, *, capture: bool) -> None:
    if not path.is_file() or path.is_symlink():
        raise Failed(f"Paper log is absent or symlinked: {path}")
    text = path.read_text(errors="replace")
    bad = [line for line in text.splitlines() if re.search(r"\[(ERROR|FATAL)\]|Exception|StackTrace|fallback|recovery|RIVET_PROBE_FAILED", line, re.I)]
    if bad:
        raise Failed(f"Paper log contains an error/fallback/recovery line: {bad[0]}")
    required = ["Done (", "All dimensions are saved", "Stopping server", "[MoonriseCommon] Paper is using 1 worker threads, 1 I/O threads"]
    if capture:
        required += ["RIVET_PROBE_READY", "RIVET_CAPTURE_TOKEN=" + token, "RIVET_SIMULATION_FROZEN"]
    if any(marker not in text for marker in required):
        raise Failed(f"Paper log is missing a required {'capture' if capture else 'create'} marker")

    def unique_marker(marker: str) -> int:
        positions = [match.start() for match in re.finditer(re.escape(marker), text)]
        if len(positions) != 1:
            raise Failed(f"Paper log marker is missing or duplicated: {marker}")
        return positions[0]

    done_at = unique_marker("Done (")
    stopping_at = unique_marker("Stopping server")
    saved_positions = [match.start() for match in re.finditer(re.escape("All dimensions are saved"), text)]
    if not saved_positions:
        raise Failed("Paper log marker is missing: All dimensions are saved")
    saved_at = saved_positions[-1]
    if not (done_at < stopping_at < saved_at):
        raise Failed("Paper graceful-stop markers are out of order")
    if capture:
        ready_at = unique_marker("RIVET_PROBE_READY")
        token_at = unique_marker("RIVET_CAPTURE_TOKEN=" + token)
        frozen_at = unique_marker("RIVET_SIMULATION_FROZEN")
        if not (done_at < frozen_at < ready_at <= token_at < stopping_at):
            raise Failed("Paper simulation/probe-ready/token markers are not ordered before graceful stop")


def _validate_config(run: Path, seed: str, manifest: dict[str, Any]) -> None:
    expected = expected_properties(seed)
    provenance = run / "provenance/server.properties"
    actual = run / "server.properties"
    if provenance.is_symlink() or actual.is_symlink() or not provenance.is_file() or not actual.is_file():
        raise Failed("server.properties provenance is absent or symlinked")
    if provenance.read_text() != properties_text(seed):
        raise Failed("server.properties provenance differs from pinned fixture")
    actual_properties = parse_properties(actual.read_text())
    if set(actual_properties) != set(expected):
        raise Failed("runtime server.properties keys differ from pinned normal-overworld config")
    ports = manifest.get("ports", {})
    configured_server = ports.get("configured_server")
    configured_query = ports.get("configured_query")
    if not isinstance(configured_server, int) or configured_server <= 0:
        raise Failed("Paper server port is absent or zero")
    if not isinstance(configured_query, int) or configured_query <= 0 or configured_query == configured_server:
        raise Failed("Paper query port is absent, zero, or collides with the server port")
    for key, value in expected.items():
        if key == "server-port":
            expected_value = str(configured_server)
        elif key == "query.port":
            expected_value = str(configured_query)
        else:
            expected_value = value
        if actual_properties.get(key) != expected_value:
            raise Failed("runtime server.properties route differs from pinned normal-overworld config")
    config = manifest.get("config", {})
    records = {
        "server_properties": (provenance, "provenance/server.properties"),
        "runtime_server_properties": (actual, "server.properties"),
        "eula": (run / "provenance/eula.txt", "provenance/eula.txt"),
        "runtime_eula": (run / "eula.txt", "eula.txt"),
    }
    for key, (path, relative) in records.items():
        record = config.get(key, {})
        if path.is_symlink() or not path.is_file() or record.get("path") != relative or record.get("sha256") != sha256(path.read_bytes()) or record.get("bytes") != path.stat().st_size:
            raise Failed(f"config provenance is absent or tampered: {relative}")
    if (run / "provenance/eula.txt").read_bytes() != b"eula=true\n" or (run / "eula.txt").read_bytes() != b"eula=true\n":
        raise Failed("runtime or provenance eula is not pinned")
    for key, relative, fixture in (
        ("paper_global", "provenance/config/paper-global.yml", "paper-global.yml"),
        ("paper_world_defaults", "provenance/config/paper-world-defaults.yml", "paper-world-defaults.yml"),
        ("runtime_paper_global", "config/paper-global.yml", "paper-global.yml"),
        ("runtime_paper_world_defaults", "config/paper-world-defaults.yml", "paper-world-defaults.yml"),
    ):
        path = run / relative
        record = config.get(key, {})
        fixture_bytes = (HERE / "fixtures" / fixture).read_bytes()
        if path.is_symlink() or not path.is_file() or path.read_bytes() != fixture_bytes:
            raise Failed(f"pinned runtime/provenance config differs: {relative}")
        if record.get("path") != relative or record.get("sha256") != sha256(fixture_bytes) or record.get("bytes") != len(fixture_bytes):
            raise Failed(f"config manifest provenance is absent or tampered: {relative}")
    simulation = manifest.get("simulation")
    if simulation != {"random_tick_speed": 0, "do_daylight_cycle": False, "do_weather_cycle": False, "do_mob_spawning": False, "spawn_limits": 0}:
        raise Failed("simulation is not frozen by the pinned contract")


def _validate_tickets(run: Path, expected_closure: list[tuple[int, int]], manifest: dict[str, Any]) -> None:
    path = run / "world/dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat"
    if path.is_symlink() or not path.is_file():
        raise Failed("post-stop forced tickets are absent")
    try:
        root = parse(strict_decompress(path.read_bytes(), gzip_stream=True))
    except NbtError as exc:
        raise Failed(f"chunk_tickets.dat is malformed/trailing: {exc}") from exc
    if root.kind != 10 or root.value.get("DataVersion") != Tag(3, EXPECTED_DATA_VERSION):
        raise Failed("chunk_tickets.dat DataVersion is not 4903")
    data = root.value.get("data") if root.kind == 10 else None
    tickets = data.value.get("tickets") if data and data.kind == 10 else None
    if tickets is None or tickets.kind != 9 or tickets.value[0] != 10:
        raise Failed("chunk_tickets.dat has no compound data.tickets list")
    coordinates: list[tuple[int, int]] = []
    for ticket in tickets.value[1]:
        if ticket.kind != 10:
            raise Failed("ticket list contains a non-compound")
        values = ticket.value
        if values.get("type") != Tag(8, "minecraft:forced") or values.get("level") != Tag(3, EXPECTED_TICKET_LEVEL) or values.get("ticks_left") != Tag(4, EXPECTED_TICKS_LEFT):
            raise Failed("ticket is not the exact held level-33 forced ticket")
        position = values.get("chunk_pos")
        if position is None or position.kind != 11 or len(position.value) != 2:
            raise Failed("ticket chunk_pos is malformed")
        coordinates.append((position.value[0], position.value[1]))
    if coordinates != expected_closure or manifest.get("ticket", {}).get("coordinates") != [list(item) for item in expected_closure]:
        raise Failed("forced ticket set/order is not the exact scheduler closure")
    digest = sha256(path.read_bytes())
    ticket_manifest = manifest.get("ticket", {})
    if (
        ticket_manifest.get("injected_sha256") != digest
        or ticket_manifest.get("post_exit_sha256") != digest
        or ticket_manifest.get("held_through_stop") is not True
    ):
        raise Failed("ticket lifecycle/hash provenance is incomplete")


def _validate_probe(run: Path, token: str, expected_closure: list[tuple[int, int]]) -> None:
    probe = _read_json(run / "probe.json", "probe.json")
    if probe.get("format") != 1 or probe.get("producer") != PROBE or probe.get("main_thread") is not True or probe.get("world") != "minecraft:overworld":
        raise Failed("main-thread Paper probe provenance is missing")
    if probe.get("token") != token or probe.get("closure_count") != len(expected_closure) or probe.get("simulation_frozen") is not True:
        raise Failed("probe closure/token/simulation evidence is incomplete")
    targets = [(item.get("x"), item.get("z")) for item in probe.get("targets", [])]
    if targets != EXPECTED_TARGETS:
        raise Failed("probe target order differs")
    for item in probe["targets"]:
        if item.get("status") != "minecraft:full" or item.get("light_correct") is not True:
            raise Failed("probe target did not prove FULL+light")


def validate_run(run: Path, expected_seed: str, expected_attempt: int, contract: dict[str, Any]) -> dict[tuple[int, int], str]:
    if not run.is_dir() or run.is_symlink():
        raise Failed(f"run root is not a real isolated directory: {run}")
    manifest = _read_json(run / "capture.json", "capture manifest")
    if manifest.get("format") != 1 or manifest.get("kind") != contract["kind"] or manifest.get("producer") != PRODUCER:
        raise Failed("wrong producer, kind, or manifest format")
    if manifest.get("parity_claim") is not None or manifest.get("rivet_commit") is not None:
        raise Failed("evidence contains a parity claim or Rivet commit")
    if manifest.get("seed") != expected_seed or manifest.get("java_seed") != str(java_seed(expected_seed)) or manifest.get("attempt") != expected_attempt:
        raise Failed("wrong seed or attempt")
    if manifest.get("paper_revision") != EXPECTED_PAPER:
        raise Failed("wrong or stale Paper revision")
    if manifest.get("java", {}).get("major") != EXPECTED_JAVA_MAJOR or "Temurin" not in manifest.get("java", {}).get("vendor", ""):
        raise Failed("Java provenance is not explicit Temurin 25")
    paper_jar = manifest.get("paper_jar", {})
    source_root = Path(paper_jar.get("source_root", ""))
    source_jar = Path(paper_jar.get("path", ""))
    if paper_jar.get("source_revision") != EXPECTED_PAPER or not isinstance(paper_jar.get("built_after_ns"), int) or paper_jar.get("built_after_ns") <= 0:
        raise Failed("Paper source/build provenance is not the pinned fresh source")
    validate_paper_source(source_root)
    if source_jar.name != "paper-paperclip-26.2.local-SNAPSHOT.jar" or not source_jar.is_file() or source_jar.is_symlink() or not source_jar.resolve().is_relative_to(source_root.resolve()):
        raise Failed("built Paperclip jar is outside the pinned Paper source tree")
    if source_jar.stat().st_mtime_ns < int(paper_jar["built_after_ns"]) or paper_jar.get("sha256") != sha256(source_jar.read_bytes()) or paper_jar.get("bytes") != source_jar.stat().st_size:
        raise Failed("built Paperclip jar provenance is absent, stale, or tampered")
    if jar_manifest(source_jar).get("Main-Class") != "io.papermc.paperclip.Main":
        raise Failed("built Paperclip jar is not the pinned Paperclip launcher")
    source_server = paper_jar.get("source_server_jar", {})
    server_path = Path(source_server.get("path", ""))
    if server_path.name != "paper-server-26.2.local-SNAPSHOT.jar" or not server_path.is_file() or server_path.is_symlink() or not server_path.resolve().is_relative_to(source_root.resolve()):
        raise Failed("built Paper server jar is outside the pinned Paper source tree")
    if (
        server_path.stat().st_mtime_ns < int(paper_jar["built_after_ns"])
        or source_server.get("git_commit") != EXPECTED_PAPER_SHORT
        or source_server.get("sha256") != sha256(server_path.read_bytes())
        or source_server.get("bytes") != server_path.stat().st_size
        or jar_manifest(server_path).get("Git-Commit") != EXPECTED_PAPER_SHORT
    ):
        raise Failed("built Paper server jar does not prove the pinned source")
    runtime = paper_jar.get("materialized_runtime", {})
    runtime_relative = runtime.get("path")
    if runtime_relative != "versions/26.2/paper-26.2.jar":
        raise Failed("fresh materialized Paper runtime path is not pinned")
    runtime_path = run / runtime_relative
    if runtime_path.is_symlink() or not runtime_path.is_file() or runtime.get("git_commit") != EXPECTED_PAPER_SHORT or runtime.get("sha256") != sha256(runtime_path.read_bytes()) or runtime.get("bytes") != runtime_path.stat().st_size:
        raise Failed("fresh materialized Paper runtime provenance is absent, stale, or tampered")
    if jar_manifest(runtime_path).get("Git-Commit") != EXPECTED_PAPER_SHORT:
        raise Failed("fresh materialized Paper runtime does not prove the pinned source")
    probe_artifact = manifest.get("probe_artifact", {})
    probe_relative = probe_artifact.get("path")
    if probe_relative != "plugins/RivetPaperNormalFullProbe.jar":
        raise Failed("compiled main-thread probe path is not pinned")
    probe_path = run / probe_relative
    probe_source = HERE / "src/PaperNormalFullProbe.java"
    plugin_yml = HERE / "src/plugin.yml"
    if probe_path.is_symlink() or not probe_path.is_file() or probe_artifact.get("sha256") != sha256(probe_path.read_bytes()) or probe_artifact.get("bytes") != probe_path.stat().st_size or paper_jar.get("probe_source_sha256") != sha256(probe_source.read_bytes()) or paper_jar.get("probe_plugin_yml_sha256") != sha256(plugin_yml.read_bytes()):
        raise Failed("compiled main-thread probe provenance is absent or tampered")
    try:
        with zipfile.ZipFile(probe_path) as archive:
            if archive.read("plugin.yml") != plugin_yml.read_bytes() or "org/rivet/paper_normal_full/PaperNormalFullProbe.class" not in archive.namelist():
                raise Failed("compiled main-thread probe jar does not contain the pinned source and entrypoint")
    except (KeyError, zipfile.BadZipFile) as exc:
        raise Failed("compiled main-thread probe jar is malformed") from exc
    if manifest.get("run_root") != str(run.resolve()) or manifest.get("run_id") != run.name:
        raise Failed("run was copied or its self-identifying root was rewritten")
    token = manifest.get("capture_token", "")
    token_path = run / "capture.token"
    if not TOKEN_RE.fullmatch(token) or token_path.is_symlink() or not token_path.is_file() or token_path.read_text().strip() != token:
        raise Failed("capture token is absent/malformed/mismatched")
    _validate_log(run / "server-create.log", token, capture=False)
    _validate_log(run / "server.log", token, capture=True)
    for key, relative in (("world_create", "server-create.log"), ("capture", "server.log")):
        path = run / relative
        record = manifest.get("logs", {}).get(key, {})
        if (
            path.is_symlink()
            or not path.is_file()
            or record.get("path") != relative
            or record.get("sha256") != sha256(path.read_bytes())
            or record.get("bytes") != path.stat().st_size
        ):
            raise Failed(f"Paper log provenance is absent or tampered: {relative}")
    driver_log = run / "driver.log"
    driver_text = driver_log.read_text() if driver_log.is_file() and not driver_log.is_symlink() else ""
    injected_sha = manifest.get("ticket", {}).get("injected_sha256")
    if (
        driver_log.is_symlink()
        or not driver_log.is_file()
        or manifest.get("driver_log", {}).get("sha256") != sha256(driver_log.read_bytes())
        or f"RIVET_TICKETS_INJECTED={injected_sha}" not in driver_text
        or "RIVET_CAPTURE_STOP_EXIT=0" not in driver_text
    ):
        raise Failed("driver lifecycle provenance is absent or tampered")
    process = manifest.get("process", {})
    if process.get("exit_code") != 0 or process.get("clean_stop") is not True or process.get("probe_ready_before_stop") is not True:
        raise Failed("Paper process did not exit cleanly with zero")
    boot1, capture = process.get("boot1", {}), process.get("capture", {})
    if boot1.get("exit_code") != 0 or capture.get("exit_code") != 0 or boot1.get("clean_stop") is not True or capture.get("clean_stop") is not True:
        raise Failed("one of the Paper boots was not a clean zero exit")
    if process.get("log") != "server.log" or boot1.get("log") != "server-create.log" or capture.get("log") != "server.log":
        raise Failed("Paper boot log route is wrong")
    _validate_config(run, expected_seed, manifest)
    if manifest.get("dimension") != "minecraft:overworld" or manifest.get("level_type") != "minecraft:normal" or manifest.get("generate_structures") is not True:
        raise Failed("wrong dimension/worldgen route")
    targets = [tuple(item) for item in manifest.get("targets", [])]
    if targets != EXPECTED_TARGETS:
        raise Failed("target order or target corpus differs")
    expected_closure = closure(EXPECTED_TARGETS, EXPECTED_RADIUS)
    closure_data = manifest.get("closure", {})
    if closure_data.get("radius") != EXPECTED_RADIUS or [tuple(item) for item in closure_data.get("coordinates", [])] != expected_closure:
        raise Failed("scheduler-derived radius-11 closure differs")
    if closure_data.get("sha256") != sha256(json.dumps(expected_closure, separators=(",", ":")).encode()):
        raise Failed("closure digest mismatch")
    ports = manifest.get("ports", {})
    configured_server = ports.get("configured_server")
    configured_query = ports.get("configured_query")
    boot1_ports = ports.get("boot1", {})
    capture_ports = ports.get("capture", {})
    if (
        ports.get("fixture_server") != 0
        or ports.get("fixture_query") != 0
        or not isinstance(configured_server, int)
        or not isinstance(configured_query, int)
        or configured_server <= 0
        or configured_query <= 0
        or configured_server == configured_query
        or not isinstance(boot1_ports.get("server"), int)
        or boot1_ports.get("server") != configured_server
        or not isinstance(capture_ports.get("server"), int)
        or capture_ports.get("server") != configured_server
    ):
        raise Failed("dynamic server/query port provenance is absent, zero, or static")
    preflight = manifest.get("preflight", {})
    if preflight.get("fresh_isolated_world_root") is not True or preflight.get("world_absent_before_boot1") is not True or preflight.get("boot1_created_world") is not True or preflight.get("reset_before_ticket_injection") is not True or preflight.get("before_injection_data_paths") != [] or preflight.get("before_injection_ticket_paths") != [] or preflight.get("no_preexisting_target_support_data") is not True or preflight.get("no_preexisting_tickets") is not True:
        raise Failed("preflight did not prove a fresh clean target/support/ticket root")
    _validate_worldgen_settings(run, expected_seed, manifest)
    _validate_tickets(run, expected_closure, manifest)
    _validate_probe(run, token, expected_closure)
    extraction = manifest.get("extraction", {})
    if extraction.get("post_exit_read_only") is not True or extraction.get("started_ns", 0) < capture.get("ended_ns", 1) or extraction.get("world_signature_before") != extraction.get("world_signature_after"):
        raise Failed("extraction was not post-exit/read-only")
    chunks = manifest.get("chunks")
    if not isinstance(chunks, list) or [(item.get("x"), item.get("z")) for item in chunks] != expected_closure:
        raise Failed("chunk evidence does not cover exact closure in order")
    actual_chunks = chunks_from_world(run / "world")
    if set(actual_chunks) < set(expected_closure):
        raise Failed("world is missing closure chunk data")
    semantic: dict[tuple[int, int], str] = {}
    for entry, coordinate in zip(chunks, expected_closure):
        raw = actual_chunks[coordinate]
        raw_path = run / entry.get("raw_path", "")
        if raw_path.is_symlink() or not raw_path.is_file() or raw_path.read_bytes() != raw:
            raise Failed(f"post-exit raw decompressed payload mismatch: {coordinate}")
        if entry.get("raw_sha256") != sha256(raw) or entry.get("raw_bytes") != len(raw):
            raise Failed(f"raw NBT hash mismatch: {coordinate}")
        _, semantic_hash, details = validate_chunk(raw, coordinate, target=coordinate in EXPECTED_TARGETS)
        if entry.get("status") != details["status"] or entry.get("semantic_sha256") != semantic_hash or entry.get("light_correct") != details["light_correct"] or entry.get("heightmaps") != details["heightmaps"]:
            raise Failed(f"chunk status/light/heightmap/semantic evidence mismatch: {coordinate}")
        semantic[coordinate] = semantic_hash
    if manifest.get("semantic_hash_dynamic_fields") != ["InhabitedTime", "LastUpdate"]:
        raise Failed("semantic hash dynamic-field contract is not narrowly documented")
    validate_inventory(run, manifest)
    return semantic


def validate_bundle(bundle_dir: Path) -> None:
    if not bundle_dir.is_dir() or bundle_dir.is_symlink():
        raise Unverified(f"bundle directory is absent or symlinked: {bundle_dir}")
    contract = _read_json(CONTRACT_PATH, "pinned contract")
    bundle_path = bundle_dir / "bundle.json"
    if not bundle_path.is_file() or bundle_path.is_symlink():
        raise Unverified("bundle.json is absent; no evidence is available")
    bundle = _read_json(bundle_path, "bundle.json")
    if bundle.get("format") != 1 or bundle.get("kind") != contract["kind"] or bundle.get("producer") != PRODUCER or bundle.get("paper_revision") != EXPECTED_PAPER:
        raise Failed("bundle provenance is wrong")
    if bundle.get("parity_claim") is not None or bundle.get("rivet_commit") is not None:
        raise Failed("bundle claims Rivet parity or stamps a Rivet commit")
    if bundle.get("contract_sha256") != sha256(CONTRACT_PATH.read_bytes()):
        raise Failed("bundle contract is stale or self-authored")
    attempts_per_seed = bundle.get("attempts_per_seed")
    if bundle.get("seeds") != EXPECTED_SEEDS or bundle.get("targets") != [list(item) for item in EXPECTED_TARGETS] or bundle.get("closure_radius") != EXPECTED_RADIUS or attempts_per_seed != 3:
        raise Failed("bundle corpus contract is incomplete or not exactly four seeds x three runs")
    runs = bundle.get("runs")
    if not isinstance(runs, list):
        raise Failed("bundle runs list is absent")
    expected = {(seed, attempt) for seed in EXPECTED_SEEDS for attempt in range(1, attempts_per_seed + 1)}
    actual = {(str(item.get("seed")), item.get("attempt")) for item in runs}
    if actual != expected or len(actual) != len(expected):
        raise Failed("bundle does not contain exactly three fresh roots for every seed")
    expected_paths = {str(Path("runs") / seed / str(attempt)) for seed, attempt in expected}
    listed_paths = {item.get("path") for item in runs}
    if listed_paths != expected_paths:
        raise Failed("bundle run paths are incomplete or duplicated")
    actual_dirs = {str(path.relative_to(bundle_dir)) for path in (bundle_dir / "runs").glob("*/*") if path.is_dir() and not path.is_symlink()}
    if actual_dirs != expected_paths:
        raise Failed("bundle contains an unlisted, stale, or symlinked run root")
    semantic_by_seed: dict[str, dict[tuple[int, int], str]] = {}
    for item in sorted(runs, key=lambda value: (str(value.get("seed")), int(value.get("attempt", 0)))):
        path = bundle_dir / item["path"]
        if path.resolve().parent.parent != (bundle_dir / "runs").resolve() or not str(path.resolve()).startswith(str((bundle_dir / "runs").resolve()) + "/"):
            raise Failed("run root escaped the isolated bundle output")
        semantic = validate_run(path, str(item["seed"]), int(item["attempt"]), contract)
        previous = semantic_by_seed.setdefault(str(item["seed"]), semantic)
        if semantic != previous:
            differing = next(coord for coord in semantic if semantic[coord] != previous[coord])
            raise Failed(f"semantic NBT hashes are nondeterministic for seed {item['seed']} at {differing}")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    args = parser.parse_args(argv)
    try:
        validate_bundle(args.bundle.resolve())
    except Unverified as exc:
        print(f"UNVERIFIED: {exc}")
        return 3
    except (Failed, OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        print(f"FAILED: {exc}")
        return 1
    print("VERIFIED: independent Paper normal-overworld FULL evidence")
    print("Paper-only evidence; no Rivet parity claim is made.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

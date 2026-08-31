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
import os
import re
import stat
import struct
import subprocess
import sys
import tempfile
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
EXPECTED_CONTRACT_SHA256 = "49933b73813b628a36205420b071d790974036b6f1d649cfbc803f89b9d794f1"
EXPECTED_INPUT_SHA256 = {
    "fixtures/server.properties": "468cdeaf43cde78f599a43fd55862d894a4470038f8df87931144131dd2e6d70",
    "fixtures/paper-global.yml": "06672c425d2a0e47b13a9f0a6e651d06ffbf2987a81a31978b673187cb8f0208",
    "fixtures/paper-world-defaults.yml": "86e97ba91308085c63a48ec7e9520031d93e2d826d827854251ad846caa7d5bc",
    "src/PaperNormalFullProbe.java": "f2c862a064d48e772874a50fd3a9caa6e7568047220cbdbce5fc4a7ffdb30af8",
    "src/plugin.yml": "150ea08d2aa8ac7f80442ccea9aedb93f20515948c4685b1dd7dd71d3cd9a784",
}
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
EXPECTED_CLOSURE_COUNT = 2451
MAX_SECTION_COUNT = 26
MAX_PALETTE_ENTRIES = 4096
MAX_BIOME_PALETTE_ENTRIES = 64
# Paper's normal overworld spans Y=-64..319, so its block sections are -4..19.
# SerializableChunkData also serializes one light boundary section on either side.
MIN_BLOCK_SECTION_Y = -4
MAX_BLOCK_SECTION_Y = 19
MIN_LIGHT_SECTION_Y = MIN_BLOCK_SECTION_Y - 1
MAX_LIGHT_SECTION_Y = MAX_BLOCK_SECTION_Y + 1
STARLIGHT_VERSION_TAG = "starlight.light_version"
STARLIGHT_LIGHT_VERSION = 10
REGION_RE = re.compile(r"^r\.(-?\d+)\.(-?\d+)\.mca$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
TOKEN_RE = re.compile(r"^[0-9a-f]{64}$")
THREAD_MARKER = "[MoonriseCommon] Paper is using 1 worker threads, 1 I/O threads"
MAX_JSON_BYTES = 32 * 1024 * 1024
MAX_TEXT_BYTES = 32 * 1024 * 1024
# Real closure captures contain region files above 8 MiB. Bound a maximally
# populated closure-covered region to 16 sectors (64 KiB) per one of its 1,024
# slots, plus the two-sector header, rather than rejecting valid Paper output.
MAX_REGION_BYTES = 2 * 4096 + min(1024, EXPECTED_CLOSURE_COUNT) * 16 * 4096
MAX_CHUNK_COMPRESSED_BYTES = 16 * 1024 * 1024
MAX_CHUNK_RAW_BYTES = 64 * 1024 * 1024
MAX_FILE_BYTES = 256 * 1024 * 1024
MAX_ARCHIVE_ENTRY_BYTES = 4 * 1024 * 1024
MAX_BUNDLE_FILES = 100_000
MAX_RUN_BYTES = 8 * (1 << 30)
MAX_BUNDLE_BYTES = 64 * (1 << 30)


class Failed(ValueError):
    pass


class Unverified(ValueError):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _require_regular_file(path: Path, label: str) -> os.stat_result:
    try:
        metadata = path.lstat()
    except OSError as exc:
        raise Failed(f"{label} is absent or unreadable: {path}") from exc
    if not stat.S_ISREG(metadata.st_mode):
        raise Failed(f"{label} is not a regular file: {path}")
    if metadata.st_nlink != 1:
        raise Failed(f"{label} is hardlinked: {path}")
    return metadata


def _read_bytes(path: Path, label: str, *, max_bytes: int = MAX_FILE_BYTES) -> bytes:
    before = _require_regular_file(path, label)
    if before.st_size > max_bytes:
        raise Failed(f"{label} exceeds size cap: {path}")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
        with os.fdopen(descriptor, "rb") as handle:
            opened = os.fstat(handle.fileno())
            if (
                not stat.S_ISREG(opened.st_mode)
                or opened.st_nlink != 1
                or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino)
                or opened.st_size > max_bytes
            ):
                raise Failed(f"{label} changed or exceeds size cap: {path}")
            data = handle.read(max_bytes + 1)
            after = os.fstat(handle.fileno())
    except OSError as exc:
        raise Failed(f"{label} is unreadable: {path}") from exc
    if len(data) > max_bytes:
        raise Failed(f"{label} exceeds size cap: {path}")
    if (
        len(data) != opened.st_size
        or (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        != (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns)
    ):
        raise Failed(f"{label} changed while being read: {path}")
    return data


def _read_text(path: Path, label: str, *, max_bytes: int = MAX_TEXT_BYTES, errors: str = "strict") -> str:
    data = _read_bytes(path, label, max_bytes=max_bytes)
    try:
        return data.decode("utf-8", errors=errors)
    except UnicodeError as exc:
        raise Failed(f"{label} is not valid UTF-8: {path}") from exc


def _pinned_input_bytes(relative: str) -> bytes:
    expected = EXPECTED_INPUT_SHA256.get(relative)
    if expected is None:
        raise Failed(f"unrecognized pinned producer input: {relative}")
    path = HERE / relative
    data = _read_bytes(path, f"pinned producer input {relative}", max_bytes=MAX_TEXT_BYTES)
    if sha256(data) != expected:
        raise Failed(f"pinned producer input was modified: {relative}")
    return data


def _pinned_input_text(relative: str) -> str:
    try:
        return _pinned_input_bytes(relative).decode("utf-8")
    except UnicodeError as exc:
        raise Failed(f"pinned producer input is not valid UTF-8: {relative}") from exc


def _validator_java_home(manifest: dict[str, Any]) -> Path:
    value = os.environ.get("JAVA_HOME")
    if not value:
        raise Unverified("JAVA_HOME is required to independently compile the pinned probe")
    home = Path(value).expanduser()
    if not home.is_absolute() or home.is_symlink() or home.resolve() != home or not home.is_dir():
        raise Unverified("JAVA_HOME is not a canonical JDK root")
    javac = home / "bin/javac"
    _require_regular_file(javac, "validator javac")
    if not os.access(javac, os.X_OK):
        raise Unverified("JAVA_HOME does not contain an executable javac")
    release = _read_text(home / "release", "validator JDK release", max_bytes=64 * 1024)
    if 'IMPLEMENTOR="Eclipse Adoptium"' not in release or 'JAVA_VERSION="25' not in release:
        raise Unverified("validator JDK is not Temurin 25")
    result = subprocess.run([str(javac), "-version"], capture_output=True, text=True, check=False)
    version = (result.stdout + result.stderr).strip()
    if result.returncode != 0 or not version.startswith("javac 25"):
        raise Unverified("validator javac is not Java 25")
    if manifest.get("java", {}).get("home") != str(home):
        raise Failed("capture and validator JDK roots differ")
    return home


def _compile_expected_probe_class(run: Path, runtime: Path, manifest: dict[str, Any]) -> bytes:
    java_home = _validator_java_home(manifest)
    libraries_root = run / "libraries"
    if libraries_root.is_symlink() or not libraries_root.is_dir():
        raise Failed("materialized Paper libraries root is absent or symlinked")
    libraries = sorted(libraries_root.rglob("*.jar"))
    if not libraries:
        raise Failed("materialized Paper libraries are absent")
    for library in libraries:
        metadata = _require_regular_file(library, "materialized Paper library")
        if metadata.st_size > MAX_FILE_BYTES:
            raise Failed(f"materialized Paper library exceeds size cap: {library}")
    classpath = os.pathsep.join([str(runtime), *(str(path) for path in libraries)])
    source_bytes = _pinned_input_bytes("src/PaperNormalFullProbe.java")
    class_relative = Path("org/rivet/paper_normal_full/PaperNormalFullProbe.class")
    with tempfile.TemporaryDirectory(prefix="rivet-probe-validation-") as directory:
        root = Path(directory)
        source = root / "PaperNormalFullProbe.java"
        classes = root / "classes"
        source.write_bytes(source_bytes)
        classes.mkdir()
        env = os.environ.copy()
        env["JAVA_HOME"] = str(java_home)
        env["PATH"] = f"{java_home / 'bin'}:{env.get('PATH', '')}"
        result = subprocess.run(
            [str(java_home / "bin/javac"), "-proc:none", "-cp", classpath, "-d", str(classes), str(source)],
            cwd=HERE,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise Failed(f"independent pinned probe compilation failed:\n{result.stdout}\n{result.stderr}")
        return _read_bytes(classes / class_relative, "independently compiled probe class", max_bytes=MAX_ARCHIVE_ENTRY_BYTES)


def _validate_compiled_probe_archive(
    probe_path: Path,
    plugin_bytes: bytes,
    expected_class_bytes: bytes,
    recorded_class_sha256: Any,
) -> None:
    class_relative = "org/rivet/paper_normal_full/PaperNormalFullProbe.class"
    try:
        with zipfile.ZipFile(probe_path) as archive:
            class_bytes = _read_archive_entry(archive, class_relative, "compiled probe class")
            archived_plugin = _read_archive_entry(archive, "plugin.yml", "compiled probe plugin.yml")
    except (KeyError, OSError, zipfile.BadZipFile) as exc:
        raise Failed("compiled main-thread probe jar is malformed") from exc
    if (
        archived_plugin != plugin_bytes
        or class_bytes != expected_class_bytes
        or recorded_class_sha256 != sha256(expected_class_bytes)
    ):
        raise Failed("compiled main-thread probe jar does not match independent pinned-source compilation")


def _validate_tree(root: Path, label: str, *, max_bytes: int = MAX_BUNDLE_BYTES) -> tuple[int, int]:
    """Reject links/non-regular nodes and bound individual and aggregate evidence size."""
    try:
        root_metadata = root.lstat()
    except OSError as exc:
        raise Failed(f"{label} is absent: {root}") from exc
    if not stat.S_ISDIR(root_metadata.st_mode) or stat.S_ISLNK(root_metadata.st_mode):
        raise Failed(f"{label} is not a real directory: {root}")
    files = 0
    total_bytes = 0
    for directory, dirnames, filenames in os.walk(root, topdown=True, followlinks=False):
        for name in dirnames:
            path = Path(directory) / name
            metadata = path.lstat()
            if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
                raise Failed(f"{label} contains an unsafe directory: {path}")
        for name in filenames:
            path = Path(directory) / name
            metadata = _require_regular_file(path, label)
            files += 1
            if metadata.st_size > MAX_FILE_BYTES:
                raise Failed(f"{label} contains a file exceeding the individual size cap: {path}")
            total_bytes += metadata.st_size
            if files > MAX_BUNDLE_FILES or total_bytes > max_bytes:
                raise Failed(f"{label} exceeds aggregate file/byte caps")
    return files, total_bytes


def probe_inputs_sha256(source_bytes: bytes, plugin_bytes: bytes) -> str:
    digest = hashlib.sha256()
    digest.update(b"PaperNormalFullProbe.java\0")
    digest.update(source_bytes)
    digest.update(b"\0plugin.yml\0")
    digest.update(plugin_bytes)
    return digest.hexdigest()


def _read_archive_entry(archive: zipfile.ZipFile, name: str, label: str) -> bytes:
    try:
        info = archive.getinfo(name)
    except KeyError as exc:
        raise Failed(f"{label} is absent from archive") from exc
    if info.file_size > MAX_ARCHIVE_ENTRY_BYTES:
        raise Failed(f"{label} exceeds archive entry size cap")
    data = archive.read(info)
    if len(data) != info.file_size:
        raise Failed(f"{label} archive entry size changed while reading")
    return data


def jar_manifest(path: Path) -> dict[str, str]:
    _require_regular_file(path, "jar manifest container")
    if path.stat().st_size > MAX_FILE_BYTES:
        raise Failed(f"jar manifest container exceeds size cap: {path}")
    try:
        with zipfile.ZipFile(path) as archive:
            text = _read_archive_entry(archive, "META-INF/MANIFEST.MF", "jar manifest").decode("utf-8", "replace")
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
    if (
        not source_root.is_absolute()
        or source_root != source_root.resolve()
        or not source_root.is_dir()
        or source_root.is_symlink()
    ):
        raise Failed("Paper source provenance does not identify a canonical Paper tree")
    top_level = subprocess.run(
        ["git", "-C", str(source_root), "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
        check=False,
    )
    if top_level.returncode != 0 or Path(top_level.stdout.strip()).resolve() != source_root:
        raise Failed("Paper source provenance does not identify the pinned checkout root")
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
    if len(data) > MAX_CHUNK_COMPRESSED_BYTES:
        raise Failed("compressed payload exceeds chunk size cap")
    obj = zlib.decompressobj(31 if gzip_stream else 15)
    try:
        raw = obj.decompress(data, MAX_CHUNK_RAW_BYTES + 1)
        if len(raw) <= MAX_CHUNK_RAW_BYTES:
            raw += obj.flush()
    except zlib.error as exc:
        raise Failed(f"compressed payload is malformed: {exc}") from exc
    if len(raw) > MAX_CHUNK_RAW_BYTES:
        raise Failed("decompressed payload exceeds chunk size cap")
    if not obj.eof:
        raise Failed("compressed payload ended before end-of-stream")
    if obj.unused_data or obj.unconsumed_tail:
        raise Failed("compressed payload has trailing or unconsumed bytes")
    return raw


def _unescape_property(value: str) -> str:
    result: list[str] = []
    escaped = False
    escapes = {"t": "\t", "r": "\r", "n": "\n", "f": "\f"}
    for character in value:
        if escaped:
            result.append(escapes.get(character, character))
            escaped = False
        elif character == "\\":
            escaped = True
        else:
            result.append(character)
    if escaped:
        result.append("\\")
    return "".join(result)


def parse_properties(text: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            key = _unescape_property(key.strip())
            if key in result:
                raise Failed(f"duplicate server.properties key: {key}")
            result[key] = _unescape_property(value.strip())
    return result


def properties_text(seed: str) -> str:
    return _pinned_input_text("fixtures/server.properties").replace("<seed>", str(java_seed(seed)))


def expected_properties(seed: str) -> dict[str, str]:
    fixture = parse_properties(_pinned_input_text("fixtures/server.properties"))
    return {key: (str(java_seed(seed)) if value == "<seed>" else value) for key, value in fixture.items()}


def _yaml_values(text: str) -> dict[tuple[str, ...], list[str]]:
    """Extract scalar paths needed from Paper's generated YAML.

    Paper rewrites the fixture files with its complete defaults on first boot.
    A small indentation-aware reader is enough for the scalar contract paths;
    it also lets validation reject duplicate effective keys without depending
    on an optional YAML package.
    """
    values: dict[tuple[str, ...], list[str]] = {}
    stack: list[tuple[int, str]] = []
    for raw_line in text.splitlines():
        if not raw_line.strip() or raw_line.lstrip().startswith("#"):
            continue
        indent = len(raw_line) - len(raw_line.lstrip(" "))
        line = raw_line.strip()
        if line in {"---", "..."} or line.startswith("-") or ":" not in line:
            continue
        key, value = line.split(":", 1)
        key = key.strip()
        value = value.strip()
        while stack and indent <= stack[-1][0]:
            stack.pop()
        if not value:
            stack.append((indent, key))
            continue
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
            value = value[1:-1]
        path = tuple(item[1] for item in stack) + (key,)
        values.setdefault(path, []).append(value)
    return values


def _validate_paper_yaml(run: Path) -> None:
    for key, relative, fixture_name in (
        ("paper_global", "provenance/config/paper-global.yml", "paper-global.yml"),
        ("paper_world_defaults", "provenance/config/paper-world-defaults.yml", "paper-world-defaults.yml"),
    ):
        fixture = _pinned_input_text(f"fixtures/{fixture_name}")
        runtime_relative = relative.removeprefix("provenance/")
        runtime = run / runtime_relative
        runtime_values = _yaml_values(_read_text(runtime, f"runtime Paper YAML {key}"))
        fixture_values = _yaml_values(fixture)
        for path, expected in fixture_values.items():
            actual = runtime_values.get(path)
            if actual != expected:
                raise Failed(f"runtime Paper YAML does not preserve {key} path {'.'.join(path)}")


def read_region(path: Path, region_coordinates: tuple[int, int]) -> dict[tuple[int, int], bytes]:
    metadata = _require_regular_file(path, "region file")
    if metadata.st_size > MAX_REGION_BYTES:
        raise Failed(f"region file exceeds size cap: {path}")
    data = _read_bytes(path, "region file", max_bytes=MAX_REGION_BYTES)
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
            external_metadata = _require_regular_file(external_path, "external chunk payload")
            if external_metadata.st_size > MAX_CHUNK_COMPRESSED_BYTES:
                raise Failed(f"external chunk payload exceeds size cap: {external_path}")
            payload = _read_bytes(external_path, "external chunk payload", max_bytes=MAX_CHUNK_COMPRESSED_BYTES)
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
        _require_regular_file(path, "region file")
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
    return decoded, list(required)


def _packed_bits(palette_size: int, *, biome: bool) -> int:
    if palette_size <= 0:
        raise Failed("paletted container has an empty palette")
    if palette_size == 1:
        return 0
    minimum = 1 if biome else 4
    return max(minimum, (palette_size - 1).bit_length())


def _packed_words(entry_count: int, bits: int) -> int:
    if bits == 0:
        return 0
    values_per_long = 64 // bits
    return (entry_count + values_per_long - 1) // values_per_long


def _decode_packed_values(
    data: Tag | None,
    *,
    palette_size: int,
    entry_count: int,
    biome: bool,
    label: str,
) -> None:
    bits = _packed_bits(palette_size, biome=biome)
    expected_words = _packed_words(entry_count, bits)
    if bits == 0:
        if data is not None:
            raise Failed(f"{label} zero-bit container must omit data")
        return
    if data is None or data.kind != 12 or len(data.value) != expected_words:
        raise Failed(f"{label} has the wrong packed data shape")
    mask = (1 << bits) - 1
    values_per_long = 64 // bits
    words = [value & ((1 << 64) - 1) for value in data.value]
    for index in range(entry_count):
        word = index // values_per_long
        shift = (index % values_per_long) * bits
        value = (words[word] >> shift) & mask
        if value >= palette_size:
            raise Failed(f"{label} packed index {value} at {index} exceeds palette size {palette_size}")


def _block_palette_names(root: Tag, coordinate: tuple[int, int], *, target: bool) -> set[str]:
    names: set[str] = set()
    sections = get_any(root, "sections")
    if sections is None or sections.kind != 9 or not isinstance(sections.value, tuple) or len(sections.value) != 2:
        raise Failed(f"{coordinate} has no sections list")
    section_kind, section_items = sections.value
    if section_kind != 10 or not isinstance(section_items, list):
        raise Failed(f"{coordinate} sections list is not a compound list")
    if len(section_items) > MAX_SECTION_COUNT:
        raise Failed(f"{coordinate} has too many sections")
    seen_sections: set[int] = set()
    for section in section_items:
        if section.kind != 10:
            raise Failed(f"{coordinate} section list contains non-compounds")

        y_tag = section.value.get("Y")
        if y_tag is None:
            section_y = 0
        elif y_tag.kind == 1:
            section_y = y_tag.value
        else:
            raise Failed(f"{coordinate} section Y is not a byte")
        if section_y in seen_sections:
            raise Failed(f"{coordinate} contains duplicate section Y {section_y}")
        seen_sections.add(section_y)
        if not MIN_LIGHT_SECTION_Y <= section_y <= MAX_LIGHT_SECTION_Y:
            raise Failed(f"{coordinate} section Y {section_y} is outside the light-section bounds")

        for light_name in ("BlockLight", "SkyLight"):
            light = section.value.get(light_name)
            if light is not None and (light.kind != 7 or len(light.value) != 2048):
                raise Failed(f"{coordinate} {light_name} is not a 2048-byte light layer")

        # Paper's SerializableChunkData drops the block/biome container for the
        # two light-only boundary sections. It only parses those codecs for the
        # normal in-bounds block sections.
        if not MIN_BLOCK_SECTION_Y <= section_y <= MAX_BLOCK_SECTION_Y:
            continue

        states = section.value.get("block_states")
        if states is None:
            raise Failed(f"{coordinate} section has no block_states codec")
        if states.kind != 10:
            raise Failed(f"{coordinate} block_states is not a compound")
        palette = states.value.get("palette")
        if palette is None or palette.kind != 9 or not isinstance(palette.value, tuple) or len(palette.value) != 2:
            raise Failed(f"{coordinate} block_states palette is absent or malformed")
        palette_kind, palette_items = palette.value
        if palette_kind != 10 or not isinstance(palette_items, list):
            raise Failed(f"{coordinate} block_states palette is not a compound list")
        if len(palette_items) > MAX_PALETTE_ENTRIES:
            raise Failed(f"{coordinate} block_states palette is too large")
        data = states.value.get("data")
        _decode_packed_values(
            data,
            palette_size=len(palette_items),
            entry_count=4096,
            biome=False,
            label=f"{coordinate} block_states",
        )
        for entry in palette_items:
            if entry.kind != 10:
                raise Failed(f"{coordinate} block_states palette contains a non-compound")
            name = entry.value.get("Name")
            if name is None or name.kind != 8:
                raise Failed(f"{coordinate} block_states palette entry has no string Name")
            names.add(name.value)

        biomes = section.value.get("biomes")
        if biomes is None or biomes.kind != 10:
            raise Failed(f"{coordinate} section has no biomes codec")
        biome_palette = biomes.value.get("palette")
        if (
            biome_palette is None
            or biome_palette.kind != 9
            or not isinstance(biome_palette.value, tuple)
            or len(biome_palette.value) != 2
        ):
            raise Failed(f"{coordinate} biomes palette is absent or malformed")
        biome_kind, biome_items = biome_palette.value
        if biome_kind != 8 or not isinstance(biome_items, list):
            raise Failed(f"{coordinate} biomes palette is not a string list")
        if len(biome_items) > MAX_BIOME_PALETTE_ENTRIES:
            raise Failed(f"{coordinate} biomes palette is too large")
        _decode_packed_values(
            biomes.value.get("data"),
            palette_size=len(biome_items),
            entry_count=64,
            biome=True,
            label=f"{coordinate} biomes",
        )
        if any(not isinstance(item.value, str) for item in biome_items):
            raise Failed(f"{coordinate} biomes palette contains a malformed entry")
    if target and len(names) < 6:
        raise Failed(f"{coordinate} has a flat/under-varied block palette")
    return names


def persisted_light_correct(root: Tag) -> bool:
    version = get_any(root, STARLIGHT_VERSION_TAG)
    return version is not None and version.kind == 3 and version.value == STARLIGHT_LIGHT_VERSION


def validate_chunk(raw: bytes, coordinate: tuple[int, int], *, target: bool) -> tuple[str, str, dict[str, Any]]:
    if len(raw) > MAX_CHUNK_RAW_BYTES:
        raise Failed(f"{coordinate} raw NBT exceeds chunk size cap")
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
    light_correct = persisted_light_correct(root)
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


def world_tree_signature(world: Path) -> str:
    rows: list[str] = []
    for path in sorted(world.rglob("*")):
        if path.is_symlink():
            raise Failed(f"Paper world tree contains a symlink: {path}")
        if path.is_dir():
            continue
        before = _require_regular_file(path, "Paper world tree file")
        data = _read_bytes(path, "Paper world tree file")
        after = path.stat()
        before_identity = (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
        after_identity = (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        if before_identity != after_identity:
            raise Failed(f"Paper world file changed while validating: {path}")
        rows.append(
            f"{path.relative_to(world)}\\0{before.st_dev}\\0{before.st_ino}\\0{before.st_size}"
            f"\\0{before.st_mtime_ns}\\0{sha256(data)}"
        )
    return sha256("\\n".join(rows).encode())


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
                    _require_regular_file(path, "inventory path")
                    result.add(str(path.relative_to(world)))
    for path in world.rglob("*.mcc"):
        _require_regular_file(path, "inventory .mcc")
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
        digest = sha256(_read_bytes(path, f"inventory path {rel}"))
        entry = _manifest_entry(manifest, rel)
        if entry.get("sha256") != digest or entry.get("bytes") != path.stat().st_size:
            raise Failed(f"inventory hash mismatch: {rel}")
        if entry.get("kind") not in {"region", "poi", "entities", "mcc", "other"}:
            raise Failed(f"inventory kind is unknown: {rel}")


def _read_json(path: Path, label: str) -> dict[str, Any]:
    metadata = _require_regular_file(path, label)
    if metadata.st_size > MAX_JSON_BYTES:
        raise Failed(f"{label} exceeds size cap")
    try:
        value = json.loads(_read_text(path, label, max_bytes=MAX_JSON_BYTES))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise Failed(f"{label} is malformed: {exc}") from exc
    if not isinstance(value, dict):
        raise Failed(f"{label} is not an object")
    return value


def _reject_symlinks_under(root: Path, label: str) -> None:
    for path in root.rglob("*"):
        if path.is_symlink():
            raise Failed(f"{label} contains a symlink: {path}")


def _validate_worldgen_settings(run: Path, expected_seed: str, manifest: dict[str, Any]) -> None:
    source = run / "world/dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat"
    contract_copy = run / "world/data/minecraft/worldgen_settings.dat"
    if source.is_symlink() or contract_copy.is_symlink() or not source.is_file() or not contract_copy.is_file():
        raise Failed("exact Paper worldgen_settings.dat capture is absent or symlinked")
    source_bytes = _read_bytes(source, "Paper worldgen settings", max_bytes=MAX_CHUNK_COMPRESSED_BYTES)
    if _read_bytes(contract_copy, "worldgen settings contract copy", max_bytes=MAX_CHUNK_COMPRESSED_BYTES) != source_bytes:
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
    text = _read_text(path, "Paper log", errors="replace")
    bad = [line for line in text.splitlines() if re.search(r"\[(ERROR|FATAL)\]|Exception|StackTrace|fallback|recovery|RIVET_PROBE_FAILED", line, re.I)]
    if bad:
        raise Failed(f"Paper log contains an error/fallback/recovery line: {bad[0]}")
    required = ["Done (", "All dimensions are saved", "Stopping server", THREAD_MARKER]
    if capture:
        required += ["RIVET_PROBE_READY", "RIVET_CAPTURE_TOKEN=" + token, "RIVET_SIMULATION_FROZEN"]
    if any(marker not in text for marker in required):
        raise Failed(f"Paper log is missing a required {'capture' if capture else 'create'} marker")

    def unique_marker(marker: str) -> int:
        positions = [match.start() for match in re.finditer(re.escape(marker), text)]
        if len(positions) != 1:
            raise Failed(f"Paper log marker is missing or duplicated: {marker}")
        return positions[0]

    unique_marker(THREAD_MARKER)
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
        if not (frozen_at < ready_at <= token_at < done_at < stopping_at):
            raise Failed("Paper simulation/probe-ready/token markers are not ordered before graceful stop")


def _validate_config(run: Path, seed: str, manifest: dict[str, Any]) -> None:
    expected = expected_properties(seed)
    provenance = run / "provenance/server.properties"
    actual = run / "server.properties"
    if provenance.is_symlink() or actual.is_symlink() or not provenance.is_file() or not actual.is_file():
        raise Failed("server.properties provenance is absent or symlinked")
    if _read_text(provenance, "server.properties provenance") != properties_text(seed):
        raise Failed("server.properties provenance differs from pinned fixture")
    actual_properties = parse_properties(_read_text(actual, "runtime server.properties"))
    if not set(expected).issubset(actual_properties):
        raise Failed("runtime server.properties is missing a pinned normal-overworld key")
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
        if path.is_symlink() or not path.is_file() or record.get("path") != relative or record.get("sha256") != sha256(_read_bytes(path, f"config provenance {relative}")) or record.get("bytes") != path.stat().st_size:
            raise Failed(f"config provenance is absent or tampered: {relative}")
    if _read_bytes(run / "provenance/eula.txt", "provenance eula", max_bytes=1024) != b"eula=true\n" or _read_bytes(run / "eula.txt", "runtime eula", max_bytes=1024) != b"eula=true\n":
        raise Failed("runtime or provenance eula is not pinned")
    for key, relative, fixture in (
        ("paper_global", "provenance/config/paper-global.yml", "paper-global.yml"),
        ("paper_world_defaults", "provenance/config/paper-world-defaults.yml", "paper-world-defaults.yml"),
    ):
        path = run / relative
        record = config.get(key, {})
        fixture_bytes = _pinned_input_bytes(f"fixtures/{fixture}")
        if path.is_symlink() or not path.is_file() or _read_bytes(path, f"provenance config {relative}") != fixture_bytes:
            raise Failed(f"pinned provenance config differs: {relative}")
        if record.get("path") != relative or record.get("sha256") != sha256(fixture_bytes) or record.get("bytes") != len(fixture_bytes):
            raise Failed(f"config manifest provenance is absent or tampered: {relative}")
    for key, relative in (
        ("runtime_paper_global", "config/paper-global.yml"),
        ("runtime_paper_world_defaults", "config/paper-world-defaults.yml"),
    ):
        path = run / relative
        record = config.get(key, {})
        if path.is_symlink() or not path.is_file() or record.get("path") != relative or record.get("sha256") != sha256(_read_bytes(path, f"runtime Paper config {relative}")) or record.get("bytes") != path.stat().st_size:
            raise Failed(f"runtime Paper config provenance is absent or tampered: {relative}")
    _validate_paper_yaml(run)
    simulation = manifest.get("simulation")
    if simulation != {"random_tick_speed": 0, "do_daylight_cycle": False, "do_weather_cycle": False, "do_mob_spawning": False, "spawn_limits": 0}:
        raise Failed("simulation is not frozen by the pinned contract")


def _validate_tickets(run: Path, expected_closure: list[tuple[int, int]], manifest: dict[str, Any]) -> None:
    ticket_manifest = manifest.get("ticket", {})
    injected_relative = ticket_manifest.get("injected_path")
    post_relative = ticket_manifest.get("post_exit_path")
    if injected_relative != "provenance/chunk_tickets.dat" or post_relative != "world/dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat":
        raise Failed("ticket provenance paths are not pinned")
    injected_path = run / injected_relative
    post_path = run / post_relative
    for path, label in ((injected_path, "injected"), (post_path, "post-stop")):
        if path.is_symlink() or not path.is_file():
            raise Failed(f"{label} forced tickets are absent or symlinked")

    def coordinates_from(path: Path) -> list[tuple[int, int]]:
        try:
            root = parse(strict_decompress(_read_bytes(path, "forced ticket data", max_bytes=MAX_CHUNK_COMPRESSED_BYTES), gzip_stream=True))
        except NbtError as exc:
            raise Failed(f"chunk_tickets.dat is malformed/trailing: {path}: {exc}") from exc
        if root.kind != 10 or root.value.get("DataVersion") != Tag(3, EXPECTED_DATA_VERSION):
            raise Failed(f"chunk_tickets.dat DataVersion is not 4903: {path}")
        data = root.value.get("data")
        tickets = data.value.get("tickets") if data is not None and data.kind == 10 else None
        if tickets is None or tickets.kind != 9 or tickets.value[0] != 10:
            raise Failed(f"chunk_tickets.dat has no compound data.tickets list: {path}")
        if len(tickets.value[1]) > EXPECTED_CLOSURE_COUNT:
            raise Failed(f"chunk_tickets.dat contains too many tickets: {path}")
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
        return coordinates

    injected_coordinates = coordinates_from(injected_path)
    post_coordinates = coordinates_from(post_path)
    if injected_coordinates != expected_closure:
        raise Failed("injected forced ticket order/set differs from scheduler closure")
    if len(post_coordinates) != len(set(post_coordinates)) or sorted(post_coordinates) != expected_closure:
        raise Failed("post-exit forced ticket set differs from scheduler closure")
    if ticket_manifest.get("coordinates") != [list(item) for item in expected_closure]:
        raise Failed("forced ticket manifest coordinates differ from scheduler closure")
    if (
        ticket_manifest.get("injected_sha256") != sha256(_read_bytes(injected_path, "injected forced ticket data", max_bytes=MAX_CHUNK_COMPRESSED_BYTES))
        or ticket_manifest.get("post_exit_sha256") != sha256(_read_bytes(post_path, "post-stop forced ticket data", max_bytes=MAX_CHUNK_COMPRESSED_BYTES))
        or ticket_manifest.get("held_through_stop") is not True
    ):
        raise Failed("ticket lifecycle/hash provenance is incomplete")


def _validate_process_timing(manifest: dict[str, Any]) -> None:
    process = manifest.get("process", {})
    boot1 = process.get("boot1", {})
    capture = process.get("capture", {})
    extraction = manifest.get("extraction", {})
    timestamps = [
        boot1.get("started_ns"),
        boot1.get("stop_command_ns"),
        boot1.get("ended_ns"),
        capture.get("started_ns"),
        capture.get("stop_command_ns"),
        capture.get("ended_ns"),
        extraction.get("started_ns"),
    ]
    if any(type(value) is not int or value <= 0 for value in timestamps):
        raise Failed("Paper process/extraction timestamps are absent or malformed")
    if not (
        boot1["started_ns"] <= boot1["stop_command_ns"] <= boot1["ended_ns"]
        <= capture["started_ns"] <= capture["stop_command_ns"] <= capture["ended_ns"]
        <= extraction["started_ns"]
    ):
        raise Failed("Paper process/extraction timestamps are out of order")


def _validate_probe(run: Path, token: str, expected_closure: list[tuple[int, int]]) -> None:
    probe = _read_json(run / "probe.json", "probe.json")
    if (
        probe.get("format") != 1
        or probe.get("producer") != PROBE
        or probe.get("side") != "server"
        or probe.get("thread") != "main"
        or probe.get("main_thread") is not True
        or probe.get("world") != "minecraft:overworld"
    ):
        raise Failed("server-side main-thread Paper probe provenance is missing")
    if probe.get("token") != token or probe.get("closure_count") != len(expected_closure) or probe.get("simulation_frozen") is not True:
        raise Failed("probe closure/token/simulation evidence is incomplete")
    targets = [(item.get("x"), item.get("z")) for item in probe.get("targets", [])]
    if targets != EXPECTED_TARGETS:
        raise Failed("probe target order differs")
    for item in probe["targets"]:
        if item.get("status") != "minecraft:full" or item.get("light_correct") is not True:
            raise Failed("probe target did not prove FULL+light")


def validate_run(run: Path, expected_seed: str, expected_attempt: int, contract: dict[str, Any]) -> dict[tuple[int, int], str]:
    _validate_tree(run, "Paper run root", max_bytes=MAX_RUN_BYTES)
    world = run / "world"
    if world.is_symlink() or not world.is_dir():
        raise Failed("Paper world root is absent or symlinked")
    _reject_symlinks_under(run, "Paper run root")
    manifest = _read_json(run / "capture.json", "capture manifest")
    if manifest.get("format") != 1 or manifest.get("kind") != contract["kind"] or manifest.get("producer") != PRODUCER:
        raise Failed("wrong producer, kind, or manifest format")
    if manifest.get("parity_claim") is not None or manifest.get("rivet_commit") is not None:
        raise Failed("evidence contains a parity claim or Rivet commit")
    if (
        type(manifest.get("seed")) is not str
        or manifest.get("seed") != expected_seed
        or type(manifest.get("java_seed")) is not str
        or manifest.get("java_seed") != str(java_seed(expected_seed))
        or type(manifest.get("attempt")) is not int
        or manifest.get("attempt") != expected_attempt
    ):
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
    if source_jar.name != "paper-paperclip-26.2.local-SNAPSHOT.jar" or not source_jar.resolve().is_relative_to(source_root.resolve()):
        raise Failed("built Paperclip jar is outside the pinned Paper source tree")
    _require_regular_file(source_jar, "built Paperclip jar")
    if source_jar.stat().st_mtime_ns < int(paper_jar["built_after_ns"]) or paper_jar.get("sha256") != sha256(_read_bytes(source_jar, "built Paperclip jar")) or paper_jar.get("bytes") != source_jar.stat().st_size:
        raise Failed("built Paperclip jar provenance is absent, stale, or tampered")
    if jar_manifest(source_jar).get("Main-Class") != "io.papermc.paperclip.Main":
        raise Failed("built Paperclip jar is not the pinned Paperclip launcher")
    source_server = paper_jar.get("source_server_jar", {})
    server_path = Path(source_server.get("path", ""))
    if server_path.name != "paper-server-26.2.local-SNAPSHOT.jar" or not server_path.resolve().is_relative_to(source_root.resolve()):
        raise Failed("built Paper server jar is outside the pinned Paper source tree")
    _require_regular_file(server_path, "built Paper server jar")
    if (
        server_path.stat().st_mtime_ns < int(paper_jar["built_after_ns"])
        or source_server.get("git_commit") != EXPECTED_PAPER_SHORT
        or source_server.get("sha256") != sha256(_read_bytes(server_path, "built Paper server jar"))
        or source_server.get("bytes") != server_path.stat().st_size
        or jar_manifest(server_path).get("Git-Commit") != EXPECTED_PAPER_SHORT
    ):
        raise Failed("built Paper server jar does not prove the pinned source")
    boot_artifact = paper_jar.get("boot_artifact", {})
    if (
        boot_artifact.get("path") != ".paper-paperclip.jar"
        or not isinstance(boot_artifact.get("sha256"), str)
        or not isinstance(boot_artifact.get("bytes"), int)
    ):
        raise Failed("stable Paperclip boot artifact provenance is absent")
    boot_artifact_path = run / boot_artifact["path"]
    if (
        boot_artifact_path.is_symlink()
        or not boot_artifact_path.is_file()
        or boot_artifact.get("sha256") != sha256(_read_bytes(boot_artifact_path, "stable Paperclip boot artifact"))
        or boot_artifact.get("bytes") != boot_artifact_path.stat().st_size
    ):
        raise Failed("stable Paperclip boot artifact is absent, stale, or tampered")
    runtime = paper_jar.get("materialized_runtime", {})
    runtime_relative = runtime.get("path")
    if runtime_relative != "versions/26.2/paper-26.2.jar":
        raise Failed("fresh materialized Paper runtime path is not pinned")
    runtime_path = run / runtime_relative
    if (
        runtime_path.is_symlink()
        or not runtime_path.is_file()
        or runtime.get("git_commit") != EXPECTED_PAPER_SHORT
        or runtime.get("paperclip_sha256") != boot_artifact["sha256"]
        or runtime.get("sha256") != sha256(_read_bytes(runtime_path, "materialized Paper runtime"))
        or runtime.get("bytes") != runtime_path.stat().st_size
    ):
        raise Failed("fresh materialized Paper runtime provenance is absent, stale, or tampered")
    if jar_manifest(runtime_path).get("Git-Commit") != EXPECTED_PAPER_SHORT:
        raise Failed("fresh materialized Paper runtime does not prove the pinned source")
    probe_artifact = manifest.get("probe_artifact", {})
    if paper_jar.get("probe_artifact") != probe_artifact:
        raise Failed("compiled probe provenance is duplicated inconsistently")
    probe_relative = probe_artifact.get("path")
    if probe_relative != "plugins/RivetPaperNormalFullProbe.jar":
        raise Failed("compiled main-thread probe path is not pinned")
    probe_path = run / probe_relative
    probe_source = HERE / "src/PaperNormalFullProbe.java"
    plugin_yml = HERE / "src/plugin.yml"
    if (
        probe_source.is_symlink()
        or plugin_yml.is_symlink()
        or not probe_source.is_file()
        or not plugin_yml.is_file()
    ):
        raise Failed("Paper probe source/plugin inputs are absent or symlinked")
    source_bytes = _pinned_input_bytes("src/PaperNormalFullProbe.java")
    plugin_bytes = _pinned_input_bytes("src/plugin.yml")
    input_digest = probe_inputs_sha256(source_bytes, plugin_bytes)
    if (
        probe_path.is_symlink()
        or not probe_path.is_file()
        or probe_artifact.get("sha256") != sha256(_read_bytes(probe_path, "compiled main-thread probe jar"))
        or probe_artifact.get("bytes") != probe_path.stat().st_size
        or paper_jar.get("probe_source_sha256") != sha256(source_bytes)
        or paper_jar.get("probe_plugin_yml_sha256") != sha256(plugin_bytes)
        or paper_jar.get("probe_inputs_sha256") != input_digest
        or probe_artifact.get("inputs_sha256") != input_digest
        or not isinstance(probe_artifact.get("class_sha256"), str)
    ):
        raise Failed("compiled main-thread probe provenance is absent or tampered")
    expected_class_bytes = _compile_expected_probe_class(run, runtime_path, manifest)
    _validate_compiled_probe_archive(
        probe_path,
        plugin_bytes,
        expected_class_bytes,
        probe_artifact.get("class_sha256"),
    )
    if manifest.get("run_root") != str(run.resolve()) or manifest.get("run_id") != run.name:
        raise Failed("run was copied or its self-identifying root was rewritten")
    token = manifest.get("capture_token", "")
    token_path = run / "capture.token"
    if not TOKEN_RE.fullmatch(token) or token_path.is_symlink() or not token_path.is_file() or _read_text(token_path, "capture token", max_bytes=1024).strip() != token:
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
            or record.get("sha256") != sha256(_read_bytes(path, f"Paper log {relative}", max_bytes=MAX_TEXT_BYTES))
            or record.get("bytes") != path.stat().st_size
        ):
            raise Failed(f"Paper log provenance is absent or tampered: {relative}")
    driver_log = run / "driver.log"
    driver_text = _read_text(driver_log, "capture driver log") if driver_log.is_file() and not driver_log.is_symlink() else ""
    injected_sha = manifest.get("ticket", {}).get("injected_sha256")
    if (
        driver_log.is_symlink()
        or not driver_log.is_file()
        or manifest.get("driver_log", {}).get("sha256") != sha256(_read_bytes(driver_log, "capture driver log", max_bytes=MAX_TEXT_BYTES))
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
    if boot1.get("paperclip") != boot_artifact or capture.get("paperclip") != boot_artifact:
        raise Failed("Paper boots are not bound to the stable Paperclip artifact")
    if process.get("log") != "server.log" or boot1.get("log") != "server-create.log" or capture.get("log") != "server.log":
        raise Failed("Paper boot log route is wrong")
    _validate_process_timing(manifest)
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
        or type(configured_server) is not int
        or type(configured_query) is not int
        or not 1 <= configured_server <= 65535
        or not 1 <= configured_query <= 65535
        or configured_server == configured_query
        or type(boot1_ports.get("server")) is not int
        or not 1 <= boot1_ports.get("server", 0) <= 65535
        or boot1_ports.get("server") != configured_server
        or type(capture_ports.get("server")) is not int
        or not 1 <= capture_ports.get("server", 0) <= 65535
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
    if world_tree_signature(world) != extraction.get("world_signature_after"):
        raise Failed("Paper world content or stable file identities changed after extraction")
    chunks = manifest.get("chunks")
    if not isinstance(chunks, list) or [(item.get("x"), item.get("z")) for item in chunks] != expected_closure:
        raise Failed("chunk evidence does not cover exact closure in order")
    actual_chunks = chunks_from_world(run / "world")
    if set(actual_chunks) != set(expected_closure):
        missing = len(set(expected_closure) - set(actual_chunks))
        extra = len(set(actual_chunks) - set(expected_closure))
        raise Failed(f"world chunk data is not the exact closure (missing={missing}, extra={extra})")
    semantic: dict[tuple[int, int], str] = {}
    for entry, coordinate in zip(chunks, expected_closure):
        raw = actual_chunks[coordinate]
        expected_raw_path = f"chunks/{coordinate[0]}.{coordinate[1]}.nbt"
        if entry.get("raw_path") != expected_raw_path:
            raise Failed(f"chunk raw path is not the pinned closure path: {coordinate}")
        raw_path = run / expected_raw_path
        if raw_path.is_symlink() or not raw_path.is_file() or _read_bytes(raw_path, f"raw decompressed chunk payload {coordinate}", max_bytes=MAX_CHUNK_RAW_BYTES) != raw:
            raise Failed(f"post-exit raw decompressed payload mismatch: {coordinate}")
        if entry.get("raw_sha256") != sha256(raw) or entry.get("raw_bytes") != len(raw):
            raise Failed(f"raw NBT hash mismatch: {coordinate}")
        _, semantic_hash, details = validate_chunk(raw, coordinate, target=coordinate in EXPECTED_TARGETS)
        if any(
            entry.get(key) != details[key]
            for key in ("status", "light_correct", "heightmaps", "heightmap_ranges", "palette_names")
        ) or entry.get("semantic_sha256") != semantic_hash:
            raise Failed(f"chunk status/light/heightmap/semantic evidence mismatch: {coordinate}")
        semantic[coordinate] = semantic_hash
    if manifest.get("semantic_hash_dynamic_fields") != ["InhabitedTime", "LastUpdate"]:
        raise Failed("semantic hash dynamic-field contract is not narrowly documented")
    validate_inventory(run, manifest)
    return semantic


def validate_bundle(bundle_dir: Path) -> None:
    try:
        metadata = bundle_dir.lstat()
    except FileNotFoundError as exc:
        raise Unverified(f"bundle directory is absent: {bundle_dir}") from exc
    except OSError as exc:
        raise Failed(f"bundle root is unreadable: {bundle_dir}") from exc
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise Failed(f"bundle root is present but unsafe: {bundle_dir}")
    _validate_tree(bundle_dir, "evidence bundle")
    contract_bytes = _read_bytes(CONTRACT_PATH, "pinned contract", max_bytes=MAX_JSON_BYTES)
    if sha256(contract_bytes) != EXPECTED_CONTRACT_SHA256:
        raise Failed("pinned contract fixture was modified")
    contract = _read_json(CONTRACT_PATH, "pinned contract")
    bundle_path = bundle_dir / "bundle.json"
    if not bundle_path.is_file() or bundle_path.is_symlink():
        raise Unverified("bundle.json is absent; no evidence is available")
    bundle = _read_json(bundle_path, "bundle.json")
    if bundle.get("format") != 1 or bundle.get("kind") != contract["kind"] or bundle.get("producer") != PRODUCER or bundle.get("paper_revision") != EXPECTED_PAPER:
        raise Failed("bundle provenance is wrong")
    if bundle.get("parity_claim") is not None or bundle.get("rivet_commit") is not None:
        raise Failed("bundle claims Rivet parity or stamps a Rivet commit")
    if bundle.get("contract_sha256") != sha256(contract_bytes):
        raise Failed("bundle contract is stale or self-authored")
    attempts_per_seed = bundle.get("attempts_per_seed")
    if (
        bundle.get("seeds") != EXPECTED_SEEDS
        or bundle.get("targets") != [list(item) for item in EXPECTED_TARGETS]
        or bundle.get("closure_radius") != EXPECTED_RADIUS
        or type(attempts_per_seed) is not int
        or attempts_per_seed != 3
    ):
        raise Failed("bundle corpus contract is incomplete or not exactly four seeds x three runs")
    runs = bundle.get("runs")
    if not isinstance(runs, list):
        raise Failed("bundle runs list is absent")
    expected = {(seed, attempt) for seed in EXPECTED_SEEDS for attempt in range(1, attempts_per_seed + 1)}
    if len(runs) != len(expected) or any(
        not isinstance(item, dict)
        or type(item.get("seed")) is not str
        or item.get("seed") not in EXPECTED_SEEDS
        or type(item.get("attempt")) is not int
        or item.get("attempt") < 1
        or item.get("attempt") > attempts_per_seed
        or type(item.get("path")) is not str
        for item in runs
    ):
        raise Failed("bundle run entries are malformed")
    actual = {(item["seed"], item["attempt"]) for item in runs}
    if actual != expected or len(actual) != len(expected):
        raise Failed("bundle does not contain exactly three fresh roots for every seed")
    expected_paths = {str(Path("runs") / seed / str(attempt)) for seed, attempt in expected}
    listed_paths = {item["path"] for item in runs}
    if listed_paths != expected_paths:
        raise Failed("bundle run paths are incomplete or duplicated")
    actual_dirs = {str(path.relative_to(bundle_dir)) for path in (bundle_dir / "runs").glob("*/*") if path.is_dir() and not path.is_symlink()}
    if actual_dirs != expected_paths:
        raise Failed("bundle contains an unlisted, stale, or symlinked run root")
    semantic_by_seed: dict[str, dict[tuple[int, int], str]] = {}
    for item in sorted(runs, key=lambda value: (value["seed"], value["attempt"])):
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
        validate_bundle(args.bundle.expanduser())
    except Unverified as exc:
        print(f"UNVERIFIED: {exc}")
        return 3
    except (Failed, OSError, AttributeError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        print(f"FAILED: {exc}")
        return 1
    print("VERIFIED: independent Paper normal-overworld FULL evidence")
    print("Paper-only evidence; no Rivet parity claim is made.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

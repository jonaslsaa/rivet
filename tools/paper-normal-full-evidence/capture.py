#!/usr/bin/env python3
"""Produce independent Paper normal-overworld FULL evidence.

This driver intentionally has no Rivet input or parity path.  It builds the
pinned Paper source, creates a new isolated two-boot world root for each seed,
injects only level-33 forced tickets in the deterministic scheduler closure,
lets a tiny Paper plugin observe FULL chunks on the main thread, stops Paper
cleanly, and extracts only after the process has exited.

Runtime and capture output are restricted to
``/home/jonas/Rivet/working/output/paper-normal-full``.  Missing prerequisites
are reported as UNVERIFIED (exit 3); a Paper/process/capture failure is FAILED
(exit 1).  This command does not claim parity with Rivet.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import time
import zlib
from pathlib import Path
from typing import Any, Iterable

import validate as evidence_validate
from nbt import NbtError, Tag, encode, get_any, get_compound, parse

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DEFAULT_SOURCE = REPO / "working" / "Paper"
OUTPUT_ROOT = Path("/home/jonas/Rivet/working/output/paper-normal-full")
CONTRACT_PATH = HERE / "fixtures" / "contract.json"
PAPER_REVISION = "0a993450f129c4942c2a9ed45ba047412b4667cf"
PAPER_SHORT = PAPER_REVISION[:7]
JAVA_MAJOR = 25
PRODUCER = "paper-normal-full-capture/1"
EXPECTED_SEEDS = [
    "5207638315753790570",
    "12807505919197044144",
    "5246862266665176429",
    "3423572188437197996",
]
TARGETS = [(0, 0), (15, 15), (31, 31), (-1, -1), (-16, -16), (-31, -31), (-1, 0), (0, -1)]
RADIUS = 11
TICKET_LEVEL = 33
TICKET_TICKS_LEFT = -(1 << 63)
DATA_VERSION = 4903
STARLIGHT_VERSION_TAG = "starlight.light_version"
STARLIGHT_LIGHT_VERSION = 10
REGION_RE = re.compile(r"^r\.(-?\d+)\.(-?\d+)\.mca$")
PORT_RE = re.compile(r"Starting Minecraft server on \*:(\d+)")
QUERY_PORT_RE = re.compile(r"(?:GS4 status listener|Query listener).*?\*:(\d+)", re.I)


class Failed(RuntimeError):
    pass


class Unverified(RuntimeError):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_record(path: Path, relative: str) -> dict[str, Any]:
    data = path.read_bytes()
    return {"path": relative, "sha256": sha256(data), "bytes": len(data)}


def java_seed(seed: str) -> int:
    value = int(seed)
    if not 0 <= value < 1 << 64:
        raise Failed(f"seed is not an unsigned Java-long representation: {seed}")
    return value - (1 << 64) if value >= 1 << 63 else value


def scheduler_closure(targets: Iterable[tuple[int, int]], radius: int) -> list[tuple[int, int]]:
    """Derive the Paper full-ticket support set, then freeze its order.

    Paper's FULL generation closure is the target set expanded by the chunk
    scheduler's support radius.  The sorted order is part of this producer's
    contract so ticket NBT and probe arguments cannot depend on set iteration.
    """
    if radius < 0:
        raise Failed("negative scheduler support radius")
    return sorted(
        {(x + dx, z + dz) for x, z in targets for dx in range(-radius, radius + 1) for dz in range(-radius, radius + 1)}
    )


def _strict_decompress(data: bytes, *, gzip_stream: bool = False) -> bytes:
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


def read_region(path: Path, region_coordinates: tuple[int, int]) -> dict[tuple[int, int], bytes]:
    data = path.read_bytes()
    if len(data) < 8192 or len(data) % 4096:
        raise Failed(f"malformed region framing: {path}")
    result: dict[tuple[int, int], bytes] = {}
    used: set[int] = {0, 1}
    for index in range(1024):
        location = int.from_bytes(data[index * 4 : index * 4 + 4], "big")
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
        length = int.from_bytes(data[start : start + 4], "big")
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
            raw = _strict_decompress(payload, gzip_stream=True)
        elif codec == 2:
            raw = _strict_decompress(payload)
        elif codec == 3:
            raw = payload
        else:
            raise Failed(f"unsupported chunk compression {codec} in {path}")
        result[coordinate] = raw
    return result


def world_chunks(world: Path) -> dict[tuple[int, int], bytes]:
    region_dir = world / "dimensions" / "minecraft" / "overworld" / "region"
    if not region_dir.is_dir():
        raise Failed(f"overworld region directory is absent: {region_dir}")
    result: dict[tuple[int, int], bytes] = {}
    for path in sorted(region_dir.iterdir()):
        if path.name.endswith(".mcc"):
            continue
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


def inventory_paths(world: Path) -> list[Path]:
    roots = [
        world / "dimensions" / "minecraft" / "overworld" / "region",
        world / "dimensions" / "minecraft" / "overworld" / "poi",
        world / "dimensions" / "minecraft" / "overworld" / "entities",
    ]
    result: set[Path] = set()
    for root in roots:
        if root.is_dir():
            for path in root.rglob("*"):
                if path.is_symlink():
                    raise Failed(f"world inventory contains a symlink: {path}")
                if path.is_file():
                    result.add(path)
    for path in world.rglob("*.mcc"):
        if path.is_symlink() or not path.is_file():
            raise Failed(f"world inventory contains a non-regular .mcc: {path}")
        result.add(path)
    return sorted(result)


def walk_data_paths(world: Path) -> list[str]:
    roots = [
        world / "dimensions" / "minecraft" / "overworld" / name
        for name in ("region", "poi", "entities")
    ]
    result: list[str] = []
    for root in roots:
        if root.is_dir():
            for path in root.rglob("*"):
                if path.is_symlink():
                    raise Failed(f"preflight root contains a symlink: {path}")
                if path.is_file() or path.name.endswith(".mcc"):
                    result.append(str(path.relative_to(world)))
    for path in world.rglob("*.mcc"):
        if path.is_file() and str(path.relative_to(world)) not in result:
            result.append(str(path.relative_to(world)))
    return sorted(result)


def properties_text(seed: str, *, server_port: int = 0, query_port: int = 0) -> str:
    template = (HERE / "fixtures" / "server.properties").read_text()
    text = template.replace("<seed>", str(java_seed(seed)))
    return text.replace("server-port=0", f"server-port={server_port}").replace("query.port=0", f"query.port={query_port}")


def allocate_dynamic_ports() -> tuple[int, int]:
    ports: list[int] = []
    for _ in range(2):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind(("127.0.0.1", 0))
            ports.append(sock.getsockname()[1])
    if len(set(ports)) != 2 or any(port <= 0 for port in ports):
        raise Failed("OS did not provide two distinct dynamic ports")
    return ports[0], ports[1]


def parse_properties(text: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            result[key.strip()] = value.strip()
    return result


def _tag_compound(**values: Tag) -> Tag:
    return Tag(10, values)


def ticket_nbt(coordinates: list[tuple[int, int]]) -> bytes:
    tickets = []
    for x, z in coordinates:
        tickets.append(
            _tag_compound(
                type=Tag(8, "minecraft:forced"),
                chunk_pos=Tag(11, [x, z]),
                level=Tag(3, TICKET_LEVEL),
                ticks_left=Tag(4, TICKET_TICKS_LEFT),
            )
        )
    root = _tag_compound(
        DataVersion=Tag(3, DATA_VERSION),
        data=_tag_compound(tickets=Tag(9, (10, tickets))),
    )
    return _gzip_deterministic(encode(root))


def _gzip_deterministic(data: bytes) -> bytes:
    return gzip.compress(data, compresslevel=9, mtime=0)


def write_tickets(world: Path, coordinates: list[tuple[int, int]]) -> tuple[Path, str]:
    path = world / "dimensions/minecraft/overworld/data/minecraft/chunk_tickets.dat"
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = ticket_nbt(coordinates)
    path.write_bytes(encoded)
    return path, sha256(encoded)


def read_ticket_coordinates(path: Path) -> list[tuple[int, int]]:
    raw = _strict_decompress(path.read_bytes(), gzip_stream=True)
    root = parse(raw)
    if root.kind != 10:
        raise Failed("ticket root is not a compound")
    version = root.value.get("DataVersion")
    if version is None or version.kind != 3 or version.value != DATA_VERSION:
        raise Failed("ticket DataVersion is not pinned Paper 26.2")
    data = root.value.get("data")
    tickets = data.value.get("tickets") if data is not None and data.kind == 10 else None
    if tickets is None or tickets.kind != 9 or tickets.value[0] != 10:
        raise Failed("ticket list is malformed")
    coordinates: list[tuple[int, int]] = []
    for ticket in tickets.value[1]:
        if ticket.kind != 10:
            raise Failed("ticket list contains a non-compound")
        values = ticket.value
        if values.get("type") != Tag(8, "minecraft:forced"):
            raise Failed("ticket is not minecraft:forced")
        if values.get("level") != Tag(3, TICKET_LEVEL):
            raise Failed("ticket level is not exactly 33")
        if values.get("ticks_left") != Tag(4, TICKET_TICKS_LEFT):
            raise Failed("ticket ticks_left is not Long.MIN_VALUE")
        position = values.get("chunk_pos")
        if position is None or position.kind != 11 or len(position.value) != 2:
            raise Failed("ticket chunk_pos is malformed")
        coordinates.append((position.value[0], position.value[1]))
    return coordinates


def nbt_field(root: Tag, *path: str) -> Tag | None:
    return get_compound(root, *path)


def capture_settings(world: Path, seed: str, run: Path) -> dict[str, Any]:
    source = world / "dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat"
    if not source.is_file() or source.is_symlink():
        raise Failed(f"Paper worldgen settings source is absent: {source}")
    source_bytes = source.read_bytes()
    settings_root = parse(_strict_decompress(source_bytes, gzip_stream=True))
    data = settings_root.value.get("data") if settings_root.kind == 10 else None
    if data is None or data.kind != 10:
        raise Failed("worldgen settings has no data compound")
    version = settings_root.value.get("DataVersion")
    generated_seed = data.value.get("seed")
    structures = data.value.get("generate_structures")
    dimensions = data.value.get("dimensions")
    overworld = dimensions.value.get("minecraft:overworld") if dimensions and dimensions.kind == 10 else None
    generator = overworld.value.get("generator") if overworld and overworld.kind == 10 else None
    generator_type = generator.value.get("type") if generator and generator.kind == 10 else None
    if version != Tag(3, DATA_VERSION) or generated_seed != Tag(4, java_seed(seed)):
        raise Failed("worldgen settings DataVersion/seed is wrong")
    if structures != Tag(1, 1):
        raise Failed("worldgen settings does not enable structures")
    if overworld is None or generator_type != Tag(8, "minecraft:noise"):
        raise Failed("worldgen route is not normal-overworld noise")
    # The task contract names the root capture path.  Paper 26.2 stores its
    # SavedData source dimension-locally as world_gen_settings.dat; preserve an
    # exact read-only copy at the contract path and record both paths/hashes.
    contract_path = world / "data/minecraft/worldgen_settings.dat"
    if contract_path.is_symlink():
        raise Failed("worldgen settings contract path is symlinked")
    contract_path.parent.mkdir(parents=True, exist_ok=True)
    contract_path.write_bytes(source_bytes)
    if contract_path.read_bytes() != source_bytes:
        raise Failed("worldgen settings contract copy was not exact")
    return {
        "path": "world/data/minecraft/worldgen_settings.dat",
        "source_path": "world/dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat",
        "sha256": sha256(source_bytes),
        "bytes": len(source_bytes),
        "source_sha256": sha256(source_bytes),
        "source_bytes": len(source_bytes),
        "data_version": DATA_VERSION,
        "seed": str(java_seed(seed)),
        "generator": "minecraft:noise",
        "generate_structures": True,
    }


def _jar_manifest(jar: Path) -> dict[str, str]:
    import zipfile

    try:
        with zipfile.ZipFile(jar) as archive:
            text = archive.read("META-INF/MANIFEST.MF").decode("utf-8", "replace")
    except (OSError, KeyError, zipfile.BadZipFile) as exc:
        raise Failed(f"Paper jar manifest is unreadable: {jar}: {exc}") from exc
    result: dict[str, str] = {}
    for line in text.replace("\r\n", "\n").splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            result[key.strip()] = value.strip()
    return result


def toolchain(source: Path) -> tuple[Path, dict[str, str]]:
    java_home_value = os.environ.get("JAVA_HOME")
    if not java_home_value:
        raise Unverified("JAVA_HOME must explicitly name Temurin 25")
    java_home = Path(java_home_value).expanduser().resolve()
    java = java_home / "bin/java"
    javac = java_home / "bin/javac"
    if not java.is_file() or not os.access(java, os.X_OK) or not javac.is_file():
        raise Unverified(f"JAVA_HOME is not an executable JDK 25: {java_home}")
    env = os.environ.copy()
    env["PATH"] = f"{java_home / 'bin'}:{env.get('PATH', '')}"
    version = subprocess.run([str(java), "-version"], capture_output=True, text=True, env=env, check=False)
    version_text = version.stdout + version.stderr
    major_match = re.search(r'version "(\d+)', version_text)
    release_match = re.search(r'version "([^"]+)', version_text)
    if version.returncode != 0 or major_match is None or release_match is None or int(major_match.group(1)) != JAVA_MAJOR or "Temurin" not in version_text:
        raise Unverified("JAVA_HOME is not explicit Temurin 25")
    source_rev = subprocess.run(["git", "-C", str(source), "rev-parse", "HEAD"], capture_output=True, text=True, check=False)
    if source_rev.returncode != 0 or source_rev.stdout.strip() != PAPER_REVISION:
        raise Unverified(f"Paper source is not pinned to {PAPER_REVISION}")
    status = subprocess.run(["git", "-C", str(source), "status", "--porcelain"], capture_output=True, text=True, check=False)
    if status.returncode != 0 or status.stdout.strip():
        raise Unverified("Paper source is dirty; refusing to capture modified/self-authored sources")
    return java_home, {"vendor": "Eclipse Adoptium / Temurin", "version": release_match.group(1), "major": str(JAVA_MAJOR), "home": str(java_home)}


def build_paper(source: Path, java_home: Path) -> tuple[Path, dict[str, str]]:
    gradle = source / "gradlew"
    if not gradle.is_file():
        raise Unverified(f"Paper Gradle wrapper is absent: {gradle}")
    env = os.environ.copy()
    env["JAVA_HOME"] = str(java_home)
    env["PATH"] = f"{java_home / 'bin'}:{env.get('PATH', '')}"
    started = time.time_ns()
    result = subprocess.run(
        [str(gradle), "--no-daemon", "--console", "plain", ":paper-server:clean", ":paper-server:createPaperclipJar"],
        cwd=source,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise Failed(f"Paper build failed ({result.returncode}):\n{result.stdout[-4000:]}\n{result.stderr[-4000:]}")
    build_libs = source / "paper-server/build/libs"
    paperclip = build_libs / "paper-paperclip-26.2.local-SNAPSHOT.jar"
    server_jar = build_libs / "paper-server-26.2.local-SNAPSHOT.jar"
    if not paperclip.is_file() or paperclip.stat().st_mtime_ns < started or not server_jar.is_file() or server_jar.stat().st_mtime_ns < started:
        raise Failed("Paper build did not produce fresh pinned Paperclip and server jars")
    manifest = _jar_manifest(paperclip)
    server_manifest = _jar_manifest(server_jar)
    if manifest.get("Main-Class") != "io.papermc.paperclip.Main" or server_manifest.get("Git-Commit") != PAPER_SHORT:
        raise Failed("built jars do not prove the pinned Paper revision")
    return paperclip, {
        "path": str(paperclip),
        "sha256": sha256(paperclip.read_bytes()),
        "bytes": paperclip.stat().st_size,
        "manifest_main": manifest["Main-Class"],
        "source_root": str(source.resolve()),
        "source_revision": PAPER_REVISION,
        "built_after_ns": started,
        "source_server_jar": {
            "path": str(server_jar),
            "sha256": sha256(server_jar.read_bytes()),
            "bytes": server_jar.stat().st_size,
            "git_commit": server_manifest.get("Git-Commit"),
            "implementation_version": server_manifest.get("Implementation-Version"),
        },
    }


def write_configs(run: Path, seed: str, server_port: int, query_port: int) -> None:
    (run / "config").mkdir(parents=True, exist_ok=True)
    (run / "provenance/config").mkdir(parents=True, exist_ok=True)
    server = properties_text(seed, server_port=server_port, query_port=query_port)
    fixture_server = properties_text(seed)
    (run / "server.properties").write_text(server)
    (run / "provenance/server.properties").write_text(fixture_server)
    for name in ("paper-global.yml", "paper-world-defaults.yml"):
        data = (HERE / "fixtures" / name).read_bytes()
        (run / "config" / name).write_bytes(data)
        (run / "provenance/config" / name).write_bytes(data)
    (run / "eula.txt").write_text("eula=true\n")
    (run / "provenance/eula.txt").write_text("eula=true\n")


def read_log(path: Path) -> str:
    return path.read_text(errors="replace") if path.is_file() else ""


def observed_ports(log: str) -> dict[str, int]:
    server = PORT_RE.findall(log)
    query = QUERY_PORT_RE.findall(log)
    return {"server": int(server[-1]) if server else 0, "query": int(query[-1]) if query else 0}


def require_dynamic_server_port(observed: dict[str, int], configured: int, label: str) -> None:
    port = observed.get("server", 0)
    if configured <= 0 or port <= 0:
        raise Failed(f"{label} did not prove a nonzero dynamic server port")
    if port != configured:
        raise Failed(f"{label} server port {port} differs from configured dynamic port {configured}")


def _wait_for(log: Path, *, ready: str, failed: str | None, timeout: float) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        text = read_log(log)
        if failed and failed in text:
            raise Failed(f"Paper log reported {failed}")
        if ready in text:
            return text
        time.sleep(0.25)
    raise Failed(f"timed out waiting for {ready} in {log}")


def boot_and_stop(
    run: Path,
    paperclip: Path,
    java_home: Path,
    log_name: str,
    *,
    plugin_args: list[str] | None = None,
    timeout: float = 600.0,
) -> dict[str, Any]:
    log_path = run / log_name
    env = os.environ.copy()
    env["JAVA_HOME"] = str(java_home)
    env["PATH"] = f"{java_home / 'bin'}:{env.get('PATH', '')}"
    command = [str(java_home / "bin/java"), "-Xms512M", "-Xmx2G"]
    command.extend(plugin_args or [])
    command.extend(["-jar", str(paperclip), "nogui"])
    started_ns = time.time_ns()
    with log_path.open("wb") as log:
        process = subprocess.Popen(command, cwd=run, env=env, stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
        try:
            _wait_for(log_path, ready="Done (", failed="RIVET_PROBE_FAILED" if plugin_args else None, timeout=timeout)
            if plugin_args:
                _wait_for(log_path, ready="RIVET_PROBE_READY", failed="RIVET_PROBE_FAILED", timeout=timeout)
            stop_sent_ns = time.time_ns()
            if process.stdin is None:
                raise Failed("Paper stdin was not available for graceful stop")
            process.stdin.write(b"stop\n")
            process.stdin.flush()
            process.stdin.close()
            try:
                process.wait(timeout=timeout)
            except subprocess.TimeoutExpired as exc:
                process.send_signal(signal.SIGTERM)
                try:
                    process.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                raise Failed(f"Paper did not stop gracefully: {log_path}") from exc
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    ended_ns = time.time_ns()
    text = read_log(log_path)
    if process.returncode != 0:
        raise Failed(f"Paper exited {process.returncode}: {log_path}")
    if "All dimensions are saved" not in text or "Stopping server" not in text:
        raise Failed(f"Paper did not prove a graceful save in {log_path}")
    return {
        "log": log_name,
        "pid": process.pid,
        "started_ns": started_ns,
        "stop_command_ns": stop_sent_ns,
        "ended_ns": ended_ns,
        "exit_code": process.returncode,
        "clean_stop": True,
        "ports": observed_ports(text),
    }


def compile_probe(run: Path, java_home: Path) -> tuple[Path, dict[str, Any]]:
    runtime = run / "versions/26.2/paper-26.2.jar"
    libraries = sorted((run / "libraries").rglob("*.jar"))
    if not runtime.is_file() or not libraries:
        raise Failed("fresh Paperclip boot did not materialize runtime jar and libraries")
    runtime_manifest = _jar_manifest(runtime)
    if runtime_manifest.get("Git-Commit") != PAPER_SHORT:
        raise Failed("fresh materialized Paper runtime is not the pinned Paper revision")
    classes = run / ".probe-classes"
    if classes.exists():
        shutil.rmtree(classes)
    classes.mkdir(parents=True)
    plugin_dir = run / "plugins"
    plugin_dir.mkdir(parents=True, exist_ok=True)
    cp = os.pathsep.join([str(runtime), *(str(path) for path in libraries)])
    env = os.environ.copy()
    env["JAVA_HOME"] = str(java_home)
    env["PATH"] = f"{java_home / 'bin'}:{env.get('PATH', '')}"
    result = subprocess.run(
        [str(java_home / "bin/javac"), "-cp", cp, "-d", str(classes), str(HERE / "src/PaperNormalFullProbe.java")],
        cwd=HERE,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise Failed(f"Paper probe compilation failed:\n{result.stdout}\n{result.stderr}")
    shutil.copy2(HERE / "src/plugin.yml", classes / "plugin.yml")
    jar = plugin_dir / "RivetPaperNormalFullProbe.jar"
    result = subprocess.run([str(java_home / "bin/jar"), "cf", str(jar), "-C", str(classes), "."], cwd=HERE, text=True, capture_output=True, check=False, env=env)
    if result.returncode != 0:
        raise Failed(f"Paper probe jar creation failed: {result.stderr}")
    probe_source = HERE / "src/PaperNormalFullProbe.java"
    probe_plugin = HERE / "src/plugin.yml"
    return jar, {
        "runtime": {
            "path": str(runtime.relative_to(run)),
            "sha256": sha256(runtime.read_bytes()),
            "bytes": runtime.stat().st_size,
            "git_commit": runtime_manifest.get("Git-Commit"),
            "implementation_version": runtime_manifest.get("Implementation-Version"),
            "libraries_count": len(libraries),
        },
        "artifact": {
            "path": str(jar.relative_to(run)),
            "sha256": sha256(jar.read_bytes()),
            "bytes": jar.stat().st_size,
        },
        "source_sha256": sha256(probe_source.read_bytes()),
        "plugin_yml_sha256": sha256(probe_plugin.read_bytes()),
    }


def copy_fixture_provenance(run: Path) -> None:
    contract = CONTRACT_PATH.read_bytes()
    (run / "provenance/contract.json").write_bytes(contract)


def world_tree_signature(world: Path) -> str:
    rows: list[str] = []
    for path in sorted(world.rglob("*")):
        if path.is_symlink():
            raise Failed(f"world tree contains a symlink: {path}")
        if path.is_file():
            stat = path.stat()
            rows.append(f"{path.relative_to(world)}\0{stat.st_size}\0{stat.st_mtime_ns}")
    return sha256("\n".join(rows).encode())


def reset_world_support_data(world: Path) -> tuple[list[str], list[str], list[str]]:
    before = walk_data_paths(world)
    tickets_before = sorted(str(path.relative_to(world)) for path in world.rglob("chunk_tickets.dat") if path.is_file())
    overworld = world / "dimensions/minecraft/overworld"
    for name in ("region", "poi", "entities"):
        path = overworld / name
        if path.is_symlink():
            raise Failed(f"world support root is a symlink: {path}")
        if path.exists():
            shutil.rmtree(path)
    for path in sorted(world.rglob("*.mcc")):
        if path.is_symlink():
            raise Failed(f"world .mcc is a symlink: {path}")
        path.unlink()
    for path in sorted(world.rglob("chunk_tickets.dat")):
        if path.is_symlink():
            raise Failed(f"world ticket file is a symlink: {path}")
        path.unlink()
    after = walk_data_paths(world)
    return before, tickets_before, after


def persisted_light_correct(root: Tag) -> bool:
    version = get_any(root, STARLIGHT_VERSION_TAG)
    return version is not None and version.kind == 3 and version.value == STARLIGHT_LIGHT_VERSION


def chunk_details(raw: bytes, coordinate: tuple[int, int], *, target: bool) -> dict[str, Any]:
    try:
        root = parse(raw)
    except NbtError as exc:
        raise Failed(f"malformed/trailing chunk NBT at {coordinate}: {exc}") from exc
    if root.kind != 10 or root.value.get("DataVersion") != Tag(3, DATA_VERSION) or root.value.get("xPos") != Tag(3, coordinate[0]) or root.value.get("zPos") != Tag(3, coordinate[1]):
        raise Failed(f"{coordinate} has wrong chunk DataVersion or coordinates")
    status = get_any(root, "Status")
    light = get_any(root, "isLightOn")
    heightmaps = get_any(root, "Heightmaps")
    if status is None or status.kind != 8 or status.value != "minecraft:full":
        raise Failed(f"{coordinate} did not serialize minecraft:full")
    if light is None or light.kind != 1:
        raise Failed(f"{coordinate} did not serialize an isLightOn byte")
    light_correct = persisted_light_correct(root)
    if target and not light_correct:
        raise Failed(f"{coordinate} did not serialize isLightOn=true")
    if heightmaps is None or heightmaps.kind != 10:
        raise Failed(f"{coordinate} has no Heightmaps compound")
    required = ("WORLD_SURFACE", "MOTION_BLOCKING", "OCEAN_FLOOR")
    selected: dict[str, Tag] = {}
    for name in required:
        item = heightmaps.value.get(name)
        if item is None or item.kind != 12 or len(item.value) != 37:
            raise Failed(f"{coordinate} has malformed {name} heightmap")
        selected[name] = item
    decoded: dict[str, list[int]] = {}
    for name, item in selected.items():
        values: list[int] = []
        for word in item.value:
            for slot in range(7):
                values.append((word >> (slot * 9)) & 0x1FF)
        values = values[:256]
        if len(values) != 256 or any(value > 384 for value in values):
            raise Failed(f"{coordinate} has out-of-range {name} heightmap")
        decoded[name] = values
    sections = get_any(root, "sections")
    if sections is None or sections.kind != 9:
        raise Failed(f"{coordinate} has no sections list")
    palette_names: set[str] = set()
    for section in sections.value[1]:
        if section.kind != 10:
            raise Failed(f"{coordinate} section is not a compound")
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
                    palette_names.add(name.value)
    if target and len(palette_names) < 6:
        raise Failed(f"{coordinate} has a flat/under-varied block palette")
    return {
        "status": status.value,
        "light_correct": light_correct,
        "heightmaps": list(required),
        "heightmap_ranges": {name: [min(values), max(values)] for name, values in decoded.items()},
        "palette_names": sorted(palette_names),
        "raw_sha256": sha256(raw),
        "raw_bytes": len(raw),
        "semantic_sha256": sha256(__import__("nbt").canonical_without_dynamic(root)),
    }


def extract_run(
    run: Path,
    seed: str,
    attempt: int,
    token: str,
    boot1: dict[str, Any],
    capture: dict[str, Any],
    paper_info: dict[str, Any],
    java_info: dict[str, str],
    closure: list[tuple[int, int]],
    settings: dict[str, Any],
    preflight: dict[str, Any],
    ticket_path: Path,
    injected_ticket_sha: str,
    configured_ports: tuple[int, int],
) -> None:
    extraction_started_ns = time.time_ns()
    if capture["ended_ns"] > extraction_started_ns:
        raise Failed("post-exit extraction clock ordering is invalid")
    world = run / "world"
    if world.is_symlink() or not world.is_dir():
        raise Failed("Paper world root is absent or symlinked")
    signature_before = world_tree_signature(world)
    coordinates = world_chunks(world)
    if set(coordinates) < set(closure):
        raise Failed(f"Paper saved only {len(coordinates)} chunks; closure has {len(closure)}")
    chunks_dir = run / "chunks"
    chunks_dir.mkdir(parents=True, exist_ok=True)
    chunks: list[dict[str, Any]] = []
    for x, z in closure:
        raw = coordinates[(x, z)]
        relative = f"chunks/{x}.{z}.nbt"
        path = run / relative
        path.write_bytes(raw)
        details = chunk_details(raw, (x, z), target=(x, z) in TARGETS)
        details.update({"x": x, "z": z, "raw_path": relative})
        chunks.append(details)
    ticket_coordinates = read_ticket_coordinates(ticket_path)
    if ticket_coordinates != closure:
        raise Failed("post-exit forced ticket order/set differs from scheduler closure")
    inventory: list[dict[str, Any]] = []
    for path in inventory_paths(world):
        relative = str(path.relative_to(world))
        kind = "region" if "/region/" in f"/{relative}" else "poi" if "/poi/" in f"/{relative}" else "entities" if "/entities/" in f"/{relative}" else "mcc" if path.suffix == ".mcc" else "other"
        entry = file_record(path, relative)
        entry["kind"] = kind
        inventory.append(entry)
    signature_after = world_tree_signature(world)
    if signature_before != signature_after:
        raise Failed("world changed during post-exit read-only extraction")
    probe_path = run / "probe.json"
    if probe_path.is_symlink() or not probe_path.is_file():
        raise Failed("Paper probe did not leave a regular probe.json")
    try:
        probe = json.loads(probe_path.read_text())
    except json.JSONDecodeError as exc:
        raise Failed(f"probe.json is malformed: {exc}") from exc
    if probe.get("token") != token or probe.get("main_thread") is not True or probe.get("closure_count") != len(closure):
        raise Failed("probe provenance/count is incomplete")
    process_log = run / "server.log"
    if not process_log.is_file():
        raise Failed("capture log is absent")
    manifest = {
        "format": 1,
        "kind": json.loads(CONTRACT_PATH.read_text())["kind"],
        "producer": PRODUCER,
        "parity_claim": None,
        "rivet_commit": None,
        "paper_revision": PAPER_REVISION,
        "paper_jar": paper_info,
        "probe_artifact": paper_info.get("probe_artifact"),
        "java": {"vendor": java_info["vendor"], "version": java_info["version"], "major": int(java_info["major"]), "home": java_info["home"]},
        "seed": seed,
        "java_seed": str(java_seed(seed)),
        "attempt": attempt,
        "run_id": run.name,
        "run_root": str(run.resolve()),
        "capture_token": token,
        "dimension": "minecraft:overworld",
        "level_type": "minecraft:normal",
        "generate_structures": True,
        "simulation": {"random_tick_speed": 0, "do_daylight_cycle": False, "do_weather_cycle": False, "do_mob_spawning": False, "spawn_limits": 0},
        "targets": [[x, z] for x, z in TARGETS],
        "closure": {"radius": RADIUS, "order": "lexicographic x,z after Paper scheduler radius-11 expansion", "coordinates": [[x, z] for x, z in closure], "sha256": sha256(json.dumps(closure, separators=(",", ":")).encode())},
        "ticket": {"type": "minecraft:forced", "level": TICKET_LEVEL, "ticks_left": TICKET_TICKS_LEFT, "coordinates": [[x, z] for x, z in closure], "injected_sha256": injected_ticket_sha, "post_exit_sha256": sha256(ticket_path.read_bytes()), "held_through_stop": True},
        "ports": {"fixture_server": 0, "fixture_query": 0, "configured_server": configured_ports[0], "configured_query": configured_ports[1], "boot1": boot1["ports"], "capture": capture["ports"]},
        "logs": {
            "world_create": file_record(run / "server-create.log", "server-create.log"),
            "capture": file_record(run / "server.log", "server.log"),
        },
        "config": {
            "server_properties": file_record(run / "provenance/server.properties", "provenance/server.properties"),
            "runtime_server_properties": file_record(run / "server.properties", "server.properties"),
            "paper_global": file_record(run / "provenance/config/paper-global.yml", "provenance/config/paper-global.yml"),
            "paper_world_defaults": file_record(run / "provenance/config/paper-world-defaults.yml", "provenance/config/paper-world-defaults.yml"),
            "runtime_paper_global": file_record(run / "config/paper-global.yml", "config/paper-global.yml"),
            "runtime_paper_world_defaults": file_record(run / "config/paper-world-defaults.yml", "config/paper-world-defaults.yml"),
            "eula": file_record(run / "provenance/eula.txt", "provenance/eula.txt"),
            "runtime_eula": file_record(run / "eula.txt", "eula.txt"),
        },
        "worldgen_settings": settings,
        "preflight": preflight,
        "process": {"boot1": boot1, "capture": capture, "log": "server.log", "exit_code": capture["exit_code"], "clean_stop": capture["clean_stop"], "probe_ready_before_stop": True},
        "probe": {"path": "probe.json", "producer": probe.get("producer"), "main_thread": probe.get("main_thread"), "closure_count": probe.get("closure_count"), "targets": probe.get("targets"), "simulation_frozen": probe.get("simulation_frozen"), "token": probe.get("token")},
        "chunks": chunks,
        "inventory": inventory,
        "extraction": {"post_exit_read_only": True, "started_ns": extraction_started_ns, "world_signature_before": signature_before, "world_signature_after": signature_after},
        "semantic_hash_dynamic_fields": ["InhabitedTime", "LastUpdate"],
    }
    (run / "capture.json").write_text(json.dumps(manifest, indent=2, sort_keys=False) + "\n")


def run_one(
    run: Path,
    seed: str,
    attempt: int,
    paperclip: Path,
    paper_info: dict[str, Any],
    java_home: Path,
    java_info: dict[str, str],
    closure: list[tuple[int, int]],
    timeout: float,
) -> None:
    run.mkdir(parents=True)
    if any(run.iterdir()):
        raise Failed("fresh run root is not empty before Paper boot")
    configured_ports = allocate_dynamic_ports()
    write_configs(run, seed, *configured_ports)
    copy_fixture_provenance(run)
    token = secrets.token_hex(32)
    (run / "capture.token").write_text(token + "\n")
    boot1 = boot_and_stop(run, paperclip, java_home, "server-create.log", timeout=timeout)
    require_dynamic_server_port(boot1["ports"], configured_ports[0], "Paper world-creation boot")
    world = run / "world"
    settings = capture_settings(world, seed, run)
    before, tickets_before, after = reset_world_support_data(world)
    if after:
        raise Failed(f"preflight support/ticket roots were not empty: {after}")
    preflight = {
        "fresh_isolated_world_root": True,
        "world_absent_before_boot1": True,
        "boot1_created_world": True,
        "before_injection_data_paths": after,
        "boot1_generated_data_paths_removed": before,
        "before_injection_ticket_paths": [],
        "boot1_generated_ticket_paths_removed": tickets_before,
        "no_preexisting_target_support_data": not after,
        "no_preexisting_tickets": not tickets_before,
        "reset_before_ticket_injection": True,
    }
    _, probe_info = compile_probe(run, java_home)
    run_paper_info = dict(paper_info)
    run_paper_info["materialized_runtime"] = probe_info["runtime"]
    run_paper_info["probe_artifact"] = probe_info["artifact"]
    run_paper_info["probe_source_sha256"] = probe_info["source_sha256"]
    run_paper_info["probe_plugin_yml_sha256"] = probe_info["plugin_yml_sha256"]
    ticket_path, injected_sha = write_tickets(world, closure)
    encoded_closure = ";".join(f"{x},{z}" for x, z in closure)
    encoded_targets = ";".join(f"{x},{z}" for x, z in TARGETS)
    plugin_args = [
        f"-Drivet.probe.output={run / 'probe.json'}",
        f"-Drivet.capture.token={token}",
        f"-Drivet.probe.closure={encoded_closure}",
        f"-Drivet.probe.targets={encoded_targets}",
    ]
    capture = boot_and_stop(run, paperclip, java_home, "server.log", plugin_args=plugin_args, timeout=timeout)
    require_dynamic_server_port(capture["ports"], configured_ports[0], "Paper FULL capture boot")
    extract_run(run, seed, attempt, token, boot1, capture, run_paper_info, java_info, closure, settings, preflight, ticket_path, injected_sha, configured_ports)
    (run / "driver.log").write_text(f"RIVET_TICKETS_INJECTED={injected_sha}\nRIVET_CAPTURE_STOP_EXIT=0\n")
    # The capture manifest is deliberately rewritten only before returning so
    # the driver log can itself be listed as provenance without touching world.
    manifest = json.loads((run / "capture.json").read_text())
    manifest["driver_log"] = file_record(run / "driver.log", "driver.log")
    (run / "capture.json").write_text(json.dumps(manifest, indent=2) + "\n")


def ensure_output(path: Path) -> Path:
    resolved = path.expanduser().resolve()
    base = OUTPUT_ROOT.resolve()
    try:
        resolved.relative_to(base)
    except ValueError as exc:
        raise Failed(f"capture output must be under {base}, not {resolved}") from exc
    if resolved.exists():
        raise Failed(f"refusing to reuse pre-existing capture output: {resolved}")
    resolved.mkdir(parents=True)
    return resolved


def capture_bundle(output: Path, source: Path, attempts: int, timeout: float) -> Path:
    if attempts != 3:
        raise Failed("exactly three isolated roots per seed are required by the corpus contract")
    java_home, java_info = toolchain(source)
    paperclip, paper_info = build_paper(source, java_home)
    bundle = ensure_output(output)
    closure = scheduler_closure(TARGETS, RADIUS)
    runs: list[dict[str, Any]] = []
    for seed in EXPECTED_SEEDS:
        for attempt in range(1, attempts + 1):
            run = bundle / "runs" / seed / str(attempt)
            try:
                run.parent.mkdir(parents=True, exist_ok=True)
                run_one(run, seed, attempt, paperclip, paper_info, java_home, java_info, closure, timeout)
            except Exception:
                # Keep the failed root as diagnostics but never turn it into a
                # bundle entry that a validator could mistake for evidence.
                raise
            runs.append({"seed": seed, "attempt": attempt, "path": str(run.relative_to(bundle))})
    contract_bytes = CONTRACT_PATH.read_bytes()
    bundle_manifest = {
        "format": 1,
        "kind": json.loads(contract_bytes)["kind"],
        "producer": PRODUCER,
        "parity_claim": None,
        "rivet_commit": None,
        "paper_revision": PAPER_REVISION,
        "java": {"vendor": java_info["vendor"], "version": java_info["version"], "major": int(java_info["major"]), "home": java_info["home"]},
        "contract_sha256": sha256(contract_bytes),
        "seeds": EXPECTED_SEEDS,
        "attempts_per_seed": attempts,
        "targets": [[x, z] for x, z in TARGETS],
        "closure_radius": RADIUS,
        "runs": runs,
    }
    bundle_manifest_path = bundle / "bundle.json"
    bundle_manifest_path.write_text(json.dumps(bundle_manifest, indent=2) + "\n")
    try:
        evidence_validate.validate_bundle(bundle)
    except (evidence_validate.Failed, evidence_validate.Unverified, OSError, AttributeError, KeyError, TypeError, ValueError) as exc:
        bundle_manifest_path.unlink(missing_ok=True)
        raise Failed(f"captured bundle did not pass its fail-closed validator: {exc}") from exc
    return bundle


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=OUTPUT_ROOT / "bundle")
    parser.add_argument("--paper-source", type=Path, default=Path(os.environ.get("RIVET_PAPER_SOURCE", DEFAULT_SOURCE)))
    parser.add_argument("--attempts", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=3600.0)
    args = parser.parse_args(argv)
    try:
        bundle = capture_bundle(args.output, args.paper_source.expanduser().resolve(), args.attempts, args.timeout)
    except Unverified as exc:
        print(f"UNVERIFIED: {exc}")
        return 3
    except (Failed, OSError, subprocess.SubprocessError) as exc:
        print(f"FAILED: {exc}")
        return 1
    print(f"CAPTURED: genuine Paper normal-overworld FULL evidence at {bundle}")
    print("This evidence is Paper-only and makes no Rivet parity claim.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

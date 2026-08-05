#!/usr/bin/env python3
"""Builds MANIFEST.tsv (package-level units) from the Paper sources in working/Paper.

Parses package declarations and imports from every Java file, groups them into
package-level units, maps packages to target crates, records inter-package
dependencies, condenses cycles (Tarjan SCC), and assigns each unit a topological
`wave` number: a unit is safe to translate once every unit with a lower wave in
its dependency closure is done. Units in the same SCC share a `cycle` id and must
be scheduled together (or split first — see the needs_split flag).

`--split-nbt` refines the net.minecraft.nbt package into class-cluster units
(epic #9): one irreducible SCC (the sealed Tag hierarchy + visitor interfaces +
type system + accounter + exceptions, which map to Rust enums and cannot split
without stubs) plus the one-directional downstream layers (io, ops, snbt, text,
utils, visitors). The split used to live in scripts/split_nbt_units.py; it is
folded here so re-running the analyzer is idempotent with the split.

Every unit also gets a `java_paths` column (comma-joined file paths relative to
the source root). This gives each split unit its concrete file set, so a unit's
dep on a package it shares (net.minecraft.nbt) is a real dep on the sibling unit
that owns those files — never a self-dependency.

Known limitation: same-package class references need no import in Java, so the
package-level graph cannot see intra-package edges; the nbt split's boundaries
and deps are therefore authored data (NBT_UNITS below), validated against disk.
"""

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
ROOTS = {
    "minecraft": REPO / "working/Paper/paper-server/src/minecraft/java",
    "paper-server": REPO / "working/Paper/paper-server/src/main/java",
    "paper-api": REPO / "working/Paper/paper-api/src/main/java",
}

CRATE_RULES = [
    ("com.mojang.brigadier", "rivet-brigadier"),
    ("com.mojang.serialization", "rivet-serialization"),
    ("com.mojang.datafixers", "rivet-serialization"),
    ("net.minecraft.nbt", "rivet-nbt"),
    ("net.minecraft.network.chat", "rivet-text"),
    ("net.minecraft.network", "rivet-protocol"),
    ("net.minecraft.util", "rivet-util"),
    ("net.minecraft.core", "rivet-registry"),
    ("net.minecraft.resources", "rivet-registry"),
    ("net.minecraft.tags", "rivet-registry"),
    ("net.minecraft.data", "rivet-registry"),
    ("net.minecraft.world.entity", "rivet-entity"),
    ("net.minecraft.world.level", "rivet-world"),
    ("net.minecraft.world", "rivet-world"),
    ("net.minecraft.gametest", "rivet-oracle"),
    ("net.minecraft.server", "rivet-server"),
    ("net.minecraft.commands", "rivet-server"),
    ("net.minecraft", "rivet-server"),
    ("org.bukkit.craftbukkit", "rivet-server"),
    ("io.papermc.paper", "rivet-api"),
    ("org.bukkit", "rivet-api"),
    ("org.spigotmc", "rivet-server"),
    ("com.destroystokyo.paper", "rivet-api"),
]

PKG_RE = re.compile(r"^\s*package\s+([\w.]+)\s*;", re.M)
IMP_RE = re.compile(r"^\s*import\s+(?:static\s+)?([\w.]+)\s*;", re.M)
INTERNAL_PREFIXES = tuple(p for p, _ in CRATE_RULES)
SPLIT_FILE_THRESHOLD = 15

# Class-cluster split of net.minecraft.nbt (epic #9): unit id -> metadata.
# (java_package, [file names relative to the nbt package dir], wave, cycle, [deps]).
# The core SCC depends only on externals; every downstream layer depends on the
# core (`net.minecraft.nbt`) plus its own externals. deps are comma-joined java
# packages; the core is referenced as `net.minecraft.nbt` (resolved by the
# wave-picker to the `mc.nbt` unit via the derived unit id).
NBT_UNITS = {
    "mc.nbt": (
        "net.minecraft.nbt",
        [
            "Tag.java", "CollectionTag.java", "PrimitiveTag.java", "NumericTag.java",
            "EndTag.java", "ByteTag.java", "ShortTag.java", "IntTag.java",
            "LongTag.java", "FloatTag.java", "DoubleTag.java", "StringTag.java",
            "ByteArrayTag.java", "IntArrayTag.java", "LongArrayTag.java",
            "ListTag.java", "CompoundTag.java", "TagVisitor.java",
            "StreamTagVisitor.java", "TagType.java", "TagTypes.java",
            "NbtAccounter.java", "NbtException.java", "NbtAccounterException.java",
            "NbtFormatException.java", "ReportedNbtException.java",
            "package-info.java",
        ],
        3, "27",
        ["com.mojang.serialization", "net.minecraft", "net.minecraft.util"],
    ),
    "mc.nbt.io": (
        "net.minecraft.nbt",
        ["NbtIo.java"],
        4, "",
        ["net.minecraft.nbt", "net.minecraft", "net.minecraft.util"],
    ),
    "mc.nbt.ops": (
        "net.minecraft.nbt",
        ["NbtOps.java"],
        4, "",
        ["net.minecraft.nbt", "com.mojang.datafixers.util", "com.mojang.serialization",
         "net.minecraft.util"],
    ),
    "mc.nbt.snbt": (
        "net.minecraft.nbt",
        ["SnbtGrammar.java", "SnbtOperations.java", "TagParser.java",
         "StringTagVisitor.java", "SnbtPrinterTagVisitor.java"],
        4, "",
        ["net.minecraft.nbt", "com.mojang.brigadier", "com.mojang.brigadier.exceptions",
         "com.mojang.serialization", "net.minecraft.core", "net.minecraft.network.chat",
         "net.minecraft.util", "net.minecraft.util.parsing.packrat",
         "net.minecraft.util.parsing.packrat.commands"],
    ),
    "mc.nbt.text": (
        "net.minecraft.nbt",
        ["TextComponentTagVisitor.java"],
        4, "",
        ["net.minecraft.nbt", "net.minecraft", "net.minecraft.network.chat"],
    ),
    "mc.nbt.utils": (
        "net.minecraft.nbt",
        ["NbtUtils.java"],
        4, "",
        ["net.minecraft.nbt", "com.mojang.brigadier.exceptions", "com.mojang.serialization",
         "net.minecraft", "net.minecraft.core", "net.minecraft.core.registries",
         "net.minecraft.network.chat", "net.minecraft.resources", "net.minecraft.util",
         "net.minecraft.world.level.block", "net.minecraft.world.level.block.state",
         "net.minecraft.world.level.block.state.properties", "net.minecraft.world.level.material",
         "net.minecraft.world.level.storage"],
    ),
    "mc.nbt.visitors": (
        "net.minecraft.nbt.visitors",
        ["visitors/CollectFields.java", "visitors/CollectToTag.java",
         "visitors/FieldSelector.java", "visitors/FieldTree.java",
         "visitors/SkipAll.java", "visitors/SkipFields.java",
         "visitors/package-info.java"],
        4, "",
        ["net.minecraft.nbt"],
    ),
}
NBT_SPLIT_PACKAGES = {"net.minecraft.nbt", "net.minecraft.nbt.visitors"}
NBT_DIR = Path("net/minecraft/nbt")


def crate_for(pkg: str) -> str:
    if pkg == "net.minecraft":  # root-package classes only; subpackages match CRATE_RULES
        return "rivet-core"
    for prefix, crate in CRATE_RULES:
        if pkg == prefix or pkg.startswith(prefix + "."):
            return crate
    return "rivet-server"


def derive_id(pkg: str) -> str:
    return (
        pkg.replace("net.minecraft.", "mc.")
        .replace("org.bukkit.", "bukkit.")
        .replace("io.papermc.paper.", "paper.")
    )


def tarjan_scc(nodes: list[str], edges: dict[str, set[str]]) -> list[list[str]]:
    index: dict[str, int] = {}
    lowlink: dict[str, int] = {}
    on_stack: set[str] = set()
    stack: list[str] = []
    sccs: list[list[str]] = []
    counter = 0

    for root in nodes:
        if root in index:
            continue
        # Iterative Tarjan: recursion overflows on ~700-node dense graphs.
        work: list[tuple[str, iter]] = [(root, iter(sorted(edges.get(root, ()))))]
        index[root] = lowlink[root] = counter
        counter += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            node, it = work[-1]
            advanced = False
            for nxt in it:
                if nxt not in index:
                    index[nxt] = lowlink[nxt] = counter
                    counter += 1
                    stack.append(nxt)
                    on_stack.add(nxt)
                    work.append((nxt, iter(sorted(edges.get(nxt, ())))))
                    advanced = True
                    break
                if nxt in on_stack:
                    lowlink[node] = min(lowlink[node], index[nxt])
            if advanced:
                continue
            work.pop()
            if work:
                parent = work[-1][0]
                lowlink[parent] = min(lowlink[parent], lowlink[node])
            if lowlink[node] == index[node]:
                scc = []
                while True:
                    w = stack.pop()
                    on_stack.discard(w)
                    scc.append(w)
                    if w == node:
                        break
                sccs.append(scc)
    return sccs


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--split-nbt", action="store_true",
        help="split net.minecraft.nbt into class-cluster units (idempotent)",
    )
    args = parser.parse_args()

    # A package can span several source roots (e.g. moonrise code appears under
    # both minecraft/java and main/java), so each file carries its own root.
    pkg_files: dict[str, list[tuple[str, Path]]] = defaultdict(list)
    pkg_loc: dict[str, int] = defaultdict(int)
    pkg_root: dict[str, str] = {}
    pkg_deps: dict[str, set[str]] = defaultdict(set)

    for root_name, root in ROOTS.items():
        if not root.is_dir():
            sys.exit(f"missing source root: {root} (run Paper's gradle setup first)")
        for f in root.rglob("*.java"):
            text = f.read_text(encoding="utf-8", errors="replace")
            m = PKG_RE.search(text)
            if not m:
                continue
            pkg = m.group(1)
            pkg_files[pkg].append((root_name, f))
            pkg_loc[pkg] += text.count("\n")
            pkg_root.setdefault(pkg, root_name)
            for imp in IMP_RE.findall(text):
                if imp.startswith(INTERNAL_PREFIXES):
                    pkg_deps[pkg].add(imp.rsplit(".", 1)[0])

    known = set(pkg_files)
    deps = {p: {d for d in pkg_deps[p] if d in known and d != p} for p in known}

    sccs = tarjan_scc(sorted(known), deps)
    scc_of: dict[str, int] = {}
    for i, scc in enumerate(sccs):
        for pkg in scc:
            scc_of[pkg] = i

    # Condensation + longest-path depth = wave number.
    scc_deps: dict[int, set[int]] = defaultdict(set)
    for pkg, ds in deps.items():
        for d in ds:
            if scc_of[d] != scc_of[pkg]:
                scc_deps[scc_of[pkg]].add(scc_of[d])

    wave: dict[int, int] = {}

    def depth(s: int) -> int:
        if s not in wave:
            wave[s] = 1 + max((depth(d) for d in scc_deps[s]), default=-1)
        return wave[s]

    sys.setrecursionlimit(100_000)
    for s in range(len(sccs)):
        depth(s)

    cycle_members = {i: scc for i, scc in enumerate(sccs) if len(scc) > 1}
    header = (
        "id\tjava_package\tjava_paths\tsource_root\tfiles\tloc\tcrate\twave\tcycle"
        "\tneeds_split\tdeps\tstatus\tattempts\tnotes"
    )

    def java_paths(pkg: str) -> str:
        return ",".join(
            sorted(f.relative_to(ROOTS[root]).as_posix() for root, f in pkg_files[pkg])
        )

    def package_row(pkg: str) -> list[str]:
        in_cycle = scc_of[pkg] if scc_of[pkg] in cycle_members else ""
        needs_split = "yes" if (
            len(pkg_files[pkg]) > SPLIT_FILE_THRESHOLD or in_cycle != ""
        ) else ""
        return [
            derive_id(pkg), pkg, java_paths(pkg), pkg_root[pkg],
            str(len(pkg_files[pkg])), str(pkg_loc[pkg]), crate_for(pkg),
            str(wave[scc_of[pkg]]), str(in_cycle), needs_split,
            ",".join(sorted(deps[pkg])), "pending", "0", "",
        ]

    def split_row() -> list[str]:
        root_dir = ROOTS["minecraft"]
        nbt_dir = root_dir / NBT_DIR
        rows = []
        for unit_id, (pkg, files, wv, cyc, deps_list) in NBT_UNITS.items():
            loc = 0
            paths: list[str] = []
            for f in files:
                path = nbt_dir / f
                if not path.is_file():
                    sys.exit(f"--split-nbt: missing file for unit {unit_id}: {path}")
                loc += sum(1 for _ in path.open(encoding="utf-8", errors="replace"))
                paths.append((NBT_DIR / f).as_posix())
            dep_str = ",".join(sorted(d for d in deps_list if d in known))
            rows.append([
                unit_id, pkg, ",".join(sorted(paths)), "minecraft",
                str(len(files)), str(loc), "rivet-nbt", str(wv), cyc, "",
                dep_str, "pending", "0", "",
            ])
        return rows

    rows = [
        r for pkg in sorted(known)
        if not (args.split_nbt and pkg in NBT_SPLIT_PACKAGES)
        for r in [package_row(pkg)]
    ]
    if args.split_nbt:
        rows.extend(split_row())

    # Stable ordering: wave, then unit id.
    rows.sort(key=lambda r: (int(r[7]), r[0]))

    out = REPO / "MANIFEST.tsv"
    with out.open("w", encoding="utf-8") as fh:
        fh.write(header + "\n")
        for r in rows:
            fh.write("\t".join(r) + "\n")

    n_split = len(NBT_UNITS) if args.split_nbt else 0
    n_cycles = len(cycle_members)
    biggest = max(cycle_members.values(), key=len, default=[])
    print(f"{len(rows)} units -> {out} (nbt split: {n_split} class-cluster units)"
          if args.split_nbt else f"{len(rows)} units -> {out}")
    print(f"waves: 0..{max(wave.values())}, cycles: {n_cycles} "
          f"(largest {len(biggest)} pkgs), needs_split: "
          f"{sum(1 for r in rows if r[9] == 'yes')}")
    per_crate: dict[str, int] = defaultdict(int)
    for r in rows:
        per_crate[r[6]] += int(r[5])
    for crate, loc in sorted(per_crate.items(), key=lambda kv: -kv[1]):
        print(f"  {crate:22} {loc:>8} LOC")


if __name__ == "__main__":
    main()

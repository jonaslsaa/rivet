#!/usr/bin/env python3
"""Builds MANIFEST.tsv (package-level units) from the Paper sources in working/Paper.

Parses package declarations and imports from every Java file, groups them into
package-level units, maps packages to target crates, records inter-package
dependencies, condenses cycles (Tarjan SCC), and assigns each unit a topological
`wave` number: a unit is safe to translate once every unit with a lower wave in
its dependency closure is done. Units in the same SCC share a `cycle` id and must
be scheduled together (or split first — see the needs_split flag).

Known limitation: same-package class references need no import in Java, so the
graph is package-level only; class-cluster refinement is epic #9.
"""

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


def crate_for(pkg: str) -> str:
    if pkg == "net.minecraft":  # root-package classes only; subpackages match CRATE_RULES
        return "rivet-core"
    for prefix, crate in CRATE_RULES:
        if pkg == prefix or pkg.startswith(prefix + "."):
            return crate
    return "rivet-server"


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
    pkg_files: dict[str, list[Path]] = defaultdict(list)
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
            pkg_files[pkg].append(f)
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

    out = REPO / "MANIFEST.tsv"
    with out.open("w", encoding="utf-8") as fh:
        fh.write("id\tjava_package\tsource_root\tfiles\tloc\tcrate\twave\tcycle\tneeds_split\tdeps\tstatus\tattempts\tnotes\n")
        for pkg in sorted(known, key=lambda p: (wave[scc_of[p]], p)):
            unit_id = pkg.replace("net.minecraft.", "mc.").replace(
                "org.bukkit.", "bukkit.").replace("io.papermc.paper.", "paper.")
            in_cycle = scc_of[pkg] if scc_of[pkg] in cycle_members else ""
            needs_split = "yes" if (
                len(pkg_files[pkg]) > SPLIT_FILE_THRESHOLD or in_cycle != "") else ""
            fh.write("\t".join([
                unit_id, pkg, pkg_root[pkg], str(len(pkg_files[pkg])),
                str(pkg_loc[pkg]), crate_for(pkg), str(wave[scc_of[pkg]]),
                str(in_cycle), needs_split, ",".join(sorted(deps[pkg])),
                "pending", "0", "",
            ]) + "\n")

    n_units = len(known)
    n_cycles = len(cycle_members)
    biggest = max(cycle_members.values(), key=len, default=[])
    print(f"{n_units} units -> {out}")
    print(f"waves: 0..{max(wave.values())}, cycles: {n_cycles} "
          f"(largest {len(biggest)} pkgs), needs_split: "
          f"{sum(1 for p in known if len(pkg_files[p]) > SPLIT_FILE_THRESHOLD or scc_of[p] in cycle_members)}")
    per_crate: dict[str, int] = defaultdict(int)
    for pkg in known:
        per_crate[crate_for(pkg)] += pkg_loc[pkg]
    for crate, loc in sorted(per_crate.items(), key=lambda kv: -kv[1]):
        print(f"  {crate:22} {loc:>8} LOC")


if __name__ == "__main__":
    main()

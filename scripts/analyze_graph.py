#!/usr/bin/env python3
"""Builds MANIFEST.tsv (v0, package-level units) from the Paper sources in working/Paper.

Parses package declarations and imports from every Java file, groups them into
package-level units, maps packages to target crates, and records inter-package
dependencies. Class-cluster refinement (splitting big packages, isolating cycles)
happens later via dedicated issues — see WORKFLOWS.md.
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


def crate_for(pkg: str) -> str:
    for prefix, crate in CRATE_RULES:
        if pkg == prefix or pkg.startswith(prefix + "."):
            return crate
    return "rivet-server"


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
                    dep_pkg = imp.rsplit(".", 1)[0]  # strip class name
                    pkg_deps[pkg].add(dep_pkg)

    known = set(pkg_files)
    out = REPO / "MANIFEST.tsv"
    with out.open("w", encoding="utf-8") as fh:
        fh.write("id\tjava_package\tsource_root\tfiles\tloc\tcrate\tdeps\tstatus\tattempts\tnotes\n")
        for pkg in sorted(known):
            deps = sorted(d for d in pkg_deps[pkg] if d in known and d != pkg)
            unit_id = pkg.replace("net.minecraft.", "mc.").replace(
                "org.bukkit.", "bukkit.").replace("io.papermc.paper.", "paper.")
            fh.write("\t".join([
                unit_id, pkg, pkg_root[pkg], str(len(pkg_files[pkg])),
                str(pkg_loc[pkg]), crate_for(pkg), ",".join(deps),
                "pending", "0", "",
            ]) + "\n")

    n_pkgs = len(known)
    n_files = sum(len(v) for v in pkg_files.values())
    n_loc = sum(pkg_loc.values())
    per_crate: dict[str, int] = defaultdict(int)
    for pkg in known:
        per_crate[crate_for(pkg)] += pkg_loc[pkg]
    print(f"{n_pkgs} packages, {n_files} files, {n_loc} LOC -> {out}")
    for crate, loc in sorted(per_crate.items(), key=lambda kv: -kv[1]):
        print(f"  {crate:22} {loc:>8} LOC")


if __name__ == "__main__":
    main()

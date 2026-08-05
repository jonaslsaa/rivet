#!/usr/bin/env python3
"""Split the mc.nbt and mc.nbt.visitors manifest rows into class-cluster units.

The original analysis produced one 36-file/6.3k-LOC unit (mc.nbt) flagged
needs_split=yes. This splits the net.minecraft.nbt package into one irreducible
SCC (the sealed Tag hierarchy + visitor interfaces + type system + accounter +
exceptions, which map to Rust enums and cannot split without stubs) plus the
one-directional downstream layers (io, ops, snbt, text, utils, visitors).

Rerun scripts/analyze_graph.py afterwards only if you want the analyzer to
re-derive everything; this script edits MANIFEST.tsv in place.
"""

import csv
import io
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MANIFEST = REPO / "MANIFEST.tsv"
NBT = (
    REPO
    / "working/Paper/paper-server/src/minecraft/java/net/minecraft/nbt"
)

# unit id -> (java_package, files, wave, cycle, deps(comma-joined java packages))
UNITS = {
    # The irreducible SCC: sealed tag hierarchy + visitor interfaces + TagType/
    # TagTypes + NbtAccounter + the NBT exceptions. package-info.java rides along.
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
        "com.mojang.serialization,net.minecraft,net.minecraft.util",
    ),
    "mc.nbt.io": (
        "net.minecraft.nbt",
        ["NbtIo.java"],
        4, "",
        "net.minecraft.nbt,net.minecraft,net.minecraft.util",
    ),
    "mc.nbt.ops": (
        "net.minecraft.nbt",
        ["NbtOps.java"],
        4, "",
        "net.minecraft.nbt,com.mojang.datafixers.util,com.mojang.serialization,net.minecraft.util",
    ),
    "mc.nbt.snbt": (
        "net.minecraft.nbt",
        ["SnbtGrammar.java", "SnbtOperations.java", "TagParser.java",
         "StringTagVisitor.java", "SnbtPrinterTagVisitor.java"],
        4, "",
        "net.minecraft.nbt,com.mojang.brigadier,com.mojang.brigadier.exceptions,com.mojang.serialization,net.minecraft.core,net.minecraft.network.chat,net.minecraft.util,net.minecraft.util.parsing.packrat,net.minecraft.util.parsing.packrat.commands",
    ),
    "mc.nbt.text": (
        "net.minecraft.nbt",
        ["TextComponentTagVisitor.java"],
        4, "",
        "net.minecraft.nbt,net.minecraft,net.minecraft.network.chat",
    ),
    "mc.nbt.utils": (
        "net.minecraft.nbt",
        ["NbtUtils.java"],
        4, "",
        "net.minecraft.nbt,com.mojang.brigadier.exceptions,com.mojang.serialization,net.minecraft,net.minecraft.core,net.minecraft.core.registries,net.minecraft.network.chat,net.minecraft.resources,net.minecraft.util,net.minecraft.world.level.block,net.minecraft.world.level.block.state,net.minecraft.world.level.block.state.properties,net.minecraft.world.level.material,net.minecraft.world.level.storage",
    ),
    "mc.nbt.visitors": (
        "net.minecraft.nbt.visitors",
        ["visitors/CollectFields.java", "visitors/CollectToTag.java",
         "visitors/FieldSelector.java", "visitors/FieldTree.java",
         "visitors/SkipAll.java", "visitors/SkipFields.java",
         "visitors/package-info.java"],
        4, "",
        "net.minecraft.nbt",
    ),
}

OLD_IDS = {"mc.nbt", "mc.nbt.visitors"}


def loc_of(files: list[str]) -> int:
    total = 0
    for f in files:
        total += sum(1 for _ in (NBT / f).open())
    return total


def main() -> None:
    with open(MANIFEST, newline="") as fh:
        rows = list(csv.reader(fh, delimiter="\t"))
    header, body = rows[0], rows[1:]

    # In-place edit: preserve the analyzer's original (wave, package) ordering.
    # Replace the old nbt rows with the new units at the same positions.
    new_rows = []
    for unit_id, (pkg, files, wave, cycle, deps) in UNITS.items():
        loc = loc_of(files)
        new_rows.append([
            unit_id, pkg, "minecraft", str(len(files)), str(loc),
            "rivet-nbt", str(wave), cycle, "", deps, "pending", "0", "",
        ])

    # Keep original order of non-nbt rows; splice nbt rows where the old rows were.
    out_rows = [header]
    for r in body:
        if r[0] not in OLD_IDS:
            out_rows.append(r)
        elif r[0] == "mc.nbt":
            # Insert the core unit + all downstream units at the mc.nbt position.
            out_rows.extend(sorted(new_rows, key=lambda x: x[0]))

    with open(MANIFEST, "w", newline="") as fh:
        w = csv.writer(fh, delimiter="\t", lineterminator="\n")
        w.writerows(out_rows)

    print(f"wrote {len(out_rows) - 1} units (was {len(body)})")
    for r in new_rows:
        print(f"  {r[0]:16s} {r[3]:>2} files  {r[4]:>5} LOC  wave={r[6]:>1}  deps={r[8]}")


if __name__ == "__main__":
    main()

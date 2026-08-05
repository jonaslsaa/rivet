#!/usr/bin/env python3
"""Scaffold rivet-nbt: crate module tree mirroring net.minecraft.nbt.

Pre-creates one snake_case module file per Java class (PORTING.md naming), a
`visitors/` submodule for the net.minecraft.nbt.visitors package, and a lib.rs
with every `mod` declaration, so each MANIFEST unit owns a disjoint file set
and translation agents never touch lib.rs.

Ownership (unit -> files it will fill):
  mc.nbt        -> the tag hierarchy + visitors interfaces + TagType(s) +
                   NbtAccounter + exceptions + package-info (crate root)
  mc.nbt.io     -> nbt_io.rs
  mc.nbt.ops    -> nbt_ops.rs
  mc.nbt.snbt   -> snbt_grammar, snbt_operations, tag_parser,
                   string_tag_visitor, snbt_printer_tag_visitor
  mc.nbt.text   -> text_component_tag_visitor.rs
  mc.nbt.utils  -> nbt_utils.rs
  mc.nbt.visitors -> src/visitors/*.rs

Every generated file is an empty stub carrying `// STUB(<unit>)` so cargo check
passes on the empty tree and agents fill their own files only.

WARNING: running scripts/analyze_graph.py regenerates MANIFEST.tsv from scratch
and will undo the class-cluster split in it. Re-run scripts/split_nbt_units.py
first if you need a fresh manifest.
"""

import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SRC = REPO / "crates" / "rivet-nbt" / "src"

# unit id -> (relative rust module path, list of java class names)
CORE = "mc.nbt"
ROOT_CLASSES = [
    "Tag", "CollectionTag", "PrimitiveTag", "NumericTag", "EndTag", "ByteTag",
    "ShortTag", "IntTag", "LongTag", "FloatTag", "DoubleTag", "StringTag",
    "ByteArrayTag", "IntArrayTag", "LongArrayTag", "ListTag", "CompoundTag",
    "TagVisitor", "StreamTagVisitor", "TagType", "TagTypes", "NbtAccounter",
    "NbtException", "NbtAccounterException", "NbtFormatException",
    "ReportedNbtException",
]
UNITS = {
    "mc.nbt": (SRC, [(cls, CORE) for cls in ROOT_CLASSES]),
    "mc.nbt.io": (SRC, [("NbtIo", "mc.nbt.io")]),
    "mc.nbt.ops": (SRC, [("NbtOps", "mc.nbt.ops")]),
    "mc.nbt.snbt": (SRC, [
        ("SnbtGrammar", "mc.nbt.snbt"),
        ("SnbtOperations", "mc.nbt.snbt"),
        ("TagParser", "mc.nbt.snbt"),
        ("StringTagVisitor", "mc.nbt.snbt"),
        ("SnbtPrinterTagVisitor", "mc.nbt.snbt"),
    ]),
    "mc.nbt.text": (SRC, [("TextComponentTagVisitor", "mc.nbt.text")]),
    "mc.nbt.utils": (SRC, [("NbtUtils", "mc.nbt.utils")]),
    "mc.nbt.visitors": (SRC / "visitors", [
        ("CollectFields", "mc.nbt.visitors"),
        ("CollectToTag", "mc.nbt.visitors"),
        ("FieldSelector", "mc.nbt.visitors"),
        ("FieldTree", "mc.nbt.visitors"),
        ("SkipAll", "mc.nbt.visitors"),
        ("SkipFields", "mc.nbt.visitors"),
    ]),
}

STUB = """// STUB({unit}) — port of the Java class `{cls}` (see working/Paper).
// Owned by manifest unit {unit}. Fill this file in during translation; do not
// delete the `mod` declaration in lib.rs. See PORTING.md for translation rules.
"""


def snake(cls: str) -> str:
    # CamelCase -> snake_case (matches PORTING.md class->file naming)
    out = []
    for i, ch in enumerate(cls):
        if ch.isupper() and i:
            out.append("_")
        out.append(ch.lower())
    return "".join(out)


def main() -> None:
    for unit, (dirpath, classes) in UNITS.items():
        dirpath.mkdir(parents=True, exist_ok=True)
        for cls, owner in classes:
            file = dirpath / f"{snake(cls)}.rs"
            if not file.exists():
                file.write_text(STUB.format(unit=owner, cls=cls))
            print(f"{unit:16s} {file.relative_to(REPO)}")

    # visitors/mod.rs + package docs
    vmod = SRC / "visitors" / "mod.rs"
    if not vmod.exists():
        vmod.write_text("// STUB(mc.nbt.visitors) — `net.minecraft.nbt.visitors` package.\n")

    # lib.rs — mod tree mirroring the flat Java package
    lib = SRC / "lib.rs"
    mods = sorted({snake(cls) for cls in ROOT_CLASSES} | {
        "nbt_io", "nbt_ops", "snbt_grammar", "snbt_operations", "tag_parser",
        "string_tag_visitor", "snbt_printer_tag_visitor",
        "text_component_tag_visitor", "nbt_utils", "crash_report",
    })
    lines = [
        "//! Port of `net.minecraft.nbt` (Mojang NBT) + `net.minecraft.nbt.visitors`.",
        "//! One module per Java class; ownership per MANIFEST.tsv units mc.nbt.*.",
        "",
    ]
    for m in mods:
        lines.append(f"pub mod {m};")
        if m == "crash_report":
            lines.append("// STUB(shared) CrashReport/ReportedException — owned by net.minecraft (rivet-server);")
            lines.append("// lives here to avoid a Cargo cycle. See crash_report.rs.")
    lines += [
        "",
        "pub mod visitors;",
        "",
    ]
    lib.write_text("\n".join(lines))
    print(f"lib.rs: {len(mods) + 1} module declarations")

    # Cargo deps: nbt needs serialization + util (both lower in the DAG)
    cargo = REPO / "crates" / "rivet-nbt" / "Cargo.toml"
    if "rivet-serialization" not in cargo.read_text():
        cargo.write_text(
            "[package]\n"
            "name = \"rivet-nbt\"\n"
            "version.workspace = true\n"
            "edition.workspace = true\n"
            "\n"
            "[lints]\n"
            "workspace = true\n"
            "\n"
            "[dependencies]\n"
            "rivet-serialization = { workspace = true }\n"
            "rivet-util = { workspace = true }\n"
        )
        print("Cargo.toml: added rivet-serialization, rivet-util")


if __name__ == "__main__":
    main()

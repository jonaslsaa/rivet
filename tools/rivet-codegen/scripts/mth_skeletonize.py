#!/usr/bin/env python3
"""Skeletonize crates/rivet-util/src/mth_golden_tests.rs for the mth-generate tool.

Reads the committed golden test file and produces:
  - data/mth_golden_skeleton.rs  — the file with every expected `rhs` of each
    `assert_eq!(lhs, rhs)` replaced by a placeholder `@@N@@` (N = assertion
    index in source order). The lhs, scaffolding, comments and whitespace are
    preserved verbatim, so substituting the values back yields the committed
    file byte-for-byte (given identical values).
  - A per-N listing (to stdout): `N: <lhs> => <committed rhs>` used to author
    and cross-check `src/java/MthGen.java`.

Only the rhs of `assert_eq!(lhs, rhs)` is replaced — the expected value — which
is exactly what the Java oracle (MthGen.java) recomputes. Placeholders are
positionally matched by the emitter against MthGen.java's ordered output.

Usage: scripts/mth_skeletonize.py [golden_rs] [out_skeleton]
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
DEFAULT_SRC = REPO / "crates/rivet-util/src/mth_golden_tests.rs"
DEFAULT_OUT = Path(__file__).resolve().parent.parent / "data/mth_golden_skeleton.rs"


def find_asserts(text: str) -> list[tuple[int, int, str, str]]:
    """Return (start, end, lhs, rhs) for every assert_eq! macro, span-inclusive.

    Scans for `assert_eq!(` and bracket-matches to its close paren. The lhs is
    everything up to the top-level comma after the open paren; rhs is the rest
    (before the close paren). Braces/parens/brackets inside are depth-tracked.
    """
    out: list[tuple[int, int, str, str]] = []
    i = 0
    while True:
        start = text.find("assert_eq!", i)
        if start < 0:
            break
        open_paren = text.find("(", start)
        if open_paren < 0:
            break
        # Find matching close paren, tracking nesting.
        depth = 0
        j = open_paren
        top_comma = -1
        while j < len(text):
            c = text[j]
            if c in "({[":
                depth += 1
            elif c in ")}]":
                depth -= 1
                if depth == 0:
                    break
            elif c == "," and depth == 1 and top_comma < 0:
                top_comma = j
            j += 1
        if top_comma < 0 or j >= len(text):
            raise ValueError(f"unbalanced assert_eq! at offset {start}: {text[start:start+60]!r}")
        end = j  # position of the closing paren
        lhs = text[open_paren + 1 : top_comma].strip()
        rhs = text[top_comma + 1 : end].strip()
        out.append((start, end, lhs, rhs))
        i = end + 1
    return out


def main() -> None:
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_SRC
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_OUT

    text = src.read_text()
    asserts = find_asserts(text)

    # Build skeleton by replacing each rhs span with a placeholder. To keep
    # offsets valid we process from the end backwards.
    skeleton = text
    pieces: list[tuple[int, str]] = []
    for n, (start, end, lhs, rhs) in enumerate(asserts):
        # The rhs span is from the top-level comma to the closing paren.
        comma_off = _top_level_comma(text, start)
        placeholder = f"@@{n}@@"
        if "ARCH-SELECTED (D14)" in rhs:
            # Arch-selected expected value (D14): the committed golden holds a
            # `#[cfg(target_arch)]` block whose x86_64 branch is the live value
            # and whose aarch64 branch is a provenance literal. Round-trip by
            # replacing ONLY the x86_64 branch literal with the @@N@@ placeholder,
            # keeping the cfg structure + aarch64 literal intact.
            skeleton_rhs = _placeholderize_arch_rhs(rhs, placeholder, n)
        else:
            skeleton_rhs = placeholder
        pieces.append((comma_off + 1, end, skeleton_rhs, lhs, rhs))

    parts = []
    cursor = 0
    for n, (cs, ce, placeholder, lhs, rhs) in enumerate(pieces):
        parts.append(text[cursor : cs + 1])  # up to and including the comma
        parts.append(placeholder)
        cursor = ce
        print(f"{n}: {lhs} => {rhs}")
    parts.append(text[cursor:])
    skeleton = "".join(parts)

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(skeleton)
    print(f"wrote {out} with {len(asserts)} placeholders", file=sys.stderr)


def _top_level_comma(text: str, assert_start: int) -> int:
    """Offset of the top-level comma inside assert_eq!(...) at assert_start."""
    open_paren = text.find("(", assert_start)
    depth = 0
    j = open_paren
    while j < len(text):
        c = text[j]
        if c in "({[":
            depth += 1
        elif c in ")}]":
            depth -= 1
        elif c == "," and depth == 1:
            return j
        j += 1
    raise ValueError("no top-level comma")


def _placeholderize_arch_rhs(rhs: str, placeholder: str, n: int) -> str:
    """Replace the x86_64-branch value inside an ARCH-SELECTED cfg block with the
    placeholder, preserving the aarch64 provenance literal and the compile_error
    fall-through, so skeletonize --> render is a stable round-trip."""
    marker = '#[cfg(target_arch = "x86_64")]'
    if marker not in rhs:
        raise ValueError(f"ARCH-SELECTED rhs #{n} missing x86_64 arm: {rhs!r}")
    head, _, tail = rhs.partition(marker)
    # The x86_64 arm is `{ <value> }` immediately after the marker.
    brace = tail.find("{")
    close = tail.find("}")
    if brace < 0 or close < 0 or close <= brace:
        raise ValueError(f"ARCH-SELECTED rhs #{n} malformed x86_64 arm: {rhs!r}")
    core = tail[brace + 1 : close].strip()
    if core.startswith("0x"):
        return f"{head}{marker}{{ {placeholder} }}{tail[close + 1:]}"
    raise ValueError(f"ARCH-SELECTED rhs #{n} unexpected x86_64 value: {core!r}")


if __name__ == "__main__":
    main()

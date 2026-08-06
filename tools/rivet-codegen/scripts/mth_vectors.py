#!/usr/bin/env python3
"""Extract (index, lhs, committed_rhs) for every assertion in the committed
golden test file, handling multi-line lhs. Writes tools/rivet-codegen/data/mth_vectors.tsv
(columns: index<TAB>lhs<TAB>rhs) for authoring/cross-checking MthGen.java."""
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
GOLDEN = REPO / "crates/rivet-util/src/mth_golden_tests.rs"
OUT = Path(__file__).resolve().parent.parent / "data/mth_vectors.tsv"


def find_asserts(text):
    """Return list of (start, end, lhs, rhs) for each assert_eq! macro."""
    out = []
    i = 0
    while True:
        start = text.find("assert_eq!", i)
        if start < 0:
            break
        open_paren = text.find("(", start)
        if open_paren < 0:
            break
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
            raise ValueError(f"unbalanced assert_eq! at {start}")
        lhs = text[open_paren + 1 : top_comma].strip()
        rhs = text[top_comma + 1 : j].strip()
        out.append((start, j, lhs, rhs))
        i = j + 1
    return out


def main():
    text = GOLDEN.read_text()
    asserts = find_asserts(text)
    with OUT.open("w") as f:
        for n, (_, _, lhs, rhs) in enumerate(asserts):
            lhs1 = re.sub(r"\s+", " ", lhs).strip()
            rhs1 = re.sub(r"\s+", " ", rhs).strip()
            f.write(f"{n}\t{lhs1}\t{rhs1}\n")
    print(f"wrote {OUT} with {len(asserts)} rows")


if __name__ == "__main__":
    main()

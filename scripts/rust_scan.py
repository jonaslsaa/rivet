#!/usr/bin/env python3
"""Shared, offline Rust-source scanning helpers for the progress ledgers.

Used by scripts/coverage_ledger.py (COVERAGE.tsv), scripts/verify_ledger.py
(VERIFY.tsv) and scripts/verify_debt.py. Everything here is a lexical
heuristic, not a Rust parser:

- `blank_comments_and_strings` replaces comment and string/char-literal bytes
  with spaces (newlines kept), so byte offsets and line numbers are unchanged
  and later regexes never match inside comments or literals. Nested block
  comments are not tracked (rare in this tree).
- `test_ranges` finds items annotated `#[cfg(test)]` (or `#[cfg(all(test, ..))]`)
  and returns the byte range of each item: from the attribute through the
  matching `}` of the item's first `{`, or through its `;` (`mod tests;`).
- `is_test_file` / `is_generated_path` / `has_generated_header` define the
  files that are never production source (VERIFICATION.md section 4.1).
"""

import re
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

_RUST_LEX_RE = re.compile(
    r"//[^\n]*"  # line comment (incl. doc comments)
    r"|/\*.*?\*/"  # block comment
    r'|b?r(#*)".*?"\1'  # raw (byte) string
    r'|b?"(?:\\.|[^"\\])*"'  # (byte) string
    r"|b?'(?:\\(?:u\{[0-9A-Fa-f]{1,6}\}|x[0-9A-Fa-f]{2}|.)|[^'\\\n])'",  # char literal
    re.S,
)

_CFG_TEST_RE = re.compile(r"#\[cfg\((?:all\(\s*)?test\b[^\]]*\]")
FN_RE = re.compile(r"\bfn\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*)")
TODO_MACRO_RE = re.compile(r"\b(?:todo|unimplemented)!\s*[\(\[\{]")


def _blank(m: re.Match) -> str:
    s = m.group(0)
    return re.sub(r"[^\n]", " ", s)


def blank_comments_and_strings(text: str) -> str:
    """Same-length copy of `text` with comments and literals blanked."""
    return _RUST_LEX_RE.sub(_blank, text)


def _match_brace(code: str, open_idx: int) -> int:
    """Index of the `}` matching the `{` at open_idx (or len-1 if unbalanced)."""
    depth = 0
    for i in range(open_idx, len(code)):
        c = code[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
    return len(code) - 1


def test_ranges(code: str) -> list[tuple[int, int]]:
    """Byte ranges [start, end) of `#[cfg(test)]` items in blanked code."""
    ranges: list[tuple[int, int]] = []
    pos = 0
    while True:
        m = _CFG_TEST_RE.search(code, pos)
        if not m:
            break
        start = m.start()
        i = m.end()
        # First `{` or `;` after the attribute ends/opens the item.
        brace = code.find("{", i)
        semi = code.find(";", i)
        if brace == -1 and semi == -1:
            break
        if semi != -1 and (brace == -1 or semi < brace):
            end = semi + 1
        else:
            end = _match_brace(code, brace) + 1
        ranges.append((start, end))
        pos = end
    return ranges


def strip_test_code(code: str) -> str:
    """Blank out every `#[cfg(test)]` item (newlines kept)."""
    out = list(code)
    for start, end in test_ranges(code):
        for i in range(start, end):
            if out[i] != "\n":
                out[i] = " "
    return "".join(out)


def production_line_count(text: str) -> int:
    """Lines of `text` outside `#[cfg(test)]` items (blank lines included,
    matching the plain line counts the plan documents use)."""
    code = blank_comments_and_strings(text)
    total = text.count("\n") + (0 if text.endswith("\n") or not text else 1)
    removed = 0
    for start, end in test_ranges(code):
        removed += code.count("\n", start, end)
    return max(total - removed, 0)


def fn_names(code: str) -> list[str]:
    """`fn` names in blanked, test-stripped code (raw `r#` prefix kept)."""
    return FN_RE.findall(code)


def is_test_file(rel: Path) -> bool:
    name = rel.name
    if name == "tests.rs" or name.endswith("_tests.rs") or name.endswith("_test.rs"):
        return True
    return "tests" in rel.parts or "benches" in rel.parts or "examples" in rel.parts


def is_generated_path(rel: Path) -> bool:
    return "generated" in rel.parts


def has_generated_header(text: str) -> bool:
    head = "\n".join(text.splitlines()[:5])
    return "@generated" in head


def tracked_crate_sources() -> list[Path]:
    """git-tracked `crates/*/src/**/*.rs`, sorted, repo-relative."""
    out = subprocess.run(
        ["git", "ls-files", "-z", "--", "crates/*/src/*.rs", "crates/*/src/**/*.rs"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    )
    paths = {Path(p) for p in out.stdout.split("\0") if p}
    return sorted(paths)

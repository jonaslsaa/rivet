#!/usr/bin/env python3
"""Validate the STUB(...) / RivetTodo(#N) comment-marker conventions.

Run with: python3 scripts/check_markers.py   (exit 0 = clean, 1 = violations)

Deterministic and fully offline: reads only git-tracked `*.rs` files and
MANIFEST.tsv (the `done` set is column 12). No network, no timestamps, no
environment dependence — the same tree always yields the same output and exit
code. Mirrors scripts/analyze_graph.py as a committed gate step.

The two marker kinds (semantic distinction, see WORKFLOWS.md / the markers doc):

  // STUB(<manifest-unit-id>) <reason>
      "No implementation here; a real port will replace this." The annotated
      code is a scaffold/placeholder. The paren argument is the exact
      MANIFEST.tsv unit id that owns the real port; it must exist and must NOT
      be `done` (a STUB on a done unit is stale — the stub should have been
      removed when the unit landed). A reason naming what is absent is
      mandatory.

  // RivetTodo(#<issue>) <reason>
      "Usable partial semantic port with a known, tracked gap." The annotated
      code is a real port; only the named aspects are deferred. The paren
      argument is a positive GitHub issue number. Format-only validated here
      (offline); issue existence/closure is a review-time check. A reason
      naming the deferred aspects is mandatory.

Enforced per comment line:
  - the literal tokens `STUB` / `RivetTodo` must appear in the canonical shape
    (bare `STUB —`, `STUB:`, prose `documented STUB`, and unknown STUB unit
    ids are all errors);
  - a STUB unit id must exist in MANIFEST.tsv;
  - a STUB whose unit is `done` is stale (error);
  - a second marker body on the same line is ambiguous (error) — it is only a
    marker when it begins a fresh sentence of the line, so a mid-sentence
    mention like "see RivetTodo(#N)" stays prose; this applies to both
    cross-form (STUB + RivetTodo) and same-form (two STUBs / two RivetTodos);
  - the reason after the reference is non-empty;
  - every `todo!()` / `unimplemented!()` must carry a RivetTodo on the same
    line or on the immediately-preceding comment line.

Excluded from the scan (markers there are ungoverned — neither counted nor
flagged): `**/src/generated/**` (codegen output), `**/tests/**` and `*_test.rs`
(test files), `fuzz/**` (fuzz-target harness), and the workspace-excluded
tools `tools/rivet-codegen/**` (golden-fixture data, e.g. `mth_golden_skeleton.rs`)
and `spikes/**`. Markers in string literals are code, not markers — the scan is
comment-anchored and ignores them.
"""

import csv
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MANIFEST = REPO / "MANIFEST.tsv"

STUB_RE = re.compile(r"^\s*//[/!]?\s*STUB\(([a-z][a-z0-9._]*)\)\s*:?\s*(.+)$")
RIVETTODO_RE = re.compile(r"^\s*//[/!]?\s*RivetTodo\(#([1-9][0-9]{0,5})\)\s*:\s*(.+)$")


def load_manifest() -> tuple[set[str], set[str]]:
    """Return (all unit ids, done unit ids) from MANIFEST.tsv."""
    units: set[str] = set()
    done: set[str] = set()
    with MANIFEST.open(newline="", encoding="utf-8") as f:
        reader = csv.reader(f, delimiter="\t")
        for row in reader:
            if not row or not row[0] or row[0] == "id":
                continue
            units.add(row[0])
            if len(row) > 11 and row[11] == "done":
                done.add(row[0])
    return units, done


def tracked_rs_files() -> list[Path]:
    """git-tracked *.rs files, sorted, with the exclusion rules applied."""
    out = subprocess.run(
        ["git", "ls-files", "*.rs"],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    )
    paths = []
    for line in out.stdout.splitlines():
        rel = Path(line)
        if is_excluded(rel):
            continue
        paths.append(rel)
    paths.sort()
    return paths


def is_excluded(rel: Path) -> bool:
    parts = rel.parts
    if parts and parts[0] == "fuzz":
        return True
    if rel.name.endswith("_test.rs"):
        return True
    if any(p == "tests" for p in parts):
        return True
    # `**/src/generated/**` — the codegen output directory.
    for i, p in enumerate(parts):
        if p == "src" and i + 1 < len(parts) and parts[i + 1] == "generated":
            return True
    # Workspace-excluded tools: `tools/rivet-codegen` (data/golden fixtures
    # like `mth_golden_skeleton.rs`) and `spikes/**`. Their files are not
    # governed source; the scan is "committed source" only.
    if parts and parts[0] == "tools" and len(parts) > 1 and parts[1] == "rivet-codegen":
        return True
    if parts and parts[0] == "spikes":
        return True
    return False


def is_comment(line: str) -> bool:
    return line.lstrip().startswith("//")


# The comment-body prefix: `//`, `//!` or `///` plus optional whitespace. A line
# is a *marker* only when its body starts with the token; prose that merely
# mentions "STUB"/"RivetTodo" mid-sentence is descriptive text, not a marker.
MARKER_BODY_RE = re.compile(r"^//[/!]?\s*(.*)$")


def marker_token(line: str) -> str | None:
    """The leading marker token of a comment line, or None if the line is
    prose (does not begin with `STUB`/`RivetTodo`)."""
    m = MARKER_BODY_RE.match(line)
    if not m:
        return None
    body = m.group(1)
    for token in ("STUB", "RivetTodo"):
        if body.startswith(token):
            return token
    return None


def has_todo_macro(line: str) -> bool:
    return "todo!(" in line or "unimplemented!(" in line


def _leads_marker_body(text: str, token: str) -> bool:
    """True when `text` contains a fresh-sentence segment that begins with the
    given marker token — a second marker body on the same line. A token that
    appears only mid-sentence (e.g. "see RivetTodo(#N)", "old STUB(abc) note")
    is prose and returns False, honoring the "a line is a marker only when its
    body begins with the token" rule."""
    for seg in text.split(". "):
        if seg.lstrip().startswith(token):
            return True
    return False


def check_file(path: Path, units: set[str], done: set[str]) -> tuple[list[tuple[str, int, str]], int, int]:
    """Return (violations, n_stub, n_todo) for one file."""
    violations: list[tuple[str, int, str]] = []
    n_stub = 0
    n_todo = 0
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as e:
        violations.append((str(path), 0, f"cannot read: {e}"))
        return violations, n_stub, n_todo
    lines = text.splitlines()
    for i, line in enumerate(lines, start=1):
        if is_comment(line):
            m = STUB_RE.match(line)
            if m:
                n_stub += 1
                unit = m.group(1)
                reason = m.group(2)
                if not reason.strip():
                    violations.append((str(path), i, "STUB with an empty reason"))
                if _leads_marker_body(reason, "RivetTodo("):
                    violations.append((str(path), i, "STUB and RivetTodo on one line"))
                elif _leads_marker_body(reason, "STUB("):
                    violations.append((str(path), i, "multiple STUB markers on one line"))
                if unit not in units:
                    violations.append(
                        (str(path), i, f"STUB unit `{unit}` not in MANIFEST.tsv")
                    )
                elif unit in done:
                    violations.append(
                        (str(path), i, f"stale STUB: unit `{unit}` is `done`")
                    )
                continue
            mr = RIVETTODO_RE.match(line)
            if mr:
                n_todo += 1
                if not mr.group(2).strip():
                    violations.append((str(path), i, "RivetTodo with an empty reason"))
                if _leads_marker_body(mr.group(2), "STUB("):
                    violations.append((str(path), i, "RivetTodo and STUB on one line"))
                elif _leads_marker_body(mr.group(2), "RivetTodo("):
                    violations.append((str(path), i, "multiple RivetTodo markers on one line"))
                continue
            # Only a line that *begins* with a marker token is a malformed
            # marker; a mid-sentence mention is prose and is ignored.
            token = marker_token(line)
            if token == "RivetTodo":
                violations.append(
                    (str(path), i, "RivetTodo must be `RivetTodo(#N): reason`")
                )
            elif token == "STUB":
                violations.append(
                    (str(path), i, "STUB must be `STUB(<manifest-unit-id>) reason`")
                )
        elif has_todo_macro(line):
            # A bare todo!()/unimplemented!() needs a RivetTodo on this line
            # or on the immediately-preceding comment line.
            if "RivetTodo" in line:
                continue
            if i > 1 and is_comment(lines[i - 2]) and RIVETTODO_RE.match(lines[i - 2]):
                continue
            violations.append(
                (str(path), i, "todo!()/unimplemented!() requires an adjacent RivetTodo")
            )
    return violations, n_stub, n_todo


def main() -> int:
    units, done = load_manifest()
    violations: list[tuple[str, int, str]] = []
    n_stub = 0
    n_todo = 0
    for rel in tracked_rs_files():
        path = REPO / rel
        file_violations, ns, nt = check_file(path, units, done)
        for _rel, line_no, message in file_violations:
            violations.append((str(rel), line_no, message))
        n_stub += ns
        n_todo += nt

    violations.sort()
    for rel, line_no, message in violations:
        print(f"{rel}:{line_no}: {message}")

    if violations:
        print(f"{len(violations)} violation(s) (tracked source: {n_stub} STUB, {n_todo} RivetTodo)")
        return 1
    print(f"0 violations ({n_stub} STUB, {n_todo} RivetTodo)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

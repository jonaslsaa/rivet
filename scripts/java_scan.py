#!/usr/bin/env python3
"""Shared, offline Java-source scanning helpers for the progress ledgers.

A lexical heuristic, not a Java parser (no javac, no network):

- `blank_comments_and_strings` replaces comments, string/char literals and
  text blocks with spaces (newlines kept), then `blank_annotation_args` blanks
  annotation argument lists (`@SuppressWarnings({"a", "b"})`), so the brace
  walk below never sees a brace that is not a block delimiter.
- `scan_methods` walks `{`, `}` and `;` with a context stack. A `{` opens a
  *named type body* when its header (the text since the previous `{`, `}` or
  `;`, cut at the first `(`) contains `class|interface|enum|record|@interface
  <Name>`. Inside a named type body, a header that reads
  `[modifiers] [<generics>] Type name(params) [throws ...]` followed by `{` or
  `;` is a method declaration. Constructors (name == enclosing type) are not
  methods. Everything inside a method body, an initializer, an enum-constant
  body or an anonymous class is skipped, so methods of anonymous and local
  classes are never counted; member (nested) types are walked like top-level
  ones, so their methods count toward the file.
"""

import re

_JAVA_LEX_RE = re.compile(
    r"//[^\n]*"
    r"|/\*.*?\*/"
    r'|""".*?"""'
    r'|"(?:\\.|[^"\\\n])*"'
    r"|'(?:\\.|[^'\\\n])+'",
    re.S,
)
_ANNOT_ARGS_RE = re.compile(r"@(?!interface\b)[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*\s*\(")
_ANNOT_BARE_RE = re.compile(r"@(?!interface\b)[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*")
_TYPE_DECL_RE = re.compile(
    r"(?<![\w$.])(?:(class|interface|enum|record)|@\s*interface)\s+([A-Za-z_$][\w$]*)"
)
_DELIM_RE = re.compile(r"[{};]")
PKG_RE = re.compile(r"^\s*package\s+([\w.]+)\s*;", re.M)
IMPORT_RE = re.compile(r"^\s*import\s+(static\s+)?([\w.]+(?:\.\*)?)\s*;", re.M)

MODIFIERS = {
    "public", "protected", "private", "static", "final", "abstract", "synchronized",
    "native", "default", "strictfp", "transient", "volatile", "sealed", "non-sealed",
}
KEYWORDS = {
    "if", "for", "while", "switch", "catch", "synchronized", "return", "new", "throw",
    "else", "case", "do", "try", "assert", "super", "this", "yield", "instanceof",
}
_TYPE_TOKEN_RE = re.compile(r"^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*(?:\[\])*$")
_IDENT_RE = re.compile(r"^[A-Za-z_$][\w$]*$")
_TAIL_RE = re.compile(r"^\s*(?:\[\s*\]\s*)*(?:throws\s+[\w$.,\s]+)?\s*(?:default\b.*)?$", re.S)


def _blank(m: re.Match) -> str:
    return re.sub(r"[^\n]", " ", m.group(0))


def blank_comments_and_strings(text: str) -> str:
    return _JAVA_LEX_RE.sub(_blank, text)


def _match_paren(s: str, open_idx: int) -> int:
    depth = 0
    for i in range(open_idx, len(s)):
        c = s[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def blank_annotation_args(code: str) -> str:
    """Blank `@Name(...)` annotations (balanced parens) and bare `@Name`."""
    out = list(code)
    pos = 0
    while True:
        m = _ANNOT_ARGS_RE.search(code, pos)
        if not m:
            break
        close = _match_paren(code, m.end() - 1)
        end = len(code) if close == -1 else close + 1
        for i in range(m.start(), end):
            if out[i] != "\n":
                out[i] = " "
        pos = end
    code = "".join(out)
    return _ANNOT_BARE_RE.sub(_blank, code)


def clean_java(text: str) -> str:
    return blank_annotation_args(blank_comments_and_strings(text))


def _strip_generics(s: str) -> str:
    prev = None
    while prev != s:
        prev = s
        s = re.sub(r"<[^<>]*>", " ", s)
    return s


def parse_method_header(header: str, type_name: str) -> str | None:
    """Method name declared by `header` (text before `{`/`;`), or None."""
    h = header.strip()
    open_idx = h.find("(")
    if open_idx <= 0:
        return None
    close = _match_paren(h, open_idx)
    if close == -1:
        return None
    if not _TAIL_RE.match(h[close + 1:]):
        return None
    prefix = _strip_generics(h[:open_idx])
    if "=" in prefix or "<" in prefix or ">" in prefix:
        return None
    prefix = re.sub(r"\s*\[\s*\]", "[]", prefix)
    tokens = prefix.split()
    if len(tokens) < 2:
        return None
    name, typ, mods = tokens[-1], tokens[-2], tokens[:-2]
    if not _IDENT_RE.match(name) or name in KEYWORDS or name in MODIFIERS:
        return None
    if typ in MODIFIERS or typ in KEYWORDS or not _TYPE_TOKEN_RE.match(typ):
        return None
    if any(m not in MODIFIERS for m in mods):
        return None
    if name == type_name:
        return None  # constructor
    return name


def scan_methods(code: str) -> list[tuple[str, str]]:
    """(enclosing type, method name) for every method declared in a named
    type body of already-cleaned Java code, in source order."""
    methods: list[tuple[str, str]] = []
    # Stack entries: ("file", "") | ("type", name) | ("other", "").
    stack: list[tuple[str, str]] = [("file", "")]
    last = 0
    for m in _DELIM_RE.finditer(code):
        ch = m.group(0)
        header = code[last:m.start()]
        last = m.end()
        kind, tname = stack[-1]
        if ch == "}":
            if len(stack) > 1:
                stack.pop()
            continue
        if ch == "{":
            if kind in ("file", "type"):
                paren = header.find("(")
                decl = _TYPE_DECL_RE.search(header if paren == -1 else header[:paren])
                if decl:
                    stack.append(("type", decl.group(2)))
                    continue
                if kind == "type":
                    name = parse_method_header(header, tname)
                    if name:
                        methods.append((tname, name))
            stack.append(("other", ""))
            continue
        # ';' — abstract / interface / native method declarations.
        if kind == "type":
            name = parse_method_header(header, tname)
            if name:
                methods.append((tname, name))
    return methods


def java_package(text: str) -> str:
    m = PKG_RE.search(text)
    return m.group(1) if m else ""


def java_imports(text: str) -> list[str]:
    return [m.group(2) for m in IMPORT_RE.finditer(text)]


def snake_case(name: str) -> str:
    """heck-style snake case: getXRot -> get_x_rot, toNBT -> to_nbt."""
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    return s.lower()

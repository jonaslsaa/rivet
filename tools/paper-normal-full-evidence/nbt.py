"""Strict NBT codec for the independent Paper evidence producer.

The codec is deliberately separate from Rivet's oracle code.  It accepts the
vanilla NBT primitives, rejects malformed lengths, duplicate compound keys, and
trailing bytes, and can emit a sorted-key canonical form for a separately
labelled semantic digest.  Raw decompressed NBT remains the authoritative
capture evidence.
"""
from __future__ import annotations

import copy
import io
import struct
from dataclasses import dataclass
from typing import Any


class NbtError(ValueError):
    """Raised for malformed, truncated, or non-canonical NBT input."""


@dataclass
class Tag:
    kind: int
    value: Any


MAX_CONTAINER = 16_000_000
MAX_DEPTH = 512


def _need(stream: io.BytesIO, n: int) -> bytes:
    if n < 0 or n > MAX_CONTAINER:
        raise NbtError(f"invalid NBT length {n}")
    data = stream.read(n)
    if len(data) != n:
        raise NbtError("truncated NBT payload")
    return data


def _u8(stream: io.BytesIO) -> int:
    return _need(stream, 1)[0]


def _i16(stream: io.BytesIO) -> int:
    return struct.unpack(">h", _need(stream, 2))[0]


def _i32(stream: io.BytesIO) -> int:
    return struct.unpack(">i", _need(stream, 4))[0]


def _i64(stream: io.BytesIO) -> int:
    return struct.unpack(">q", _need(stream, 8))[0]


def _string(stream: io.BytesIO) -> str:
    size = struct.unpack(">H", _need(stream, 2))[0]
    try:
        return _need(stream, size).decode("utf-8", "strict")
    except UnicodeDecodeError as exc:
        raise NbtError(f"invalid UTF-8 string: {exc}") from exc


def _array_length(stream: io.BytesIO, kind: str) -> int:
    size = _i32(stream)
    if size < 0 or size > MAX_CONTAINER:
        raise NbtError(f"invalid {kind} length {size}")
    return size


def _payload(stream: io.BytesIO, kind: int, depth: int) -> Tag:
    if depth > MAX_DEPTH:
        raise NbtError("NBT nesting exceeds limit")
    if kind == 1:
        return Tag(kind, struct.unpack(">b", _need(stream, 1))[0])
    if kind == 2:
        return Tag(kind, _i16(stream))
    if kind == 3:
        return Tag(kind, _i32(stream))
    if kind == 4:
        return Tag(kind, _i64(stream))
    if kind == 5:
        return Tag(kind, struct.unpack(">f", _need(stream, 4))[0])
    if kind == 6:
        return Tag(kind, struct.unpack(">d", _need(stream, 8))[0])
    if kind == 7:
        size = _array_length(stream, "byte-array")
        return Tag(kind, _need(stream, size))
    if kind == 8:
        return Tag(kind, _string(stream))
    if kind == 9:
        elem_kind = _u8(stream)
        if elem_kind > 12:
            raise NbtError(f"unknown list element tag {elem_kind}")
        size = _array_length(stream, "list")
        if elem_kind == 0 and size:
            raise NbtError("non-empty TAG_End list")
        return Tag(kind, (elem_kind, [_payload(stream, elem_kind, depth + 1) for _ in range(size)]))
    if kind == 10:
        values: dict[str, Tag] = {}
        while True:
            child_kind = _u8(stream)
            if child_kind == 0:
                return Tag(kind, values)
            if child_kind not in range(1, 13):
                raise NbtError(f"unknown compound child tag {child_kind}")
            name = _string(stream)
            if name in values:
                raise NbtError(f"duplicate compound key {name!r}")
            values[name] = _payload(stream, child_kind, depth + 1)
    if kind == 11:
        size = _array_length(stream, "int-array")
        return Tag(kind, [_i32(stream) for _ in range(size)])
    if kind == 12:
        size = _array_length(stream, "long-array")
        return Tag(kind, [_i64(stream) for _ in range(size)])
    raise NbtError(f"unknown NBT tag {kind}")


def parse(data: bytes) -> Tag:
    """Parse one unnamed compound root and consume every input byte."""
    stream = io.BytesIO(data)
    kind = _u8(stream)
    if kind != 10:
        raise NbtError(f"root tag must be TAG_Compound, got {kind}")
    _string(stream)  # NBT root names are present even when empty.
    root = _payload(stream, kind, 0)
    if stream.read(1):
        raise NbtError("trailing bytes after complete NBT root")
    return root


def _write_string(out: io.BytesIO, value: str) -> None:
    encoded = value.encode("utf-8")
    if len(encoded) > 0xFFFF:
        raise NbtError("NBT string is too long")
    out.write(struct.pack(">H", len(encoded)))
    out.write(encoded)


def _encode_payload(out: io.BytesIO, tag: Tag, canonical: bool) -> None:
    kind, value = tag.kind, tag.value
    if kind == 1:
        out.write(struct.pack(">b", value))
    elif kind == 2:
        out.write(struct.pack(">h", value))
    elif kind == 3:
        out.write(struct.pack(">i", value))
    elif kind == 4:
        out.write(struct.pack(">q", value))
    elif kind == 5:
        out.write(struct.pack(">f", value))
    elif kind == 6:
        out.write(struct.pack(">d", value))
    elif kind == 7:
        out.write(struct.pack(">i", len(value)))
        out.write(value)
    elif kind == 8:
        _write_string(out, value)
    elif kind == 9:
        elem_kind, items = value
        out.write(bytes([elem_kind]))
        out.write(struct.pack(">i", len(items)))
        for item in items:
            _encode_payload(out, item, canonical)
    elif kind == 10:
        items = value.items()
        if canonical:
            items = sorted(items, key=lambda item: item[0])
        for name, child in items:
            out.write(bytes([child.kind]))
            _write_string(out, name)
            _encode_payload(out, child, canonical)
        out.write(b"\x00")
    elif kind == 11:
        out.write(struct.pack(">i", len(value)))
        for item in value:
            out.write(struct.pack(">i", item))
    elif kind == 12:
        out.write(struct.pack(">i", len(value)))
        for item in value:
            out.write(struct.pack(">q", item))
    else:
        raise NbtError(f"cannot encode tag {kind}")


def encode(root: Tag, *, canonical: bool = False) -> bytes:
    """Encode an unnamed compound root without adding compression."""
    if root.kind != 10:
        raise NbtError("root tag must be TAG_Compound")
    out = io.BytesIO()
    out.write(b"\x0a")
    _write_string(out, "")
    _encode_payload(out, root, canonical)
    return out.getvalue()


def clone(root: Tag) -> Tag:
    return copy.deepcopy(root)


def canonical_without_dynamic(root: Tag) -> bytes:
    """Canonicalize compound keys after documented save-clock removal.

    Paper changes ``Data.InhabitedTime`` and ``Data.LastUpdate`` as a function
    of the boot/save clock.  They are removed only from the semantic digest;
    raw decompressed bytes and raw hashes are never changed.
    """
    result = clone(root)
    if result.kind == 10:
        data = result.value.get("Data")
        container = data.value if data is not None and data.kind == 10 else result.value
        for key in ("InhabitedTime", "LastUpdate"):
            container.pop(key, None)
    return encode(result, canonical=True)


def get_compound(root: Tag, *names: str) -> Tag | None:
    current = root
    for name in names:
        if current.kind != 10:
            return None
        current = current.value.get(name)
        if current is None:
            return None
    return current


def get_any(root: Tag, name: str) -> Tag | None:
    """Get a field at root or in the conventional chunk ``Data`` compound."""
    if root.kind != 10:
        return None
    if name in root.value:
        return root.value[name]
    data = root.value.get("Data")
    if data is not None and data.kind == 10:
        return data.value.get(name)
    return None

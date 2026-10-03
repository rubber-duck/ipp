"""Bounded WASM section accounting; names are explanatory, never exact crate costs.

This reader validates the framing it consumes, not WASM instruction semantics.
Code body sizes include local declarations but exclude their LEB size prefixes.
Shader matches are a lower bound within data-section payloads, not extra bytes.
"""

from collections import defaultdict
import hashlib
from pathlib import Path


class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.position = 0

    def take(self, size: int) -> bytes:
        end = self.position + size
        if end > len(self.data):
            raise ValueError("Truncated WASM payload")
        result = self.data[self.position : end]
        self.position = end
        return result

    def uint(self) -> int:
        value = 0
        for index in range(5):
            byte = self.take(1)[0]
            if index == 4 and byte & 0xF0:
                raise ValueError("WASM u32 LEB overflow")
            value |= (byte & 0x7F) << (index * 7)
            if not byte & 0x80:
                return value
        raise ValueError("Invalid WASM u32 LEB")

    def name(self) -> str:
        try:
            return self.take(self.uint()).decode("utf-8")
        except UnicodeDecodeError as error:
            raise ValueError("Invalid WASM UTF-8 name") from error

    def done(self) -> None:
        if self.position != len(self.data):
            raise ValueError("Trailing WASM payload bytes")


def function_names(payload: bytes) -> dict[int, str]:
    reader = Reader(payload)
    names = {}
    while reader.position < len(payload):
        kind = reader.take(1)[0]
        section = Reader(reader.take(reader.uint()))
        if kind == 1:
            for _ in range(section.uint()):
                index = section.uint()
                if index in names:
                    raise ValueError("Duplicate WASM function name")
                names[index] = section.name()
            section.done()
    return names


def imported_functions(payload: bytes) -> int:
    reader = Reader(payload)
    functions = 0
    for _ in range(reader.uint()):
        reader.name()
        reader.name()
        kind = reader.take(1)[0]
        if kind == 0:
            reader.uint()
            functions += 1
        elif kind in (1, 2):
            if kind == 1:
                reader.take(1)  # reference type (current builds use funcref)
            flags = reader.uint()
            if flags & ~3:
                raise ValueError("Unsupported WASM import limits")
            reader.uint()
            if flags & 1:
                reader.uint()
        elif kind == 3:
            reader.take(2)  # value type and mutability
        elif kind == 4:
            reader.take(1)
            reader.uint()
        else:
            raise ValueError("Unsupported WASM import kind")
    reader.done()
    return functions


def analyze_wasm(data: bytes, shaders: tuple[Path, ...] = ()) -> dict:
    reader = Reader(data)
    if reader.take(8) != b"\x00asm\x01\x00\x00\x00":
        raise ValueError("Expected WASM version 1 header")
    sections = []
    payloads = {}
    names = {}
    seen = set()
    while reader.position < len(data):
        start = reader.position
        kind = reader.take(1)[0]
        size = reader.uint()
        payload = reader.take(size)
        if kind > 13 or (kind and kind in seen):
            raise ValueError("Invalid or duplicate WASM section")
        seen.add(kind)
        label = str(kind)
        if kind == 0:
            custom = Reader(payload)
            label = custom.name()
            if label == "name":
                names.update(function_names(payload[custom.position :]))
        else:
            payloads[kind] = payload
        sections.append(
            {
                "id": kind,
                "name": label,
                "payload_bytes": size,
                "framing_bytes": reader.position - start - size,
            }
        )
    bodies = []
    if 10 in payloads:
        code = Reader(payloads[10])
        for _ in range(code.uint()):
            bodies.append(len(code.take(code.uint())))
        code.done()
    if 3 in payloads:
        functions = Reader(payloads[3])
        count = functions.uint()
        for _ in range(count):
            functions.uint()
        functions.done()
        if count != len(bodies):
            raise ValueError("WASM function/code count mismatch")
    elif bodies:
        raise ValueError("WASM code without function declarations")
    exports: dict[int, list[str]] = defaultdict(list)
    if 7 in payloads:
        export_reader = Reader(payloads[7])
        export_names = set()
        for _ in range(export_reader.uint()):
            name = export_reader.name()
            kind = export_reader.take(1)[0]
            index = export_reader.uint()
            if name in export_names or kind > 4:
                raise ValueError("Invalid WASM export")
            export_names.add(name)
            if kind == 0:
                exports[index].append(name)
        export_reader.done()
    offset = imported_functions(payloads[2]) if 2 in payloads else 0
    symbols = []
    groups: dict[str, int] = defaultdict(int)
    unknown = 0
    for index, size in enumerate(bodies, offset):
        aliases = sorted(exports.get(index, []))
        name = names.get(index) or (aliases[0] if aliases else None)
        if not name:
            unknown += size
            continue
        symbols.append(
            {
                "index": index,
                "name": name,
                "body_bytes": size,
                "name_source": "name_section" if names.get(index) else "export",
                "export_aliases": aliases,
            }
        )
        # Raw names may be mangled, optimized, shared, or compiler-generated.
        group = name.split("::", 1)[0] if "::" in name else "named_without_namespace"
        groups[group] += size
    data_payload = payloads.get(11, b"")
    intervals = []
    shader_records = []
    for path in shaders:
        shader = path.read_bytes()
        if not shader:
            continue
        cursor = 0
        matches = 0
        while (start := data_payload.find(shader, cursor)) >= 0:
            intervals.append((start, start + len(shader)))
            matches += 1
            cursor = start + 1
        shader_records.append(
            {
                "path": str(path),
                "bytes": len(shader),
                "sha256": hashlib.sha256(shader).hexdigest(),
                "exact_matches": matches,
            }
        )
    covered = 0
    end = 0
    for start, stop in sorted(intervals):
        covered += max(0, stop - max(start, end))
        end = max(end, stop)
    code_bytes = len(payloads.get(10, b""))
    return {
        "sections": sections,
        "header_bytes": 8,
        "code_payload_bytes": code_bytes,
        "data_payload_bytes": len(data_payload),
        "other_and_framing_bytes": len(data) - code_bytes - len(data_payload),
        "attribution": {
            "basis": "function bodies; raw names only; not exact crate costs",
            "named_body_bytes": sum(item["body_bytes"] for item in symbols),
            "unknown_body_bytes": unknown,
            "code_framing_bytes": code_bytes - sum(bodies),
            "groups": dict(sorted(groups.items())),
            "symbols": sorted(
                symbols, key=lambda item: (-item["body_bytes"], item["index"])
            ),
        },
        "embedded_shaders": {
            "matched_bytes": covered,
            "data_remainder_bytes": len(data_payload) - covered,
            "accounting": "subset of data payload; exact-match lower bound; never add to WASM total",
            "sources": shader_records,
        },
    }

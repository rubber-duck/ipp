"""Guard limits that must keep one Rust source against copies in other files.

Generated clients and composed shaders receive these limits from Rust: the contract
generator emits them from the executed target export and the renderer defines shader
array sizes from its constants. This check fails when a copy reappears in a
hand-written source, and compares the few copies that cannot be generated (the
schema-independent Host wire helpers, the standalone Blender add-on) with their Rust
source. Tests state their own expected values and are not checked.
"""

import ast
from pathlib import Path, PurePosixPath
import re


# Hand-written sources that must take mirrored limits from generation or the runtime.
TYPESCRIPT = re.compile(r"^(packages/[^/]+/src|tools/ipp-schema-gen/src)/.*\.(ts|tsx)$")
GLSL = re.compile(r"^crates/ipp-render-gl/src/.*\.(glsl|vert|frag)$")
TEMPLATE = re.compile(r"^tools/ipp-schema-gen/src/.*\.template\.ts$")
RUST = re.compile(r"^(crates|tools)/[^/]+/src/.*\.rs$")
TEST_PATH = re.compile(r"(^|/)(tests?|examples|fixtures)/|_tests\.rs$|\.test\.tsx?$")

# A shader array sized by a number instead of an `IPP_*` limit definition.
GLSL_ARRAY = re.compile(r"^\s*uniform\s+\w+\s+\w+\s*\[\s*\d+\s*\]", re.MULTILINE)

# Codec bounds written as numbers in templates: count/string limits and index ranges.
TEMPLATE_BOUNDS = (
    re.compile(r"\.count\(\s*(?:[^(),]+,\s*)?\d[\d_]*\s*\)"),
    re.compile(r"\.string\(\s*\d[\d_]*\s*\)"),
    re.compile(r"\bmax\s*=\s*\d[\d_]*\b"),
    re.compile(r"\buint\([^,()]+,\s*\d[\d_]*\s*\)"),
    re.compile(r"\.length\s*>\s*\d[\d_]*\b"),
    re.compile(r"\bcount\s*>\s*\d[\d_]*\b"),
)

# Names of mirrored limits that no hand-written TypeScript or GLSL may define.
MIRRORED_NAMES = re.compile(
    r"\b(?:const|let|var)\s+(MAX_JOINTS|MAX_LIGHTS|MAX_POINTERS|MAX_CONNECTIONS|"
    r"MAX_WORKER_CONNECTIONS|MAX_DELIVERIES|INGRESS_CREDIT_MESSAGES|ROW_REGION_SPAN|"
    r"MAX_ROW_PROPERTIES|MAX_MESH_VERTICES|SURFACE_CACHE_MAX_DIMENSION|"
    r"MAX_LIFECYCLE_PUBLICATIONS|BATCH_OUTCOME_ALIASES|COMMAND_PAGE_(?:BYTES|COMMANDS))"
    r"\s*=\s*\d"
)

# Limits with one Rust definition; another literal definition is a drifting copy.
RUST_SOURCES = (
    "GUI_INPUT_MAX_POINTERS",
    "MAX_JOINTS",
    "MAX_LIGHTS",
    "MAX_MESH_VERTICES",
    "MAX_MESSAGE_BYTES",
    "MAX_FIELD_BYTES",
    "MAX_PENDING",
    "MAX_ROW_PROPERTIES",
    "MAX_ROW_TEXT_BYTES",
    "ROW_REGION_SPAN",
    "SURFACE_CACHE_MAX_DIMENSION",
)
RUST_DEFINITION = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+("
    + "|".join(RUST_SOURCES)
    + r")\s*:\s*\w+\s*=\s*(\d[^;]*);",
    re.MULTILINE,
)

# Copies outside generation, each compared with its Rust source:
# (file, pattern, Rust file, Rust pattern); each pattern captures one numeric expression.
VERIFIED_COPIES = (
    (
        "packages/ipp-client/src/host-contract.ts",
        r"export const HOST_MESSAGE_BYTES = ([\d_]+);",
        "crates/ipp-protocol/src/lib.rs",
        r"pub const MAX_MESSAGE_BYTES: usize = ([\d_]+);",
    ),
    (
        "packages/ipp-client/src/host-protocol.ts",
        r"value\.length > ([\d_]+)\)",
        "crates/ipp-protocol/src/lib.rs",
        r"pub const MAX_FIELD_BYTES: usize = ([\d_]+);",
    ),
    (
        "packages/ipp-client/src/host-protocol.ts",
        r"this\.raw\(this\.count\(([\d_]+)\)\)",
        "crates/ipp-protocol/src/lib.rs",
        r"pub const MAX_FIELD_BYTES: usize = ([\d_]+);",
    ),
    (
        "packages/ipp-client/src/host-presentation.ts",
        r"const maxPresentationSources = Math\.floor\(([^;]+)\);",
        "crates/ipp-protocol/src/presentation.rs",
        r"pub const MAX_PRESENTATION_SOURCES: usize = ([^;]+);",
    ),
    (
        "packages/ipp-client/src/bulk-reads.ts",
        r"export const BULK_CHUNK_BYTES = ([\d_]+);",
        "crates/ipp-protocol/src/bulk_read.rs",
        r"pub const CHUNK_BYTES: usize = ([\d_ *]+);",
    ),
    (
        "packages/ipp-client/src/world-persistence-client.ts",
        r"const CHUNK_BYTES = ([\d_]+);",
        "crates/ipp-protocol/src/lib.rs",
        r"pub const MAX_FIELD_BYTES: usize = ([\d_]+);",
    ),
    (
        "packages/ipp-client/src/world-persistence-client.ts",
        r"options\.maxBytes \?\? ([\d_ *]+);",
        "crates/ipp-core/src/services/world_serialization/mod.rs",
        r"max_bytes: ([\d_ <]+),",
    ),
    (
        "packages/ipp-client/src/resource-worker.ts",
        r"if \(length > ([\d_]+)\) throw new Error\(\"Resource output",
        "crates/ipp-protocol/src/lib.rs",
        r"pub const MAX_MESSAGE_BYTES: usize = ([\d_]+);",
    ),
    (
        "packages/ipp-react/src/gui/clipboard.ts",
        r"export const CLIPBOARD_TEXT_MAX_BYTES = ([\d_]+);",
        "crates/ipp-protocol/src/lib.rs",
        r"pub const MAX_FIELD_BYTES: usize = ([\d_]+);",
    ),
    (
        "integrations/blender/ipp_blender/assets.py",
        r"MAX_JOINTS = ([\d_]+)",
        "crates/ipp-core/src/services/asset_management/mod.rs",
        r"pub const MAX_JOINTS: usize = ([\d_]+);",
    ),
    (
        "integrations/blender/ipp_blender/assets.py",
        r"count <= ([\d_]+)",
        "crates/ipp-core/src/services/asset_management/mesh.rs",
        r"pub const MAX_MESH_VERTICES: u32 = ([\d_]+);",
    ),
    (
        "integrations/blender/ipp_blender/exporter/mesh.py",
        r"loop_triangles\) \* 3 > ([\d_]+)",
        "crates/ipp-core/src/services/asset_management/mesh.rs",
        r"pub const MAX_MESH_VERTICES: u32 = ([\d_]+);",
    ),
)

# Names a verified copy may define, keyed by file.
VERIFIED_NAMES = {"integrations/blender/ipp_blender/assets.py": {"MAX_JOINTS"}}


def mirrored_limit_errors(root: Path, names: list[str]) -> list[str]:
    """Return mirrored-limit copies that reappeared or drifted from their Rust source."""
    errors = []
    definitions = {}
    for name in sorted(names):
        path = root / name
        if not name or TEST_PATH.search(name) or not path.is_file():
            continue
        if TYPESCRIPT.match(name) or GLSL.match(name):
            source = path.read_text()
            for match in MIRRORED_NAMES.finditer(source):
                if match[1] not in VERIFIED_NAMES.get(name, ()):
                    errors.append(
                        f"{name}:{line(source, match)}: {match[1]} copies a limit that "
                        "the contract generator or runtime provides"
                    )
            if GLSL.match(name):
                for match in GLSL_ARRAY.finditer(source):
                    errors.append(
                        f"{name}:{line(source, match)}: size shader arrays with an "
                        "IPP_* limit definition from Rust"
                    )
            if TEMPLATE.match(name):
                for pattern in TEMPLATE_BOUNDS:
                    for match in pattern.finditer(strip_comments(source)):
                        errors.append(
                            f"{name}:{line(source, match)}: codec bound `{match[0]}` "
                            "must be a generated contract constant"
                        )
        elif RUST.match(name) and PurePosixPath(name).suffix == ".rs":
            source = path.read_text()
            for match in RUST_DEFINITION.finditer(source):
                definitions.setdefault(match[1], []).append(
                    f"{name}:{line(source, match)}"
                )
    for constant, sites in sorted(definitions.items()):
        if len(sites) > 1:
            errors.append(
                f"{sites[1]}: {constant} is also defined at {sites[0]}; "
                "reference the one Rust source instead"
            )
    for copy, pattern, source, source_pattern in VERIFIED_COPIES:
        try:
            copied = value(root / copy, pattern)
            original = value(root / source, source_pattern)
        except (OSError, ValueError) as error:
            errors.append(f"{copy}: mirrored limit check failed: {error}")
            continue
        if copied != original:
            errors.append(
                f"{copy}: mirrored limit {copied} differs from {original} in {source}"
            )
    return errors


def line(source: str, match: re.Match) -> int:
    return source.count("\n", 0, match.start()) + 1


def strip_comments(source: str) -> str:
    """Blank line and block comments, keeping offsets so line numbers still match."""
    return re.sub(
        r"//[^\n]*|/\*.*?\*/",
        lambda match: re.sub(r"[^\n]", " ", match[0]),
        source,
        flags=re.DOTALL,
    )


def value(path: Path, pattern: str) -> int:
    """Evaluate the one integer expression `pattern` captures in `path`."""
    matches = re.findall(pattern, path.read_text())
    if len(matches) != 1:
        raise ValueError(f"expected one match of {pattern!r}, found {len(matches)}")
    expression = re.sub(r"(?<=\d)_(?=\d)", "", matches[0]).replace("crate::", "crate.")
    return evaluate(ast.parse(expression, mode="eval").body, path)


def evaluate(node: ast.AST, path: Path) -> int:
    """Integer arithmetic with names resolved from Rust constants in the same crate."""
    if isinstance(node, ast.Constant) and isinstance(node.value, int):
        return node.value
    if isinstance(node, ast.BinOp):
        left, right = evaluate(node.left, path), evaluate(node.right, path)
        operations = {
            ast.Add: lambda: left + right,
            ast.Sub: lambda: left - right,
            ast.Mult: lambda: left * right,
            ast.Div: lambda: left // right,
            ast.FloorDiv: lambda: left // right,
            ast.LShift: lambda: left << right,
        }
        if type(node.op) in operations:
            return operations[type(node.op)]()
    if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
        if node.value.id == "crate" and path.suffix == ".rs":
            crate = next(parent for parent in path.parents if parent.name == "src")
            return value(crate / "lib.rs", rf"pub const {node.attr}: usize = ([\d_]+);")
    raise ValueError(f"unsupported limit expression {ast.dump(node)}")

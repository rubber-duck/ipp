"""Mechanical source ownership checks; responsibility cohesion still needs review."""

from pathlib import Path, PurePosixPath
import re


SOURCE_TOKEN = re.compile(
    r"(?P<comment>//[^\n]*|/\*.*?\*/)"
    r"|(?P<string>\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*')"
    r"|(?P<template>`(?:\\.|[^`\\])*`)"
    r"|(?P<word>[a-zA-Z_$][a-zA-Z0-9_$]*)"
    r"|(?P<symbol>[^\s])",
    re.DOTALL,
)


def source_imports(source: str) -> list[str]:
    """Read conventional literal imports, skipping comments and quoted source.

    This is an ownership guard, not a JavaScript parser. Computed specifiers and
    template interpolation require review alongside the dependency boundary.
    """
    tokens = [
        (match.lastgroup, match[0])
        for match in SOURCE_TOKEN.finditer(source)
        if match.lastgroup not in ("comment", "template")
    ]
    imports = []
    for index, token in enumerate(tokens[:-1]):
        if token not in (("word", "import"), ("word", "export")):
            continue
        if index and tokens[index - 1][1] == ".":
            continue
        following = tokens[index + 1 :]
        if token[1] == "import":
            if following[0][0] == "string":
                imports.append(following[0][1][1:-1])
                continue
            if following[0][1] == "(":
                if (
                    len(following) >= 3
                    and following[1][0] == "string"
                    and following[2][1] == ")"
                ):
                    imports.append(following[1][1][1:-1])
                continue
            if following[0][1] == ".":
                continue
        elif following[0][1] not in ("*", "{", "type"):
            continue
        for offset, part in enumerate(following):
            if part[1] == ";":
                break
            if part[0] == "string":
                if offset and following[offset - 1] == ("word", "from"):
                    imports.append(part[1][1:-1])
                break
    return imports


RUST_PATH_ATTRIBUTE = re.compile(r'#\[\s*path\s*=\s*"([^"]*)"\]')
RUST_RAW_STRING = re.compile(r"r(#*)\"")


def rust_code(source: str) -> str:
    """Return source with comments and string/char literals blanked.

    The full input is scanned with no line or length limits; newlines are
    preserved so reported positions stay aligned with the original file.
    """
    out: list[str] = []
    i, end = 0, len(source)
    while i < end:
        if source.startswith("//", i):
            stop = source.find("\n", i)
            stop = end if stop < 0 else stop
            out.append(" " * (stop - i))
            i = stop
        elif source.startswith("/*", i):
            depth = 0
            while i < end:
                if source.startswith("/*", i):
                    depth += 1
                    out.append("  ")
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    out.append("  ")
                    i += 2
                    if depth == 0:
                        break
                else:
                    out.append("\n" if source[i] == "\n" else " ")
                    i += 1
        elif source[i] == '"':
            i = _blank_quoted(source, out, i, '"')
        elif source[i] == "r":
            raw = RUST_RAW_STRING.match(source, i)
            if raw:
                close = '"' + "#" * len(raw.group(1))
                stop = source.find(close, raw.end())
                stop = end if stop < 0 else stop + len(close)
                out.append("".join("\n" if c == "\n" else " " for c in source[i:stop]))
                i = stop
            else:
                out.append(source[i])
                i += 1
        elif source[i] == "'":
            char = re.match(r"'(?:\\.|[^'\\\n])'", source[i:])
            if char:
                out.append(" " * len(char[0]))
                i += len(char[0])
            else:
                out.append(source[i])
                i += 1
        else:
            out.append(source[i])
            i += 1
    return "".join(out)


def _blank_quoted(source: str, out: list[str], start: int, quote: str) -> int:
    """Blank one `"..."` literal starting at `start`; return the next index."""
    out.append(" ")
    i = start + 1
    while i < len(source):
        if source[i] == "\\":
            out.append("  ")
            i += 2
        elif source[i] == quote:
            out.append(" ")
            return i + 1
        else:
            out.append("\n" if source[i] == "\n" else " ")
            i += 1
    return i


def rust_path_errors(root: Path, rust: set[Path]) -> list[str]:
    """Reject `#[path]` targets that leave their module directory.

    Adjacent files stay allowed, which covers adjacent test modules and test
    support alongside the production module they exercise. Production paths
    must follow their directories rather than importing implementation from
    another subsystem through `#[path]`.
    """
    errors = []
    for path in sorted(rust):
        try:
            source = (root / path).read_text(encoding="utf-8")
        except OSError:
            continue
        # Match against the raw source (blanking erases string contents),
        # then keep only attributes that sit in real code rather than in a
        # comment or string literal.
        code = rust_code(source)
        for match in RUST_PATH_ATTRIBUTE.finditer(source):
            if code[match.start() : match.start() + 2] != "#[":
                continue
            target = match.group(1)
            if (
                not target
                or "\\" in target
                or PurePosixPath(target).parent != PurePosixPath(".")
            ):
                errors.append(
                    f"{path}: production #[path] {target!r} leaves its module "
                    "directory; keep the implementation adjacent instead of "
                    "importing it through #[path]"
                )
    return errors


def structure_errors(root: Path, names: list[str]) -> list[str]:
    root = root.resolve()
    paths = {Path(name) for name in names if name and (root / name).is_file()}
    rust = {path for path in paths if path.suffix == ".rs" and "src" in path.parts}
    errors = []
    errors.extend(rust_path_errors(root, rust))
    for path in sorted(rust):
        if path.stem not in ("lib", "main", "mod") and any(
            child.is_relative_to(path.with_suffix("")) for child in rust
        ):
            errors.append(
                f"{path}: use {path.with_suffix('')}/mod.rs for a module with children"
            )

    for directory in sorted({path.parent for path in rust}):
        children = {
            path for path in rust if path.parent == directory and path.name != "mod.rs"
        }
        has_nested_modules = any(
            path.parent != directory and path.is_relative_to(directory) for path in rust
        )
        if (
            len(children) == 1
            and next(iter(children)).name.endswith("_tests.rs")
            and directory.name != "src"
            and not has_nested_modules
        ):
            errors.append(
                f"{directory}: keep a standalone module and its sole test file adjacent"
            )

    for path in sorted(paths):
        if path.suffix not in (".ts", ".tsx", ".js", ".jsx", ".mjs"):
            continue
        production = (
            (
                path.parts[0] in ("examples", "integrations", "packages")
                and "tests" not in path.parts
                and "src" in path.parts
            )
            or path.parts[0] == "examples"
            or path.is_relative_to("integrations/blender/client")
        )
        if not production:
            continue
        source = (root / path).read_text()
        for specifier in source_imports(source):
            if not specifier.startswith("."):
                continue
            target = (root / path.parent / specifier).resolve()
            if not target.is_relative_to(root):
                continue
            relative = target.relative_to(root)
            if "tests" in relative.parts:
                errors.append(
                    f"{path}: production code imports test-owned module {specifier}"
                )
            elif path.parts[0] != "examples" and relative.parts[0] == "examples":
                errors.append(
                    f"{path}: reusable code imports example-owned module {specifier}"
                )
    return errors

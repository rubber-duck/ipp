"""Literal build-product reads of test sources against their declared builds.

A test, or a fixture module it imports through a literal relative path, that
names a directory one build step produces must depend on that step, directly
or through another declared build. A suite command whose test-name pattern or
arguments select only part of a shared test file lists the products that part
never reaches as `partitionExcludes`.

Only literal `target/...` paths are seen. Paths assembled at runtime, products
named through variables, imports by package name or type only, and bundles
whose source no build script names under the same file stem stay outside this
check.
"""

from pathlib import Path
import re

from .model import ROOT, Task

PRODUCT = re.compile(
    r"(?<![\w.-])(?<!dist/)target/[A-Za-z0-9._-]+(?:/[A-Za-z0-9._@-]+)*"
)
TYPE_ONLY = re.compile(r"\bimport\s+type\s[^;]*;|\btypeof\s+import\([^)]*\)")
SCRIPT_SOURCE = re.compile(r"[\w@./-]+\.(?:ts|tsx|mts|mjs|js)\b")
RELATIVE_IMPORT = re.compile(
    r"""(?:\bfrom\s*|\bimport\s*\(\s*|\bimport\s+)["'](\.{1,2}/[^"'$]+)["']"""
)
SOURCE_SUFFIXES = (".ts", ".tsx", ".mts", ".js", ".mjs")
SOURCE_ROOTS = ("tests", "packages", "crates", "examples", "tools", "integrations")


def source_index() -> dict[str, list[Path]]:
    """Sources a build script names literally, by the file name it emits."""
    scripts = [
        path
        for root in SOURCE_ROOTS
        for path in (ROOT / root).rglob("*.mjs")
        if "node_modules" not in path.parts and "dist" not in path.parts
    ]
    index: dict[str, list[Path]] = {}
    for script in scripts:
        for literal in SCRIPT_SOURCE.findall(script.read_text(errors="replace")):
            path = ROOT / literal
            if path.suffix in (".ts", ".tsx") and path.is_file():
                emitted = f"{path.name.removesuffix(path.suffix)}.js"
                if path not in index.setdefault(emitted, []):
                    index[emitted].append(path)
    return index


def resolve_source(argument: str, index: dict[str, list[Path]]) -> Path | None:
    """The source file a test command argument runs, when it is identifiable."""
    if argument.startswith("dist/"):
        base = ROOT / argument.removeprefix("dist/").removesuffix(".js")
        for suffix in (".ts", ".tsx"):
            if base.with_name(base.name + suffix).is_file():
                return base.with_name(base.name + suffix)
        return None
    if argument.startswith("target/"):
        candidates = index.get(Path(argument).name, [])
        return candidates[0] if len(candidates) == 1 else None
    path = ROOT / argument
    if path.suffix in SOURCE_SUFFIXES and path.is_file():
        return path
    return None


def imported(source: Path, specifier: str) -> Path | None:
    target = (source.parent / specifier).resolve()
    if not target.is_relative_to(ROOT):
        return None
    if target.is_file():
        return target
    stem = target.with_suffix("") if target.suffix in (".js", ".mjs") else target
    for suffix in SOURCE_SUFFIXES:
        candidate = stem.with_name(stem.name + suffix)
        if candidate.is_file():
            return candidate
    return None


def literal_reads(entry: Path) -> dict[str, Path]:
    """Literal product paths of a source and its relative imports, by reader."""
    reads: dict[str, Path] = {}
    pending = [entry]
    seen: set[Path] = set()
    while pending:
        source = pending.pop()
        if source in seen:
            continue
        seen.add(source)
        text = TYPE_ONLY.sub("", source.read_text(errors="replace"))
        for match in PRODUCT.finditer(text):
            reads.setdefault(match.group(0), source)
        for specifier in RELATIVE_IMPORT.findall(text):
            dependency = imported(source, specifier)
            if dependency is not None:
                pending.append(dependency)
    return reads


def undeclared_product_reads(
    tasks: dict[str, Task], excluded: dict[str, tuple[str, ...]]
) -> list[str]:
    """Each literal product read whose producer a test does not depend on.

    `excluded` names, per test, products its selected partition never reads.
    """
    products = [
        (output, task.id)
        for task in tasks.values()
        if task.id.startswith("build:")
        for output in task.outputs
    ]

    def producer(path: str) -> str | None:
        for output, owner in products:
            if path == output or path.startswith(f"{output}/"):
                return owner
        return None

    def prerequisites(task: Task) -> set[str]:
        closure: set[str] = set()
        pending = list(task.dependencies)
        while pending:
            name = pending.pop()
            if name not in closure:
                closure.add(name)
                pending.extend(tasks[name].dependencies)
        return closure

    index = source_index()
    failures: list[str] = []
    for task in tasks.values():
        if not task.id.startswith(("test:", "check:gles-")):
            continue
        declared = prerequisites(task)
        unreached = excluded.get(task.id, ())
        reads: dict[str, str] = {}
        for argument in task.command:
            if producer(argument) is not None:
                reads.setdefault(argument, "its command")
            source = resolve_source(argument, index)
            if source is not None:
                for path, module in literal_reads(source).items():
                    reads.setdefault(path, str(module.relative_to(ROOT)))
        for item in unreached:
            if not any(path == item or path.startswith(f"{item}/") for path in reads):
                failures.append(f"{task.id} excludes {item}, which it never names")
        for path, reader in sorted(reads.items()):
            if any(path == item or path.startswith(f"{item}/") for item in unreached):
                continue
            owner = producer(path)
            if owner is not None and owner not in declared:
                failures.append(f"{task.id} reads {path} ({reader}) without {owner}")
    return failures

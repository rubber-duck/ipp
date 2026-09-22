"""Leaf operations invoked by the supervised executor, without a second task graph."""

import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from .builds import build, verify_browser_identities
from .catalog import CI_PROFILES, PROFILES, SUITES, TEST_INPUTS, catalog, regression_ids
from .environment import development_python
from .model import ROOT, select
from .processes import node, run


def source_files(paths: list[str]) -> list[str]:
    # Git enumerates this checkout's files without descending into nested
    # repositories/worktrees, unlike the formatters' directory walkers.
    result = subprocess.run(
        [
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            *paths,
        ],
        cwd=ROOT,
        capture_output=True,
        check=True,
    )
    return sorted(
        {
            name
            for name in os.fsdecode(result.stdout).split("\0")
            if name and (ROOT / name).is_file() and not (ROOT / name).is_symlink()
        }
    )


def format_source(language: str, mode: str, paths: list[str]) -> None:
    check = mode == "check"
    if language == "rust":
        if paths:
            raise ValueError(
                "Rust formatting uses Cargo workspace discovery; omit explicit paths"
            )
        run(["cargo", "fmt", "--all", *(["--", "--check"] if check else [])])
        return

    suffixes = {
        "js": (".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".mts", ".cts"),
        "python": (".py", ".pyi"),
        "md": (".md",),
    }
    if language not in suffixes:
        raise ValueError(f"Unknown formatter: {language}")
    selected = [
        name for name in source_files(paths) if name.endswith(suffixes[language])
    ]
    if not selected:
        return

    if language == "js":
        run(
            [
                node(),
                "node_modules/@biomejs/biome/bin/biome",
                "format",
                *([] if check else ["--write"]),
                "--",
                *selected,
            ]
        )
    elif language == "python":
        run(
            [
                development_python(),
                "-m",
                "ruff",
                "format",
                *(["--check"] if check else []),
                "--",
                *selected,
            ]
        )
    elif language == "md":
        run(
            [
                node(),
                "node_modules/prettier/bin/prettier.cjs",
                "--check" if check else "--write",
                "--",
                *selected,
            ]
        )


def validate_catalog() -> None:
    tasks = catalog("/validation/egl")
    for name in CI_PROFILES:
        select(tasks, regression_ids(tasks, name))
    declared = {path for suite in SUITES.values() for path in suite.get("files", [])}
    if set(TEST_INPUTS) != declared:
        raise ValueError(
            f"Suite inputs differ from declared files: {sorted(set(TEST_INPUTS) ^ declared)}"
        )
    for name, suite in SUITES.items():
        for root in suite.get("sourceRoots", []):
            # A trailing slash owns a directory; otherwise the root is one file.
            if (
                not isinstance(root, str)
                or Path(root).is_absolute()
                or ".." in Path(root).parts
                or not (
                    (ROOT / root).is_dir()
                    if root.endswith("/")
                    else (ROOT / root).is_file()
                )
            ):
                raise ValueError(f"Suite {name} has invalid source root: {root!r}")
        for path in suite.get("files", []):
            source = path.removeprefix("dist/")
            source = (
                source.removesuffix(".js") + ".ts"
                if path.startswith("dist/")
                else source
            )
            if not (ROOT / source).is_file():
                raise ValueError(f"Suite {name} references missing source: {source}")
    workflow = (ROOT / ".github/workflows/gallery-pages.yml").read_text()
    # The Pages workflow uses explicit named invocations on single lines.
    import shlex

    from .cli import parser, make_plan

    count = 0
    for line in workflow.splitlines():
        command = line.strip().removeprefix("run: ")
        invocation = re.search(r"(?:^|\s)python3? tools/ipp\.py (.+)$", command)
        if invocation is None:
            continue
        args = parser().parse_args(shlex.split(invocation[1]))
        if args.command not in ("setup", "doctor"):
            make_plan(args)
        count += 1
    if count == 0:
        raise ValueError("CI must invoke the maintained Python pipeline")
    # Feature records describe selection; manifest checks remain an independent oracle.
    for name, profile in PROFILES["browser"].items():
        if set(profile) != {"features", "builtins"} or not isinstance(
            profile["builtins"], bool
        ):
            raise ValueError(f"Invalid browser profile: {name}")
    print(
        f"Validated {len(tasks)} tasks, {len(SUITES)} suites and {count} CI invocations."
    )


def contracts(profiles: list[str]) -> None:
    spec = importlib.util.spec_from_file_location(
        "ipp_contract_checks", ROOT / "tools/check_contracts.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.main(profiles)


def main(args: list[str]) -> None:
    operation, *remaining = args
    if operation == "build":
        build(remaining[0])
    elif operation == "benchmark-build":
        from .benchmark import build_browser, build_native

        if remaining[0] == "browser":
            build_browser()
        else:
            build_native(remaining[0] == "native-instrumented")
    elif operation == "benchmark":
        from .benchmark import scene

        scene(json.loads(remaining[0]))
    elif operation == "format":
        format_source(remaining[0], remaining[1], remaining[2:])
    elif operation == "python-types":
        run([development_python(), "-m", "mypy", "--config-file", "mypy.ini"])
        if sys.platform != "win32":
            run(
                [
                    development_python(),
                    "-m",
                    "mypy",
                    "--config-file",
                    "mypy.ini",
                    "--platform",
                    "win32",
                ]
            )
    elif operation == "catalog":
        validate_catalog()
    elif operation == "contracts":
        contracts(remaining)
    elif operation == "browser-identities":
        verify_browser_identities()
    elif operation == "contract-identities":
        reports = [
            json.loads(
                (
                    ROOT
                    / "target/integration-artifacts/contracts"
                    / name
                    / "target-report.json"
                ).read_text()
            )
            for name in PROFILES["contracts"]
        ]
        for target in ("native", "wasm"):
            if len({report[target]["hash"] for report in reports}) != len(reports):
                raise ValueError(
                    f"{target}: distinct feature selections have equal contract identity"
                )
    elif operation == "serve":
        from .server import serve

        serve(remaining[0], int(remaining[1]) if len(remaining) > 1 else None)
    elif operation == "setup":
        from .setup import setup

        setup(remaining)
    else:
        raise ValueError(f"Unknown operation: {operation}")

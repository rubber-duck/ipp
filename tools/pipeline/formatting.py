"""Formatter drivers over this checkout's current source files."""

import os
import subprocess

from .environment import development_python
from .model import ROOT
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

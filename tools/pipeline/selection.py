"""Conservative changed-file selection; uncertainty is visible before execution."""

import os
from pathlib import Path
import subprocess

from .catalog import SUITES, suite_ids
from .model import ROOT


def changed_files(base: str | None = None) -> list[str]:
    reference = "HEAD"
    if base:
        reference = subprocess.run(
            ["git", "merge-base", "HEAD", base],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    tracked = subprocess.run(
        ["git", "diff", "--name-only", "-z", reference],
        cwd=ROOT,
        capture_output=True,
        check=True,
    ).stdout
    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        capture_output=True,
        check=True,
    ).stdout
    return sorted(set(os.fsdecode(tracked + untracked).split("\0")) - {""})


def owns(root: str, path: str) -> bool:
    """Directory roots end with a slash; any other root names one file."""
    return path.startswith(root) if root.endswith("/") else path == root


def affected(
    paths: list[str], explicit_suites: list[str]
) -> tuple[list[str], list[str]]:
    ids = ["check:repository", "check:catalog", "check:diff"]
    notes = []
    uncertain = []
    for path in paths:
        suffix = Path(path).suffix
        if suffix == ".md":
            ids.append("check:format-md")
        elif suffix == ".py":
            ids.extend(["check:format-python", "check:python-types"])
        elif suffix == ".rs":
            ids.append("check:format-rust")
        elif suffix in (".ts", ".tsx", ".js", ".mjs"):
            ids.append("check:format-js")
        if suffix == ".md":
            continue
        # Declared owners add to the conservative area rules below; a file
        # without either still needs an explicit suite.
        owners = [
            name
            for name, suite in SUITES.items()
            if any(owns(root, path) for root in suite.get("sourceRoots", []))
        ]
        if owners:
            ids.extend(suite_ids(owners))
        if path.startswith(("tools/pipeline/", "tools/tests/", ".github/")) or path in (
            "tools/ipp.py",
            "mypy.ini",
            ".python-version",
            "requirements-dev.txt",
        ):
            ids.extend(suite_ids(["runner"]))
        elif path.startswith("packages/ipp-react/"):
            ids.extend(suite_ids(["react", "canvas"]))
        elif path.startswith("packages/ipp-client/"):
            ids.extend(suite_ids(["client", "native", "browser"]))
        elif path.endswith(".test.ts") or path.endswith(".test.mjs"):
            compiled = f"dist/{path[:-3]}.js" if path.endswith(".ts") else path
            if any(compiled in suite.get("files", []) for suite in SUITES.values()):
                ids.append(f"test:{compiled}")
            elif not owners:
                uncertain.append(path)
        elif path in (
            "package.json",
            "package-lock.json",
            "biome.json",
            ".prettierrc.json",
            "ruff.toml",
        ):
            ids.extend(["check:format-js", "check:format-python", "check:format-md"])
            ids.extend(suite_ids(["runner"]))
        elif path.startswith(("tools/check_", "tools/repository_structure")):
            ids.append("check:repository")
        elif not owners:
            uncertain.append(path)
    if uncertain and not explicit_suites:
        raise ValueError(
            "Select affected coverage with --suite for these changes: "
            + ", ".join(uncertain[:12])
        )
    if uncertain:
        notes.append(
            "Explicit suites cover changes without a precise automatic mapping: "
            + ", ".join(uncertain)
        )
    notes.append(
        f"Selected checks from {len(paths)} changed files; no full regression was inferred."
    )
    return list(dict.fromkeys(ids)), notes

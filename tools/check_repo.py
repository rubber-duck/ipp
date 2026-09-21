#!/usr/bin/env python3
"""Check documentation, source ownership and coordination with the standard library."""

import argparse
import html
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

from repository_structure import structure_errors


ROOT = Path(__file__).resolve().parents[1]
REQUIRED = (
    "README.md",
    "AGENTS.md",
    "CLAUDE.md",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "docs/architecture.md",
    "docs/development/integration-testing.md",
    "docs/architecture/rust-workspace.md",
    "docs/plans/README.md",
    "docs/plans/ecs-serialization.md",
    "docs/plans/react-reconciler.md",
    "docs/plans/runtime-and-rendering.md",
    "docs/development/workflow.md",
    "docs/development/building.md",
    "docs/development/coordination.md",
    "docs/development/task-template.md",
    "docs/development/handoff-template.md",
    ".beads/PRIME.md",
    ".beads/config.yaml",
    ".beads/metadata.json",
    "tools/coordination-versions.json",
    "tools/check_workspace.py",
    ".github/pull_request_template.md",
    ".github/workflows/validate.yml",
)
# Repository prose uses inline links; code examples are excluded below.
LINK = re.compile(r"!?\[[^\]\n]*\]\(\s*(?:<([^>]+)>|([^\s)]+))(?:\s+\"[^\"]*\")?\s*\)")


def run(*command):
    return subprocess.run(
        command, cwd=ROOT, text=True, capture_output=True, check=True, timeout=30
    ).stdout.strip()


def markdown(path):
    """Return visible lines, GFM-style heading anchors, and formatting errors."""
    data = path.read_text(encoding="utf-8")
    lines, anchors, errors = [], set(), []
    fence = None
    if data and not data.endswith("\n"):
        errors.append("missing final newline")
    for number, line in enumerate(data.splitlines(), 1):
        if line.rstrip() != line:
            errors.append(f"{number}: trailing whitespace")
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if fence:
            if (
                marker
                and marker[1][0] == fence[0]
                and len(marker[1]) >= len(fence)
                and not marker[2].strip()
            ):
                fence = None
            continue
        if marker:
            fence = marker[1]
            continue
        heading = re.match(r"^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        if heading:
            label = re.sub(r"\[([^]]+)\]\([^)]+\)", r"\1", heading[1])
            label = re.sub(r"<[^>]+>", "", html.unescape(label)).lower()
            slug = "".join(c for c in label if c.isalnum() or c in " _-").replace(
                " ", "-"
            )
            anchor, suffix = slug, 0
            while anchor in anchors:
                suffix += 1
                anchor = f"{slug}-{suffix}"
            anchors.add(anchor)
        lines.append((number, re.sub(r"(`+).*?\1", "", line)))
    if fence:
        errors.append("unclosed code fence")
    return lines, anchors, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tools",
        action="store_true",
        help="also verify installed Beads/Dolt versions without database access",
    )
    args = parser.parse_args()
    errors = [
        f"{name}: required file missing"
        for name in REQUIRED
        if not (ROOT / name).is_file()
    ]
    alias = ROOT / "CLAUDE.md"
    if not alias.is_symlink() or alias.readlink() != Path("AGENTS.md"):
        errors.append("CLAUDE.md: expected a relative symlink to AGENTS.md")
    # Include untracked work under review, exclude ignored databases/dependencies,
    # and tolerate tracked files that the current change intentionally deletes.
    names = run(
        "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"
    ).split("\0")
    errors.extend(structure_errors(ROOT, names))
    paths = sorted(
        {
            ROOT / name
            for name in names
            if name.endswith(".md") and (ROOT / name).is_file()
        }
    )
    documents = {path.resolve(): markdown(path) for path in paths}
    link_count = 0
    for path, (lines, _, problems) in documents.items():
        label = path.relative_to(ROOT)
        errors.extend(f"{label}:{problem}" for problem in problems)
        for number, line in lines:
            for match in LINK.finditer(line):
                destination = html.unescape(match[1] or match[2])
                url = urlsplit(destination)
                if url.scheme or url.netloc:
                    continue
                link_count += 1
                target = (
                    (
                        ROOT / unquote(url.path).lstrip("/")
                        if url.path.startswith("/")
                        else path.parent / unquote(url.path)
                    ).resolve()
                    if url.path
                    else path
                )
                if not target.is_relative_to(ROOT):
                    errors.append(
                        f"{label}:{number}: link escapes repository: {destination}"
                    )
                elif not target.exists():
                    errors.append(
                        f"{label}:{number}: missing link target: {destination}"
                    )
                elif url.fragment and target.suffix == ".md":
                    if target not in documents:
                        documents_for_target = markdown(target)
                    else:
                        documents_for_target = documents[target]
                    if unquote(url.fragment) not in documents_for_target[1]:
                        errors.append(
                            f"{label}:{number}: missing heading: {destination}"
                        )
    for name in (".beads/metadata.json", "tools/coordination-versions.json"):
        try:
            value = json.loads((ROOT / name).read_text())
            if name.endswith("metadata.json") and (
                value.get("backend"),
                value.get("dolt_mode"),
                value.get("dolt_database"),
            ) != ("dolt", "server", "ipp"):
                errors.append(f"{name}: expected IPP Dolt server configuration")
            if name.endswith("coordination-versions.json"):
                for tool in ("beads", "dolt"):
                    if not re.fullmatch(r"\d+\.\d+\.\d+", str(value.get(tool, ""))):
                        errors.append(f"{name}: {tool} must have an exact release pin")
        except (OSError, ValueError, AttributeError) as exc:
            errors.append(f"{name}: {exc}")
    if args.tools and not errors:
        pins = json.loads((ROOT / "tools/coordination-versions.json").read_text())
        for binary, key in (("bd", "beads"), ("dolt", "dolt")):
            try:
                output = run(binary, "version")
                version = re.search(r"\b\d+\.\d+\.\d+\b", output)
                if not version or version[0] != pins[key]:
                    errors.append(f"{binary}: expected {pins[key]}, got {output}")
                else:
                    print(f"{binary}: {version[0]}")
            except (OSError, subprocess.SubprocessError) as exc:
                errors.append(f"{binary}: {exc}; see docs/development/coordination.md")
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(
        f"Checked {len(paths)} Markdown files, {link_count} local links, source ownership, and coordination files."
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.SubprocessError, UnicodeError) as error:
        print(f"Repository check failed: {error}", file=sys.stderr)
        sys.exit(1)

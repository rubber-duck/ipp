"""Subprocess operations; the outer executor owns cancellation of the process tree."""

from contextlib import nullcontext
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys

from .model import ROOT


def node() -> str:
    return os.environ.get("NODE_BIN", "node")


def blender() -> str:
    import json

    directory = json.loads((ROOT / "tools/pipeline/toolchains.json").read_text())[
        "blenderLinuxX64"
    ]["directory"]
    local = ROOT / "target/tools" / directory / "blender"
    selected = os.environ.get(
        "BLENDER_BIN", str(local) if local.is_file() else "blender"
    )
    return shutil.which(selected) or selected


def npm() -> list[str]:
    # npm.cmd is a shell script on Windows; invoke the installed JS CLI directly.
    path = os.environ.get("npm_execpath")
    if path:
        return [node(), path]
    if os.name == "nt":
        import shutil

        binary = shutil.which("node")
        if binary:
            cli = Path(binary).parent / "node_modules/npm/bin/npm-cli.js"
            if cli.is_file():
                return [node(), str(cli)]
    return ["npm"]


def run(
    command: list[str],
    *,
    output: Path | None = None,
    timeout: int = 1800,
    env: dict[str, str] | None = None,
) -> None:
    print(f"$ {shlex.join(command)}", flush=True)
    temporary = (
        output.with_name(output.name + f".tmp-{os.getpid()}") if output else None
    )
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with temporary.open("wb") if temporary else nullcontext(None) as stream:
            subprocess.run(
                command,
                cwd=ROOT,
                stdout=stream,
                check=True,
                timeout=timeout,
                env={**os.environ, **env} if env else None,
            )
        if temporary and output:
            temporary.replace(output)
    finally:
        if temporary:
            temporary.unlink(missing_ok=True)


def python_tool(path: str, *args: str) -> list[str]:
    return [sys.executable, path, *args]

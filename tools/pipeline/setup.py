"""Explicit dependency installation; ordinary plans and checks never install tools."""

import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request

from .environment import development_python
from .model import ROOT
from .processes import blender, node, npm, python_tool, run


def setup(args: list[str]) -> None:
    name, *flags = args
    if name == "node":
        run([*npm(), "ci"])
    elif name == "python":
        if development_python() == sys.executable:
            run([sys.executable, "-m", "venv", str(ROOT / ".venv")])
        run(
            [development_python(), "-m", "pip", "install", "-r", "requirements-dev.txt"]
        )
    elif name == "rust":
        config = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]
        run(
            [
                "rustup",
                "toolchain",
                "install",
                config["channel"],
                "--profile",
                config["profile"],
                "--component",
                ",".join(config["components"]),
                "--target",
                ",".join(config["targets"]),
            ]
        )
    elif name == "browser":
        run(
            [
                node(),
                "node_modules/playwright/cli.js",
                "install",
                *(["--with-deps"] if "--with-deps" in flags else []),
                "chromium",
            ]
        )
    elif name == "certificates":
        run(python_tool("tools/blender.py", "certificate", *flags))
    elif name == "blender":
        if (platform.system(), platform.machine()) != ("Linux", "x86_64"):
            raise ValueError(
                "Pinned Blender setup currently supports Linux x64; configure BLENDER_BIN for other environments"
            )
        config = json.loads((ROOT / "tools/pipeline/toolchains.json").read_text())[
            "blenderLinuxX64"
        ]
        parent = ROOT / "target/tools"
        parent.mkdir(parents=True, exist_ok=True)
        destination = parent / config["directory"]
        try:
            # This leaf runs inside the executor's owned process tree.
            version = subprocess.run(
                [blender(), "--version"],
                capture_output=True,
                text=True,
                check=True,
                timeout=30,
            )
            installed = version.stdout.startswith("Blender 5.2.")
        except OSError, subprocess.SubprocessError:
            installed = False
        if not (destination / "blender").is_file() and not installed:
            with tempfile.TemporaryDirectory(
                prefix="blender-", dir=parent
            ) as temporary:
                archive = Path(temporary) / "blender.tar.xz"
                with (
                    urllib.request.urlopen(config["url"], timeout=60) as response,
                    archive.open("wb") as output,
                ):
                    shutil.copyfileobj(response, output)
                with archive.open("rb") as stream:
                    if (
                        hashlib.file_digest(stream, "sha256").hexdigest()
                        != config["sha256"]
                    ):
                        raise ValueError("Blender release checksum mismatch")
                with tarfile.open(archive) as source:
                    source.extractall(temporary, filter="data")
                (Path(temporary) / config["directory"]).rename(destination)
        run(python_tool("tools/blender.py", "prepare"))
    else:
        raise ValueError(f"Unknown setup profile: {name}")

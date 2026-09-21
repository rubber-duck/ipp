"""Read-only, selection-specific prerequisite checks with actionable diagnostics."""

import ctypes
from dataclasses import asdict, dataclass
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import threading
import tomllib

from .model import ROOT, Task
from .processes import blender, node


@dataclass(frozen=True)
class Requirement:
    name: str
    ready: bool
    detail: str
    remedy: str = ""


def development_python() -> str:
    path = ROOT / ".venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    return str(path) if path.is_file() else sys.executable


def probe(
    command: list[str], timeout: int = 30, cancel: threading.Event | None = None
) -> str:
    # Imported on execution to keep the environment/runner modules import-safe.
    from .runner import execute

    event = cancel or threading.Event()
    with tempfile.TemporaryDirectory(prefix="ipp-probe-") as temporary:
        log = Path(temporary) / "probe.log"
        result = execute(
            Task("probe", "Inspect environment", tuple(command), timeout=timeout),
            log,
            event,
        )
        output = log.read_text(errors="replace").strip()
        if event.is_set():
            raise ValueError("Environment inspection cancelled")
        if result["exitCode"] or result["timedOut"]:
            raise ValueError(output[-2500:] or "Environment probe failed or timed out")
        return output


def inspect(
    requirements: set[str],
    egl_directory: str | None = None,
    cancel: threading.Event | None = None,
) -> list[Requirement]:
    def inspect_command(command: list[str], timeout: int = 30) -> str:
        return probe(command, timeout, cancel)

    results = []
    for name in sorted(requirements | {"python"}):
        remedy = ""
        if cancel and cancel.is_set():
            results.append(Requirement(name, False, "Environment inspection cancelled"))
            break
        try:
            if name == "python":
                required = (ROOT / ".python-version").read_text().strip()
                if f"{sys.version_info.major}.{sys.version_info.minor}" != required:
                    raise ValueError(
                        f"requires Python {required}; found {platform.python_version()}"
                    )
                detail = f"{sys.executable} ({platform.python_version()})"
            elif name == "git":
                detail = inspect_command(["git", "--version"])
            elif name == "node":
                remedy = "Install Node from .node-version, or set NODE_BIN."
                required = (ROOT / ".node-version").read_text().strip()
                detail = inspect_command([node(), "--version"])
                if detail != f"v{required}":
                    raise ValueError(f"requires Node {required}; found {detail}")
            elif name == "npm":
                remedy = "python tools/ipp.py setup node"
                metadata = json.loads((ROOT / "package.json").read_text())
                for package, version in metadata["devDependencies"].items():
                    installed = json.loads(
                        (ROOT / "node_modules" / package / "package.json").read_text()
                    )
                    if installed["version"] != version:
                        raise ValueError(
                            f"{package}: expected {version}; installed {installed['version']}"
                        )
                detail = "locked JavaScript development dependencies installed"
            elif name in ("rust", "wasm"):
                remedy = "python tools/ipp.py setup rust"
                toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())[
                    "toolchain"
                ]
                if name == "rust":
                    if os.environ.get("CARGO_BUILD_TARGET"):
                        raise ValueError(
                            "Unset CARGO_BUILD_TARGET; pipeline products select their own native/WASM target"
                        )
                    if (
                        os.environ.get("CARGO_TARGET_DIR")
                        and Path(os.environ["CARGO_TARGET_DIR"]).resolve()
                        != ROOT / "target"
                    ):
                        raise ValueError(
                            "Use this checkout's target directory; unset CARGO_TARGET_DIR for pipeline builds"
                        )
                    detail = inspect_command(["rustc", "--version"])
                    if not detail.startswith(f"rustc {toolchain['channel']} "):
                        raise ValueError(
                            f"expected Rust {toolchain['channel']}; found {detail}"
                        )
                else:
                    installed = inspect_command(
                        ["rustup", "target", "list", "--installed"]
                    )
                    if "wasm32-unknown-unknown" not in installed.splitlines():
                        raise ValueError("wasm32-unknown-unknown is not installed")
                    detail = "wasm32-unknown-unknown installed"
            elif name == "python-tools":
                remedy = "python tools/ipp.py setup python"
                pins = dict(
                    line.split("==", 1)
                    for line in (ROOT / "requirements-dev.txt").read_text().splitlines()
                    if "==" in line
                )
                versions = json.loads(
                    inspect_command(
                        [
                            development_python(),
                            "-c",
                            "import importlib.metadata,json,sys; print(json.dumps({n:importlib.metadata.version(n) for n in sys.argv[1:]}))",
                            *pins,
                        ]
                    )
                )
                for package, version in pins.items():
                    if versions[package] != version:
                        raise ValueError(
                            f"{package}: expected {version}; found {versions[package]}"
                        )
                detail = f"{development_python()}: {versions}"
            elif name == "browser":
                remedy = "python tools/ipp.py setup browser --with-deps; then run in the configured browser environment."
                detail = inspect_command(
                    [node(), "tools/build/probe-browser.mjs"], timeout=45
                )
            elif name == "blender":
                remedy = "python tools/ipp.py setup blender, or set BLENDER_BIN."
                detail = inspect_command([blender(), "--version"])
                if not re.search(r"Blender 5\.2\.", detail):
                    raise ValueError(f"requires Blender 5.2; found {detail[:200]}")
                detail = detail.splitlines()[0]
            elif name == "blender-wheels":
                remedy = "python tools/ipp.py setup blender"
                if (platform.system(), platform.machine()) != ("Linux", "x86_64"):
                    raise ValueError("Blender packaging currently supports Linux x64")
                lock = json.loads(
                    (ROOT / "integrations/blender/wheels-linux-x64.json").read_text()
                )
                for wheel, expected in lock.items():
                    path = ROOT / "target/blender/wheels" / wheel
                    if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                        raise ValueError(f"Blender wheel checksum mismatch: {wheel}")
                detail = "locked Blender wheels verified"
            elif name == "certificates":
                remedy = "python tools/ipp.py setup certificates --install-trust"
                for variable, filename in (
                    ("BLENDER_TEST_CERTIFICATE", "localhost.pem"),
                    ("BLENDER_TEST_PRIVATE_KEY", "localhost-key.pem"),
                ):
                    path = Path(
                        os.environ.get(
                            variable,
                            str(ROOT / "target/blender/certificates" / filename),
                        )
                    )
                    if not path.is_file():
                        raise ValueError(f"Missing configured {variable}")
                detail = (
                    "configured certificate files exist; browser scenarios verify trust"
                )
            elif name == "gles":
                remedy = "Set IPP_EGL_LIBRARY_DIR or pass --egl-dir with the EGL/GLES library directory."
                if not egl_directory:
                    raise ValueError(
                        "native GLES coverage requires an explicit library directory"
                    )
                directory = Path(egl_directory)
                for candidates in (
                    ("libEGL.so.1", "libEGL.so", "libEGL.dylib", "libEGL.dll"),
                    (
                        "libGLESv2.so.2",
                        "libGLESv2.so",
                        "libGLESv2.dylib",
                        "libGLESv2.dll",
                    ),
                ):
                    library = next(
                        (
                            directory / name
                            for name in candidates
                            if (directory / name).is_file()
                        ),
                        None,
                    )
                    if library is None:
                        raise ValueError(f"Missing {candidates[0]} in {directory}")
                    ctypes.CDLL(str(library))
                detail = f"EGL/GLES libraries load from {directory}"
            elif name == "coordination":
                pins = json.loads(
                    (ROOT / "tools/coordination-versions.json").read_text()
                )
                for binary, key in (("bd", "beads"), ("dolt", "dolt")):
                    version = inspect_command([binary, "version"])
                    if pins[key] not in version:
                        raise ValueError(
                            f"{binary}: expected {pins[key]}; found {version}"
                        )
                detail = "Beads/Dolt versions verified; database startup and claims remain explicit"
            else:
                raise ValueError(f"Unknown environment requirement: {name}")
            results.append(Requirement(name, True, detail))
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            results.append(Requirement(name, False, str(error), remedy))
    return results


def records(results: list[Requirement]) -> list[dict]:
    return [asdict(result) for result in results]

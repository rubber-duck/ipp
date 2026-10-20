"""Install the exact Chrome Stable pin once in a user cache shared by worktrees."""

from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from typing import Iterator
import urllib.request
import zipfile

from .model import ROOT


def browser_platform() -> str:
    machine = platform.machine().lower()
    if sys.platform == "linux":
        if machine in ("x86_64", "amd64"):
            return "linux64"
        if machine in ("aarch64", "arm64"):
            return "linux-arm64"
    elif sys.platform == "darwin":
        return "mac-arm64" if machine == "arm64" else "mac-x64"
    elif sys.platform == "win32":
        return "win64" if machine in ("amd64", "x86_64", "arm64") else "win32"
    raise ValueError(f"No Chrome for Testing build for {sys.platform}/{machine}")


def browser_cache() -> Path:
    if sys.platform == "darwin":
        base = Path.home() / "Library/Caches"
    elif sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData/Local"))
    else:
        base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    return base / "ipp/chrome-for-testing"


def browser_executable(directory: Path, target: str) -> Path:
    unpacked = directory / f"chrome-{target}"
    if target.startswith("mac-"):
        return (
            unpacked
            / "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
        )
    return unpacked / ("chrome.exe" if target.startswith("win") else "chrome")


@contextmanager
def installation_lock(path: Path, timeout: float = 180) -> Iterator[None]:
    """A process lock protects publication; OS cleanup makes interrupted setup retryable."""
    with path.open("a+b") as stream:
        if path.stat().st_size == 0:
            stream.write(b"\0")
            stream.flush()
        stream.seek(0)
        deadline = time.monotonic() + timeout
        waiting = False
        while True:
            try:
                if sys.platform == "win32":
                    import msvcrt

                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl

                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except OSError as error:
                if time.monotonic() >= deadline:
                    raise ValueError(
                        "Timed out waiting for another checkout to install this Chrome pin"
                    ) from error
                if not waiting:
                    print(
                        "Waiting for the shared Chrome installation to finish",
                        flush=True,
                    )
                    waiting = True
                time.sleep(0.1)
        try:
            yield
        finally:
            stream.seek(0)
            if sys.platform == "win32":
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(stream, fcntl.LOCK_UN)


def critical_files(directory: Path, target: str) -> dict[str, dict]:
    """Cheap warm-install identity: sizes and header digests of browser/runtime data."""
    executable = browser_executable(directory, target)
    files = [executable]
    for name in ("icudtl.dat", "resources.pak"):
        candidates = list(directory.rglob(name))
        if not candidates:
            raise ValueError(f"Chrome archive is missing {name}")
        files.extend(candidates)
    result = {}
    for path in files:
        with path.open("rb") as stream:
            header = stream.read(65536)
        result[path.relative_to(directory).as_posix()] = {
            "bytes": path.stat().st_size,
            "modifiedNs": path.stat().st_mtime_ns,
            "headerSha256": hashlib.sha256(header).hexdigest(),
        }
    return result


def verified_installation(directory: Path, pin: dict, target: str) -> bool:
    manifest = directory / "installation.json"
    if not manifest.is_file():
        return False
    try:
        installed = json.loads(manifest.read_text())
        return installed["identity"] == release_identity(pin, target) and installed[
            "criticalFiles"
        ] == critical_files(directory, target)
    except OSError, ValueError, KeyError:
        return False


def release_identity(pin: dict, target: str) -> dict:
    return {
        "version": pin["version"],
        "revision": pin["revision"],
        "platform": target,
        "url": pin["downloads"][target],
    }


def extract_browser(archive: Path, staging: Path) -> None:
    """Preserve application-bundle symlinks without allowing archive path escapes."""
    with zipfile.ZipFile(archive) as source:
        for entry in source.infolist():
            relative = Path(entry.filename)
            destination = staging / relative
            if (
                relative.is_absolute()
                or ".." in relative.parts
                or not destination.resolve().is_relative_to(staging.resolve())
            ):
                raise ValueError("Unsafe Chrome archive entry")
            permissions = entry.external_attr >> 16
            if stat.S_ISLNK(permissions):
                target = Path(source.read(entry).decode("utf-8"))
                if target.is_absolute() or not (
                    destination.parent / target
                ).resolve().is_relative_to(staging.resolve()):
                    raise ValueError("Unsafe Chrome archive symlink")
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.symlink_to(target)
                continue
            source.extract(entry, staging)
            if permissions and os.name != "nt":
                destination.chmod(permissions & 0o777)


def install_browser() -> Path:
    pin = json.loads((ROOT / "tools/pipeline/toolchains.json").read_text())[
        "chromeForTesting"
    ]
    version = pin["version"]
    target = browser_platform()
    url = pin["downloads"].get(target)
    if not url:
        raise ValueError(f"Chrome {version} has no pinned download for {target}")
    parent = browser_cache()
    parent.mkdir(parents=True, exist_ok=True)
    destination = parent / f"{version}-{target}"
    executable = browser_executable(destination, target)
    with installation_lock(parent / f"{version}-{target}.lock"):
        if verified_installation(destination, pin, target):
            print(f"Reusing Chrome {version}: {executable}")
            return executable
        if destination.exists():
            raise ValueError(
                f"Unverified Chrome installation at {destination}; inspect it before retrying"
            )
        with tempfile.TemporaryDirectory(
            prefix=f"{version}-{target}-", dir=parent
        ) as temporary:
            staging = Path(temporary)
            archive = staging / "chrome.zip"
            print(f"Installing pinned Chrome {version} from {url}", flush=True)
            with (
                urllib.request.urlopen(url, timeout=60) as response,
                archive.open("wb") as output,
            ):
                shutil.copyfileobj(response, output)
            extract_browser(archive, staging)
            unpacked = browser_executable(staging, target)
            actual = subprocess.run(
                [str(unpacked), "--version"],
                capture_output=True,
                text=True,
                check=True,
                timeout=30,
            ).stdout
            if version not in actual.split():
                raise ValueError(f"Chrome release identity mismatch: {actual.strip()}")
            (staging / "installation.json").write_text(
                json.dumps(
                    {
                        "identity": release_identity(pin, target),
                        "provenance": pin,
                        "criticalFiles": critical_files(staging, target),
                    },
                    indent=2,
                )
                + "\n"
            )
            staging.rename(destination)
    return executable

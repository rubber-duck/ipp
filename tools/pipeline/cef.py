"""Locked CEF installations and the small external-runtime build-script patch.

Published installations are immutable. A shared flock spans every build/run;
explicit pruning takes the exclusive lock and never follows adopted symlinks.
Only the installer downloads, and warm validation needs stat calls, not hashes
of the multi-gigabyte runtime. Receipts retain the initial content hashes.
"""

from collections.abc import Iterator
from contextlib import ExitStack, contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import sys
import tarfile
import tempfile
from typing import BinaryIO, NotRequired, TypedDict
import urllib.request

from .model import ROOT


class RuntimePin(TypedDict):
    version: str
    platform: str
    archive: str
    sha1: str
    required: list[str]
    crate: str
    crate_sha256: str
    build_sha256: str


class FileReceipt(TypedDict):
    stat: list[int]
    sha256: str


class Receipt(TypedDict):
    identity: str
    files: dict[str, FileReceipt]
    archive_sha1: NotRequired[str]
    archive_layout: NotRequired[int]


SOURCE = ROOT / "tests/rendering/cef"
PIN: RuntimePin = json.loads((SOURCE / "runtime.json").read_text())


def cache_root() -> Path:
    return Path(os.environ.get("IPP_CEF_CACHE", Path.home() / ".cache/ipp-cef"))


def identity(pin: RuntimePin) -> str:
    return f"{pin['version']}-{pin['platform']}"


def digest(path: Path, algorithm: str = "sha256") -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, algorithm).hexdigest()


def lock_file(
    handle: BinaryIO, *, shared: bool = False, nonblocking: bool = False
) -> None:
    if sys.platform == "win32":
        raise RuntimeError("CEF DMA-BUF installation manager requires Linux")
    else:
        import fcntl

        mode = (fcntl.LOCK_SH if shared else fcntl.LOCK_EX) | (
            fcntl.LOCK_NB if nonblocking else 0
        )
        fcntl.flock(handle, mode)


@contextmanager
def locked(
    root: Path, key: str, *, nonblocking: bool = False, shared: bool = False
) -> Iterator[BinaryIO]:
    directory = root / "locks"
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / f"{key}.lock").open("a+b") as handle:
        lock_file(handle, shared=shared, nonblocking=nonblocking)
        yield handle


def file_stamp(path: Path) -> list[int]:
    stat = path.stat()
    return [stat.st_size, stat.st_mtime_ns, stat.st_ctime_ns, stat.st_ino]


def receipt(directory: Path, key: str) -> Receipt:
    files: dict[str, FileReceipt] = {
        p.relative_to(directory).as_posix(): {
            "stat": file_stamp(p),
            "sha256": digest(p),
        }
        for p in sorted(directory.rglob("*"))
        if p.is_file()
    }
    return {"identity": key, "files": files}


def verify_receipt(directory: Path, record: Receipt, key: str) -> None:
    if record["identity"] != key:
        raise RuntimeError(f"Shared CEF cache identity mismatch: {directory}")
    for name, value in record["files"].items():
        if file_stamp(directory / name) != value["stat"]:
            raise RuntimeError(f"Immutable CEF cache file changed: {directory / name}")


def verify_runtime(runtime: Path, pin: RuntimePin) -> None:
    archive = json.loads((runtime / "archive.json").read_text())
    if archive.get("name") != pin["archive"] or archive.get("sha1") != pin["sha1"]:
        raise RuntimeError(f"CEF archive identity mismatch: {runtime}")
    header = (runtime / "include/cef_version.h").read_text()
    if f'#define CEF_VERSION "{pin["version"]}"' not in header:
        raise RuntimeError(f"CEF header version mismatch: {runtime}")
    for name in pin["required"]:
        if not (runtime / name).is_file() or (runtime / name).stat().st_size == 0:
            raise RuntimeError(f"Missing CEF runtime file: {runtime / name}")


def fetch(url: str, destination: Path, expected: str, algorithm: str) -> None:
    with urllib.request.urlopen(url, timeout=60) as source:
        with destination.open("wb") as target:
            shutil.copyfileobj(source, target, 1024 * 1024)
    if digest(destination, algorithm) != expected:
        raise RuntimeError(f"Downloaded checksum mismatch: {destination.name}")


def runtime_archive(root: Path, pin: RuntimePin, *, offline: bool) -> Path:
    directory = root / "archives"
    directory.mkdir(parents=True, exist_ok=True)
    archive = directory / pin["archive"]
    if archive.is_file():
        if digest(archive, "sha1") != pin["sha1"]:
            raise RuntimeError(f"CEF archive checksum mismatch: {archive}")
        return archive
    old = Path(os.environ.get("CEF_PATH", Path.home() / ".cache/ipp-cef-154"))
    candidates = [
        old / pin["archive"],
        old / pin["version"].split("+")[0] / pin["archive"],
    ]
    legacy = next((p for p in candidates if p.is_file()), None)
    if legacy is not None:
        if digest(legacy, "sha1") != pin["sha1"]:
            raise RuntimeError(f"CEF archive checksum mismatch: {legacy}")
        archive.symlink_to(legacy.resolve())
        return archive
    if offline:
        raise RuntimeError("CEF archive is absent; offline mode cannot download it")
    partial = directory / f".{pin['archive']}.partial"
    try:
        fetch(
            f"https://cef-builds.spotifycdn.com/{pin['archive']}",
            partial,
            pin["sha1"],
            "sha1",
        )
        partial.replace(archive)
    finally:
        partial.unlink(missing_ok=True)
    return archive


def verify_adopted(
    runtime: Path, root: Path, pin: RuntimePin, record: Receipt, *, offline: bool
) -> None:
    # Compare in place against the checksum-verified archive. Do not bless
    # self-reported headers/metadata or rewrite/copy an existing installation.
    archive = runtime_archive(root, pin, offline=offline)
    expected = set(record["files"]) - {"archive.json"}
    seen: set[str] = set()
    with tarfile.open(archive, "r|bz2") as bundle:
        for member in bundle:
            if not member.isfile():
                continue
            parts = PurePosixPath(member.name).parts[1:]
            if not parts:
                continue
            if parts[0] in ("Release", "Resources"):
                parts = parts[1:]
            elif parts[0] not in (
                "include",
                "cmake",
                "libcef_dll",
                "CMakeLists.txt",
                "CREDITS.html",
            ):
                continue
            name = PurePosixPath(*parts).as_posix()
            if name not in expected:
                raise RuntimeError(
                    f"Adopted CEF file missing from installation: {runtime / name}"
                )
            source = bundle.extractfile(member)
            if source is None:
                raise RuntimeError(f"Missing archived CEF file: {name}")
            with source:
                hasher = hashlib.sha256()
                while data := source.read(1024 * 1024):
                    hasher.update(data)
                actual = hasher.hexdigest()
            if actual != record["files"][name]["sha256"]:
                raise RuntimeError(
                    f"Adopted CEF file differs from verified archive: {runtime / name}"
                )
            seen.add(name)
    if seen != expected:
        raise RuntimeError(
            f"Adopted CEF files absent from verified archive: {sorted(expected - seen)}"
        )
    record["archive_sha1"] = pin["sha1"]
    record["archive_layout"] = 1


def install_runtime(stage: Path, root: Path, pin: RuntimePin, *, offline: bool) -> None:
    archive = runtime_archive(root, pin, offline=offline)
    with tarfile.open(archive) as bundle:
        bundle.extractall(stage / "extracted", filter="data")
    extracted = stage / "extracted" / pin["archive"].removesuffix(".tar.bz2")
    runtime = stage / "runtime"
    (extracted / "Release").rename(runtime)
    for item in (extracted / "Resources").iterdir():
        item.rename(runtime / item.name)
    for name in ("include", "cmake", "libcef_dll", "CMakeLists.txt", "CREDITS.html"):
        (extracted / name).rename(runtime / name)
    (runtime / "archive.json").write_text(
        json.dumps({"type": "minimal", "name": pin["archive"], "sha1": pin["sha1"]})
        + "\n"
    )
    shutil.rmtree(stage / "extracted")


def adoption_candidates(pin: RuntimePin) -> list[Path]:
    explicit = os.environ.get("IPP_CEF_RUNTIME")
    if explicit:
        return [Path(explicit)]
    old = Path(os.environ.get("CEF_PATH", Path.home() / ".cache/ipp-cef-154"))
    return [old, old / pin["version"].split("+")[0] / "cef_linux_x86_64"]


@contextmanager
def installation(
    root: Path | None = None,
    pin: RuntimePin | None = None,
    *,
    offline: bool = False,
    candidates: list[Path] | None = None,
) -> Iterator[Path]:
    root = cache_root() if root is None else root
    pin = PIN if pin is None else pin
    key = identity(pin)
    published = root / "installations" / key
    with ExitStack() as lifetime:
        with locked(root, f"{key}-setup"):
            lease = lifetime.enter_context(locked(root, key, shared=True))
            if not published.exists():
                lock_file(lease)
            if not published.exists():
                published.parent.mkdir(parents=True, exist_ok=True)
                # Only our unpublished partials for this exact identity are reclaimed.
                for stale in published.parent.glob(f".{key}-*"):
                    shutil.rmtree(stale)
                stage = Path(tempfile.mkdtemp(prefix=f".{key}-", dir=published.parent))
                try:
                    available = (
                        adoption_candidates(pin) if candidates is None else candidates
                    )
                    adopted = next(
                        (p for p in available if (p / "libcef.so").is_file()), None
                    )
                    if adopted is not None:
                        verify_runtime(adopted, pin)
                        (stage / "runtime").symlink_to(
                            adopted.resolve(), target_is_directory=True
                        )
                    else:
                        install_runtime(stage, root, pin, offline=offline)
                    runtime = stage / "runtime"
                    verify_runtime(runtime, pin)
                    record = receipt(runtime, key)
                    if adopted is not None:
                        verify_adopted(runtime, root, pin, record, offline=offline)
                    record["archive_sha1"] = pin["sha1"]
                    record["archive_layout"] = 1
                    (stage / "receipt.json").write_text(json.dumps(record) + "\n")
                    stage.rename(published)
                finally:
                    if stage.exists():
                        shutil.rmtree(stage)
            runtime = published / "runtime"
            verify_runtime(runtime, pin)
            record = json.loads((published / "receipt.json").read_text())
            verify_receipt(runtime, record, key)
            if (
                record.get("archive_sha1") != pin["sha1"]
                or record.get("archive_layout") != 1
            ):
                # Upgrade an earlier adoption receipt under the setup lock;
                # runtime bytes stay unchanged and existing consumers stay valid.
                verify_adopted(runtime, root, pin, record, offline=offline)
                temporary = published / "receipt.json.partial"
                temporary.write_text(json.dumps(record) + "\n")
                temporary.replace(published / "receipt.json")
            # Atomically downgrade; prune cannot race between verification and use.
            lock_file(lease, shared=True)
        yield runtime.resolve()


def prune(root: Path | None = None, pin: RuntimePin | None = None) -> None:
    root = cache_root() if root is None else root
    pin = PIN if pin is None else pin
    key = identity(pin)
    with locked(root, f"{key}-setup", nonblocking=True):
        with locked(root, key, nonblocking=True):
            published = root / "installations" / key
            if published.exists():
                shutil.rmtree(published)


def patched_package(
    root: Path | None = None, pin: RuntimePin | None = None, *, offline: bool = False
) -> Path:
    root = cache_root() if root is None else root
    pin = PIN if pin is None else pin
    patch = SOURCE / "cef-dll-sys-build.patch"
    key = f"{pin['crate_sha256']}-{digest(patch)}"
    published = root / "packages" / key
    with locked(root, key):
        if not published.exists():
            published.parent.mkdir(parents=True, exist_ok=True)
            for stale in published.parent.glob(f".{key}-*"):
                shutil.rmtree(stale)
            stage = Path(tempfile.mkdtemp(prefix=f".{key}-", dir=published.parent))
            try:
                cargo = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
                archives = sorted(
                    (cargo / "registry/cache").glob(f"*/{pin['crate']}.crate")
                )
                if archives:
                    archive = archives[0]
                    if digest(archive) != pin["crate_sha256"]:
                        raise RuntimeError(
                            f"Cargo archive checksum mismatch: {archive}"
                        )
                else:
                    if offline:
                        raise RuntimeError("Patched CEF bindings unavailable offline")
                    archive = stage / "package.crate"
                    fetch(
                        f"https://static.crates.io/crates/cef-dll-sys/{pin['crate']}.crate",
                        archive,
                        pin["crate_sha256"],
                        "sha256",
                    )
                with tarfile.open(archive) as bundle:
                    bundle.extractall(stage, filter="data")
                package = stage / pin["crate"]
                if digest(package / "build.rs") != pin["build_sha256"]:
                    raise RuntimeError(
                        "Upstream CEF build.rs differs from reviewed patch base"
                    )
                # This maintained patch has one exact hunk. Reject drift instead
                # of fuzzing line offsets or requiring a system patch executable.
                lines = patch.read_text().splitlines(keepends=True)[3:]
                before = "".join(
                    line[1:] for line in lines if line.startswith((" ", "-"))
                )
                after = "".join(
                    line[1:] for line in lines if line.startswith((" ", "+"))
                )
                build = package / "build.rs"
                original = build.read_text()
                if original.count(before) != 1:
                    raise RuntimeError("CEF build patch does not match exactly once")
                build.write_text(original.replace(before, after, 1))
                (stage / "receipt.json").write_text(
                    json.dumps(receipt(package, key)) + "\n"
                )
                if archive.parent == stage:
                    archive.unlink()
                stage.rename(published)
            finally:
                if stage.exists():
                    shutil.rmtree(stage)
        package = published / pin["crate"]
        verify_receipt(
            package, json.loads((published / "receipt.json").read_text()), key
        )
        return package

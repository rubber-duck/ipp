"""Atomic evidence, content identities, and process-scoped workspace locking."""

from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Iterator


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f"{path.name}.tmp-{os.getpid()}")
    try:
        temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_identity(root: Path) -> dict:
    """Hash current tracked/unignored source bytes, including dirty and new files."""
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        capture_output=True,
        check=True,
    )
    names = sorted(set(os.fsdecode(result.stdout).split("\0")) - {""})
    hasher = hashlib.sha256()
    for name in names:
        path = root / name
        hasher.update(name.encode() + b"\0")
        if path.is_symlink():
            hasher.update(b"link:" + os.fsencode(path.readlink()))
        elif path.is_file():
            hasher.update(digest(path).encode())
        else:
            hasher.update(b"missing")
        hasher.update(b"\0")
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    return {"revision": revision, "sourceSha256": hasher.hexdigest()}


def output_records(root: Path, outputs: tuple[str, ...]) -> list[dict]:
    records = []
    for name in outputs:
        path = root / name
        if not path.exists():
            raise ValueError(f"Declared output is missing: {name}")
        paths = (
            sorted(p for p in path.rglob("*") if p.is_file())
            if path.is_dir()
            else [path]
        )
        for item in paths:
            records.append(
                {
                    "path": item.relative_to(root).as_posix(),
                    "bytes": item.stat().st_size,
                    "sha256": digest(item),
                }
            )
    return records


@contextmanager
def workspace_lock(root: Path) -> Iterator[None]:
    """OS locks release after crashes; never guess whether another owner's PID is stale."""
    path = root / "target/pipeline/workspace.lock"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as stream:
        if path.stat().st_size == 0:
            stream.write(b"\0")
            stream.flush()
        stream.seek(0)
        try:
            if sys.platform == "win32":
                import msvcrt

                msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl

                fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            raise ValueError(
                "Another pipeline is using this checkout. Wait or use a separate source worktree."
            ) from error
        try:
            yield
        finally:
            stream.seek(0)
            if sys.platform == "win32":
                import msvcrt

                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl

                fcntl.flock(stream, fcntl.LOCK_UN)

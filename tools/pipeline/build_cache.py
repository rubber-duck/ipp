"""Verify local build products; test/check outcomes are never reusable products.

One record per product points to its original build manifest. The workspace lock
serializes writers. Cache records become visible only after a run's source stayed
unchanged; a failed rebuild first invalidates the old record. No stored command
is executed and no output is restored over the checkout.
"""

from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import sys

from .artifacts import digest, output_records, source_records, write_json
from .model import Task


def identity(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


class BuildCache:
    def __init__(self, root: Path, report: dict, environment: dict, *, enabled: bool):
        self.root = root
        self.report = report
        self.enabled = enabled
        self.sources = source_records(root)
        self.environment = identity(
            {
                key: value
                for key, value in environment.items()
                if key
                not in {
                    "_",
                    "SHLVL",
                    "PWD",
                    "OLDPWD",
                    "IPP_PIPELINE_RUN",
                    "BEADS_ACTOR",
                }
            }
        )
        self.configuration = self.external_configuration(environment)

    def external_configuration(self, environment: dict) -> dict:
        # Cargo merges ancestor and user configuration; these can change without
        # a repository edit or a RUSTFLAGS change. Store hashes, never contents.
        cargo = Path(environment.get("CARGO_HOME", Path.home() / ".cargo"))
        directories = {
            cargo,
            *(parent / ".cargo" for parent in (self.root, *self.root.parents)),
        }
        paths = {
            directory / name
            for directory in directories
            for name in ("config", "config.toml")
        }
        paths.update(
            {Path.home() / ".npmrc", self.root / "node_modules/.package-lock.json"}
        )
        return {
            str(path): digest(path) if path.is_file() else None
            for path in sorted(paths)
        }

    def record_path(self, task: Task) -> Path:
        return self.root / "target/pipeline/products" / f"{identity(task.id)}.json"

    def key(self, task: Task, inputs: dict) -> str | None:
        if (
            not task.id.startswith("build:")
            or not task.outputs
            or task.interactive
            or task.mutates_source
        ):
            return None
        requirements = set(task.requirements) | {"python"}
        environment = [
            item for item in self.report["environment"] if item["name"] in requirements
        ]
        if task.requirements and not set(task.requirements).issubset(
            item["name"] for item in environment if item["ready"]
        ):
            # Tests may disable preflight; real products require verified tools.
            return None
        dependencies = {}
        for name, path in inputs.items():
            if not path:
                return None
            manifest = json.loads(Path(path).read_text())
            dependencies[name] = manifest["artifacts"]
        sources = {
            name: value
            for name, value in self.sources.items()
            if not task.source_roots
            or "/" not in name
            or any(
                name.startswith(prefix) if prefix.endswith("/") else name == prefix
                for prefix in task.source_roots
            )
        }
        # Directory symlinks can hide unenumerated external inputs. Rebuild them.
        if any(value.startswith("directory-link:") for value in sources.values()):
            return None
        return identity(
            {
                "version": 1,
                "root": str(self.root),
                # Some products stamp Git revision into their build identity.
                "revision": self.report["source"]["revision"],
                "task": asdict(task),
                "sources": sources,
                "dependencies": dependencies,
                "tools": environment,
                "python": [sys.executable, sys.version],
                "platform": self.report["platform"],
                "environment": self.environment,
                "configuration": self.configuration,
            }
        )

    def reuse(self, task: Task, key: str) -> dict | None:
        if not self.enabled:
            return None
        try:
            saved = json.loads(self.record_path(task).read_text())
            if saved["key"] != key:
                return None
            path = Path(saved["manifest"])
            if not path.resolve().is_relative_to(
                (self.root / "target/pipeline/runs").resolve()
            ):
                return None
            if digest(path) != saved["manifestSha256"]:
                return None
            manifest = json.loads(path.read_text())
            if manifest["task"] != task.id or manifest.get("buildKey") != key:
                return None
            if output_records(self.root, task.outputs) != manifest["artifacts"]:
                return None
            return {"manifest": str(path), "source": manifest["source"]}
        except OSError, ValueError, KeyError, TypeError:
            return None

    def invalidate(self, task: Task) -> None:
        self.record_path(task).unlink(missing_ok=True)

    def publish(self, task: Task, result: dict) -> None:
        path = Path(result["manifest"])
        manifest = json.loads(path.read_text())
        if not manifest.get("buildKey") or result.get("reusedFrom"):
            return
        # A later test may have modified the product. Do not bless those bytes.
        try:
            if output_records(self.root, task.outputs) != manifest["artifacts"]:
                return
        except OSError, ValueError:
            return
        write_json(
            self.record_path(task),
            {
                "key": manifest["buildKey"],
                "manifest": str(path),
                "manifestSha256": digest(path),
            },
        )

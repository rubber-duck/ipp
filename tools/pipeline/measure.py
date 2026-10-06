"""Measure artifacts and current browser distributions without size ceilings."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any
import zlib

from .artifacts import source_identity, write_json
from .model import ROOT
from .registries import PROFILES
from .wasm_sizes import analyze_wasm


def artifact(path: Path) -> dict:
    data = path.read_bytes()
    return {
        "path": str(path),
        "bytes": len(data),
        "gzip_bytes": len(gzip.compress(data, compresslevel=9, mtime=0)),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def distribution(root: Path, profile: str, features: list[str]) -> dict:
    directory = root / "target/browser-build" / profile
    manifest_path = directory / "build-report.json"
    manifest = json.loads(manifest_path.read_text())
    if manifest["configuration"] != profile or manifest["features"] != features:
        raise ValueError(f"Distribution identity mismatch: {profile}")
    wasm = directory / "runtime.wasm"
    javascript = sorted(directory.glob("*.js"))
    # Use the renderer's actual generated, comment-stripped shader copies.
    shader_root = root / "target/wasm32-unknown-unknown/release-small/build"
    shaders = tuple(sorted(shader_root.glob("ipp-render-gl-*/out/render/**/*")))
    shaders = tuple(path for path in shaders if path.is_file())
    record = artifact(wasm)
    record["wasm"] = analyze_wasm(wasm.read_bytes(), shaders)
    js = [artifact(path) for path in javascript]
    ancillary = []
    for declared in manifest["artifacts"]:
        path = root / declared["path"]
        if not path.resolve().is_relative_to(directory.resolve()):
            raise ValueError("Distribution manifest path escapes its directory")
        if path != wasm and path.suffix != ".js":
            ancillary.append(artifact(path))
    measured = {
        str(Path(item["path"]).relative_to(root)): item
        for item in [record, *js, *ancillary]
    }
    for declared in manifest["artifacts"]:
        item = measured.get(declared["path"])
        if item is not None and item["sha256"] != declared["sha256"]:
            raise ValueError(
                f"Artifact differs from verified distribution: {declared['path']}"
            )
    declared_paths = {item["path"] for item in manifest["artifacts"]}
    if set(measured) != declared_paths:
        raise ValueError(f"Distribution artifact inventory mismatch: {profile}")
    return {
        "profile": profile,
        "features": features,
        "cargo_profile": "release-small",
        "target": "wasm32-unknown-unknown",
        "manifest": artifact(manifest_path),
        "wasm": record,
        "javascript": js,
        "ancillary_artifacts": ancillary,
        "totals": {
            "raw_bytes": record["bytes"] + sum(item["bytes"] for item in js),
            "gzip_bytes": record["gzip_bytes"] + sum(item["gzip_bytes"] for item in js),
        },
        "accounting": "WASM plus unique emitted JS files; per-file gzip; shaders already in WASM; notices listed separately; maps/TS/contracts excluded",
    }


def main(argv: list[str]) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="*", type=Path)
    parser.add_argument(
        "--profile",
        action="append",
        choices=["headless", "render", "render-instrumentation"],
    )
    parser.add_argument("--output", type=Path)
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--native-shim", action="append", default=[], type=Path)
    args = parser.parse_args(argv)
    if not args.artifacts and not args.profile:
        parser.error("Select explicit artifacts or --profile")
    if (
        not args.profile
        and not args.output
        and not args.compare
        and not args.native_shim
    ):
        print(json.dumps([artifact(path) for path in args.artifacts], indent=2))
        return
    profiles = PROFILES["browser"]
    report: dict[str, Any] = {
        "schema_version": 1,
        "source": source_identity(ROOT),
        "source_role": "measurement checkout; artifact build inputs belong to pipeline build manifests",
        "pipeline_run": os.environ.get("IPP_PIPELINE_RUN"),
        "build_configuration": [
            artifact(ROOT / name)
            for name in (
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                "tools/pipeline/profiles.json",
            )
        ],
        "toolchain": {
            **{
                tool: subprocess.check_output([tool, "--version"], text=True).strip()
                for tool in ("rustc", "cargo", "node")
            },
            "python": sys.version,
            "zlib": zlib.ZLIB_RUNTIME_VERSION,
        },
        "distributions": [
            distribution(ROOT, name, profiles[name]["features"])
            for name in dict.fromkeys(args.profile or [])
        ],
        "artifacts": [artifact(path) for path in dict.fromkeys(args.artifacts)],
        "native_shims": [artifact(path) for path in dict.fromkeys(args.native_shim)],
        "native_shim_accounting": "explicit platform shim artifacts only; excluded from browser totals",
    }
    run_directory = Path(report["pipeline_run"]) if report["pipeline_run"] else None
    for item in report["distributions"]:
        item["build_evidence"] = None
        if run_directory:
            for manifest in sorted(run_directory.glob("*.manifest.json")):
                evidence = json.loads(manifest.read_text())
                if evidence.get("task") == f"build:browser:{item['profile']}":
                    item["build_evidence"] = {
                        "manifest": artifact(manifest),
                        "source": evidence["source"],
                    }
    if args.compare:
        previous = json.loads(args.compare.read_text())
        before = {item["profile"]: item for item in previous["distributions"]}
        report["comparison"] = {
            "baseline": artifact(args.compare),
            "deltas": [
                {
                    "profile": item["profile"],
                    "wasm_raw_bytes": item["wasm"]["bytes"]
                    - before[item["profile"]]["wasm"]["bytes"],
                    "wasm_gzip_bytes": item["wasm"]["gzip_bytes"]
                    - before[item["profile"]]["wasm"]["gzip_bytes"],
                    "code_payload_bytes": item["wasm"]["wasm"]["code_payload_bytes"]
                    - before[item["profile"]]["wasm"]["wasm"]["code_payload_bytes"],
                    "data_payload_bytes": item["wasm"]["wasm"]["data_payload_bytes"]
                    - before[item["profile"]]["wasm"]["wasm"]["data_payload_bytes"],
                    **{
                        key: item["totals"][key]
                        - before[item["profile"]]["totals"][key]
                        for key in ("raw_bytes", "gzip_bytes")
                    },
                }
                for item in report["distributions"]
                if item["profile"] in before
            ],
        }
    if args.output:
        write_json(args.output, report)
    if report["pipeline_run"]:
        write_json(Path(report["pipeline_run"]) / "distribution-sizes.json", report)
    print(json.dumps(report, indent=2))

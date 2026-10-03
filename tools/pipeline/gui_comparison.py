"""Bounded A/B/B/A GUI measurements using caller-owned prepared checkouts."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import time

from .artifacts import digest, write_json
from .model import ROOT, Task
from .processes import node


def register_pair(
    args: argparse.Namespace,
    tasks: dict[str, Task],
    dependencies: tuple[str, ...],
    checkout: Path,
) -> list[str]:
    from .catalog import operation

    config = {
        "checkout": str(checkout),
        "backend": args.backend,
        "pairs": args.pairs,
        "repetitions": args.repetitions,
        "samples": args.samples,
        "egl_dir": args.egl_dir,
        "output": str(Path(args.output or "target/performance/gui-paired").resolve()),
    }
    tasks["benchmark:gui-paired"] = Task(
        "benchmark:gui-paired",
        "Alternate ordinary GUI timing between prepared checkouts",
        operation("gui-paired", json.dumps(config)),
        () if args.reuse_build else dependencies,
        ("node", "browser", *(["gles"] if args.backend == "native" else ["wasm"])),
        timeout=28800,
    )
    return ["benchmark:gui-paired"]


def compatibility(report: dict) -> dict:
    """Compare stable settings/identity, never dynamic counters or source revisions."""
    if report["build"].get("sourceMatches") is not True:
        raise ValueError(
            "Paired timing requires verified build/current-source identity"
        )
    device = report.get("device") or {}
    return {
        "workload": report["workloadSha256"],
        "fixtureSources": report["fixture"]["sources"],
        "assets": sorted(
            (Path(item["path"]).name, item["sha256"]) for item in report["assets"]
        ),
        "machine": report["machine"],
        "browser": report["browser"],
        "arrangement": report["build"]["arrangement"],
        "mode": report["measurementMode"],
        "repetitions": report["repetitions"],
        "samples": report["samplesPerSweep"],
        "device": {
            key: device.get(key)
            for key in (
                "api",
                "version",
                "renderer",
                "unmaskedRenderer",
                "unmaskedVendor",
            )
        },
        "software": report["softwareRenderer"],
        "compositor": report["compositor"]["settings"],
        "contract": report["build"]["contractSha256"],
    }


def observed_compositor(report: dict) -> list:
    result = []
    for item in report["compositor"]["observed"]:
        caches = (item["surfaces"] or {}).get("surfaceCaches", [])
        # Repaint/reuse/animated describes one frame of the same cached mode.
        states = sorted(
            (
                "cached"
                if cache["mode"] in ("reused", "repainted", "animated")
                else cache["mode"],
                cache["band"],
                cache["width"],
                cache["height"],
            )
            for cache in caches
        )
        result.append((item["sweep"], states))
    return result


def timing_values(report: dict) -> dict[str, list[float]]:
    result: dict[str, list[float]] = {}
    for sweep in report["sweeps"]:
        mounted = sweep.get("mount", {}).get("buildToFrameMs")
        if mounted is not None:
            result.setdefault(f"{sweep['sweep']['name']}:buildToFrameMs", []).append(
                mounted
            )
        for action, timing in sweep["timings"].items():
            for metric in ("updateMs", "updateToFrameMs"):
                value = timing[metric]
                if value is not None:
                    result.setdefault(
                        f"{sweep['sweep']['name']}:{action}:{metric}:median", []
                    ).append(value["medianMs"])
    for panel in report.get("panelDiagnostics", []):
        prefix = f"entities-{panel['observedEntities']}"
        for metric in ("buildMs", "inspectMs"):
            if panel[metric] is not None:
                result.setdefault(f"{prefix}:{metric}", []).append(panel[metric])
        for action, timing in panel["timings"].items():
            for metric in ("updateMs", "updateToFrameMs"):
                if timing[metric] is not None:
                    result.setdefault(f"{prefix}:{action}:{metric}:median", []).append(
                        timing[metric]["medianMs"]
                    )
    return result


def compare(config: dict) -> None:
    output = Path(config["output"])
    output.mkdir(parents=True, exist_ok=True)
    checkouts = {"A": ROOT, "B": Path(config["checkout"])}
    if checkouts["A"].resolve() == checkouts["B"].resolve():
        raise ValueError("Paired runs require two distinct prepared checkouts")
    harness = ROOT / "target/gui-stress/gui-stress.js"
    if not harness.is_file():
        raise ValueError("Prepare GUI fixtures before paired runs")
    reports: list[dict] = []
    comparable: dict | None = None
    modes: list | None = None
    report_path = output / "gui-paired-report.json"
    summary: dict = {
        "order": [
            label for _ in range(config["pairs"]) for label in ("A", "B", "B", "A")
        ],
        "checkouts": {label: str(path) for label, path in checkouts.items()},
        "harness": {"path": str(harness), "sha256": digest(harness)},
        "runs": reports,
        "status": "running",
        "interpretation": "Per-run timing distributions and paired median differences; no timing thresholds. Machine load captures expose external contention. Caller-owned source is never reset, committed or overwritten.",
    }
    write_json(report_path, summary)
    try:
        for index, label in enumerate(summary["order"]):
            checkout = checkouts[label]
            destination = output / f"{index + 1:02d}-{label}"
            arguments = [
                node(),
                str(harness),
                "browser" if config["backend"] == "browser" else "native-gles",
                str(config["repetitions"]),
                str(destination),
                str(config["samples"]),
            ]
            if config["backend"] == "native":
                arguments.extend(
                    [
                        config["egl_dir"],
                        "--host-build",
                        str(checkout / "target/performance-build/gui-native"),
                    ]
                )
            started = time.time()
            subprocess.run(
                arguments, cwd=checkout, check=True, timeout=7200, env=os.environ.copy()
            )
            path = destination / "gui-stress-report.json"
            report = json.loads(path.read_text())
            settings = compatibility(report)
            compositor = observed_compositor(report)
            reports.append(
                {
                    "label": label,
                    "started": started,
                    "report": str(path),
                    "sha256": digest(path),
                    "source": report["source"],
                    "build": report["build"],
                    "contention": report["contention"],
                    "timings": timing_values(report),
                    "compatibility": settings,
                    "observedCompositor": compositor,
                }
            )
            if settings["mode"] != "post-admission-draw":
                raise ValueError("Paired comparison requires ordinary timing reports")
            if comparable is not None and (
                settings != comparable or compositor != modes
            ):
                raise ValueError(
                    f"Run {index + 1} differs in workload, environment, renderer or compositor; reports retained"
                )
            comparable = settings
            modes = compositor
            write_json(report_path, summary)
        differences = []
        for block in range(config["pairs"]):
            runs = reports[block * 4 : block * 4 + 4]
            keys = set(runs[0]["timings"])
            if any(set(item["timings"]) != keys for item in runs):
                raise ValueError("Paired reports have different timing metrics")
            differences.append(
                {
                    "block": block,
                    "B_minus_A_ms": {
                        key: sum(
                            sum(item["timings"][key]) / len(item["timings"][key])
                            for item in runs[1:3]
                        )
                        / 2
                        - sum(
                            sum(item["timings"][key]) / len(item["timings"][key])
                            for item in (runs[0], runs[3])
                        )
                        / 2
                        for key in sorted(keys)
                    },
                }
            )
        summary.update(
            {
                "status": "complete",
                "compatibility": comparable,
                "observedCompositor": modes,
                "differences": differences,
            }
        )
    except BaseException as error:
        summary.update({"status": "failed", "error": str(error)})
        raise
    finally:
        write_json(report_path, summary)

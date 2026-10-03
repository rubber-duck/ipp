"""Bounded, opt-in native chart/data measurements; no regression registration."""

import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path

from .artifacts import source_identity, write_json
from .builds import cargo, compile_client, native_contract, product
from .model import ROOT, Task
from .processes import node, run

DIRECTORY = ROOT / "target/performance-build/chart-data"
CHART_PROFILE = "services::render::plot_label_layout::profile_tests::chart_profile"


def build() -> None:
    with product(DIRECTORY) as output:
        cargo("build", "--release", "-p", "ipp-server", "--example", "gles_host")
        suffix = ".exe" if os.name == "nt" else ""
        executable = output / f"gles_host{suffix}"
        shutil.copy2(ROOT / f"target/release/examples/gles_host{suffix}", executable)
        native_contract(output / "contract.bin", release=True)
        compile_client(output, output / "contract.bin")
        run([node(), "tools/build/chart-data.mjs", str(output)])
        write_json(
            output / "build-identity.json",
            {
                "source": source_identity(ROOT),
                "profile": "release",
                "features": [],
                "instrumented": False,
                "executableSha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                "contractSha256": hashlib.sha256(
                    (output / "contract.bin").read_bytes()
                ).hexdigest(),
                "fixtureSha256": hashlib.sha256(
                    (output / "chart-data.js").read_bytes()
                ).hexdigest(),
            },
        )


def register(tasks: dict[str, Task]) -> None:
    from .catalog import operation

    tasks["build:performance-chart-data"] = Task(
        "build:performance-chart-data",
        "Build release GLES Host and bounded chart/data benchmark",
        operation("chart-benchmark-build"),
        ("build:client", "build:font-assets"),
        ("node", "npm", "rust"),
        ("target/performance-build/chart-data",),
    )
    for kind in ("data", "chart"):
        for mode in ("timing", "allocations"):
            name = f"chart-{kind}-{mode}"
            tasks[f"build:performance-{name}"] = Task(
                f"build:performance-{name}",
                f"Build isolated {kind} {mode} diagnostic",
                operation("chart-diagnostic-build", kind, mode),
                (),
                ("rust",),
                (f"target/performance-build/{name}",),
            )


def plan(args: argparse.Namespace, tasks: dict[str, Task]) -> list[str]:
    if args.backend != "native":
        raise ValueError("chart-data currently requires the real native GLES driver")
    if not args.diagnostic and not args.egl_dir:
        raise ValueError("chart-data requires --egl-dir")
    if not 1 <= args.samples <= 64 or not 1 <= args.repetitions <= 8:
        raise ValueError("chart-data accepts 1..64 samples and 1..8 repetitions")
    if not 0 <= args.warmup <= 16:
        raise ValueError("chart-data warmup must be in 0..16")
    if args.modes and any(
        mode not in ("idle", "edit", "parameter", "animation", "camera")
        for mode in args.modes.split(",")
    ):
        raise ValueError(
            "chart-data modes must select idle,edit,parameter,animation,camera"
        )
    if args.instrumented and not args.diagnostic:
        raise ValueError(
            "Native end-to-end timings require the normal release build; use paired local diagnostics for allocations"
        )
    mode = "allocations" if args.instrumented else "timing"
    dependency = (
        f"build:performance-chart-{args.diagnostic}-{mode}"
        if args.diagnostic
        else "build:performance-chart-data"
    )
    if args.build_only:
        return [dependency]
    configuration = {
        "preset": args.preset or "smoke",
        "samples": args.samples,
        "repetitions": args.repetitions,
        "warmup": args.warmup,
        "cases": args.cases,
        "modes": args.modes,
        "eglDir": args.egl_dir,
        "allowSoftware": args.allow_software,
        "output": args.output or "target/performance/chart-data",
    }
    if args.diagnostic:
        from .catalog import operation

        configuration.update({"kind": args.diagnostic, "mode": mode})
        tasks["benchmark:chart-diagnostic"] = Task(
            "benchmark:chart-diagnostic",
            f"Measure local {args.diagnostic} {mode}",
            operation("chart-diagnostic", json.dumps(configuration)),
            () if args.reuse_build else (dependency,),
            (),
            timeout=900,
        )
        return ["benchmark:chart-diagnostic"]
    tasks["benchmark:chart-data"] = Task(
        "benchmark:chart-data",
        "Measure bounded chart/data native transport and completed frames",
        (node(), str(DIRECTORY / "chart-data.js"), json.dumps(configuration)),
        () if args.reuse_build else (dependency,),
        ("node", "npm", "gles"),
        timeout=900,
    )
    return ["benchmark:chart-data"]


def build_diagnostic(kind: str, allocations: bool) -> None:
    mode = "allocations" if allocations else "timing"
    with product(ROOT / f"target/performance-build/chart-{kind}-{mode}") as output:
        features = ["--features", "instrumentation"] if allocations else []
        if kind == "data":
            cargo(
                "build",
                "--release",
                "-p",
                "ipp-server",
                "--example",
                "data_profile",
                *features,
            )
            suffix = ".exe" if os.name == "nt" else ""
            source = ROOT / f"target/release/examples/data_profile{suffix}"
        else:
            log = output / "build.jsonl"
            cargo(
                "test",
                "--release",
                "-p",
                "ipp-render-gl",
                "--lib",
                "--no-run",
                "--message-format=json",
                *features,
                output=log,
            )
            executables = []
            for line in log.read_text().splitlines():
                if not line.startswith("{"):
                    continue
                record = json.loads(line)
                if record.get("reason") == "compiler-artifact" and record.get(
                    "executable"
                ):
                    executables.append(Path(record["executable"]))
            if len(executables) != 1:
                raise ValueError(
                    f"Expected one Plot profile executable, got {executables}"
                )
            source = executables[0]
        executable = output / ("profile.exe" if os.name == "nt" else "profile")
        shutil.copy2(source, executable)
        write_json(
            output / "build-identity.json",
            {
                "source": source_identity(ROOT),
                "profile": "release",
                "kind": kind,
                "features": ["instrumentation"] if allocations else [],
                "mode": mode,
                "executableSha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
            },
        )


def diagnostic(configuration: dict) -> None:
    """Prebuilt fixture execution only; source generation and build are never timed."""
    kind, mode = configuration["kind"], configuration["mode"]
    product_dir = ROOT / f"target/performance-build/chart-{kind}-{mode}"
    executable = product_dir / ("profile.exe" if os.name == "nt" else "profile")
    identity = json.loads((product_dir / "build-identity.json").read_text())
    if (
        hashlib.sha256(executable.read_bytes()).hexdigest()
        != identity["executableSha256"]
    ):
        raise ValueError("Diagnostic executable changed since build")
    output = Path(configuration["output"]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    records = []
    for repeat in range(configuration["repetitions"]):
        if kind == "chart":
            commands = [
                [
                    str(executable),
                    CHART_PROFILE,
                    "--ignored",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ]
            ]
        else:
            scales = (
                [1000, 10000, 100000] if configuration["preset"] == "full" else [1000]
            )
            cases = (
                configuration["cases"].split(",")
                if configuration["cases"]
                else [
                    "stream-idle",
                    "stream-range-idle",
                    "buffer-edit",
                    "binding-edit",
                    "binding-idle",
                    "binding-stream-edit",
                ]
            )
            commands = [
                [
                    str(executable),
                    "--case",
                    case,
                    "--rows",
                    str(rows),
                    "--bindings",
                    str(bindings),
                    "--iterations",
                    str(configuration["samples"]),
                    *(["--allocations"] if mode == "allocations" else []),
                ]
                for rows in scales
                for case in cases
                for bindings in ([1, 4] if case.startswith("binding") else [1])
            ]
        for index, command in enumerate(commands):
            log = output / f"{kind}-{mode}-{repeat}-{index}.log"
            environment = (
                {"IPP_CHART_PROFILE_MODE": "allocations"}
                if mode == "allocations"
                else {"IPP_CHART_PROFILE_MODE": "timing"}
            )
            run(command, output=log, env=environment)
            for line in log.read_text().splitlines():
                if line.startswith("{"):
                    records.append({"repeat": repeat, **json.loads(line)})
    write_json(
        output / "results.json",
        {
            "configuration": configuration,
            "build": identity,
            "scope": "local diagnostic operations; excludes transport, presentation and GPU",
            "records": records,
        },
    )

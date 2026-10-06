"""Opt-in bounded gallery diagnostics, using maintained build and environment drivers."""

import argparse
from pathlib import Path

from .model import ROOT, Task
from .processes import node


def register(tasks: dict[str, Task]) -> None:
    tasks["build:gallery-trace-fixture"] = Task(
        "build:gallery-trace-fixture",
        "Bundle the maintained gallery trace runner and instrumented application",
        (node(), "tools/products/gallery-trace-fixture.mjs"),
        (
            "build:gallery-fixtures",
            "build:browser:render-instrumentation",
            "build:native",
            "build:react-gui-authoring",
        ),
        ("node", "npm"),
        ("target/gallery-trace",),
    )


def plan(args: argparse.Namespace, tasks: dict[str, Task]) -> list[str]:
    if args.cdp and args.backend != "browser":
        raise ValueError("--cdp is a browser-only optional adapter")
    if args.max_events < 1 or args.max_events > 0xFFFFFFFF:
        raise ValueError("--max-events must be an explicit positive u32 capacity")
    if args.max_artifact_bytes < 1 or args.max_artifact_bytes > 0x1FFFFFFFFFFFFF:
        raise ValueError("--max-artifact-bytes must be a positive safe integer")
    output = Path(args.output).resolve()
    if output == ROOT or ROOT.is_relative_to(output):
        raise ValueError("Trace output must not contain the source checkout")
    dependencies: tuple[str, ...] = ("build:gallery-trace-fixture",)
    requirements: tuple[str, ...] = ("node", "browser")
    if args.backend == "native":
        if not args.egl_dir:
            raise ValueError("Native trace requires --egl-dir or IPP_EGL_LIBRARY_DIR")
        dependencies += ("build:gles-host-instrumentation", "build:shared-host")
        requirements += ("gles",)
    task = Task(
        "trace:gallery-gui",
        f"Capture bounded {args.backend} gallery CPU spans and images",
        (
            node(),
            "target/gallery-trace/gallery-gui-trace.mjs",
            "--backend",
            args.backend,
            "--output",
            str(output),
            "--max-events",
            str(args.max_events),
            "--max-artifact-bytes",
            str(args.max_artifact_bytes),
            *(("--cdp",) if args.cdp else ()),
        ),
        dependencies,
        requirements,
    )
    tasks[task.id] = task
    return [task.id]

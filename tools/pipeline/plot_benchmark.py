"""Ordinary worker/WebGL Plot scaling, separate from correctness regression."""

import argparse
import json
import os
from pathlib import Path

from .artifacts import source_identity
from .model import ROOT, Task
from .processes import node


def plan(args: argparse.Namespace, tasks: dict[str, Task]) -> list[str]:
    if args.backend != "browser" or args.instrumented or args.diagnostic:
        raise ValueError("plot-coverage requires ordinary browser rendering")
    if args.device == "software":
        raise ValueError("Plot throughput requires hardware rendering")
    if not 1 <= args.samples <= 30 or not 1 <= args.repetitions <= 8:
        raise ValueError("plot-coverage accepts 1..30 sample seconds and 1..8 repeats")
    directory = "target/performance-build/plot-coverage"
    tasks["build:performance-plot-coverage"] = Task(
        "build:performance-plot-coverage",
        "Bundle real-client Plot throughput fixture",
        (node(), "tools/performance/plot-coverage.mjs", "build"),
        ("build:browser:render", "build:font-assets", "build:native"),
        ("node", "npm"),
        (directory,),
    )
    if args.build_only:
        return ["build:performance-plot-coverage"]
    configuration = {
        "source": source_identity(ROOT),
        "runtime": str(
            Path(
                os.environ.get(
                    "IPP_PLOT_BENCHMARK_RUNTIME", ROOT / "target/browser-build/render"
                )
            ).resolve()
        ),
        "rows": [600, 2400] if args.preset == "full" else [600],
        "sampleMs": args.samples * 1000,
        "warmupMs": args.warmup * 1000,
        "repetitions": args.repetitions,
        "cases": args.cases,
        "output": args.output or "target/performance/plot-coverage",
    }
    tasks["benchmark:plot-coverage"] = Task(
        "benchmark:plot-coverage",
        "Measure uncapped hardware Plot feature combinations",
        (node(), "tools/performance/plot-coverage.mjs", json.dumps(configuration)),
        () if args.reuse_build else ("build:performance-plot-coverage",),
        ("node", "npm", "browser"),
        timeout=1200,
    )
    return ["benchmark:plot-coverage"]

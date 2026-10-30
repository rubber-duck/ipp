"""Serial native cockpit comparison through the maintained gallery and real Host."""

import argparse
import json

from .artifacts import source_identity
from .model import ROOT, Task
from .processes import node


def plan(args: argparse.Namespace, tasks: dict[str, Task]) -> list[str]:
    if args.backend != "native" or not args.egl_dir:
        raise ValueError("cockpit-surfaces requires native --egl-dir")
    if args.device == "software":
        raise ValueError("Cockpit measurements require hardware native GLES")
    if not 1 <= args.repetitions <= 8 or not 1 <= args.frames <= 600:
        raise ValueError("Cockpit accepts 1..8 repetitions and 1..600 frames")
    if args.compare_checkout or args.pairs != 1:
        raise ValueError("Cockpit compares both rendering paths in the same build")

    product = "gui-native-instrumented" if args.instrumented else "gui-native"
    dependencies = (
        f"build:performance-{product}",
        "build:shared-host",
        "build:font-assets",
        "build:demos-cockpit-native-assets",
        "build:demos-drawings",
    )
    driver = ROOT / "target/performance-build/cockpit-surfaces.mjs"
    tasks["build:performance-cockpit-driver"] = Task(
        "build:performance-cockpit-driver",
        "Bundle native cockpit comparison driver",
        (
            node(),
            "tools/performance/performance.mjs",
            "cockpit-surfaces",
            str(driver),
            str(ROOT / "target/performance-build" / product),
        ),
        dependencies,
        ("node", "npm"),
        (str(driver.relative_to(ROOT)),),
    )
    if args.build_only:
        return ["build:performance-cockpit-driver"]

    configuration = {
        "source": source_identity(ROOT),
        "product": product,
        "instrumented": args.instrumented,
        "framePacing": "uncapped",
        "minimumSampleMs": 0 if args.instrumented else 5000,
        "frames": args.frames,
        "repetitions": args.repetitions,
        "eglDirectory": args.egl_dir,
        "output": args.output or "target/performance/cockpit-surfaces",
        "allowSoftware": args.allow_software,
    }
    tasks["benchmark:cockpit-surfaces"] = Task(
        "benchmark:cockpit-surfaces",
        "Compare analytic and textured cockpit surfaces with matched content",
        (node(), str(driver), json.dumps(configuration)),
        () if args.reuse_build else ("build:performance-cockpit-driver",),
        ("node", "npm", "gles"),
        timeout=1200,
    )
    return ["benchmark:cockpit-surfaces"]

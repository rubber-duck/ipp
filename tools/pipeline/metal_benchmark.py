"""Paired native release benchmarks on Apple hardware."""

import argparse
import json
import sys

from .artifacts import source_identity
from .model import ROOT, Task
from .processes import node


def plan(args: argparse.Namespace, tasks: dict[str, Task]) -> list[str]:
    if sys.platform != "darwin" or args.backend != "native":
        raise ValueError(f"{args.scene} requires native macOS")
    include_gles = args.renderer != "metal"
    if include_gles and not args.egl_dir:
        raise ValueError(f"{args.scene} requires --egl-dir for GLES")
    if args.device == "software" or args.allow_software:
        raise ValueError(f"{args.scene} requires hardware")
    if not 1 <= args.repetitions <= 8 or not 1 <= args.frames <= 10_000:
        raise ValueError(f"{args.scene} accepts 1..8 repetitions and 1..10000 frames")
    if args.compare_checkout or args.pairs != 1:
        raise ValueError(f"{args.scene} measures both backends from the same checkout")

    scene = args.scene
    gallery = scene == "metal-gallery"
    gui = scene == "gui-rendering"
    assets = (
        (
            "build:shared-host",
            "build:font-assets",
            "build:gallery-assets",
            "build:gallery-platformer-native-assets",
            "build:demos-cockpit-native-assets",
            "build:demos-drawings",
        )
        if gallery
        else ("build:shared-host", "build:font-assets", "build:demos-drawings")
        if gui
        else ()
    )

    suffix = "-instrumentation" if args.instrumented else ""
    metal = f"metal-host{suffix}"
    gles = "gui-native-instrumented" if args.instrumented else "gui-native"
    products = []
    if args.renderer != "gles":
        products.append(f"build:{metal}")
    if include_gles:
        products.append(f"build:performance-{gles}")
    contract = (
        f"target/performance-build/{gles}"
        if args.renderer == "gles"
        else f"target/{metal}"
    )
    bundle = f"target/performance-build/{scene}.mjs"
    build = f"build:performance-{scene}"
    tasks[build] = Task(
        build,
        "Bundle the paired native Metal benchmark",
        (
            node(),
            "tools/performance/performance.mjs",
            scene,
            str(ROOT / bundle),
            *([str(ROOT / contract)] if gui else []),
        ),
        (*products, *assets),
        ("node", "npm"),
        (bundle,),
    )
    if args.build_only:
        return [build]

    run = f"benchmark:{scene}"
    tasks[run] = Task(
        run,
        "Measure matched release GLES and Metal presentations",
        (
            node(),
            bundle,
            json.dumps(
                {
                    "scene": scene,
                    "renderer": args.renderer,
                    "themes": args.themes.split(",") if args.themes else None,
                    "projections": args.projections.split(",")
                    if args.projections
                    else None,
                    "source": source_identity(ROOT),
                    "metal": f"target/{metal}",
                    "gles": f"target/performance-build/{gles}",
                    "eglDirectory": args.egl_dir,
                    "repetitions": args.repetitions,
                    "frames": args.frames,
                    "cases": args.cases.split(",")
                    if (gallery or gui) and args.cases
                    else None,
                    "instrumented": args.instrumented,
                    "minimumSampleMs": (2000 if gui else 0)
                    if args.instrumented
                    else 3000,
                    "output": args.output or f"target/performance/{scene}",
                }
            ),
        ),
        (build,),
        ("node", "npm", "gles") if include_gles else ("node", "npm"),
        timeout=2400 if gallery or gui else 300,
    )
    return [run]

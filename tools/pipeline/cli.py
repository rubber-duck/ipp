"""A consistent CLI over the shared catalog, planner, prerequisites and executor."""

import argparse
from dataclasses import replace
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading

from .artifacts import source_identity
from .catalog import (
    PROFILES,
    REGRESSION_GROUPS,
    SUITES,
    catalog,
    operation,
    regression_group_ids,
    regression_ids,
    suite_ids,
)
from .environment import (
    HARDWARE_DEVICES,
    Requirement,
    device_environment,
    inspect,
    records,
    select_browser_device,
)
from .model import ROOT, Plan, Task, select
from .processes import node
from .runner import retry_ids, run_plan
from .selection import affected, changed_files


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        description="IPP builds, checks and development environments through one Python pipeline."
    )
    commands = result.add_subparsers(dest="command", required=True)

    def common(command: argparse.ArgumentParser) -> None:
        command.add_argument(
            "--plan", action="store_true", help="show prerequisites without executing"
        )
        command.add_argument(
            "--json",
            action="store_true",
            help="emit structured output; progress goes to stderr",
        )
        command.add_argument(
            "--list", action="store_true", help="list available selections"
        )
        command.add_argument(
            "--egl-dir",
            default=os.environ.get("IPP_EGL_LIBRARY_DIR"),
            help="directory containing native EGL/GLES libraries",
        )
        command.add_argument(
            "--live",
            action="store_true",
            help="stream child output while retaining complete logs",
        )
        command.add_argument(
            "--fail-fast", action="store_true", help="stop after the first failed step"
        )

    def hardware(command: argparse.ArgumentParser) -> None:
        # Without either option the run detects hardware and falls back to software.
        devices = command.add_mutually_exclusive_group()
        devices.add_argument(
            "--hardware",
            dest="device",
            choices=HARDWARE_DEVICES,
            help="run browser steps on this hardware ANGLE backend and fail on a software renderer "
            "(default: the first available of vulkan and gl-egl, otherwise software)",
        )
        devices.add_argument(
            "--software",
            dest="device",
            action="store_const",
            const="software",
            help="run browser steps on software rendering (SwiftShader)",
        )

    for name in ("build", "test", "check"):
        command = commands.add_parser(name)
        command.add_argument("names", nargs="*")
        common(command)
        if name != "build":
            hardware(command)
        if name == "check":
            command.add_argument("--changed", action="store_true")
            command.add_argument(
                "--base", help="compare changes against the merge base with this ref"
            )
            command.add_argument("--suite", action="append", default=[])
    regression = commands.add_parser(
        "regression",
        help="run core regression, add on-demand groups, or select focused steps",
    )
    common(regression)
    hardware(regression)
    regression.add_argument(
        "--full",
        action="store_true",
        help="run every maintained check and suite, excluding benchmarks (default: core)",
    )
    regression.add_argument(
        "--group",
        choices=REGRESSION_GROUPS,
        action="append",
        default=[],
        help="add to core; with --retry, add only the group; repeat to combine",
    )
    regression.add_argument(
        "--only",
        action="append",
        default=[],
        help="select a task ID; repeat to combine",
    )
    regression.add_argument(
        "--suite",
        action="append",
        default=[],
        help="select a named suite; repeat to combine",
    )
    regression.add_argument("--retry", type=Path)

    retry = commands.add_parser(
        "retry", help="rerun unfinished steps with current prerequisites"
    )
    common(retry)
    hardware(retry)
    retry.add_argument("report", type=Path)
    retry.add_argument("--only", action="append", default=[])
    retry.add_argument("--suite", action="append", default=[])
    retry.add_argument(
        "--group",
        choices=REGRESSION_GROUPS,
        action="append",
        default=[],
        help="add affected groups to unfinished steps without adding core",
    )

    doctor = commands.add_parser("doctor", help="read-only prerequisite diagnosis")
    common(doctor)
    doctor.add_argument("--for", dest="suites", nargs="*", default=[])
    doctor.add_argument("--build", action="append", default=[])
    doctor.add_argument("--coordination", action="store_true")

    setup = commands.add_parser(
        "setup", help="explicitly install a selected development environment"
    )
    setup.add_argument(
        "name", choices=("node", "python", "rust", "browser", "blender", "certificates")
    )
    common(setup)
    setup.add_argument("--with-deps", action="store_true")
    setup.add_argument("--install-trust", action="store_true")
    setup.add_argument("--directory")

    formatting = commands.add_parser("format")
    common(formatting)
    formatting.add_argument("paths", nargs="*")
    formatting.add_argument("--check", action="store_true")
    formatting.add_argument(
        "--language",
        choices=("js", "python", "rust", "md"),
        action="append",
        default=[],
    )

    dev = commands.add_parser(
        "dev", help="build and launch an owned development environment"
    )
    common(dev)
    dev.add_argument(
        "name", choices=("gallery", "blender-viewer", "headless-client", "blender")
    )
    dev.add_argument("url", nargs="?")
    dev.add_argument("--build", action="store_true", help="prepare without launching")
    dev.add_argument("--port", type=int)
    dev.add_argument(
        "--args",
        nargs=argparse.REMAINDER,
        default=[],
        help="arguments for the Blender environment",
    )
    importing = commands.add_parser(
        "import-blender",
        help="import a disk export through its matching real browser runtime",
    )
    common(importing)
    importing.add_argument("input")
    importing.add_argument("output")
    importing.add_argument("--namespace", default="scene")
    importing.add_argument("--world", default="world.ipp")
    importing.add_argument("--clips-only", action="store_true")
    benchmarking = commands.add_parser(
        "benchmark", help="run opt-in scene performance experiments outside regression"
    )
    common(benchmarking)
    # Performance claims need real hardware, so benchmarks never fall back to software.
    benchmarking.add_argument(
        "--hardware",
        dest="device",
        choices=HARDWARE_DEVICES,
        default=HARDWARE_DEVICES[0],
        help="run browser steps on this hardware ANGLE backend (default: %(default)s)",
    )
    benchmarking.add_argument("backend", choices=("native", "browser"))
    # Stress defaults are applied by the planner so other scenes can reject them.
    benchmarking.add_argument("--preset", choices=("smoke", "full"))
    benchmarking.add_argument(
        "--scene", choices=("stress", "retained-gui", "gui-stress"), default="stress"
    )
    benchmarking.add_argument(
        "--frames",
        type=int,
        default=60,
        help="stress frames, or streaming updates for --scene retained-gui",
    )
    benchmarking.add_argument("--group", type=int)
    benchmarking.add_argument(
        "--repetitions",
        type=int,
        default=3,
        help="complete deterministic sweeps per --scene gui-stress",
    )
    benchmarking.add_argument(
        "--samples",
        type=int,
        default=6,
        help="timed action cycles per sweep and repetition for --scene gui-stress",
    )
    benchmarking.add_argument("--output")
    benchmarking.add_argument("--scene-dir")
    benchmarking.add_argument("--bundle-dir")
    benchmarking.add_argument("--geometry-index", choices=("flat", "bvh"))
    for flag in (
        "instrumented",
        "build-only",
        "reuse-build",
        "reuse-scene",
        "reuse-import",
        "allow-software",
        "skip-moving",
        "culling-views",
        "draw-sweep",
        "render-profile",
        "compare-culling",
    ):
        benchmarking.add_argument(f"--{flag}", action="store_true")
    benchmarking.add_argument(
        "--surface-cache",
        action="store_true",
        help="opt the retained-gui terminal panels into whole-Surface caching",
    )
    measuring = commands.add_parser(
        "measure", help="record explicit artifact sizes and hashes"
    )
    common(measuring)
    measuring.add_argument("paths", nargs="+")
    return result


def make_plan(args: argparse.Namespace) -> Plan:
    tasks = catalog(args.egl_dir)
    notes: list[str] = []
    coverage = "focused"
    requested: list[str] = []
    if args.command in ("build", "test", "check"):
        if args.command == "build":
            requested = [f"build:{name}" for name in args.names]
        elif args.command == "test":
            if not args.names:
                raise ValueError(
                    "Name focused suites, e.g. test cameras; use regression for core checks or regression --full for a full run."
                )
            requested = suite_ids(args.names)
        else:
            aliases = {
                "format": [
                    f"check:format-{name}" for name in ("js", "python", "rust", "md")
                ]
            }
            requested = [
                id_
                for name in args.names
                for id_ in aliases.get(name, [f"check:{name}"])
            ]
            if args.changed:
                selected, notes = affected(changed_files(args.base), args.suite)
                requested.extend(selected)
            elif not args.names:
                requested = ["check:repository", "check:catalog", "check:diff"]
            requested.extend(suite_ids(args.suite))
    elif args.command in ("regression", "retry"):
        requested = [*args.only, *suite_ids(args.suite)]
        retry = args.report if args.command == "retry" else args.retry
        full = args.command == "regression" and args.full
        if retry and full:
            raise ValueError(
                "A retry cannot select --full; add affected --group/--only/--suite selections."
            )
        if retry:
            unfinished, previous = retry_ids(retry.resolve())
            requested.extend(unfinished)
            if not args.egl_dir:
                args.egl_dir = previous.get("eglDirectory")
                tasks = catalog(args.egl_dir)
            # A retry completes the earlier run on its device instead of detecting
            # again, unless --hardware or --software overrides it.
            device = (previous.get("browser") or {}).get("device")
            if args.device is None and device:
                args.device = device
                args.browser_choice = "inherited"
            if previous.get("source") != source_identity(ROOT):
                notes.append(
                    "Source differs from the previous run. Old passing steps are not revalidated; add affected --group/--only/--suite selections."
                )
        # --only/--suite alone and retries stay focused. Groups add to core.
        coverage = "partial-regression"
        if full:
            requested = [*regression_ids(tasks, full=True), *requested]
            coverage = "full"
        elif not retry and (args.group or not (args.only or args.suite)):
            requested = [*regression_ids(tasks), *requested]
            coverage = "core"
            if args.only or args.suite:
                coverage = "partial-regression"
            elif args.group:
                coverage += "+" + "+".join(dict.fromkeys(args.group))
            notes.append(
                "Core covers the routine gate; add affected --group selections. Use --full for complete regression."
            )
        requested.extend(regression_group_ids(args.group))
        for name in dict.fromkeys(args.group):
            notes.append(f"Group {name}: {REGRESSION_GROUPS[name].description}")
        if retry and not requested:
            notes.append("Previous report has no unfinished steps.")
            return Plan(args.command, (), (), coverage, tuple(notes))
    elif args.command == "format":
        if args.paths and len(args.language) != 1:
            raise ValueError("Explicit paths require one --language")
        for language in args.language or ("js", "python", "rust", "md"):
            base = tasks[f"check:format-{language}"]
            id_ = f"format:{language}"
            tasks[id_] = replace(
                base,
                id=id_,
                command=operation(
                    "format", language, "check" if args.check else "write", *args.paths
                ),
                mutates_source=not args.check,
            )
            requested.append(id_)
    elif args.command == "setup":
        flags = [
            *(["--with-deps"] if args.with_deps else []),
            *(["--install-trust"] if args.install_trust else []),
            *(["--directory", args.directory] if args.directory else []),
        ]
        if (
            args.with_deps
            and args.name != "browser"
            or args.install_trust
            and args.name != "certificates"
            or args.directory
            and args.name != "certificates"
        ):
            raise ValueError(
                "Setup options must match their browser or certificates profile"
            )
        id_ = f"setup:{args.name}"
        requirements = (
            ("node", "npm")
            if args.name == "browser"
            else ("node",)
            if args.name == "node"
            else ()
        )
        tasks[id_] = Task(
            id_,
            f"Install {args.name}",
            operation("setup", args.name, *flags),
            requirements=requirements,
        )
        requested = [id_]
    elif args.command == "dev":
        dependencies = {
            "gallery": ("build:gallery",),
            "blender-viewer": ("build:blender-viewer",),
            "headless-client": ("build:headless-client",),
            "blender": ("build:blender-addon",),
        }[args.name]
        if args.build:
            requested = list(dependencies)
        else:
            if args.name == "gallery":
                command = operation(
                    "serve",
                    "gallery",
                    *([str(args.port)] if args.port is not None else []),
                )
            elif args.name == "blender-viewer":
                command = operation(
                    "serve",
                    "blender-viewer",
                    *([str(args.port)] if args.port is not None else []),
                )
            elif args.name == "headless-client":
                command = (
                    node(),
                    "target/headless-client/main.js",
                    *([args.url] if args.url else []),
                )
            else:
                command = (sys.executable, "tools/blender.py", "serve", *args.args)
            if (
                args.url
                and args.name != "headless-client"
                or args.args
                and args.name != "blender"
            ):
                raise ValueError(
                    "Only headless-client accepts a URL; only Blender accepts --args"
                )
            id_ = f"dev:{args.name}"
            tasks[id_] = Task(
                id_,
                f"Launch {args.name}",
                command,
                dependencies,
                ("blender",) if args.name == "blender" else ("node",),
                interactive=args.name != "headless-client",
            )
            requested = [id_]
    elif args.command == "import-blender":
        id_ = "import:blender"
        tasks[id_] = Task(
            id_,
            "Import Blender disk export",
            (
                node(),
                "tools/import_blender_scene.mjs",
                args.input,
                args.output,
                "--namespace",
                args.namespace,
                "--world",
                args.world,
                *(["--clips-only"] if args.clips_only else []),
            ),
            ("build:browser:render",),
            ("node", "npm", "browser"),
        )
        requested = [id_]
    elif args.command == "benchmark":
        from .benchmark import plan

        requested = plan(args, tasks)
    elif args.command == "measure":
        id_ = "measure:artifacts"
        tasks[id_] = Task(
            id_,
            "Measure explicit artifacts",
            (sys.executable, "tools/measure_artifacts.py", *args.paths),
        )
        requested = [id_]
    if not requested:
        raise ValueError(f"Select a {args.command} target. Use --list.")
    return Plan(
        args.command,
        tuple(dict.fromkeys(requested)),
        select(tasks, requested),
        coverage,
        tuple(notes),
    )


def list_selections(args: argparse.Namespace) -> dict:
    tasks = catalog(args.egl_dir)
    if args.command in ("test", "doctor"):
        return {name: suite["description"] for name, suite in SUITES.items()}
    if args.command == "build":
        return {
            name.removeprefix("build:"): task.description
            for name, task in tasks.items()
            if name.startswith("build:")
        }
    if args.command in ("regression", "retry", "check"):
        return {
            "default": "core",
            "--full": "Every maintained check and suite, excluding benchmarks",
            "groups": {
                name: group.description for name, group in REGRESSION_GROUPS.items()
            },
            "steps": list(tasks),
        }
    if args.command == "benchmark":
        return {
            "native": "Release GLES timing and optional allocation instrumentation",
            "browser": "Instrumented worker/WASM/WebGL scene profile",
        }
    raise ValueError(f"Use {args.command} --help for available options")


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv == ["_child"]:
        # Windows task commands cannot spawn until their launcher joins its job.
        return subprocess.run(
            json.load(sys.stdin), stdin=subprocess.DEVNULL, check=False
        ).returncode
    if argv and argv[0] == "_operation":
        from .operations import main as operation_main

        try:
            operation_main(argv[1:])
            return 0
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            print(f"IPP operation failed: {error}", file=sys.stderr)
            return 1
    args = parser().parse_args(argv)
    cancel = threading.Event()
    interrupted: list[int] = []

    def stop(signum: int, _frame: object) -> None:
        interrupted.append(signum)
        cancel.set()

    previous = {s: signal.signal(s, stop) for s in (signal.SIGINT, signal.SIGTERM)}
    try:
        if args.list:
            value = list_selections(args)
            print(
                json.dumps(value, indent=2)
                if args.json
                else "\n".join(
                    f"{name}: {description}" for name, description in value.items()
                )
            )
            return 0
        if args.command == "doctor":
            tasks = catalog(args.egl_dir)
            ids = [*suite_ids(args.suites), *(f"build:{name}" for name in args.build)]
            requirements = (
                {r for task in select(tasks, ids) for r in task.requirements}
                if ids
                else {"node", "npm", "rust", "python-tools", "git"}
            )
            if args.coordination:
                requirements.add("coordination")
            if args.plan:
                print(
                    json.dumps(
                        {"requirements": sorted(requirements | {"python"})}, indent=2
                    )
                )
                return 0
            # Report the device a default run would select, and check on it.
            egl = {"IPP_EGL_LIBRARY_DIR": args.egl_dir} if args.egl_dir else {}
            selected = select_browser_device(cancel, egl)
            environment = inspect(
                requirements - {"browser"} if selected.verified else requirements,
                args.egl_dir,
                cancel,
                {**device_environment(selected.device), **egl},
            )
            failures = "; ".join(
                f"{attempt['device']}: {attempt['detail']}"
                for attempt in selected.attempts
                if not attempt["ready"]
            )
            environment = sorted(
                [
                    *environment,
                    *(
                        [selected.verified]
                        if selected.verified and "browser" in requirements
                        else []
                    ),
                    Requirement(
                        "browser-device",
                        True,
                        f"{selected.device} ({selected.reason})"
                        + (f"; {failures}" if failures else ""),
                    ),
                ],
                key=lambda result: result.name,
            )
            if args.json:
                print(json.dumps(records(environment), indent=2))
            else:
                for result in environment:
                    print(
                        f"{'ready' if result.ready else 'missing'} {result.name}: {result.detail}"
                    )
                    if result.remedy:
                        print(f"  {result.remedy}")
            return (
                128 + interrupted[0]
                if interrupted
                else 0
                if all(result.ready for result in environment)
                else 1
            )
        plan = make_plan(args)
        if args.plan:
            if args.json:
                print(json.dumps(plan.data(), indent=2))
            else:
                print(
                    f"{plan.command}: {len(plan.tasks)} steps; coverage: {plan.coverage}"
                )
                for task in plan.tasks:
                    print(
                        f"{task.id}\n  requires: {', '.join(task.dependencies) or 'none'}\n  environment: {', '.join(task.requirements) or 'Python only'}"
                    )
                for note in plan.notes:
                    print(note)
            return 0
        report = run_plan(
            plan,
            egl_directory=args.egl_dir,
            fail_fast=args.fail_fast,
            live=args.live,
            cancel=cancel,
            browser_device=getattr(args, "device", None),
            browser_choice=getattr(args, "browser_choice", "requested"),
        )
        if args.json:
            print(json.dumps(report, indent=2))
        return (
            128 + interrupted[0]
            if interrupted
            else 0
            if report["status"] == "passed"
            else 1
        )
    except (OSError, ValueError, KeyError) as error:
        if args.json:
            print(json.dumps({"status": "invalid", "error": str(error)}))
        else:
            print(f"IPP: {error}", file=sys.stderr)
        return 2
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)

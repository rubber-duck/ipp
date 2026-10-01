"""Shared process supervision and durable evidence for every public command."""

from dataclasses import asdict
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import queue
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time

from .artifacts import output_records, source_identity, workspace_lock, write_json
from .environment import inspect, records
from .model import ROOT, Plan, Task
from .processes import blender, node
from .windows_job import WindowsJob


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def terminate_tree(child: subprocess.Popen, *, hard: bool = False) -> None:
    if sys.platform == "win32":
        subprocess.run(
            ["taskkill", "/pid", str(child.pid), "/t", *(["/f"] if hard else [])],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    else:
        try:
            os.killpg(child.pid, signal.SIGKILL if hard else signal.SIGTERM)
        except ProcessLookupError:
            pass


BROWSER_DEVICES = ("software", "vulkan", "gl-egl")


def device_environment(device: str) -> dict[str, str | None]:
    """The pipeline, not the caller's shell, selects every child's browser device."""
    if device not in BROWSER_DEVICES:
        raise ValueError(f"Unknown browser device: {device}")
    return {"IPP_BROWSER_ANGLE": None if device == "software" else device}


def child_environment(overrides: dict[str, str | None] | None = None) -> dict:
    environment = dict(os.environ)
    for name, value in (overrides or {}).items():
        if value is None:
            environment.pop(name, None)
        else:
            environment[name] = value
    return environment


def scheduler_widths(serial: bool = False) -> dict:
    """Derive concurrency from the cores this process may use; never a fixed count."""
    cores = os.process_cpu_count() or 1
    steps = 1 if serial else max(2, cores // 2)
    return {
        "availableCores": cores,
        "logicalCores": os.cpu_count(),
        "serial": serial,
        "steps": steps,
        # Bounds the browser steps running at once, and so the separate
        # Chromium instances they launch.
        "browser": 1 if serial else max(1, steps // 2),
        # Cargo steps share one build directory and already use every core.
        "cargo": 1,
    }


def execute(
    task: Task,
    log: Path,
    cancel: threading.Event,
    *,
    root: Path = ROOT,
    live: bool = False,
    environment: dict[str, str | None] | None = None,
) -> dict:
    """Retain complete output, bounded timeout, and cleanup on every exit path."""
    started = time.monotonic()
    with log.open("xb") as output:
        variables = {
            **child_environment(environment),
            "BLENDER_BIN": blender(),
            "NODE_BIN": node(),
            "PYTHON_BIN": sys.executable,
            # Evidence written by a step can name the run that produced it.
            "IPP_PIPELINE_RUN": str(log.parent),
        }
        windows = sys.platform == "win32"
        command = (
            (sys.executable, str(ROOT / "tools/ipp.py"), "_child")
            if windows
            else task.command
        )
        child = subprocess.Popen(
            command,
            cwd=root,
            env=variables,
            stdin=subprocess.PIPE if windows else subprocess.DEVNULL,
            stdout=subprocess.PIPE if live else output,
            stderr=subprocess.STDOUT,
            start_new_session=os.name != "nt",
            creationflags=0x00000200 if os.name == "nt" else 0,
        )
        job = None
        if windows:
            try:
                job = WindowsJob(int(getattr(child, "_handle")))
                assert child.stdin is not None
                child.stdin.write(json.dumps(task.command).encode())
                child.stdin.close()
            except BaseException:
                if job:
                    job.close()
                child.kill()
                child.wait(timeout=10)
                raise
        reader = None
        if live:

            def relay() -> None:
                assert child.stdout is not None
                for line in iter(child.stdout.readline, b""):
                    output.write(line)
                    output.flush()
                    sys.stderr.write(line.decode("utf-8", errors="replace"))
                    sys.stderr.flush()

            reader = threading.Thread(target=relay, daemon=True)
            reader.start()
        timed_out = False
        try:
            while child.poll() is None:
                if cancel.wait(0.1) or (
                    not task.interactive and time.monotonic() - started >= task.timeout
                ):
                    timed_out = not cancel.is_set()
                    terminate_tree(child)
                    try:
                        child.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        terminate_tree(child, hard=True)
                        child.wait(timeout=10)
                    break
        finally:
            terminate_tree(child, hard=True)
            if job:
                job.close()
            if child.poll() is None:
                child.wait(timeout=10)
            if reader:
                reader.join(timeout=5)
            if child.stdout:
                child.stdout.close()
        return {
            "exitCode": child.returncode,
            "timedOut": timed_out,
            "durationMs": round((time.monotonic() - started) * 1000),
        }


def run_plan(
    plan: Plan,
    *,
    root: Path = ROOT,
    egl_directory: str | None = None,
    fail_fast: bool = False,
    live: bool = False,
    cancel: threading.Event | None = None,
    preflight: bool = True,
    browser_device: str = "software",
) -> dict:
    cancel = cancel or threading.Event()
    # CLI selections reach every child as the environment its harness reads.
    selection = {
        **device_environment(browser_device),
        **({"IPP_EGL_LIBRARY_DIR": egl_directory} if egl_directory else {}),
    }
    # Streamed or interactive output stays readable only one step at a time.
    widths = scheduler_widths(live or any(task.interactive for task in plan.tasks))
    with workspace_lock(root):
        runs = root / "target/pipeline/runs"
        runs.mkdir(parents=True, exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix="run-", dir=runs))
        report: dict = {
            "version": 2,
            "root": str(root),
            "directory": str(directory),
            "command": plan.command,
            "requested": list(plan.requested),
            "coverage": plan.coverage,
            "notes": list(plan.notes),
            "eglDirectory": egl_directory,
            "startedAt": now(),
            "status": "running",
            "source": source_identity(root),
            "platform": {
                "system": platform.system(),
                "release": platform.release(),
                "machine": platform.machine(),
            },
            "environmentSelection": {
                **{
                    name: os.environ[name]
                    for name in (
                        "NODE_BIN",
                        "BLENDER_BIN",
                        "LIBGL_ALWAYS_SOFTWARE",
                        "LD_LIBRARY_PATH",
                        "FONTCONFIG_FILE",
                        "RUSTFLAGS",
                    )
                    if name in os.environ
                },
                **({"IPP_EGL_LIBRARY_DIR": egl_directory} if egl_directory else {}),
                "browserDevice": browser_device,
            },
            "scheduler": widths,
            "browser": {"device": browser_device},
            "environment": [],
            "steps": [{**asdict(task), "status": "pending"} for task in plan.tasks],
        }
        save = lambda: write_json(directory / "summary.json", report)
        save()
        print(
            f"IPP {plan.command}: {len(plan.tasks)} steps; browser device: {browser_device}; "
            f"width {widths['steps']} (browser {widths['browser']}, cargo {widths['cargo']}) "
            f"on {widths['availableCores']} cores; evidence: {directory}",
            file=sys.stderr,
            flush=True,
        )
        if preflight:
            environment = inspect(
                {r for task in plan.tasks for r in task.requirements},
                egl_directory,
                cancel,
                selection,
            )
            report["environment"] = records(environment)
            failed = [result for result in environment if not result.ready]
            if failed:
                for result in failed:
                    print(
                        f"[missing] {result.name}: {result.detail}\n  {result.remedy}",
                        file=sys.stderr,
                    )
                report["status"] = (
                    "cancelled" if cancel.is_set() else "environment_failed"
                )
                for result in report["steps"]:
                    result["status"] = "cancelled" if cancel.is_set() else "not_run"
                report["finishedAt"] = now()
                save()
                return report

        results = {task.id: result for task, result in zip(plan.tasks, report["steps"])}
        pending = list(range(len(plan.tasks)))
        running: dict[int, float] = {}
        finished: queue.Queue[tuple[int, dict]] = queue.Queue()
        stopped = False

        def step(index: int, task: Task, log: Path, inputs: dict) -> None:
            try:
                update = execute(
                    task,
                    log,
                    cancel,
                    root=root,
                    live=live or task.interactive,
                    environment=selection,
                )
                update["status"] = (
                    "cancelled"
                    if cancel.is_set()
                    else "passed"
                    if update["exitCode"] == 0 and not update["timedOut"]
                    else "failed"
                )
                if update["status"] == "passed" and task.outputs:
                    manifest = {
                        "version": 1,
                        "task": task.id,
                        "source": report["source"],
                        "command": list(task.command),
                        "environment": report["environment"],
                        "platform": report["platform"],
                        "environmentSelection": report["environmentSelection"],
                        "dependencies": inputs,
                        "artifacts": output_records(root, task.outputs),
                    }
                    path = log.with_name(
                        log.name.removesuffix(".log") + ".manifest.json"
                    )
                    write_json(path, manifest)
                    update["manifest"] = str(path)
            except Exception as error:
                # Every failure must reach the scheduler, which is waiting for this step.
                update = {"status": "failed", "error": str(error)}
            finished.put((index, update))

        def start(index: int) -> None:
            task, result = plan.tasks[index], report["steps"][index]
            result.update(status="running", startedAt=now())
            name = re.sub(r"[^a-zA-Z0-9._-]", "-", task.id)
            log = directory / f"{index + 1:03d}-{name}.log"
            result["log"] = str(log)
            print(
                f"[{index + 1}/{len(plan.tasks)}] {task.id}",
                file=sys.stderr,
                flush=True,
            )
            inputs = {id_: results[id_].get("manifest") for id_ in task.dependencies}
            running[index] = time.monotonic()
            threading.Thread(
                target=step, args=(index, task, log, inputs), daemon=True
            ).start()

        def available(task: Task) -> bool:
            active = [plan.tasks[index] for index in running]
            if len(active) >= widths["steps"]:
                return False
            if "rust" in task.requirements and any(
                "rust" in t.requirements for t in active
            ):
                return False
            return (
                "browser" not in task.requirements
                or sum("browser" in t.requirements for t in active) < widths["browser"]
            )

        def schedule() -> bool:
            progressed = False
            for index in list(pending):
                task, result = plan.tasks[index], report["steps"][index]
                states = [results.get(d, {}).get("status") for d in task.dependencies]
                if any(state in ("pending", "running") for state in states):
                    continue
                blocked = [
                    d
                    for d in task.dependencies
                    if results.get(d, {}).get("status") != "passed"
                ]
                if cancel.is_set():
                    result["status"] = "cancelled"
                elif blocked:
                    result.update(status="blocked", blockedBy=blocked)
                elif stopped:
                    result["status"] = "not_run"
                elif available(task):
                    start(index)
                else:
                    continue
                pending.remove(index)
                progressed = True
            return progressed

        def complete(index: int, update: dict) -> None:
            nonlocal stopped
            task, result = plan.tasks[index], report["steps"][index]
            started = running.pop(index)
            result.update(update)
            result["finishedAt"] = now()
            print(
                f"[{result['status']}] {task.id} ({time.monotonic() - started:.0f}s)",
                file=sys.stderr,
                flush=True,
            )
            if result["status"] == "failed":
                stopped = stopped or fail_fast
                if "error" in result:
                    print(result["error"], file=sys.stderr)
                log = Path(result["log"])
                if log.is_file():
                    with log.open("rb") as stream:
                        stream.seek(max(0, log.stat().st_size - 16384))
                        tail = stream.read().decode("utf-8", errors="replace")
                    print("\n".join(tail.splitlines()[-25:]), file=sys.stderr)

        heartbeat = time.monotonic() + 30
        try:
            while pending or running:
                if schedule():
                    save()
                elif not running:
                    # Unreachable for dependency-ordered plans; never wait forever.
                    break
                try:
                    complete(*finished.get(timeout=0.1))
                    save()
                except queue.Empty:
                    pass
                if running and time.monotonic() >= heartbeat:
                    clock = time.monotonic()
                    print(
                        f"[running {len(running)}] "
                        + ", ".join(
                            f"{plan.tasks[index].id} ({clock - started:.0f}s)"
                            for index, started in sorted(running.items())
                        ),
                        file=sys.stderr,
                        flush=True,
                    )
                    heartbeat = clock + 30
        finally:
            if running:
                # An unexpected scheduler error must still stop every child tree.
                cancel.set()
                while running:
                    complete(*finished.get())
        report["sourceAtFinish"] = source_identity(root)
        report["sourceChangedDuringRun"] = report["source"] != report["sourceAtFinish"]
        report["sourceMutationExpected"] = any(
            task.mutates_source for task in plan.tasks
        )
        report["status"] = (
            "cancelled"
            if cancel.is_set()
            else "passed"
            if all(r["status"] == "passed" for r in results.values())
            else "failed"
        )
        report["finishedAt"] = now()
        save()
        print(
            f"IPP {plan.command} {report['status']}; coverage: {plan.coverage}",
            file=sys.stderr,
        )
        if report["sourceChangedDuringRun"] and not report["sourceMutationExpected"]:
            print(
                "Source changed during execution; this run is not evidence for one unchanged checkout.",
                file=sys.stderr,
            )
        print(f"Report: {directory / 'summary.json'}", file=sys.stderr)
        if report["status"] != "passed":
            print(
                f"Retry: python tools/ipp.py retry {directory / 'summary.json'}",
                file=sys.stderr,
            )
        return report


def retry_ids(path: Path, root: Path = ROOT) -> tuple[list[str], dict]:
    report = json.loads(path.read_text())
    if report.get("version") != 2 or report.get("root") != str(root):
        raise ValueError("Retry requires a pipeline report from this checkout")
    ids = [step["id"] for step in report["steps"] if step["status"] != "passed"]
    return ids, report

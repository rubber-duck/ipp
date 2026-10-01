"""Concurrent step scheduling and browser device ownership."""

from contextlib import redirect_stderr
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.cli import main, parser
from pipeline.environment import probe
from pipeline.model import ROOT, Plan, Task
from pipeline.runner import (
    device_environment,
    retry_ids,
    run_plan,
    scheduler_widths,
)

# Records the wall-clock interval of a step for overlap assertions.
TIMED = "import pathlib,time; s=time.time(); time.sleep({seconds}); pathlib.Path('times').mkdir(exist_ok=True); pathlib.Path('times/{id}').write_text(f'{{s}} {{time.time()}}')"


class Fixture(unittest.TestCase):
    """A temporary checkout with helpers; holds no tests of its own."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ipp-scheduler-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        (self.root / ".gitignore").write_text("target/\n")
        subprocess.run(["git", "add", ".gitignore"], cwd=self.root, check=True)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Pipeline test",
                "-c",
                "user.email=pipeline@example.invalid",
                "commit",
                "-qm",
                "Test fixture",
            ],
            cwd=self.root,
            check=True,
        )

    def command(self, id_, source, dependencies=(), **options):
        return Task(id_, id_, (sys.executable, "-c", source), dependencies, **options)

    def timed(self, id_, seconds, dependencies=(), **options):
        source = TIMED.format(seconds=seconds, id=id_)
        return self.command(id_, source, dependencies, **options)

    def interval(self, id_):
        start, end = (self.root / "times" / id_).read_text().split()
        return float(start), float(end)

    def overlap(self, first, second):
        a, b = self.interval(first), self.interval(second)
        return a[0] < b[1] and b[0] < a[1]

    def run_tasks(self, tasks, **options):
        selected = Plan("test", tuple(task.id for task in tasks), tuple(tasks))
        with redirect_stderr(io.StringIO()):
            return run_plan(selected, root=self.root, preflight=False, **options)

    def device_task(self, id_="device", requirements=()):
        source = f"import os,pathlib; pathlib.Path('{id_}').write_text(repr(os.environ.get('IPP_BROWSER_ANGLE')))"
        return self.command(id_, source, requirements=requirements)

    def seen(self, id_="device"):
        return eval((self.root / id_).read_text())


class SchedulerTests(Fixture):
    def test_widths_come_from_the_machine_and_are_recorded(self):
        report = self.run_tasks([self.command("ok", "pass")])
        cores = os.process_cpu_count() or 1
        self.assertEqual(report["scheduler"]["availableCores"], cores)
        self.assertEqual(report["scheduler"]["steps"], max(2, cores // 2))
        self.assertGreaterEqual(report["scheduler"]["browser"], 1)
        self.assertEqual(report["scheduler"]["cargo"], 1)
        saved = json.loads((Path(report["directory"]) / "summary.json").read_text())
        self.assertEqual(saved["scheduler"], report["scheduler"])
        self.assertEqual(saved["version"], 2)

    def test_live_runs_are_serial(self):
        report = self.run_tasks(
            [self.timed("one", 0.3), self.timed("two", 0.3)], live=True
        )
        self.assertTrue(report["scheduler"]["serial"])
        self.assertFalse(self.overlap("one", "two"))

    def test_independent_steps_overlap_and_dependents_wait(self):
        tasks = [
            self.timed("slow", 1.0),
            self.timed("quick", 0.6),
            self.timed("after-slow", 0.1, ("slow",)),
            self.timed("after-both", 0.1, ("slow", "quick")),
        ]
        report = self.run_tasks(tasks)
        self.assertEqual(report["status"], "passed")
        self.assertTrue(self.overlap("slow", "quick"))
        for dependent, dependencies in (
            ("after-slow", ("slow",)),
            ("after-both", ("slow", "quick")),
        ):
            for dependency in dependencies:
                self.assertGreaterEqual(
                    self.interval(dependent)[0], self.interval(dependency)[1]
                )
        # Report order and log names follow the plan, not completion order.
        self.assertEqual(
            [step["id"] for step in report["steps"]], [t.id for t in tasks]
        )
        self.assertEqual(
            [Path(step["log"]).name[:3] for step in report["steps"]],
            ["001", "002", "003", "004"],
        )

    def test_cargo_steps_never_overlap(self):
        report = self.run_tasks(
            [
                self.timed("cargo-a", 0.5, requirements=("rust",)),
                self.timed("cargo-b", 0.5, requirements=("rust", "wasm")),
                self.timed("node", 0.8, requirements=("node",)),
            ]
        )
        self.assertEqual(report["status"], "passed")
        self.assertFalse(self.overlap("cargo-a", "cargo-b"))
        self.assertTrue(self.overlap("node", "cargo-a"))

    def test_browser_steps_respect_the_browser_width(self):
        widths = {**scheduler_widths(), "steps": 4, "browser": 1}
        with patch("pipeline.runner.scheduler_widths", return_value=widths):
            report = self.run_tasks(
                [
                    self.timed("browser-a", 0.5, requirements=("browser",)),
                    self.timed("browser-b", 0.5, requirements=("browser",)),
                    self.timed("other", 0.8),
                ]
            )
        self.assertEqual(report["status"], "passed")
        self.assertFalse(self.overlap("browser-a", "browser-b"))
        self.assertTrue(self.overlap("other", "browser-a"))

    def test_failures_block_dependents_while_independent_work_continues(self):
        tasks = [
            self.command("failure", "import sys; sys.exit(3)"),
            self.timed("independent", 0.8),
            self.command("blocked", "raise SystemExit('must not run')", ("failure",)),
            self.timed("after-independent", 0.1, ("independent",)),
        ]
        report = self.run_tasks(tasks)
        self.assertEqual(
            [step["status"] for step in report["steps"]],
            ["failed", "passed", "blocked", "passed"],
        )
        self.assertEqual(report["steps"][2]["blockedBy"], ["failure"])
        retry, _ = retry_ids(Path(report["directory"]) / "summary.json", self.root)
        self.assertEqual(retry, ["failure", "blocked"])

    def test_fail_fast_stops_new_starts_and_lets_running_steps_finish(self):
        tasks = [
            self.command("timeout", "import time; time.sleep(60)", timeout=1),
            self.timed("running", 2.0),
            self.command("blocked", "pass", ("timeout",)),
            self.command("later", "pass", ("running",)),
        ]
        report = self.run_tasks(tasks, fail_fast=True)
        statuses = [step["status"] for step in report["steps"]]
        self.assertEqual(statuses, ["failed", "passed", "blocked", "not_run"])
        self.assertTrue(report["steps"][0]["timedOut"])
        retry, _ = retry_ids(Path(report["directory"]) / "summary.json", self.root)
        self.assertEqual(retry, ["timeout", "blocked", "later"])

    def test_summary_stays_valid_json_while_steps_run(self):
        reader = "import json,os,pathlib,time; time.sleep(0.3); report=json.loads((pathlib.Path(os.environ['IPP_PIPELINE_RUN'])/'summary.json').read_text()); pathlib.Path('{id}').write_text(json.dumps([s['status'] for s in report['steps']]))"
        tasks = [
            self.command(f"reader-{index}", reader.format(id=f"reader-{index}"))
            for index in range(4)
        ]
        report = self.run_tasks(tasks)
        self.assertEqual(report["status"], "passed")
        for index in range(4):
            seen = json.loads((self.root / f"reader-{index}").read_text())
            self.assertEqual(seen[index], "running")

    @unittest.skipIf(
        os.name == "nt",
        "POSIX signal-handler fixture; Windows uses job/process-tree termination",
    )
    def test_cancellation_terminates_every_running_tree(self):
        descendant = "from pathlib import Path; import os,signal,sys,time; signal.signal(signal.SIGTERM, lambda *_: (Path(f'stopped-{sys.argv[1]}').touch(), os._exit(0))); Path(f'ready-{sys.argv[1]}').touch(); time.sleep(60)"
        parent = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{descendant!r},'NAME']); time.sleep(60)"
        cancel = threading.Event()

        def stop_when_ready():
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline and not all(
                (self.root / f"ready-{name}").exists() for name in ("a", "b")
            ):
                time.sleep(0.02)
            cancel.set()

        stopper = threading.Thread(target=stop_when_ready)
        stopper.start()
        try:
            report = self.run_tasks(
                [
                    self.command("a", parent.replace("NAME", "a")),
                    self.command("b", parent.replace("NAME", "b")),
                    self.command("later", "pass", ("a",)),
                ],
                cancel=cancel,
            )
        finally:
            cancel.set()
            stopper.join(timeout=11)
        for name in ("a", "b"):
            self.assertTrue((self.root / f"stopped-{name}").exists())
        self.assertEqual(
            [step["status"] for step in report["steps"]],
            ["cancelled", "cancelled", "cancelled"],
        )
        self.assertEqual(report["status"], "cancelled")


class BrowserDeviceTests(Fixture):
    PROBE = "import os; print(os.environ.get('IPP_BROWSER_ANGLE'))"

    def test_software_removes_the_ambient_device_and_hardware_sets_it(self):
        with patch.dict(os.environ, {"IPP_BROWSER_ANGLE": "vulkan"}):
            for device, expected in (
                ("software", None),
                ("vulkan", "vulkan"),
                ("gl-egl", "gl-egl"),
            ):
                with self.subTest(device=device):
                    report = self.run_tasks([self.device_task()], browser_device=device)
                    self.assertEqual(self.seen(), expected)
                    self.assertEqual(report["browser"]["device"], device)
                    self.assertEqual(
                        report["environmentSelection"]["browserDevice"], device
                    )
                    (self.root / "device").unlink()

    def test_selected_egl_directory_reaches_every_child(self):
        source = "import os,pathlib; pathlib.Path('egl').write_text(repr(os.environ.get('IPP_EGL_LIBRARY_DIR')))"
        for ambient, selected, expected in (
            (None, "/selected/egl", "/selected/egl"),
            ("/ambient/egl", "/selected/egl", "/selected/egl"),
            (None, None, None),
        ):
            with self.subTest(ambient=ambient, selected=selected):
                with patch.dict(os.environ):
                    os.environ.pop("IPP_EGL_LIBRARY_DIR", None)
                    if ambient:
                        os.environ["IPP_EGL_LIBRARY_DIR"] = ambient
                    with patch("pipeline.runner.inspect", return_value=[]) as inspect:
                        task = self.command("egl", source, requirements=("gles",))
                        with redirect_stderr(io.StringIO()):
                            report = run_plan(
                                Plan("test", (task.id,), (task,)),
                                root=self.root,
                                egl_directory=selected,
                            )
                self.assertEqual(eval((self.root / "egl").read_text()), expected)
                self.assertEqual(
                    report["environmentSelection"].get("IPP_EGL_LIBRARY_DIR"), expected
                )
                # The preflight probes the same selection.
                self.assertEqual(
                    inspect.call_args.args[3].get("IPP_EGL_LIBRARY_DIR"), expected
                )

    def test_retry_passes_the_recorded_egl_directory_to_the_runner(self):
        report = self.root / "summary.json"
        report.write_text(
            json.dumps(
                {
                    "version": 2,
                    "root": str(ROOT),
                    "eglDirectory": "/recorded/egl",
                    "steps": [{"id": "check:catalog", "status": "failed"}],
                }
            )
        )
        for extra, expected in (([], "/recorded/egl"), (["--egl-dir", "/new"], "/new")):
            with self.subTest(extra=extra):
                with patch.dict(os.environ):
                    os.environ.pop("IPP_EGL_LIBRARY_DIR", None)
                    with (
                        patch(
                            "pipeline.cli.run_plan", return_value={"status": "passed"}
                        ) as runner,
                        redirect_stderr(io.StringIO()),
                    ):
                        self.assertEqual(main(["retry", str(report), *extra]), 0)
                self.assertEqual(runner.call_args.kwargs["egl_directory"], expected)

    def test_manifests_record_the_device(self):
        build = self.command(
            "build",
            "from pathlib import Path; Path('target/out').mkdir(parents=True); Path('target/out/file').touch()",
            outputs=("target/out",),
        )
        report = self.run_tasks([build], browser_device="gl-egl")
        manifest = json.loads(Path(report["steps"][0]["manifest"]).read_text())
        self.assertEqual(manifest["environmentSelection"]["browserDevice"], "gl-egl")

    def test_preflight_probe_sees_the_selected_device(self):
        with patch.dict(os.environ, {"IPP_BROWSER_ANGLE": "vulkan"}):
            command = [sys.executable, "-c", self.PROBE]
            self.assertEqual(
                probe(command, environment=device_environment("software")),
                "None",
            )
            self.assertEqual(
                probe(command, environment=device_environment("gl-egl")),
                "gl-egl",
            )
        with patch("pipeline.runner.inspect", return_value=[]) as inspect:
            task = self.device_task(requirements=("browser",))
            selected = Plan("test", (task.id,), (task,))
            with redirect_stderr(io.StringIO()):
                run_plan(selected, root=self.root, browser_device="vulkan")
        self.assertEqual(inspect.call_args.args[3], {"IPP_BROWSER_ANGLE": "vulkan"})

    def test_retry_inherits_the_device_unless_hardware_overrides_it(self):
        report = self.root / "summary.json"
        for previous, extra, expected in (
            ("vulkan", [], "vulkan"),
            ("gl-egl", [], "gl-egl"),
            ("software", [], "software"),
            ("vulkan", ["--hardware", "gl-egl"], "gl-egl"),
            ("software", ["--hardware", "vulkan"], "vulkan"),
        ):
            report.write_text(
                json.dumps(
                    {
                        "version": 2,
                        "root": str(ROOT),
                        "browser": {"device": previous},
                        "steps": [{"id": "check:catalog", "status": "failed"}],
                    }
                )
            )
            for argv in (
                ["retry", str(report), *extra],
                ["regression", "--retry", str(report), *extra],
            ):
                with self.subTest(argv=argv, previous=previous):
                    with (
                        patch(
                            "pipeline.cli.run_plan", return_value={"status": "passed"}
                        ) as runner,
                        redirect_stderr(io.StringIO()),
                    ):
                        self.assertEqual(main(argv), 0)
                    self.assertEqual(
                        runner.call_args.kwargs["browser_device"], expected
                    )

    def test_hardware_option_reaches_the_runner(self):
        arguments = parser()
        for command in (
            ["regression"],
            ["regression", "--full"],
            ["regression", "--only", "check:catalog"],
            ["test", "cameras"],
            ["check", "repository"],
            ["retry", "summary.json"],
        ):
            with self.subTest(command=command):
                self.assertIsNone(arguments.parse_args(command).hardware)
                for device in ("vulkan", "gl-egl"):
                    parsed = arguments.parse_args([*command, "--hardware", device])
                    self.assertEqual(parsed.hardware, device)
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            arguments.parse_args(["regression", "--hardware", "software"])
        self.assertEqual(
            arguments.parse_args(["benchmark", "browser"]).hardware, "vulkan"
        )
        for argv, expected in (
            (["regression", "--only", "check:catalog"], "software"),
            (
                ["regression", "--only", "check:catalog", "--hardware", "gl-egl"],
                "gl-egl",
            ),
            (["benchmark", "browser", "--build-only"], "vulkan"),
        ):
            with self.subTest(argv=argv):
                with patch(
                    "pipeline.cli.run_plan", return_value={"status": "passed"}
                ) as runner:
                    self.assertEqual(main(argv), 0)
                self.assertEqual(runner.call_args.kwargs["browser_device"], expected)


if __name__ == "__main__":
    unittest.main()

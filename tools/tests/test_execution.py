"""Real subprocess, evidence, retry, environment probe and workspace lock lifecycle."""

from contextlib import redirect_stderr
import io
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.artifacts import source_identity, workspace_lock
from pipeline.environment import Requirement, probe
from pipeline.model import ROOT, Plan, Task
from pipeline.runner import retry_ids, run_plan


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ipp-pipeline-test-")
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

    def run_tasks(self, tasks, **options):
        selected = Plan("test", tuple(task.id for task in tasks), tuple(tasks))
        with redirect_stderr(io.StringIO()):
            return run_plan(selected, root=self.root, preflight=False, **options)

    def test_failures_block_dependents_but_retain_independent_evidence(self):
        tasks = [
            self.command(
                "build",
                "from pathlib import Path; Path('target/product').mkdir(parents=True); Path('target/product/file').write_text('built')",
                outputs=("target/product",),
            ),
            self.command(
                "failure",
                "import sys; print('useful failure'); sys.exit(7)",
                ("build",),
            ),
            self.command("blocked", "raise Exception('must not run')", ("failure",)),
            self.command("independent", "print('still ran')", ("build",)),
            Task("missing", "missing", (str(self.root / "missing"),)),
        ]
        report = self.run_tasks(tasks)
        self.assertEqual(
            [step["status"] for step in report["steps"]],
            ["passed", "failed", "blocked", "passed", "failed"],
        )
        self.assertEqual(report["steps"][1]["exitCode"], 7)
        self.assertIn("useful failure", Path(report["steps"][1]["log"]).read_text())
        manifest = json.loads(Path(report["steps"][0]["manifest"]).read_text())
        self.assertEqual(manifest["artifacts"][0]["path"], "target/product/file")
        saved = json.loads((Path(report["directory"]) / "summary.json").read_text())
        self.assertEqual(saved["status"], "failed")
        retry, _ = retry_ids(Path(report["directory"]) / "summary.json", self.root)
        self.assertEqual(retry, ["failure", "blocked", "missing"])

    def test_missing_declared_output_fails_even_when_process_succeeds(self):
        report = self.run_tasks(
            [self.command("build", "pass", outputs=("target/missing",))]
        )
        self.assertEqual(report["status"], "failed")
        self.assertIn("Declared output", report["steps"][0]["error"])

    def test_preflight_failure_runs_no_builds(self):
        task = self.command(
            "build", "from pathlib import Path; Path('unexpected').touch()"
        )
        with patch(
            "pipeline.runner.inspect",
            return_value=[Requirement("browser", False, "missing", "setup browser")],
        ):
            with redirect_stderr(io.StringIO()):
                report = run_plan(Plan("test", (task.id,), (task,)), root=self.root)
        self.assertEqual(report["status"], "environment_failed")
        self.assertFalse((self.root / "unexpected").exists())
        self.assertEqual(report["steps"][0]["status"], "not_run")

    def test_content_identity_distinguishes_two_edits_of_the_same_file(self):
        source = self.root / "source.py"
        source.write_text("first")
        first = source_identity(self.root)
        source.write_text("second")
        second = source_identity(self.root)
        self.assertEqual(first["revision"], second["revision"])
        self.assertNotEqual(first["sourceSha256"], second["sourceSha256"])

    def test_evidence_directories_are_unique(self):
        one = self.run_tasks([self.command("ok", "pass")])
        two = self.run_tasks([self.command("ok", "pass")])
        self.assertNotEqual(one["directory"], two["directory"])
        self.assertTrue((Path(one["directory"]) / "summary.json").is_file())

    def test_exited_launcher_leaves_no_listening_descendants(self):
        descendant = "from pathlib import Path; import socket,time; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); Path('port').write_text(str(s.getsockname()[1])); time.sleep(60)"
        parent = f"from pathlib import Path; import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{descendant!r}], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL);\nwhile not Path('port').exists(): time.sleep(0.02)"
        report = self.run_tasks([self.command("launcher", parent, timeout=10)])
        self.assertEqual(report["status"], "passed")
        port = int((self.root / "port").read_text())
        with socket.socket() as connection:
            connection.settimeout(1)
            self.assertNotEqual(connection.connect_ex(("127.0.0.1", port)), 0)

    def test_environment_probe_is_cancellable(self):
        ready = self.root / "probe-ready"
        command = [
            sys.executable,
            "-c",
            f"from pathlib import Path; import time; Path({str(ready)!r}).touch(); time.sleep(60)",
        ]
        cancel = threading.Event()

        def stop_when_ready():
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not ready.exists():
                time.sleep(0.02)
            cancel.set()

        stopper = threading.Thread(target=stop_when_ready)
        stopper.start()
        try:
            with self.assertRaisesRegex(ValueError, "cancelled"):
                probe(command, cancel=cancel)
        finally:
            cancel.set()
            stopper.join(timeout=6)
        self.assertTrue(ready.exists())

    def test_workspace_lock_is_released_and_prevents_other_writers(self):
        script = "import sys; sys.path.insert(0, sys.argv[1]); from pathlib import Path; from pipeline.artifacts import workspace_lock;\nwith workspace_lock(Path(sys.argv[2])): print('locked')"
        command = [sys.executable, "-c", script, str(ROOT / "tools"), str(self.root)]
        with workspace_lock(self.root):
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Another pipeline", result.stderr)
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()

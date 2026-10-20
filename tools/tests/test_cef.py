"""Real file/process cache mechanics; tiny archives do not claim CEF rendering."""

from contextlib import contextmanager, redirect_stdout
import importlib.util
import io
import json
import multiprocessing
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pipeline import cef


def hold_installation(root, pin, ready, release, start):
    start.wait()
    try:
        with cef.installation(Path(root), pin, offline=True, candidates=[]) as runtime:
            ready.send(str(runtime))
            release.recv()
    except Exception as error:
        ready.send(f"ERROR: {error}")


def interrupt_installation(root, pin, ready):
    def interrupted(stage, _root, _pin, *, offline):
        (stage / "incomplete").write_text("not yet published")
        ready.send(str(stage))
        multiprocessing.Event().wait()

    cef.install_runtime = interrupted
    with cef.installation(Path(root), pin, offline=True, candidates=[]):
        pass


@unittest.skipUnless(sys.platform == "linux", "CEF DMA-BUF cache is Linux-specific")
class CefCacheTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.cache = self.base / "shared"
        self.runtime = self.base / "existing"
        self.runtime.mkdir()
        self.pin = {
            "version": "154.0.34+test",
            "platform": "linux64",
            "archive": "cef_binary_154.0.34+test_linux64_minimal.tar.bz2",
            "sha1": "test archive receipt",
            "required": ["libcef.so", "include/cef_version.h"],
        }
        (self.runtime / "libcef.so").write_bytes(b"fixture library, not executable")
        (self.runtime / "include").mkdir()
        (self.runtime / "include/cef_version.h").write_text(
            '#define CEF_VERSION "154.0.34+test"\n'
        )
        for name in ("CMakeLists.txt", "CREDITS.html"):
            (self.runtime / name).write_text("fixture")
        for name in ("cmake", "libcef_dll"):
            (self.runtime / name).mkdir()
            (self.runtime / name / "source").write_text("fixture")
        self.archive_fixture()
        (self.runtime / "archive.json").write_text(
            json.dumps({"name": self.pin["archive"], "sha1": self.pin["sha1"]})
        )

    def test_adoption_warm_offline_and_profile_cleanup_leave_shared_bytes(self):
        original = cef.file_stamp(self.runtime / "libcef.so")
        with cef.installation(
            self.cache, self.pin, offline=True, candidates=[self.runtime]
        ) as runtime:
            self.assertEqual(runtime, self.runtime)
            profile = self.base / "worktree-one/target/run/profile"
            profile.mkdir(parents=True)
            (profile / "cookies").write_text("private")
            shutil.rmtree(profile.parent.parent.parent)
        with patch.object(
            cef, "digest", side_effect=AssertionError("warm path rehashed data")
        ):
            with patch.object(
                cef, "fetch", side_effect=AssertionError("warm path used network")
            ):
                with cef.installation(
                    self.cache, self.pin, offline=True, candidates=[]
                ) as runtime:
                    self.assertEqual(cef.file_stamp(runtime / "libcef.so"), original)
        cef.prune(self.cache, self.pin)
        self.assertEqual(cef.file_stamp(self.runtime / "libcef.so"), original)

    def archive_fixture(self):
        cached = self.cache / "archives" / self.pin["archive"]
        if cached.exists():
            return cached
        source = self.base / "bundle" / self.pin["archive"].removesuffix(".tar.bz2")
        shutil.copytree(self.runtime, source / "Release")
        (source / "Release/archive.json").unlink(missing_ok=True)
        for name in (
            "include",
            "cmake",
            "libcef_dll",
            "CMakeLists.txt",
            "CREDITS.html",
        ):
            shutil.move(source / "Release" / name, source / name)
        (source / "Resources").mkdir()
        store = self.cache / "archives"
        store.mkdir(parents=True)
        archive = store / self.pin["archive"]
        with tarfile.open(archive, "w:bz2") as bundle:
            bundle.add(source, arcname=source.name)
        self.pin["sha1"] = cef.digest(archive, "sha1")
        return archive

    def test_concurrent_first_setup_and_live_prune_protection(self):
        archive = self.archive_fixture()
        context = multiprocessing.get_context("fork")
        children = []
        leases = []
        start = context.Event()
        try:
            for _ in range(2):
                ready_parent, ready_child = context.Pipe()
                release_parent, release_child = context.Pipe()
                process = context.Process(
                    target=hold_installation,
                    args=(str(self.cache), self.pin, ready_child, release_child, start),
                )
                process.start()
                children.append(process)
                leases.append((ready_parent, release_parent))
            start.set()
            paths = []
            for ready, _ in leases:
                self.assertTrue(
                    ready.poll(10), "second consumer blocked behind live shared lease"
                )
                path = ready.recv()
                self.assertFalse(path.startswith("ERROR"), path)
                paths.append(path)
            self.assertEqual(paths[0], paths[1])
            self.assertTrue(archive.is_file())
            with self.assertRaises(BlockingIOError):
                cef.prune(self.cache, self.pin)
        finally:
            for _, release in leases:
                release.send("done")
            for process in children:
                process.join(10)
                if process.is_alive():
                    process.terminate()
                    process.join()
        cef.prune(self.cache, self.pin)
        self.assertTrue(
            archive.is_file(), "explicit runtime prune must retain repair archive"
        )
        with cef.installation(
            self.cache, self.pin, offline=True, candidates=[]
        ) as repaired:
            self.assertTrue((repaired / "libcef.so").is_file())

    def test_interrupted_setup_reclaims_only_unpublished_identity(self):
        self.archive_fixture()
        directory = self.cache / "installations"
        context = multiprocessing.get_context("fork")
        ready_parent, ready_child = context.Pipe()
        process = context.Process(
            target=interrupt_installation, args=(str(self.cache), self.pin, ready_child)
        )
        process.start()
        try:
            self.assertTrue(ready_parent.poll(10))
            stale = Path(ready_parent.recv())
        finally:
            process.kill()
            process.join(10)
        self.assertTrue(stale.exists())
        unrelated = directory / ".other-version-in-progress"
        unrelated.mkdir()
        with cef.installation(
            self.cache, self.pin, offline=True, candidates=[]
        ) as runtime:
            self.assertTrue((runtime / "libcef.so").is_file())
            self.assertFalse(stale.exists())
            self.assertTrue(unrelated.exists())

    def test_modified_installation_and_wrong_version_fail_without_replacement(self):
        with cef.installation(
            self.cache, self.pin, offline=True, candidates=[self.runtime]
        ):
            pass
        (self.runtime / "libcef.so").write_bytes(b"changed")
        with self.assertRaisesRegex(RuntimeError, "Immutable CEF cache file changed"):
            with cef.installation(self.cache, self.pin, offline=True, candidates=[]):
                self.fail("changed installation accepted")
        self.assertEqual((self.runtime / "libcef.so").read_bytes(), b"changed")
        other = {**self.pin, "version": "155.0.0+other"}
        with self.assertRaisesRegex(RuntimeError, "header version mismatch"):
            with cef.installation(
                self.cache, other, offline=True, candidates=[self.runtime]
            ):
                self.fail("wrong version accepted")

    def test_cold_adoption_rejects_damaged_bytes_with_correct_metadata(self):
        (self.runtime / "libcef.so").write_bytes(b"damaged but nonempty")
        with self.assertRaisesRegex(RuntimeError, "differs from verified archive"):
            with cef.installation(
                self.cache, self.pin, offline=True, candidates=[self.runtime]
            ):
                self.fail("corrupted adoption accepted")
        self.assertFalse(
            (self.cache / "installations" / cef.identity(self.pin)).exists()
        )

    def test_cold_adoption_rejects_missing_file_outside_minimum_required_list(self):
        (self.runtime / "CREDITS.html").unlink()
        with self.assertRaisesRegex(RuntimeError, "missing from installation"):
            with cef.installation(
                self.cache, self.pin, offline=True, candidates=[self.runtime]
            ):
                self.fail("incomplete adoption accepted")


@unittest.skipUnless(sys.platform == "linux", "CEF process groups use Linux procfs")
class CefRunnerTests(unittest.TestCase):
    def test_failed_launch_retains_primary_error_and_bad_capture_diagnostic(self):
        path = cef.SOURCE / "run.py"
        spec = importlib.util.spec_from_file_location("cef_runner", path)
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory) / "evidence"
            args = type("Arguments", (), {"evidence": evidence, "offline": True})()
            original = Path.mkdir

            def create_with_truncated_capture(path, *args, **kwargs):
                original(path, *args, **kwargs)
                if path == evidence:
                    (path / "partial.rgba").write_bytes(b"truncated")

            output = io.StringIO()
            with patch.object(runner, "BINARY", Path(directory) / "absent"):
                with patch.object(Path, "mkdir", create_with_truncated_capture):
                    with redirect_stdout(output):
                        with self.assertRaisesRegex(RuntimeError, "Build first"):
                            runner.exercise(args)
            result = json.loads((evidence / "runner-result.json").read_text())
            self.assertFalse(result["passed"])
            self.assertIn("Build first", result["error"])
            self.assertIn("Bad capture size", result["cleanup_errors"][0])
            self.assertEqual(output.getvalue().count(f"Evidence: {evidence}"), 2)

    def test_browser_timeout_kills_owned_process_and_keeps_logs(self):
        spec = importlib.util.spec_from_file_location(
            "cef_runner_timeout", cef.SOURCE / "run.py"
        )
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            evidence = base / "evidence"
            executable = base / "timeout-fixture"
            executable.write_text("""#!/bin/sh
printf truncated > "$IPP_CEF_EVIDENCE/partial.rgba"
echo $$ > "$IPP_CEF_EVIDENCE/pid"
echo child-started
exec sleep 30
""")
            executable.chmod(0o700)
            args = type(
                "Arguments",
                (),
                {
                    "evidence": evidence,
                    "offline": True,
                    "headless": True,
                    "egl_dir": "/unused",
                },
            )()

            @contextmanager
            def fixture_lease(**_kwargs):
                yield base

            with (
                patch.object(runner, "BINARY", executable),
                patch.object(runner, "BROWSER_TIMEOUT", 0.1),
                patch.object(cef, "installation", fixture_lease),
            ):
                with redirect_stdout(io.StringIO()):
                    with self.assertRaisesRegex(RuntimeError, "TimeoutExpired"):
                        runner.exercise(args)
            result = json.loads((evidence / "runner-result.json").read_text())
            self.assertIn("TimeoutExpired", result["error"])
            self.assertIn("Bad capture size", result["cleanup_errors"][0])
            self.assertIn("child-started", (evidence / "run.log").read_text())
            with self.assertRaises(ProcessLookupError):
                os.kill(int((evidence / "pid").read_text()), 0)

    def test_cleanup_kills_term_ignoring_descendant_after_leader_exit(self):
        spec = importlib.util.spec_from_file_location(
            "cef_runner_group", cef.SOURCE / "run.py"
        )
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        with tempfile.TemporaryDirectory() as directory:
            ready = Path(directory) / "child-ready"
            child = (
                "import signal,time,pathlib; signal.signal(signal.SIGTERM,signal.SIG_IGN); pathlib.Path("
                + repr(str(ready))
                + ").write_text('ready'); time.sleep(30)"
            )
            leader = (
                "import subprocess,sys; subprocess.Popen([sys.executable,'-c',"
                + repr(child)
                + "])"
            )
            process = subprocess.Popen(
                [sys.executable, "-c", leader], start_new_session=True
            )
            try:
                process.wait(timeout=5)
                deadline = time.monotonic() + 5
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(ready.exists())
                self.assertTrue(runner.group_alive(process.pid))
                runner.stop(process, grace=0.1)
                self.assertFalse(runner.group_alive(process.pid))
            finally:
                runner.stop(process, grace=0.1)


if __name__ == "__main__":
    unittest.main()

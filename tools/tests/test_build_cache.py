"""Build reuse is verified against real subprocess effects, never test outcomes."""

from contextlib import redirect_stderr
from dataclasses import replace
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.build_cache import BuildCache, identity
from pipeline.model import Plan, Task
from pipeline.runner import run_plan


class BuildReuseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ipp-reuse-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        (self.root / ".gitignore").write_text("target/\n")
        (self.root / "source").write_text("one")
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "Fixture",
            ],
            cwd=self.root,
            check=True,
        )
        self.build = self.task("build:fixture", outputs=("target/product",))
        self.check = self.task("test:fixture", dependencies=(self.build.id,))

    def task(self, name, *, outputs=(), dependencies=(), extra=""):
        counter = f"target/{name.replace(':', '-')}-runs"
        command = (
            "from pathlib import Path; "
            f"p=Path({counter!r}); p.parent.mkdir(parents=True, exist_ok=True); "
            "p.write_text(str(int(p.read_text())+1) if p.exists() else '1'); "
            "assert not Path('target/fail').exists(); "
            + (
                "Path('target/product').write_text(Path('source').read_text()); "
                if outputs
                else ""
            )
            + extra
        )
        return Task(
            name, name, (sys.executable, "-c", command), dependencies, outputs=outputs
        )

    def run_tasks(self, *tasks, **options):
        tasks = tasks or (self.build, self.check)
        with redirect_stderr(io.StringIO()):
            return run_plan(
                Plan("test", tuple(t.id for t in tasks), tuple(tasks)),
                root=self.root,
                preflight=False,
                **options,
            )

    def assert_reused(self, report, expected=True):
        self.assertEqual(report["status"], "passed", report)
        self.assertEqual(bool(report["steps"][0].get("reusedFrom")), expected)

    def test_warm_build_reuses_but_test_executes_and_provenance_is_retained(self):
        first = self.run_tasks()
        second = self.run_tasks()
        self.assert_reused(first, False)
        self.assert_reused(second)
        self.assertEqual((self.root / "target/build-fixture-runs").read_text(), "1")
        self.assertEqual((self.root / "target/test-fixture-runs").read_text(), "2")
        step = second["steps"][0]
        self.assertEqual(step["reusedFrom"], first["steps"][0]["manifest"])
        self.assertNotEqual(step["manifest"], step["reusedFrom"])
        self.assertIn("Reused verified", Path(step["log"]).read_text())

    def test_changed_added_deleted_and_mode_changed_source_rebuild(self):
        self.run_tasks()
        for operation in (
            lambda: (self.root / "source").write_text("two"),
            lambda: (self.root / "added").touch(),
            lambda: (self.root / "added").unlink(),
            lambda: (self.root / "source").chmod(0o700),
            lambda: (self.root / "source").unlink(),
        ):
            operation()
            result = self.run_tasks()
            self.assertFalse(result["steps"][0].get("reusedFrom"))
        self.assertEqual(result["status"], "failed")

    def test_changed_missing_outputs_and_corrupt_evidence_rebuild(self):
        self.run_tasks()
        record = (
            self.root / "target/pipeline/products" / f"{identity(self.build.id)}.json"
        )
        for operation in (
            lambda: (self.root / "target/product").write_text("wrong"),
            lambda: (self.root / "target/product").chmod(0o700),
            lambda: (self.root / "target/product").unlink(),
            lambda: Path(json.loads(record.read_text())["manifest"]).write_text("{}"),
            lambda: record.write_text("not json"),
        ):
            operation()
            self.assert_reused(self.run_tasks(), False)
            self.assert_reused(self.run_tasks())

    def test_environment_command_and_external_configuration_invalidate(self):
        self.run_tasks()
        with patch.dict(os.environ, {"RUSTFLAGS": "changed"}):
            self.assert_reused(self.run_tasks(), False)
            self.assert_reused(self.run_tasks())
        changed = replace(
            self.build,
            command=(
                *self.build.command[:-1],
                self.build.command[-1] + "print('changed')",
            ),
        )
        self.assert_reused(self.run_tasks(changed), False)
        # External config lives outside the Git source tree.
        with tempfile.TemporaryDirectory() as cargo:
            with patch.dict(os.environ, {"CARGO_HOME": cargo}):
                self.run_tasks()
                Path(cargo, "config.toml").write_text(
                    '[build]\nrustflags=["--cfg=fixture"]\n'
                )
                self.assert_reused(self.run_tasks(), False)

    def test_scoped_sources_and_dependency_artifacts(self):
        (self.root / "runtime").mkdir()
        (self.root / "runtime/input").write_text("runtime")
        build = replace(self.build, source_roots=("runtime/",))
        dependent = Task(
            "build:dependent",
            "consumer",
            (
                sys.executable,
                "-c",
                "from pathlib import Path; Path('target/consumer').write_text(Path('target/product').read_text())",
            ),
            (build.id,),
            outputs=("target/consumer",),
            source_roots=("consumer/",),
        )
        self.run_tasks(build, dependent)
        (self.root / "ui").mkdir()
        (self.root / "ui/panel").write_text("ui edit")
        report = self.run_tasks(build, dependent)
        self.assertTrue(all(step.get("reusedFrom") for step in report["steps"]))
        # A dependency can change without changing the consumer's selected source.
        (self.root / "runtime/input").write_text("new")
        build = replace(
            build,
            command=(
                *build.command[:-1],
                build.command[-1]
                + "Path('target/product').write_text('new dependency')",
            ),
        )
        report = self.run_tasks(build, dependent)
        self.assertTrue(all(not step.get("reusedFrom") for step in report["steps"]))
        self.assertEqual((self.root / "target/consumer").read_text(), "new dependency")

    def test_failed_forced_rebuild_invalidates_previous_product(self):
        self.run_tasks()
        (self.root / "target/fail").touch()
        self.assertEqual(self.run_tasks(rebuild=True)["status"], "failed")
        (self.root / "target/fail").unlink()
        self.assert_reused(self.run_tasks(), False)
        self.assert_reused(self.run_tasks())
        self.assert_reused(self.run_tasks(rebuild=True), False)

    def test_source_mutation_and_modified_product_are_not_published(self):
        mutator = self.task(
            "test:mutator",
            dependencies=(self.build.id,),
            extra="Path('source').write_text('two')",
        )
        report = self.run_tasks(self.build, mutator)
        self.assertTrue(report["sourceChangedDuringRun"])
        self.assert_reused(self.run_tasks(), False)
        mutator = replace(
            mutator,
            command=(
                *mutator.command[:-1],
                mutator.command[-1] + "; Path('target/product').unlink()",
            ),
        )
        self.run_tasks(self.build, mutator, rebuild=True)
        self.assert_reused(self.run_tasks(), False)

    def test_non_build_outputs_are_never_reused(self):
        check = replace(self.build, id="check:fixture")
        self.run_tasks(check)
        self.assert_reused(self.run_tasks(check), False)

    def test_revision_stamp_and_external_symlink_changes_rebuild(self):
        self.run_tasks()
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "New artifact revision",
            ],
            cwd=self.root,
            check=True,
        )
        self.assert_reused(self.run_tasks(), False)
        with tempfile.TemporaryDirectory() as external:
            path = Path(external, "source")
            path.write_text("external input")
            (self.root / "linked").symlink_to(path)
            self.assert_reused(self.run_tasks(), False)
            self.assert_reused(self.run_tasks())
            path.write_text("changed external input")
            self.assert_reused(self.run_tasks(), False)

    def test_tool_identity_changes_invalidate(self):
        report = {
            "platform": {},
            "source": {"revision": "fixture"},
            "environment": [{"name": "node", "ready": True, "detail": "22"}],
        }
        task = replace(self.build, requirements=("node",))
        one = BuildCache(self.root, report, {}, enabled=True).key(task, {})
        report["environment"][0]["detail"] = "23"
        two = BuildCache(self.root, report, {}, enabled=True).key(task, {})
        self.assertNotEqual(one, two)
